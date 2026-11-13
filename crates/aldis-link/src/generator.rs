//! The `PacketGenerator`: fills fixed-length data fields with packets (LNK-010…LNK-015, LNK-002,
//! LNK-003, LNK-074, LNK-075; docs/04-engines.md §2.1, §4).
//!
//! The engine serves one VC (one MAP for USLP) of one protocol (TM, AOS, or USLP rule `000`,
//! LNK-001). It takes packets whole (`SubmitPacket`), places them contiguously in the data
//! fields of the VC in submission order, splits a packet that does not fit, and gives the field
//! with its First Header Pointer to the caller when it is released (`Release`). It holds no
//! output queue (API-059): the framework feeds the returned field to the multiplexer.
//!
//! **Fields.** A field is *started* when its first octet is written, and *releasable* when it is
//! full or when its release deadline has passed since it started and `Timeout` has said so
//! (LNK-012). A releasable field that is not full is completed with idle packets when it is
//! released (LNK-013). The octets of a packet that do not fit are queued whole behind it
//! (LNK-015).
//!
//! **Bounds (LNK-074, LNK-075, API-062).** Every input returns at most one action
//! ([`PACKET_GENERATOR_MAX_ACTIONS`]). `Release` writes at most `D` octets to the output area,
//! the data field of the VC, and fails with `BufferTooSmall` when the area is shorter than the
//! field it would return. The other inputs write nothing.

use aldis_core::engine::{Actions, OutRange};
use aldis_core::time::{Duration, Instant};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, IdlePacketKind, IdlePolicy, Occupancy, ValidPvns};
use crate::packet::{self, Kind, Length};
use crate::protocol::{IdleField, PacketProtocol, Pointer, Route};

/// The most actions one input of the `PacketGenerator` returns: `Release` returns at most one
/// `DataField`, `Reconfigure` at most one `Discarded` (LNK-074, docs/04-engines.md §4).
pub const PACKET_GENERATOR_MAX_ACTIONS: usize = 1;

/// The longest Space Packet Idle Packet that is not shorter than its header and one data octet
/// (133.0 §4.1.3.3.4.4, SPP-010): 7 octets. A remainder below it spills into the next field
/// (LNK-013).
const MIN_SPACE_IDLE: usize = 7;

/// The configuration of a `PacketGenerator` (LNK-010, LNK-012…LNK-014, LNK-078).
///
/// The managed parameters of one VC or MAP that carries Packets (132.0 §5.4 Table 5-4; 732.0
/// §5.5 Table 5-4; 732.1 §5.6 Table 5-5).
///
/// Built only by [`new`](Self::new) and the `with_` methods that check the relations between
/// parameters (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketGeneratorConfig<P: PacketProtocol> {
    vc: P::Vc,
    protocol: P::Params,
    field_len: usize,
    valid_pvns: ValidPvns,
    max_packet_len: usize,
    release_deadline: Option<Duration>,
    idle_policy: IdlePolicy,
    idle_packet_kind: IdlePacketKind,
    idle_pattern: u8,
}

impl<P: PacketProtocol> PacketGeneratorConfig<P> {
    /// The configuration of VC `vc` whose data field is `field_len` octets, header included
    /// (TM data field, AOS `M_PDU`, USLP TFDF), for packets of the PVNs `valid_pvns` of at most
    /// `max_packet_len` octets. It has no release deadline, the `Oid` idle policy, the pattern
    /// `0x00`, and the first idle packet kind of the set that the protocol allows
    /// (LNK-010, LNK-012…LNK-014).
    ///
    /// # Errors
    /// `InvalidConfig` (`field_len`) unless the data field holds the header, at least one octet
    /// of packet zone, and at most what the protocol's pointer can address (132.0 §4.1.2.7.6;
    /// 732.0 §4.1.4.2.2; 732.1 §4.1.4.2.4), or (`idle_packet_kind`) a Space Packet idle packet
    /// on a field with a packet zone shorter than 7 octets (LNK-078); `InvalidConfig`
    /// (`max_packet_len`) for 0.
    pub fn new(
        vc: P::Vc,
        protocol: P::Params,
        field_len: usize,
        valid_pvns: ValidPvns,
        max_packet_len: usize,
    ) -> Result<PacketGeneratorConfig<P>> {
        if max_packet_len == 0 {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_PACKET_LEN));
        }
        let zone = field_len.saturating_sub(P::PACKET_HEADER_LEN);
        let kind = P::IDLE_KIND.unwrap_or(if valid_pvns.space() && zone >= MIN_SPACE_IDLE {
            IdlePacketKind::SpacePacket
        } else {
            IdlePacketKind::Encapsulation
        });
        PacketGeneratorConfig {
            vc,
            protocol,
            field_len,
            valid_pvns,
            max_packet_len,
            release_deadline: None,
            idle_policy: IdlePolicy::Oid,
            idle_packet_kind: kind,
            idle_pattern: 0x00,
        }
        .checked()
    }

    /// The same configuration with the idle policy, the idle packet kind that completes a field
    /// (ignored where the protocol fixes it, USLP), and the idle data octet (LNK-013, LNK-014).
    ///
    /// # Errors
    /// `InvalidConfig` (`idle_packet_kind`) if the kind's PVN is not in the valid set, or it is
    /// a Space Packet idle packet and the packet zone is shorter than 7 octets (LNK-078).
    pub fn with_idle(
        self,
        policy: IdlePolicy,
        kind: IdlePacketKind,
        pattern: u8,
    ) -> Result<PacketGeneratorConfig<P>> {
        PacketGeneratorConfig {
            idle_policy: policy,
            idle_packet_kind: P::IDLE_KIND.unwrap_or(kind),
            idle_pattern: pattern,
            ..self
        }
        .checked()
    }

    /// The same configuration with a release deadline: the longest a started field waits to be
    /// completed before it must be released (732.1 §5.4 Table 5-3 and note 4; LNK-012).
    #[must_use]
    pub const fn with_release_deadline(self, deadline: Duration) -> PacketGeneratorConfig<P> {
        PacketGeneratorConfig {
            release_deadline: Some(deadline),
            ..self
        }
    }

    /// Checks the relations between the parameters (LNK-078).
    fn checked(self) -> Result<PacketGeneratorConfig<P>> {
        let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
        let zone = self.zone_len();
        if zone == 0 || zone > P::max_zone_len(&self.protocol) {
            return Err(bad(field::FIELD_LEN));
        }
        if P::IDLE_KIND.is_none() {
            let fits = match self.idle_packet_kind {
                IdlePacketKind::SpacePacket => zone >= MIN_SPACE_IDLE,
                IdlePacketKind::Encapsulation => true,
            };
            if !self.idle_packet_kind.is_valid_in(self.valid_pvns) || !fits {
                return Err(bad(field::IDLE_PACKET_KIND));
            }
        }
        Ok(self)
    }

    /// The octets of the packet zone: the data field without its header.
    const fn zone_len(&self) -> usize {
        self.field_len.saturating_sub(P::PACKET_HEADER_LEN)
    }

    /// The VC (the MAP for USLP) the generator serves.
    ///
    /// Requirements: LNK-010…LNK-015, LNK-002, LNK-003; docs/04-engines.md §2.1.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
    }

    /// The octets of the data field, header included.
    ///
    /// Requirements: LNK-010…LNK-015, LNK-002, LNK-003; docs/04-engines.md §2.1.
    #[must_use]
    pub const fn field_len(&self) -> usize {
        self.field_len
    }

    /// The valid Packet Version Numbers (LNK-010).
    #[must_use]
    pub const fn valid_pvns(&self) -> ValidPvns {
        self.valid_pvns
    }

    /// The Maximum Packet Length (LNK-010).
    #[must_use]
    pub const fn max_packet_len(&self) -> usize {
        self.max_packet_len
    }

    /// The release deadline, if any (LNK-012).
    #[must_use]
    pub const fn release_deadline(&self) -> Option<Duration> {
        self.release_deadline
    }

    /// The idle policy (LNK-014).
    #[must_use]
    pub const fn idle_policy(&self) -> IdlePolicy {
        self.idle_policy
    }

    /// The idle packet kind that completes a field (LNK-013).
    #[must_use]
    pub const fn idle_packet_kind(&self) -> IdlePacketKind {
        self.idle_packet_kind
    }

    /// The octet idle packets and idle fields are filled with (LNK-013).
    #[must_use]
    pub const fn idle_pattern(&self) -> u8 {
        self.idle_pattern
    }
}

/// An input of the `PacketGenerator` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorInput<'a, P: PacketProtocol> {
    /// A packet, whole and with its length field agreeing with the octets (LNK-010, LNK-015).
    SubmitPacket(&'a [u8]),
    /// Give out the next releasable field, or the idle field of the `IdleField` policy
    /// (LNK-012, LNK-014).
    Release,
    /// The release deadline of the started field may have passed (API-053, LNK-012).
    Timeout,
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(PacketGeneratorConfig<P>),
}

/// A data field the generator gives out, for the multiplexer's `Offer` (LNK-076, API-054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataField {
    /// The VCID of the generator.
    pub vc: u8,
    /// Where the field is in the output area: the TM data field, the AOS `M_PDU`, the USLP TFDF
    /// (header and TFDZ).
    pub field: OutRange,
    /// The raw First Header Pointer of the field (TM carries it in the frame header; AOS and
    /// USLP also have it inside the field).
    pub pointer: u16,
    /// The MAP and `QoS` of the field (USLP).
    pub route: Option<Route>,
}

/// An action of the `PacketGenerator` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorOutput {
    /// A data field, in the output area (LNK-011).
    DataField(DataField),
    /// `Reconfigure` discarded this many octets: the started field and the queue (LNK-003).
    Discarded {
        /// The octets held, an idle residue included.
        octets: usize,
    },
}

/// The actions of one input of the `PacketGenerator` (LNK-074).
pub type GeneratorActions = Actions<GeneratorOutput, PACKET_GENERATOR_MAX_ACTIONS>;

/// The read-only status of a `PacketGenerator`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GeneratorStatus {
    /// Packets taken.
    pub packets_in: Counter,
    /// Data fields given out (idle fields included).
    pub fields_out: Counter,
    /// Idle packets generated to complete a field (LNK-013).
    pub idle_packets: Counter,
    /// Idle fields given out under the `IdleField` policy (LNK-014).
    pub idle_fields: Counter,
    /// Packets refused as invalid (LNK-010).
    pub refused: Counter,
    /// Packets refused with `WouldBlock` (LNK-015).
    pub would_block: Counter,
    /// Octets discarded by `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The packet queue (API-060).
    pub queue: Occupancy,
    /// The field being assembled (API-060).
    pub field: Occupancy,
}

/// The `PacketGenerator` of one VC (LNK-010…LNK-015).
///
/// `D` is the capacity of the data field in octets and `Q` the capacity of the packet queue in
/// octets (LNK-015, GEN-011); a configuration that needs more is refused with
/// `CapacityExceeded`.
///
/// A caller builds the engine in place in its own storage, without an engine on the stack, from
/// [`new_head`](Self::new_head) and all-zero buffers, the empty state (CABI-040, GEN-012).
/// [`HEAD_OFFSET`](Self::HEAD_OFFSET) and [`BUFFERS_OFFSET`](Self::BUFFERS_OFFSET) are the offsets
/// of the two fields.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketGenerator<P: PacketProtocol, const D: usize, const Q: usize> {
    head: PacketGeneratorHead<P>,
    bufs: PacketGeneratorBuffers<D, Q>,
}

/// Everything of a [`PacketGenerator`] but its octet buffers: the part that is built from the
/// configuration (LNK-010…LNK-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketGeneratorHead<P: PacketProtocol> {
    config: PacketGeneratorConfig<P>,
    fill: usize,
    first_start: Option<usize>,
    started: Option<Instant>,
    releasable: bool,
    queue_len: usize,
    tail: usize,
    status: GeneratorStatus,
}

/// The octet buffers of a [`PacketGenerator`]: the data field zone (`D` octets) and the packet
/// queue (`Q` octets). Plain arrays of octets, valid when all zero, which is the empty state
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-010…LNK-015, LNK-002, LNK-003; docs/04-engines.md §2.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketGeneratorBuffers<const D: usize, const Q: usize> {
    zone: [u8; D],
    queue: [u8; Q],
}

impl<const D: usize, const Q: usize> PacketGeneratorBuffers<D, Q> {
    /// Empty buffers.
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-010…LNK-015, LNK-002, LNK-003; docs/04-engines.md §2.1.
    pub const ZERO: Self = Self {
        zone: [0; D],
        queue: [0; Q],
    };
}

impl<P: PacketProtocol, const D: usize, const Q: usize> PacketGenerator<P, D, Q> {
    /// A generator with an empty field and queue.
    ///
    /// # Errors
    /// `CapacityExceeded` (`field_len` or `max_packet_len`) if the configuration needs more
    /// than `D` octets of data field, or a Maximum Packet Length above the octets of the packet zone
    /// and the queue together, which no empty generator could take (GEN-011, LNK-015).
    pub fn new(config: PacketGeneratorConfig<P>) -> Result<PacketGenerator<P, D, Q>> {
        Ok(PacketGenerator {
            head: Self::new_head(config)?,
            bufs: PacketGeneratorBuffers::ZERO,
        })
    }

    /// The offset of the head field, for the in-place build of the C ABI (CABI-040). Not part of
    /// the API.
    #[doc(hidden)]
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the buffers field, for the in-place build of the C ABI (CABI-040). Not part
    /// of the API.
    #[doc(hidden)]
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

    /// The head of a generator with an empty field and queue: the part of [`Self::new`] that is
    /// built from the configuration.
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-010…LNK-015, LNK-002, LNK-003; docs/04-engines.md §2.1.
    pub fn new_head(config: PacketGeneratorConfig<P>) -> Result<PacketGeneratorHead<P>> {
        PacketGenerator::<P, D, Q>::check_capacity(&config)?;
        Ok(PacketGeneratorHead {
            config,
            fill: 0,
            first_start: None,
            started: None,
            releasable: false,
            queue_len: 0,
            tail: 0,
            status: GeneratorStatus::default(),
        })
    }

    fn check_capacity(config: &PacketGeneratorConfig<P>) -> Result<()> {
        let exceeded = |f| Error::new(ErrorKind::CapacityExceeded).with_field(f);
        if config.field_len > D {
            Err(exceeded(field::FIELD_LEN))
        } else if config.max_packet_len > config.zone_len().saturating_add(Q) {
            Err(exceeded(field::MAX_PACKET_LEN))
        } else {
            Ok(())
        }
    }

    /// The configuration in force.
    ///
    /// Requirements: LNK-010…LNK-015, LNK-002, LNK-003; docs/04-engines.md §2.1.
    #[must_use]
    pub const fn config(&self) -> &PacketGeneratorConfig<P> {
        &self.head.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &GeneratorStatus {
        &self.head.status
    }

    /// The earliest instant at which `Timeout` must be fed: when the started field that is not
    /// yet releasable reaches its release deadline; none without a deadline (API-053, LNK-012).
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        let deadline = self.head.config.release_deadline?;
        match (self.head.started, self.head.releasable) {
            (Some(at), false) => Some(at.saturating_add(deadline)),
            _ => None,
        }
    }

    /// Runs one input (API-057).
    ///
    /// An input that fails leaves the field, the queue, and the configuration as they were and
    /// writes no action (API-058); the housekeeping counters of the status count a refusal.
    ///
    /// # Errors
    /// - `SubmitPacket`: `InvalidField` (`packet_version`) for a PVN outside the valid set,
    ///   `LengthMismatch` (`packet_len`) if the length field disagrees with the octets given,
    ///   `OutOfRange` (`max_packet_len`) for a longer packet (LNK-010); `WouldBlock` if the
    ///   field and the queue cannot take the packet whole (LNK-015, API-052).
    /// - `Release`: `BufferTooSmall` if `out` is shorter than the data field (LNK-075).
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(
        &mut self,
        input: GeneratorInput<'_, P>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<GeneratorActions> {
        let mut actions = GeneratorActions::new();
        match input {
            GeneratorInput::SubmitPacket(packet) => self.submit(packet, now)?,
            GeneratorInput::Release => {
                if let Some(field) = self.release(now, out)? {
                    actions.push(GeneratorOutput::DataField(field))?;
                }
            }
            GeneratorInput::Timeout => self.timeout(now),
            GeneratorInput::Reconfigure(config) => {
                PacketGenerator::<P, D, Q>::check_capacity(&config)?;
                let octets = self.head.fill.saturating_add(self.head.queue_len);
                self.head.config = config;
                self.reset_field();
                self.head.queue_len = 0;
                self.head.tail = 0;
                self.head.status.discarded_octets.add(octets);
                self.refresh_occupancy();
                if octets > 0 {
                    actions.push(GeneratorOutput::Discarded { octets })?;
                }
            }
        }
        Ok(actions)
    }

    const fn zone_len(&self) -> usize {
        self.head.config.zone_len()
    }

    const fn refresh_occupancy(&mut self) {
        self.head.status.queue.set(self.head.queue_len);
        self.head.status.field.set(self.head.fill);
    }

    const fn reset_field(&mut self) {
        self.head.fill = 0;
        self.head.first_start = None;
        self.head.started = None;
        self.head.releasable = false;
    }

    /// `SubmitPacket` (LNK-010, LNK-011, LNK-015).
    fn submit(&mut self, packet: &[u8], now: Instant) -> Result<()> {
        if let Err(e) = self.check_packet(packet) {
            self.head.status.refused.incr();
            return Err(e);
        }
        let room = if self.head.releasable {
            0
        } else {
            self.zone_len().saturating_sub(self.head.fill)
        };
        let free = room.saturating_add(Q.saturating_sub(self.head.queue_len));
        if packet.len() > free {
            self.head.status.would_block.incr();
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        let into_field = room.min(packet.len());
        let (head, rest) = packet.split_at_checked(into_field).unwrap_or((&[], packet));
        if !head.is_empty() {
            self.head.first_start.get_or_insert(self.head.fill);
            self.append_to_zone(head);
            self.head.started.get_or_insert(now);
            self.head.releasable = self.head.fill >= self.zone_len();
        }
        if !rest.is_empty() {
            if !head.is_empty() {
                // The rest of a packet that began in the field is a continuation: it is the
                // only content of the queue, because a field with room takes the whole packet.
                self.head.tail = rest.len();
            }
            self.append_to_queue(rest);
        }
        self.head.status.packets_in.incr();
        self.refresh_occupancy();
        Ok(())
    }

    /// The checks of `SubmitPacket` that depend on the packet only (LNK-010).
    fn check_packet(&self, packet: &[u8]) -> Result<()> {
        let kind = packet
            .first()
            .and_then(|&b| packet::kind_of(b))
            .filter(|k| match k {
                Kind::Space => self.head.config.valid_pvns.space(),
                Kind::Encapsulation => self.head.config.valid_pvns.encapsulation(),
            })
            .ok_or(Error::new(ErrorKind::InvalidField).with_field(field::PACKET_VERSION))?;
        match packet::length(kind, packet) {
            Length::Total(n) if n == packet.len() => {}
            Length::Total(_) | Length::More | Length::Invalid => {
                return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::PACKET_LEN));
            }
        }
        if packet.len() > self.head.config.max_packet_len {
            return Err(Error::out_of_range(
                field::MAX_PACKET_LEN,
                u64::try_from(self.head.config.max_packet_len).unwrap_or(u64::MAX),
                u64::try_from(packet.len()).unwrap_or(u64::MAX),
            ));
        }
        Ok(())
    }

    /// Appends octets that fit the packet zone (the caller checked the room).
    fn append_to_zone(&mut self, octets: &[u8]) {
        for (d, s) in self.bufs.zone.iter_mut().skip(self.head.fill).zip(octets) {
            *d = *s;
        }
        self.head.fill = self.head.fill.saturating_add(octets.len());
    }

    /// Appends octets that fit the queue (the caller checked the room).
    fn append_to_queue(&mut self, octets: &[u8]) {
        for (d, s) in self
            .bufs
            .queue
            .iter_mut()
            .skip(self.head.queue_len)
            .zip(octets)
        {
            *d = *s;
        }
        self.head.queue_len = self.head.queue_len.saturating_add(octets.len());
    }

    /// `Timeout` (LNK-012, API-053): marks the started field releasable at its deadline and
    /// changes nothing before it.
    fn timeout(&mut self, now: Instant) {
        if let Some(due) = self.next_deadline()
            && now >= due
        {
            self.head.releasable = true;
        }
    }

    /// `Release` (LNK-011…LNK-014).
    fn release(&mut self, now: Instant, out: &mut [u8]) -> Result<Option<DataField>> {
        let field_len = self.head.config.field_len;
        if out.len() < field_len {
            return Err(Error::buffer_too_small(field_len));
        }
        if self.head.releasable {
            let mut spill = [0_u8; MIN_SPACE_IDLE];
            let (field, spill_len) = self.write_field(out, &mut spill)?;
            self.head.status.fields_out.incr();
            self.reset_field();
            let spilled = spill.get(..spill_len).unwrap_or(&[]);
            self.append_to_zone(spilled);
            self.pull_from_queue()?;
            if self.head.fill > 0 {
                self.head.started = Some(now);
            }
            self.head.releasable = self.head.fill >= self.zone_len();
            self.refresh_occupancy();
            Ok(Some(field))
        } else if self.head.config.idle_policy == IdlePolicy::IdleField
            && !self.continues_a_packet()
        {
            let field = self.write_idle_field(out)?;
            self.head.status.idle_fields.incr();
            self.head.status.fields_out.incr();
            Ok(Some(field))
        } else {
            Ok(None)
        }
    }

    /// Whether the field being assembled starts with the continuation of a packet that began in
    /// an earlier field: the next field of the VC must carry it, so an idle field in between
    /// would split the packet (132.0 §4.2.2.2, §4.1.2.7.6.2; 732.0 §4.2.2.2; 732.1 §4.2.2.1.2; LNK-011, LNK-014).
    const fn continues_a_packet(&self) -> bool {
        self.head.fill > 0
            && match self.head.first_start {
                Some(offset) => offset > 0,
                None => true,
            }
    }

    /// Writes the releasable field to `out`: header, packet zone, and the idle completion; the
    /// octets of an idle packet that did not fit go to `spill` (LNK-013). Changes no state but
    /// the idle counter.
    fn write_field(&mut self, out: &mut [u8], spill: &mut [u8]) -> Result<(DataField, usize)> {
        let field_len = self.head.config.field_len;
        let area = out
            .get_mut(..field_len)
            .ok_or(Error::buffer_too_small(field_len))?;
        let (header, zone) = area
            .split_at_mut_checked(P::PACKET_HEADER_LEN)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let (held, completion) = zone
            .split_at_mut_checked(self.head.fill)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let pointer = match self.head.first_start {
            Some(n) => Pointer::StartsAt(n),
            None if completion.is_empty() => Pointer::NoStart,
            None => Pointer::StartsAt(self.head.fill),
        };
        let raw = P::encode_pointer(&self.head.config.protocol, pointer)?;
        let mut spill_len = 0;
        if !completion.is_empty() {
            spill_len = self.write_idle(completion, spill)?;
            self.head.status.idle_packets.incr();
        }
        for (d, s) in held.iter_mut().zip(&self.bufs.zone) {
            *d = *s;
        }
        P::write_header(&self.head.config.protocol, raw, header);
        Ok((self.data_field(raw), spill_len))
    }

    /// The `DataField` action for the field at the start of the output area.
    fn data_field(&self, raw: u16) -> DataField {
        DataField {
            vc: self.head.config.vc.into(),
            field: OutRange::new(0, self.head.config.field_len),
            pointer: raw,
            route: P::route(&self.head.config.protocol),
        }
    }

    /// Writes idle packets that fill `completion` exactly, or, for a Space Packet remainder
    /// shorter than a Space Packet Idle Packet, as many octets as it holds of one of 7 octets,
    /// whose rest goes to `spill`. Returns the octets spilled (LNK-013).
    fn write_idle(&self, completion: &mut [u8], spill: &mut [u8]) -> Result<usize> {
        let pattern = self.head.config.idle_pattern;
        match self.head.config.idle_packet_kind {
            IdlePacketKind::Encapsulation => {
                aldis_epp::idle(completion.len(), pattern, completion)?;
                Ok(0)
            }
            IdlePacketKind::SpacePacket => {
                let remainder = completion.len();
                let total = remainder.max(MIN_SPACE_IDLE);
                let cfg = aldis_spp::SppConfig::new(total)?;
                let octet = [pattern];
                let idle = aldis_spp::IdlePacket::new(&cfg, total)?.with_pattern(&octet);
                if remainder >= MIN_SPACE_IDLE {
                    idle.encode(completion)?;
                    return Ok(0);
                }
                let mut whole = [0_u8; MIN_SPACE_IDLE];
                idle.encode(&mut whole)?;
                let (now, later) = whole
                    .split_at_checked(remainder)
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                for (d, s) in completion.iter_mut().zip(now) {
                    *d = *s;
                }
                for (d, s) in spill.iter_mut().zip(later) {
                    *d = *s;
                }
                Ok(later.len())
            }
        }
    }

    /// Writes the field of only idle data of the `IdleField` policy to `out` (LNK-014). The
    /// started field and the queue stay as they are.
    fn write_idle_field(&self, out: &mut [u8]) -> Result<DataField> {
        let field_len = self.head.config.field_len;
        let area = out
            .get_mut(..field_len)
            .ok_or(Error::buffer_too_small(field_len))?;
        let (header, zone) = area
            .split_at_mut_checked(P::PACKET_HEADER_LEN)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let pointer = match P::IDLE_FIELD {
            IdleField::Packets => Pointer::StartsAt(0),
            IdleField::Pattern => Pointer::OnlyIdle,
        };
        let raw = P::encode_pointer(&self.head.config.protocol, pointer)?;
        match P::IDLE_FIELD {
            IdleField::Packets => {
                let mut none = [0_u8; MIN_SPACE_IDLE];
                self.write_idle(zone, &mut none)?;
            }
            IdleField::Pattern => zone.fill(self.head.config.idle_pattern),
        }
        P::write_header(&self.head.config.protocol, raw, header);
        Ok(self.data_field(raw))
    }

    /// Starts the next field with the octets of the queue: the continuation of a packet that
    /// began in the previous field, then whole packets, as far as they go (LNK-011).
    fn pull_from_queue(&mut self) -> Result<()> {
        let zone_len = self.zone_len();
        // bound: every pass moves at least one octet from the queue or ends the loop, and the
        // queue holds at most Q octets.
        while self.head.fill < zone_len && self.head.queue_len > 0 {
            let room = zone_len.saturating_sub(self.head.fill);
            let moved = if self.head.tail > 0 {
                let n = self.head.tail.min(room);
                self.head.tail = self.head.tail.saturating_sub(n);
                n
            } else {
                let head = self
                    .bufs
                    .queue
                    .get(..self.head.queue_len)
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                let kind = head
                    .first()
                    .and_then(|&b| packet::kind_of(b))
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                let Length::Total(total) = packet::length(kind, head) else {
                    return Err(Error::new(ErrorKind::InternalError));
                };
                let n = total.min(room);
                self.head.first_start.get_or_insert(self.head.fill);
                self.head.tail = total.saturating_sub(n);
                n
            };
            self.move_queue_to_zone(moved);
        }
        Ok(())
    }

    /// Moves the first `n` octets of the queue (at most the queue holds) to the end of the
    /// packet zone, and closes the gap.
    fn move_queue_to_zone(&mut self, n: usize) {
        let moved = n.min(self.head.queue_len);
        for (d, s) in self
            .bufs
            .zone
            .iter_mut()
            .skip(self.head.fill)
            .zip(self.bufs.queue.iter().take(moved))
        {
            *d = *s;
        }
        self.head.fill = self.head.fill.saturating_add(moved);
        let left = self.head.queue_len.saturating_sub(moved);
        // Close the gap octet by octet: a slice method that moves a range could panic.
        for i in 0..left {
            let octet = self.bufs.queue.get(moved.saturating_add(i)).copied();
            if let (Some(octet), Some(dst)) = (octet, self.bufs.queue.get_mut(i)) {
                *dst = octet;
            }
        }
        self.head.queue_len = left;
    }
}
