//! The ground node's AOS downlink on scripted frames (docs/reference-application.md §4.3.1,
//! §4.4.1, §7.4; A4 WP-4a): strict decoding, packet extraction on VC 0, 1, and 7, the CLCW of
//! every frame, and the events of APP-138 as amended.
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use super::tests::{
    CRC_MISMATCH, HK, PAYLOAD, Rec, Recorder, UNKNOWN_CHANNEL, event, field as tm_field,
    frame as tm_frame, packet,
};
use super::{
    CLCW_CHANGED, EXTRACTION_ERROR, FRAME_DISCONTINUITY, FRAME_GAP, FRAME_REJECTED, FrameDownlink,
    PARTIAL_DISCARDED,
};
use crate::{Call, Execution, Fop1Directive, GroundNode, OpsEntry};
use aldis::aos::{
    AosEdition, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig, AosVirtualChannelConfig,
    DataFieldContent, PacketOffset, PacketPointer, PduHeader, VcFrameCount, VcTable, encode,
    encode_oid,
};
use aldis::core::ids::{Apid, SeqCount, VcId};
use aldis::core::ocf::Ocf;
use aldis::core::oid::OidGenerator;
use aldis::link::protocol::Aos;
use aldis::spp::{PacketFields, PacketType, SequenceFlags, SppConfig, encode as spp_encode};
use refapp_scenario::generated::SPACECRAFT_ID;
use refapp_scenario::{Delivered, Instant, Node, Output, StepInput, Variant};

const INITIAL_OCF: [u8; 4] = [0x01, 0, 0, 0];
/// The packet zone of a frame (APP-169): 223 − 6 − 2 − 2 − 4 − 2.
const ZONE_LEN: usize = 207;
/// The idle `M_PDU` pointer's meaning: all ones minus one (APP-172).
const IDLE_POINTER: PacketPointer = PacketPointer::OnlyIdleData;
const FHEC_UNCORRECTABLE: u64 = 9;
const FOP1_TRANSFER: u16 = 201;
const FOP1_STATE: u16 = 203;
const FOP1_DIRECTIVE: u16 = 202;

/// An idle packet (APID 2047) of `len` octets, at least 7.
fn idle_packet(len: usize) -> Vec<u8> {
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        apid: Apid::new(2047).unwrap(),
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::new(0).unwrap(),
    };
    let user = vec![0; len - 6];
    let data = SppConfig::DEFAULT.packet(fields, &[], &user).unwrap();
    let mut out = vec![0; data.packet_len()];
    let n = spp_encode(&data, &mut out).unwrap();
    out.truncate(n);
    out
}

/// The packet zone that holds `parts`, filled with one Idle Packet (APP-172).
fn zone(parts: &[&[u8]]) -> Vec<u8> {
    let mut data: Vec<u8> = parts.concat();
    data.extend_from_slice(&idle_packet(ZONE_LEN - data.len()));
    data
}

const fn at(offset: u16) -> PacketPointer {
    PacketPointer::StartsAt(PacketOffset::new(offset).unwrap())
}

/// A frame of the scenario's master channel (APP-169): VC `vc`, count `count`, `zone` and `ocf`.
fn frame(vc: u8, count: u32, pointer: PacketPointer, zone: &[u8], ocf: [u8; 4]) -> Vec<u8> {
    frame_of(SPACECRAFT_ID, vc, count, pointer, zone, ocf)
}

/// As [`frame`], of the master channel `scid` whose only VC is `vc`, so that a frame of another
/// channel can be built.
fn frame_of(
    scid: u16,
    vc: u8,
    count: u32,
    pointer: PacketPointer,
    zone: &[u8],
    ocf: [u8; 4],
) -> Vec<u8> {
    let scid = SpacecraftId10::new(scid).unwrap();
    let vc = VcId::<6>::new(vc).unwrap();
    let pc = AosPhysicalChannelConfig::new(223, true, 0, true, AosEdition::Issue5).unwrap();
    let table = VcTable::<1>::new(
        pc,
        &[
            AosVirtualChannelConfig::new(scid, vc, DataFieldContent::Mpdu)
                .unwrap()
                .with_ocf(),
        ],
    )
    .unwrap();
    let fields = AosFrameFields::new(
        scid,
        vc,
        VcFrameCount::new(count).unwrap(),
        PduHeader::Mpdu(pointer),
    );
    let content = AosFrameContent {
        ocf: Some(Ocf::decode(ocf)),
        ..AosFrameContent::new(zone)
    };
    let mut out = vec![0; 223];
    let n = encode(&pc, &table, &fields, &content, &mut out).unwrap();
    assert_eq!(n, 223);
    out
}

type SpacecraftId10 = aldis::core::ids::SpacecraftId<10>;

/// An idle `M_PDU` of VC 7 (APP-170, APP-172).
fn idle_frame(count: u32, ocf: [u8; 4]) -> Vec<u8> {
    frame(7, count, IDLE_POINTER, &[0; ZONE_LEN], ocf)
}

/// Frames at the slots of the harness, one per step, and the outputs of each.
fn deliver(node: &mut GroundNode, frames: &[Vec<u8>]) -> Vec<Vec<Rec>> {
    frames
        .iter()
        .enumerate()
        .map(|(i, unit)| {
            let mut out = Recorder::default();
            let delivered: [&[u8]; 1] = [unit];
            let input = StepInput {
                now: Instant::from_micros(135_000 + 125_000 * u64::try_from(i).unwrap()),
                delivered: Delivered::new(&delivered),
                ops: &[],
                tx_slot: true,
            };
            assert_eq!(node.step(&input, &mut out), None);
            out.0
        })
        .collect()
}

fn node() -> GroundNode {
    GroundNode::new_frames(Variant::Aos, &[]).unwrap()
}

fn aos_downlink(node: &GroundNode) -> &FrameDownlink<Aos> {
    match node.downlink.as_ref() {
        Some(super::Downlink::Aos(down)) => down,
        _ => panic!("not an AOS downlink"),
    }
}

#[test]
fn a_data_frame_gives_its_packets_to_the_telemetry_path() {
    // req: APP-174, APP-172, APP-011, LNK-060
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let outs = deliver(
        &mut node(),
        &[frame(0, 0, at(0), &zone(&[&a, &b]), INITIAL_OCF)],
    );
    assert_eq!(outs, vec![vec![Rec::PacketIn(HK, a), Rec::PacketIn(HK, b)]]);
}

#[test]
fn a_packet_that_spans_two_frames_is_delivered_with_the_second() {
    // req: APP-174, APP-172, LNK-060
    let hk = packet(HK, 0, 32);
    let long = packet(PAYLOAD, 0, 300);
    let (head, tail) = long.split_at(ZONE_LEN - hk.len());
    let first = [hk.as_slice(), head].concat();
    // The residue of the packet starts the zone of the next frame; the pointer names the idle
    // packet after it.
    let tail_len = u16::try_from(tail.len()).unwrap();
    let second = [tail, &idle_packet(ZONE_LEN - tail.len())].concat();
    let outs = deliver(
        &mut node(),
        &[
            frame(1, 0, at(0), &first, INITIAL_OCF),
            frame(1, 1, at(tail_len), &second, INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![Rec::PacketIn(PAYLOAD, long)]);
}

#[test]
fn vc_0_and_vc_1_are_extracted_independently() {
    // req: APP-174, APP-170, LNK-060
    let (hk, p) = (packet(HK, 0, 32), packet(PAYLOAD, 0, 32));
    let outs = deliver(
        &mut node(),
        &[
            frame(0, 0, at(0), &zone(&[&hk]), INITIAL_OCF),
            frame(1, 0, at(0), &zone(&[&p]), INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![Rec::PacketIn(PAYLOAD, p)]);
}

#[test]
fn an_idle_m_pdu_of_vc_7_gives_no_packet_and_no_event() {
    // req: APP-174, APP-138, LNK-064
    // VC 7 has an extractor: an idle M_PDU in sequence is delivered and gives nothing,
    // `frame_discarded` included (APP-138, D10).
    let mut n = node();
    let outs = deliver(
        &mut n,
        &[idle_frame(0, INITIAL_OCF), idle_frame(1, INITIAL_OCF)],
    );
    assert_eq!(outs, vec![vec![], vec![]]);
    assert_eq!(aos_downlink(&n).receiver.status().oid_frames.get(), 0);
}

#[test]
fn an_idle_packet_in_a_data_frame_is_dropped() {
    // req: APP-174, LNK-063
    // A zone that holds only an Idle Packet, and one that holds a packet and an Idle Packet.
    let hk = packet(HK, 0, 32);
    let outs = deliver(
        &mut node(),
        &[
            frame(0, 0, at(0), &idle_packet(ZONE_LEN), INITIAL_OCF),
            frame(0, 1, at(0), &zone(&[&hk]), INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![]);
    assert_eq!(outs[1], vec![Rec::PacketIn(HK, hk)]);
}

#[test]
fn a_changed_ocf_on_any_vc_is_one_clcw_changed_before_the_packets() {
    // req: APP-174, APP-173, APP-138
    let hk = packet(HK, 0, 32);
    let counter_one = [0x01, 0x00, 0x02, 0x00];
    let mut n = node();
    let outs = deliver(
        &mut n,
        &[
            frame(0, 0, at(0), &zone(&[&hk]), INITIAL_OCF),
            idle_frame(0, counter_one),
            idle_frame(1, counter_one),
            frame(1, 0, at(0), &zone(&[]), [0x01, 0x00, 0x04, 0x00]),
        ],
    );
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![event(CLCW_CHANGED, &[0x0100_0200])]);
    assert_eq!(outs[2], vec![]);
    assert_eq!(outs[3], vec![event(CLCW_CHANGED, &[0x0100_0400])]);
    assert_eq!(n.last_clcw().unwrap().farm_b_counter.get(), 2);
}

#[test]
fn the_initial_clcw_is_that_of_the_initial_ocf_word() {
    // req: APP-173, APP-138
    let mut n = node();
    assert_eq!(n.last_clcw().unwrap().farm_b_counter.get(), 0);
    // A first frame with the initial word is no change.
    let outs = deliver(&mut n, &[idle_frame(0, INITIAL_OCF)]);
    assert_eq!(outs, vec![vec![]]);
}

#[test]
fn a_gap_is_that_of_the_vc_and_has_no_scope_of_the_master_channel() {
    // req: APP-138, LNK-051, LNK-061, APP-171
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let (c, d) = (packet(PAYLOAD, 0, 32), packet(PAYLOAD, 1, 32));
    let outs = deliver(
        &mut node(),
        &[
            frame(0, 0, at(0), &zone(&[&a]), INITIAL_OCF),
            // Count 1 of VC 0 is lost; no master-channel count exists to show it.
            frame(0, 2, at(0), &zone(&[&b]), INITIAL_OCF),
            // Count 5 of VC 1 after its first frame at 0: three are missing there too.
            frame(1, 0, at(0), &zone(&[&c]), INITIAL_OCF),
            frame(1, 4, at(0), &zone(&[&d]), INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, a)]);
    assert_eq!(
        outs[1],
        vec![event(FRAME_GAP, &[1, 0, 1]), Rec::PacketIn(HK, b)]
    );
    assert_eq!(
        outs[3],
        vec![event(FRAME_GAP, &[1, 1, 3]), Rec::PacketIn(PAYLOAD, d)]
    );
    for out in &outs {
        for rec in out {
            if let Rec::Event(FRAME_GAP | FRAME_DISCONTINUITY, args) = rec {
                assert_eq!(args.first(), Some(&1), "scope 0 does not exist on AOS");
            }
        }
    }
}

#[test]
fn the_idle_vc_has_its_own_count_and_gaps_in_it_are_reported() {
    // req: APP-138, APP-171, LNK-051
    let outs = deliver(
        &mut node(),
        &[
            idle_frame(0, INITIAL_OCF),
            idle_frame(3, INITIAL_OCF),
            idle_frame(4, INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![]);
    assert_eq!(outs[1], vec![event(FRAME_GAP, &[1, 7, 2])]);
    assert_eq!(outs[2], vec![]);
}

#[test]
fn the_missing_number_is_counted_modulo_the_width_of_the_count() {
    // req: APP-138, APP-171, LNK-051
    // The VC Frame Count has 24 bits (APP-171): 0xFFFFFF to 0 is in sequence.
    let outs = deliver(
        &mut node(),
        &[
            idle_frame(0x00FF_FFFF, INITIAL_OCF),
            idle_frame(0, INITIAL_OCF),
        ],
    );
    assert_eq!(outs[1], vec![]);
    // From 0xFFFFF0 to 0x010010 the missing frames are 0xFFFFF1 to 0x01000F: 15 + 65_552 = 65_567,
    // which a count of 16 or 8 bits would give as 31.
    let outs = deliver(
        &mut node(),
        &[
            idle_frame(0x00FF_FFF0, INITIAL_OCF),
            idle_frame(0x0001_0010, INITIAL_OCF),
        ],
    );
    assert_eq!(outs[1], vec![event(FRAME_GAP, &[1, 7, 65_567])]);
}

#[test]
fn a_repeated_count_is_a_discontinuity_not_a_gap() {
    // req: APP-138, LNK-051
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let outs = deliver(
        &mut node(),
        &[
            frame(0, 5, at(0), &zone(&[&a]), INITIAL_OCF),
            frame(0, 5, at(0), &zone(&[&b]), INITIAL_OCF),
        ],
    );
    assert_eq!(
        outs[1],
        vec![event(FRAME_DISCONTINUITY, &[1, 0]), Rec::PacketIn(HK, b)]
    );
}

#[test]
fn a_gap_makes_the_extractor_drop_the_partial_packet_and_resynchronise() {
    // req: APP-138, LNK-061, LNK-062
    let hk = packet(HK, 0, 32);
    let long = packet(PAYLOAD, 0, 300);
    let (head, _) = long.split_at(ZONE_LEN);
    let outs = deliver(
        &mut node(),
        &[
            frame(1, 0, at(0), head, INITIAL_OCF),
            // Frame 1 is lost; frame 2 starts a new packet after the lost residue.
            frame(1, 2, at(0), &zone(&[&hk]), INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![]);
    assert_eq!(
        outs[1],
        vec![
            event(FRAME_GAP, &[1, 1, 1]),
            event(PARTIAL_DISCARDED, &[1, u64::try_from(ZONE_LEN).unwrap()]),
            Rec::PacketIn(HK, hk),
        ]
    );
}

#[test]
fn a_packet_with_a_bad_pvn_is_an_extraction_error() {
    // req: APP-138, LNK-062
    let mut bad = packet(HK, 0, 32);
    bad[0] |= 0x20;
    let hk = packet(HK, 1, 32);
    let outs = deliver(
        &mut node(),
        &[frame(0, 0, at(0), &zone(&[&bad, &hk]), INITIAL_OCF)],
    );
    assert_eq!(outs[0][0], event(EXTRACTION_ERROR, &[0, 2]));
}

#[test]
fn a_wrong_fecf_is_rejected_and_counted_without_other_events() {
    // req: APP-138, APP-174, LNK-080
    let hk = packet(HK, 0, 32);
    let good = frame(0, 0, at(0), &zone(&[&hk]), INITIAL_OCF);
    let mut bad = good.clone();
    bad[20] ^= 0x01;
    let mut n = node();
    let outs = deliver(&mut n, &[bad, good]);
    assert_eq!(outs[0], vec![event(FRAME_REJECTED, &[CRC_MISMATCH])]);
    assert_eq!(aos_downlink(&n).receiver.status().crc_mismatch.get(), 1);
    // The rejected frame left no count: the next frame is the first.
    assert_eq!(outs[1], vec![Rec::PacketIn(HK, hk)]);
}

#[test]
fn a_header_with_one_error_is_corrected_and_one_with_more_is_rejected() {
    // req: APP-174, APP-138, LNK-080
    let hk = packet(HK, 0, 32);
    let good = frame(0, 0, at(0), &zone(&[&hk]), INITIAL_OCF);
    // One bit error in the FHEC is corrected (732.0 §4.3.7.2): the frame is read as written, and
    // its FECF is checked over the corrected header.
    let mut hit = good.clone();
    hit[6] ^= 0x10;
    let mut n = node();
    assert_eq!(deliver(&mut n, &[hit]), vec![vec![Rec::PacketIn(HK, hk)]]);
    let mut bad = good;
    for octet in &mut bad[..6] {
        *octet ^= 0xFF;
    }
    let outs = deliver(&mut n, &[bad]);
    assert_eq!(
        outs,
        vec![vec![event(FRAME_REJECTED, &[FHEC_UNCORRECTABLE])]]
    );
    assert_eq!(
        aos_downlink(&n)
            .receiver
            .status()
            .header_uncorrectable
            .get(),
        1
    );
}

#[test]
fn a_frame_of_another_channel_or_spacecraft_is_rejected() {
    // req: APP-138, APP-170, AOS-010
    let hk = packet(HK, 0, 32);
    let mut n = node();
    // VC 2 is not configured.
    let unknown_vc = frame_of(SPACECRAFT_ID, 2, 0, at(0), &zone(&[&hk]), INITIAL_OCF);
    let outs = deliver(&mut n, &[unknown_vc]);
    assert_eq!(outs, vec![vec![event(FRAME_REJECTED, &[UNKNOWN_CHANNEL])]]);
    // Another Spacecraft ID, with a valid FHEC and FECF of its own.
    let other = frame_of(SPACECRAFT_ID + 1, 0, 0, at(0), &zone(&[&hk]), INITIAL_OCF);
    let outs = deliver(&mut n, &[other]);
    assert_eq!(outs, vec![vec![event(FRAME_REJECTED, &[UNKNOWN_CHANNEL])]]);
}

#[test]
fn a_frame_of_the_wrong_length_is_rejected() {
    // req: APP-138, APP-169
    let hk = packet(HK, 0, 32);
    let good = frame(0, 0, at(0), &zone(&[&hk]), INITIAL_OCF);
    let outs = deliver(&mut node(), &[good[..222].to_vec()]);
    assert!(matches!(
        outs[0].as_slice(),
        [Rec::Event(FRAME_REJECTED, _)]
    ));
}

#[test]
fn an_oid_frame_gives_no_packet_and_no_event() {
    // req: APP-138, LNK-052
    // The flight node sends none (APP-170, OI-43); the receiver counts one and gives nothing.
    let down = FrameDownlink::<Aos>::new().unwrap();
    let fields = AosFrameFields::oid(
        SpacecraftId10::new(SPACECRAFT_ID).unwrap(),
        VcFrameCount::new(0).unwrap(),
    );
    let mut oid = vec![0; 223];
    encode_oid(
        &down.channel.physical_channel(),
        &down.channel,
        &fields,
        &[],
        &mut OidGenerator::new(),
        &mut oid,
    )
    .unwrap();
    let mut n = node();
    assert_eq!(deliver(&mut n, &[oid]), vec![vec![]]);
    assert_eq!(aos_downlink(&n).receiver.status().oid_frames.get(), 1);
}

// The CLCW of every frame goes to FOP-1 (APP-173, APP-174, APP-144).

const SLOT: u64 = 250_000;

fn noop() -> OpsEntry {
    OpsEntry::Command {
        execution: Execution::Immediate,
        call: Call::Noop,
        extra_octets: 0,
    }
}

/// The CLCW word of §4.5.3 for VC 0 with N(R) `n_r`.
const fn clcw(n_r: u8) -> [u8; 4] {
    [0x01, 0, 0, n_r]
}

/// An output that counts the frames of the uplink.
#[derive(Default)]
struct Frames(usize);

impl Output for Frames {
    fn unit(&mut self, _: &[u8]) {
        self.0 += 1;
    }
    fn packet_out(&mut self, _: Apid, _: &[u8]) {}
    fn packet_in(&mut self, _: Apid, _: &[u8]) {}
    fn app_event(&mut self, _: u16, _: &[u64]) {}
}

/// A node at `cop1` in the variant `aos` that has sent AD frame 0 at 1 s, so that a CLCW with
/// N(R) 1 acknowledges it.
fn sent_one_frame() -> (GroundNode, usize) {
    let script = [
        OpsEntry::Fop1Directive(Fop1Directive::InitiateAdWithoutClcw),
        noop(),
    ];
    let mut n = GroundNode::new_cop1(Variant::Aos, &script).unwrap();
    let mut sent = 0;
    for (now, ops) in [(0, vec![0_u16]), (1_000_000, vec![1])] {
        let mut out = Frames::default();
        let input = StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(&[]),
            ops: &ops,
            tx_slot: now.is_multiple_of(SLOT),
        };
        n.step(&input, &mut out);
        sent += out.0;
    }
    (n, sent)
}

#[test]
fn the_clcw_of_a_frame_of_any_vc_goes_to_fop1() {
    // req: APP-173, APP-174, APP-144, APP-036
    let hk = packet(HK, 0, 32);
    for (vc, unit) in [
        (0, frame(0, 0, at(0), &zone(&[&hk]), clcw(1))),
        (1, frame(1, 0, at(0), &zone(&[]), clcw(1))),
        (7, idle_frame(0, clcw(1))),
    ] {
        let (mut n, sent) = sent_one_frame();
        // The AD frame of FDU 0 went out at 1 s.
        assert_eq!(sent, 1, "vc {vc}");
        let mut out = Recorder::default();
        let delivered: [&[u8]; 1] = [&unit];
        let input = StepInput {
            now: Instant::from_micros(1_135_000),
            delivered: Delivered::new(&delivered),
            ops: &[],
            tx_slot: false,
        };
        n.step(&input, &mut out);
        // The change of the word, then the Positive Confirm of FDU 0 (E2), then the packets.
        let mut expected = vec![
            event(CLCW_CHANGED, &[0x0100_0001]),
            event(FOP1_TRANSFER, &[3, 0]),
        ];
        if vc == 0 {
            expected.push(Rec::PacketIn(HK, hk.clone()));
        }
        assert_eq!(out.0, expected, "vc {vc}");
    }
}

#[test]
fn the_aos_node_at_cop1_runs_the_directives_of_the_tc_uplink() {
    // req: APP-044, APP-144, APP-174
    let script = [OpsEntry::Fop1Directive(
        Fop1Directive::InitiateAdWithoutClcw,
    )];
    let mut n = GroundNode::new_cop1(Variant::Aos, &script).unwrap();
    let mut out = Recorder::default();
    let input = StepInput {
        now: Instant::from_micros(0),
        delivered: Delivered::new(&[]),
        ops: &[0],
        tx_slot: true,
    };
    n.step(&input, &mut out);
    assert_eq!(
        out.0,
        vec![
            event(FOP1_DIRECTIVE, &[1, 1, 0]),
            event(FOP1_DIRECTIVE, &[3, 1, 0]),
            event(FOP1_STATE, &[1]),
        ]
    );
}

#[test]
fn a_rejected_aos_frame_gives_nothing_to_fop1() {
    // req: APP-138, APP-144
    let (mut n, _) = sent_one_frame();
    let mut bad = idle_frame(0, clcw(1));
    bad[100] ^= 1;
    let mut out = Recorder::default();
    let delivered: [&[u8]; 1] = [&bad];
    let input = StepInput {
        now: Instant::from_micros(1_135_000),
        delivered: Delivered::new(&delivered),
        ops: &[],
        tx_slot: false,
    };
    n.step(&input, &mut out);
    assert_eq!(out.0, vec![event(FRAME_REJECTED, &[CRC_MISMATCH])]);
}

#[test]
fn the_tm_node_does_not_take_aos_frames_and_the_reverse() {
    // req: APP-138, APP-030, APP-169
    let hk = packet(HK, 0, 32);
    let aos_unit = frame(0, 0, at(0), &zone(&[&hk]), INITIAL_OCF);
    let mut tm = GroundNode::new_frames(Variant::TmTc, &[]).unwrap();
    let outs = deliver(&mut tm, &[aos_unit]);
    assert!(matches!(
        outs[0].as_slice(),
        [Rec::Event(FRAME_REJECTED, _)]
    ));
    let tm_unit = tm_frame(0, 0, 0, 0, &tm_field(&[&hk]), INITIAL_OCF);
    let outs = deliver(&mut node(), &[tm_unit]);
    assert!(matches!(
        outs[0].as_slice(),
        [Rec::Event(FRAME_REJECTED, _)]
    ));
}
