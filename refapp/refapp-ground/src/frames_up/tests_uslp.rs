//! The ground node's USLP uplink at `cop1` on scripted inputs (docs/reference-application.md
//! §4.3.3, §4.5.1, §7.4; A4 WP-4c): the frames of VC 0 and MAP 0, blocking into one TFDZ, the
//! whole-SDU table upload, the acknowledgement by the CLCW, the retransmission that returns the
//! bytes of the first transmission, and the Type-BC frame that goes before Type-AD.
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use super::{FrameUplink, Uplink};
use crate::ops::Fop1Directive;
use crate::{GroundNode, OpsEntry};
use aldis::cop1::{FrameKind as Fop1Kind, HeldKind};
use aldis::core::DecodeMode;
use aldis::core::ids::Apid as CoreApid;
use aldis::core::security::SecurityLayout;
use aldis::link::protocol::Uslp;
use aldis::uslp;
use refapp_scenario::generated::{SPACECRAFT_ID, apid, uslp_up};
use refapp_scenario::{
    Apid, Delivered, Instant, Node, Output, StepInput, TableId, TableLength, Variant,
};

/// The USLP uplink, whose fields some tests reach.
type Typed = FrameUplink<Uslp, 1, 512>;

/// Application event `fop1_transfer` (§7.4).
const FOP1_TRANSFER: u16 = 201;
/// Application event `uplink_refused` (§7.4).
const UPLINK_REFUSED: u16 = 114;
/// A CLCW word with COP in effect, no flags, and N(R) = 1 (232.1 §4.2, Table 4-1).
const ACK_ONE: u32 = 0x0100_0001;

#[derive(Default)]
struct Recorder {
    /// The octets of each unit output, in order.
    frames: Vec<Vec<u8>>,
    /// The packets released, in order.
    packets: Vec<Vec<u8>>,
    events: Vec<(u16, Vec<u64>)>,
}

impl Output for Recorder {
    fn unit(&mut self, unit: &[u8]) {
        self.frames.push(unit.to_vec());
    }
    fn packet_out(&mut self, _: CoreApid, octets: &[u8]) {
        self.packets.push(octets.to_vec());
    }
    fn packet_in(&mut self, _: CoreApid, _: &[u8]) {}
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.events.push((code, args.to_vec()));
    }
}

const fn at(ms: u64) -> Instant {
    Instant::from_micros(ms * 1_000)
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

fn commands_apid() -> Apid {
    Apid::new(apid::COMMANDS).unwrap()
}

/// The USLP channel of §4.3.3 for the strict decode of the tests: VC 0, MAP 0, variable length,
/// the FECF, a one-octet VCF Count (APP-181).
fn channels() -> uslp::VcTable<1, 1> {
    let scid = uslp::UslpSpacecraftId::new(SPACECRAFT_ID).unwrap();
    let vc = uslp::UslpVcId::new(uslp_up::VCID).unwrap();
    let physical = uslp::PhysicalChannelConfig::new(
        uslp::TransferFrameType::Variable,
        usize::from(uslp_up::MAX_FRAME_LENGTH),
        0,
        uslp_up::FECF,
    )
    .unwrap();
    let params = uslp::VcParams {
        vcf_count_len_sequence: uslp::VcfCountLen::new(1).unwrap(),
        ..uslp::VcParams::new(uslp::TransferFrameType::Variable)
    };
    let map = uslp::MapConfig::new(
        scid,
        vc,
        uslp::UslpMapId::new(uslp_up::MAP_ID).unwrap(),
        SecurityLayout::NONE,
    );
    uslp::VcTable::new(
        physical,
        &[uslp::VcConfig::new(scid, vc, params).unwrap()],
        &[map],
    )
    .unwrap()
}

/// The strict decode of `frame` (FECF checked), and its TFDF Header, TFDZ, and kind.
fn decoded(frame: &[u8]) -> (uslp::FrameKind, uslp::TfdfHeader, Vec<u8>) {
    let table = channels();
    let checked = uslp::check_frame(frame, &table, DecodeMode::Strict).unwrap();
    let view = checked.into_view().into_frame();
    let full = view.full().unwrap();
    (full.kind(), full.tfdf_header(), full.tfdz().to_vec())
}

/// The octet of the TFDF Header of the rule `111` (APP-183) with `upid`: the rule in the three
/// high bits, the UPID in the five low (732.1 §4.1.4.2.2).
const fn tfdf_octet(upid: uslp::Upid) -> u8 {
    0b1110_0000 | upid.bits()
}

/// An uplink at `cop1` with FOP-1 in the Active state: `InitiateAdWithoutClcw` at 0 ms.
fn active() -> (Uplink, Recorder) {
    let mut uplink = Uplink::cop1(Variant::Uslp).unwrap();
    let mut out = Recorder::default();
    uplink.directive(Fop1Directive::InitiateAdWithoutClcw, at(0), &mut out);
    (uplink, out)
}

#[test]
fn a_type_ad_frame_has_the_fields_of_the_schedule() {
    // req: APP-181, APP-183, APP-044
    let (mut uplink, mut out) = active();
    let first = command(100);
    let second = command(60);
    uplink.submit(commands_apid(), &first, at(0), &mut out);
    uplink.slot_ad(at(1), &mut out);
    uplink.submit(commands_apid(), &second, at(1), &mut out);
    uplink.slot_ad(at(251), &mut out);
    assert_eq!(out.frames.len(), 2);
    for (n, (frame, packet)) in out.frames.iter().zip([&first, &second]).enumerate() {
        // Primary Header: version `1100`, SCID 341, Destination, VCID 0, MAP ID 0, End of Frame
        // Primary Header Flag 0 (732.1 Fig. 4-1), then the Frame Length, one less than the octets.
        assert_eq!(frame[..4], [0xC0, 0x15, 0x58, 0x00], "frame {n}");
        let length = u16::from_be_bytes([frame[4], frame[5]]);
        assert_eq!(usize::from(length) + 1, frame.len(), "frame {n}");
        // Bypass 0, Protocol Control Command 0, spares `00`, OCF Flag 0, VCF Count Length `001`.
        assert_eq!(frame[6], 0b0000_0001, "frame {n}");
        // N(S) is the VCF Count (one octet); then the TFDF Header and the packet as the TFDZ.
        assert_eq!(usize::from(frame[7]), n, "frame {n}");
        assert_eq!(frame[8], tfdf_octet(uslp::Upid::Packets), "frame {n}");
        // The header (8 octets), TFDF Header (1), the packet, the FECF (2).
        assert_eq!(frame.len(), 8 + 1 + packet.len() + 2, "frame {n}");
        let (kind, header, zone) = decoded(frame);
        assert_eq!(kind, uslp::FrameKind::SequenceControlled, "frame {n}");
        assert_eq!(header.upid, uslp::Upid::Packets, "frame {n}");
        assert_eq!(&zone, packet, "frame {n}");
    }
    assert_eq!(uplink.refused(), 0);
}

#[test]
fn the_clcw_that_acknowledges_the_frame_confirms_its_request() {
    // req: APP-044, APP-144, APP-184
    let (mut uplink, mut out) = active();
    uplink.submit(commands_apid(), &command(100), at(0), &mut out);
    uplink.slot_ad(at(1), &mut out);
    assert_eq!(out.frames.len(), 1);
    assert!(out.events.iter().all(|(code, _)| *code != FOP1_TRANSFER));
    // N(R) = 1 acknowledges N(S) = 0: the Positive Confirm of request 0 (232.1 §5.1.4).
    uplink.clcw(ACK_ONE, at(250), &mut out);
    assert_eq!(
        out.events
            .iter()
            .filter(|(code, _)| *code == FOP1_TRANSFER)
            .collect::<Vec<_>>(),
        [&(FOP1_TRANSFER, vec![3, 0])]
    );
    assert_eq!(uplink.refused(), 0);
}

#[test]
fn the_commands_released_together_share_one_tfdz_while_they_fit() {
    // req: APP-184, APP-181, LNK-040
    let (mut uplink, mut out) = active();
    // Six commands of 100 octets: five fill the 501-octet TFDZ except one octet, the sixth goes
    // in the next frame (APP-182).
    let packets: Vec<Vec<u8>> = (0..6).map(|_| command(100)).collect();
    for packet in &packets {
        uplink.submit(commands_apid(), packet, at(0), &mut out);
    }
    uplink.slot_ad(at(1), &mut out);
    uplink.slot_ad(at(251), &mut out);
    assert_eq!(out.frames.len(), 2);
    let (_, _, first) = decoded(&out.frames[0]);
    let (_, _, second) = decoded(&out.frames[1]);
    assert_eq!(first, packets[..5].concat());
    assert_eq!(second, packets[5]);
    // One request per frame: N(S) 0 and 1, and no refusal.
    assert_eq!([out.frames[0][7], out.frames[1][7]], [0, 1]);
    assert_eq!(uplink.refused(), 0);
}

#[test]
fn a_packet_of_the_maximum_length_fills_the_frame_and_a_longer_one_is_refused() {
    // req: APP-182, APP-143
    let max = usize::from(uslp_up::MAX_PACKET_LENGTH);
    let (mut uplink, mut out) = active();
    uplink.submit(commands_apid(), &command(max), at(0), &mut out);
    uplink.slot_ad(at(1), &mut out);
    // The Maximum Transfer Frame Length is 512 octets, the Frame Length field 511.
    assert_eq!(out.frames.len(), 1);
    assert_eq!(out.frames[0].len(), 512);
    assert_eq!(out.frames[0][4..6], [0x01, 0xFF]);
    let (_, _, zone) = decoded(&out.frames[0]);
    assert_eq!(zone.len(), max);
    assert_eq!(uplink.refused(), 0);
    // One octet more has no frame: nothing is segmented, so the segmenter refuses it.
    uplink.submit(commands_apid(), &command(max + 1), at(1), &mut out);
    assert_eq!(uplink.refused(), 1);
    assert_eq!(
        out.events.last().map(|(code, _)| *code),
        Some(UPLINK_REFUSED)
    );
}

#[test]
fn the_table_upload_goes_in_one_frame_whole() {
    // req: APP-182, APP-184, APP-183
    let script = [
        OpsEntry::Fop1Directive(Fop1Directive::InitiateAdWithoutClcw),
        OpsEntry::TableUpload {
            table_id: TableId::new(1).unwrap(),
            length: TableLength::new(300).unwrap(),
        },
    ];
    let mut node = GroundNode::new_cop1(Variant::Uslp, &script).unwrap();
    let mut out = Recorder::default();
    for (index, now) in [(0, at(0)), (1, at(250))] {
        let input = StepInput {
            now,
            delivered: Delivered::new(&[]),
            ops: &[index],
            tx_slot: true,
        };
        node.step(&input, &mut out);
    }
    // The packet of the 300-octet table is 309 octets (APP-182), the frame 320 with the Frame
    // Length 319, and the packet is the TFDZ, unsegmented.
    let [packet] = out.packets.as_slice() else {
        panic!("one packet released");
    };
    assert_eq!(packet.len(), 309);
    let [frame] = out.frames.as_slice() else {
        panic!("one frame");
    };
    assert_eq!(frame.len(), 320);
    assert_eq!(frame[4..6], [0x01, 0x3F]);
    let (kind, header, zone) = decoded(frame);
    assert_eq!(kind, uslp::FrameKind::SequenceControlled);
    assert_eq!(header.rule, uslp::TfdzRule::NoSegmentation);
    assert_eq!(&zone, packet);
    assert_eq!(node.uplink_refused(), 0);
}

#[test]
fn a_retransmitted_frame_is_the_master_copy_byte_for_byte() {
    // req: APP-044, APP-184, COP-011
    let mut uplink = Typed::new_cop1().unwrap();
    let mut out = Recorder::default();
    uplink.directive(Fop1Directive::InitiateAdWithoutClcw, at(0), &mut out);
    // Three packets of different lengths, one in each slot (the ones of one instant would share
    // a frame), so that a frame rebuilt from another FDU would differ.
    for (slot, len) in [(1, 100), (2, 90), (3, 80)] {
        uplink.submit(commands_apid(), &command(len), at(slot), &mut out);
        uplink.slot_ad(at(slot * 250), &mut out);
    }
    let sent = out.frames.clone();
    assert_eq!(sent.len(), 3);
    // The Retransmit flag with N(R) = 1 returns the frames from N(S) = 1 (232.1 §5.1.8.1).
    uplink.clcw(0x0100_0000 + 1 + 0x0800, at(2_000), &mut out);
    assert_eq!(out.frames.len(), sent.len());
    assert!(!uplink.fop.out_flag_ready(Fop1Kind::Ad));
    let held = uplink.fop.held_frame(HeldKind::Ad).unwrap().to_vec();
    assert_eq!(held[7], 1);
    assert_eq!(held, sent[1]);
    uplink.slot_ad(at(2_250), &mut out);
    assert_eq!(out.frames.last(), sent.get(1));
    assert_eq!(uplink.refused(), 0);
}

#[test]
fn the_type_bc_frame_of_the_unlock_goes_out_before_any_type_ad_frame() {
    // req: APP-183, APP-184
    let mut uplink = Uplink::cop1(Variant::Uslp).unwrap();
    let mut out = Recorder::default();
    // A command is submitted before the directive that sends the Unlock Control Command. FOP-1
    // takes no Type-AD request while it initializes with the Unlock (232.1 §5.1.9), so the first
    // frame of the node is the Type-BC frame and the request of the command is rejected.
    uplink.submit(commands_apid(), &command(100), at(0), &mut out);
    uplink.directive(Fop1Directive::InitiateAdWithUnlock, at(0), &mut out);
    uplink.slot_ad(at(1), &mut out);
    assert_eq!(out.frames.len(), 1);
    assert!(out.events.contains(&(FOP1_TRANSFER, vec![2, 0])));
    let bc = &out.frames[0];
    // Bypass 1, Protocol Control Command 1, spares `00`, OCF Flag 0, VCF Count Length `000`: no
    // VCF Count, so the TFDF Header follows the Primary Header (APP-183).
    assert_eq!(bc[6], 0b1100_0000);
    assert_eq!(bc[7], tfdf_octet(uslp::Upid::Cop1ControlCommands));
    // The Unlock Control Command is one octet, `00` (232.0 §4.1.3.3).
    assert_eq!(bc.len(), 7 + 1 + 1 + 2);
    let (kind, header, zone) = decoded(bc);
    assert_eq!(kind, uslp::FrameKind::ProtocolControl);
    assert_eq!(header.upid, uslp::Upid::Cop1ControlCommands);
    assert_eq!(zone, [0x00]);
    // The CLCW that answers the Unlock opens the AD service: the next command is Type-AD with
    // N(S) = 0, the first frame the receiver sees in sequence.
    uplink.clcw(0x0100_0000, at(250), &mut out);
    let next = command(60);
    uplink.submit(commands_apid(), &next, at(250), &mut out);
    uplink.slot_ad(at(251), &mut out);
    assert_eq!(out.frames.len(), 2);
    assert_eq!(out.frames[1][7], 0);
    let (kind, _, zone) = decoded(&out.frames[1]);
    assert_eq!(kind, uslp::FrameKind::SequenceControlled);
    assert_eq!(zone, next);
    assert_eq!(uplink.refused(), 0);
}

#[test]
fn the_node_of_uslp_is_built_at_cop1_only() {
    // req: APP-187, APP-044
    assert!(GroundNode::new_cop1(Variant::Uslp, &[]).is_ok());
    assert!(GroundNode::new_frames(Variant::Uslp, &[]).is_err());
}
