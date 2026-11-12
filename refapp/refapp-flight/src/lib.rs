//! Flight node of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md` §2.1 (applications and their per-instant
//! order), §3.3.1–3.3.3 (layouts), §5 (time), §8.1 (step contract). `no_std`, no heap, flight
//! lint set (APP-102), no `unsafe` (APP-105).
//!
//! **A1 WP-3a:** on-board time, housekeeping, the event reporter, and the payload application.
//! **A1 WP-3b:** the command handler with its time-tag queue, the table store, and the memory
//! dump (APP-012, APP-013, APP-016, APP-017, APP-024); the node ignores ops-script entries,
//! which only the ground node uses.
//! **A2 WP-3a:** the downlink at frame level ([`frames_down`]): generators, multiplexer, OCF, and
//! TM frames, selected by [`FlightNode::new_frames`], which also selects the uplink at frame level (APP-030…APP-035, APP-090, APP-132).
#![no_std]
#![forbid(unsafe_code)]

pub mod command;
pub mod downlink;
pub mod dump;
pub mod event;
pub mod frames_down;
pub mod frames_up;
pub mod layout;
pub mod queue;
pub mod schedule;
pub mod table;

mod handler;

use aldis::core::Result;
use aldis::spp::SequenceFlags;
use aldis::tc::Clcw;
use refapp_scenario::{Instant, Node, Output, StepInput};

pub use downlink::{Downlink, Stream, onboard_time};
use dump::Dump;
pub use event::Event;
use frames_down::FrameDownlink;
pub use frames_up::FrameUplink;
pub use layout::{PayloadMode, Status};
use layout::{housekeeping, payload};
use queue::TimeTagQueue;
use schedule::Periodic;
use table::TableStore;

/// The flight node (APP-010).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlightNode {
    downlink: Downlink,
    /// The frame level of the downlink; none at the packet level of A1 (§6.1).
    frames: Option<FrameDownlink>,
    status: Status,
    /// Time-tagged commands waiting for their execution time (§2.3).
    queue: TimeTagQueue,
    /// The most recently uploaded table (APP-017).
    table: TableStore,
    /// The dump in progress, if any (APP-016).
    dump: Option<Dump>,
    /// Index of the next housekeeping slot; also the number of the next HK packet (§3.3.1).
    hk_slot: u32,
    /// Index of the next payload slot (§2.1).
    payload_slot: u32,
    /// Number of the next payload packet (§3.3.3).
    payload_number: u32,
    /// Packets that could not be built; 0 in a correctly configured run.
    build_failures: u32,
    /// The uplink at frame level (A2 on), `None` at packet level.
    uplink: Option<FrameUplink>,
}

impl FlightNode {
    /// A flight node in its start-of-run state: every count at 0, payload mode *on* (§2.1).
    #[must_use]
    pub fn new() -> FlightNode {
        FlightNode::default()
    }

    /// A flight node at frame level (A2 on, §6.1): the downlink unit is the TM frame, one at every
    /// step with `tx_slot` (APP-090, APP-132), and the uplink units are TC frames (APP-139), each
    /// through the receiver, FARM-1, and the MAP reassembler before the command handler
    /// (APP-040…APP-043). [`FlightNode::new`] is the packet level of A1.
    ///
    /// # Errors
    /// `InvalidConfig` if the scenario values are no valid engine configuration, which the
    /// generated constants rule out.
    pub fn new_frames() -> Result<FlightNode> {
        Ok(FlightNode {
            frames: Some(FrameDownlink::new()?),
            uplink: Some(FrameUplink::new()?),
            ..FlightNode::default()
        })
    }

    /// A flight node at the COP-1 level (A3 on, §6.1): as [`FlightNode::new_frames`], with FARM-1
    /// seeing the Type-AD and Type-BC frames and its state changes and Unlock raised as the
    /// events 210 and 211 (APP-146, APP-149).
    ///
    /// # Errors
    /// As [`FlightNode::new_frames`].
    pub fn new_cop1() -> Result<FlightNode> {
        Ok(FlightNode {
            frames: Some(FrameDownlink::new()?),
            uplink: Some(FrameUplink::new_cop1()?),
            ..FlightNode::default()
        })
    }

    /// The CLCW FARM-1 reports now, for the OCF of the downlink (§4.4.1, APP-134); `None` at
    /// packet level, which has no FARM-1. The downlink converts it with `Clcw::to_ocf`.
    #[must_use]
    pub fn clcw(&self) -> Option<Clcw> {
        self.uplink.as_ref().map(FrameUplink::clcw)
    }

    /// The status housekeeping reports (§3.3.1).
    #[must_use]
    pub const fn status(&self) -> &Status {
        &self.status
    }

    /// The status, for a test or a harness to set up a state; the command handler and the table
    /// store update it themselves.
    pub const fn status_mut(&mut self) -> &mut Status {
        &mut self.status
    }

    /// Failures of the node's output: 0 in a correctly configured run. A packet it could not
    /// build (the time conversion or the packet writer failed, which the scenario's values rule
    /// out, APP-060) is raised as the application event `packet_rejected` (APP-131). At frame
    /// level it also counts a packet a generator refused and a slot an engine failed, each raised
    /// as `downlink_refused` (APP-142).
    #[must_use]
    pub const fn build_failures(&self) -> u32 {
        self.build_failures
    }

    /// The event reporter (§2.1): one event packet at the instant `now` of the event.
    pub fn report(&mut self, now: Instant, event: &Event, out: &mut dyn Output) {
        self.send(Stream::Events, now, &event.user_data(), out);
    }

    /// Sends an unsegmented packet of `stream`; a failure is counted, never panics.
    fn send(&mut self, stream: Stream, now: Instant, user_data: &[u8], out: &mut dyn Output) {
        let (sent, refused) = {
            let mut tap = FlightNode::tap(self.frames.as_mut(), stream, now, out);
            let sent =
                self.downlink
                    .send(stream, SequenceFlags::Unsegmented, now, user_data, &mut tap);
            (sent, tap.refused())
        };
        if let Some(kind) = refused {
            self.packet_refused(kind, frames_down::vc_of(stream), out);
        }
        if let Err(e) = sent {
            self.build_failed(e.kind(), out);
        }
    }

    /// Housekeeping, step 4 of §2.1: one packet at every slot instant reached (APP-010). A
    /// packet of a slot passed before this step (a late step) is stamped with its own slot
    /// instant and reports the status as it is now.
    fn housekeeping(&mut self, now: Instant, out: &mut dyn Output) {
        // bound: each pass takes one slot; `Periodic::at` is `None` from slot index 30 on.
        while let Some(at) = Periodic::HOUSEKEEPING
            .at(self.hk_slot)
            .filter(|at| *at <= now)
        {
            let data = housekeeping(&self.status, self.hk_slot);
            self.send(Stream::Housekeeping, at, &data, out);
            self.hk_slot = self.hk_slot.saturating_add(1);
        }
    }

    /// Payload, step 5 of §2.1: one packet at every slot instant reached while the mode is
    /// *on*; a slot reached while it is *off* produces nothing (APP-015). A late step stamps
    /// each packet with its own slot instant and uses the mode as it is now.
    fn payload(&mut self, now: Instant, out: &mut dyn Output) {
        // bound: each pass takes one slot; `Periodic::at` is `None` from slot index 15 on.
        while let Some(at) = Periodic::PAYLOAD
            .at(self.payload_slot)
            .filter(|at| *at <= now)
        {
            if self.status.payload_mode == PayloadMode::On {
                let data = payload(self.payload_number);
                self.send(Stream::Payload, at, &data, out);
                self.payload_number = self.payload_number.saturating_add(1);
            }
            self.payload_slot = self.payload_slot.saturating_add(1);
        }
    }

    /// The earliest instant after `now` at which a periodic application is due.
    fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let [queued, dump] = self.next_command_deadline();
        [
            Periodic::HOUSEKEEPING.at(self.hk_slot),
            Periodic::PAYLOAD.at(self.payload_slot),
            queued,
            dump,
            self.frames.as_ref().and_then(|f| f.next_deadline(now)),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

impl Node for FlightNode {
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        // §2.1 steps 1–3: delivered packets, time-tagged commands, dump packets.
        self.handle_delivered(input.now, input.delivered, out);
        self.execute_due(input.now, out);
        self.dump_segments(input.now, out);
        self.housekeeping(input.now, out);
        self.payload(input.now, out);
        // §2.1 step 6 (A2 on): the frame of the slot.
        self.downlink_frame(input.tx_slot, input.now, out);
        self.next_deadline(input.now)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;

    use std::vec::Vec;

    use aldis::core::ids::Apid;
    use aldis::spp::{SecondaryHeaderLayout, SequenceFlags, SpacePacket, TimeHeaderLayout};
    use aldis::time::{CucEpoch, CucFormat, TimeCode, TimeFormat};
    use refapp_scenario::generated::apid;
    use refapp_scenario::{Delivered, Instant, Node, Output, StepInput};

    use super::{Event, FlightNode, PayloadMode};

    /// A recorded output: kind, `packet_out` APID or 0, octets.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Out {
        Unit(Vec<u8>),
        Packet(u16, Vec<u8>),
        /// An application event: code and arguments.
        Event(u16, Vec<u64>),
    }

    #[derive(Default)]
    pub(crate) struct Rec(pub(crate) Vec<(u64, Out)>);

    pub(crate) struct At<'a>(pub(crate) u64, pub(crate) &'a mut Rec);

    impl Output for At<'_> {
        fn unit(&mut self, octets: &[u8]) {
            self.1.0.push((self.0, Out::Unit(octets.to_vec())));
        }
        fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
            self.1
                .0
                .push((self.0, Out::Packet(apid.get(), octets.to_vec())));
        }
        fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {
            panic!("the flight node has no packet_in");
        }
        fn app_event(&mut self, code: u16, args: &[u64]) {
            self.1.0.push((self.0, Out::Event(code, args.to_vec())));
        }
    }

    pub(crate) fn input(now: u64) -> StepInput<'static> {
        StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(&[]),
            ops: &[],
            tx_slot: true,
        }
    }

    /// Steps the node at each of its deadlines, from t = 0 to the end of the run at 35 s.
    fn run(node: &mut FlightNode, extra_wakeups: bool) -> Rec {
        let mut rec = Rec::default();
        let mut t = 0_u64;
        loop {
            let due = node
                .step(&input(t), &mut At(t, &mut rec))
                .map(Instant::as_micros);
            let grid = t.saturating_add(125_000);
            t = match (due, extra_wakeups) {
                (Some(d), true) => d.min(grid),
                (None, true) => grid,
                (Some(d), false) => d,
                (None, false) => break,
            };
            if t >= 35_000_000 {
                break;
            }
        }
        rec
    }

    pub(crate) fn packets(rec: &Rec, apid: u16) -> Vec<(u64, Vec<u8>)> {
        rec.0
            .iter()
            .filter_map(|(t, o)| match o {
                Out::Packet(a, p) if *a == apid => Some((*t, p.clone())),
                Out::Packet(..) | Out::Unit(_) | Out::Event(..) => None,
            })
            .collect()
    }

    fn count(rec: &Rec, apid: u16) -> Vec<u16> {
        packets(rec, apid)
            .iter()
            .map(|(_, p)| SpacePacket::decode(p).unwrap().sequence_count().get())
            .collect()
    }

    fn layout(explicit: bool) -> SecondaryHeaderLayout {
        let f = CucFormat::new(CucEpoch::Tai1958, 4, 2).unwrap();
        SecondaryHeaderLayout::Time(TimeHeaderLayout::new(TimeFormat::Cuc(f), explicit, 0).unwrap())
    }

    #[test]
    fn nominal_telemetry_counts() {
        // req: APP-010, APP-015
        let mut node = FlightNode::new();
        let rec = run(&mut node, false);
        assert_eq!(packets(&rec, apid::HOUSEKEEPING).len(), 30);
        assert_eq!(packets(&rec, apid::PAYLOAD).len(), 15);
        assert_eq!(packets(&rec, apid::EVENTS).len(), 0);
        assert_eq!(node.build_failures(), 0);
        let hk = packets(&rec, apid::HOUSEKEEPING);
        assert_eq!(hk[0].0, 0);
        assert_eq!(hk[29].0, 29_000_000);
        let pl = packets(&rec, apid::PAYLOAD);
        assert_eq!(pl[0].0, 500_000);
        assert_eq!(pl[14].0, 28_500_000);
    }

    #[test]
    fn extra_wakeups_change_nothing() {
        // req: APP-074
        let plain = run(&mut FlightNode::new(), false);
        let busy = run(&mut FlightNode::new(), true);
        assert_eq!(plain.0, busy.0);
    }

    #[test]
    fn every_packet_is_a_unit_after_its_packet_out() {
        // req: APP-010
        let rec = run(&mut FlightNode::new(), false);
        assert_eq!(rec.0.len() % 2, 0);
        for pair in rec.0.chunks(2) {
            let [(t1, Out::Packet(_, p)), (t2, Out::Unit(u))] = pair else {
                panic!("expected packet_out then unit: {pair:?}");
            };
            assert_eq!((t1, p), (t2, u));
        }
    }

    #[test]
    fn housekeeping_before_payload_within_an_instant() {
        // req: APP-010
        let mut node = FlightNode::new();
        let mut rec = Rec::default();
        // A late step at 2.5 s finds HK slots 0, 1, 2 and payload slots 0, 1 due; one instant
        // gives HK first, then payload (§2.1 steps 4, 5).
        node.step(&input(2_500_000), &mut At(2_500_000, &mut rec));
        let order: Vec<u16> = rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Packet(a, _) => Some(*a),
                Out::Unit(_) | Out::Event(..) => None,
            })
            .collect();
        assert_eq!(order[..3], [apid::HOUSEKEEPING; 3]);
        assert_eq!(order[3..], [apid::PAYLOAD; 2]);
        // Each packet carries the time stamp of its own slot, not of the late step (APP-060):
        // the coarse low octet is the seconds after T0 and the fraction is octets 10 and 11.
        let stamps: Vec<(u8, [u8; 2])> = rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Packet(_, p) => Some((p[9], [p[10], p[11]])),
                Out::Unit(_) | Out::Event(..) => None,
            })
            .collect();
        assert_eq!(
            stamps,
            [
                (0, [0, 0]),
                (1, [0, 0]),
                (2, [0, 0]),
                (0, [0x80, 0]),
                (2, [0x80, 0])
            ]
        );
    }

    #[test]
    fn apids_have_their_own_sequence_counts() {
        // req: APP-021
        let mut node = FlightNode::new();
        let mut rec = Rec::default();
        let ev = Event::CmdExecuted {
            function: 1,
            seq: 0,
        };
        node.report(Instant::from_micros(0), &ev, &mut At(0, &mut rec));
        node.step(&input(0), &mut At(0, &mut rec));
        node.report(Instant::from_micros(0), &ev, &mut At(0, &mut rec));
        assert_eq!(count(&rec, apid::EVENTS), [0, 1]);
        assert_eq!(count(&rec, apid::HOUSEKEEPING), [0]);
        node.step(&input(1_000_000), &mut At(1_000_000, &mut rec));
        assert_eq!(count(&rec, apid::HOUSEKEEPING), [0, 1]);
        assert_eq!(count(&rec, apid::PAYLOAD), [0]);
    }

    #[test]
    fn sequence_counts_wrap_modulo_16384() {
        // req: APP-021
        let mut node = FlightNode::new();
        let mut rec = Rec::default();
        let ev = Event::CmdExecuted {
            function: 1,
            seq: 0,
        };
        for _ in 0..16_385 {
            node.report(Instant::from_micros(0), &ev, &mut At(0, &mut rec));
        }
        let events = packets(&rec, apid::EVENTS);
        let count = |i: usize| {
            SpacePacket::decode(&events[i].1)
                .unwrap()
                .sequence_count()
                .get()
        };
        assert_eq!((count(0), count(16_383), count(16_384)), (0, 16_383, 0));
    }

    #[test]
    fn packets_decode_strictly_with_their_secondary_header_form() {
        // req: APP-022, APP-060
        let mut node = FlightNode::new();
        let mut rec = Rec::default();
        let t = 3_000_000;
        node.report(
            Instant::from_micros(t),
            &Event::PayloadMode {
                mode: PayloadMode::Off,
            },
            &mut At(t, &mut rec),
        );
        node.step(&input(t), &mut At(t, &mut rec));
        for (apid, explicit, len) in [(apid::EVENTS, true, 21), (apid::HOUSEKEEPING, false, 44)] {
            let (_, bytes) = packets(&rec, apid).pop().unwrap();
            assert_eq!(bytes.len(), len);
            let p = SpacePacket::decode(&bytes).unwrap();
            assert!(p.has_secondary_header());
            assert_eq!(p.sequence_flags(), SequenceFlags::Unsegmented);
            let split = p.split(&layout(explicit)).unwrap();
            assert_eq!(split.secondary_header().len(), if explicit { 7 } else { 6 });
            // CUC T-field: T0 = 2 100 000 000 s = 0x7D2B7500, plus 3 s, fraction 0.
            let tfield = &split.secondary_header()[usize::from(explicit)..];
            assert_eq!(tfield, [0x7D, 0x2B, 0x75, 0x03, 0, 0]);
            if explicit {
                assert_eq!(split.secondary_header()[0], 0x1E);
            }
            let Some(TimeCode::Cuc(cuc)) = split.time_code().map(|d| d.code()) else {
                panic!("not a CUC code");
            };
            assert_eq!(cuc.to_seconds_nanos(), (2_100_000_003, 0));
        }
    }

    #[test]
    fn time_stamps_are_exact_on_the_125_ms_grid() {
        // req: APP-060, APP-061
        for (micros, tfield) in [
            (125_000_u64, [0x7D, 0x2B, 0x75, 0x00, 0x20, 0x00]),
            (28_500_000, [0x7D, 0x2B, 0x75, 0x1C, 0x80, 0x00]),
        ] {
            let mut rec = Rec::default();
            FlightNode::new().report(
                Instant::from_micros(micros),
                &Event::CmdExecuted {
                    function: 1,
                    seq: 0,
                },
                &mut At(micros, &mut rec),
            );
            let (_, bytes) = packets(&rec, apid::EVENTS).pop().unwrap();
            assert_eq!(bytes[7..13], tfield);
        }
    }

    #[test]
    fn payload_mode_off_skips_slots_and_keeps_the_numbering() {
        // req: APP-015
        let mut node = FlightNode::new();
        let mut rec = Rec::default();
        node.step(&input(500_000), &mut At(500_000, &mut rec));
        node.status_mut().payload_mode = PayloadMode::Off;
        node.step(&input(2_500_000), &mut At(2_500_000, &mut rec));
        node.status_mut().payload_mode = PayloadMode::On;
        node.step(&input(4_500_000), &mut At(4_500_000, &mut rec));
        let numbers: Vec<u8> = packets(&rec, apid::PAYLOAD)
            .iter()
            .map(|(_, p)| p[12 + 3])
            .collect();
        assert_eq!(numbers, [0, 1]);
        assert_eq!(packets(&rec, apid::PAYLOAD).len(), 2);
    }

    #[test]
    fn housekeeping_reports_the_status() {
        // req: APP-023
        let mut node = FlightNode::new();
        node.status_mut().commands_executed = 3;
        let mut rec = Rec::default();
        node.step(&input(0), &mut At(0, &mut rec));
        let (_, hk) = packets(&rec, apid::HOUSEKEEPING).pop().unwrap();
        // 6 header + 6 time octets, then the user data of §3.3.1.
        assert_eq!(hk[12 + 4..12 + 6], [0, 3]);
        assert_eq!(hk[12 + 9], 1);
    }

    #[test]
    fn deadline_is_the_next_slot_and_none_after_the_last() {
        // req: APP-010
        let mut node = FlightNode::new();
        let mut rec = Rec::default();
        let next = node.step(&input(0), &mut At(0, &mut rec));
        assert_eq!(next, Some(Instant::from_micros(500_000)));
        let next = node.step(&input(29_000_000), &mut At(29_000_000, &mut rec));
        assert_eq!(next, None);
    }
}

#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod frames_down_tests;
