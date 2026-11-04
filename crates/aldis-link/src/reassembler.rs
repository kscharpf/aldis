//! The `MapReassembler`: rebuilds SDUs from FDUs and TFDFs (LNK-070…LNK-073, LNK-002, LNK-003,
//! LNK-074, LNK-075; docs/04-engines.md §3.3, §4).
//!
//! It rebuilds packets, `MAP_SDUs`, and octet streams from the Frame Data Units of a TC Virtual
//! Channel or the TFDFs of a USLP Virtual Channel.
//!
//! The engine takes the FDUs the `FrameReceiver` delivers (`Fdu`), reads the Segment Header (TC)
//! or the TFDF Header (USLP), and reassembles each MAP separately (232.0 §4.4.1–3, 732.1
//! §4.3.2–4): a MAP has its own SDU in progress, so FDUs of different MAPs of a VC may be
//! interleaved. A segmented SDU is delivered once, when its last segment arrives; a whole SDU at
//! once; the packets of a blocked FDU as one run (the same run as the `PacketExtractor`'s,
//! [`PacketRun`](crate::extractor::PacketRun) walks it); the chunk of an octet stream as it
//! arrives.
//!
//! **Sequence (LNK-070, OI-72, OI-73).** A continuing or last segment without a first segment is
//! a `SequenceViolation` and delivers nothing. A first segment, a whole SDU, or the start of a
//! fixed-length SDU (rule `001`) while an SDU of the MAP is in progress discards the partial SDU
//! with a `SequenceViolation` event, and the new segment then begins an SDU (a whole SDU is
//! delivered). A TFDF for a MAP that is not configured is discarded with `UnknownMap`.
//!
//! **Gaps (LNK-051, LNK-070; OI-78).** `Fdu` on USLP says whether a gap was flagged on the field.
//! The VCF Count is the count of the VC, so a gap may have taken a TFDF of any MAP: it discards
//! the SDUs in progress of every MAP of that VC, with one `GapDiscard` event, which names the MAP
//! of the field if an SDU of it was in progress, else the first MAP that had one. TC has no gap
//! input: FARM-1 decides (LNK-051). An octet stream has no SDU in progress; the chunk that follows
//! a gap says so (`after_gap`).
//!
//! **Maximum SDU size (LNK-073).** Every MAP that assembles has a maximum SDU length, which the
//! engine's capacity `S` bounds. An SDU that would exceed it is discarded with an `SduTooLong`
//! event, partial or whole, and nothing is delivered.
//!
//! **Refused input.** An FDU that cannot be read is refused with no state change (API-058): a
//! TFDZ construction rule that the MAP's service does not use, a Last Valid Octet Pointer beyond
//! the TFDZ, packets that do not tile a blocked FDU or a reassembled packet that disagrees with its
//! own length field, a PVN other than 0 and 7.
//!
//! **Bounds (LNK-074, LNK-075).** One `Fdu` returns at most [`MAP_REASSEMBLER_MAX_ACTIONS`]
//! actions: the events `GapDiscard` and `SequenceViolation` or `SduTooLong`, and the delivery,
//! never all three (see the constant). It writes at most `max_sdu_len − 1 + z` octets to the
//! output area, `z` being the octets of the user data of the FDU: the SDU in progress and the data
//! of this FDU; an area shorter than that fails the input with `BufferTooSmall`. `Reconfigure`
//! writes nothing.

use core::fmt::Debug;

use aldis_core::engine::{Actions, OutRange};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy};
use crate::packet::{self, Length};
use crate::protocol::Protocol;

/// The most actions one input of the `MapReassembler` returns (LNK-074, API-057).
///
/// An input has at most two events, and a delivery only after a `SequenceViolation`:
/// - `GapDiscard` discards every SDU in progress of the VC, so what follows meets an empty MAP:
///   a continuing or last segment is a `SequenceViolation` (2 actions), a first segment may be
///   too long (`SduTooLong`, 2 actions), a whole SDU or the start of a fixed-length SDU is
///   delivered (2 actions);
/// - without a gap, a first segment or a whole SDU while an SDU is in progress is a
///   `SequenceViolation` and then a `SduTooLong` or a delivery (2 actions);
/// - a continuing segment is a `SequenceViolation` or an `SduTooLong`, never both (1 action).
pub const MAP_REASSEMBLER_MAX_ACTIONS: usize = 2;

/// The actions of one input of a `MapReassembler` (LNK-074).
pub type ReassemblerActions = Actions<ReassemblerOutput, MAP_REASSEMBLER_MAX_ACTIONS>;

/// The raw identifier.
fn raw<I: Into<u8>>(id: I) -> u8 {
    id.into()
}

/// What a MAP carries and so how its SDUs are delivered (232.0 §4.4.1–2; 732.1 §4.3.2–4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    /// Packets: whole packets are split by their length fields (LNK-071), a segmented packet is
    /// delivered once.
    Packets,
    /// `MAP_SDUs` or `MAPA_SDUs` (and the `VCA_SDU` of a TC VC without a Segment Header).
    Sdu,
    /// An octet stream (USLP rule `011`), delivered as it arrives (LNK-072).
    OctetStream,
}

/// How an FDU relates to its SDU, whichever rule or flags said so (232.0 Table 4-2; 732.1
/// Table 4-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit<'a> {
    /// Flags `11`, rule `111`, or a fixed-length SDU that starts and ends in the TFDZ (rules `001`
    /// with a pointer): the user data of one whole SDU, or several whole packets.
    Whole(&'a [u8]),
    /// Flags `01`, rule `100`, or rule `001` with the pointer "all ones": the first portion.
    First(&'a [u8]),
    /// Flags `00`, rule `101`, or rule `010` with the pointer "all ones": a continuing portion.
    Continuing(&'a [u8]),
    /// Flags `10`, rule `110`, or rule `010` with a pointer: the last portion.
    Last(&'a [u8]),
    /// Rule `011`: octets of a stream.
    Stream(&'a [u8]),
    /// A TFDF of only idle data (UPID "Only Idle Data"), which carries nothing (732.1
    /// §4.3.2.2.5 note 1).
    Idle,
}

/// An FDU or TFDF read by the protocol (LNK-070).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parsed<'a> {
    /// The MAP Identifier: from the Segment Header (TC), or given with the frame (USLP).
    pub map: u8,
    /// A gap was flagged on the field (LNK-051); always false on TC.
    pub gap: bool,
    /// Whether the user data is a fixed-length TFDZ (rules `001` and `010`), which only
    /// `MAPA_SDUs` use (732.1 §4.1.4.2.2.2.2–3).
    pub fixed_length: bool,
    /// The user data and its place in the SDU.
    pub unit: Unit<'a>,
}

/// A frame protocol whose VCs the `MapReassembler` serves (LNK-001): TC and USLP.
pub trait ReassemblerProtocol: Protocol {
    /// The MAP Identifier of the protocol: 6 bits (TC Segment Header), 4 bits (USLP).
    type Map: Copy + Debug + Eq + TryFrom<u8, Error = Error> + Into<u8>;
    /// The parameters of the VCs that the protocol has and the others lack.
    type Channel: Copy + Debug + Eq;
    /// What the caller adds to the data of an FDU: nothing on TC, whose Segment Header names the
    /// MAP; the MAP and the gap flag on USLP, which the frame header and the receiver give.
    type Meta: Copy + Debug + Eq;

    /// Reads an FDU or TFDF.
    ///
    /// # Errors
    /// `InvalidField` (`frame_data_field`) for no user data, (`tfdz_construction_rule`) for rule
    /// `000`; `Malformed` (`fhp_lvo_pointer`) for a Last Valid Octet Pointer beyond the TFDZ.
    fn parse<'a>(channel: &Self::Channel, meta: &Self::Meta, data: &'a [u8]) -> Result<Parsed<'a>>;
}

/// One MAP of a `MapReassembler` (232.0 §5.4 Table 5-4; 732.1 §5.5 Table 5-4; LNK-070, LNK-073).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReassemblerMap<P: ReassemblerProtocol> {
    vc: P::Vc,
    id: P::Map,
    service: Service,
    max_sdu_len: usize,
}

impl<P: ReassemblerProtocol> ReassemblerMap<P> {
    /// A packet MAP whose segmented packets are at most `max_sdu_len` octets (the Maximum Packet
    /// Length, 132.0-like Table 5-4) (LNK-073).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_sdu_len`) for 0.
    pub const fn packets(vc: P::Vc, id: P::Map, max_sdu_len: usize) -> Result<ReassemblerMap<P>> {
        ReassemblerMap::checked(vc, id, Service::Packets, max_sdu_len)
    }

    /// A MAP of SDUs of at most `max_sdu_len` octets, the maximum `MAP_SDU` length (232.0 Table
    /// 5-4; 732.1 Table 5-4) (LNK-073).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_sdu_len`) for 0.
    pub const fn sdu(vc: P::Vc, id: P::Map, max_sdu_len: usize) -> Result<ReassemblerMap<P>> {
        ReassemblerMap::checked(vc, id, Service::Sdu, max_sdu_len)
    }

    /// An octet-stream MAP: no SDU is assembled, so it has no maximum.
    #[must_use]
    pub const fn octet_stream(vc: P::Vc, id: P::Map) -> ReassemblerMap<P> {
        ReassemblerMap {
            vc,
            id,
            service: Service::OctetStream,
            max_sdu_len: 0,
        }
    }

    const fn checked(
        vc: P::Vc,
        id: P::Map,
        service: Service,
        max_sdu_len: usize,
    ) -> Result<ReassemblerMap<P>> {
        if max_sdu_len == 0 {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_SDU_LEN))
        } else {
            Ok(ReassemblerMap {
                vc,
                id,
                service,
                max_sdu_len,
            })
        }
    }

    /// The VC of the MAP.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
    }

    /// The MAP Identifier.
    #[must_use]
    pub const fn id(&self) -> P::Map {
        self.id
    }

    /// What the MAP carries.
    #[must_use]
    pub const fn service(&self) -> Service {
        self.service
    }

    /// The maximum SDU length; `None` for an octet stream.
    #[must_use]
    pub const fn max_sdu_len(&self) -> Option<usize> {
        match self.service {
            Service::OctetStream => None,
            Service::Packets | Service::Sdu => Some(self.max_sdu_len),
        }
    }
}

/// The configuration of a `MapReassembler` for up to `M` MAPs (LNK-070, LNK-073).
///
/// Built only by [`new`](Self::new), which checks the relations between parameters (API-040,
/// RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapReassemblerConfig<P: ReassemblerProtocol, const M: usize> {
    channel: P::Channel,
    maps: [Option<ReassemblerMap<P>>; M],
}

impl<P: ReassemblerProtocol, const M: usize> MapReassemblerConfig<P, M> {
    /// The configuration of the MAPs `maps`, of the VCs `channel` describes.
    ///
    /// # Errors
    /// `InvalidConfig` (`map_id`) for no MAP and for two MAPs of one VC with one Identifier;
    /// `CapacityExceeded` (`map_id`) for more than `M` MAPs.
    pub fn new(
        channel: P::Channel,
        maps: &[ReassemblerMap<P>],
    ) -> Result<MapReassemblerConfig<P, M>> {
        let bad = Error::new(ErrorKind::InvalidConfig).with_field(field::MAP_ID);
        if maps.is_empty() {
            return Err(bad);
        }
        if maps.len() > M {
            return Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID));
        }
        let mut table = [None; M];
        for (i, (slot, map)) in table.iter_mut().zip(maps).enumerate() {
            let same = |m: &ReassemblerMap<P>| raw(m.vc) == raw(map.vc) && raw(m.id) == raw(map.id);
            if maps.iter().take(i).any(same) {
                return Err(bad);
            }
            *slot = Some(*map);
        }
        Ok(MapReassemblerConfig {
            channel,
            maps: table,
        })
    }

    /// The parameters of the VCs that only this protocol has.
    #[must_use]
    pub const fn channel(&self) -> &P::Channel {
        &self.channel
    }

    /// The MAPs, in the order of configuration.
    pub fn maps(&self) -> impl Iterator<Item = &ReassemblerMap<P>> {
        self.maps.iter().flatten()
    }

    /// The index of the MAP `id` of the VC `vc`.
    fn find(&self, vc: u8, id: u8) -> Option<usize> {
        self.maps
            .iter()
            .position(|m| m.is_some_and(|m| raw(m.vc) == vc && raw(m.id) == id))
    }
}

/// An FDU or TFDF for the reassembler, as the `FrameReceiver` delivers it (docs/04-engines.md
/// §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FduIn<'a, P: ReassemblerProtocol> {
    /// The VC of the frame.
    pub vc: P::Vc,
    /// The MAP and the gap flag on USLP; nothing on TC.
    pub meta: P::Meta,
    /// TC: the Frame Data Field with its Segment Header (if the VC has one). USLP: the TFDF, the
    /// TFDF Header and the TFDZ.
    pub data: &'a [u8],
}

/// An input of the `MapReassembler` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReassemblerInput<'a, P: ReassemblerProtocol, const M: usize> {
    /// An FDU or TFDF of a VC (LNK-070).
    Fdu(FduIn<'a, P>),
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(MapReassemblerConfig<P, M>),
}

/// A reassembly event (LNK-070; docs/04-engines.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReassemblyEvent {
    /// The SDUs in progress were discarded because of a gap in the VCF Count.
    GapDiscard,
    /// A segment broke the sequence of the flags or rules; a partial SDU was discarded.
    SequenceViolation,
    /// An FDU for a MAP that is not configured was discarded.
    UnknownMap,
    /// An SDU would exceed the maximum SDU length of its MAP and was discarded.
    SduTooLong,
}

/// An action of the `MapReassembler` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReassemblerOutput {
    /// A reassembly event (LNK-070).
    Event {
        /// What happened.
        event: ReassemblyEvent,
        /// The VC of the FDU.
        vc: u8,
        /// The MAP the event concerns.
        map: u8,
    },
    /// An SDU, whole, in the output area (LNK-070).
    Sdu {
        /// The VC.
        vc: u8,
        /// The MAP.
        map: u8,
        /// Where the SDU is in the output area.
        sdu: OutRange,
    },
    /// Complete packets, one after the other in the output area, in the order of the FDU
    /// (LNK-071); [`PacketRun`](crate::extractor::PacketRun) splits them, and the PVN and the
    /// length of each come from its own header (API-061).
    Packets {
        /// The VC.
        vc: u8,
        /// The MAP.
        map: u8,
        /// Where the run is in the output area.
        run: OutRange,
        /// How many packets it holds.
        count: usize,
    },
    /// Octets of a stream, as they arrived (LNK-072).
    StreamData {
        /// The VC.
        vc: u8,
        /// The MAP.
        map: u8,
        /// Where the octets are in the output area.
        data: OutRange,
        /// A gap was flagged on the field that carried them.
        after_gap: bool,
    },
    /// `Reconfigure` discarded the SDUs in progress: this many octets (LNK-003).
    Discarded {
        /// The octets of partial SDUs.
        octets: usize,
    },
}

/// The read-only status of a `MapReassembler`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReassemblerStatus {
    /// FDUs taken.
    pub fdus_in: Counter,
    /// SDUs and packets delivered (a run counts each packet).
    pub delivered: Counter,
    /// FDUs refused as unreadable (API-058).
    pub refused: Counter,
    /// Events `GapDiscard` (LNK-070).
    pub gap_discards: Counter,
    /// Events `SequenceViolation` (LNK-070).
    pub sequence_violations: Counter,
    /// Events `UnknownMap` (LNK-070).
    pub unknown_maps: Counter,
    /// Events `SduTooLong` (LNK-073).
    pub sdus_too_long: Counter,
    /// Octets of partial SDUs discarded by an event or `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The octets of the SDUs in progress (API-060).
    pub assembly: Occupancy,
}

/// The SDU in progress of a MAP (RS-021: the segment flags passed in decide the next state, so a closed enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Assembly {
    /// No SDU is in progress.
    Idle,
    /// An SDU has begun, and `len` octets are in the buffer of the MAP.
    Building { len: usize },
}

impl Assembly {
    const fn held(self) -> usize {
        match self {
            Assembly::Idle => 0,
            Assembly::Building { len } => len,
        }
    }
}

/// The output area and the actions of one input.
struct Cx<'a> {
    out: &'a mut [u8],
    actions: ReassemblerActions,
}

impl Cx<'_> {
    /// Copies `head` then `tail` to the start of the output area; returns their range.
    fn write(&mut self, head: &[u8], tail: &[u8]) -> Result<OutRange> {
        let total = head.len().saturating_add(tail.len());
        let dst = self
            .out
            .get_mut(..total)
            .ok_or(Error::buffer_too_small(total))?;
        for (d, s) in dst.iter_mut().zip(head.iter().chain(tail)) {
            *d = *s;
        }
        Ok(OutRange::new(0, total))
    }
}

/// The `MapReassembler` (LNK-070…LNK-073).
///
/// `M` is the number of MAPs and `S` the capacity of the assembly buffer of each, which is the
/// longest SDU it can assemble (LNK-073, GEN-011); a configuration with a longer maximum SDU
/// length is refused with `CapacityExceeded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapReassembler<P: ReassemblerProtocol, const M: usize, const S: usize> {
    config: MapReassemblerConfig<P, M>,
    bufs: [[u8; S]; M],
    slots: [Assembly; M],
    status: ReassemblerStatus,
}

impl<P: ReassemblerProtocol, const M: usize, const S: usize> MapReassembler<P, M, S> {
    /// A reassembler with no SDU in progress.
    ///
    /// # Errors
    /// `CapacityExceeded` (`max_sdu_len`) if a MAP's maximum SDU length exceeds `S` (LNK-073).
    pub fn new(config: MapReassemblerConfig<P, M>) -> Result<MapReassembler<P, M, S>> {
        MapReassembler::<P, M, S>::check_capacity(&config)?;
        Ok(MapReassembler {
            config,
            bufs: [[0; S]; M],
            slots: [Assembly::Idle; M],
            status: ReassemblerStatus::default(),
        })
    }

    fn check_capacity(config: &MapReassemblerConfig<P, M>) -> Result<()> {
        if config
            .maps()
            .any(|m| m.max_sdu_len().is_some_and(|max| max > S))
        {
            Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAX_SDU_LEN))
        } else {
            Ok(())
        }
    }

    /// The configuration in force.
    #[must_use]
    pub const fn config(&self) -> &MapReassemblerConfig<P, M> {
        &self.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &ReassemblerStatus {
        &self.status
    }

    /// Runs one input (API-057). The reassembler has no timer (API-053).
    ///
    /// An input that fails leaves the engine as it was and writes no action (API-058).
    ///
    /// # Errors
    /// - `Fdu`: the errors of the protocol's reading ([`ReassemblerProtocol::parse`]);
    ///   `InvalidField` (`tfdz_construction_rule`) for a rule that the service of the MAP does
    ///   not use (rule `011` on a MAP that is not a stream, rules `001` and `010` on a MAP that
    ///   is not `MAPA_SDUs`, a segment or a whole SDU on a stream); `InvalidField`
    ///   (`packet_version`) for a packet of a PVN other than 0 and 7; `LengthMismatch`
    ///   (`packet_len`) if the length fields do not tile the packets of a blocked FDU or do not
    ///   give the length of a segmented packet; `BufferTooSmall` if `out` is shorter than
    ///   `max_sdu_len − 1` plus the user data (LNK-075).
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(
        &mut self,
        input: ReassemblerInput<'_, P, M>,
        out: &mut [u8],
    ) -> Result<ReassemblerActions> {
        match input {
            ReassemblerInput::Fdu(fdu) => self.fdu(&fdu, out),
            ReassemblerInput::Reconfigure(config) => {
                MapReassembler::<P, M, S>::check_capacity(&config)?;
                let mut actions = Actions::new();
                let octets = self.slots.iter().map(|s| s.held()).sum::<usize>();
                self.config = config;
                self.slots = [Assembly::Idle; M];
                self.status.discarded_octets.add(octets);
                self.status.assembly.set(0);
                if octets > 0 {
                    actions.push(ReassemblerOutput::Discarded { octets })?;
                }
                Ok(actions)
            }
        }
    }

    /// `Fdu` (LNK-070…LNK-072).
    fn fdu(&mut self, input: &FduIn<'_, P>, out: &mut [u8]) -> Result<ReassemblerActions> {
        let parsed = match P::parse(&self.config.channel, &input.meta, input.data) {
            Ok(parsed) => parsed,
            Err(e) => {
                self.status.refused.incr();
                return Err(e);
            }
        };
        let vc = raw(input.vc);
        let found = self.config.find(vc, parsed.map);
        if let Some(index) = found
            && let Err(e) = self.check(index, &parsed, out.len())
        {
            self.status.refused.incr();
            return Err(e);
        }
        // From here on nothing fails except an internal inconsistency.
        let mut cx = Cx {
            out,
            actions: Actions::new(),
        };
        self.status.fdus_in.incr();
        if parsed.gap {
            self.gap_discard(vc, parsed.map, found, &mut cx)?;
        }
        match found {
            None => self.event(ReassemblyEvent::UnknownMap, vc, parsed.map, &mut cx)?,
            Some(index) => self.apply(index, vc, &parsed, &mut cx)?,
        }
        Ok(cx.actions)
    }

    /// The checks of an FDU for a configured MAP that precede any change (API-058).
    fn check(&self, index: usize, parsed: &Parsed<'_>, area: usize) -> Result<()> {
        let map = self
            .config
            .maps
            .get(index)
            .copied()
            .flatten()
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let rule = Error::new(ErrorKind::InvalidField).with_field(field::TFDZ_CONSTRUCTION_RULE);
        let data = match parsed.unit {
            Unit::Idle => return Ok(()),
            Unit::Stream(data) => {
                if map.service != Service::OctetStream {
                    return Err(rule);
                }
                data
            }
            Unit::Whole(data) | Unit::First(data) | Unit::Continuing(data) | Unit::Last(data) => {
                let fixed_ok = !parsed.fixed_length || map.service == Service::Sdu;
                if map.service == Service::OctetStream || !fixed_ok {
                    return Err(rule);
                }
                data
            }
        };
        // An octet stream assembles nothing: it needs the octets of the FDU only.
        let held_max = match map.max_sdu_len() {
            Some(max) => max.saturating_sub(1),
            None if map.service == Service::OctetStream => 0,
            None => return Err(Error::new(ErrorKind::InternalError)),
        };
        let needed = held_max.saturating_add(data.len());
        if area < needed {
            return Err(Error::buffer_too_small(needed));
        }
        if map.service == Service::Packets {
            match parsed.unit {
                Unit::Whole(data) => {
                    count_packets(data)?;
                }
                Unit::Last(data) => {
                    let held = match self.slots.get(index) {
                        Some(Assembly::Building { len }) => *len,
                        _ => 0,
                    };
                    let buf = self
                        .bufs
                        .get(index)
                        .and_then(|b| b.get(..held))
                        .ok_or(Error::new(ErrorKind::InternalError))?;
                    // A packet that is too long is the event `SduTooLong`, not an error.
                    if held.saturating_add(data.len()) <= map.max_sdu_len && held > 0 {
                        single_packet(buf, data)?;
                    }
                }
                Unit::First(_) | Unit::Continuing(_) | Unit::Stream(_) | Unit::Idle => {}
            }
        }
        Ok(())
    }

    /// Discards the SDUs in progress of the VC `vc` after a gap, with one event that names the MAP
    /// of the field if an SDU of it was in progress, else the first MAP that had one (LNK-070).
    fn gap_discard(
        &mut self,
        vc: u8,
        map: u8,
        found: Option<usize>,
        cx: &mut Cx<'_>,
    ) -> Result<()> {
        let field_had = found.is_some_and(|i| self.held(i) > 0);
        let mut total = 0_usize;
        let mut first = None;
        for (slot, entry) in self.slots.iter_mut().zip(self.config.maps.iter()) {
            let Some(entry) = entry else { continue };
            if raw(entry.vc) != vc || slot.held() == 0 {
                continue;
            }
            total = total.saturating_add(slot.held());
            first.get_or_insert(raw(entry.id));
            *slot = Assembly::Idle;
        }
        let Some(first) = first else { return Ok(()) };
        self.status.discarded_octets.add(total);
        self.refresh_occupancy();
        let named = if field_had { map } else { first };
        self.event(ReassemblyEvent::GapDiscard, vc, named, cx)
    }

    /// Counts and returns an event.
    fn event(&mut self, event: ReassemblyEvent, vc: u8, map: u8, cx: &mut Cx<'_>) -> Result<()> {
        match event {
            ReassemblyEvent::GapDiscard => self.status.gap_discards.incr(),
            ReassemblyEvent::SequenceViolation => self.status.sequence_violations.incr(),
            ReassemblyEvent::UnknownMap => self.status.unknown_maps.incr(),
            ReassemblyEvent::SduTooLong => self.status.sdus_too_long.incr(),
        }
        cx.actions.push(ReassemblerOutput::Event { event, vc, map })
    }

    fn refresh_occupancy(&mut self) {
        let held = self.slots.iter().map(|s| s.held()).sum::<usize>();
        self.status.assembly.set(held);
    }

    /// Drops the SDU in progress of a MAP, counting its octets; returns whether there was one.
    fn drop_partial(&mut self, index: usize) -> bool {
        let held = self.held(index);
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = Assembly::Idle;
        }
        self.status.discarded_octets.add(held);
        self.refresh_occupancy();
        held > 0
    }

    /// Takes the unit of a configured MAP (LNK-070, LNK-071, LNK-072).
    fn apply(&mut self, index: usize, vc: u8, parsed: &Parsed<'_>, cx: &mut Cx<'_>) -> Result<()> {
        let map = self
            .config
            .maps
            .get(index)
            .copied()
            .flatten()
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let id = parsed.map;
        // Only an octet stream (and an idle TFDF) has no maximum, and uses none.
        let max = match (parsed.unit, map.max_sdu_len()) {
            (Unit::Stream(_) | Unit::Idle, _) => 0,
            (_, Some(max)) => max,
            (_, None) => return Err(Error::new(ErrorKind::InternalError)),
        };
        let in_progress = self.held(index) > 0;
        let violation = ReassemblyEvent::SequenceViolation;
        let too_long = ReassemblyEvent::SduTooLong;
        match parsed.unit {
            Unit::Idle => Ok(()),
            Unit::Stream(data) => {
                let range = cx.write(&[], data)?;
                self.status.delivered.incr();
                cx.actions.push(ReassemblerOutput::StreamData {
                    vc,
                    map: id,
                    data: range,
                    after_gap: parsed.gap,
                })
            }
            Unit::Whole(data) => {
                if self.drop_partial(index) {
                    self.event(violation, vc, id, cx)?;
                }
                if data.len() > max {
                    return self.event(too_long, vc, id, cx);
                }
                self.deliver(map.service, vc, id, &[], data, cx)
            }
            Unit::First(data) => {
                if self.drop_partial(index) {
                    self.event(violation, vc, id, cx)?;
                }
                if data.len() > max {
                    return self.event(too_long, vc, id, cx);
                }
                self.append(index, 0, data)
            }
            Unit::Continuing(data) => {
                let held = self.held(index);
                if !in_progress {
                    return self.event(violation, vc, id, cx);
                }
                if held.saturating_add(data.len()) > max {
                    self.drop_partial(index);
                    return self.event(too_long, vc, id, cx);
                }
                self.append(index, held, data)
            }
            Unit::Last(data) => {
                let held = self.held(index);
                if !in_progress {
                    return self.event(violation, vc, id, cx);
                }
                if held.saturating_add(data.len()) > max {
                    self.drop_partial(index);
                    return self.event(too_long, vc, id, cx);
                }
                let buf = self
                    .bufs
                    .get(index)
                    .and_then(|b| b.get(..held))
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                let result =
                    Self::deliver_from(&mut self.status, map.service, (vc, id), buf, data, cx);
                if let Some(slot) = self.slots.get_mut(index) {
                    *slot = Assembly::Idle;
                }
                self.refresh_occupancy();
                result
            }
        }
    }

    fn held(&self, index: usize) -> usize {
        self.slots.get(index).map_or(0, |s| s.held())
    }

    /// Appends `data` to the SDU of a MAP, which holds `held` octets.
    fn append(&mut self, index: usize, held: usize, data: &[u8]) -> Result<()> {
        let end = held.saturating_add(data.len());
        let dst = self
            .bufs
            .get_mut(index)
            .and_then(|b| b.get_mut(held..end))
            .ok_or(Error::new(ErrorKind::InternalError))?;
        for (d, s) in dst.iter_mut().zip(data) {
            *d = *s;
        }
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = Assembly::Building { len: end };
        }
        self.refresh_occupancy();
        Ok(())
    }

    /// Delivers `head` then `tail` as an SDU or as packets, by the service of the MAP.
    fn deliver(
        &mut self,
        service: Service,
        vc: u8,
        map: u8,
        head: &[u8],
        tail: &[u8],
        cx: &mut Cx<'_>,
    ) -> Result<()> {
        Self::deliver_from(&mut self.status, service, (vc, map), head, tail, cx)
    }

    fn deliver_from(
        status: &mut ReassemblerStatus,
        service: Service,
        (vc, map): (u8, u8),
        head: &[u8],
        tail: &[u8],
        cx: &mut Cx<'_>,
    ) -> Result<()> {
        let range = cx.write(head, tail)?;
        match service {
            Service::Packets => {
                let count = count_packets_of(head, tail)?;
                status.delivered.add(count);
                cx.actions.push(ReassemblerOutput::Packets {
                    vc,
                    map,
                    run: range,
                    count,
                })
            }
            Service::Sdu | Service::OctetStream => {
                status.delivered.incr();
                cx.actions.push(ReassemblerOutput::Sdu {
                    vc,
                    map,
                    sdu: range,
                })
            }
        }
    }
}

/// How many packets the octets `data` hold, each as long as its own length field says, with no
/// octet left over (LNK-071; 133.0 §4.1.3.5, 133.1 §4.1.2.8).
///
/// # Errors
/// `InvalidField` (`packet_version`) for a PVN other than 0 and 7, `LengthMismatch`
/// (`packet_len`) for a packet that does not end inside `data`.
fn count_packets(data: &[u8]) -> Result<usize> {
    let mut rest = data;
    let mut count = 0_usize;
    let mismatch = Error::new(ErrorKind::LengthMismatch).with_field(field::PACKET_LEN);
    // bound: each pass takes a packet of at least one octet off `rest`, or returns.
    while let Some(&first) = rest.first() {
        let kind = packet::kind_of(first)
            .ok_or(Error::new(ErrorKind::InvalidField).with_field(field::PACKET_VERSION))?;
        let Length::Total(total) = packet::length(kind, rest) else {
            return Err(mismatch);
        };
        if total == 0 {
            return Err(mismatch);
        }
        rest = rest.get(total..).ok_or(mismatch)?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

/// Whether `head` then `tail` are exactly one packet (LNK-070).
fn single_packet(head: &[u8], tail: &[u8]) -> Result<()> {
    // The header of a packet is at most 8 octets (133.1 §4.1.2), so the first 8 octets say its
    // length.
    let mut prefix = [0_u8; 8];
    let mut seen = 0_usize;
    for (d, s) in prefix.iter_mut().zip(head.iter().chain(tail)) {
        *d = *s;
        seen = seen.saturating_add(1);
    }
    let start = prefix
        .get(..seen)
        .ok_or(Error::new(ErrorKind::InternalError))?;
    let kind = start
        .first()
        .and_then(|&b| packet::kind_of(b))
        .ok_or(Error::new(ErrorKind::InvalidField).with_field(field::PACKET_VERSION))?;
    match packet::length(kind, start) {
        Length::Total(total) if total == head.len().saturating_add(tail.len()) => Ok(()),
        _ => Err(Error::new(ErrorKind::LengthMismatch).with_field(field::PACKET_LEN)),
    }
}

/// [`count_packets`] over two parts.
fn count_packets_of(head: &[u8], tail: &[u8]) -> Result<usize> {
    if head.is_empty() {
        count_packets(tail)
    } else {
        // A reassembled packet is one packet; `single_packet` checked it before the change.
        Ok(1)
    }
}

#[cfg(feature = "tc")]
mod tc_impl {
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_tc::{SequenceFlags, TcMapId};

    use super::{Parsed, ReassemblerProtocol, Unit};
    use crate::protocol::Tc;

    /// The parameters of the TC VCs of a `MapReassembler`: whether their FDUs have a Segment Header
    /// (232.0 §5.3 Table 5-3; the setting is the same for all VCs of the reassembler).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TcReassemblerParams {
        segment_header: bool,
    }

    impl TcReassemblerParams {
        /// VCs whose FDUs have a Segment Header, which names the MAP and the sequence flags.
        #[must_use]
        pub const fn with_segment_header() -> TcReassemblerParams {
            TcReassemblerParams {
                segment_header: true,
            }
        }

        /// VCs without a Segment Header: every FDU is whole user data of the implicit MAP 0
        /// (232.0 §4.1.3.2.1.3).
        #[must_use]
        pub const fn without_segment_header() -> TcReassemblerParams {
            TcReassemblerParams {
                segment_header: false,
            }
        }
    }

    impl ReassemblerProtocol for Tc {
        type Map = TcMapId;
        type Channel = TcReassemblerParams;
        type Meta = ();

        fn parse<'a>(channel: &TcReassemblerParams, (): &(), data: &'a [u8]) -> Result<Parsed<'a>> {
            let no_data = Error::new(ErrorKind::InvalidField).with_field(field::FRAME_DATA_FIELD);
            if !channel.segment_header {
                return if data.is_empty() {
                    Err(no_data)
                } else {
                    Ok(Parsed {
                        map: 0,
                        gap: false,
                        fixed_length: false,
                        unit: Unit::Whole(data),
                    })
                };
            }
            let (&header, user) = data.split_first().ok_or(no_data)?;
            if user.is_empty() {
                return Err(no_data);
            }
            let unit = match SequenceFlags::from_bits(header >> 6) {
                SequenceFlags::First => Unit::First(user),
                SequenceFlags::Continuing => Unit::Continuing(user),
                SequenceFlags::Last => Unit::Last(user),
                SequenceFlags::Unsegmented => Unit::Whole(user),
            };
            Ok(Parsed {
                map: header & 0x3F,
                gap: false,
                fixed_length: false,
                unit,
            })
        }
    }
}

#[cfg(feature = "tc")]
pub use tc_impl::TcReassemblerParams;

#[cfg(feature = "uslp")]
mod uslp_impl {
    use aldis_core::ids::MapId;
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_uslp::{ConstructionRule, LastValidOctet, Upid};

    use super::{Parsed, ReassemblerProtocol, Unit};
    use crate::protocol::Uslp;

    /// What the frame header and the receiver give with the TFDF of a USLP FDU (LNK-050, LNK-051).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct UslpFdu {
        /// The MAP ID of the frame.
        pub map: MapId<4>,
        /// A gap was flagged on the field (LNK-051).
        pub gap: bool,
    }

    impl ReassemblerProtocol for Uslp {
        type Map = MapId<4>;
        type Channel = ();
        type Meta = UslpFdu;

        fn parse<'a>(&(): &(), meta: &UslpFdu, data: &'a [u8]) -> Result<Parsed<'a>> {
            let invalid = |f| Error::new(ErrorKind::InvalidField).with_field(f);
            let &first = data.first().ok_or(invalid(field::FRAME_DATA_FIELD))?;
            let map = meta.map.get();
            let (gap, rule) = (meta.gap, ConstructionRule::from_bits(first >> 5));
            let parsed = |fixed_length, unit| Parsed {
                map,
                gap,
                fixed_length,
                unit,
            };
            // The TFDZ of an OID frame is idle data whatever its rule (732.1 §4.1.4.1.9).
            if Upid::from_bits(first & 0x1F) == Some(Upid::OnlyIdleData) {
                return Ok(parsed(false, Unit::Idle));
            }
            // The data after a header of `skip` octets.
            let user = |skip: usize| {
                data.get(skip..)
                    .filter(|u| !u.is_empty())
                    .ok_or(invalid(field::FRAME_DATA_FIELD))
            };
            match rule {
                ConstructionRule::PacketsSpanningFrames => {
                    Err(invalid(field::TFDZ_CONSTRUCTION_RULE))
                }
                ConstructionRule::SduStart | ConstructionRule::SduContinuation => {
                    let zone = user(3)?;
                    let pointer = match data.get(1..3) {
                        Some(&[a, b]) => u16::from_be_bytes([a, b]),
                        _ => return Err(invalid(field::FRAME_DATA_FIELD)),
                    };
                    let (data, ends) = match LastValidOctet::from_raw(pointer) {
                        LastValidOctet::SduContinues => (zone, false),
                        LastValidOctet::At(at) => (
                            zone.get(..=usize::from(at.get())).ok_or(
                                Error::new(ErrorKind::Malformed).with_field(field::FHP_LVO_POINTER),
                            )?,
                            true,
                        ),
                    };
                    let unit = match (rule, ends) {
                        (ConstructionRule::SduStart, true) => Unit::Whole(data),
                        (ConstructionRule::SduStart, false) => Unit::First(data),
                        (_, true) => Unit::Last(data),
                        (_, false) => Unit::Continuing(data),
                    };
                    Ok(parsed(true, unit))
                }
                ConstructionRule::OctetStream => Ok(parsed(false, Unit::Stream(user(1)?))),
                ConstructionRule::StartingSegment => Ok(parsed(false, Unit::First(user(1)?))),
                ConstructionRule::ContinuingSegment => {
                    Ok(parsed(false, Unit::Continuing(user(1)?)))
                }
                ConstructionRule::LastSegment => Ok(parsed(false, Unit::Last(user(1)?))),
                ConstructionRule::NoSegmentation => Ok(parsed(false, Unit::Whole(user(1)?))),
            }
        }
    }
}

#[cfg(feature = "uslp")]
pub use uslp_impl::UslpFdu;
