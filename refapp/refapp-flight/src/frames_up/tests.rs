//! Tests of the uplink at frame level through the node interface (docs/reference-application.md
//! §4.2, §4.4.1, §4.4.4, §7.4).

extern crate std;

use std::vec::Vec;

use aldis::core::ErrorKind;
use aldis::core::ids::{Apid, FrameSeqNum, SeqCount};
use aldis::core::security::SecurityLayout;
use aldis::spp::{PacketFields, PacketType, SpacePacketWriter, SppConfig};
use aldis::tc::{
    FrameData, FrameFields, FrameType, MapConfig, PhysicalChannelConfig, SegmentHeader,
    SegmentHeaderUse, SequenceFlags, TcMapId, TcSpacecraftId, TcVcId, VcConfig, VcTable, encode,
};
use refapp_scenario::generated::{SPACECRAFT_ID, apid};
use refapp_scenario::{Delivered, Instant, Node, StepInput};

use super::APP_FRAME_REJECTED;
use super::APP_SEQUENCE_VIOLATION;
use crate::FlightNode;
use crate::tests::{At, Out, Rec};

/// `cmd_received` and `table_received` (§7.4).
const CMD_RECEIVED: u16 = 20;
const TABLE_RECEIVED: u16 = 21;
const PACKET_REJECTED: u16 = 6;
const DOWNLINK_REFUSED: u16 = 113;

/// The encoder's view of the uplink: VC 0 with MAPs 0, 1, and 5, and a VC 1 the node does not
/// serve, so a test can send a frame the node's own table refuses.
fn sender() -> VcTable<2, 4> {
    let scid = TcSpacecraftId::new(SPACECRAFT_ID).unwrap();
    let vc = |id: u8| {
        VcConfig::new(
            scid,
            TcVcId::new(id).unwrap(),
            128,
            SegmentHeaderUse::Present,
        )
        .unwrap()
    };
    let map = |vc: u8, id: u8| {
        MapConfig::new(
            scid,
            TcVcId::new(vc).unwrap(),
            TcMapId::new(id).unwrap(),
            SecurityLayout::NONE,
        )
    };
    VcTable::new(
        PhysicalChannelConfig::new(128, true).unwrap(),
        &[vc(0), vc(1)],
        &[map(0, 0), map(0, 1), map(0, 5), map(1, 0)],
    )
    .unwrap()
}

/// A Type-BD frame (N(S) = 0) on `vc` with a Segment Header (§4.4.4).
pub(crate) fn bd_frame(vc: u8, flags: SequenceFlags, map: u8, fdu: &[u8]) -> Vec<u8> {
    let fields = FrameFields {
        frame_type: FrameType::Bd,
        spacecraft_id: TcSpacecraftId::new(SPACECRAFT_ID).unwrap(),
        vc_id: TcVcId::new(vc).unwrap(),
        sequence_number: FrameSeqNum::new(0),
    };
    let data = FrameData::Fdu {
        segment_header: Some(SegmentHeader {
            sequence_flags: flags,
            map_id: TcMapId::new(map).unwrap(),
        }),
        user_data: fdu,
    };
    let mut out = [0_u8; 128];
    let n = encode(&sender(), fields, data, &mut out).unwrap();
    out[..n].to_vec()
}

fn packet(apid_value: u16, seq: u16, data: &[u8]) -> Vec<u8> {
    let mut buffer = [0_u8; 400];
    let fields = PacketFields {
        packet_type: PacketType::Telecommand,
        apid: Apid::new(apid_value).unwrap(),
        sequence_flags: aldis::spp::SequenceFlags::Unsegmented,
        sequence_count: SeqCount::new(seq).unwrap(),
    };
    let mut w =
        SpacePacketWriter::begin(&mut buffer, &SppConfig::DEFAULT, fields, 0, data.len()).unwrap();
    w.user_data_mut().copy_from_slice(data);
    let n = w.finish(data.len()).unwrap();
    buffer[..n].to_vec()
}

/// A time-tagged `NOOP` at 6 s (§3.3.5, §2.4): 14 octets.
fn noop(seq: u16) -> Vec<u8> {
    command(seq, 1, &[])
}

fn command(seq: u16, function: u8, args: &[u8]) -> Vec<u8> {
    // 6 s after T0 = 2 100 000 006 s = 0x7D2B7506.
    let mut data = std::vec![function, 1, 0x7D, 0x2B, 0x75, 0x06, 0, 0];
    data.extend_from_slice(args);
    packet(apid::COMMANDS, seq, &data)
}

/// The table-upload packet of the nominal run (§3.3.6, §2.4): 309 octets.
fn table_packet() -> Vec<u8> {
    let mut data = std::vec![1, 0x01, 0x2C];
    data.extend(
        (0..300_usize).map(|i| u8::try_from(i.wrapping_mul(3).wrapping_add(1) % 256).unwrap()),
    );
    packet(apid::TABLE_UPLOAD, 0, &data)
}

/// The node with a frame uplink and the output of its steps.
struct Run {
    node: FlightNode,
    rec: Rec,
}

impl Run {
    fn new() -> Run {
        Run {
            node: FlightNode::new_frames().unwrap(),
            rec: Rec::default(),
        }
    }

    /// Delivers `frames` in one step at `now` µs.
    fn deliver(&mut self, now: u64, frames: &[Vec<u8>]) {
        let refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
        let input = StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(&refs),
            ops: &[],
            tx_slot: false,
        };
        self.node.step(&input, &mut At(now, &mut self.rec));
    }

    /// The application events of the uplink output so far, as (code, args), and forgets them. The
    /// `downlink_refused` events (113) are left out: a step at 5 s catches up with the packets of
    /// 5 s at once, which the downlink without slots cannot take (APP-142).
    fn app_events(&mut self) -> Vec<(u16, Vec<u64>)> {
        let events = self
            .rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Event(code, args) if *code != DOWNLINK_REFUSED => Some((*code, args.clone())),
                Out::Event(..) | Out::Unit(_) | Out::Packet(..) => None,
            })
            .collect();
        self.rec = Rec::default();
        events
    }

    /// The OCF word FARM-1 reports (§4.4.1).
    fn ocf_word(&self) -> u32 {
        self.node.clcw().unwrap().to_word()
    }
}

fn code(kind: ErrorKind) -> u64 {
    u64::from(kind.code())
}

#[test]
fn the_clcw_starts_as_the_initial_ocf_and_exists_only_at_frame_level() {
    // req: APP-134, APP-031
    assert_eq!(Run::new().ocf_word(), 0x0100_0000);
    assert_eq!(FlightNode::new().clcw(), None);
}

#[test]
fn a_command_frame_reaches_the_handler_and_counts_in_the_farm_b_counter() {
    // req: APP-040, APP-041, APP-043, APP-139, APP-134
    let mut run = Run::new();
    let frame = bd_frame(0, SequenceFlags::Unsegmented, 0, &noop(0));
    // §4.4.4, index 0: `21 55 00 15 00` and the Segment Header `C0`; 22 octets.
    assert_eq!(frame.len(), 22);
    assert_eq!(frame[..6], [0x21, 0x55, 0x00, 0x15, 0x00, 0xC0]);
    run.deliver(1_010_000, &[frame]);
    assert_eq!(run.app_events(), [(CMD_RECEIVED, std::vec![1, 0])]);
    assert_eq!(run.ocf_word(), 0x0100_0200);
}

#[test]
fn the_farm_b_counter_wraps_modulo_four() {
    // req: APP-043, APP-134
    let mut run = Run::new();
    let frame = bd_frame(0, SequenceFlags::Unsegmented, 0, &noop(0));
    let words: Vec<u32> = (0..5)
        .map(|i| {
            run.deliver(1_000_000 + i * 250_000, std::slice::from_ref(&frame));
            run.ocf_word()
        })
        .collect();
    assert_eq!(
        words,
        [
            0x0100_0200,
            0x0100_0400,
            0x0100_0600,
            0x0100_0000,
            0x0100_0200
        ]
    );
}

#[test]
fn two_commands_in_one_fdu_are_delivered_in_order() {
    // req: APP-042, APP-139
    let mut run = Run::new();
    let mut fdu = command(3, 4, &[0]);
    fdu.extend(noop(4));
    let frame = bd_frame(0, SequenceFlags::Unsegmented, 0, &fdu);
    // §4.4.4, index 6: 30 octets of FDU, 37 of frame.
    assert_eq!((fdu.len() + 1, frame.len()), (30, 37));
    run.deliver(5_010_000, &[frame]);
    assert_eq!(
        run.app_events(),
        [
            (CMD_RECEIVED, std::vec![4, 3]),
            (CMD_RECEIVED, std::vec![1, 4])
        ]
    );
    assert_eq!(run.ocf_word(), 0x0100_0200);
}

#[test]
fn a_table_upload_in_three_segments_is_stored() {
    // req: APP-042, APP-017, APP-139
    let mut run = Run::new();
    let table = table_packet();
    assert_eq!(table.len(), 309);
    let frames = [
        bd_frame(0, SequenceFlags::First, 1, &table[..120]),
        bd_frame(0, SequenceFlags::Continuing, 1, &table[120..240]),
        bd_frame(0, SequenceFlags::Last, 1, &table[240..]),
    ];
    // §4.4.4, indexes 1 to 3: 128, 128, and 77 octets, Segment Headers 41, 01, 81.
    assert_eq!(
        frames.iter().map(|f| (f.len(), f[5])).collect::<Vec<_>>(),
        [(128, 0x41), (128, 0x01), (77, 0x81)]
    );
    for (i, frame) in frames.iter().enumerate() {
        run.deliver(
            2_010_000 + 250_000 * u64::try_from(i).unwrap(),
            std::slice::from_ref(frame),
        );
        let expected = if i == 2 {
            std::vec![(TABLE_RECEIVED, std::vec![1, 300])]
        } else {
            Vec::new()
        };
        assert_eq!(run.app_events(), expected);
    }
    assert_eq!(run.ocf_word(), 0x0100_0600);
}

#[test]
fn a_frame_for_another_vc_is_rejected_and_not_counted() {
    // req: APP-139, APP-041, TC-021
    let mut run = Run::new();
    run.deliver(
        1_010_000,
        &[bd_frame(1, SequenceFlags::Unsegmented, 0, &noop(0))],
    );
    assert_eq!(
        run.app_events(),
        [(
            APP_FRAME_REJECTED,
            std::vec![code(ErrorKind::UnknownChannel)]
        )]
    );
    assert_eq!(run.ocf_word(), 0x0100_0000);
}

#[test]
fn a_frame_for_another_map_is_rejected_and_not_counted() {
    // req: APP-139, APP-041, TC-021, TC-022
    // The strict codec refuses the MAP ID at validation, so FARM-1 does not count the frame and
    // `unknown_map` (111) is not raised. The Blue Book reads otherwise (232.0 §4.4.3.3,
    // §4.4.9.3.3): OI-98.
    let mut run = Run::new();
    run.deliver(
        1_010_000,
        &[bd_frame(0, SequenceFlags::Unsegmented, 5, &noop(0))],
    );
    assert_eq!(
        run.app_events(),
        [(
            APP_FRAME_REJECTED,
            std::vec![code(ErrorKind::UnknownChannel)]
        )]
    );
    assert_eq!(run.ocf_word(), 0x0100_0000);
}

#[test]
fn a_corrupt_frame_is_rejected_before_farm_1_and_the_next_frame_is_accepted() {
    // req: APP-139, TC-021, TC-023, APP-043
    let mut run = Run::new();
    let mut bad = bd_frame(0, SequenceFlags::Unsegmented, 0, &noop(0));
    // §6.7, `a2-uplink-faults` index 0: octet 12, bit 7.
    bad[12] ^= 0x80;
    run.deliver(1_010_000, &[bad]);
    assert_eq!(
        run.app_events(),
        [(APP_FRAME_REJECTED, std::vec![code(ErrorKind::CrcMismatch)])]
    );
    assert_eq!(run.ocf_word(), 0x0100_0000);
    run.deliver(
        2_010_000,
        &[bd_frame(0, SequenceFlags::Unsegmented, 0, &noop(1))],
    );
    assert_eq!(run.app_events(), [(CMD_RECEIVED, std::vec![1, 1])]);
    assert_eq!(run.ocf_word(), 0x0100_0200);
}

#[test]
fn a_truncated_frame_is_rejected_with_a_length_error() {
    // req: APP-139, TC-007
    let mut run = Run::new();
    let frame = bd_frame(0, SequenceFlags::Unsegmented, 0, &noop(0));
    run.deliver(1_010_000, &[frame[..10].to_vec()]);
    assert_eq!(
        run.app_events(),
        [(
            APP_FRAME_REJECTED,
            std::vec![code(ErrorKind::LengthMismatch)]
        )]
    );
    assert_eq!(run.ocf_word(), 0x0100_0000);
}

#[test]
fn a_lost_segment_gives_packet_rejected_and_no_table() {
    // req: APP-139, APP-131, LNK-070
    let mut run = Run::new();
    let table = table_packet();
    // §6.7, `a2-uplink-faults`: the second segment is dropped.
    run.deliver(
        2_010_000,
        &[bd_frame(0, SequenceFlags::First, 1, &table[..120])],
    );
    run.deliver(
        2_510_000,
        &[bd_frame(0, SequenceFlags::Last, 1, &table[240..])],
    );
    assert_eq!(
        run.app_events(),
        [(PACKET_REJECTED, std::vec![code(ErrorKind::LengthMismatch)])]
    );
    // Both frames were accepted by FARM-1 (§4.4.1).
    assert_eq!(run.ocf_word(), 0x0100_0400);
}

#[test]
fn a_segment_out_of_sequence_is_a_sequence_violation() {
    // req: APP-139, LNK-070
    let mut run = Run::new();
    let table = table_packet();
    // A continuing portion with no first portion: the reassembler's `SequenceViolation`.
    run.deliver(
        2_010_000,
        &[bd_frame(0, SequenceFlags::Continuing, 1, &table[120..240])],
    );
    assert_eq!(run.app_events(), [(APP_SEQUENCE_VIOLATION, std::vec![1])]);
    assert_eq!(run.ocf_word(), 0x0100_0200);
}

#[test]
fn a_duplicate_frame_is_accepted_twice_without_recovery() {
    // req: APP-043, APP-139, APP-134
    let mut run = Run::new();
    let frame = bd_frame(0, SequenceFlags::Unsegmented, 0, &noop(0));
    // §6.7, `a2-uplink-faults` index 6: both copies arrive; the Bypass service has no
    // duplicate detection (232.1 Table 6-1, E6).
    run.deliver(5_010_000, &[frame.clone(), frame]);
    assert_eq!(
        run.app_events(),
        [
            (CMD_RECEIVED, std::vec![1, 0]),
            (CMD_RECEIVED, std::vec![1, 0])
        ]
    );
    assert_eq!(run.ocf_word(), 0x0100_0400);
}

#[test]
fn a_packet_that_is_not_a_command_is_rejected_like_at_packet_level() {
    // req: APP-131, APP-139
    let mut run = Run::new();
    let other = packet(0x123, 0, &[1, 2, 3]);
    run.deliver(
        1_010_000,
        &[bd_frame(0, SequenceFlags::Unsegmented, 0, &other)],
    );
    assert_eq!(
        run.app_events(),
        [(PACKET_REJECTED, std::vec![code(ErrorKind::UnknownChannel)])]
    );
    assert_eq!(run.ocf_word(), 0x0100_0200);
}
