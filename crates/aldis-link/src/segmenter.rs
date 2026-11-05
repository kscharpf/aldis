//! The `MapSegmenter`: divides SDUs into FDUs and TFDFs (LNK-040…LNK-043, LNK-002, LNK-003,
//! LNK-012, LNK-074, LNK-075, LNK-078; docs/04-engines.md §2.3, §4).
//!
//! It serves a TC Virtual Channel or a variable-length USLP Virtual Channel, and divides packets,
//! `MAP_SDUs`, and octet streams into the Frame Data Units or TFDFs of its MAPs.
//!
//! The engine serves one VC of one protocol (LNK-001). It takes packets (`SubmitPacket`), SDUs
//! (`SubmitSdu`), and, on USLP, octets of a stream (`SubmitOctets`) for a MAP, queues them whole
//! (LNK-040, API-052), and hands out one FDU or TFDF for each `NextFdu` request, which names the
//! request (`handle`) and the quality of service (LNK-043). It holds no output queue (API-059):
//! the caller feeds the FDU to FOP-1 (`RequestAd`, `RequestBd`) or to the multiplexer.
//!
//! **Order (LNK-041).** The segmenter serves the queue in the order of submission. It completes
//! the SDU in progress, whatever the MAP of the next one, before it starts another: the Blue Book
//! lets a TC sender interleave MAPs (232.0 §4.3.3.1–3), and the engine does not use that freedom.
//! A `NextFdu` whose `QoS` differs from that of the SDU in progress fails with `InvalidField`
//! (`qos`) and changes nothing.
//!
//! **Blocking (LNK-040, LNK-042).** Where a MAP permits blocking, the packets that follow each
//! other in the queue, of that MAP, share one FDU or TFDF as long as each fits whole; a packet
//! that needs segmentation is never blocked, and an SDU is never blocked.
//!
//! **Octet streams (LNK-042, LNK-012).** The octets of a stream MAP fill a TFDZ in order (rule
//! `011`) and join one queue entry for each stream MAP. A TFDZ is *started* when its first octet is
//! submitted, *releasable* when it is full or when the MAP's release deadline has passed since it
//! started and `Timeout` has said so, and `NextFdu` hands out a releasable one; one that is not
//! releasable waits, and does not hold the packets and SDUs behind it. The octets that did not fit
//! start the next TFDZ when the full one is handed out. A stream TFDZ never interrupts the SDU in
//! progress (732.1 §4.1.4.2.2.1.6).
//!
//! **Bounds (LNK-074, LNK-075, API-062).** Every input returns at most one action
//! ([`MAP_SEGMENTER_MAX_ACTIONS`]): `NextFdu` one `Fdu`, `Reconfigure` one `Discarded`, the other
//! inputs none. `NextFdu` writes at most `max_fdu_len` octets (`L`, the FDU or TFDF with its
//! header) to the output area and fails with `BufferTooSmall` when the area is shorter, whether
//! or not an FDU is ready; the other inputs write nothing.

use core::fmt::Debug;

use aldis_core::engine::{Actions, OutRange};
use aldis_core::time::{Duration, Instant};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy, Qos};
use crate::packet::{self, Length};
use crate::protocol::Protocol;

/// The most actions one input of the `MapSegmenter` returns: `NextFdu` returns at most one `Fdu`,
/// `Reconfigure` at most one `Discarded`, the other inputs none (LNK-074, docs/04-engines.md §4).
pub const MAP_SEGMENTER_MAX_ACTIONS: usize = 1;

/// The handle of a `NextFdu` request, echoed in the FDU it produces (LNK-043, API-054); the type
/// of the handle of FOP-1 requests.
pub type Handle = u32;

/// The octets in front of a queued entry: its kind, its MAP, and its length (16 bits).
const PREFIX: usize = 4;

/// The longest entry (SDU, packet, or run of stream octets) that its 16-bit length can count.
const MAX_ENTRY: usize = 0xFFFF;

/// The raw MAP Identifier.
fn raw<M: Into<u8>>(id: M) -> u8 {
    id.into()
}

/// How an FDU or TFDF relates to the SDU it carries (232.0 Table 4-2; 732.1 Table 4-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Framing {
    /// A whole SDU, or several whole packets: flags `11` (TC), rule `111` (USLP).
    Whole,
    /// The first portion of an SDU: flags `01`, rule `100`.
    First,
    /// A continuing portion: flags `00`, rule `101`.
    Continuing,
    /// The last portion: flags `10`, rule `110`.
    Last,
    /// A TFDZ of an octet stream: rule `011` (USLP only).
    Stream,
}

/// What a MAP carries (232.0 §5.3 Table 5-3; 732.1 §5.4 Table 5-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapService {
    /// Packets and SDUs alike: a TC MAP (the Segment Header does not say which), or a TC VC
    /// without a Segment Header, whose data field content is packets or a `VCA_SDU`.
    Any,
    /// Packets (USLP).
    Packets,
    /// `MAPA_SDUs` (USLP).
    Sdu,
    /// An octet stream (USLP rule `011`), released when full or after the deadline: the longest
    /// time a started TFDZ waits (732.1 §5.4 Table 5-3 note 4; LNK-012).
    OctetStream {
        /// The release deadline of a started TFDZ.
        release_deadline: Duration,
    },
}

/// A frame protocol whose VCs the `MapSegmenter` serves (LNK-001): TC and USLP.
pub trait SegmenterProtocol: Protocol {
    /// The MAP Identifier of the protocol: 6 bits (TC Segment Header), 4 bits (USLP).
    type Map: Copy + Debug + Eq + TryFrom<u8, Error = Error> + Into<u8>;
    /// The parameters of the VC that the protocol has and the others lack.
    type Channel: Copy + Debug + Eq;
    /// The parameters of a MAP that the protocol has and the others lack.
    type MapParams: Copy + Debug + Eq;

    /// Whether the protocol has octet streams (USLP).
    const STREAMS: bool;

    /// The octets in front of the user data of an FDU or TFDF.
    fn header_len(channel: &Self::Channel) -> usize;

    /// The longest FDU or TFDF the protocol allows on the VC.
    fn max_fdu_len(channel: &Self::Channel) -> usize;

    /// The one MAP of a VC that has no MAP Identifier on the wire (a TC VC without a Segment
    /// Header), or `None`.
    fn implicit_map(channel: &Self::Channel) -> Option<MapEntry<Self>>;

    /// The checks of a MAP that depend on the protocol (LNK-078).
    ///
    /// # Errors
    /// `InvalidConfig` naming the parameter.
    fn check_map(channel: &Self::Channel, map: &MapEntry<Self>) -> Result<()>;

    /// Whether an FDU of `framing` (`blocked`: of several packets) can be sent with `qos`: the
    /// portions of an SDU and the packets of a block need a VCF Count of the `QoS` (LNK-078).
    fn allows(channel: &Self::Channel, qos: Qos, framing: Framing, blocked: bool) -> bool;

    /// Writes the header of an FDU of `map` and `framing` into `header`, `header_len` octets.
    fn write_header(
        channel: &Self::Channel,
        map: &MapEntry<Self>,
        framing: Framing,
        header: &mut [u8],
    );
}

/// The configuration of one MAP (232.0 §5.4 Table 5-4; 732.1 §5.5 Table 5-4; LNK-040,
/// LNK-042, LNK-078).
///
/// Built by [`new`](Self::new) and the `with_` methods that check the parameter they set
/// (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEntry<P: SegmenterProtocol> {
    id: P::Map,
    service: MapService,
    max_sdu_len: Option<usize>,
    blocking: bool,
    params: P::MapParams,
}

impl<P: SegmenterProtocol> MapEntry<P> {
    /// A MAP `id` of `service` that neither segments nor blocks: an SDU longer than one FDU is
    /// refused (LNK-040).
    #[must_use]
    pub const fn new(id: P::Map, service: MapService, params: P::MapParams) -> MapEntry<P> {
        MapEntry {
            id,
            service,
            max_sdu_len: None,
            blocking: false,
            params,
        }
    }

    /// The same MAP with segmentation permitted for SDUs of at most `max_sdu_len` octets, the
    /// maximum `MAP_SDU` length (232.0 Table 5-4; LNK-040, LNK-078).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_sdu_len`) for 0 and for more than 65 535 octets, the length a queued
    /// entry counts.
    pub const fn with_segmentation(self, max_sdu_len: usize) -> Result<MapEntry<P>> {
        if max_sdu_len == 0 || max_sdu_len > MAX_ENTRY {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_SDU_LEN))
        } else {
            Ok(MapEntry {
                max_sdu_len: Some(max_sdu_len),
                ..self
            })
        }
    }

    /// The same MAP with blocking of packets permitted (LNK-040, LNK-042).
    #[must_use]
    pub const fn with_blocking(self) -> MapEntry<P> {
        MapEntry {
            blocking: true,
            ..self
        }
    }

    /// The MAP Identifier.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn id(&self) -> P::Map {
        self.id
    }

    /// What the MAP carries.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn service(&self) -> MapService {
        self.service
    }

    /// The maximum `MAP_SDU` length if the MAP permits segmentation, else `None`.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn segmentation(&self) -> Option<usize> {
        self.max_sdu_len
    }

    /// Whether blocking of packets is permitted.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn blocking(&self) -> bool {
        self.blocking
    }

    /// The parameters of the MAP that only this protocol has.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn params(&self) -> &P::MapParams {
        &self.params
    }
}

/// The configuration of a `MapSegmenter` for up to `M` MAPs (LNK-040…LNK-042, LNK-078).
///
/// Built only by [`new`](Self::new), which checks the relations between parameters (API-040,
/// RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSegmenterConfig<P: SegmenterProtocol, const M: usize> {
    vc: P::Vc,
    channel: P::Channel,
    max_fdu_len: usize,
    maps: [Option<MapEntry<P>>; M],
}

impl<P: SegmenterProtocol, const M: usize> MapSegmenterConfig<P, M> {
    /// The configuration of VC `vc` whose FDUs (TC) or TFDFs (USLP) are at most `max_fdu_len`
    /// octets, header included, with the MAPs `maps`.
    ///
    /// A VC that has no MAP Identifier on the wire (a TC VC without a Segment Header) takes no
    /// MAPs and has one implicit MAP (MAP 0).
    ///
    /// # Errors
    /// - `InvalidConfig` (`max_fdu_len`) unless the FDU holds the header and one octet and is no
    ///   longer than the protocol allows (TC: 1 019 octets, 1 017 with a FECF, 232.0 §4.1.3.1.2).
    /// - `InvalidConfig` (`map_id`) for no MAP, a MAP given to a VC that has none, and a MAP
    ///   given twice; `CapacityExceeded` (`map_id`) for more than `M` MAPs.
    /// - `InvalidConfig` (`data_field_content`) for an octet stream on a protocol that has none,
    ///   (`blocking`) or (`segmentation`) for a stream MAP that blocks or segments.
    /// - those of the protocol (LNK-078): `InvalidConfig` (`vcf_count_len`, `upid`).
    pub fn new(
        vc: P::Vc,
        channel: P::Channel,
        max_fdu_len: usize,
        maps: &[MapEntry<P>],
    ) -> Result<MapSegmenterConfig<P, M>> {
        let header = P::header_len(&channel);
        if max_fdu_len <= header || max_fdu_len > P::max_fdu_len(&channel) {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_FDU_LEN));
        }
        let bad_map = Error::new(ErrorKind::InvalidConfig).with_field(field::MAP_ID);
        let mut table = [None; M];
        let implicit = P::implicit_map(&channel);
        let given = match &implicit {
            Some(entry) if maps.is_empty() => core::slice::from_ref(entry),
            Some(_) => return Err(bad_map),
            None => maps,
        };
        if given.is_empty() {
            return Err(bad_map);
        }
        if given.len() > M {
            return Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID));
        }
        for (i, (slot, map)) in table.iter_mut().zip(given).enumerate() {
            let duplicate = given
                .iter()
                .take(i)
                .any(|earlier| raw(earlier.id) == raw(map.id));
            if duplicate {
                return Err(bad_map);
            }
            MapSegmenterConfig::<P, M>::check_map(&channel, map)?;
            *slot = Some(*map);
        }
        Ok(MapSegmenterConfig {
            vc,
            channel,
            max_fdu_len,
            maps: table,
        })
    }

    fn check_map(channel: &P::Channel, map: &MapEntry<P>) -> Result<()> {
        if let MapService::OctetStream { .. } = map.service {
            if !P::STREAMS {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::DATA_FIELD_CONTENT)
                );
            }
            if map.blocking {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::BLOCKING));
            }
            if map.max_sdu_len.is_some() {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SEGMENTATION));
            }
        }
        P::check_map(channel, map)
    }

    /// The VC the segmenter serves.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
    }

    /// The parameters of the VC that only this protocol has.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn channel(&self) -> &P::Channel {
        &self.channel
    }

    /// The longest FDU or TFDF, header included.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn max_fdu_len(&self) -> usize {
        self.max_fdu_len
    }

    /// The longest user data of one FDU or TFDF: `max_fdu_len` without the header.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub fn zone_len(&self) -> usize {
        self.max_fdu_len
            .saturating_sub(P::header_len(&self.channel))
    }

    /// The MAP `id`, or `None` if it is not configured.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub fn map(&self, id: u8) -> Option<&MapEntry<P>> {
        self.maps.iter().flatten().find(|m| raw(m.id) == id)
    }

    /// The position of the MAP `id` in the table, which indexes the state kept for each MAP.
    fn position(&self, id: u8) -> Option<usize> {
        self.maps
            .iter()
            .position(|m| m.as_ref().is_some_and(|m| raw(m.id) == id))
    }

    /// The MAPs, in the order of configuration.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    pub fn maps(&self) -> impl Iterator<Item = &MapEntry<P>> {
        self.maps.iter().flatten()
    }
}

/// An input of the `MapSegmenter` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmenterInput<'a, P: SegmenterProtocol, const M: usize> {
    /// A packet, whole, for a MAP (LNK-040, LNK-042). On a TC VC without a Segment Header the
    /// MAP is the implicit MAP 0.
    SubmitPacket {
        /// The MAP.
        map: P::Map,
        /// The packet, with its length field agreeing with the octets.
        packet: &'a [u8],
    },
    /// An SDU, whole, for a MAP (a `MAP_SDU`, a `MAPA_SDU`, or the `VCA_SDU` of a TC VC without a
    /// Segment Header) (LNK-040, LNK-042).
    SubmitSdu {
        /// The MAP.
        map: P::Map,
        /// The SDU.
        sdu: &'a [u8],
    },
    /// Octets of the stream of a MAP (USLP rule `011`) (LNK-042).
    SubmitOctets {
        /// The MAP.
        map: P::Map,
        /// The octets.
        octets: &'a [u8],
    },
    /// The next FDU or TFDF, for a request of the caller (LNK-043).
    NextFdu {
        /// The handle of the request, echoed in the action.
        handle: Handle,
        /// The quality of service: sequence-controlled for FOP-1 `RequestAd`, expedited for
        /// `RequestBd` (232.1 §4.4).
        qos: Qos,
    },
    /// The release deadline of the started TFDZ of a stream MAP may have passed (API-053,
    /// LNK-012).
    Timeout,
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(MapSegmenterConfig<P, M>),
}

/// An FDU or TFDF the segmenter hands out (LNK-043, API-054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentedFdu {
    /// The handle of the `NextFdu` request.
    pub handle: Handle,
    /// The VCID of the segmenter.
    pub vc: u8,
    /// The MAP Identifier; `None` on a TC VC without a Segment Header.
    pub map: Option<u8>,
    /// The quality of service of the request.
    pub qos: Qos,
    /// How the FDU relates to its SDU: the Sequence Flags or the TFDZ Construction Rule.
    pub framing: Framing,
    /// Where the FDU is in the output area, header included (the Segment Header of TC, the
    /// TFDF Header of USLP).
    pub fdu: OutRange,
}

/// An action of the `MapSegmenter` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmenterOutput {
    /// An FDU or TFDF, in the output area (LNK-043).
    Fdu(SegmentedFdu),
    /// `Reconfigure` discarded this many octets: the unsent octets of the SDU in progress and of
    /// the queue (LNK-003).
    Discarded {
        /// The octets of SDUs, packets, and streams not yet handed out.
        octets: usize,
    },
}

/// The actions of one input of the `MapSegmenter` (LNK-074).
pub type SegmenterActions = Actions<SegmenterOutput, MAP_SEGMENTER_MAX_ACTIONS>;

/// The read-only status of a `MapSegmenter`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SegmenterStatus {
    /// Packets, SDUs, and runs of stream octets taken.
    pub submitted: Counter,
    /// FDUs and TFDFs handed out.
    pub fdus_out: Counter,
    /// Inputs refused as invalid (LNK-040, LNK-042).
    pub refused: Counter,
    /// Inputs refused with `WouldBlock`.
    pub would_block: Counter,
    /// Octets discarded by `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The unsent user octets in the queue (API-060).
    pub queue: Occupancy,
}

/// What a queued entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Packet,
    Sdu,
    Octets,
}

impl Kind {
    const fn tag(self) -> u8 {
        match self {
            Kind::Packet => 1,
            Kind::Sdu => 2,
            Kind::Octets => 3,
        }
    }

    const fn from_tag(tag: u8) -> Option<Kind> {
        match tag {
            1 => Some(Kind::Packet),
            2 => Some(Kind::Sdu),
            3 => Some(Kind::Octets),
            _ => None,
        }
    }
}

/// A queued entry: where it starts, what it is, and its user octets.
#[derive(Debug, Clone, Copy)]
struct Entry {
    at: usize,
    kind: Kind,
    map: u8,
    len: usize,
}

impl Entry {
    /// Where the entry ends: the start of the next one.
    const fn end(&self) -> usize {
        self.at.saturating_add(PREFIX).saturating_add(self.len)
    }

    /// Where the user octets start.
    const fn payload(&self) -> usize {
        self.at.saturating_add(PREFIX)
    }
}

/// The SDU in progress (RS-021: the SDU length passed in decides the next state, so a
/// closed enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    /// No SDU is in progress: the next `NextFdu` starts the first eligible entry.
    Idle,
    /// The first entry that is not a stream has been started with `qos`, and `sent` of its octets
    /// are out.
    Sending { sent: usize, qos: Qos },
}

/// The TFDZ of an octet stream that is being assembled (LNK-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamTimer {
    /// When the first octet of the TFDZ was submitted.
    started: Option<Instant>,
    /// `Timeout` found the release deadline passed: the TFDZ is releasable.
    due: bool,
}

impl StreamTimer {
    const IDLE: StreamTimer = StreamTimer {
        started: None,
        due: false,
    };
}

/// The `MapSegmenter` of one VC (LNK-040…LNK-043).
///
/// `M` is the number of MAPs of its configuration and `Q` the capacity of the queue in octets,
/// four octets of kind, MAP, and length included for each entry (GEN-011, LNK-015); a
/// configuration whose longest SDU does not fit is refused with `CapacityExceeded`.
///
/// The engine is its [`MapSegmenterHead`] followed by its [`MapSegmenterBuffers`] (`repr(C)`), so
/// that a caller can build it in place (see [`crate::inplace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct MapSegmenter<P: SegmenterProtocol, const M: usize, const Q: usize> {
    head: MapSegmenterHead<P, M>,
    bufs: MapSegmenterBuffers<Q>,
}

/// Everything of a [`MapSegmenter`] but its queue (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapSegmenterHead<P: SegmenterProtocol, const M: usize> {
    config: MapSegmenterConfig<P, M>,
    queue_len: usize,
    progress: Progress,
    octets: usize,
    streams: [StreamTimer; M],
    status: SegmenterStatus,
}

/// The queue of a [`MapSegmenter`]: `Q` octets, valid when all zero, which is the empty state
/// (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct MapSegmenterBuffers<const Q: usize> {
    queue: [u8; Q],
}

impl<const Q: usize> MapSegmenterBuffers<Q> {
    /// An empty queue.
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    pub const ZERO: Self = Self { queue: [0; Q] };
}

impl<P: SegmenterProtocol, const M: usize, const Q: usize> MapSegmenter<P, M, Q> {
    /// A segmenter with an empty queue.
    ///
    /// # Errors
    /// `CapacityExceeded` (`max_sdu_len`, `max_fdu_len`) if the queue cannot hold the longest SDU
    /// of a MAP, or a TFDZ of an octet stream, with the four octets of its entry.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    pub fn new(config: MapSegmenterConfig<P, M>) -> Result<MapSegmenter<P, M, Q>> {
        Ok(MapSegmenter {
            head: Self::new_head(config)?,
            bufs: MapSegmenterBuffers::ZERO,
        })
    }

    /// The offset of the head in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the buffers in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

    /// The head of a segmenter with an empty queue: the part of [`Self::new`] that is built from
    /// the configuration (see [`crate::inplace`]).
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    pub fn new_head(config: MapSegmenterConfig<P, M>) -> Result<MapSegmenterHead<P, M>> {
        MapSegmenter::<P, M, Q>::check_capacity(&config)?;
        Ok(MapSegmenterHead {
            config,
            queue_len: 0,
            progress: Progress::Idle,
            octets: 0,
            streams: [StreamTimer::IDLE; M],
            status: SegmenterStatus::default(),
        })
    }

    fn check_capacity(config: &MapSegmenterConfig<P, M>) -> Result<()> {
        let zone = config.zone_len();
        for map in config.maps() {
            let (longest, f) = match map.service {
                MapService::OctetStream { .. } => (zone, field::MAX_FDU_LEN),
                MapService::Any | MapService::Packets | MapService::Sdu => match map.max_sdu_len {
                    Some(max) => (max, field::MAX_SDU_LEN),
                    // No segmentation: one FDU holds the longest SDU.
                    None => (zone, field::MAX_SDU_LEN),
                },
            };
            if longest.saturating_add(PREFIX) > Q {
                return Err(Error::new(ErrorKind::CapacityExceeded).with_field(f));
            }
        }
        Ok(())
    }

    /// The configuration in force.
    ///
    /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn config(&self) -> &MapSegmenterConfig<P, M> {
        &self.head.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &SegmenterStatus {
        &self.head.status
    }

    /// The earliest instant at which `Timeout` must be fed: the earliest release deadline of a
    /// started TFDZ of a stream MAP that is neither full nor releasable yet; none otherwise
    /// (API-053, LNK-012).
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        (0..M).filter_map(|i| self.deadline_of(i)).min()
    }

    /// The instant at which the TFDZ of the stream MAP at position `i` is due, if it is waiting.
    fn deadline_of(&self, i: usize) -> Option<Instant> {
        let map = self.head.config.maps.get(i)?.as_ref()?;
        let MapService::OctetStream { release_deadline } = map.service else {
            return None;
        };
        let timer = self.head.streams.get(i)?;
        let held = self.stream_entry(raw(map.id)).map_or(0, |e| e.len);
        if timer.due || held >= self.head.config.zone_len() {
            None
        } else {
            Some(timer.started?.saturating_add(release_deadline))
        }
    }

    /// Runs one input (API-057).
    ///
    /// An input that fails leaves the queue, the SDU in progress, and the configuration as they
    /// were and writes no action (API-058); the housekeeping counters of the status count a
    /// refusal.
    ///
    /// # Errors
    /// - `SubmitPacket`: `UnknownChannel` for a MAP that is not configured, `InvalidField`
    ///   (`data_field_content`) for a MAP of another service, (`packet_version`) for a PVN
    ///   other than 0 and 7, `LengthMismatch` (`packet_len`) if the length field disagrees with
    ///   the octets, `OutOfRange` (`max_sdu_len`) for a packet longer than the maximum `MAP_SDU`
    ///   length of a MAP that segments, (`max_fdu_len`) for one longer than the FDU of a MAP
    ///   that does not (LNK-040, LNK-042); `WouldBlock` if the queue cannot take it whole
    ///   (API-052).
    /// - `SubmitSdu`: as for a packet without the packet checks; `LengthMismatch` (`sdu_len`)
    ///   for an empty SDU.
    /// - `SubmitOctets`: `UnknownChannel`, `InvalidField` (`data_field_content`) for a MAP that
    ///   is not a stream, `LengthMismatch` (`sdu_len`) for no octet, `OutOfRange`
    ///   (`max_sdu_len`) above 65 535 octets, `WouldBlock`.
    /// - `NextFdu`: `BufferTooSmall` if `out` is shorter than `max_fdu_len` (LNK-075);
    ///   `InvalidField` (`qos`) if the `QoS` differs from that of the SDU in progress, or if the
    ///   FDU needs a VCF Count that the `QoS` does not have (LNK-041, LNK-078).
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(
        &mut self,
        input: SegmenterInput<'_, P, M>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<SegmenterActions> {
        let mut actions = SegmenterActions::new();
        match input {
            SegmenterInput::SubmitPacket { map, packet } => {
                self.submit(Kind::Packet, raw(map), packet, now)?;
            }
            SegmenterInput::SubmitSdu { map, sdu } => {
                self.submit(Kind::Sdu, raw(map), sdu, now)?;
            }
            SegmenterInput::SubmitOctets { map, octets } => {
                self.submit(Kind::Octets, raw(map), octets, now)?;
            }
            SegmenterInput::NextFdu { handle, qos } => {
                if let Some(fdu) = self.next_fdu(handle, qos, now, out)? {
                    actions.push(SegmenterOutput::Fdu(fdu))?;
                }
            }
            SegmenterInput::Timeout => {
                for i in 0..M {
                    if self.deadline_of(i).is_some_and(|due| now >= due)
                        && let Some(timer) = self.head.streams.get_mut(i)
                    {
                        timer.due = true;
                    }
                }
            }
            SegmenterInput::Reconfigure(config) => {
                MapSegmenter::<P, M, Q>::check_capacity(&config)?;
                let octets = self.head.octets;
                self.head.config = config;
                self.head.queue_len = 0;
                self.head.progress = Progress::Idle;
                self.head.octets = 0;
                self.head.streams = [StreamTimer::IDLE; M];
                self.head.status.discarded_octets.add(octets);
                self.head.status.queue.set(0);
                if octets > 0 {
                    actions.push(SegmenterOutput::Discarded { octets })?;
                }
            }
        }
        Ok(actions)
    }

    /// The entry that starts at `at`, or `None` past the end of the queue.
    fn entry_at(&self, at: usize) -> Option<Entry> {
        if at >= self.head.queue_len {
            return None;
        }
        let head = self.bufs.queue.get(at..at.checked_add(PREFIX)?)?;
        let [tag, map, hi, lo] = *head else {
            return None;
        };
        Some(Entry {
            at,
            kind: Kind::from_tag(tag)?,
            map,
            len: usize::from(u16::from_be_bytes([hi, lo])),
        })
    }

    /// The entry that holds the octets of the stream of `map`, if any: a stream MAP has at most
    /// one, which later octets are appended to.
    fn stream_entry(&self, map: u8) -> Option<Entry> {
        let mut at = 0;
        // bound: each pass moves `at` past one entry of the queue, which holds at most `Q` octets.
        while let Some(e) = self.entry_at(at) {
            if e.kind == Kind::Octets && e.map == map {
                return Some(e);
            }
            at = e.end();
        }
        None
    }

    /// The first entry that is a packet or an SDU: the one in progress, or the next to start.
    fn first_sdu(&self) -> Option<Entry> {
        let mut at = 0;
        // bound: each pass moves `at` past one entry of the queue, which holds at most `Q` octets.
        while let Some(e) = self.entry_at(at) {
            if e.kind != Kind::Octets {
                return Some(e);
            }
            at = e.end();
        }
        None
    }

    /// `SubmitPacket`, `SubmitSdu`, `SubmitOctets` (LNK-040, LNK-042).
    fn submit(&mut self, kind: Kind, map: u8, data: &[u8], now: Instant) -> Result<()> {
        if let Err(e) = self.check_submit(kind, map, data) {
            self.head.status.refused.incr();
            return Err(e);
        }
        let existing = if kind == Kind::Octets {
            self.stream_entry(map)
        } else {
            None
        };
        // The octets join the entry of their stream; a new entry costs its four octets too.
        let needed = match existing {
            Some(_) => data.len(),
            None => data.len().saturating_add(PREFIX),
        };
        let merged = existing.map_or(0, |e| e.len).saturating_add(data.len());
        if needed > Q.saturating_sub(self.head.queue_len) || merged > MAX_ENTRY {
            self.head.status.would_block.incr();
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        if let Some(e) = existing {
            self.insert_at(e.end(), data);
            self.set_len(e.at, merged)?;
        } else {
            let [hi, lo] = u16::try_from(data.len())
                .ok()
                .ok_or(Error::new(ErrorKind::InternalError))?
                .to_be_bytes();
            self.insert_at(self.head.queue_len, &[kind.tag(), map, hi, lo]);
            self.insert_at(self.head.queue_len, data);
            if kind == Kind::Octets {
                // The first octet of a TFDZ starts it (LNK-012).
                let index = self
                    .head
                    .config
                    .position(map)
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                if let Some(timer) = self.head.streams.get_mut(index) {
                    *timer = StreamTimer {
                        started: Some(now),
                        due: false,
                    };
                }
            }
        }
        self.head.octets = self.head.octets.saturating_add(data.len());
        self.head.status.submitted.incr();
        self.head.status.queue.set(self.head.octets);
        Ok(())
    }

    /// The checks of a submission that depend on the input only (LNK-040, LNK-042).
    fn check_submit(&self, kind: Kind, map: u8, data: &[u8]) -> Result<()> {
        let entry = self
            .head
            .config
            .map(map)
            .ok_or(Error::new(ErrorKind::UnknownChannel))?;
        let content = Error::new(ErrorKind::InvalidField).with_field(field::DATA_FIELD_CONTENT);
        let served = match (kind, entry.service) {
            (Kind::Packet, MapService::Any | MapService::Packets)
            | (Kind::Sdu, MapService::Any | MapService::Sdu)
            | (Kind::Octets, MapService::OctetStream { .. }) => true,
            (Kind::Packet | Kind::Sdu | Kind::Octets, _) => false,
        };
        if !served {
            return Err(content);
        }
        if data.is_empty() {
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::SDU_LEN));
        }
        let out_of_range = |f, limit: usize| {
            Error::out_of_range(
                f,
                u64::try_from(limit).unwrap_or(u64::MAX),
                u64::try_from(data.len()).unwrap_or(u64::MAX),
            )
        };
        if kind == Kind::Packet {
            let packet_kind = data
                .first()
                .and_then(|&b| packet::kind_of(b))
                .ok_or(Error::new(ErrorKind::InvalidField).with_field(field::PACKET_VERSION))?;
            match packet::length(packet_kind, data) {
                Length::Total(n) if n == data.len() => {}
                Length::Total(_) | Length::More | Length::Invalid => {
                    return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::PACKET_LEN));
                }
            }
        }
        if kind == Kind::Octets {
            return if data.len() > MAX_ENTRY {
                Err(out_of_range(field::MAX_SDU_LEN, MAX_ENTRY))
            } else {
                Ok(())
            };
        }
        match entry.max_sdu_len {
            Some(max) if data.len() > max => Err(out_of_range(field::MAX_SDU_LEN, max)),
            None if data.len() > self.head.config.zone_len() => Err(out_of_range(
                field::MAX_FDU_LEN,
                self.head.config.zone_len(),
            )),
            Some(_) | None => Ok(()),
        }
    }

    /// Whether the entry is the TFDZ of a stream that `NextFdu` may hand out: full, or due
    /// (LNK-012).
    fn stream_ready(&self, entry: &Entry) -> bool {
        entry.len >= self.head.config.zone_len()
            || self
                .head
                .config
                .position(entry.map)
                .and_then(|i| self.head.streams.get(i))
                .is_some_and(|t| t.due)
    }

    /// The entry `NextFdu` serves, in the order of the queue: the SDU in progress; else the
    /// first packet or SDU, or stream TFDZ that is ready. A stream TFDZ that is not ready does
    /// not hold the entries behind it (LNK-041, LNK-012).
    fn eligible(&self) -> Option<Entry> {
        if matches!(self.head.progress, Progress::Sending { .. }) {
            return self.first_sdu();
        }
        let mut at = 0;
        // bound: each pass moves `at` past one entry of the queue, which holds at most `Q` octets.
        while let Some(e) = self.entry_at(at) {
            if e.kind != Kind::Octets || self.stream_ready(&e) {
                return Some(e);
            }
            at = e.end();
        }
        None
    }

    /// `NextFdu` (LNK-040…LNK-043).
    fn next_fdu(
        &mut self,
        handle: Handle,
        qos: Qos,
        now: Instant,
        out: &mut [u8],
    ) -> Result<Option<SegmentedFdu>> {
        let l = self.head.config.max_fdu_len;
        let area = out.get_mut(..l).ok_or(Error::buffer_too_small(l))?;
        let Some(head) = self.eligible() else {
            return if self.head.queue_len == 0 || self.first_sdu().is_none() {
                Ok(None)
            } else {
                Err(Error::new(ErrorKind::InternalError))
            };
        };
        let map = *self
            .head
            .config
            .map(head.map)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let header_len = P::header_len(&self.head.config.channel);
        let (header, zone) = area
            .split_at_mut_checked(header_len)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let (framing, used) = if head.kind == Kind::Octets {
            (Framing::Stream, self.stream_fdu(&head, zone, now)?)
        } else {
            self.sdu_fdu(&head, &map, qos, zone)?
        };
        P::write_header(&self.head.config.channel, &map, framing, header);
        self.head.status.fdus_out.incr();
        self.head.status.queue.set(self.head.octets);
        Ok(Some(SegmentedFdu {
            handle,
            vc: raw(self.head.config.vc),
            map: if P::implicit_map(&self.head.config.channel).is_some() {
                None
            } else {
                Some(head.map)
            },
            qos,
            framing,
            fdu: OutRange::new(0, header_len.saturating_add(used)),
        }))
    }

    /// The octets and the count of the packets of the queue that block with `head` into one
    /// Whole FDU of at most `capacity` octets: `head` and the packets of its MAP behind it.
    fn blocked_run(&self, head: &Entry, capacity: usize) -> (usize, usize) {
        let (mut total, mut blocked) = (head.len, 1_usize);
        let mut at = head.end();
        // bound: each pass moves `at` past one entry of the queue.
        while let Some(next) = self.entry_at(at) {
            let fits = total.saturating_add(next.len) <= capacity;
            if next.kind != Kind::Packet || next.map != head.map || !fits {
                break;
            }
            total = total.saturating_add(next.len);
            blocked = blocked.saturating_add(1);
            at = next.end();
        }
        (total, blocked)
    }

    /// The FDU of the SDU or packets of `head`, written into `zone`; returns its framing and the
    /// octets written (LNK-040, LNK-041, LNK-042).
    fn sdu_fdu(
        &mut self,
        head: &Entry,
        map: &MapEntry<P>,
        qos: Qos,
        zone: &mut [u8],
    ) -> Result<(Framing, usize)> {
        let capacity = zone.len();
        let unsupported = || Error::new(ErrorKind::InvalidField).with_field(field::QOS);
        match self.head.progress {
            Progress::Idle if head.len <= capacity => {
                let (mut total, mut blocked) = (head.len, 1_usize);
                if head.kind == Kind::Packet && map.blocking {
                    (total, blocked) = self.blocked_run(head, capacity);
                }
                if !P::allows(&self.head.config.channel, qos, Framing::Whole, blocked > 1) {
                    return Err(unsupported());
                }
                // Write the entries, then drop them from the queue.
                let mut at = head.at;
                let mut written = 0;
                // bound: `blocked` entries are copied, one per pass.
                for _ in 0..blocked {
                    let e = self
                        .entry_at(at)
                        .ok_or(Error::new(ErrorKind::InternalError))?;
                    written = self.copy_payload(&e, 0, e.len, zone, written)?;
                    at = e.end();
                }
                self.remove_range(head.at, at.saturating_sub(head.at));
                self.head.octets = self.head.octets.saturating_sub(total);
                Ok((Framing::Whole, total))
            }
            Progress::Idle => {
                if !P::allows(&self.head.config.channel, qos, Framing::First, false) {
                    return Err(unsupported());
                }
                self.copy_payload(head, 0, capacity, zone, 0)?;
                self.head.progress = Progress::Sending {
                    sent: capacity,
                    qos,
                };
                self.head.octets = self.head.octets.saturating_sub(capacity);
                Ok((Framing::First, capacity))
            }
            Progress::Sending { sent, qos: begun } => {
                if qos != begun {
                    return Err(unsupported());
                }
                let remaining = head.len.saturating_sub(sent);
                if remaining <= capacity {
                    self.copy_payload(head, sent, remaining, zone, 0)?;
                    self.remove_range(head.at, PREFIX.saturating_add(head.len));
                    self.head.progress = Progress::Idle;
                    self.head.octets = self.head.octets.saturating_sub(remaining);
                    Ok((Framing::Last, remaining))
                } else {
                    self.copy_payload(head, sent, capacity, zone, 0)?;
                    self.head.progress = Progress::Sending {
                        sent: sent.saturating_add(capacity),
                        qos: begun,
                    };
                    self.head.octets = self.head.octets.saturating_sub(capacity);
                    Ok((Framing::Continuing, capacity))
                }
            }
        }
    }

    /// The TFDZ of the stream in `head`, written into `zone`: the octets that fit, the whole
    /// entry if it does; returns the octets written. A TFDZ started with the octets that remain
    /// starts at `now` (LNK-012, LNK-042).
    fn stream_fdu(&mut self, head: &Entry, zone: &mut [u8], now: Instant) -> Result<usize> {
        let take = head.len.min(zone.len());
        self.copy_payload(head, 0, take, zone, 0)?;
        let index = self
            .head
            .config
            .position(head.map)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let timer = if take == head.len {
            self.remove_range(head.at, PREFIX.saturating_add(head.len));
            StreamTimer::IDLE
        } else {
            self.remove_range(head.payload(), take);
            self.set_len(head.at, head.len.saturating_sub(take))?;
            StreamTimer {
                started: Some(now),
                due: false,
            }
        };
        if let Some(slot) = self.head.streams.get_mut(index) {
            *slot = timer;
        }
        self.head.octets = self.head.octets.saturating_sub(take);
        Ok(take)
    }

    /// Copies `n` octets of the entry from its octet `from` into `zone` at `to`; returns the
    /// offset after them.
    fn copy_payload(
        &self,
        entry: &Entry,
        from: usize,
        n: usize,
        zone: &mut [u8],
        to: usize,
    ) -> Result<usize> {
        let end = to.saturating_add(n);
        let start = entry.payload().saturating_add(from);
        let dst = zone
            .get_mut(to..end)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let src = self
            .bufs
            .queue
            .get(start..start.saturating_add(n))
            .ok_or(Error::new(ErrorKind::InternalError))?;
        for (d, s) in dst.iter_mut().zip(src) {
            *d = *s;
        }
        Ok(end)
    }

    /// Writes the length of the entry at `at`.
    fn set_len(&mut self, at: usize, len: usize) -> Result<()> {
        let [hi, lo] = u16::try_from(len)
            .ok()
            .ok_or(Error::new(ErrorKind::InternalError))?
            .to_be_bytes();
        let slots = self
            .bufs
            .queue
            .get_mut(at.saturating_add(2)..at.saturating_add(PREFIX))
            .ok_or(Error::new(ErrorKind::InternalError))?;
        for (d, s) in slots.iter_mut().zip([hi, lo]) {
            *d = s;
        }
        Ok(())
    }

    /// Removes `n` octets from `at` of the queue and closes the gap, octet by octet (a slice
    /// method that moves a range could panic).
    fn remove_range(&mut self, at: usize, n: usize) {
        let left = self.head.queue_len.saturating_sub(at.saturating_add(n));
        for i in 0..left {
            let from = at.saturating_add(n).saturating_add(i);
            let octet = self.bufs.queue.get(from).copied();
            if let (Some(octet), Some(dst)) = (octet, self.bufs.queue.get_mut(at.saturating_add(i)))
            {
                *dst = octet;
            }
        }
        self.head.queue_len = at.saturating_add(left).min(self.head.queue_len);
    }

    /// Inserts `data` at `at` of the queue, which has room for it, moving the octets after `at`
    /// up, last first, octet by octet.
    fn insert_at(&mut self, at: usize, data: &[u8]) {
        let n = data.len();
        let tail = self.head.queue_len.saturating_sub(at);
        for i in (0..tail).rev() {
            let octet = self.bufs.queue.get(at.saturating_add(i)).copied();
            if let (Some(octet), Some(dst)) = (
                octet,
                self.bufs
                    .queue
                    .get_mut(at.saturating_add(i).saturating_add(n)),
            ) {
                *dst = octet;
            }
        }
        for (d, s) in self.bufs.queue.iter_mut().skip(at).zip(data) {
            *d = *s;
        }
        self.head.queue_len = self.head.queue_len.saturating_add(n).min(Q);
    }
}

#[cfg(feature = "tc")]
mod tc_impl {
    use aldis_core::Result;
    use aldis_core::ids::MapId;
    use aldis_tc::{SegmentHeader, SequenceFlags, TcMapId};

    use super::{Framing, MapEntry, MapService, SegmenterProtocol};
    use crate::common::Qos;
    use crate::protocol::Tc;

    /// The longest Frame Data Field of a TC frame (232.0 §4.1.3.1.2).
    const MAX_DATA_FIELD: usize = 1019;
    /// The octets of the FECF (232.0 §4.1.4).
    const FECF_LEN: usize = 2;

    /// The parameters of a TC VC that the `MapSegmenter` needs (232.0 §5.1 Table 5-1, §5.3
    /// Table 5-3).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TcSegmenterParams {
        segment_header: bool,
        fecf: bool,
        blocking: bool,
    }

    impl TcSegmenterParams {
        /// A VC with a Segment Header on every FDU; blocking is a parameter of each MAP.
        /// `fecf`: the Physical Channel has a FECF, which takes two octets of the data field's
        /// limit (232.0 §4.1.3.1.2).
        #[must_use]
        pub const fn with_segment_header(fecf: bool) -> TcSegmenterParams {
            TcSegmenterParams {
                segment_header: true,
                fecf,
                blocking: false,
            }
        }

        /// A VC without a Segment Header: one implicit MAP 0, whose FDU is packets (blocked when
        /// `blocking`) or a `VCA_SDU`, never segmented (232.0 §4.1.3.2.1.3, §5.3 Table 5-3).
        #[must_use]
        pub const fn without_segment_header(fecf: bool, blocking: bool) -> TcSegmenterParams {
            TcSegmenterParams {
                segment_header: false,
                fecf,
                blocking,
            }
        }

        /// Whether every FDU has a Segment Header.
        ///
        /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
        #[must_use]
        pub const fn segment_header(&self) -> bool {
            self.segment_header
        }

        /// Whether the Physical Channel has a FECF.
        ///
        /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
        #[must_use]
        pub const fn fecf(&self) -> bool {
            self.fecf
        }
    }

    impl SegmenterProtocol for Tc {
        type Map = TcMapId;
        type Channel = TcSegmenterParams;
        type MapParams = ();
        const STREAMS: bool = false;

        fn header_len(channel: &TcSegmenterParams) -> usize {
            usize::from(channel.segment_header)
        }

        fn max_fdu_len(channel: &TcSegmenterParams) -> usize {
            MAX_DATA_FIELD.saturating_sub(if channel.fecf { FECF_LEN } else { 0 })
        }

        fn implicit_map(channel: &TcSegmenterParams) -> Option<MapEntry<Tc>> {
            if channel.segment_header {
                return None;
            }
            let entry = MapEntry::<Tc>::new(MapId::<6>::new(0)?, MapService::Any, ());
            Some(if channel.blocking {
                entry.with_blocking()
            } else {
                entry
            })
        }

        fn check_map(_: &TcSegmenterParams, _: &MapEntry<Tc>) -> Result<()> {
            Ok(())
        }

        fn allows(_: &TcSegmenterParams, _: Qos, _: Framing, _: bool) -> bool {
            true
        }

        fn write_header(
            channel: &TcSegmenterParams,
            map: &MapEntry<Tc>,
            framing: Framing,
            header: &mut [u8],
        ) {
            if !channel.segment_header {
                return;
            }
            let sequence_flags = match framing {
                Framing::First => SequenceFlags::First,
                Framing::Continuing | Framing::Stream => SequenceFlags::Continuing,
                Framing::Last => SequenceFlags::Last,
                Framing::Whole => SequenceFlags::Unsegmented,
            };
            let octet = SegmentHeader {
                sequence_flags,
                map_id: map.id(),
            }
            .octet();
            for (d, s) in header.iter_mut().zip([octet]) {
                *d = s;
            }
        }
    }
}

#[cfg(feature = "tc")]
pub use tc_impl::TcSegmenterParams;

#[cfg(feature = "uslp")]
mod uslp_impl {
    use aldis_core::ids::MapId;
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_uslp::{TfdfHeader, TfdzRule, Upid, VcfCountLen};

    use super::{Framing, MapEntry, SegmenterProtocol};
    use crate::common::Qos;
    use crate::protocol::Uslp;

    /// The longest TFDF the 16-bit Frame Length of a USLP frame allows (732.1 §4.1.2.7).
    const MAX_TFDF: usize = 0xFFFF;

    /// The parameters of a variable-length USLP VC that the `MapSegmenter` needs: the VCF Count
    /// Length for each `QoS` (732.1 §4.1.2.11–12, Table 4-2; LNK-078).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct UslpSegmenterParams {
        vcf_len_sc: VcfCountLen,
        vcf_len_exp: VcfCountLen,
    }

    impl UslpSegmenterParams {
        /// A VC whose frames carry a VCF Count of `vcf_len_sc` octets on the
        /// sequence-controlled service and `vcf_len_exp` on the expedited service.
        ///
        /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
        #[must_use]
        pub const fn new(vcf_len_sc: VcfCountLen, vcf_len_exp: VcfCountLen) -> UslpSegmenterParams {
            UslpSegmenterParams {
                vcf_len_sc,
                vcf_len_exp,
            }
        }

        /// The VCF Count Length of the sequence-controlled service.
        ///
        /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
        #[must_use]
        pub const fn vcf_len_sc(&self) -> VcfCountLen {
            self.vcf_len_sc
        }

        /// The VCF Count Length of the expedited service.
        ///
        /// Requirements: LNK-040…LNK-043, LNK-002, LNK-003; docs/04-engines.md §2.3.
        #[must_use]
        pub const fn vcf_len_exp(&self) -> VcfCountLen {
            self.vcf_len_exp
        }

        const fn count_len(self, qos: Qos) -> VcfCountLen {
            match qos {
                Qos::SequenceControlled => self.vcf_len_sc,
                Qos::Expedited => self.vcf_len_exp,
            }
        }
    }

    impl SegmenterProtocol for Uslp {
        type Map = MapId<4>;
        type Channel = UslpSegmenterParams;
        type MapParams = Upid;
        const STREAMS: bool = true;

        fn header_len(_: &UslpSegmenterParams) -> usize {
            1
        }

        fn max_fdu_len(_: &UslpSegmenterParams) -> usize {
            MAX_TFDF
        }

        fn implicit_map(_: &UslpSegmenterParams) -> Option<MapEntry<Uslp>> {
            None
        }

        // A VC that segments, or a MAP that blocks, needs a VCF Count to verify that the portions
        // are contiguous (732.1 §4.1.4.2.2.1.6, §4.2.2.2.7 note; LNK-078). The sequence-controlled
        // service is the one COP-1 uses; a request of the expedited service for an FDU that needs
        // the count is refused when it is made (`allows`).
        fn check_map(channel: &UslpSegmenterParams, map: &MapEntry<Uslp>) -> Result<()> {
            if *map.params() == Upid::OnlyIdleData {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::UPID));
            }
            let counted = map.segmentation().is_some() || map.blocking();
            if counted && channel.vcf_len_sc.get() == 0 {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VCF_COUNT_LEN));
            }
            Ok(())
        }

        fn allows(
            channel: &UslpSegmenterParams,
            qos: Qos,
            framing: Framing,
            blocked: bool,
        ) -> bool {
            let counted = !matches!(framing, Framing::Whole | Framing::Stream) || blocked;
            !counted || channel.count_len(qos).get() != 0
        }

        fn write_header(
            _: &UslpSegmenterParams,
            map: &MapEntry<Uslp>,
            framing: Framing,
            header: &mut [u8],
        ) {
            let rule = match framing {
                Framing::Whole => TfdzRule::NoSegmentation,
                Framing::First => TfdzRule::StartingSegment,
                Framing::Continuing => TfdzRule::ContinuingSegment,
                Framing::Last => TfdzRule::LastSegment,
                Framing::Stream => TfdzRule::OctetStream,
            };
            let [first, ..] = TfdfHeader {
                rule,
                upid: *map.params(),
            }
            .octets();
            for (d, s) in header.iter_mut().zip([first]) {
                *d = s;
            }
        }
    }
}

#[cfg(feature = "uslp")]
pub use uslp_impl::UslpSegmenterParams;
