//! C ABI of the `MapSegmenter` (LNK-040…LNK-043, LNK-012, LNK-003, LNK-074, LNK-075,
//! CABI-030…032, CABI-040; M4 plan D4, WP-7d).
//!
//! One opaque handle (`aldis_map_segmenter_t`) in the pattern of [`crate::link`]: the caller
//! declares the typed storage, `_init` places the engine of the protocol named in the
//! configuration in it, and every input is one `_step` call that returns the actions of the
//! input in a fixed-capacity actions struct. The octets of an FDU go to the caller's `out` area
//! and the action holds their `aldis_range_t` (API-061). The status is a read-only call
//! (API-056); there is no callback. A failed call leaves the engine as it was and writes no
//! actions (API-058).
//!
//! **Protocol.** The first member of the configuration is an `aldis_link_channel_t` whose
//! `protocol` names the protocol: TC or USLP (TM and AOS are `ALDIS_ERR_INVALID_CONFIG` on
//! `link_protocol`). The other members of the channel are not read (`protocol` is the tag of the
//! configuration, CABI-040, OI-92); the segmenter reads the `vc_id` of the channel, the VC it
//! serves. A reconfiguration cannot change the protocol (LNK-003). The MAP table is `map_count`
//! entries of `maps`; the segmenter of a TC VC without a Segment Header takes none (it has the
//! implicit MAP 0, LNK-040).
//!
//! The queue is `ALDIS_LINK_PACKET_QUEUE_OCTETS` octets, four of them for each queued entry
//! (LNK-015), and the segmenter serves `ALDIS_LINK_MAX_MAP_CHANNELS` MAPs (CABI-030). It is built
//! in place (`crate::handle::place`, OI-91), so the queue is not on the stack.

use aldis::core::time::{Duration, Instant};
use aldis::core::{Error, ErrorKind, Result, field};
use aldis::link::protocol::{Tc, Uslp};
use aldis::link::segmenter::{
    MAP_SEGMENTER_MAX_ACTIONS, MapEntry, MapSegmenter, MapSegmenterBuffers, MapSegmenterConfig,
    MapSegmenterHead, SegmenterActions, SegmenterInput, SegmenterOutput, SegmenterProtocol,
    SegmenterService, SegmenterStatus, TcSegmenterParams, UslpSegmenterParams,
};
use aldis::tc::{MAX_FRAME_LEN as TC_MAX_FRAME_LEN, PhysicalChannelConfig as TcPhysical};
use aldis::uslp::{TransferFrameType, Upid, VcParams, VcfCountLen};

use super::common::{aldis_map_service, framing_code, listed, range, wrong_service};
use crate::abi::{self, Out, flag};
use crate::capacities::{ALDIS_LINK_MAX_MAP_CHANNELS, ALDIS_LINK_PACKET_QUEUE_OCTETS};
use crate::handle::{self, InPlace, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, protocol, protocol_tag, qos,
    qos_code, unsupported_protocol,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The MAPs of the engine: `ALDIS_LINK_MAX_MAP_CHANNELS` (CABI-030).
const M: usize = ALDIS_LINK_MAX_MAP_CHANNELS;
/// The queue of the segmenter: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const Q: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;

/// The most actions one `aldis_map_segmenter_step` returns (LNK-074): an FDU or the octets a
/// reconfiguration discarded.
pub const ALDIS_MAP_SEGMENTER_MAX_ACTIONS: usize = 1;
const _: () = assert!(ALDIS_MAP_SEGMENTER_MAX_ACTIONS == MAP_SEGMENTER_MAX_ACTIONS);

/// 64-bit words of one `aldis_map_segmenter_t` (CABI-031): the tag and the larger of the engines
/// of TC and USLP at the build capacities; the build fails when it outgrows it.
pub const ALDIS_MAP_SEGMENTER_WORDS: usize = 1172;
const _: () =
    assert!(size_of::<crate::handle::Slot<MapSegmenterState>>() <= ALDIS_MAP_SEGMENTER_WORDS * 8);

type Sg<P> = MapSegmenter<P, M, Q>;

handle::engine_enum! {
    /// The `MapSegmenter` of the protocol chosen at `aldis_map_segmenter_init` (LNK-001).
    #[expect(
        dead_code,
        reason = "the variants are written in place by the init function, not constructed"
    )]
    pub(crate) enum MapSegmenterState {
        /// TC.
        Tc(Sg<Tc>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC),
        /// USLP.
        Uslp(Sg<Uslp>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP),
    }
}

/// The kind of an `aldis_map_segmenter_input_t` (LNK-040…LNK-043, LNK-012, LNK-003; API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_map_segmenter_input {
    /// A whole packet (`data`) for the MAP `map_id` (LNK-040, LNK-042).
    ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_PACKET = 0,
    /// A whole SDU (`data`) for the MAP `map_id` (LNK-040, LNK-042).
    ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU = 1,
    /// Octets of the stream (`data`) of the MAP `map_id` (USLP; LNK-042).
    ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS = 2,
    /// The next FDU or TFDF for the request `handle` with the quality of service `qos`
    /// (LNK-043).
    ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU = 3,
    /// The release deadline of a started stream TFDZ may have passed (API-053, LNK-012).
    ALDIS_MAP_SEGMENTER_INPUT_TIMEOUT = 4,
    /// Replace the configuration (`config`) between data units, discarding the unsent octets
    /// (LNK-003). The protocol cannot change.
    ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE = 5,
}
mirror!(pub aldis_map_segmenter_input: u32 {
    ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_PACKET,
    ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU,
    ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS,
    ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU,
    ALDIS_MAP_SEGMENTER_INPUT_TIMEOUT,
    ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE
});

/// The kind of an `aldis_map_segmenter_action_t` (API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_map_segmenter_action {
    /// An FDU or TFDF, in `out` (LNK-043).
    ALDIS_MAP_SEGMENTER_ACTION_FDU = 0,
    /// A reconfiguration discarded `octets` of the unsent queue (LNK-003).
    ALDIS_MAP_SEGMENTER_ACTION_DISCARDED = 1,
}
mirror!(pub aldis_map_segmenter_action: u32 {
    ALDIS_MAP_SEGMENTER_ACTION_FDU,
    ALDIS_MAP_SEGMENTER_ACTION_DISCARDED
});

/// A `MapSegmenter` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-040, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_map_segmenter_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_MAP_SEGMENTER_WORDS],
}

impl Storage for aldis_map_segmenter_t {
    type State = MapSegmenterState;
    const MAGIC: u32 = 0x414C_4D53; // "ALMS"
}

/// One MAP of a `MapSegmenter` (232.0 §5.4 Table 5-4; 732.1 §5.5 Table 5-4; LNK-040, LNK-042).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_segmenter_map_t {
    /// The MAP ID: 0 to 63 on TC, 0 to 15 on USLP.
    pub map_id: u8,
    /// `ALDIS_MAP_SERVICE_*`: `ANY` on TC; `PACKETS`, `SDU`, or `OCTET_STREAM` on USLP.
    pub service: u32,
    /// 1 when SDUs longer than one FDU are segmented, else 0 (not for a stream).
    pub segmentation: u8,
    /// The maximum `MAP_SDU` length when `segmentation` is 1, from 1 to 65 535 and at most the
    /// queue less four octets; ignored otherwise.
    pub max_sdu_len: usize,
    /// 1 when packets of the MAP may share an FDU (blocking), else 0 (not for a stream).
    pub blocking: u8,
    /// USLP: the UPID of the TFDFs of the MAP, 0 to 31 but not "Only Idle Data" (732.1
    /// §4.1.4.2.3).
    pub uslp_upid: u8,
    /// USLP, `OCTET_STREAM`: the release deadline of a started TFDZ in microseconds (732.1 §5.4
    /// Table 5-3 note 4; LNK-012).
    pub release_deadline_us: u64,
}

/// The managed parameters of a `MapSegmenter` (232.0 §5.3, §5.4; 732.1 §5.3, §5.4; LNK-040…
/// LNK-042, LNK-078).
///
/// Zero-initialize the struct before setting members: a member that the chosen `protocol` does not
/// read is ignored (OI-92).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_segmenter_config_t {
    /// The protocol (TC or USLP) and the VC it serves (`vc_id`); the other members of the
    /// channel are not read.
    pub channel: aldis_link_channel_t,
    /// The longest FDU or TFDF in octets, header included (TC: at most 1 019, 1 017 with a FECF;
    /// USLP: at most 65 535).
    pub max_fdu_len: usize,
    /// TC: 1 when every FDU has a Segment Header, else 0.
    pub tc_segment_header: u8,
    /// TC: 1 when the Physical Channel has a FECF, else 0.
    pub tc_fecf: u8,
    /// TC without a Segment Header: 1 when the packets of the implicit MAP 0 may be blocked.
    pub tc_blocking: u8,
    /// USLP: the VCF Count Length of sequence-controlled frames, 0 to 7 octets (732.1
    /// §4.1.2.11).
    pub uslp_vcf_len_sc: u8,
    /// USLP: the VCF Count Length of expedited frames, 0 to 7 octets.
    pub uslp_vcf_len_exp: u8,
    /// The entries of `maps` in use: none for a TC VC without a Segment Header, else 1 to
    /// `ALDIS_LINK_MAX_MAP_CHANNELS`.
    pub map_count: usize,
    /// The MAPs; the first `map_count` are read.
    pub maps: [aldis_map_segmenter_map_t; ALDIS_LINK_MAX_MAP_CHANNELS],
}

/// One input of a `MapSegmenter` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_map_segmenter_input_t {
    /// `ALDIS_MAP_SEGMENTER_INPUT_*`.
    pub kind: u32,
    /// For `SUBMIT_*`: the packet, SDU, or octets, read once and copied (API-060). `data` is
    /// checked on every kind and must not overlap the storage or `out`: `NULL` only with
    /// `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `SUBMIT_*`: the MAP ID (on a TC VC without a Segment Header, 0).
    pub map_id: u8,
    /// For `NEXT_FDU`: the handle of the request, echoed in the FDU (API-054).
    pub handle: u32,
    /// For `NEXT_FDU`: `ALDIS_LINK_QOS_*`; sequence-controlled for FOP-1 `RequestAd`, expedited
    /// for `RequestBd`.
    pub qos: u32,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_map_segmenter_config_t,
}

/// One action of a `MapSegmenter` (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_segmenter_action_t {
    /// `ALDIS_MAP_SEGMENTER_ACTION_*`.
    pub kind: u32,
    /// For `FDU`: the handle of the request.
    pub handle: u32,
    /// For `FDU`: the VCID of the segmenter.
    pub vc_id: u8,
    /// For `FDU`: 1 when the FDU has a MAP ID, 0 on a TC VC without a Segment Header.
    pub map_present: u8,
    /// For `FDU`: the MAP ID.
    pub map_id: u8,
    /// For `FDU`: `ALDIS_LINK_QOS_*` of the request.
    pub qos: u32,
    /// For `FDU`: `ALDIS_MAP_FRAMING_*`.
    pub framing: u32,
    /// For `FDU`: where the FDU or TFDF is in `out`, its header included.
    pub fdu: aldis_range_t,
    /// For `DISCARDED`: the octets of the unsent queue.
    pub octets: usize,
}

/// The actions of one input of a `MapSegmenter`.
///
/// Requirements: LNK-040, LNK-070, LNK-074, CABI-040; docs/04-engines.md §4.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_segmenter_actions_t {
    /// The number of actions in `items`, at most `ALDIS_MAP_SEGMENTER_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_map_segmenter_action_t; ALDIS_MAP_SEGMENTER_MAX_ACTIONS],
}

/// A read-only snapshot of a `MapSegmenter` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_segmenter_status_t {
    /// Packets, SDUs, and runs of stream octets taken.
    pub submitted: u64,
    /// FDUs and TFDFs handed out.
    pub fdus_out: u64,
    /// Inputs refused as invalid.
    pub refused: u64,
    /// Inputs refused with `WouldBlock` (LNK-015).
    pub would_block: u64,
    /// Octets discarded by `RECONFIGURE` (LNK-003).
    pub discarded_octets: u64,
    /// The queue: the unsent user octets.
    pub queue: aldis_link_occupancy_t,
}

/// The protocol-specific part of the segmenter configuration (LNK-001).
trait SgProtocol: SegmenterProtocol {
    /// The parameters of the VC.
    fn channel(c: &aldis_map_segmenter_config_t) -> Result<Self::Channel>;
    /// The MAP `m`.
    fn entry(m: &aldis_map_segmenter_map_t) -> Result<MapEntry<Self>>;
}

/// The MAP `m` with the service and the parameters the protocol read from it.
fn sg_entry<P: SegmenterProtocol>(
    m: &aldis_map_segmenter_map_t,
    service: SegmenterService,
    params: P::MapParams,
) -> Result<MapEntry<P>> {
    let mut entry = MapEntry::<P>::new(P::Map::try_from(m.map_id)?, service, params);
    if flag(m.segmentation)? {
        entry = entry.with_segmentation(m.max_sdu_len)?;
    }
    if flag(m.blocking)? {
        entry = entry.with_blocking();
    }
    Ok(entry)
}

impl SgProtocol for Tc {
    fn channel(c: &aldis_map_segmenter_config_t) -> Result<TcSegmenterParams> {
        // The configuration gives the FECF and no frame length, and the segmenter reads only
        // the FECF: the longest frame the standard allows stands for the channel's (232.0
        // §4.1.3.1.2).
        let physical = TcPhysical::new(TC_MAX_FRAME_LEN, flag(c.tc_fecf)?)?;
        Ok(if flag(c.tc_segment_header)? {
            TcSegmenterParams::with_segment_header(physical)
        } else {
            TcSegmenterParams::without_segment_header(physical, flag(c.tc_blocking)?)
        })
    }

    fn entry(m: &aldis_map_segmenter_map_t) -> Result<MapEntry<Tc>> {
        match parse::<aldis_map_service>(m.service)? {
            aldis_map_service::ALDIS_MAP_SERVICE_ANY => {
                sg_entry::<Tc>(m, SegmenterService::Any, ())
            }
            aldis_map_service::ALDIS_MAP_SERVICE_PACKETS
            | aldis_map_service::ALDIS_MAP_SERVICE_SDU
            | aldis_map_service::ALDIS_MAP_SERVICE_OCTET_STREAM => Err(wrong_service()),
        }
    }
}

/// The UPID of a MAP (732.1 §4.1.4.2.3).
///
/// # Errors
/// `OutOfRange` (`upid`) above 31.
fn upid(bits: u8) -> Result<Upid> {
    Upid::from_bits(bits).ok_or(Error::out_of_range(field::UPID, 31, u64::from(bits)))
}

impl SgProtocol for Uslp {
    fn channel(c: &aldis_map_segmenter_config_t) -> Result<UslpSegmenterParams> {
        let mut vc = VcParams::new(TransferFrameType::Variable);
        vc.vcf_count_len_sequence = VcfCountLen::new(c.uslp_vcf_len_sc)?;
        vc.vcf_count_len_expedited = VcfCountLen::new(c.uslp_vcf_len_exp)?;
        UslpSegmenterParams::new(vc)
    }

    fn entry(m: &aldis_map_segmenter_map_t) -> Result<MapEntry<Uslp>> {
        let service = match parse::<aldis_map_service>(m.service)? {
            aldis_map_service::ALDIS_MAP_SERVICE_PACKETS => SegmenterService::Packets,
            aldis_map_service::ALDIS_MAP_SERVICE_SDU => SegmenterService::Sdu,
            aldis_map_service::ALDIS_MAP_SERVICE_OCTET_STREAM => SegmenterService::OctetStream {
                release_deadline: Duration::from_micros(m.release_deadline_us),
            },
            aldis_map_service::ALDIS_MAP_SERVICE_ANY => return Err(wrong_service()),
        };
        sg_entry::<Uslp>(m, service, upid(m.uslp_upid)?)
    }
}

/// The segmenter configuration of `c` for the protocol `P`.
fn sg_config<P: SgProtocol>(c: &aldis_map_segmenter_config_t) -> Result<MapSegmenterConfig<P, M>> {
    let channel = P::channel(c)?;
    let vc = P::Vc::try_from(c.channel.vc_id)?;
    let mut table = [None; M];
    let count = listed(&c.maps, c.map_count, P::entry, &mut table)?;
    // The first entry fills the array; only the first `count` of it are given on. Without an
    // entry there is nothing to list.
    let first = table.first().copied().flatten().filter(|_| count > 0);
    let mut entries = first.map(|filler| [filler; M]);
    if let Some(array) = entries.as_mut() {
        for (dst, src) in array.iter_mut().zip(&table).take(count) {
            if let Some(entry) = src {
                *dst = *entry;
            }
        }
    }
    let given = match &entries {
        None => &[],
        Some(array) => array
            .get(..count)
            .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID))?,
    };
    MapSegmenterConfig::new(vc, channel, c.max_fdu_len, given)
}

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are plain octet arrays, valid when zeroed.
unsafe impl<P: SegmenterProtocol, const NM: usize, const NQ: usize> InPlace
    for MapSegmenter<P, NM, NQ>
{
    type Head = MapSegmenterHead<P, NM>;
    type Buffers = MapSegmenterBuffers<NQ>;
    const HEAD_OFFSET: usize = <MapSegmenter<P, NM, NQ>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <MapSegmenter<P, NM, NQ>>::BUFFERS_OFFSET;
}

/// One step of `engine`, in the protocol `P` (API-057).
fn sg_run<P: SgProtocol>(
    engine: &mut Sg<P>,
    input: &aldis_map_segmenter_input_t,
    data: &[u8],
    now: Instant,
    area: &mut [u8],
) -> Result<SegmenterActions> {
    let input = match parse::<aldis_map_segmenter_input>(input.kind)? {
        aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_PACKET => {
            SegmenterInput::SubmitPacket {
                map: P::Map::try_from(input.map_id)?,
                packet: data,
            }
        }
        aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU => {
            SegmenterInput::SubmitSdu {
                map: P::Map::try_from(input.map_id)?,
                sdu: data,
            }
        }
        aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS => {
            SegmenterInput::SubmitOctets {
                map: P::Map::try_from(input.map_id)?,
                octets: data,
            }
        }
        aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU => SegmenterInput::NextFdu {
            handle: input.handle,
            qos: qos(input.qos)?,
        },
        aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_TIMEOUT => SegmenterInput::Timeout,
        aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE => {
            SegmenterInput::Reconfigure(sg_config::<P>(&input.config)?)
        }
    };
    engine.step(input, now, area)
}

impl MapSegmenterState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_map_segmenter_input_t,
        data: &[u8],
        now: Instant,
        area: &mut [u8],
    ) -> Result<SegmenterActions> {
        let reconfigure = parse::<aldis_map_segmenter_input>(input.kind)?
            == aldis_map_segmenter_input::ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE;
        let ch = &input.config.channel;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want| protocol(ch).map(|p| p == want);
        match self {
            MapSegmenterState::Tc(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC)? {
                    return Err(unsupported_protocol());
                }
                sg_run(e, input, data, now, area)
            }
            MapSegmenterState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                sg_run(e, input, data, now, area)
            }
        }
    }

    const fn status(&self) -> SegmenterStatus {
        match self {
            MapSegmenterState::Tc(e) => *e.status(),
            MapSegmenterState::Uslp(e) => *e.status(),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        match self {
            MapSegmenterState::Tc(e) => e.next_deadline(),
            MapSegmenterState::Uslp(e) => e.next_deadline(),
        }
    }
}

fn sg_action(output: &SegmenterOutput) -> aldis_map_segmenter_action_t {
    match output {
        SegmenterOutput::Fdu(f) => aldis_map_segmenter_action_t {
            kind: ALDIS_MAP_SEGMENTER_ACTION_FDU,
            handle: f.handle,
            vc_id: f.vc,
            map_present: u8::from(f.map.is_some()),
            map_id: f.map.unwrap_or(0),
            qos: qos_code(f.qos),
            framing: framing_code(f.framing),
            fdu: range(f.fdu),
            ..aldis_map_segmenter_action_t::default()
        },
        SegmenterOutput::Discarded { octets } => aldis_map_segmenter_action_t {
            kind: ALDIS_MAP_SEGMENTER_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_map_segmenter_action_t::default()
        },
    }
}

/// Places a `MapSegmenter` with an empty queue in `storage` (LNK-040, LNK-042; CABI-031,
/// CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0
/// or 1, or a constant that is none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TM and
/// AOS, (`max_fdu_len`, `map_id`, `data_field_content`, `blocking`, `segmentation`,
/// `max_sdu_len`, `vcf_count_len`, `upid`) for a value the configuration relations refuse;
/// `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `map_id`, `upid`);
/// `ALDIS_ERR_CAPACITY_EXCEEDED` (`map_id`) for more MAPs than
/// `ALDIS_LINK_MAX_MAP_CHANNELS`, (`max_sdu_len`, `max_fdu_len`) for an SDU or a TFDZ the queue
/// cannot hold.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_map_segmenter_t` and outlives its use; `cfg` is
/// valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_segmenter_init(
    storage: *mut aldis_map_segmenter_t,
    cfg: *const aldis_map_segmenter_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            match protocol(&cfg.channel)? {
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => handle::init_variant::<_, Sg<Tc>>(
                    storage,
                    Sg::new_head(sg_config::<Tc>(&cfg)?)?,
                ),
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
                    handle::init_variant::<_, Sg<Uslp>>(
                        storage,
                        Sg::new_head(sg_config::<Uslp>(&cfg)?)?,
                    )
                }
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM
                | aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => Err(unsupported_protocol()),
            }
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn sg_engine(h: *mut aldis_map_segmenter_t) -> Result<handle::Handle<MapSegmenterState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

handle::step_io!(
    aldis_map_segmenter_t,
    aldis_map_segmenter_input_t,
    aldis_map_segmenter_actions_t
);

impl handle::TimedStep for aldis_map_segmenter_t {
    fn step(
        state: &mut MapSegmenterState,
        input: &aldis_map_segmenter_input_t,
        now: Instant,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_map_segmenter_actions_t>,
    ) -> Result<()> {
        let list = state.step(input, data, now, area)?;
        let mut result = aldis_map_segmenter_actions_t::default();
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = sg_action(output);
        }
        // `list` holds at most `MAP_SEGMENTER_MAX_ACTIONS` actions, the length of `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
}

/// Runs one input of the `MapSegmenter` (API-057, CABI-040).
///
/// `now_us` is the time in microseconds (API-003). The FDU goes to `out`, which must hold at
/// least `max_fdu_len` octets for `NEXT_FDU` (LNK-075).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a constant
/// that is none; `ALDIS_ERR_OUT_OF_RANGE` (`map_id`) above the MAP IDs of the protocol; for
/// `SUBMIT_*`: `ALDIS_ERR_UNKNOWN_CHANNEL`, `ALDIS_ERR_INVALID_FIELD` (`data_field_content`,
/// `packet_version`), `ALDIS_ERR_LENGTH_MISMATCH` (`packet_len`, `sdu_len`),
/// `ALDIS_ERR_OUT_OF_RANGE` (`max_sdu_len`, `max_fdu_len`), `ALDIS_ERR_WOULD_BLOCK` (the queue
/// cannot take it whole, LNK-015); for `NEXT_FDU`: `ALDIS_ERR_BUFFER_TOO_SMALL`,
/// `ALDIS_ERR_INVALID_FIELD` (`qos`); for `RECONFIGURE`: as `aldis_map_segmenter_init`, and
/// `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another protocol. The engine is unchanged
/// and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_map_segmenter_init` whose storage is still valid, or detectably
/// invalid storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is
/// valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is
/// valid for one write; `err` is `NULL` or valid for one write. No other thread uses the
/// storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_segmenter_step(
    h: *mut aldis_map_segmenter_t,
    input: *const aldis_map_segmenter_input_t,
    now_us: u64,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_map_segmenter_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe {
        handle::step_timed::<aldis_map_segmenter_t>(h, input, now_us, out, out_cap, actions, err)
    }
}

/// Reads the release deadline of the started stream TFDZs (LNK-012, API-053, CABI-040).
///
/// While a deadline is pending, `armed` is 1 and `deadline_us` the instant in microseconds at
/// which the caller is to feed the `TIMEOUT` input; otherwise both are 0. It changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_map_segmenter_step`]; `armed` and `deadline_us` are valid for one write each.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_segmenter_next_deadline(
    h: *mut aldis_map_segmenter_t,
    armed: *mut u8,
    deadline_us: *mut u64,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = sg_engine(h)?;
            let armed = abi::out_disjoint(armed, &[h.range()])?;
            let deadline_us = abi::out_disjoint(deadline_us, &[h.range()])?;
            let deadline = h.read(MapSegmenterState::next_deadline);
            armed.write(u8::from(deadline.is_some()));
            deadline_us.write(deadline.map_or(0, Instant::as_micros));
            Ok(())
        })
    }
}

/// Reads the counters and the occupancy of the `MapSegmenter` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_map_segmenter_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_segmenter_status(
    h: *mut aldis_map_segmenter_t,
    status: *mut aldis_map_segmenter_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = sg_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(MapSegmenterState::status);
            status.write(aldis_map_segmenter_status_t {
                submitted: s.submitted.get(),
                fdus_out: s.fdus_out.get(),
                refused: s.refused.get(),
                would_block: s.would_block.get(),
                discarded_octets: s.discarded_octets.get(),
                queue: s.queue.into(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_map_segmenter_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_segmenter_deinit(h: *mut aldis_map_segmenter_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
