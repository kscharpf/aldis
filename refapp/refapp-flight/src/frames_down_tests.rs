//! Tests of the downlink at frame level through the node interface (docs/reference-application.md
//! §2.1 step 6, §4.1, §4.3.1, §4.4; APP-026, APP-030…APP-035, APP-090, APP-132, APP-134, APP-135,
//! APP-169…APP-173).

extern crate std;

use std::vec::Vec;

use aldis::core::crc::verify_fecf;
use aldis::core::ids::{Apid, SeqCount};
use aldis::spp::{PacketFields, PacketType, SequenceFlags, SpacePacketWriter, SppConfig};
use aldis::tc::SequenceFlags as TcFlags;
use refapp_scenario::generated::{SPACECRAFT_ID, apid, time, tm};
use refapp_scenario::{Delivered, Instant, Node, StepInput, Variant};

use crate::frames_up::tests::bd_frame;
use crate::tests::{At, Out, Rec};
use crate::{FlightNode, PayloadMode};

const SLOT: u64 = 125_000;
const END: u64 = 35_000_000;

/// A command packet (§3.3.5) with the execution time `at` µs after T0.
fn command(seq: u16, function: u8, at: u64, args: &[u8]) -> Vec<u8> {
    let seconds = u32::try_from(at / 1_000_000)
        .unwrap()
        .wrapping_add(time::T0_COARSE_S);
    let [s3, s2, s1, s0] = seconds.to_be_bytes();
    let mut data = std::vec![function, 1, s3, s2, s1, s0, 0, 0];
    data.extend_from_slice(args);
    let mut buffer = [0_u8; 64];
    let fields = PacketFields {
        packet_type: PacketType::Telecommand,
        apid: Apid::new(apid::COMMANDS).unwrap(),
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::new(seq).unwrap(),
    };
    let mut writer =
        SpacePacketWriter::begin(&mut buffer, &SppConfig::DEFAULT, fields, 0, data.len()).unwrap();
    writer.user_data_mut().copy_from_slice(&data);
    let total = writer.finish(data.len()).unwrap();
    buffer[..total].to_vec()
}

/// The commands of the nominal timeline (§2.4) that the downlink sees, delivered at their release
/// instants as Type-BD frames of the uplink (§4.2). The table upload is left out: the
/// `ACTIVATE_TABLE` is then rejected with `TABLE_REJECTED`, an event packet of the same size.
fn nominal_commands() -> Vec<(u64, Vec<u8>)> {
    let packets = std::vec![
        (1_000_000, command(0, 1, 6_000_000, &[])),
        (3_000_000, command(1, 3, 10_000_000, &[1, 0x0C, 0x4F])),
        (
            4_000_000,
            command(2, 2, 12_000_000, &[0, 1, 0, 0, 0x02, 0x58])
        ),
        (5_000_000, command(3, 4, 20_000_000, &[0])),
        (5_000_000, command(4, 1, 20_000_000, &[])),
        (6_000_000, command(5, 4, 24_000_000, &[1])),
    ];
    packets
        .into_iter()
        .map(|(at, packet)| (at, bd_frame(0, TcFlags::Unsegmented, 0, &packet)))
        .collect()
}

/// A node at frame level with the output of every step.
struct Run {
    node: FlightNode,
    rec: Rec,
}

impl Run {
    fn new() -> Self {
        Self::of(Variant::TmTc)
    }

    fn of(variant: Variant) -> Self {
        Self {
            node: FlightNode::new_frames(variant).unwrap(),
            rec: Rec::default(),
        }
    }

    fn step(&mut self, now: u64, tx_slot: bool, delivered: &[Vec<u8>]) -> Option<u64> {
        let refs: Vec<&[u8]> = delivered.iter().map(Vec::as_slice).collect();
        let input = StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(&refs),
            ops: &[],
            tx_slot,
        };
        self.node
            .step(&input, &mut At(now, &mut self.rec))
            .map(Instant::as_micros)
    }

    /// Steps at every frame slot until the end of the run, with `commands` delivered at their
    /// instants.
    fn run(&mut self, commands: &[(u64, Vec<u8>)]) {
        for slot in 0..END / SLOT {
            let now = slot.saturating_mul(SLOT);
            let delivered: Vec<Vec<u8>> = commands
                .iter()
                .filter(|(at, _)| *at == now)
                .map(|(_, c)| c.clone())
                .collect();
            self.step(now, true, &delivered);
        }
    }

    fn frames(&self) -> Vec<Vec<u8>> {
        self.rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Unit(u) => Some(u.clone()),
                Out::Packet(..) | Out::Event(..) => None,
            })
            .collect()
    }
}

fn nominal() -> Run {
    nominal_of(Variant::TmTc)
}

fn nominal_of(variant: Variant) -> Run {
    let mut run = Run::of(variant);
    run.run(&nominal_commands());
    run
}

/// A transfer frame split into the fields the scenario fixes (§4.1).
struct Frame<'a> {
    octets: &'a [u8],
}

impl Frame<'_> {
    fn vc(&self) -> u8 {
        (self.octets[1] >> 1) & 7
    }
    fn mc_count(&self) -> u8 {
        self.octets[2]
    }
    fn vc_count(&self) -> u8 {
        self.octets[3]
    }
    /// Data Field Status, 2 octets (132.0 §4.1.2.7).
    fn status(&self) -> [u8; 2] {
        [self.octets[4], self.octets[5]]
    }
    /// The First Header Pointer, 11 bits.
    fn fhp(&self) -> u16 {
        u16::from_be_bytes(self.status()) & 0x7FF
    }
    fn data(&self) -> &[u8] {
        &self.octets[6..217]
    }
    fn ocf(&self) -> [u8; 4] {
        self.octets[217..221].try_into().unwrap()
    }
}

/// The packets in the data fields of `vc` in order: the concatenated fields, cut by the length
/// field of each packet (132.0 §4.1.4.2, 133.0 §4.1.3.5), Idle Packets included.
fn packets_of(frames: &[Vec<u8>], vc: u8) -> Vec<Vec<u8>> {
    let stream: Vec<u8> = frames
        .iter()
        .map(|f| Frame { octets: f })
        .filter(|f| f.vc() == vc)
        .flat_map(|f| f.data().to_vec())
        .collect();
    let mut packets = Vec::new();
    let mut rest = stream.as_slice();
    while rest.len() >= 6 {
        let len = usize::from(u16::from_be_bytes([rest[4], rest[5]])) + 7;
        packets.push(rest[..len].to_vec());
        rest = &rest[len..];
    }
    assert_eq!(rest.len(), 0);
    packets
}

fn apid_of(packet: &[u8]) -> u16 {
    u16::from_be_bytes([packet[0], packet[1]]) & 0x7FF
}

#[test]
fn one_frame_at_every_slot_and_none_otherwise() {
    // req: APP-090, APP-030
    let mut run = Run::new();
    run.step(0, true, &[]);
    assert_eq!(run.frames().len(), 1);
    run.step(10_000, false, &[]);
    run.step(60_000, false, &[]);
    assert_eq!(run.frames().len(), 1);
    run.step(SLOT, true, &[]);
    assert_eq!(run.frames().len(), 2);
    let full = nominal();
    assert_eq!(full.frames().len(), 280);
}

#[test]
fn the_frame_is_the_last_output_and_the_only_unit_of_the_step() {
    // req: APP-090, APP-132
    let mut run = Run::new();
    run.step(0, true, &[]);
    // HK packet 0 as packet_out, then the OID frame as the unit.
    let kinds: Vec<&str> = run
        .rec
        .0
        .iter()
        .map(|(_, o)| match o {
            Out::Packet(..) => "packet_out",
            Out::Unit(_) => "unit",
            Out::Event(..) => "event",
        })
        .collect();
    assert_eq!(kinds, ["packet_out", "unit"]);
    // A step without a slot reports its packets and no frame.
    run.step(1_000_000, false, &[]);
    assert_eq!(run.frames().len(), 1);
}

#[test]
fn frames_have_the_fields_of_the_scenario() {
    // req: APP-030, APP-026, APP-031, APP-032
    let run = nominal();
    for octets in run.frames() {
        assert_eq!(octets.len(), usize::from(tm::FRAME_LENGTH));
        verify_fecf(&octets).unwrap();
        let f = Frame { octets: &octets };
        // Version 00, SCID 0x155, OCF flag 1 (first two octets 15 5x), no Secondary Header, Sync
        // Flag 0, Packet Order Flag 0, Segment Length Identifier 11: status `18` + FHP high bits.
        assert_eq!(
            u16::from_be_bytes([octets[0], octets[1]]) >> 4,
            SPACECRAFT_ID
        );
        assert_eq!(octets[1] & 1, 1);
        assert_eq!(f.status()[0] >> 3, 0b11);
        assert!([0, 1, 7].contains(&f.vc()));
    }
}

#[test]
fn the_first_frames_follow_the_schedule() {
    // req: APP-033, APP-034, APP-137
    let frames = nominal().frames();
    // OID frames 0 and 1; HK 0 released by the VC 0 deadline at frame 2; payload 0 at frame 4.
    assert_eq!(frames[0][..6], [0x15, 0x5F, 0x00, 0x00, 0x1F, 0xFE]);
    // The second OID frame: MC count 1, VC 7 count 1.
    assert_eq!(frames[1][..6], [0x15, 0x5F, 0x01, 0x01, 0x1F, 0xFE]);
    assert_eq!(frames[2][..6], [0x15, 0x51, 0x02, 0x00, 0x18, 0x00]);
    assert_eq!(frames[4][..6], [0x15, 0x53, 0x04, 0x00, 0x18, 0x00]);
}

#[test]
fn a_started_field_is_completed_with_an_idle_packet_at_its_deadline() {
    // req: APP-033
    let frames = nominal().frames();
    let f = Frame { octets: &frames[2] };
    // HK packet 0 (44 octets) then a Space Packet Idle Packet of 167 octets: APID 2047, flags 11,
    // count 0 (OI-90), packet data length 160, and the idle data 0x00.
    assert_eq!(apid_of(f.data()), apid::HOUSEKEEPING);
    assert_eq!(f.data()[44..50], [0x07, 0xFF, 0xC0, 0x00, 0x00, 0xA0]);
    assert!(f.data()[50..].iter().all(|b| *b == 0));
    // Not before the deadline: frames 0 and 1 (t = 0, 0.125 s) are OID frames.
    assert_eq!(Frame { octets: &frames[1] }.vc(), 7);
}

#[test]
fn the_payload_packet_fills_a_field_and_its_residue_waits_for_the_deadline() {
    // req: APP-033, APP-032
    let frames = nominal().frames();
    // Payload packet 0 (412 octets) at 0.5 s: the first 211 fill field 1 at once (frame 4).
    let first = Frame { octets: &frames[4] };
    assert_eq!(first.fhp(), 0);
    assert_eq!(apid_of(first.data()), apid::PAYLOAD);
    // The other 201 octets wait 1 s (frame 12), then 10 octets of idle fill; FHP 201.
    let second = Frame {
        octets: &frames[12],
    };
    assert_eq!((second.vc(), second.fhp()), (1, 201));
    assert_eq!(
        second.data()[201..207],
        [0x07, 0xFF, 0xC0, 0x00, 0x00, 0x03]
    );
    // Nothing of VC 1 in between.
    assert!(frames[5..12].iter().all(|f| Frame { octets: f }.vc() != 1));
}

#[test]
fn the_spanning_dump_and_payload_schedule_matches_the_hand_derivation() {
    // req: APP-033, APP-137
    let frames = nominal().frames();
    // §4.4.4: frame, VC 1 count, FHP.
    for (i, count, fhp) in [
        (97, 12, 0),
        (99, 13, 69),
        (100, 14, 138),
        (101, 15, 33),
        (103, 16, 0x7FF),
        (110, 17, 23),
    ] {
        let f = Frame { octets: &frames[i] };
        assert_eq!(
            (f.vc(), f.vc_count(), f.fhp()),
            (1, count, fhp),
            "frame {i}"
        );
    }
    // Slot 102: both VCs hold a field, VC 0 goes first (event packet, idle of 190).
    let f = Frame {
        octets: &frames[102],
    };
    assert_eq!((f.vc(), f.fhp()), (0, 0));
    assert_eq!(f.data()[21..27], [0x07, 0xFF, 0xC0, 0x00, 0x00, 0xB7]);
    // Frame 110 ends with an idle packet of 188 octets after the last 23 octets of payload 6.
    let f = Frame {
        octets: &frames[110],
    };
    assert_eq!(f.data()[23..29], [0x07, 0xFF, 0xC0, 0x00, 0x00, 0xB5]);
}

#[test]
fn totals_and_counts_match_the_hand_derivation() {
    // req: APP-035, APP-034, APP-137
    let frames = nominal().frames();
    let by_vc = |vc: u8| {
        frames
            .iter()
            .filter(|f| Frame { octets: f }.vc() == vc)
            .count()
    };
    assert_eq!((by_vc(0), by_vc(1), by_vc(7)), (31, 30, 219));
    // MC count: frame i has i mod 256, so the last (279) has 23; VC counts count each VC's frames.
    for (i, octets) in frames.iter().enumerate() {
        assert_eq!(usize::from(Frame { octets }.mc_count()), i % 256);
    }
    for vc in [0, 1, 7] {
        let counts: Vec<u8> = frames
            .iter()
            .map(|f| Frame { octets: f })
            .filter(|f| f.vc() == vc)
            .map(|f| f.vc_count())
            .collect();
        for (n, c) in counts.iter().enumerate() {
            assert_eq!(usize::from(*c), n % 256);
        }
    }
    let last = Frame {
        octets: &frames[279],
    };
    assert_eq!(last.mc_count(), 23);
}

#[test]
fn the_packets_in_the_fields_are_the_packets_generated() {
    // req: APP-032, APP-033, APP-003
    let run = nominal();
    let frames = run.frames();
    for (vc, apids) in [
        (0, [apid::HOUSEKEEPING, apid::EVENTS].as_slice()),
        (1, [apid::PAYLOAD, apid::DUMP].as_slice()),
    ] {
        let sent: Vec<Vec<u8>> = run
            .rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Packet(a, p) if apids.contains(a) => Some(p.clone()),
                Out::Packet(..) | Out::Unit(_) | Out::Event(..) => None,
            })
            .collect();
        let extracted: Vec<Vec<u8>> = packets_of(&frames, vc)
            .into_iter()
            .filter(|p| apid_of(p) != 2047)
            .collect();
        assert_eq!(extracted, sent, "VC {vc}");
    }
    // The packet totals of §2.4.
    let count = |a: u16| {
        run.rec
            .0
            .iter()
            .filter(|(_, o)| matches!(o, Out::Packet(x, _) if *x == a))
            .count()
    };
    assert_eq!(
        (
            count(apid::HOUSEKEEPING),
            count(apid::EVENTS),
            count(apid::PAYLOAD),
            count(apid::DUMP)
        ),
        (30, 10, 13, 5)
    );
    assert_eq!(run.node.build_failures(), 0);
}

#[test]
fn oid_frames_use_vc_7_and_the_generator_that_is_never_restarted() {
    // req: APP-034
    let frames = nominal().frames();
    let oid: Vec<&Vec<u8>> = frames
        .iter()
        .filter(|f| Frame { octets: f }.vc() == 7)
        .collect();
    assert!(oid.iter().all(|f| Frame { octets: f }.fhp() == 0x7FE));
    // 132.0 §4.1.4.6.2 note: the sequence begins FF FF FF FF 6D B6 D8 61 45 1F.
    assert_eq!(
        Frame { octets: oid[0] }.data()[..10],
        [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6, 0xD8, 0x61, 0x45, 0x1F]
    );
    // The sequence continues across OID frames: the second one is not the first again.
    assert_ne!(
        Frame { octets: oid[1] }.data(),
        Frame { octets: oid[0] }.data()
    );
}

#[test]
fn the_ocf_is_the_initial_clcw_in_every_frame_oid_frames_included() {
    // req: APP-031, APP-134
    // No uplink frame is delivered, so FARM-1 reports the initial word.
    let mut run = Run::new();
    run.run(&[]);
    let frames = run.frames();
    assert!(
        frames
            .iter()
            .all(|f| Frame { octets: f }.ocf() == [0x01, 0, 0, 0])
    );
    assert_eq!(tm::OCF_INITIAL.to_be_bytes(), [0x01, 0, 0, 0]);
}

#[test]
fn the_clcw_after_an_uplink_frame_is_in_the_next_downlink_frame_and_stays() {
    // req: APP-134, APP-043
    let mut run = Run::new();
    run.step(0, true, &[]);
    // A Type-BD frame delivered at the slot is accepted in step 1, so FARM-B counter 1 is in the
    // frame of the same slot (§4.4.1).
    let noop = command(0, 1, 6_000_000, &[]);
    let uplink = bd_frame(0, TcFlags::Unsegmented, 0, &noop);
    run.step(SLOT, true, &[uplink]);
    run.step(2 * SLOT, true, &[]);
    let frames = run.frames();
    let ocfs: Vec<[u8; 4]> = frames.iter().map(|f| Frame { octets: f }.ocf()).collect();
    assert_eq!(
        ocfs,
        [[1, 0, 0, 0], [1, 0, 2, 0], [1, 0, 2, 0]],
        "FARM-B counter 1 is 0x200 in the word"
    );
    assert_eq!(Frame { octets: &frames[2] }.vc(), 0);
    for f in &frames {
        verify_fecf(f).unwrap();
    }
}

#[test]
fn the_deadline_is_the_generators_and_never_in_the_past() {
    // req: APP-135, APP-074
    let mut run = Run::new();
    // HK 0 starts a VC 0 field at 0: deadline 0.25 s, but the periodic payload at 0.5 s comes
    // later, so the next deadline is the field's.
    let next = run.step(0, true, &[]);
    assert_eq!(next, Some(250_000));
    // After the slot at 0.25 s releases it, the next wake-up is the payload at 0.5 s.
    run.step(125_000, true, &[]);
    let next = run.step(250_000, true, &[]);
    assert_eq!(next, Some(500_000));
    assert_eq!(tm::RELEASE_DEADLINE_REALTIME_US, 250_000);
}

#[test]
fn a_step_off_the_grid_past_a_deadline_reports_the_next_slot() {
    // req: APP-167, APP-090, APP-135
    let mut run = Run::new();
    // The payload packet of 0.5 s is generated at 500 600 µs off the grid: its VC 1 field
    // starts there, with the deadline at 1 500 600 µs.
    run.step(0, true, &[]);
    let started = run.step(500_600, false, &[]);
    assert!(started.is_some_and(|next| next > 500_600));
    // A step at 1 501 000 µs without `tx_slot` is past that deadline, which only a slot step
    // releases: the report is the next slot instant, 1 625 000 µs, not the deadline.
    let next = run.step(1_501_000, false, &[]).unwrap();
    assert!(next > 1_501_000, "not after now: {next}");
    assert_eq!(next, 1_625_000);
    // A step on the grid reports after its own instant.
    let next = run.step(1_625_000, true, &[]).unwrap();
    assert!(next > 1_625_000, "not after now: {next}");
}

#[test]
fn extra_wakeups_between_slots_change_nothing() {
    // req: APP-074, APP-135
    let plain = nominal();
    let mut busy = Run::new();
    let commands = nominal_commands();
    for step in 0..END / 25_000 {
        let now = step.saturating_mul(25_000);
        let delivered: Vec<Vec<u8>> = commands
            .iter()
            .filter(|(at, _)| *at == now)
            .map(|(_, c)| c.clone())
            .collect();
        busy.step(now, now % SLOT == 0, &delivered);
    }
    assert_eq!(plain.rec.0, busy.rec.0);
}

#[test]
fn a_packet_node_still_outputs_its_packets_as_units() {
    // req: APP-090
    let mut node = FlightNode::new();
    let mut rec = Rec::default();
    let input = StepInput {
        now: Instant::from_micros(0),
        delivered: Delivered::new(&[]),
        ops: &[],
        tx_slot: false,
    };
    node.step(&input, &mut At(0, &mut rec));
    assert!(matches!(rec.0.last(), Some((_, Out::Unit(u))) if u.len() == 44));
    assert_eq!(node.status().payload_mode, PayloadMode::On);
}

#[test]
fn the_engines_fit_the_frame_and_the_queue_is_not_the_limit() {
    // req: APP-030, APP-033
    assert_eq!(usize::from(tm::FRAME_LENGTH), 223);
    let run = nominal();
    assert_eq!(run.node.build_failures(), 0);
    assert!(
        run.rec
            .0
            .iter()
            .all(|(_, o)| !matches!(o, Out::Event(113, _)))
    );
}

#[test]
fn a_packet_a_generator_cannot_take_raises_downlink_refused_and_is_counted() {
    // req: APP-142, APP-131
    // Without frame slots nothing leaves the generators; each holds 211 + 512 = 723 octets. VC 1
    // takes the payload packet of 0.5 s (412) and refuses the next, at 2.5 s, and every later
    // one (14 in all); VC 0 takes 16 housekeeping packets (704) and refuses those of seconds 16
    // to 29 (14), each with `WouldBlock`.
    let mut run = Run::new();
    for step in 0_u64..60 {
        run.step(step.saturating_mul(500_000), false, &[]);
    }
    let refused: Vec<(u64, Vec<u64>)> = run
        .rec
        .0
        .iter()
        .filter_map(|(t, o)| match o {
            Out::Event(113, args) => Some((*t, args.clone())),
            Out::Packet(..) | Out::Unit(_) | Out::Event(..) => None,
        })
        .collect();
    let would_block = u64::from(aldis::core::ErrorKind::WouldBlock.code());
    assert_eq!(refused.len(), 28);
    assert_eq!(refused[0], (2_500_000, std::vec![would_block, 1]));
    assert!(refused.contains(&(16_000_000, std::vec![would_block, 0])));
    assert_eq!(run.node.build_failures(), 28);
    // The packets were built: 30 `packet_out` records, and no frame without a slot.
    let built = run
        .rec
        .0
        .iter()
        .filter(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::HOUSEKEEPING))
        .count();
    assert_eq!(built, 30);
    assert_eq!(run.frames().len(), 0);
}

#[test]
fn a_failed_slot_raises_downlink_refused_with_the_error_code_alone() {
    // req: APP-142
    let mut node = FlightNode::new_frames(Variant::TmTc).unwrap();
    let mut rec = Rec::default();
    node.slot_failed(aldis::core::ErrorKind::InternalError, &mut At(0, &mut rec));
    let code = u64::from(aldis::core::ErrorKind::InternalError.code());
    assert_eq!(rec.0, [(0, Out::Event(113, std::vec![code]))]);
    assert_eq!(node.build_failures(), 1);
}

// The AOS downlink (A4 WP-3a, docs/reference-application.md §4.3.1, APP-169…APP-173).

use aldis::aos::{
    AosEdition, AosFrame, AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent,
    DataUnit, PacketPointer, VcTable, fhec,
};
use aldis::core::ids::{SpacecraftId, VcId};
use refapp_scenario::generated::aos;

fn aos_run() -> Run {
    nominal_of(Variant::Aos)
}

/// The channels of §4.3.1 for the strict decode of the test.
fn aos_channels() -> (AosPhysicalChannelConfig, VcTable<3>) {
    let physical = AosPhysicalChannelConfig::new(
        usize::from(aos::FRAME_LENGTH),
        aos::FHEC,
        0,
        aos::FECF,
        AosEdition::Issue5,
    )
    .unwrap();
    let scid = SpacecraftId::<10>::new(SPACECRAFT_ID).unwrap();
    let vc = |id: u8| {
        AosVirtualChannelConfig::new(scid, VcId::<6>::new(id).unwrap(), DataFieldContent::Mpdu)
            .unwrap()
            .with_ocf()
    };
    let table = VcTable::new(
        physical,
        &[
            vc(aos::VCID_REALTIME),
            vc(aos::VCID_PAYLOAD),
            vc(aos::VCID_IDLE),
        ],
    )
    .unwrap();
    (physical, table)
}

/// What a strict decode of an AOS frame holds: the VCID, the pointer, the packet zone, and the
/// OCF.
struct AosView {
    vc: u8,
    pointer: PacketPointer,
    zone: Vec<u8>,
    ocf: [u8; 4],
}

fn aos_view(octets: &[u8]) -> AosView {
    let (physical, table) = aos_channels();
    let frame = AosFrame::decode(octets, &physical, &table).unwrap();
    let DataUnit::Mpdu(mpdu) = frame.data_unit() else {
        panic!("an M_PDU")
    };
    AosView {
        vc: frame.vcid().get(),
        pointer: mpdu.pointer(),
        zone: mpdu.packet_zone().to_vec(),
        ocf: frame.ocf().unwrap().to_octets(),
    }
}

/// The packets of `vc` in order, cut from the concatenated packet zones by the length field of
/// each (133.0 §4.1.3.5), Idle Packets included, and the offsets in the VC's stream at which each
/// starts.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on offsets inside a stream that was just measured"
)]
fn aos_packets_of(frames: &[Vec<u8>], vc: u8) -> (Vec<Vec<u8>>, Vec<usize>) {
    let stream: Vec<u8> = frames
        .iter()
        .map(|f| aos_view(f))
        .filter(|v| v.vc == vc)
        .flat_map(|v| v.zone)
        .collect();
    let (mut packets, mut starts) = (Vec::new(), Vec::new());
    let mut at = 0;
    while at + 6 <= stream.len() {
        let len = usize::from(u16::from_be_bytes([stream[at + 4], stream[at + 5]])) + 7;
        starts.push(at);
        packets.push(stream[at..at + len].to_vec());
        at += len;
    }
    assert_eq!(at, stream.len());
    (packets, starts)
}

#[test]
fn aos_frames_have_the_geometry_and_decode_strictly_with_fhec_and_fecf() {
    // req: APP-169, APP-171, LNK-035
    let run = aos_run();
    let frames = run.frames();
    assert_eq!(frames.len(), 280);
    for octets in &frames {
        assert_eq!(octets.len(), 223);
        // The strict decode corrects the FHEC and checks the FECF; no symbol was corrected.
        let (physical, table) = aos_channels();
        let frame = AosFrame::decode(octets, &physical, &table).unwrap();
        assert_eq!(frame.corrected_symbols().get(), 0);
        assert_eq!(frame.scid().get(), SPACECRAFT_ID);
        assert_eq!(frame.insert_zone(), []);
        // Version 01 (`0b01` in the top bits), the SCID, and the VCID: octets 0 and 1.
        assert_eq!(octets[0] >> 6, 0b01);
        // The FHEC is the code of §4.1.2.6.5 over octets 0, 1, and 5 (not the VCF Count),
        // computed after the header was final, and the FECF the CRC over the whole frame, FHEC included: LNK-035.
        let header: [u8; 6] = octets[..6].try_into().unwrap();
        assert_eq!(octets[6..8], fhec::encode(&header));
        verify_fecf(octets).unwrap();
        // Primary header 6, FHEC 2, M_PDU 209 (header 2, zone 207), VC_OCF 4, FECF 2.
        assert_eq!(frame.data_field().len(), 209);
        assert_eq!(
            octets[8 + 209..8 + 209 + 4],
            frame.ocf().unwrap().to_octets()
        );
        assert!(!frame.is_oid());
    }
    assert_eq!(run.node.build_failures(), 0);
}

#[test]
fn the_aos_vc_frame_count_starts_at_0_and_counts_each_vc_idle_frames_included() {
    // req: APP-171, APP-035
    let frames = aos_run().frames();
    let mut next = [0_u32; 8];
    for octets in &frames {
        let (physical, table) = aos_channels();
        let frame = AosFrame::decode(octets, &physical, &table).unwrap();
        let vc = usize::from(frame.vcid().get());
        assert_eq!(frame.vc_frame_count().get(), next[vc], "VC {vc}");
        next[vc] += 1;
        // Replay Flag 0, VC Frame Count Cycle Use Flag 0, cycle field 0000 (APP-171).
        assert!(!frame.replay_flag());
        assert_eq!(frame.vc_frame_count_cycle(), None);
        // Octet 5: Replay Flag, Use Flag, two spare bits that Issue 5 gives to the SCID (0x155
        // has `01` above its low 8 bits), and the cycle field.
        assert_eq!(octets[5], 0b0001_0000);
    }
    // Each VC counted from 0: the three VCs all sent frames, and VC 7's idle frames count.
    assert!(next[0] > 0 && next[1] > 0 && next[7] > 0);
    assert_eq!(next.iter().sum::<u32>(), 280);
}

#[test]
fn idle_slots_are_an_idle_m_pdu_on_vc_7_and_no_frame_is_an_oid_frame() {
    // req: APP-170, APP-172
    let frames = aos_run().frames();
    // Slots 0 and 1: nothing is released (the first deadline is the 250 ms of VC 0), so VC 7
    // sends an idle M_PDU: "only idle data" and the idle pattern in the zone.
    for octets in &frames[..2] {
        let v = aos_view(octets);
        assert_eq!(v.vc, aos::VCID_IDLE);
        assert_eq!(v.pointer, PacketPointer::OnlyIdleData);
        assert_eq!(v.zone.len(), 207);
        assert!(v.zone.iter().all(|b| *b == aos::IDLE_PATTERN));
        // First octets of the M_PDU: the pointer "all ones minus one", 16 bits in Issue 5
        // (732.0 §4.1.4.2.2.5).
        assert_eq!(octets[8..10], [0xFF, 0xFE]);
    }
    // VC 0's field released at its deadline at slot 2 goes before VC 7's: priority.
    assert_eq!(aos_view(&frames[2]).vc, aos::VCID_REALTIME);
    assert_eq!(
        aos_view(&frames[2]).pointer,
        PacketPointer::StartsAt(0.try_into().unwrap())
    );
    // Every VC 7 frame is idle, and no frame carries VC 63.
    for octets in &frames {
        let v = aos_view(octets);
        assert_ne!(octets[1] & 0x3F, 63);
        if v.vc == aos::VCID_IDLE {
            assert_eq!(v.pointer, PacketPointer::OnlyIdleData);
        } else {
            assert_ne!(v.pointer, PacketPointer::OnlyIdleData);
        }
    }
}

#[test]
fn the_aos_packet_zones_hold_the_packets_generated_and_the_pointer_names_the_first_start() {
    // req: APP-172, APP-033, APP-003
    let run = aos_run();
    let frames = run.frames();
    for (vc, apids) in [
        (
            aos::VCID_REALTIME,
            [apid::HOUSEKEEPING, apid::EVENTS].as_slice(),
        ),
        (aos::VCID_PAYLOAD, [apid::PAYLOAD, apid::DUMP].as_slice()),
    ] {
        let sent: Vec<Vec<u8>> = run
            .rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Packet(a, p) if apids.contains(a) => Some(p.clone()),
                Out::Packet(..) | Out::Unit(_) | Out::Event(..) => None,
            })
            .collect();
        let (packets, starts) = aos_packets_of(&frames, vc);
        let extracted: Vec<Vec<u8>> = packets
            .iter()
            .filter(|p| apid_of(p) != 2047)
            .cloned()
            .collect();
        assert_eq!(extracted, sent, "VC {vc}");
        // An Idle Packet completes a field released at its deadline: APID 2047, idle data 0x00,
        // sequence flags 11 and count 0 (APP-172, OI-90).
        for idle in packets.iter().filter(|p| apid_of(p) == 2047) {
            assert_eq!(idle[2..4], [0xC0, 0x00]);
            assert!(idle[6..].iter().all(|b| *b == aos::IDLE_PATTERN));
        }
        // The pointer of each field of the VC is the offset of the first packet that starts in
        // its zone, or "all ones" when none does (APP-172); the oracle is the starts of the
        // packets in the concatenated zones.
        let mut base = 0;
        for v in frames.iter().map(|f| aos_view(f)).filter(|v| v.vc == vc) {
            let first = starts
                .iter()
                .find(|s| (base..base + 207).contains(*s))
                .map(|s| s - base);
            let expected = match first {
                Some(offset) => {
                    PacketPointer::StartsAt(u16::try_from(offset).unwrap().try_into().unwrap())
                }
                None => PacketPointer::NoPacketStart,
            };
            assert_eq!(v.pointer, expected, "VC {vc}, zone at {base}");
            base += 207;
        }
    }
    // The packet totals of §2.4, as for TM (APP-003).
    let count = |a: u16| {
        run.rec
            .0
            .iter()
            .filter(|(_, o)| matches!(o, Out::Packet(x, _) if *x == a))
            .count()
    };
    assert_eq!(
        (
            count(apid::HOUSEKEEPING),
            count(apid::EVENTS),
            count(apid::PAYLOAD),
            count(apid::DUMP)
        ),
        (30, 10, 13, 5)
    );
    assert_eq!(run.node.build_failures(), 0);
    assert!(
        run.rec
            .0
            .iter()
            .all(|(_, o)| !matches!(o, Out::Event(113, _)))
    );
}

#[test]
fn the_vc_ocf_is_the_clcw_of_the_slot_in_every_frame_of_every_vc() {
    // req: APP-173, APP-134
    // No uplink frame: FARM-1 reports the initial word in every frame, VC 7's idle frames and
    // the data VCs' alike.
    let mut run = Run::of(Variant::Aos);
    run.run(&[]);
    let views: Vec<AosView> = run.frames().iter().map(|f| aos_view(f)).collect();
    assert!(views.iter().any(|v| v.vc == 0) && views.iter().any(|v| v.vc == 1));
    assert!(views.iter().any(|v| v.vc == 7));
    assert!(views.iter().all(|v| v.ocf == [0x01, 0, 0, 0]));
    // A Type-BD frame accepted in step 1 of slot 1 is in the frame of that slot and every later
    // one, whatever the VC (the word has Type Flag 0: a Type-1 report).
    let mut run = Run::of(Variant::Aos);
    let noop = command(0, 1, 6_000_000, &[]);
    let uplink = bd_frame(0, TcFlags::Unsegmented, 0, &noop);
    run.step(0, true, &[]);
    for slot in 1..END / SLOT {
        let delivered = if slot == 1 {
            std::vec![uplink.clone()]
        } else {
            Vec::new()
        };
        run.step(slot * SLOT, true, &delivered);
    }
    let views: Vec<AosView> = run.frames().iter().map(|f| aos_view(f)).collect();
    assert_eq!(views[0].ocf, [1, 0, 0, 0]);
    assert!(views[1..].iter().all(|v| v.ocf == [1, 0, 2, 0]));
    assert!(views[1..].iter().all(|v| v.ocf[0] >> 7 == 0));
    assert!(views[1..].iter().any(|v| v.vc == 0));
    assert!(views[1..].iter().any(|v| v.vc == 1));
    assert!(views[1..].iter().any(|v| v.vc == 7));
}

#[test]
fn the_aos_node_takes_the_cop1_level_too() {
    // req: APP-169, APP-187
    let mut node = FlightNode::new_cop1(Variant::Aos).unwrap();
    let mut rec = Rec::default();
    let input = StepInput {
        now: Instant::from_micros(0),
        delivered: Delivered::new(&[]),
        ops: &[],
        tx_slot: true,
    };
    node.step(&input, &mut At(0, &mut rec));
    let frames: Vec<&Vec<u8>> = rec
        .0
        .iter()
        .filter_map(|(_, o)| match o {
            Out::Unit(u) => Some(u),
            Out::Packet(..) | Out::Event(..) => None,
        })
        .collect();
    assert_eq!(frames.len(), 1);
    assert_eq!(aos_view(frames[0]).vc, aos::VCID_IDLE);
}
