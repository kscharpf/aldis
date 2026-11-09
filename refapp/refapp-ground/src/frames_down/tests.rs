//! The frame-level downlink on scripted frames (docs/reference-application.md §4.1, §4.4.1,
//! §7.4; A2 WP-4a).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use super::{
    CLCW_CHANGED, EXTRACTION_ERROR, FRAME_DISCARDED, FRAME_DISCONTINUITY, FRAME_GAP,
    FRAME_REJECTED, FrameDownlink, PARTIAL_DISCARDED,
};
use crate::{GroundNode, telemetry};
use aldis::core::ids::{Apid, SeqCount, VcId};
use aldis::core::ocf::Ocf;
use aldis::core::oid::OidGenerator;
use aldis::spp::{PacketFields, PacketType, SequenceFlags, SppConfig, encode as spp_encode};
use aldis::tm::{
    DataFieldStatus, PacketOffset, PacketPointer, TmFrameContent, TmFrameFields, encode, encode_oid,
};
use refapp_scenario::{Delivered, Instant, Node, Output, StepInput};

/// One output of a step, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rec {
    PacketIn(u16, Vec<u8>),
    Event(u16, Vec<u64>),
}

#[derive(Default)]
struct Recorder(Vec<Rec>);

impl Output for Recorder {
    fn unit(&mut self, _: &[u8]) {}
    fn packet_out(&mut self, _: Apid, _: &[u8]) {}
    fn packet_in(&mut self, apid: Apid, octets: &[u8]) {
        self.0.push(Rec::PacketIn(apid.get(), octets.to_vec()));
    }
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.0.push(Rec::Event(code, args.to_vec()));
    }
}

const HK: u16 = 16;
const PAYLOAD: u16 = 32;
const INITIAL_OCF: [u8; 4] = [0x01, 0, 0, 0];
const DATA_LEN: usize = 211;

/// Catalogue codes of docs/03-api-model.md §9.
const CRC_MISMATCH: u64 = 8;
const UNKNOWN_CHANNEL: u64 = 10;

fn event(code: u16, args: &[u64]) -> Rec {
    Rec::Event(code, args.to_vec())
}

/// A telemetry packet with the implicit-P-field CUC secondary header (§3.2).
fn packet(apid: u16, count: u16, user_len: usize) -> Vec<u8> {
    let mut secondary = (2_100_000_000_u32 + u32::from(count))
        .to_be_bytes()
        .to_vec();
    secondary.extend_from_slice(&[0, 0]);
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        apid: Apid::new(apid).unwrap(),
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::new(count).unwrap(),
    };
    let user = vec![0xA5; user_len];
    let data = SppConfig::DEFAULT
        .packet(fields, &secondary, &user)
        .unwrap();
    let mut out = vec![0; data.packet_len()];
    let n = spp_encode(&data, &mut out).unwrap();
    out.truncate(n);
    out
}

/// The data field that holds `parts`, filled with one Idle Packet (APID 2047, §4.1).
fn field(parts: &[&[u8]]) -> Vec<u8> {
    let mut data: Vec<u8> = parts.concat();
    let rest = DATA_LEN - data.len();
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        apid: Apid::new(2047).unwrap(),
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::new(0).unwrap(),
    };
    let user = vec![0; rest - 6];
    let idle = SppConfig::DEFAULT.packet(fields, &[], &user).unwrap();
    let mut out = vec![0; idle.packet_len()];
    let n = spp_encode(&idle, &mut out).unwrap();
    data.extend_from_slice(&out[..n]);
    data
}

/// A frame of the scenario's master channel.
fn frame(vc: u8, mc: u8, vcc: u8, pointer: u16, data: &[u8], ocf: [u8; 4]) -> Vec<u8> {
    let down = FrameDownlink::new().unwrap();
    let fields = TmFrameFields::new(
        VcId::new(vc).unwrap(),
        mc,
        vcc,
        DataFieldStatus::Packets(match pointer {
            0x7FF => PacketPointer::NoPacketStart,
            0x7FE => PacketPointer::OnlyIdleData,
            n => PacketPointer::StartsAt(PacketOffset::new(n).unwrap()),
        }),
    );
    let mut content = TmFrameContent::new(data);
    content.ocf = Some(Ocf::decode(ocf));
    let mut out = vec![0; 223];
    let n = encode(&down.channel, &fields, &content, &mut out).unwrap();
    out.truncate(n);
    out
}

fn oid_frame(mc: u8, vcc: u8, ocf: [u8; 4]) -> Vec<u8> {
    let down = FrameDownlink::new().unwrap();
    let mut out = vec![0; 223];
    let n = encode_oid(
        &down.channel,
        VcId::new(7).unwrap(),
        mc,
        vcc,
        &[],
        Some(Ocf::decode(ocf)),
        &mut OidGenerator::new(),
        &mut out,
    )
    .unwrap();
    out.truncate(n);
    out
}

/// A node at the frame level that receives `frames`, one per step, and the outputs of each.
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
    GroundNode::new_frames(&[]).unwrap()
}

#[test]
fn a_data_frame_gives_its_packets_to_the_telemetry_path() {
    // req: APP-036, APP-138, APP-011
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let data = field(&[&a, &b]);
    let outs = deliver(&mut node(), &[frame(0, 0, 0, 0, &data, INITIAL_OCF)]);
    assert_eq!(outs, vec![vec![Rec::PacketIn(HK, a), Rec::PacketIn(HK, b)]]);
}

#[test]
fn a_packet_that_spans_two_frames_is_delivered_with_the_second() {
    // req: APP-036, APP-138, LNK-060
    let p = packet(PAYLOAD, 0, 400);
    let (first, second) = p.split_at(DATA_LEN);
    let f1 = frame(1, 0, 0, 0, first, INITIAL_OCF);
    let f2 = frame(
        1,
        1,
        1,
        u16::try_from(second.len()).unwrap(),
        &field(&[second]),
        INITIAL_OCF,
    );
    let outs = deliver(&mut node(), &[f1, f2]);
    assert_eq!(outs[0], vec![]);
    assert_eq!(outs[1], vec![Rec::PacketIn(PAYLOAD, p)]);
}

#[test]
fn the_two_vcs_are_extracted_independently() {
    // req: APP-036, LNK-060
    let p = packet(PAYLOAD, 0, 400);
    let hk = packet(HK, 0, 32);
    let (first, second) = p.split_at(DATA_LEN);
    let outs = deliver(
        &mut node(),
        &[
            frame(1, 0, 0, 0, first, INITIAL_OCF),
            frame(0, 1, 0, 0, &field(&[&hk]), INITIAL_OCF),
            frame(
                1,
                2,
                1,
                u16::try_from(second.len()).unwrap(),
                &field(&[second]),
                INITIAL_OCF,
            ),
        ],
    );
    assert_eq!(outs[1], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[2], vec![Rec::PacketIn(PAYLOAD, p)]);
}

#[test]
fn an_oid_frame_gives_no_packet_and_no_event() {
    // req: APP-138, LNK-052
    let mut n = node();
    let outs = deliver(&mut n, &[oid_frame(0, 0, INITIAL_OCF)]);
    assert_eq!(outs, vec![vec![]]);
    let rx = &n.downlink.as_ref().unwrap().receiver;
    assert_eq!(rx.status().oid_frames.get(), 1);
}

#[test]
fn a_changed_ocf_is_one_clcw_changed_before_the_packets() {
    // req: APP-138, APP-134
    // D2 of the plan: the CLCW is reported, not given to FOP-1.
    let hk = packet(HK, 0, 32);
    let counter_one = [0x01, 0x00, 0x02, 0x00];
    let mut n = node();
    let outs = deliver(
        &mut n,
        &[
            frame(0, 0, 0, 0, &field(&[&hk]), INITIAL_OCF),
            oid_frame(1, 1, counter_one),
            oid_frame(2, 2, counter_one),
        ],
    );
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![event(CLCW_CHANGED, &[0x0100_0200])]);
    assert_eq!(outs[2], vec![]);
    let clcw = n.last_clcw().unwrap();
    assert_eq!(clcw.farm_b_counter.get(), 1);
}

#[test]
fn a_data_frame_reports_the_ocf_before_its_packets() {
    // req: APP-138
    let hk = packet(HK, 0, 32);
    let outs = deliver(
        &mut node(),
        &[frame(0, 0, 0, 0, &field(&[&hk]), [0x01, 0x00, 0x04, 0x00])],
    );
    assert_eq!(
        outs[0],
        vec![event(CLCW_CHANGED, &[0x0100_0400]), Rec::PacketIn(HK, hk)]
    );
}

#[test]
fn a_wrong_fecf_is_rejected_and_counted_without_other_events() {
    // req: APP-138, TM-005, LNK-080
    let hk = packet(HK, 0, 32);
    let mut bad = frame(0, 0, 0, 0, &field(&[&hk]), INITIAL_OCF);
    bad[10] ^= 0x01;
    let mut n = node();
    let outs = deliver(&mut n, &[bad]);
    assert_eq!(outs, vec![vec![event(FRAME_REJECTED, &[CRC_MISMATCH])]]);
    let status = n.downlink.as_ref().unwrap().receiver.status();
    assert_eq!(status.crc_mismatch.get(), 1);
}

#[test]
fn a_rejected_frame_leaves_the_ocf_and_the_counts_as_they_were() {
    // req: APP-138
    let hk = |c| packet(HK, c, 32);
    let mut bad = frame(0, 1, 1, 0, &field(&[&hk(1)]), [0x01, 0, 0x02, 0]);
    bad[10] ^= 0x01;
    let outs = deliver(
        &mut node(),
        &[
            frame(0, 0, 0, 0, &field(&[&hk(0)]), INITIAL_OCF),
            bad,
            frame(0, 1, 1, 0, &field(&[&hk(1)]), INITIAL_OCF),
        ],
    );
    // No gap after the rejected frame, and the OCF word is still the initial one.
    assert_eq!(outs[2], vec![Rec::PacketIn(HK, hk(1))]);
}

#[test]
fn a_frame_of_another_channel_or_spacecraft_is_rejected() {
    // req: APP-138, TM-011
    let hk = packet(HK, 0, 32);
    // VC 2 is not configured.
    let unknown_vc = frame_with(2, &field(&[&hk]), |_| {});
    let outs = deliver(&mut node(), &[unknown_vc]);
    assert_eq!(outs, vec![vec![event(FRAME_REJECTED, &[UNKNOWN_CHANNEL])]]);
}

/// A frame of an unconfigured VC, built on a channel that has it, with the FECF fixed up.
fn frame_with(vc: u8, data: &[u8], tweak: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    use aldis::tm::{
        DataFieldContent, TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig,
    };
    let base = FrameDownlink::new().unwrap();
    let channel = TmMasterChannelConfig::new(
        TmPhysicalChannelConfig::new(223, true).unwrap(),
        base.channel.scid(),
        None,
        true,
    )
    .with_vc(TmVirtualChannelConfig::new(
        VcId::new(vc).unwrap(),
        DataFieldContent::Packets,
    ))
    .unwrap();
    let fields = TmFrameFields::new(
        VcId::new(vc).unwrap(),
        0,
        0,
        DataFieldStatus::Packets(PacketPointer::StartsAt(PacketOffset::new(0).unwrap())),
    );
    let mut content = TmFrameContent::new(data);
    content.ocf = Some(Ocf::decode(INITIAL_OCF));
    let mut out = vec![0; 223];
    let n = encode(&channel, &fields, &content, &mut out).unwrap();
    out.truncate(n);
    tweak(&mut out);
    out
}

#[test]
fn gaps_of_the_mc_and_the_vc_come_first_and_the_extractor_resynchronises() {
    // req: APP-138, LNK-051, LNK-061
    let p = packet(PAYLOAD, 0, 400);
    let (first, _) = p.split_at(DATA_LEN);
    let next = packet(PAYLOAD, 5, 8);
    let after = packet(HK, 3, 32);
    let outs = deliver(
        &mut node(),
        &[
            frame(1, 0, 0, 0, first, INITIAL_OCF),
            // Two frames lost: the MC count jumps by 3, the VC count by 2. The partial packet
            // is thrown away and the frame is read from its pointer.
            frame(1, 3, 2, 0, &field(&[&next]), INITIAL_OCF),
            // Another VC's frame lost: only the MC count has a gap.
            frame(0, 5, 0, 0, &field(&[&after]), INITIAL_OCF),
        ],
    );
    assert_eq!(outs[0], vec![]);
    assert_eq!(
        outs[1],
        vec![
            event(FRAME_GAP, &[0, 0, 2]),
            event(FRAME_GAP, &[1, 1, 1]),
            event(PARTIAL_DISCARDED, &[1, u64::try_from(first.len()).unwrap()]),
            Rec::PacketIn(PAYLOAD, next),
        ]
    );
    assert_eq!(
        outs[2],
        vec![event(FRAME_GAP, &[0, 0, 1]), Rec::PacketIn(HK, after)]
    );
}

#[test]
fn a_repeated_count_is_a_discontinuity_not_a_gap() {
    // req: APP-138, LNK-051
    let hk = |c| packet(HK, c, 32);
    let f = frame(0, 0, 0, 0, &field(&[&hk(0)]), INITIAL_OCF);
    let outs = deliver(&mut node(), &[f.clone(), f]);
    assert_eq!(
        outs[1],
        vec![
            event(FRAME_DISCONTINUITY, &[0, 0]),
            event(FRAME_DISCONTINUITY, &[1, 0]),
            Rec::PacketIn(HK, hk(0)),
            // The packet-level repeat follows the frame events (APP-130).
            event(telemetry::SEQ_REPEAT, &[16, 0]),
        ]
    );
}

#[test]
fn a_packet_with_a_bad_pvn_is_an_extraction_error_before_the_next_packets() {
    // req: APP-138, LNK-062
    let hk = packet(HK, 0, 32);
    let mut bad = packet(HK, 1, 32);
    bad[0] |= 0x20; // Packet Version Number 1.
    let outs = deliver(
        &mut node(),
        &[frame(0, 0, 0, 0, &field(&[&bad, &hk]), INITIAL_OCF)],
    );
    assert_eq!(outs[0], vec![event(EXTRACTION_ERROR, &[0, 2])]);
}

#[test]
fn a_data_frame_of_the_idle_vc_is_discarded_as_not_served() {
    // req: APP-138
    let hk = packet(HK, 0, 32);
    let outs = deliver(
        &mut node(),
        &[frame(7, 0, 0, 0, &field(&[&hk]), INITIAL_OCF)],
    );
    assert_eq!(outs[0], vec![event(FRAME_DISCARDED, &[1, 2])]);
}

#[test]
fn a_telemetry_packet_that_fails_the_a1_decoding_is_rejected_after_the_frame_events() {
    // req: APP-138, APP-130
    // An unknown APID in a well-formed Space Packet.
    let alien = packet(200, 0, 8);
    let outs = deliver(
        &mut node(),
        &[frame(0, 0, 0, 0, &field(&[&alien]), INITIAL_OCF)],
    );
    assert_eq!(
        outs[0],
        vec![event(telemetry::PACKET_REJECTED, &[UNKNOWN_CHANNEL])]
    );
}

#[test]
fn the_packet_level_node_is_unchanged() {
    // req: APP-011
    let hk = packet(HK, 0, 32);
    let mut n = GroundNode::new(&[]).unwrap();
    let outs = deliver(&mut n, std::slice::from_ref(&hk));
    assert_eq!(outs, vec![vec![Rec::PacketIn(HK, hk)]]);
    assert!(n.last_clcw().is_none());
}
