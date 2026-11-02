//! The `PacketExtractor`: recovers packets from consecutive data fields (LNK-060…LNK-064,
//! LNK-002, LNK-003, LNK-074, LNK-075; docs/04-engines.md §3.2, §4).
//!
//! The engine serves one VC (one MAP for USLP) of one protocol (TM, AOS, or USLP rule `000`,
//! LNK-001). It takes the data fields the `FrameReceiver` delivers (`DataField`), reassembles the
//! packets that span fields with the First Header Pointer and the length field of each packet
//! (SPP or EPP by its PVN, LNK-060), and returns each complete packet once. After a frame gap,
//! at start-up, and after an extraction error it resynchronises at the next First Header Pointer
//! (LNK-061, LNK-062).
//!
//! **Bounds (LNK-074, LNK-075).** One `DataField` returns at most
//! [`EXTRACTOR_MAX_ACTIONS`] actions, whatever the length of the field: the packets between two
//! events are one `Packets` run. It writes at most `max_packet_len − 1 + z` octets to the
//! output area, `z` being the octets of the packet zone it is given: the packet in progress
//! (at most `max_packet_len − 1` octets) and the zone. An area shorter than that fails the input with `BufferTooSmall`, so that the
//! caller sizes the area once. `Reconfigure` writes nothing.

use aldis_core::engine::{Actions, OutRange};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy, ValidPvns};
use crate::packet::{self, Kind, Length};
use crate::protocol::{PacketProtocol, ParsedField, Pointer};

/// The octets the assembly buffer needs besides the packet: the longest header of a packet
/// (an Encapsulation Packet header of 8 octets, 133.1 §4.1.2) is read before the length is
/// known (LNK-062).
pub const MIN_ASSEMBLY: usize = 8;

/// The most actions one input of a `PacketExtractor` returns (LNK-074, API-057).
///
/// It does not grow with the field: the complete packets of a field that follow each other
/// without an event between them are one [`ExtractorOutput::Packets`] action, a run of
/// self-delimiting packets in the output area. One `DataField` has at most three events and one
/// run: the outcome for the packet in progress and `LengthInconsistent` because the pointer
/// disagrees with it, then the packets from the pointer on as one run, then one error after
/// them. A continuation that ends early emits nothing, so its packet and those after the
/// pointer never make two runs.
pub const EXTRACTOR_MAX_ACTIONS: usize = 4;

/// The actions of one input of a `PacketExtractor` (LNK-074).
pub type ExtractorActions = Actions<ExtractorOutput, EXTRACTOR_MAX_ACTIONS>;

/// The packets of a [`ExtractorOutput::Packets`] run, one by one: each is as long as its own
/// length field says (133.0 §4.1.3.5, 133.1 §4.1.2.8), so the run needs no table of lengths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketRun<'a> {
    rest: &'a [u8],
}

impl<'a> PacketRun<'a> {
    /// The Packet Version Number of a packet of a run, read from its header (API-061, LNK-060).
    #[must_use]
    pub fn pvn(packet: &[u8]) -> Option<u8> {
        packet.first().map(|&b| packet::pvn(b))
    }

    /// The packets in `run`, a range of `area` that a `Packets` action named; `None` if `run`
    /// does not lie inside `area`, which is the wrong area.
    #[must_use]
    pub fn new(run: OutRange, area: &'a [u8]) -> Option<PacketRun<'a>> {
        run.slice(area).map(|rest| PacketRun { rest })
    }
}

impl<'a> Iterator for PacketRun<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let kind = packet::kind_of(*self.rest.first()?)?;
        let Length::Total(total) = packet::length(kind, self.rest) else {
            return None;
        };
        let (one, rest) = self.rest.split_at_checked(total)?;
        self.rest = rest;
        Some(one)
    }
}

/// An extraction error (LNK-061, LNK-062; docs/04-engines.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtractionError {
    /// A packet is longer than the Maximum Packet Length.
    PacketTooLong,
    /// A packet's PVN is not in the valid set.
    InvalidPvn,
    /// The length fields disagree with the First Header Pointer, or a length field is invalid.
    LengthInconsistent,
}

/// The configuration of a `PacketExtractor`: the managed parameters of one VC or MAP that carries
/// Packets (132.0 §5.4 Table 5-4; 732.0 §5.5 Table 5-4; 732.1 §5.6 Table 5-5; LNK-060…LNK-063).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketExtractorConfig<P: PacketProtocol> {
    protocol: P::Params,
    valid_pvns: ValidPvns,
    max_packet_len: usize,
    deliver_incomplete: bool,
    deliver_idle: bool,
}

impl<P: PacketProtocol> PacketExtractorConfig<P> {
    /// The configuration for packets of the PVNs `valid_pvns` of at most `max_packet_len`
    /// octets. It discards partial packets and drops idle packets (LNK-061, LNK-063).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_packet_len`) for 0.
    pub const fn new(
        protocol: P::Params,
        valid_pvns: ValidPvns,
        max_packet_len: usize,
    ) -> Result<PacketExtractorConfig<P>> {
        if max_packet_len == 0 {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_PACKET_LEN));
        }
        Ok(PacketExtractorConfig {
            protocol,
            valid_pvns,
            max_packet_len,
            deliver_incomplete: false,
            deliver_idle: false,
        })
    }

    /// The same configuration with "deliver incomplete packets" (the Packet Quality Indicator,
    /// 132.0 §5.4 Table 5-4; LNK-061) and the delivery of idle packets for diagnostics
    /// (LNK-063).
    #[must_use]
    pub const fn with_delivery(
        self,
        deliver_incomplete: bool,
        deliver_idle: bool,
    ) -> PacketExtractorConfig<P> {
        PacketExtractorConfig {
            deliver_incomplete,
            deliver_idle,
            ..self
        }
    }

    /// The valid Packet Version Numbers (LNK-062).
    #[must_use]
    pub const fn valid_pvns(&self) -> ValidPvns {
        self.valid_pvns
    }

    /// The Maximum Packet Length (LNK-062).
    #[must_use]
    pub const fn max_packet_len(&self) -> usize {
        self.max_packet_len
    }

    /// Whether partial packets are delivered flagged incomplete (LNK-061).
    #[must_use]
    pub const fn deliver_incomplete(&self) -> bool {
        self.deliver_incomplete
    }

    /// Whether idle packets are delivered (LNK-063).
    #[must_use]
    pub const fn deliver_idle(&self) -> bool {
        self.deliver_idle
    }

    /// The octets of the shortest packet the extractor delivers: 1 when idle packets are
    /// delivered, else 3 where Encapsulation Packets are valid (a 2-octet header and one data
    /// octet), else 7 for Space Packets.
    #[must_use]
    pub const fn min_packet_len(&self) -> usize {
        if self.deliver_idle {
            1
        } else if self.valid_pvns.encapsulation() {
            3
        } else {
            7
        }
    }

    /// Whether a packet whose first octet is `first` is valid: its PVN is in the set, or it is
    /// an Encapsulation Idle Packet on a protocol that delimits one whatever the set
    /// (LNK-062, LNK-063).
    fn accepts(&self, first: u8) -> Option<Kind> {
        let kind = packet::kind_of(first)?;
        let valid = match kind {
            Kind::Space => self.valid_pvns.space(),
            Kind::Encapsulation => {
                self.valid_pvns.encapsulation()
                    || (P::DELIMITS_IDLE_EIP && packet::is_idle(kind, &[first]))
            }
        };
        valid.then_some(kind)
    }
}

/// A data field for the extractor, as the `FrameReceiver` delivers it (docs/04-engines.md §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldIn<'a> {
    /// The data field: the TM data field, the AOS `M_PDU` (header and Packet Zone), or the USLP
    /// TFDF (header and TFDZ).
    pub field: &'a [u8],
    /// The First Header Pointer of the TM frame header; AOS and USLP carry it in `field`.
    pub header_pointer: Option<u16>,
    /// A frame gap was flagged on this field (LNK-051, LNK-061).
    pub gap: bool,
}

/// An input of the `PacketExtractor` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractorInput<'a, P: PacketProtocol> {
    /// A data field of the VC (LNK-060).
    DataField(FieldIn<'a>),
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(PacketExtractorConfig<P>),
}

/// An action of the `PacketExtractor` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractorOutput {
    /// Complete packets, one after the other in the output area, in the order of the field
    /// (LNK-060). [`PacketRun`] splits them.
    Packets {
        /// Where the run is in the output area.
        run: OutRange,
        /// How many packets it holds.
        count: usize,
    },
    /// A partial packet flagged incomplete, the Packet Quality Indicator (LNK-061).
    IncompletePacket {
        /// The Packet Version Number in the header: 0 (Space Packet) or 7 (Encapsulation).
        pvn: u8,
        /// Where the packet is in the output area.
        packet: OutRange,
    },
    /// The partial packet was discarded: this many octets (LNK-061, LNK-003).
    Discarded {
        /// The octets of the partial packet.
        octets: usize,
    },
    /// An extraction error (LNK-061, LNK-062).
    Error(ExtractionError),
}

/// The read-only status of a `PacketExtractor`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExtractorStatus {
    /// Data fields taken.
    pub fields_in: Counter,
    /// Packets returned, incomplete ones included.
    pub packets_out: Counter,
    /// Packets returned flagged incomplete.
    pub incomplete_out: Counter,
    /// Idle packets seen, whether delivered or dropped (LNK-063).
    pub idle_packets: Counter,
    /// Octets of partial packets discarded (LNK-061).
    pub discarded_octets: Counter,
    /// Fields flagged with a frame gap (LNK-051).
    pub gaps: Counter,
    /// Extraction errors `PacketTooLong` (LNK-062).
    pub packet_too_long: Counter,
    /// Extraction errors `InvalidPvn` (LNK-062).
    pub invalid_pvn: Counter,
    /// Extraction errors `LengthInconsistent` (LNK-061).
    pub length_inconsistent: Counter,
    /// The assembly buffer: the octets of the packet in progress (API-060).
    pub assembly: Occupancy,
}

/// Where a run of octets ended (see [`PacketExtractor::run`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    /// The octets were taken.
    Complete,
    /// An extraction error was raised; the rest of the octets is discarded.
    Stopped,
    /// A packet ended before the last octet of a run that must end with a packet or inside one.
    Early,
}

/// What a run of octets must do at the end of a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Packets follow each other to the end of the octets.
    Packets,
    /// The octets are the continuation of the packet in progress, to the First Header Pointer
    /// or the end of a field in which no packet starts.
    Continuation,
}

/// The result of looking at the octets of the packet in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    More,
    Done,
    Fail(ExtractionError),
}

/// The output area and the actions of one input.
struct Cx<'a> {
    out: &'a mut [u8],
    used: usize,
    actions: ExtractorActions,
    run_start: usize,
    run_count: usize,
}

impl Cx<'_> {
    /// Copies `octets` to the next free place of the output area.
    fn write(&mut self, octets: &[u8]) -> Result<OutRange> {
        let start = self.used;
        let end = start.saturating_add(octets.len());
        let dst = self
            .out
            .get_mut(start..end)
            .ok_or(Error::buffer_too_small(end))?;
        for (d, s) in dst.iter_mut().zip(octets) {
            *d = *s;
        }
        self.used = end;
        Ok(OutRange::new(start, octets.len()))
    }

    /// Adds a complete packet to the run in progress.
    fn deliver(&mut self, octets: &[u8]) -> Result<()> {
        let range = self.write(octets)?;
        if self.run_count == 0 {
            self.run_start = range.start();
        }
        self.run_count = self.run_count.saturating_add(1);
        Ok(())
    }

    /// Ends the run in progress with its action.
    fn close_run(&mut self) -> Result<()> {
        if self.run_count > 0 {
            let run = OutRange::new(self.run_start, self.used.saturating_sub(self.run_start));
            self.actions.push(ExtractorOutput::Packets {
                run,
                count: self.run_count,
            })?;
            self.run_count = 0;
        }
        Ok(())
    }

    /// Adds an action after the run in progress.
    fn push(&mut self, action: ExtractorOutput) -> Result<()> {
        self.close_run()?;
        self.actions.push(action)
    }
}

/// The `PacketExtractor` of one VC (LNK-060…LNK-064).
///
/// `M` is the capacity of the assembly buffer in octets, which is the longest packet it can
/// assemble (at least [`MIN_ASSEMBLY`]); and `D` the longest data field it is given,
/// header included (GEN-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketExtractor<P: PacketProtocol, const M: usize, const D: usize> {
    config: PacketExtractorConfig<P>,
    buf: [u8; M],
    fill: usize,
    total: usize,
    sync: bool,
    status: ExtractorStatus,
}

impl<P: PacketProtocol, const M: usize, const D: usize> PacketExtractor<P, M, D> {
    /// An extractor that has not yet synchronised: it skips the octets before the first pointer
    /// (LNK-061).
    ///
    /// # Errors
    /// `CapacityExceeded` (`max_packet_len`) if `M` is below [`MIN_ASSEMBLY`] or below the
    /// Maximum Packet Length (GEN-011).
    pub fn new(config: PacketExtractorConfig<P>) -> Result<PacketExtractor<P, M, D>> {
        PacketExtractor::<P, M, D>::check_capacity(&config)?;
        Ok(PacketExtractor {
            config,
            buf: [0; M],
            fill: 0,
            total: 0,
            sync: false,
            status: ExtractorStatus::default(),
        })
    }

    const fn check_capacity(config: &PacketExtractorConfig<P>) -> Result<()> {
        if M < MIN_ASSEMBLY || config.max_packet_len > M {
            Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAX_PACKET_LEN))
        } else {
            Ok(())
        }
    }

    /// The configuration in force.
    #[must_use]
    pub const fn config(&self) -> &PacketExtractorConfig<P> {
        &self.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &ExtractorStatus {
        &self.status
    }

    /// Runs one input (API-057). The extractor has no timer (API-053).
    ///
    /// An input that fails leaves the engine as it was and writes no action (API-058).
    ///
    /// # Errors
    /// - `DataField`: the codec errors of the data field (`P::parse`): `InvalidField`
    ///   (`first_header_pointer`) for a TM field without a pointer, `Malformed`
    ///   (`first_header_pointer` or `fhp_lvo_pointer`) for a pointer outside the zone, and the
    ///   header errors of AOS and USLP; `CapacityExceeded` (`field_len`) for a zone longer
    ///   than `D`; `BufferTooSmall` if `out` is shorter than `max_packet_len − 1` plus the
    ///   zone (LNK-075).
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(
        &mut self,
        input: ExtractorInput<'_, P>,
        out: &mut [u8],
    ) -> Result<ExtractorActions> {
        match input {
            ExtractorInput::DataField(f) => self.data_field(f, out),
            ExtractorInput::Reconfigure(config) => {
                PacketExtractor::<P, M, D>::check_capacity(&config)?;
                let mut actions = Actions::new();
                let octets = self.fill;
                self.config = config;
                self.clear();
                self.sync = false;
                if octets > 0 {
                    self.status.discarded_octets.add(octets);
                    actions.push(ExtractorOutput::Discarded { octets })?;
                }
                Ok(actions)
            }
        }
    }

    const fn clear(&mut self) {
        self.fill = 0;
        self.total = 0;
        self.status.assembly.set(0);
    }

    /// `DataField` (LNK-060…LNK-064).
    fn data_field(&mut self, input: FieldIn<'_>, out: &mut [u8]) -> Result<ExtractorActions> {
        let ParsedField { pointer, zone } =
            P::parse(&self.config.protocol, input.header_pointer, input.field)?;
        if input.field.len() > D {
            return Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::FIELD_LEN));
        }
        let needed = self
            .config
            .max_packet_len
            .saturating_sub(1)
            .saturating_add(zone.len());
        if out.len() < needed {
            return Err(Error::buffer_too_small(needed));
        }
        let mut cx = Cx {
            out,
            used: 0,
            actions: Actions::new(),
            run_start: 0,
            run_count: 0,
        };
        self.status.fields_in.incr();
        if input.gap {
            self.status.gaps.incr();
            self.abandon(&mut cx)?;
            self.sync = false;
        }
        self.place(pointer, zone, &mut cx)?;
        cx.close_run()?;
        Ok(cx.actions)
    }

    /// Takes the zone of a field by its pointer (LNK-060, LNK-061, LNK-064).
    fn place(&mut self, pointer: Pointer, zone: &[u8], cx: &mut Cx<'_>) -> Result<()> {
        match pointer {
            // LNK-064: the field is ignored and the packet in progress is kept.
            Pointer::OnlyIdle => Ok(()),
            Pointer::NoStart if !self.sync => Ok(()),
            Pointer::NoStart if self.fill == 0 => {
                // Nothing is in progress, so the next octet must start a packet, but the field
                // says that none starts: the pointer is correct (LNK-060).
                self.raise(ExtractionError::LengthInconsistent, cx)?;
                self.sync = false;
                Ok(())
            }
            Pointer::NoStart => {
                let run = self.run(zone, Mode::Continuation, cx)?;
                // A packet that ends inside a field in which none starts disagrees with the
                // pointer (LNK-060, OI-78).
                if run == Run::Early {
                    self.raise(ExtractionError::LengthInconsistent, cx)?;
                }
                self.sync = run == Run::Complete;
                Ok(())
            }
            Pointer::StartsAt(offset) => {
                let (before, from) = zone
                    .split_at_checked(offset)
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                if self.sync {
                    self.continue_to_pointer(before, cx)?;
                }
                self.sync = self.run(from, Mode::Packets, cx)? != Run::Stopped;
                Ok(())
            }
        }
    }

    /// The octets before the First Header Pointer belong to the packet in progress; the pointer
    /// is correct where they disagree with the length fields (LNK-060, LNK-061).
    fn continue_to_pointer(&mut self, before: &[u8], cx: &mut Cx<'_>) -> Result<()> {
        if self.fill == 0 {
            if !before.is_empty() {
                self.raise(ExtractionError::LengthInconsistent, cx)?;
            }
            return Ok(());
        }
        match self.run(before, Mode::Continuation, cx)? {
            Run::Early => self.raise(ExtractionError::LengthInconsistent, cx),
            Run::Complete if self.fill > 0 => {
                self.abandon(cx)?;
                self.raise(ExtractionError::LengthInconsistent, cx)
            }
            Run::Stopped | Run::Complete => Ok(()),
        }
    }

    /// Feeds `octets` to the packet in progress and, in [`Mode::Packets`], to the packets that
    /// follow it. [`Run::Stopped`] says an extraction error ended the run (LNK-062).
    fn run(&mut self, octets: &[u8], mode: Mode, cx: &mut Cx<'_>) -> Result<Run> {
        let mut rest = octets;
        // bound: every pass takes at least one octet of `rest` or returns.
        while !rest.is_empty() {
            let want = if self.total > 0 {
                self.total.saturating_sub(self.fill).max(1)
            } else {
                1
            };
            let (take, after) = rest
                .split_at_checked(want.min(rest.len()))
                .ok_or(Error::new(ErrorKind::InternalError))?;
            self.push(take)?;
            rest = after;
            match self.look() {
                Look::More => {}
                Look::Fail(kind) => {
                    self.clear();
                    self.raise(kind, cx)?;
                    return Ok(Run::Stopped);
                }
                Look::Done => {
                    if mode == Mode::Continuation && !rest.is_empty() {
                        self.clear();
                        return Ok(Run::Early);
                    }
                    self.emit(cx)?;
                    self.clear();
                }
            }
        }
        Ok(Run::Complete)
    }

    /// Appends octets to the packet in progress.
    fn push(&mut self, octets: &[u8]) -> Result<()> {
        let end = self.fill.saturating_add(octets.len());
        let dst = self
            .buf
            .get_mut(self.fill..end)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        for (d, s) in dst.iter_mut().zip(octets) {
            *d = *s;
        }
        self.fill = end;
        self.status.assembly.set(end);
        Ok(())
    }

    /// What the octets of the packet in progress say: its PVN as soon as the first octet is
    /// there, its length as soon as the length field is complete (LNK-062).
    fn look(&mut self) -> Look {
        let held = self.buf.get(..self.fill).unwrap_or(&[]);
        let Some(&first) = held.first() else {
            return Look::More;
        };
        let Some(kind) = self.config.accepts(first) else {
            return Look::Fail(ExtractionError::InvalidPvn);
        };
        if self.total == 0 {
            match packet::length(kind, held) {
                Length::More => return Look::More,
                Length::Invalid => return Look::Fail(ExtractionError::LengthInconsistent),
                Length::Total(total) if total > self.config.max_packet_len => {
                    return Look::Fail(ExtractionError::PacketTooLong);
                }
                Length::Total(total) => self.total = total,
            }
        }
        if self.fill >= self.total {
            Look::Done
        } else {
            Look::More
        }
    }

    /// Returns the complete packet in progress, unless it is an idle packet to drop
    /// (LNK-063).
    fn emit(&mut self, cx: &mut Cx<'_>) -> Result<()> {
        let held = self.buf.get(..self.fill).unwrap_or(&[]);
        let Some(&first) = held.first() else {
            return Ok(());
        };
        let kind = packet::kind_of(first).ok_or(Error::new(ErrorKind::InternalError))?;
        if packet::is_idle(kind, held) {
            self.status.idle_packets.incr();
            if !self.config.deliver_idle {
                return Ok(());
            }
        }
        cx.deliver(held)?;
        self.status.packets_out.incr();
        Ok(())
    }

    /// The outcome for the packet in progress that cannot be completed: it is delivered flagged
    /// incomplete if so configured and its header says what it is, else discarded (LNK-061).
    fn abandon(&mut self, cx: &mut Cx<'_>) -> Result<()> {
        if self.fill == 0 {
            return Ok(());
        }
        let held = self.buf.get(..self.fill).unwrap_or(&[]);
        let first = held.first().copied().unwrap_or(0);
        let idle = packet::kind_of(first).is_some_and(|k| packet::is_idle(k, held));
        if self.config.deliver_incomplete && self.total > 0 && !idle {
            cx.close_run()?;
            let range = cx.write(held)?;
            cx.actions.push(ExtractorOutput::IncompletePacket {
                pvn: packet::pvn(first),
                packet: range,
            })?;
            self.status.packets_out.incr();
            self.status.incomplete_out.incr();
        } else {
            let octets = self.fill;
            self.status.discarded_octets.add(octets);
            cx.push(ExtractorOutput::Discarded { octets })?;
        }
        self.clear();
        Ok(())
    }

    /// Counts and returns an extraction error.
    fn raise(&mut self, kind: ExtractionError, cx: &mut Cx<'_>) -> Result<()> {
        match kind {
            ExtractionError::PacketTooLong => self.status.packet_too_long.incr(),
            ExtractionError::InvalidPvn => self.status.invalid_pvn.incr(),
            ExtractionError::LengthInconsistent => self.status.length_inconsistent.incr(),
        }
        cx.push(ExtractorOutput::Error(kind))
    }
}
