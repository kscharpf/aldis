//! Tests of the uplink at the `cop1` level: FARM-1 for Type-AD and Type-BC frames and its
//! events (docs/reference-application.md §4.5.3, §7.4; APP-146, APP-147, APP-149).

extern crate std;

use std::vec::Vec;

use aldis::core::ErrorKind;
use aldis::core::crc::crc16;
use aldis::core::ids::FrameSeqNum;
use aldis::tc::{ControlCommand, FrameData, FrameFields, FrameType, SegmentHeader, SequenceFlags};
use aldis::tc::{TcMapId, TcSpacecraftId, TcVcId, encode};
use refapp_scenario::generated::SPACECRAFT_ID;

use super::{APP_FRAME_REJECTED, APP_SEQUENCE_VIOLATION, CMD_RECEIVED, Run, code, noop, sender};
use crate::FlightNode;
use crate::frames_up::{APP_FARM1_STATE, APP_FARM1_UNLOCK, APP_FRAME_DISCARDED};

const OPEN: u64 = 1;
const LOCKOUT: u64 = 3;

fn fields(frame_type: FrameType, n_s: u8) -> FrameFields {
    FrameFields {
        frame_type,
        spacecraft_id: TcSpacecraftId::new(SPACECRAFT_ID).unwrap(),
        vc_id: TcVcId::new(0).unwrap(),
        sequence_number: FrameSeqNum::new(n_s),
    }
}

/// A Type-AD frame with N(S) = `n_s` on MAP `map`.
fn ad_frame(n_s: u8, flags: SequenceFlags, map: u8, fdu: &[u8]) -> Vec<u8> {
    let data = FrameData::Fdu {
        segment_header: Some(SegmentHeader {
            sequence_flags: flags,
            map_id: TcMapId::new(map).unwrap(),
        }),
        user_data: fdu,
    };
    let mut out = [0_u8; 128];
    let n = encode(&sender(), fields(FrameType::Ad, n_s), data, &mut out).unwrap();
    out[..n].to_vec()
}

/// A Type-AD frame with a command in N(S) order.
fn ad_noop(n_s: u8) -> Vec<u8> {
    ad_frame(n_s, SequenceFlags::Unsegmented, 0, &noop(u16::from(n_s)))
}

/// A Type-BC frame with `command`.
fn bc_frame(command: ControlCommand) -> Vec<u8> {
    let mut out = [0_u8; 128];
    let n = encode(
        &sender(),
        fields(FrameType::Bc, 0),
        FrameData::Control(command),
        &mut out,
    )
    .unwrap();
    out[..n].to_vec()
}

fn discarded(event: u64) -> (u16, Vec<u64>) {
    (APP_FRAME_DISCARDED, std::vec![2, event])
}

fn cmd(count: u64) -> (u16, Vec<u64>) {
    (CMD_RECEIVED, std::vec![1, count])
}

#[test]
fn an_ad_frame_in_sequence_is_delivered_and_advances_nr() {
    // req: APP-146, APP-147, APP-040
    let mut run = Run::new_cop1();
    run.deliver(1_010_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [cmd(0)]);
    assert_eq!(run.ocf_word(), 0x0100_0001);
    run.deliver(1_260_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [cmd(1)]);
    assert_eq!(run.ocf_word(), 0x0100_0002);
}

#[test]
fn a_frame_in_the_positive_window_sets_retransmit_and_the_next_in_sequence_clears_it() {
    // req: APP-146, APP-147, APP-149
    let mut run = Run::new_cop1();
    // E3: N(S) = V(R) + 1 is discarded and Retransmit set; no state change, so no event 210.
    run.deliver(1_010_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [discarded(3)]);
    assert_eq!(run.ocf_word(), 0x0100_0800);
    // E1 clears it.
    run.deliver(1_260_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [cmd(0)]);
    assert_eq!(run.ocf_word(), 0x0100_0001);
}

#[test]
fn a_duplicate_is_discarded_and_changes_nothing() {
    // req: APP-146, APP-147
    let mut run = Run::new_cop1();
    run.deliver(1_010_000, &[ad_noop(0), ad_noop(1)]);
    run.app_events();
    // E4: N(S) = V(R) - 1 is in the negative window.
    run.deliver(1_260_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [discarded(4)]);
    assert_eq!(run.ocf_word(), 0x0100_0002);
}

#[test]
fn a_frame_in_the_lockout_area_locks_the_channel_until_unlock() {
    // req: APP-146, APP-147, APP-149
    let mut run = Run::new_cop1();
    // E5: discard, then the state change after it.
    run.deliver(1_010_000, &[ad_noop(10)]);
    assert_eq!(
        run.app_events(),
        [discarded(5), (APP_FARM1_STATE, std::vec![LOCKOUT])]
    );
    assert_eq!(run.ocf_word(), 0x0100_2000);
    // In Lockout every AD frame is discarded, E1 for the one in sequence, with no new state.
    run.deliver(1_260_000, &[ad_noop(0)]);
    assert_eq!(run.app_events(), [discarded(1)]);
    assert_eq!(run.ocf_word(), 0x0100_2000);
}

#[test]
fn unlock_leaves_lockout_counts_and_keeps_vr() {
    // req: APP-146, APP-147, APP-149
    let mut run = Run::new_cop1();
    run.deliver(1_010_000, &[ad_noop(0), ad_noop(10)]);
    run.app_events();
    run.deliver(1_260_000, &[bc_frame(ControlCommand::Unlock)]);
    assert_eq!(
        run.app_events(),
        [
            (APP_FARM1_UNLOCK, std::vec![]),
            (APP_FARM1_STATE, std::vec![OPEN])
        ]
    );
    // V(R) = 1, Lockout clear, FARM-B Counter 1.
    assert_eq!(run.ocf_word(), 0x0100_0201);
    run.deliver(1_510_000, &[ad_noop(1)]);
    assert_eq!(run.app_events(), [cmd(1)]);
}

#[test]
fn unlock_in_the_open_state_raises_only_the_unlock() {
    // req: APP-146, APP-147
    let mut run = Run::new_cop1();
    run.deliver(1_010_000, &[ad_noop(1)]);
    run.app_events();
    run.deliver(1_260_000, &[bc_frame(ControlCommand::Unlock)]);
    // The state stays S1, so no event 210; Retransmit is cleared.
    assert_eq!(run.app_events(), [(APP_FARM1_UNLOCK, std::vec![])]);
    assert_eq!(run.ocf_word(), 0x0100_0200);
}

#[test]
fn unlock_purges_a_segment_in_progress() {
    // req: APP-146, D4 c of the A3 plan
    let mut run = Run::new_cop1();
    let command = noop(0);
    let (head, tail) = command.split_at(command.len() / 2);
    run.deliver(1_010_000, &[ad_frame(0, SequenceFlags::First, 1, head)]);
    assert_eq!(run.app_events(), []);
    run.deliver(1_260_000, &[ad_noop(20)]);
    run.deliver(1_510_000, &[bc_frame(ControlCommand::Unlock)]);
    run.app_events();
    // The last segment would complete the command without the purge.
    run.deliver(1_760_000, &[ad_frame(1, SequenceFlags::Last, 1, tail)]);
    assert_eq!(run.app_events(), [(APP_SEQUENCE_VIOLATION, std::vec![1])]);
}

#[test]
fn set_vr_moves_nr_and_counts() {
    // req: APP-146, APP-147
    let mut run = Run::new_cop1();
    run.deliver(
        1_010_000,
        &[bc_frame(ControlCommand::SetVr(FrameSeqNum::new(5)))],
    );
    // No notification of its own (E8); FARM-B Counter 1, N(R) = 5.
    assert_eq!(run.app_events(), []);
    assert_eq!(run.ocf_word(), 0x0100_0205);
    run.deliver(1_260_000, &[ad_noop(5)]);
    assert_eq!(run.app_events(), [cmd(5)]);
    assert_eq!(run.ocf_word(), 0x0100_0206);
}

#[test]
fn a_bc_frame_with_another_data_field_is_rejected_and_changes_nothing() {
    // req: APP-146, APP-131, COP-031
    let mut run = Run::new_cop1();
    run.deliver(1_010_000, &[ad_noop(0)]);
    run.app_events();
    let mut frame = bc_frame(ControlCommand::Unlock);
    let data = frame.len() - 3;
    frame[data] = 0x01;
    let body = frame.len() - 2;
    let fecf = crc16(&frame[..body]).get().to_be_bytes();
    frame[body..].copy_from_slice(&fecf);
    run.deliver(1_260_000, &[frame]);
    assert_eq!(
        run.app_events(),
        [(
            APP_FRAME_REJECTED,
            std::vec![code(ErrorKind::ReservedValue)]
        )]
    );
    assert_eq!(run.ocf_word(), 0x0100_0001);
}

#[test]
fn the_frames_level_raises_no_cop1_event() {
    // req: APP-146, APP-139
    let mut run = Run::new();
    run.deliver(1_010_000, &[ad_noop(10)]);
    assert_eq!(run.app_events(), [discarded(5)]);
    run.deliver(1_260_000, &[bc_frame(ControlCommand::Unlock)]);
    assert_eq!(run.app_events(), []);
    assert_eq!(
        FlightNode::new_cop1().unwrap().clcw().unwrap().to_word(),
        0x0100_0000
    );
}
