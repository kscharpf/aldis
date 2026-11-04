//! Kani proof harnesses for FARM-1 (docs/verification/formal-verification.md, OI-42).
//!
//! Compiled only by `cargo kani` (`cfg(kani)`); not part of the library, the tests, or the
//! coverage run. The expected results are written from Table 6-1 and §6.1.8 of CCSDS 232.1-B-2
//! (`SPEC_NEXT`, `spec`, and the distances in the harnesses), not from `FARM1_TABLE` or
//! `Farm1::ad_event`.
//!
//! The harnesses of table exhaustiveness (`every_cell_matches_table_6_1`) and window arithmetic
//! (`window_constructors_accept_exactly_table_7_2`, `ad_frame_classification_modulo_256`) take
//! every state, event, V(R), N(S), and window, with no bound. The step harness
//! (`step_obeys_table_6_1`) starts from an arbitrary state (any State,
//! V(R), Retransmit Flag, and FARM-B Counter, with a window that Table 7-2 allows) and checks one
//! input of any kind against Table 6-1: next state, V(R), counter, flags, and actions. Its
//! `invariant` is only the window constraint, which is configuration and which a step cannot
//! change, so the harness is a one-step check from every state, not an inductive proof of a state
//! invariant. Buffers: the FDU is 0 to 2 octets and the output area 2 octets.

use aldis_core::ErrorKind;
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::Instant;
use aldis_tc::{ClcwStatus, ControlCommand, FarmBCounter, TcVcId};

use super::{
    Context, FARM1_MAX_ACTIONS, Farm1, Farm1Config, Farm1Frame, Farm1Input, Farm1Notify,
    Farm1Output, Farm1Window, FduKind,
};
use crate::farm1_table::{EVENT_COUNT, Farm1Event, Farm1State, STATE_COUNT};

const EVENTS: [Farm1Event; EVENT_COUNT] = [
    Farm1Event::E1,
    Farm1Event::E2,
    Farm1Event::E3,
    Farm1Event::E4,
    Farm1Event::E5,
    Farm1Event::E6,
    Farm1Event::E7,
    Farm1Event::E8,
    Farm1Event::E9,
    Farm1Event::E10,
    Farm1Event::E11,
];
const STATES: [Farm1State; STATE_COUNT] = [Farm1State::Open, Farm1State::Wait, Farm1State::Lockout];

/// The next state of Table 6-1 of CCSDS 232.1-B-2, by event (row, E1 to E11) and state (column,
/// S1 to S3); 0 is "Not applicable", the others the state number in parentheses in the cell.
const SPEC_NEXT: [[u8; 3]; 11] = [
    [1, 0, 3], // E1  Accept / not applicable / Discard
    [2, 2, 3], // E2  Discard, Retransmit := 1, Wait := 1 / Discard / Discard
    [1, 2, 3], // E3
    [1, 2, 3], // E4
    [3, 3, 3], // E5  Lockout := 1
    [1, 2, 3], // E6  Accept, increment FARM-B Counter
    [1, 1, 1], // E7  Unlock
    [1, 1, 3], // E8  Set V(R); in S3 only the counter
    [1, 2, 3], // E9
    [1, 1, 3], // E10 Wait := 0
    [1, 2, 3], // E11 Report
];

const fn state_number(s: Farm1State) -> u8 {
    match s {
        Farm1State::Open => 1,
        Farm1State::Wait => 2,
        Farm1State::Lockout => 3,
    }
}

/// What Table 6-1 prescribes for a state and an event that applies: the next state and the
/// effects on V(R), the Retransmit Flag, the FARM-B Counter, and the returned actions.
struct Spec {
    next: u8,
    v_r_incremented: bool,
    v_r_set: bool,
    /// Retransmit_Flag afterwards; `None` unchanged.
    retransmit: Option<bool>,
    farm_b_incremented: bool,
    accept: bool,
    discard: bool,
    unlock: bool,
}

/// The cell of state number `s` (1 to 3) and event number `e` (1 to 11); `None` for "Not
/// applicable".
fn spec(s: u8, e: u8) -> Option<Spec> {
    let next = SPEC_NEXT[usize::from(e) - 1][usize::from(s) - 1];
    if next == 0 {
        return None;
    }
    Some(Spec {
        next,
        v_r_incremented: e == 1 && s == 1,
        v_r_set: e == 8 && s != 3,
        retransmit: match (e, s) {
            (2 | 3, 1) => Some(true),
            (1, 1) | (7, _) | (8, 1 | 2) => Some(false),
            _ => None,
        },
        farm_b_incremented: matches!(e, 6..=8),
        accept: (e == 1 && s == 1) || e == 6,
        discard: matches!((e, s), (1, 3) | (2..=5, _) | (9, _)),
        unlock: e == 7,
    })
}

/// The invariant of the inductive step: the Sliding Window is one that Table 7-2 and §6.1.8.3
/// allow (1 ≤ PW, PW + NW ≤ 256, NW ≤ 127). The Lockout and Wait Flags are functions of the State
/// (§6.1.3, §6.1.4; OI-64) and so hold by construction. V(R), the Retransmit Flag, and the
/// FARM-B Counter take every value of their types.
fn invariant(f: &Farm1) -> bool {
    let w = f.config.window;
    w.pw >= 1 && u32::from(w.pw) + u32::from(w.nw) <= 256 && w.nw <= 127
}

impl kani::Arbitrary for Farm1 {
    /// Any state with the window invariant assumed; the only assumption.
    fn any() -> Farm1 {
        let vc_id = TcVcId::new(kani::any());
        let status = ClcwStatus::new(kani::any());
        let counter = FarmBCounter::new(kani::any());
        kani::assume(vc_id.is_some() && status.is_some() && counter.is_some());
        let (Some(vc_id), Some(status), Some(farm_b_counter)) = (vc_id, status, counter) else {
            unreachable!()
        };
        let state = match kani::any::<u8>() % 3 {
            0 => Farm1State::Open,
            1 => Farm1State::Wait,
            _ => Farm1State::Lockout,
        };
        let f = Farm1 {
            config: Farm1Config {
                vc_id,
                status,
                window: Farm1Window {
                    pw: kani::any(),
                    nw: kani::any(),
                },
            },
            state,
            retransmit: kani::any(),
            v_r: FrameSeqNum::new(kani::any()),
            farm_b_counter,
            no_rf_available: kani::any(),
            no_bit_lock: kani::any(),
        };
        kani::assume(invariant(&f));
        f
    }
}

fn fdu_buffer() -> ([u8; 2], usize) {
    let buf: [u8; 2] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 2);
    (buf, len)
}

fn count(actions: &super::Farm1Actions, f: impl Fn(&Farm1Output) -> bool) -> usize {
    actions.iter().filter(|a| f(a)).count()
}

/// Table exhaustiveness: for every state, every event, and every value of the variables, the
/// cell run by the engine has the next state, the Accept and Discard actions, and the effects on
/// V(R), the Retransmit Flag, and the FARM-B Counter that Table 6-1 gives; "Not applicable" is
/// `InvalidState`.
#[kani::proof]
#[kani::unwind(6)]
fn every_cell_matches_table_6_1() {
    // req: COP-002, COP-003, COP-005, COP-034, COP-036
    let f: Farm1 = kani::any();
    let s: usize = kani::any();
    let e: usize = kani::any();
    kani::assume(s < STATE_COUNT && e < EVENT_COUNT);
    let before = Farm1 {
        state: STATES[s],
        ..f
    };
    let (buf, len) = fdu_buffer();
    let ctx = Context {
        kind: if kani::any() {
            FduKind::Ad
        } else {
            FduKind::Bd
        },
        sequence_number: FrameSeqNum::new(kani::any()),
        v_star: FrameSeqNum::new(kani::any()),
        fdu: &buf[..len],
    };
    let mut out = [0u8; 2];
    let result = before.apply(EVENTS[e], &ctx, &mut out);
    let (s_no, e_no) = (s as u8 + 1, e as u8 + 1);
    match (spec(s_no, e_no), result) {
        (None, Err(error)) => assert!(error.kind() == ErrorKind::InvalidState),
        (Some(sp), Ok((after, actions))) => {
            assert!(state_number(after.state) == sp.next);
            let v_r = if sp.v_r_incremented {
                before.v_r.get().wrapping_add(1)
            } else if sp.v_r_set {
                ctx.v_star.get()
            } else {
                before.v_r.get()
            };
            assert!(after.v_r.get() == v_r);
            assert!(after.retransmit == sp.retransmit.unwrap_or(before.retransmit));
            let counter = if sp.farm_b_incremented {
                (before.farm_b_counter.get() + 1) % 4
            } else {
                before.farm_b_counter.get()
            };
            assert!(after.farm_b_counter.get() == counter);
            assert!(after.config == before.config);
            assert!(after.no_rf_available == before.no_rf_available);
            assert!(after.no_bit_lock == before.no_bit_lock);
            let delivered = count(&actions, |a| matches!(a, Farm1Output::Deliver { .. }));
            let discarded = count(&actions, |a| {
                matches!(a, Farm1Output::Notify(Farm1Notify::FrameDiscarded { .. }))
            });
            let unlocked = count(&actions, |a| {
                matches!(a, Farm1Output::Notify(Farm1Notify::Unlocked))
            });
            let changed = count(&actions, |a| {
                matches!(a, Farm1Output::Notify(Farm1Notify::StateChanged { .. }))
            });
            assert!(delivered == usize::from(sp.accept));
            assert!(discarded == usize::from(sp.discard));
            assert!(unlocked == usize::from(sp.unlock));
            assert!(changed == usize::from(sp.next != s_no));
            assert!(actions.len() == delivered + discarded + unlocked + changed);
        }
        _ => panic!("applicable exactly when Table 6-1 gives a next state"),
    }
}

/// Window arithmetic of the Type-AD frame (§6.1.8.3, Table 6-1 E1 to E5): for every V(R), every
/// N(S), every window the invariant admits, and both values of the buffer flag, the event is E1
/// or E2 for N(S) = V(R), E3 for V(R) < N(S) ≤ V(R) + PW − 1, E4 for V(R) − NW ≤ N(S) < V(R),
/// and E5 otherwise, with the comparisons taken modulo 256; the positive and negative parts do
/// not overlap.
#[kani::proof]
fn ad_frame_classification_modulo_256() {
    // req: COP-030, GEN-024
    let f: Farm1 = kani::any();
    let n_s: u8 = kani::any();
    let buffer_available: bool = kani::any();
    let (pw, nw) = (u32::from(f.config.window.pw), u32::from(f.config.window.nw));
    // How far N(S) is ahead of V(R) on the circle of 256, and how far behind.
    let ahead = (u32::from(n_s) + 256 - u32::from(f.v_r.get())) % 256;
    let behind = (256 - ahead) % 256;
    let in_positive = ahead >= 1 && ahead < pw;
    let in_negative = behind >= 1 && behind <= nw;
    assert!(!(in_positive && in_negative));
    let expected = if ahead == 0 {
        if buffer_available {
            Farm1Event::E1
        } else {
            Farm1Event::E2
        }
    } else if in_positive {
        Farm1Event::E3
    } else if in_negative {
        Farm1Event::E4
    } else {
        Farm1Event::E5
    };
    assert!(f.ad_event(FrameSeqNum::new(n_s), buffer_available) == expected);
}

/// The window constructors accept exactly the widths of §6.1.8.3 and Table 7-2, for every `u16`:
/// retransmission allowed, W even in 2 to 254 with PW = NW = W/2; not allowed, 1 ≤ PW ≤ W ≤ 256
/// with NW = W − PW ≤ 127 (Cor. 1 permits PW > NW). Every accepted window has W = PW + NW.
#[kani::proof]
fn window_constructors_accept_exactly_table_7_2() {
    // req: COP-030, API-040, GEN-020
    let w: u16 = kani::any();
    let pw: u16 = kani::any();
    let allowed = Farm1Window::retransmission_allowed(w);
    assert!(allowed.is_ok() == (w % 2 == 0 && (2..=254).contains(&w)));
    if let Ok(win) = allowed {
        assert!(win.positive() == w / 2 && win.negative() == w / 2 && win.width() == w);
    }
    let not_allowed = Farm1Window::retransmission_not_allowed(w, pw);
    let valid = (1..=256).contains(&w) && pw >= 1 && pw <= w && w - pw <= 127;
    assert!(not_allowed.is_ok() == valid);
    if let Ok(win) = not_allowed {
        assert!(win.positive() == pw && win.negative() == w - pw && win.width() == w);
    }
}

/// The base case of the induction: a new FARM-1 built on either constructor's window satisfies
/// the invariant and starts in S1 with the Retransmit Flag and the FARM-B Counter at 0 (OI-58).
#[kani::proof]
fn new_satisfies_invariant() {
    // req: COP-002, COP-034, API-040
    let w: u16 = kani::any();
    let pw: u16 = kani::any();
    let window = if kani::any() {
        Farm1Window::retransmission_allowed(w)
    } else {
        Farm1Window::retransmission_not_allowed(w, pw)
    };
    let vc_id = TcVcId::new(kani::any());
    kani::assume(window.is_ok() && vc_id.is_some());
    let (Ok(window), Some(vc_id)) = (window, vc_id) else {
        unreachable!()
    };
    let config = Farm1Config {
        vc_id,
        status: ClcwStatus::default(),
        window,
    };
    let f = Farm1::new(config, FrameSeqNum::new(kani::any()));
    assert!(invariant(&f));
    assert!(f.state == Farm1State::Open && !f.retransmit);
    assert!(f.farm_b_counter.get() == 0);
}

/// The inductive step: from an arbitrary state satisfying `invariant`, any input either fails
/// (`step` takes `&self`, so nothing changes) or returns a state that satisfies `invariant`,
/// keeps the configuration, and follows Table 6-1 for the event the input is (classified here
/// from §6.1.8.3 and §6.3.2.1, not by the engine): Lockout keeps V(R) (E8 does not execute
/// there); the other states leave it unchanged except at E1 (+1) and E8 (V*(R)); the FARM-B
/// Counter counts the Type-B frames accepted, modulo 4; the CLCW flags follow the State.
#[kani::proof]
#[kani::unwind(6)]
fn step_obeys_table_6_1() {
    // req: COP-002, COP-030, COP-031, COP-033, COP-034, COP-037, API-058
    let before: Farm1 = kani::any();
    let (buf, len) = fdu_buffer();
    let data = &buf[..len];
    let vc_id = TcVcId::new(kani::any());
    kani::assume(vc_id.is_some());
    let Some(vc_id) = vc_id else { unreachable!() };
    let buffer_available: bool = kani::any();
    let n_s: u8 = kani::any();
    let v_star: u8 = kani::any();
    let which: u8 = kani::any::<u8>() % 8;
    let ahead = (u32::from(n_s) + 256 - u32::from(before.v_r.get())) % 256;
    let behind = (256 - ahead) % 256;
    let (pw, nw) = (
        u32::from(before.config.window.pw),
        u32::from(before.config.window.nw),
    );
    let ad_event: u8 = if ahead == 0 {
        if buffer_available { 1 } else { 2 }
    } else if ahead < pw {
        3
    } else if behind >= 1 && behind <= nw {
        4
    } else {
        5
    };
    // The input and its event; `None` for the physical-flag input, which is no event.
    let frame = |frame| Farm1Input::Frame {
        vc_id,
        frame,
        buffer_available,
    };
    let (input, event): (Farm1Input<'_>, Option<u8>) = match which {
        0 => (
            frame(Farm1Frame::Ad {
                sequence_number: FrameSeqNum::new(n_s),
                data,
            }),
            Some(ad_event),
        ),
        1 => (frame(Farm1Frame::Bd { data }), Some(6)),
        2 => (
            frame(Farm1Frame::Bc {
                command: Some(ControlCommand::Unlock),
            }),
            Some(7),
        ),
        3 => (
            frame(Farm1Frame::Bc {
                command: Some(ControlCommand::SetVr(FrameSeqNum::new(v_star))),
            }),
            Some(8),
        ),
        4 => (frame(Farm1Frame::Bc { command: None }), Some(9)),
        5 => (Farm1Input::InvalidFrame, Some(9)),
        6 => (Farm1Input::BufferReleased, Some(10)),
        _ => (
            Farm1Input::SetPhysicalFlags {
                no_rf_available: kani::any(),
                no_bit_lock: kani::any(),
            },
            None,
        ),
    };
    let mut out = [0u8; 2];
    let result = before.step(input, Instant::from_micros(0), &mut out);
    let Ok((after, actions)) = result else {
        // A frame of another VC (COP-037) or E1 in S2 (COP-005): the failures with these buffers.
        let wrong_vc = vc_id != before.config.vc_id && which <= 4;
        let not_applicable = event == Some(1) && before.state == Farm1State::Wait;
        assert!(wrong_vc || not_applicable);
        return;
    };
    assert!(invariant(&after));
    assert!(after.config == before.config);
    let Some(e) = event else {
        assert!(actions.is_empty());
        assert!(after.state == before.state && after.v_r == before.v_r);
        assert!(after.retransmit == before.retransmit);
        assert!(after.farm_b_counter == before.farm_b_counter);
        return;
    };
    let Some(sp) = spec(state_number(before.state), e) else {
        panic!("the engine applied a cell marked not applicable");
    };
    assert!(state_number(after.state) == sp.next);
    assert!(after.lockout_flag() == (sp.next == 3) && after.wait_flag() == (sp.next == 2));
    let v_r = if sp.v_r_incremented {
        before.v_r.get().wrapping_add(1)
    } else if sp.v_r_set {
        v_star
    } else {
        before.v_r.get()
    };
    assert!(after.v_r.get() == v_r);
    if before.state == Farm1State::Lockout {
        assert!(after.v_r == before.v_r);
    }
    let counter = if sp.farm_b_incremented {
        (before.farm_b_counter.get() + 1) % 4
    } else {
        before.farm_b_counter.get()
    };
    assert!(after.farm_b_counter.get() == counter);
    assert!(after.retransmit == sp.retransmit.unwrap_or(before.retransmit));
    assert!(actions.len() <= FARM1_MAX_ACTIONS);
}
