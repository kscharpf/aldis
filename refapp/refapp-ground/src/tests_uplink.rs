//! The ground node's uplink at frame level on scripted inputs (docs/reference-application.md
//! §2.2, §4.2, §4.4.4; A2 WP-4b).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use crate::{Call, Execution, GroundNode, OpsEntry};
use aldis::core::crc::Crc16;
use aldis::core::ids::{Apid, SpacecraftId, VcId};
use aldis::tc::{
    FrameType, PhysicalChannelConfig, SegmentHeaderUse, TcFrame, VcConfig, VcTable, validate_frame,
};
use refapp_scenario::generated::{SPACECRAFT_ID, tc};
use refapp_scenario::{
    Delivered, Duration, Instant, Node, Output, StepInput, TableId, TableLength,
};

/// One output of a step, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rec {
    Unit(Vec<u8>),
    PacketOut(u16, Vec<u8>),
}

#[derive(Default)]
struct Recorder(Vec<Rec>);

impl Output for Recorder {
    fn unit(&mut self, octets: &[u8]) {
        self.0.push(Rec::Unit(octets.to_vec()));
    }
    fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
        self.0.push(Rec::PacketOut(apid.get(), octets.to_vec()));
    }
    fn packet_in(&mut self, _: Apid, _: &[u8]) {}
    fn app_event(&mut self, _: u16, _: &[u64]) {}
}

const UPLINK_SLOT_US: u64 = 250_000;

const fn command(call: Call) -> OpsEntry {
    OpsEntry::Command {
        execution: Execution::TimeTagged {
            after_t0: Duration::from_secs(6),
        },
        call,
        extra_octets: 0,
    }
}

/// The ops script of `a2-nominal`'s uplink (§2.4, §4.4.4) and the instant of each entry, µs.
fn nominal() -> (Vec<OpsEntry>, Vec<u64>) {
    let script = vec![
        command(Call::Noop),
        OpsEntry::TableUpload {
            table_id: TableId::new(1).unwrap(),
            length: TableLength::new(300).unwrap(),
        },
        command(Call::ActivateTable {
            table_id: 1,
            crc: Crc16::from_wire(0xABCD),
        }),
        command(Call::DumpMemory {
            address: 0x1000,
            length: 64,
        }),
        command(Call::SetPayloadMode { mode: 0 }),
        command(Call::Noop),
        command(Call::SetPayloadMode { mode: 1 }),
    ];
    let at = vec![
        1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000, 5_000_000, 6_000_000,
    ];
    (script, at)
}

/// Steps `node` every uplink slot from 0 to `until_us` inclusive, releasing entry `i` of the
/// script at `at[i]`; returns the outputs of each step, with the instant.
fn run(node: &mut GroundNode, at: &[u64], until_us: u64) -> Vec<(u64, Vec<Rec>)> {
    let mut steps = Vec::new();
    let mut t = 0;
    while t <= until_us {
        let due: Vec<u16> = at
            .iter()
            .enumerate()
            .filter(|&(_, &when)| when == t)
            .map(|(i, _)| u16::try_from(i).unwrap())
            .collect();
        let input = StepInput {
            now: Instant::from_micros(t),
            delivered: Delivered::new(&[]),
            ops: &due,
            tx_slot: true,
        };
        let mut out = Recorder::default();
        assert_eq!(node.step(&input, &mut out), None);
        steps.push((t, out.0));
        t += UPLINK_SLOT_US;
    }
    steps
}

/// The uplink frames of a run: the units, with the instant.
fn frames(steps: &[(u64, Vec<Rec>)]) -> Vec<(u64, Vec<u8>)> {
    steps
        .iter()
        .flat_map(|(t, recs)| {
            recs.iter().filter_map(move |r| match r {
                Rec::Unit(u) => Some((*t, u.clone())),
                Rec::PacketOut(..) => None,
            })
        })
        .collect()
}

/// The TC channel of §4.2: VC 0 with a Segment Header and MAPs 0 and 1.
fn channels() -> VcTable<1, 2> {
    let scid = SpacecraftId::<10>::new(SPACECRAFT_ID).unwrap();
    let vc = VcId::<6>::new(tc::VCID).unwrap();
    let maps = [tc::MAP_COMMANDS, tc::MAP_TABLE_UPLOAD].map(|m| {
        aldis::tc::MapConfig::new(
            scid,
            vc,
            aldis::core::ids::MapId::<6>::new(m).unwrap(),
            aldis::core::security::SecurityLayout::NONE,
        )
    });
    VcTable::new(
        PhysicalChannelConfig::new(usize::from(tc::MAX_FRAME_LENGTH), tc::FECF).unwrap(),
        &[VcConfig::new(
            scid,
            vc,
            usize::from(tc::MAX_FRAME_LENGTH),
            SegmentHeaderUse::Present,
        )
        .unwrap()],
        &maps,
    )
    .unwrap()
}

fn nominal_run() -> (Vec<(u64, Vec<Rec>)>, GroundNode) {
    let (script, at) = nominal();
    let mut node = GroundNode::new(&script)
        .unwrap()
        .with_frame_uplink()
        .unwrap();
    let steps = run(&mut node, &at, 6_000_000);
    (steps, node)
}

#[test]
fn the_uplink_frames_have_the_fields_of_the_schedule() {
    // req: APP-040, APP-041, APP-043, APP-137, TC-008, COP-010
    let (steps, node) = nominal_run();
    let sent = frames(&steps);
    // (instant s, first five octets, Segment Header, frame length): §4.4.4.
    let expected: [(u64, [u8; 5], u8, usize); 8] = [
        (1_000_000, [0x21, 0x55, 0x00, 0x15, 0x00], 0xC0, 22),
        (2_000_000, [0x21, 0x55, 0x00, 0x7F, 0x00], 0x41, 128),
        (2_250_000, [0x21, 0x55, 0x00, 0x7F, 0x00], 0x01, 128),
        (2_500_000, [0x21, 0x55, 0x00, 0x4C, 0x00], 0x81, 77),
        (3_000_000, [0x21, 0x55, 0x00, 0x18, 0x00], 0xC0, 25),
        (4_000_000, [0x21, 0x55, 0x00, 0x1B, 0x00], 0xC0, 28),
        (5_000_000, [0x21, 0x55, 0x00, 0x24, 0x00], 0xC0, 37),
        (6_000_000, [0x21, 0x55, 0x00, 0x16, 0x00], 0xC0, 23),
    ];
    assert_eq!(sent.len(), expected.len());
    let table = channels();
    for ((t, frame), (when, head, segment, len)) in sent.iter().zip(expected) {
        assert_eq!((*t, frame.len()), (when, len));
        assert_eq!(frame[..5], head);
        assert_eq!(frame[5], segment);
        // Strict decoding with the FECF (TC-021): Type-BD, VC 0, N(S) = 0.
        assert!(validate_frame(frame, &table).is_ok(), "frame at {when}");
        let decoded = TcFrame::decode(frame, &table).unwrap();
        assert_eq!(decoded.frame_type(), FrameType::Bd);
        assert_eq!(decoded.spacecraft_id().get(), SPACECRAFT_ID);
        assert_eq!(decoded.vc_id().get(), tc::VCID);
        assert_eq!(decoded.sequence_number().get(), 0);
        assert!(decoded.fecf().is_some());
        assert!(decoded.segment_header().is_some());
    }
    assert_eq!(node.uplink_refused(), 0);
}

#[test]
fn the_two_commands_released_at_5_s_share_one_fdu() {
    // req: APP-042, LNK-040, LNK-071, APP-133
    let (steps, _) = nominal_run();
    let (_, at_five) = steps.iter().find(|(t, _)| *t == 5_000_000).unwrap();
    // Two packets out, in release order, then one frame as the last output of the step.
    let [
        Rec::PacketOut(a, p0),
        Rec::PacketOut(b, p1),
        Rec::Unit(frame),
    ] = at_five.as_slice()
    else {
        panic!("outputs at 5.0 s: {at_five:?}");
    };
    assert_eq!((*a, *b), (128, 128));
    assert_eq!((p0.len(), p1.len()), (15, 14));
    // Segment Header 0xC0 (unsegmented, MAP 0), then the two packets whole, then the FECF.
    assert_eq!(frame[5], 0xC0);
    assert_eq!(&frame[6..6 + 15], p0.as_slice());
    assert_eq!(&frame[6 + 15..6 + 29], p1.as_slice());
    assert_eq!(frame.len(), 5 + 1 + 29 + 2);
}

#[test]
fn the_table_upload_takes_three_frames_without_interleaving() {
    // req: LNK-041, LNK-070, APP-042
    let (script, _) = nominal();
    // A command is released at 2.0 s with the upload, and a second at 2.25 s: neither is
    // interleaved with the segments of the table packet.
    let never = u64::MAX;
    let at = [never, 2_000_000, never, never, 2_000_000, 2_250_000, never];
    let mut node = GroundNode::new(&script)
        .unwrap()
        .with_frame_uplink()
        .unwrap();
    let steps = run(&mut node, &at, 3_500_000);
    let packet = steps
        .iter()
        .flat_map(|(_, r)| r)
        .find_map(|r| match r {
            Rec::PacketOut(129, p) => Some(p.clone()),
            Rec::Unit(_) | Rec::PacketOut(..) => None,
        })
        .unwrap();
    assert_eq!(packet.len(), 309);
    let sent = frames(&steps);
    let flags_and_map: Vec<u8> = sent.iter().map(|(_, f)| f[5]).collect();
    // Segments `01`, `00`, `10` of MAP 1, then the two commands, which were both queued by the
    // time their FDU is taken and share it (APP-042).
    assert_eq!(flags_and_map, [0x41, 0x01, 0x81, 0xC0]);
    assert_eq!(sent[3].1.len(), 5 + 1 + 15 + 14 + 2);
    let instants: Vec<u64> = sent.iter().map(|(t, _)| *t).collect();
    assert_eq!(instants, [2_000_000, 2_250_000, 2_500_000, 2_750_000]);
    // The user data of the three segments rejoins the packet (120 + 120 + 69).
    let mut joined = Vec::new();
    for (_, f) in &sent[..3] {
        joined.extend_from_slice(&f[6..f.len() - 2]);
    }
    assert_eq!(joined, packet);
    assert_eq!(
        [120, 120, 69],
        [
            sent[0].1.len() - 8,
            sent[1].1.len() - 8,
            sent[2].1.len() - 8
        ]
    );
}

#[test]
fn a_packet_is_output_once_and_goes_to_the_segmenter_not_out() {
    // req: APP-011, APP-133
    let (script, at) = nominal();
    let mut packet_level = GroundNode::new(&script).unwrap();
    let before = run(&mut packet_level, &at, 6_000_000);
    let (after, _) = nominal_run();
    let packets = |steps: &[(u64, Vec<Rec>)]| -> Vec<(u64, u16, Vec<u8>)> {
        steps
            .iter()
            .flat_map(|(t, r)| {
                r.iter().filter_map(move |r| match r {
                    Rec::PacketOut(a, p) => Some((*t, *a, p.clone())),
                    Rec::Unit(_) => None,
                })
            })
            .collect()
    };
    // The `packet_out` records are those of A1 (APP-003); only the units differ.
    assert_eq!(packets(&before), packets(&after));
    assert_eq!(frames(&before).len(), 7);
    assert_eq!(frames(&after).len(), 8);
}

#[test]
fn one_frame_per_uplink_slot_and_none_without_a_slot_or_an_fdu() {
    // req: APP-071, APP-090
    let (script, _) = nominal();
    let mut node = GroundNode::new(&script)
        .unwrap()
        .with_frame_uplink()
        .unwrap();
    // The table upload released at an instant without `tx_slot` waits for the next slot, and
    // each slot hands out one FDU.
    let mut step = |t: u64, ops: &[u16], tx_slot: bool| {
        let input = StepInput {
            now: Instant::from_micros(t),
            delivered: Delivered::new(&[]),
            ops,
            tx_slot,
        };
        let mut out = Recorder::default();
        node.step(&input, &mut out);
        out.0.iter().filter(|r| matches!(r, Rec::Unit(_))).count()
    };
    assert_eq!(step(100, &[1], false), 0);
    assert_eq!(step(200, &[], false), 0);
    assert_eq!(step(250_000, &[], true), 1);
    assert_eq!(step(500_000, &[], true), 1);
    assert_eq!(step(750_000, &[], true), 1);
    assert_eq!(step(1_000_000, &[], true), 0);
}
