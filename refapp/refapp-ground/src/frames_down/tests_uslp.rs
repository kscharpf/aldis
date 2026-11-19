//! The ground node's USLP downlink on scripted frames (docs/reference-application.md §4.3.2,
//! §4.4.1, §7.4; A4 WP-4b): strict decoding with the FECF, packet extraction on VC 0 and VC 1,
//! the Encapsulation Idle Packets, the OID frames of VCID 63, the CLCW of every frame, and the
//! events of APP-138 as amended. The node of the variant is not built before WP-4c, so these
//! tests drive the [`FrameDownlink`] of the protocol type.
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use super::protocol::DownlinkProtocol;
use super::tests::{CRC_MISMATCH, HK, PAYLOAD, Rec, Recorder, event, packet};
use super::{
    CLCW_CHANGED, Downlink, EXTRACTION_ERROR, FRAME_DISCARDED, FRAME_DISCONTINUITY, FRAME_GAP,
    FRAME_REJECTED, FrameDownlink, PARTIAL_DISCARDED,
};
use crate::frames_up::Uplink;
use crate::telemetry::Telemetry;
use crate::{Fop1Directive, GroundError};
use aldis::core::ocf::Ocf;
use aldis::core::oid::OidGenerator;
use aldis::link::protocol::Uslp;
use aldis::uslp::{
    FirstHeaderPointer, FrameContent, FrameFields, FrameKind, SourceOrDestination, TfdfHeader,
    TfdzOffset, TfdzRule, Upid, UslpMapId, encode, encode_oid,
};
use refapp_scenario::generated::{SPACECRAFT_ID, apid};
use refapp_scenario::{Apid, Instant, Output, Variant};

const INITIAL_OCF: [u8; 4] = [0x01, 0, 0, 0];
/// The TFDZ of a data frame (APP-176): 223 − 7 − 1 − 3 − 4 − 2.
const ZONE_LEN: usize = 206;
const FOP1_TRANSFER: u16 = 201;

const fn offset(n: u16) -> TfdzOffset {
    TfdzOffset::new(n).unwrap()
}

/// An Encapsulation Idle Packet of `len` octets (LNK-013).
fn idle(len: usize) -> Vec<u8> {
    let mut out = vec![0; len];
    let n = aldis::epp::idle(len, 0, &mut out).unwrap();
    assert_eq!(n, len);
    out
}

/// The TFDZ that holds `parts`, filled with one Encapsulation Idle Packet (APP-177).
fn zone(parts: &[&[u8]]) -> Vec<u8> {
    let mut data: Vec<u8> = parts.concat();
    data.extend_from_slice(&idle(ZONE_LEN - data.len()));
    data
}

fn fields(vc: u8, count: u64) -> FrameFields {
    FrameFields {
        spacecraft_id: aldis::uslp::UslpSpacecraftId::new(SPACECRAFT_ID).unwrap(),
        source_or_destination: SourceOrDestination::Source,
        vc_id: aldis::uslp::UslpVcId::new(vc).unwrap(),
        map_id: UslpMapId::new(0).unwrap(),
        kind: FrameKind::Expedited,
        vcf_count: count,
    }
}

/// A data frame of the scenario's master channel (APP-175, APP-176): VC `vc`, count `count`,
/// the first packet at `pointer` (`None`: no packet starts), `tfdz` and `ocf`.
fn frame(vc: u8, count: u64, pointer: Option<u16>, tfdz: &[u8], ocf: [u8; 4]) -> Vec<u8> {
    let table = Uslp::managed().unwrap();
    let pointer = pointer.map_or(FirstHeaderPointer::NoPacketStartOrEnd, |n| {
        FirstHeaderPointer::PacketStartsAt(offset(n))
    });
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::PacketsSpanningFrames(pointer),
            upid: Upid::Packets,
        },
        insert_zone: &[],
        ocf: Some(Ocf::decode(ocf)),
    };
    let mut out = vec![0; 223];
    let n = encode(&table, fields(vc, count), content, tfdz, &mut out).unwrap();
    assert_eq!(n, 223);
    out
}

/// An OID frame of VCID 63 with the OCF `ocf` (APP-178).
fn oid_frame(ocf: [u8; 4]) -> Vec<u8> {
    let table = Uslp::managed().unwrap();
    let mut out = vec![0; 223];
    let n = encode_oid(
        &table,
        fields(63, 0),
        &[],
        Some(Ocf::decode(ocf)),
        &mut OidGenerator::new(),
        &mut out,
    )
    .unwrap();
    assert_eq!(n, 223);
    out
}

/// The downlink and the telemetry path it serves.
struct Rig {
    down: FrameDownlink<Uslp>,
    telemetry: Telemetry,
    ticks: u64,
}

impl Rig {
    fn new() -> Self {
        Self {
            down: FrameDownlink::new().unwrap(),
            telemetry: Telemetry::new(),
            ticks: 0,
        }
    }

    /// Delivers `unit` at the next slot and returns the outputs.
    fn deliver(&mut self, unit: &[u8], uplink: Option<&mut Uplink>) -> Vec<Rec> {
        let mut out = Recorder::default();
        let now = Instant::from_micros(135_000 + 125_000 * self.ticks);
        self.ticks += 1;
        self.down
            .receive(unit, &mut self.telemetry, uplink, now, &mut out);
        out.0
    }
}

fn deliver(frames: &[Vec<u8>]) -> Vec<Vec<Rec>> {
    let mut rig = Rig::new();
    frames.iter().map(|unit| rig.deliver(unit, None)).collect()
}

#[test]
fn a_data_frame_gives_its_packets_to_the_telemetry_path() {
    // req: APP-180, APP-011, LNK-060
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let outs = deliver(&[frame(0, 0, Some(0), &zone(&[&a, &b]), INITIAL_OCF)]);
    assert_eq!(outs, vec![vec![Rec::PacketIn(HK, a), Rec::PacketIn(HK, b)]]);
}

#[test]
fn a_packet_that_spans_two_frames_is_delivered_with_the_second() {
    // req: APP-180, LNK-060
    let hk = packet(HK, 0, 32);
    let long = packet(PAYLOAD, 0, 300);
    let (head, tail) = long.split_at(ZONE_LEN - hk.len());
    let first = [hk.as_slice(), head].concat();
    // The residue of the packet starts the TFDZ of the next frame; the pointer names the Idle
    // Packet after it.
    let tail_len = u16::try_from(tail.len()).unwrap();
    let second = [tail, &idle(ZONE_LEN - tail.len())].concat();
    let outs = deliver(&[
        frame(1, 0, Some(0), &first, INITIAL_OCF),
        frame(1, 1, Some(tail_len), &second, INITIAL_OCF),
    ]);
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![Rec::PacketIn(PAYLOAD, long)]);
}

#[test]
fn vc_0_and_vc_1_are_extracted_independently() {
    // req: APP-180, LNK-060
    let (hk, p) = (packet(HK, 0, 32), packet(PAYLOAD, 0, 32));
    let outs = deliver(&[
        frame(0, 0, Some(0), &zone(&[&hk]), INITIAL_OCF),
        frame(1, 0, Some(0), &zone(&[&p]), INITIAL_OCF),
    ]);
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![Rec::PacketIn(PAYLOAD, p)]);
}

#[test]
fn a_frame_in_which_no_packet_starts_continues_the_packet_of_the_one_before() {
    // req: APP-180, LNK-060
    let long = packet(PAYLOAD, 0, 450);
    let (first, rest) = long.split_at(ZONE_LEN);
    let (second, tail) = rest.split_at(ZONE_LEN);
    let tail_len = u16::try_from(tail.len()).unwrap();
    let last = [tail, &idle(ZONE_LEN - tail.len())].concat();
    let outs = deliver(&[
        frame(1, 0, Some(0), first, INITIAL_OCF),
        frame(1, 1, None, second, INITIAL_OCF),
        frame(1, 2, Some(tail_len), &last, INITIAL_OCF),
    ]);
    assert_eq!(outs[0], vec![]);
    assert_eq!(outs[1], vec![]);
    assert_eq!(outs[2], vec![Rec::PacketIn(PAYLOAD, long)]);
}

#[test]
fn an_encapsulation_idle_packet_is_dropped() {
    // req: APP-180, LNK-063
    // A TFDZ that holds only an Encapsulation Idle Packet, and one that holds a packet and one.
    let hk = packet(HK, 0, 32);
    let outs = deliver(&[
        frame(0, 0, Some(0), &idle(ZONE_LEN), INITIAL_OCF),
        frame(0, 1, Some(0), &zone(&[&hk]), INITIAL_OCF),
    ]);
    assert_eq!(outs[0], vec![]);
    assert_eq!(outs[1], vec![Rec::PacketIn(HK, hk)]);
}

#[test]
fn an_oid_frame_gives_no_packet_and_no_event_when_its_ocf_is_unchanged() {
    // req: APP-180, APP-138
    let mut rig = Rig::new();
    let outs = [
        rig.deliver(&oid_frame(INITIAL_OCF), None),
        rig.deliver(&oid_frame(INITIAL_OCF), None),
    ];
    assert_eq!(outs, [vec![], vec![]]);
    assert_eq!(rig.down.receiver.status().oid_frames.get(), 2);
}

#[test]
fn an_oid_frame_with_a_changed_ocf_gives_clcw_changed_alone() {
    // req: APP-180, APP-179, APP-138
    let mut rig = Rig::new();
    let outs = rig.deliver(&oid_frame([0x01, 0x00, 0x02, 0x00]), None);
    assert_eq!(outs, vec![event(CLCW_CHANGED, &[0x0100_0200])]);
    assert!(
        !outs
            .iter()
            .any(|r| matches!(r, Rec::Event(FRAME_DISCARDED, _)))
    );
    assert_eq!(rig.down.clcw().unwrap().farm_b_counter.get(), 1);
}

#[test]
fn a_changed_ocf_on_any_vc_is_one_clcw_changed_before_the_packets() {
    // req: APP-180, APP-179, APP-138
    let hk = packet(HK, 0, 32);
    let outs = deliver(&[
        frame(0, 0, Some(0), &zone(&[&hk]), INITIAL_OCF),
        oid_frame([0x01, 0x00, 0x02, 0x00]),
        oid_frame([0x01, 0x00, 0x02, 0x00]),
        frame(1, 0, Some(0), &zone(&[]), [0x01, 0x00, 0x04, 0x00]),
    ]);
    assert_eq!(outs[0], vec![Rec::PacketIn(HK, hk)]);
    assert_eq!(outs[1], vec![event(CLCW_CHANGED, &[0x0100_0200])]);
    assert_eq!(outs[2], vec![]);
    assert_eq!(outs[3], vec![event(CLCW_CHANGED, &[0x0100_0400])]);
}

#[test]
fn the_initial_clcw_is_that_of_the_initial_ocf_word() {
    // req: APP-179, APP-138
    let mut rig = Rig::new();
    assert_eq!(rig.down.clcw().unwrap().farm_b_counter.get(), 0);
    assert_eq!(rig.deliver(&oid_frame(INITIAL_OCF), None), vec![]);
}

#[test]
fn a_gap_is_that_of_the_vc_and_has_no_scope_of_the_master_channel() {
    // req: APP-138, LNK-051, LNK-061
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let (c, d) = (packet(PAYLOAD, 0, 32), packet(PAYLOAD, 1, 32));
    let outs = deliver(&[
        frame(0, 0, Some(0), &zone(&[&a]), INITIAL_OCF),
        // Count 1 of VC 0 is lost; no master-channel count exists to show it.
        frame(0, 2, Some(0), &zone(&[&b]), INITIAL_OCF),
        // Count 4 of VC 1 after its first frame at 0: three are missing there too.
        frame(1, 0, Some(0), &zone(&[&c]), INITIAL_OCF),
        frame(1, 4, Some(0), &zone(&[&d]), INITIAL_OCF),
    ]);
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
                assert_eq!(args.first(), Some(&1), "scope 0 does not exist on USLP");
            }
        }
    }
}

#[test]
fn the_missing_number_is_counted_modulo_the_width_of_the_count() {
    // req: APP-138, LNK-051
    let hk = |n| packet(HK, n, 32);
    // The VCF Count has one octet (APP-176): 255 to 0 is in sequence.
    let outs = deliver(&[
        frame(0, 255, Some(0), &zone(&[&hk(0)]), INITIAL_OCF),
        frame(0, 0, Some(0), &zone(&[&hk(1)]), INITIAL_OCF),
    ]);
    assert_eq!(outs[1], vec![Rec::PacketIn(HK, hk(1))]);
    // From 250 to 4 the missing frames are 251 to 255 and 0 to 3: 9, where a wider count would
    // have no wrap to count across.
    let outs = deliver(&[
        frame(0, 250, Some(0), &zone(&[&hk(0)]), INITIAL_OCF),
        frame(0, 4, Some(0), &zone(&[&hk(1)]), INITIAL_OCF),
    ]);
    assert_eq!(
        outs[1],
        vec![event(FRAME_GAP, &[1, 0, 9]), Rec::PacketIn(HK, hk(1))]
    );
    // From 10 to 120 the missing frames are 11 to 119: 109, which a count of 6 bits would give as
    // 45 and one of 5 bits as 13 (a jump of half the ring or more is a discontinuity, LNK-051).
    let outs = deliver(&[
        frame(0, 10, Some(0), &zone(&[&hk(0)]), INITIAL_OCF),
        frame(0, 120, Some(0), &zone(&[&hk(1)]), INITIAL_OCF),
    ]);
    assert_eq!(
        outs[1],
        vec![event(FRAME_GAP, &[1, 0, 109]), Rec::PacketIn(HK, hk(1))]
    );
}

#[test]
fn a_repeated_count_is_a_discontinuity_not_a_gap() {
    // req: APP-138, LNK-051
    let (a, b) = (packet(HK, 0, 32), packet(HK, 1, 32));
    let outs = deliver(&[
        frame(0, 5, Some(0), &zone(&[&a]), INITIAL_OCF),
        frame(0, 5, Some(0), &zone(&[&b]), INITIAL_OCF),
    ]);
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
    let outs = deliver(&[
        frame(1, 0, Some(0), head, INITIAL_OCF),
        // Frame 1 is lost; frame 2 starts a new packet after the lost residue.
        frame(1, 2, Some(0), &zone(&[&hk]), INITIAL_OCF),
    ]);
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
    // PVN 1 is neither a Space Packet (0) nor an Encapsulation Packet (7).
    let mut bad = packet(HK, 0, 32);
    bad[0] |= 0x20;
    let hk = packet(HK, 1, 32);
    let outs = deliver(&[frame(0, 0, Some(0), &zone(&[&bad, &hk]), INITIAL_OCF)]);
    assert_eq!(outs[0][0], event(EXTRACTION_ERROR, &[0, 2]));
}

#[test]
fn a_wrong_fecf_is_rejected_and_counted_without_other_events() {
    // req: APP-138, APP-180, LNK-080
    let hk = packet(HK, 0, 32);
    let good = frame(0, 0, Some(0), &zone(&[&hk]), INITIAL_OCF);
    let mut bad = good.clone();
    bad[20] ^= 0x01;
    let mut rig = Rig::new();
    assert_eq!(
        rig.deliver(&bad, None),
        vec![event(FRAME_REJECTED, &[CRC_MISMATCH])]
    );
    assert_eq!(rig.down.receiver.status().crc_mismatch.get(), 1);
    // The rejected frame left no count: the next frame is the first.
    assert_eq!(rig.deliver(&good, None), vec![Rec::PacketIn(HK, hk)]);
}

#[test]
fn a_frame_the_strict_check_refuses_is_rejected() {
    // req: APP-138, APP-180
    let hk = packet(HK, 0, 32);
    let good = frame(0, 0, Some(0), &zone(&[&hk]), INITIAL_OCF);
    let mut rig = Rig::new();
    // A short frame, a long one, and a frame of another Spacecraft ID are each `frame_rejected`.
    let mut longer = good.clone();
    longer.push(0);
    let mut other = good.clone();
    other[1] ^= 0x10;
    for unit in [good[..222].to_vec(), longer, other] {
        let outs = rig.deliver(&unit, None);
        assert!(
            matches!(outs.as_slice(), [Rec::Event(FRAME_REJECTED, _)]),
            "{outs:?}"
        );
    }
}

#[test]
fn the_enum_builds_the_downlink_of_the_variant_and_the_node_does_not_yet() {
    // req: APP-180
    assert!(matches!(
        Downlink::new(Variant::Uslp),
        Ok(Downlink::Uslp(_))
    ));
    // The node of the variant is not built before WP-4c.
    assert!(matches!(
        crate::GroundNode::new_frames(Variant::Uslp, &[]),
        Err(GroundError::Unsupported(Variant::Uslp))
    ));
}

/// A command packet of `len` octets on the commands APID.
fn command(len: usize) -> Vec<u8> {
    let mut packet = vec![0; len];
    let word0 = 0x1000 | apid::COMMANDS;
    packet[..2].copy_from_slice(&word0.to_be_bytes());
    packet[2..4].copy_from_slice(&0xC000_u16.to_be_bytes());
    let data_len = u16::try_from(len - 7).unwrap();
    packet[4..6].copy_from_slice(&data_len.to_be_bytes());
    packet
}

struct Sink;

impl Output for Sink {
    fn unit(&mut self, _: &[u8]) {}
    fn packet_out(&mut self, _: Apid, _: &[u8]) {}
    fn packet_in(&mut self, _: Apid, _: &[u8]) {}
    fn app_event(&mut self, _: u16, _: &[u64]) {}
}

/// An uplink at `cop1` that has sent AD frame 0 at 1 s, so that a CLCW with N(R) 1 acknowledges
/// it.
fn uplink_with_one_frame_sent() -> Uplink {
    let mut uplink = Uplink::new_cop1().unwrap();
    let at = |ms: u64| Instant::from_micros(ms * 1_000);
    uplink.directive(Fop1Directive::InitiateAdWithoutClcw, at(0), &mut Sink);
    uplink.submit(
        Apid::new(apid::COMMANDS).unwrap(),
        &command(100),
        at(0),
        &mut Sink,
    );
    uplink.slot_ad(at(1_000), &mut Sink);
    uplink
}

#[test]
fn the_clcw_of_every_frame_including_an_oid_frame_goes_to_fop1() {
    // req: APP-180, APP-179, APP-144
    let hk = packet(HK, 0, 32);
    let ack = [0x01, 0, 0, 1];
    for (label, unit) in [
        ("vc 0", frame(0, 0, Some(0), &zone(&[&hk]), ack)),
        ("vc 1", frame(1, 0, Some(0), &zone(&[]), ack)),
        ("oid", oid_frame(ack)),
    ] {
        let mut uplink = uplink_with_one_frame_sent();
        let mut rig = Rig::new();
        let outs = rig.deliver(&unit, Some(&mut uplink));
        // The change of the word, then the Positive Confirm of FDU 0 (E2), then the packets: an
        // OID frame has no packet.
        let mut expected = vec![
            event(CLCW_CHANGED, &[0x0100_0001]),
            event(FOP1_TRANSFER, &[3, 0]),
        ];
        if label == "vc 0" {
            expected.push(Rec::PacketIn(HK, hk.clone()));
        }
        assert_eq!(outs, expected, "{label}");
    }
}

#[test]
fn a_rejected_frame_gives_nothing_to_fop1() {
    // req: APP-138, APP-144
    let mut uplink = uplink_with_one_frame_sent();
    let mut bad = oid_frame([0x01, 0, 0, 1]);
    bad[100] ^= 1;
    let outs = Rig::new().deliver(&bad, Some(&mut uplink));
    assert_eq!(outs, vec![event(FRAME_REJECTED, &[CRC_MISMATCH])]);
}
