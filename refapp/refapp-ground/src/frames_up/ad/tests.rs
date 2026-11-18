//! `slot_ad` when FOP-1 refuses a `RequestAd` with an error (docs/reference-application.md
//! §4.5.1, §7.4; APP-143, APP-145).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use super::Uplink;
use crate::ops::Fop1Directive;
use aldis::cop1::{Fop1, Fop1Config, Fop1Target, FrameKind, TimeoutType};
use aldis::core::ErrorKind;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::time::{Duration, Instant};
use aldis::tc::PhysicalChannelConfig;
use core::num::NonZeroU8;
use refapp_scenario::generated::{SPACECRAFT_ID, apid, cop1, tc};
use refapp_scenario::{Apid, Output};

#[derive(Default)]
struct Recorder {
    units: usize,
    /// The octets of each unit output, in order.
    frames: Vec<Vec<u8>>,
    events: Vec<(u16, Vec<u64>)>,
}

impl Output for Recorder {
    fn unit(&mut self, unit: &[u8]) {
        self.units += 1;
        self.frames.push(unit.to_vec());
    }
    fn packet_out(&mut self, _: Apid, _: &[u8]) {}
    fn packet_in(&mut self, _: Apid, _: &[u8]) {}
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.events.push((code, args.to_vec()));
    }
}

/// A command packet of `len` octets on the commands APID.
fn packet(len: usize) -> Vec<u8> {
    let mut packet = vec![0; len];
    let word0 = 0x1000 | apid::COMMANDS;
    packet[..2].copy_from_slice(&word0.to_be_bytes());
    packet[2..4].copy_from_slice(&0xC000_u16.to_be_bytes());
    let data_len = u16::try_from(len - 7).unwrap();
    packet[4..6].copy_from_slice(&data_len.to_be_bytes());
    packet
}

/// An uplink at `cop1` whose FOP-1 takes FDUs of at most a short frame, so that it refuses
/// the FDU of a long packet with `OutOfRange`.
fn uplink_with_short_fop1() -> Uplink {
    let mut uplink = Uplink::new_cop1().unwrap();
    let spacecraft = SpacecraftId::<10>::new(SPACECRAFT_ID).unwrap();
    let vc = VcId::<6>::new(tc::VCID).unwrap();
    let physical = PhysicalChannelConfig::new(32, tc::FECF).unwrap();
    uplink.fop = Fop1::new(Fop1Config {
        target: Fop1Target::tc(spacecraft, vc, physical).unwrap(),
        t1_initial: Duration::from_micros(cop1::T1_INITIAL_US),
        transmission_limit: NonZeroU8::new(cop1::TRANSMISSION_LIMIT).unwrap(),
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: cop1::K,
        ad_repetitions: cop1::FOP_REPETITIONS_TYPE_A,
        bc_repetitions: cop1::FOP_REPETITIONS_CONTROL,
    })
    .unwrap();
    uplink
}

#[test]
fn a_request_ad_the_engine_refuses_is_not_unanswered_and_the_next_slot_requests_again() {
    // req: APP-143, APP-145
    let mut uplink = uplink_with_short_fop1();
    let mut out = Recorder::default();
    let now = Instant::from_micros(0);
    uplink.submit(
        Apid::new(apid::COMMANDS).unwrap(),
        &packet(100),
        now,
        &mut out,
    );
    uplink.submit(
        Apid::new(apid::COMMANDS).unwrap(),
        &packet(100),
        now,
        &mut out,
    );
    uplink.slot_ad(now, &mut out);
    // The refusal is reported once, at the FOP-1 stage (APP-143); no frame leaves.
    let code = u64::from(ErrorKind::OutOfRange.code());
    assert_eq!(out.events, [(114, vec![3, code, 0])]);
    assert_eq!(uplink.refused(), 1);
    assert_eq!(out.units, 0);
    // A refused request has no answer to wait for: the next slot requests the next FDU (the
    // old code stayed unanswered and never took one again) and the node does not retry the
    // lost one (APP-143), so the segmenter's next FDU is the one refused now.
    let ad = uplink.ad.as_ref().unwrap();
    assert!(!ad.unanswered);
    assert_eq!(ad.next_request, 1);
    uplink.slot_ad(now, &mut out);
    assert_eq!(uplink.refused(), 2);
}

#[test]
fn a_frame_held_across_a_slot_leaves_with_the_bytes_the_engine_holds() {
    // req: APP-071, APP-101, APP-144, COP-011
    // The node keeps no copy of a held transmit request: at the slot it outputs what
    // `Fop1::held_frame` returns (232.1 §5.1.7).
    let mut uplink = Uplink::new_cop1().unwrap();
    let mut out = Recorder::default();
    let at = |ms: u64| Instant::from_micros(ms * 1_000);
    uplink.directive(Fop1Directive::InitiateAdWithoutClcw, at(0), &mut out);
    for _ in 0..3 {
        uplink.submit(
            Apid::new(apid::COMMANDS).unwrap(),
            &packet(100),
            at(0),
            &mut out,
        );
    }
    // Frames 0, 1, and 2 leave in three slots.
    for slot in 1..=3 {
        uplink.slot_ad(at(slot * 250), &mut out);
    }
    assert_eq!(out.frames.len(), 3);
    // The Retransmit flag with N(R) = 1 returns frames 1 and 2: the first is held, not output.
    uplink.clcw(0x0100_0000 + 1 + 0x0800, at(800), &mut out);
    assert_eq!(out.frames.len(), 3);
    assert!(!uplink.fop.out_flag_ready(FrameKind::Ad));
    let held = uplink.fop.held_frame(FrameKind::Ad).unwrap().to_vec();
    assert_eq!(held[4], 1);
    assert_eq!(held, out.frames[1]);
    // The slot outputs exactly those bytes, as its last output; the next one, frame 2.
    uplink.slot_ad(at(1_000), &mut out);
    assert_eq!(out.frames.len(), 4);
    assert_eq!(out.frames[3], held);
    let next = uplink.fop.held_frame(FrameKind::Ad).unwrap().to_vec();
    assert_eq!(next[4], 2);
    uplink.slot_ad(at(1_250), &mut out);
    assert_eq!(out.frames.len(), 5);
    assert_eq!(out.frames[4], next);
    assert_eq!(uplink.refused(), 0);
}
