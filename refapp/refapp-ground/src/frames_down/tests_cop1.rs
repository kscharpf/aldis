//! The ground node at the `cop1` level on scripted inputs: FOP-1 as the AD service, its CLCW
//! and timer inputs, the transmit step, the directives, and the events 200 to 203
//! (docs/reference-application.md §4.5.1, §4.5.2, §7.4; A3 WP-4).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use super::{CLCW_CHANGED, FrameDownlink};
use crate::{Call, Execution, Fop1Directive, GroundNode, OpsEntry};
use aldis::core::ids::{Apid, VcId};
use aldis::core::ocf::Ocf;
use aldis::core::oid::OidGenerator;
use aldis::tm::encode_oid;
use refapp_scenario::{Delivered, Instant, Node, Output, StepInput, TableId, TableLength};

/// One output of a step, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rec {
    Unit(Vec<u8>),
    Event(u16, Vec<u64>),
}

#[derive(Default)]
struct Recorder(Vec<Rec>);

impl Output for Recorder {
    fn unit(&mut self, octets: &[u8]) {
        self.0.push(Rec::Unit(octets.to_vec()));
    }
    fn packet_out(&mut self, _: Apid, _: &[u8]) {}
    fn packet_in(&mut self, _: Apid, _: &[u8]) {}
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.0.push(Rec::Event(code, args.to_vec()));
    }
}

const SLOT: u64 = 250_000;
const FOP1_ALERT: u16 = 200;
const FOP1_TRANSFER: u16 = 201;
const FOP1_DIRECTIVE: u16 = 202;
const FOP1_STATE: u16 = 203;
const UPLINK_REFUSED: u16 = 114;

const fn event(code: u16, args: &[u64]) -> (u16, &[u64]) {
    (code, args)
}

/// A node at `cop1`, the script with the instant of each entry, and the counters of the downlink
/// frames it is given.
struct Rig {
    node: GroundNode,
    at: Vec<u64>,
    down: FrameDownlink,
    mc: u8,
}

/// What one step output: the frames and the events, with the deadline the node asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    now: u64,
    recs: Vec<Rec>,
    deadline: Option<u64>,
}

impl Step {
    fn units(&self) -> Vec<&Vec<u8>> {
        self.recs
            .iter()
            .filter_map(|r| match r {
                Rec::Unit(u) => Some(u),
                Rec::Event(..) => None,
            })
            .collect()
    }

    fn events(&self) -> Vec<(u16, &[u64])> {
        self.recs
            .iter()
            .filter_map(|r| match r {
                Rec::Event(code, args) => Some((*code, args.as_slice())),
                Rec::Unit(_) => None,
            })
            .collect()
    }
}

impl Rig {
    fn new(entries: &[(u64, OpsEntry)]) -> Rig {
        let script: Vec<OpsEntry> = entries.iter().map(|(_, e)| *e).collect();
        Rig {
            node: GroundNode::new_cop1(&script).unwrap(),
            at: entries.iter().map(|(t, _)| *t).collect(),
            down: FrameDownlink::new().unwrap(),
            mc: 0,
        }
    }

    /// An OID frame of the downlink with `ocf` and the next counts (no gap events).
    fn frame(&mut self, ocf: u32) -> Vec<u8> {
        let mut out = vec![0; 223];
        let n = encode_oid(
            &self.down.channel,
            VcId::new(7).unwrap(),
            self.mc,
            self.mc,
            &[],
            Some(Ocf::decode(ocf.to_be_bytes())),
            &mut OidGenerator::new(),
            &mut out,
        )
        .unwrap();
        self.mc = self.mc.wrapping_add(1);
        out.truncate(n);
        out
    }

    /// One step at `now` (µs) that delivers a frame with each OCF word of `ocfs`; the link
    /// takes a unit at every multiple of the uplink slot.
    fn step(&mut self, now: u64, ocfs: &[u32]) -> Step {
        let frames: Vec<Vec<u8>> = ocfs.iter().map(|&w| self.frame(w)).collect();
        let units: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
        let ops: Vec<u16> = self
            .at
            .iter()
            .enumerate()
            .filter(|&(_, &when)| when == now)
            .map(|(i, _)| u16::try_from(i).unwrap())
            .collect();
        let input = StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(&units),
            ops: &ops,
            tx_slot: now.is_multiple_of(SLOT),
        };
        let mut out = Recorder::default();
        let deadline = self.node.step(&input, &mut out).map(Instant::as_micros);
        Step {
            now,
            recs: out.0,
            deadline,
        }
    }

    /// Steps at every slot from `from` to `until`, delivering nothing.
    fn slots(&mut self, from: u64, until: u64) -> Vec<Step> {
        (from..=until)
            .step_by(usize::try_from(SLOT).unwrap())
            .map(|t| self.step(t, &[]))
            .collect()
    }
}

/// The CLCW word of §4.5.3 for VC 0: N(R) and the flags.
fn clcw(n_r: u8, retransmit: bool, lockout: bool) -> u32 {
    0x0100_0000
        + u32::from(n_r)
        + if retransmit { 0x0800 } else { 0 }
        + if lockout { 0x2000 } else { 0 }
}

const fn noop() -> OpsEntry {
    OpsEntry::Command {
        execution: Execution::Immediate,
        call: Call::Noop,
        extra_octets: 0,
    }
}

const fn directive(d: Fop1Directive) -> OpsEntry {
    OpsEntry::Fop1Directive(d)
}

/// N(S) of a TC frame (octet 4, the low 8 bits of the sequence field) and its first octet.
fn kind_and_ns(frame: &[u8]) -> (u8, u8) {
    (frame[0], frame[4])
}

const AD: u8 = 0x01;
const BC: u8 = 0x31;

#[test]
fn the_first_directive_and_a_nominal_acknowledgement() {
    // req: APP-044, APP-144, APP-145, APP-149, COP-010
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (1_000_000, noop()),
    ]);
    let at_zero = rig.step(0, &[]);
    // E23 leaves S6 for S1 in one cell: Accept, Positive Confirm, then the state (§4.5.5).
    assert_eq!(
        at_zero.events(),
        [
            event(FOP1_DIRECTIVE, &[1, 1, 0]),
            event(FOP1_DIRECTIVE, &[3, 1, 0]),
            event(FOP1_STATE, &[1]),
        ]
    );
    assert_eq!(at_zero.units().len(), 0);
    assert_eq!(at_zero.deadline, None);
    for step in rig.slots(250_000, 750_000) {
        assert_eq!(step.recs, []);
    }
    let sent = rig.step(1_000_000, &[]);
    let units = sent.units();
    assert_eq!(units.len(), 1);
    assert_eq!(kind_and_ns(units[0]), (AD, 0));
    // The frame is the last output of the step; the timer runs from the request at the slot.
    assert!(matches!(sent.recs.last(), Some(Rec::Unit(_))));
    assert_eq!(sent.deadline, Some(2_000_000));
    // The CLCW of the frame at 1.135 s: the change, then the Positive Confirm of FDU 0 (E2).
    let ack = rig.step(1_135_000, &[clcw(1, false, false)]);
    assert_eq!(
        ack.events(),
        [
            event(CLCW_CHANGED, &[u64::from(clcw(1, false, false))]),
            event(FOP1_TRANSFER, &[3, 0]),
        ]
    );
    assert_eq!(ack.deadline, None);
    assert_eq!(rig.node.uplink_refused(), 0);
}

#[test]
fn the_clcw_of_a_frame_comes_before_the_directive_of_the_same_instant() {
    // req: APP-036, APP-144
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (1_000_000, noop()),
        (1_135_000, directive(Fop1Directive::TerminateAd)),
    ]);
    rig.step(0, &[]);
    rig.slots(250_000, 1_000_000);
    let both = rig.step(1_135_000, &[clcw(1, false, false)]);
    let codes: Vec<u16> = both.events().iter().map(|e| e.0).collect();
    // clcw_changed, the Positive Confirm, and only then the directive: Accept, the Alert (the
    // Terminate in S1 raises Alert[term], E29), Positive Confirm, and the state.
    assert_eq!(
        codes,
        [
            CLCW_CHANGED,
            FOP1_TRANSFER,
            FOP1_DIRECTIVE,
            FOP1_ALERT,
            FOP1_DIRECTIVE,
            FOP1_STATE
        ]
    );
    assert_eq!(both.events()[3], event(FOP1_ALERT, &[8, 29, 1]));
    assert_eq!(both.events()[5], event(FOP1_STATE, &[6]));
}

#[test]
fn a_word_that_is_not_a_clcw_is_invalid_clcw() {
    // req: APP-036, APP-144, COP-004
    let mut rig = Rig::new(&[(0, directive(Fop1Directive::InitiateAdWithoutClcw))]);
    rig.step(0, &[]);
    // Control Word Type 1 (bit 0): `Clcw::decode` refuses it, FOP-1 takes E15 (Alert[CLCW]).
    let bad = rig.step(135_000, &[0x8100_0000]);
    assert_eq!(
        bad.events(),
        [
            event(CLCW_CHANGED, &[0x8100_0000]),
            event(FOP1_ALERT, &[6, 15, 1]),
            event(FOP1_STATE, &[6]),
        ]
    );
}

#[test]
fn a_retransmission_follows_the_retransmit_flag_in_order_before_a_new_frame() {
    // req: APP-144, COP-011, APP-090
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (1_000_000, noop()),
        (1_250_000, noop()),
        (1_500_000, noop()),
        (1_750_000, noop()),
    ]);
    rig.step(0, &[]);
    let mut frames = Vec::new();
    for t in [1_000_000, 1_250_000, 1_500_000] {
        frames.push(rig.step(t, &[]).units()[0].clone());
    }
    assert_eq!(
        frames.iter().map(|f| kind_and_ns(f)).collect::<Vec<_>>(),
        [(AD, 0), (AD, 1), (AD, 2)]
    );
    // The CLCW of frame 0 reports N(R) = 1 with Retransmit: frames 1 and 2 are lost.
    let flag = rig.step(1_635_000, &[clcw(1, true, false)]);
    assert_eq!(
        flag.events(),
        [
            event(CLCW_CHANGED, &[u64::from(clcw(1, true, false))]),
            event(FOP1_TRANSFER, &[3, 0]),
            event(FOP1_STATE, &[2]),
        ]
    );
    assert_eq!(flag.units().len(), 0);
    // Both leave in the next slots in queue order, before the new command of 1.75 s, which
    // FOP-1 takes only after them.
    let first = rig.step(1_750_000, &[]);
    assert_eq!(first.units(), [&frames[1]]);
    let second = rig.step(2_000_000, &[]);
    assert_eq!(second.units(), [&frames[2]]);
    assert_eq!(rig.node.uplink_refused(), 0);
}

#[test]
fn a_frame_is_retransmitted_when_the_timer_expires() {
    // req: APP-044, APP-074, APP-144, COP-011, COP-013
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (1_000_000, noop()),
    ]);
    rig.step(0, &[]);
    let first = rig.step(1_000_000, &[]);
    let frame = first.units()[0].clone();
    assert_eq!(first.deadline, Some(2_000_000));
    // A step at an instant where nothing is due changes no output (APP-074).
    let idle = rig.step(1_500_000, &[]);
    assert_eq!(
        (idle.recs.as_slice(), idle.deadline),
        (&[][..], Some(2_000_000))
    );
    // At the deadline `Timeout` is given before the slot, so the frame leaves in that slot;
    // the timer restarts (E16, Transmission_Count 2).
    let again = rig.step(2_000_000, &[]);
    assert_eq!(again.units(), [&frame]);
    assert_eq!(again.events(), []);
    assert_eq!(again.deadline, Some(3_000_000));
}

#[test]
fn the_transmission_limit_raises_the_alert_and_the_node_does_nothing_more() {
    // req: APP-044, APP-145, COP-004, COP-013
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (1_000_000, noop()),
        (5_000_000, noop()),
    ]);
    rig.step(0, &[]);
    let frame = rig.step(1_000_000, &[]).units()[0].clone();
    let second = rig.step(2_000_000, &[]);
    assert_eq!(second.units(), [&frame]);
    let third = rig.step(3_000_000, &[]);
    assert_eq!(third.units(), [&frame]);
    // Transmission_Limit = 3 counts the first transmission: the third expiry alerts (E17) with
    // the negative confirm of FDU 0 first, the state change last (§7.4).
    let last = rig.step(4_000_000, &[]);
    assert_eq!(last.units().len(), 0);
    assert_eq!(
        last.events(),
        [
            event(FOP1_TRANSFER, &[4, 0]),
            event(FOP1_ALERT, &[2, 17, 1]),
            event(FOP1_STATE, &[6]),
        ]
    );
    assert_eq!(last.deadline, None);
    // Nothing is resubmitted. The next command is rejected once, in S6, as `fop1_transfer`
    // and not as `uplink_refused` (D6, APP-145), and nothing leaves.
    for step in rig.slots(4_250_000, 4_750_000) {
        assert_eq!(step.recs, []);
    }
    let rejected = rig.step(5_000_000, &[]);
    assert_eq!(rejected.events(), [event(FOP1_TRANSFER, &[2, 1])]);
    assert_eq!(rejected.units().len(), 0);
    for step in rig.slots(5_250_000, 6_000_000) {
        assert_eq!(step.recs, []);
    }
    assert_eq!(rig.node.uplink_refused(), 0);
}

#[test]
fn the_window_of_four_blocks_the_commands_behind_a_table_upload() {
    // req: APP-044, APP-090, APP-145, COP-001
    let table = OpsEntry::TableUpload {
        table_id: TableId::new(1).unwrap(),
        length: TableLength::new(300).unwrap(),
    };
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (0, table),
        (0, noop()),
        (500_000, noop()),
        (750_000, noop()),
        (1_000_000, noop()),
        (1_250_000, noop()),
    ]);
    let steps: Vec<Step> = (0..=1_500_000)
        .step_by(250_000)
        .map(|t| rig.step(t, &[]))
        .collect();
    // One frame per slot, N(S) 0 to 3: the three segments of the table, then the commands.
    let sent: Vec<(u64, u8)> = steps
        .iter()
        .flat_map(|s| s.units().into_iter().map(|u| (s.now, kind_and_ns(u).1)))
        .collect();
    assert_eq!(
        sent,
        [(0, 0), (250_000, 1), (500_000, 2), (750_000, 3)],
        "window K = 4: nothing leaves while four frames are unacknowledged"
    );
    assert!(steps.iter().all(|s| s.units().len() <= 1));
    // The CLCW acknowledges all four (E2): the FDU waiting in FOP-1 and the one after it go
    // out in the next slots, N(S) 4 and 5.
    let ack = rig.step(1_635_000, &[clcw(4, false, false)]);
    let confirms: Vec<_> = ack
        .events()
        .into_iter()
        .filter(|e| e.0 == FOP1_TRANSFER)
        .collect();
    assert_eq!(
        confirms,
        [
            event(FOP1_TRANSFER, &[3, 0]),
            event(FOP1_TRANSFER, &[3, 1]),
            event(FOP1_TRANSFER, &[3, 2]),
            event(FOP1_TRANSFER, &[3, 3]),
        ]
    );
    let next = rig.slots(1_750_000, 2_000_000);
    let after: Vec<u8> = next
        .iter()
        .flat_map(|s| s.units().into_iter().map(|u| kind_and_ns(u).1))
        .collect();
    assert_eq!(after, [4, 5]);
    assert_eq!(rig.node.uplink_refused(), 0);
}

#[test]
fn an_initiate_with_unlock_sends_a_bc_frame_in_its_slot() {
    // req: APP-144, COP-010, TC-008, COP-036
    let mut rig = Rig::new(&[(0, directive(Fop1Directive::InitiateAdWithUnlock))]);
    let at_zero = rig.step(0, &[]);
    assert_eq!(
        at_zero.events(),
        [event(FOP1_DIRECTIVE, &[1, 3, 0]), event(FOP1_STATE, &[5])]
    );
    let units = at_zero.units();
    assert_eq!(units.len(), 1);
    // Type-BC (Bypass 1, Control Command 1): first octet 0x31, N(S) 0, the Unlock command
    // octet 0x00 and the FECF: 5 + 1 + 2 = 8 octets (232.0 §4.1.3.3).
    assert_eq!(kind_and_ns(units[0]), (BC, 0));
    assert_eq!((units[0].len(), units[0][5]), (8, 0x00));
    // The CLCW of the Unlock (E1 in S5; the word is the initial one, so no `clcw_changed`)
    // confirms the directive and leaves S5.
    let done = rig.step(135_000, &[clcw(0, false, false)]);
    assert_eq!(
        done.events(),
        [event(FOP1_DIRECTIVE, &[3, 3, 0]), event(FOP1_STATE, &[1]),]
    );
}

#[test]
fn set_vs_after_a_terminate_sets_the_sequence_number_of_the_next_frame() {
    // req: APP-044, APP-148, COP-015
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (250_000, directive(Fop1Directive::TerminateAd)),
        (250_000, directive(Fop1Directive::SetVs(5))),
        (250_000, directive(Fop1Directive::InitiateAdWithClcw)),
        (500_000, noop()),
    ]);
    rig.step(0, &[]);
    let recovery = rig.step(250_000, &[]);
    // Terminate in S1: Accept, Alert[term], Positive Confirm; Set V(S) in S6: Accept and
    // Positive Confirm with the Qualifier; Initiate with CLCW check: Accept, S4.
    assert_eq!(
        recovery.events(),
        [
            event(FOP1_DIRECTIVE, &[1, 5, 0]),
            event(FOP1_ALERT, &[8, 29, 1]),
            event(FOP1_DIRECTIVE, &[3, 5, 0]),
            event(FOP1_STATE, &[6]),
            event(FOP1_DIRECTIVE, &[1, 7, 5]),
            event(FOP1_DIRECTIVE, &[3, 7, 5]),
            event(FOP1_DIRECTIVE, &[1, 2, 0]),
            event(FOP1_STATE, &[4]),
        ]
    );
    // The first CLCW after it is clean, N(R) = NN(R) = 5: E1 in S4 confirms and goes to S1.
    let clean = rig.step(385_000, &[clcw(5, false, false)]);
    assert_eq!(
        clean.events(),
        [
            event(CLCW_CHANGED, &[u64::from(clcw(5, false, false))]),
            event(FOP1_DIRECTIVE, &[3, 2, 0]),
            event(FOP1_STATE, &[1]),
        ]
    );
    let frame = rig.step(500_000, &[]);
    assert_eq!(kind_and_ns(frame.units()[0]), (AD, 5));
}

#[test]
fn a_bc_frame_leaves_before_a_held_ad_frame() {
    // req: APP-071, APP-144, COP-010
    let mut rig = Rig::new(&[
        (0, directive(Fop1Directive::InitiateAdWithoutClcw)),
        (1_000_000, noop()),
        (1_250_000, noop()),
        (1_500_000, noop()),
        (1_750_000, directive(Fop1Directive::TerminateAd)),
        (1_750_000, directive(Fop1Directive::InitiateAdWithUnlock)),
    ]);
    rig.step(0, &[]);
    rig.slots(1_000_000, 1_500_000);
    // Frame 1 is held after the Retransmit flag, and at 1.75 s the directives return a BC
    // frame as well: one frame leaves per slot, the BC frame first (§4.5.1).
    let flag = rig.step(1_635_000, &[clcw(1, true, false)]);
    assert_eq!(flag.units().len(), 0);
    let first = rig.step(1_750_000, &[]);
    assert_eq!(first.units().len(), 1);
    assert_eq!(kind_and_ns(first.units()[0]).0, BC);
    let second = rig.step(2_000_000, &[]);
    assert_eq!(second.units().len(), 1);
    assert_eq!(kind_and_ns(second.units()[0]).0, AD);
    assert_eq!(rig.node.uplink_refused(), 0);
}

#[test]
fn an_input_the_engine_refuses_is_uplink_refused() {
    // req: APP-143, APP-145
    // A CLCW of another VC is an error of the engine (`UnknownChannel`, 10), not a Reject.
    let mut rig = Rig::new(&[(0, directive(Fop1Directive::InitiateAdWithoutClcw))]);
    rig.step(0, &[]);
    let other_vc = 0x0100_0000 | (1 << 18);
    let step = rig.step(135_000, &[other_vc]);
    assert_eq!(
        step.events(),
        [
            event(CLCW_CHANGED, &[u64::from(other_vc)]),
            event(UPLINK_REFUSED, &[3, 10, 0]),
        ]
    );
    assert_eq!(rig.node.uplink_refused(), 1);
}
