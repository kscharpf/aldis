//! Tests of the uplink over USLP: FARM-1 for the three frame types, the MAP reassembler on MAP 0,
//! and the events (docs/reference-application.md §4.3.3, §4.5.3, §7.4; APP-181…APP-186).

extern crate std;

use std::vec::Vec;

use aldis::core::ErrorKind;
use aldis::core::crc::crc16;
use aldis::core::security::SecurityLayout;
use aldis::link::reassembler::ReassemblyEvent;
use aldis::tc::ControlCommand;
use aldis::uslp::{
    FrameContent, FrameFields, FrameKind, MapConfig, OcfUse, PhysicalChannelConfig,
    SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, Upid, UslpMapId,
    UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen, encode,
};
use refapp_scenario::generated::SPACECRAFT_ID;
use refapp_scenario::{Delivered, Instant, Node, StepInput};

use super::{
    APP_FRAME_REJECTED, CMD_RECEIVED, Run, TABLE_RECEIVED, code, command, noop, table_packet,
};
use crate::frames_up::{
    APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED, APP_GAP_DISCARD, reassembly_code,
};
use crate::tests::Out;

const LOCKOUT: u64 = 3;

/// The encoder's view of the uplink: VC 0 and a VC 1 the node does not have, MAPs 0 and 5.
fn sender() -> VcTable<2, 3> {
    let scid = UslpSpacecraftId::new(SPACECRAFT_ID).unwrap();
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(1).unwrap(),
        vcf_count_len_expedited: VcfCountLen::new(0).unwrap(),
        ocf: OcfUse::Absent,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc = |id: u8| VcConfig::new(scid, UslpVcId::new(id).unwrap(), params).unwrap();
    let map = |vc: u8, id: u8| {
        MapConfig::new(
            scid,
            UslpVcId::new(vc).unwrap(),
            UslpMapId::new(id).unwrap(),
            SecurityLayout::NONE,
        )
    };
    VcTable::new(
        PhysicalChannelConfig::new(TransferFrameType::Variable, 512, 0, true).unwrap(),
        &[vc(0), vc(1)],
        &[map(0, 0), map(0, 5), map(1, 0)],
    )
    .unwrap()
}

/// A USLP uplink frame of `kind` on `vc` and `map`, with the rule `111` and `upid` (APP-183).
fn frame(kind: FrameKind, vc: u8, map: u8, count: u64, upid: Upid, tfdz: &[u8]) -> Vec<u8> {
    let fields = FrameFields {
        spacecraft_id: UslpSpacecraftId::new(SPACECRAFT_ID).unwrap(),
        source_or_destination: SourceOrDestination::Destination,
        vc_id: UslpVcId::new(vc).unwrap(),
        map_id: UslpMapId::new(map).unwrap(),
        kind,
        vcf_count: count,
    };
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid,
        },
        insert_zone: &[],
        ocf: None,
    };
    let mut out = [0_u8; 512];
    let n = encode(&sender(), fields, content, tfdz, &mut out).unwrap();
    out[..n].to_vec()
}

/// A Type-AD frame with N(S) = `n_s`, the Space Packets of `tfdz` on VC 0 and MAP 0.
fn ad(n_s: u8, tfdz: &[u8]) -> Vec<u8> {
    frame(
        FrameKind::SequenceControlled,
        0,
        0,
        u64::from(n_s),
        Upid::Packets,
        tfdz,
    )
}

fn ad_noop(n_s: u8) -> Vec<u8> {
    ad(n_s, &noop(u16::from(n_s)))
}

/// A Type-BD frame (the Bypass flag 1, no Protocol Control Command, no count).
fn bd(tfdz: &[u8]) -> Vec<u8> {
    frame(FrameKind::Expedited, 0, 0, 0, Upid::Packets, tfdz)
}

/// A Type-BC frame with `tfdz` under the UPID of the COP-1 Control Commands.
fn bc_raw(tfdz: &[u8]) -> Vec<u8> {
    frame(
        FrameKind::ProtocolControl,
        0,
        0,
        0,
        Upid::Cop1ControlCommands,
        tfdz,
    )
}

fn bc(command: ControlCommand) -> Vec<u8> {
    bc_raw(command.to_octets().as_slice())
}

fn discarded(event: u64) -> (u16, Vec<u64>) {
    (APP_FRAME_DISCARDED, std::vec![2, event])
}

fn cmd(count: u64) -> (u16, Vec<u64>) {
    (CMD_RECEIVED, std::vec![1, count])
}

fn rejected(kind: ErrorKind) -> (u16, Vec<u64>) {
    (APP_FRAME_REJECTED, std::vec![code(kind)])
}

#[test]
fn an_ad_frame_in_sequence_is_delivered_and_advances_nr() {
    // req: APP-185, APP-186, APP-044
    let mut run = Run::new_uslp();
    let first = ad_noop(0);
    // APP-183: version 1100 in the top nibble, the Frame Length one less than the octets, the
    // sequence-controlled VCF Count after the Primary Header, then the one-octet TFDF Header.
    assert_eq!(first[0] >> 4, 0b1100);
    assert_eq!(
        usize::from(u16::from_be_bytes([first[4], first[5]])),
        first.len() - 1
    );
    run.deliver(1_010_000, &[first]);
    // A valid frame in sequence raises no frame-level event.
    assert_eq!(run.app_events(), [cmd(0)]);
    assert_eq!(run.ocf_word(), 0x0100_0001);
    run.deliver(1_260_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [cmd(1)]);
    assert_eq!(run.ocf_word(), 0x0100_0002);
}

#[test]
fn the_vcf_count_is_n_s_over_its_whole_octet() {
    // req: APP-185, APP-044
    let mut run = Run::new_uslp();
    // N(S) 255 is V(R) after the Set V(R); the next in sequence wraps to 0 and then 1. Each of
    // 255, 0 and 1 differs from the others at every narrower count width.
    run.deliver(1_010_000, &[bc(ControlCommand::SetVr(255_u8.into()))]);
    run.app_events();
    assert_eq!(run.ocf_word(), 0x0100_02FF);
    run.deliver(1_260_000, &[ad_noop(255)]);
    assert_eq!(run.app_events(), [cmd(255)]);
    assert_eq!(run.ocf_word(), 0x0100_0200);
    run.deliver(1_510_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [cmd(0)]);
    assert_eq!(run.ocf_word(), 0x0100_0201);
}

#[test]
fn a_frame_in_the_positive_window_is_discarded_as_e3_and_sets_retransmit() {
    // req: APP-185, APP-186, APP-044
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [discarded(3)]);
    assert_eq!(run.ocf_word(), 0x0100_0800);
    // E1 clears it.
    run.deliver(1_260_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [cmd(0)]);
    assert_eq!(run.ocf_word(), 0x0100_0001);
}

#[test]
fn a_duplicate_is_discarded_as_e4_and_changes_nothing() {
    // req: APP-185, APP-186
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[ad_noop(0), ad_noop(1)]);
    run.app_events();
    run.deliver(1_260_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [discarded(4)]);
    assert_eq!(run.ocf_word(), 0x0100_0002);
}

#[test]
fn a_frame_in_the_lockout_area_is_e5_and_locks_the_channel() {
    // req: APP-185, APP-186, APP-044
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[ad_noop(10)]);
    assert_eq!(
        run.app_events(),
        [discarded(5), (APP_FARM1_STATE, std::vec![LOCKOUT])]
    );
    assert_eq!(run.ocf_word(), 0x0100_2000);
    // In Lockout the frame in sequence is discarded as E1 and delivers nothing.
    run.deliver(1_260_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [discarded(1)]);
    assert_eq!(run.ocf_word(), 0x0100_2000);
}

#[test]
fn unlock_leaves_lockout_counts_and_keeps_vr() {
    // req: APP-185, APP-186, APP-044
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[ad_noop(0), ad_noop(10)]);
    run.app_events();
    run.deliver(1_260_000, &[bc(ControlCommand::Unlock)]);
    assert_eq!(
        run.app_events(),
        [
            (APP_FARM1_UNLOCK, std::vec![]),
            (APP_FARM1_STATE, std::vec![1])
        ]
    );
    // V(R) = 1, Lockout clear, FARM-B Counter 1.
    assert_eq!(run.ocf_word(), 0x0100_0201);
    run.deliver(1_510_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [cmd(1)]);
}

#[test]
fn set_vr_moves_nr_and_counts() {
    // req: APP-185, APP-186
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[bc(ControlCommand::SetVr(5_u8.into()))]);
    // No notification of its own (E8); FARM-B Counter 1, N(R) = 5.
    assert_eq!(run.app_events(), []);
    assert_eq!(run.ocf_word(), 0x0100_0205);
    run.deliver(1_260_000, &[ad_noop(5)]);
    assert_eq!(run.app_events(), [cmd(5)]);
    assert_eq!(run.ocf_word(), 0x0100_0206);
}

#[test]
fn a_type_bd_frame_is_delivered_and_counts_in_the_farm_b_counter() {
    // req: APP-185, APP-043
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[bd(&noop(0))]);
    assert_eq!(run.app_events(), [cmd(0)]);
    // E6 leaves V(R) alone.
    assert_eq!(run.ocf_word(), 0x0100_0200);
}

#[test]
fn blocked_commands_in_one_tfdz_are_delivered_in_order() {
    // req: APP-183, APP-186
    let mut run = Run::new_uslp();
    let mut tfdz = command(3, 4, &[0]);
    tfdz.extend(noop(4));
    run.deliver(5_010_000, &[ad(0, &tfdz)]);
    assert_eq!(
        run.app_events(),
        [
            (CMD_RECEIVED, std::vec![4, 3]),
            (CMD_RECEIVED, std::vec![1, 4])
        ]
    );
    // One frame is one FDU for FARM-1: V(R) advances by one.
    assert_eq!(run.ocf_word(), 0x0100_0001);
}

#[test]
fn the_table_upload_is_one_whole_sdu_in_one_frame() {
    // req: APP-182, APP-186
    let mut run = Run::new_uslp();
    let table = table_packet();
    assert_eq!(table.len(), 309);
    let frame = ad(0, &table);
    // 7 (Primary Header) + 1 (VCF Count) + 1 (TFDF Header) + 309 + 2 (FECF), Frame Length 319.
    assert_eq!(frame.len(), 320);
    assert_eq!(u16::from_be_bytes([frame[4], frame[5]]), 319);
    run.deliver(2_010_000, &[frame]);
    assert_eq!(run.app_events(), [(TABLE_RECEIVED, std::vec![1, 300])]);
    assert_eq!(run.ocf_word(), 0x0100_0001);
}

#[test]
fn a_bc_frame_with_another_tfdz_is_e9_and_changes_nothing() {
    // req: APP-185, APP-186, COP-031
    let mut run = Run::new_uslp();
    run.deliver(1_010_000, &[ad_noop(0)]);
    run.app_events();
    // OI-100 (a): not one Unlock or Set V(R), two commands, one command with octets added, and
    // a Control Command under the UPID of Space Packets.
    let others = [
        bc_raw(&[1]),
        bc_raw(&[0, 0]),
        bc_raw(&[0x82, 0, 5, 0]),
        frame(FrameKind::ProtocolControl, 0, 0, 0, Upid::Packets, &[0]),
    ];
    for (i, other) in others.into_iter().enumerate() {
        run.deliver(1_260_000 + 250_000 * u64::try_from(i).unwrap(), &[other]);
        assert_eq!(
            run.app_events(),
            [rejected(ErrorKind::ReservedValue)],
            "{i}"
        );
        assert_eq!(run.ocf_word(), 0x0100_0001, "{i}");
    }
}

#[test]
fn a_frame_the_decode_refuses_is_rejected_and_the_next_one_is_accepted() {
    // req: APP-186, USLP-014
    let mut run = Run::new_uslp();
    let mut bad = ad_noop(0);
    bad[12] ^= 0x80;
    run.deliver(1_010_000, &[bad]);
    assert_eq!(run.app_events(), [rejected(ErrorKind::CrcMismatch)]);
    assert_eq!(run.ocf_word(), 0x0100_0000);
    run.deliver(1_260_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [cmd(0)]);
    // A wrong version fails the validation after the FECF is made right.
    let mut version = ad_noop(1);
    version[0] ^= 0x10;
    let body = version.len() - 2;
    let fecf = crc16(&version[..body]).get().to_be_bytes();
    version[body..].copy_from_slice(&fecf);
    run.deliver(1_510_000, &[version]);
    let events = run.app_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, APP_FRAME_REJECTED);
    assert_eq!(run.ocf_word(), 0x0100_0001);
}

#[test]
fn a_frame_for_another_vc_or_map_is_rejected_and_not_counted() {
    // req: APP-186, APP-181
    let mut run = Run::new_uslp();
    let other_vc = frame(FrameKind::Expedited, 1, 0, 0, Upid::Packets, &noop(0));
    let other_map = frame(FrameKind::Expedited, 0, 5, 0, Upid::Packets, &noop(0));
    for (i, other) in [other_vc, other_map].into_iter().enumerate() {
        run.deliver(1_010_000 + 250_000 * u64::try_from(i).unwrap(), &[other]);
        let events = run.app_events();
        assert_eq!(events.len(), 1, "{i}");
        assert_eq!(events[0].0, APP_FRAME_REJECTED, "{i}");
        assert_eq!(run.ocf_word(), 0x0100_0000, "{i}");
    }
}

#[test]
fn a_packet_that_is_not_a_command_is_rejected_like_at_packet_level() {
    // req: APP-186, APP-131
    let mut run = Run::new_uslp();
    // A TFDZ that is not a Space Packet: the reassembler refuses it, the handler never sees it.
    run.deliver(1_010_000, &[ad(0, &[0xFF; 4])]);
    let events = run.app_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 6);
}

#[test]
fn the_mc_ocf_of_the_downlink_carries_the_clcw_of_farm_1() {
    // req: APP-179, APP-185
    let mut run = Run::new_uslp();
    let slot = |run: &mut Run, now: u64, frames: &[Vec<u8>]| {
        let refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
        let input = StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(&refs),
            ops: &[],
            tx_slot: true,
        };
        run.rec.0.clear();
        run.node
            .step(&input, &mut crate::tests::At(now, &mut run.rec));
        let units: Vec<&Vec<u8>> = run
            .rec
            .0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Unit(octets) => Some(octets),
                Out::Packet(..) | Out::Event(..) => None,
            })
            .collect();
        // One frame per slot: 223 octets, the OCF before the FECF (APP-175, APP-179).
        assert_eq!(units.len(), 1);
        let frame = units[0];
        assert_eq!(frame.len(), 223);
        frame[frame.len() - 6..frame.len() - 2].to_vec()
    };
    // E1 in the slot's own step: the report is the one after the delivery (§4.4.1).
    assert_eq!(slot(&mut run, 1_000_000, &[ad_noop(0)]), [1, 0, 0, 1]);
    // E3 sets Retransmit; the next frame carries it.
    assert_eq!(slot(&mut run, 1_250_000, &[ad_noop(2)]), [1, 0, 8, 1]);
}

#[test]
fn gap_discard_is_event_115_on_uslp_and_none_on_tc() {
    // req: APP-186
    use aldis::link::protocol::{Tc, Uslp};

    use crate::frames_up::UpProtocol;
    // No frame of A4 raises it, so the mapping is pinned where it is made.
    assert_eq!(
        reassembly_code(
            ReassemblyEvent::GapDiscard,
            <Uslp as UpProtocol>::GAP_DISCARD
        ),
        Some(APP_GAP_DISCARD)
    );
    assert_eq!(APP_GAP_DISCARD, 115);
    assert_eq!(
        reassembly_code(ReassemblyEvent::GapDiscard, <Tc as UpProtocol>::GAP_DISCARD),
        None
    );
}
