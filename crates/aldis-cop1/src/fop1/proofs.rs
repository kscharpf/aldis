//! Kani proof harnesses for FOP-1 (docs/verification/formal-verification.md, OI-42).
//!
//! Compiled only by `cargo kani` (`cfg(kani)`); not part of the library, the tests, or the
//! coverage run. The expected results are written from Table 5-1 and §5.1 of CCSDS 232.1-B-2 and
//! from docs/standards/cop1.md (`SPEC_NEXT`, `spec_alert`, the circular distances), not from
//! `FOP1_TABLE`, `Fop1::clcw_event`, or the code of the actions.
//!
//! - **Table exhaustiveness** (`table_5_1_next_states`, `table_5_1_alerts`): every state and
//!   every event, no bound.
//! - **Modulo-256 arithmetic** (`clcw_event_modulo_256`): every V(S), NN(R), N(R), flag
//!   combination, and COP pattern, on a `Core` without `invariant`, no bound.
//!   The base case of the invariant is `new_satisfies_invariant`.
//!
//! The inductive step of the invariant over the engine's inputs and the engine's cell check
//! (`decide_event` for every state and event) were written and did not finish within 5 minutes
//! of CBMC time each (formal-verification.md §9), so they are not harnesses.

use core::num::NonZeroU8;

use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{Clcw, ClcwStatus, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId};

use super::{
    Core, Fop1, Fop1Config, Fop1Target, Origin, Sent, SentKind, Waiting, fop1_max_actions,
};
use crate::fop1_table::{EVENT_COUNT, Fop1Event, Fop1State, STATE_COUNT, SuspendState};
use crate::notify::{AlertReason, TimeoutType};

const Q: usize = 2;
const F: usize = 12;
const N: usize = fop1_max_actions(Q);
type Engine = Fop1<Q, F, N>;

const STATES: [Fop1State; STATE_COUNT] = [
    Fop1State::Active,
    Fop1State::RetransmitWithoutWait,
    Fop1State::RetransmitWithWait,
    Fop1State::InitializingWithoutBcFrame,
    Fop1State::InitializingWithBcFrame,
    Fop1State::Initial,
];

/// The events of Table 5-1 in the order of `Fop1Event`: E1 to E46, then E101 to E104.
const EVENTS: [Fop1Event; EVENT_COUNT] = {
    use Fop1Event as E;
    [
        E::E1,
        E::E2,
        E::E3,
        E::E4,
        E::E5,
        E::E6,
        E::E7,
        E::E8,
        E::E9,
        E::E10,
        E::E11,
        E::E12,
        E::E13,
        E::E14,
        E::E15,
        E::E16,
        E::E17,
        E::E18,
        E::E19,
        E::E20,
        E::E21,
        E::E22,
        E::E23,
        E::E24,
        E::E25,
        E::E26,
        E::E27,
        E::E28,
        E::E29,
        E::E30,
        E::E31,
        E::E32,
        E::E33,
        E::E34,
        E::E35,
        E::E36,
        E::E37,
        E::E38,
        E::E39,
        E::E40,
        E::E41,
        E::E42,
        E::E43,
        E::E44,
        E::E45,
        E::E46,
        E::E101,
        E::E102,
        E::E103,
        E::E104,
    ]
};

/// The event number of `EVENTS[i]`.
const fn number(i: usize) -> u16 {
    if i < 46 {
        i as u16 + 1
    } else {
        i as u16 + 101 - 46
    }
}

/// The next state of Table 5-1 of CCSDS 232.1-B-2 by event (row, in the order of `EVENTS`) and
/// state (column, S1 to S6); 0 is "Not applicable", the others the state number in parentheses.
const STAY: [u8; 6] = [1, 2, 3, 4, 5, 6];
const ALERT: [u8; 6] = [6; 6];
const SPEC_NEXT: [[u8; 6]; EVENT_COUNT] = [
    [1, 6, 6, 1, 1, 6], // E1
    [1, 1, 1, 0, 0, 6], // E2
    ALERT,              // E3
    [6, 6, 6, 6, 5, 6], // E4
    [1, 6, 6, 0, 0, 6], // E5
    [1, 1, 1, 0, 0, 6], // E6
    [6, 6, 6, 0, 0, 6], // E7
    [2, 2, 2, 0, 0, 6], // E8
    [3, 3, 3, 0, 0, 6], // E9
    [2, 2, 2, 0, 0, 6], // E10
    [3, 3, 3, 0, 0, 6], // E11
    [2, 2, 2, 0, 0, 6], // E12
    [6, 6, 6, 6, 5, 6], // E13
    [6, 6, 6, 6, 5, 6], // E14
    ALERT,              // E15
    [1, 2, 3, 6, 5, 0], // E16
    [6, 6, 6, 6, 6, 0], // E17
    [6, 6, 6, 6, 6, 0], // E18
    STAY,               // E19
    STAY,               // E20
    STAY,               // E21
    STAY,               // E22
    [1, 2, 3, 4, 5, 1], // E23
    [1, 2, 3, 4, 5, 4], // E24
    [1, 2, 3, 4, 5, 5], // E25
    STAY,               // E26
    [1, 2, 3, 4, 5, 5], // E27
    STAY,               // E28
    ALERT,              // E29
    STAY,               // E30
    [0, 0, 0, 0, 0, 1], // E31
    [0, 0, 0, 0, 0, 2], // E32
    [0, 0, 0, 0, 0, 3], // E33
    [0, 0, 0, 0, 0, 4], // E34
    STAY,               // E35
    STAY,               // E36
    STAY,               // E37
    STAY,               // E38
    STAY,               // E39
    STAY,               // E40
    STAY,               // E41
    ALERT,              // E42
    STAY,               // E43
    ALERT,              // E44
    STAY,               // E45
    ALERT,              // E46
    [6, 6, 6, 0, 0, 6], // E101
    [6, 6, 6, 0, 0, 6], // E102
    [3, 3, 3, 0, 0, 6], // E103
    [1, 2, 3, 6, 5, 0], // E104
];

/// The Alert a cell lists in Table 5-1, by event number and state number (1 to 6); `None` when
/// the cell lists no Alert.
fn spec_alert(event: u16, s: u8) -> Option<AlertReason> {
    match (event, s) {
        (1 | 5, 2 | 3) | (4, 1..=4) => Some(AlertReason::Synch),
        (3, 1..=5) | (7, 1..=3) | (15, 1..=5) => Some(AlertReason::Clcw),
        (101 | 102, 1..=3) => Some(AlertReason::Limit),
        (13, 1..=4) => Some(AlertReason::NnR),
        (14, 1..=4) => Some(AlertReason::Lockout),
        (16, 4) | (17, 1..=5) | (18, 5) => Some(AlertReason::T1),
        (29, 1..=5) => Some(AlertReason::Term),
        (42 | 44 | 46, _) => Some(AlertReason::Llif),
        _ => None,
    }
}

const fn state_number(s: Fop1State) -> u8 {
    match s {
        Fop1State::Active => 1,
        Fop1State::RetransmitWithoutWait => 2,
        Fop1State::RetransmitWithWait => 3,
        Fop1State::InitializingWithoutBcFrame => 4,
        Fop1State::InitializingWithBcFrame => 5,
        Fop1State::Initial => 6,
    }
}

/// Every cell of the table has the next state of Table 5-1 and is "Not applicable" exactly where
/// the Blue Book says so (COP-001, COP-005).
#[kani::proof]
#[kani::unwind(51)]
fn table_5_1_next_states() {
    // req: COP-001, COP-003, COP-005
    for (s, state) in STATES.into_iter().enumerate() {
        for (e, event) in EVENTS.into_iter().enumerate() {
            let next = super::cell(state, event).next_state.map_or(0, state_number);
            assert!(next == SPEC_NEXT[e][s]);
        }
    }
}

/// Every cell lists the Alert of Table 5-1 (at most one), and every cell with an Alert goes to S6
/// (COP-017).
#[kani::proof]
#[kani::unwind(51)]
fn table_5_1_alerts() {
    // req: COP-001, COP-017
    for (s, state) in STATES.into_iter().enumerate() {
        for (e, event) in EVENTS.into_iter().enumerate() {
            let cell = super::cell(state, event);
            let mut found: Option<AlertReason> = None;
            let mut many = false;
            for a in cell.actions {
                if let crate::fop1_table::Fop1Action::Alert(r) = a {
                    many |= found.is_some();
                    found = Some(*r);
                }
            }
            assert!(!many);
            assert!(found == spec_alert(number(e), s as u8 + 1));
            assert!(found.is_none() || cell.next_state == Some(Fop1State::Initial));
        }
    }
}

fn target() -> Fop1Target {
    let (Some(sc), Some(vc)) = (TcSpacecraftId::new(42), TcVcId::new(1)) else {
        unreachable!()
    };
    match Fop1Target::tc(sc, vc, F, false) {
        Ok(t) => t,
        Err(_) => unreachable!(),
    }
}

fn config() -> Fop1Config {
    Fop1Config {
        target: target(),
        t1_initial: Duration::from_micros(100),
        transmission_limit: NonZeroU8::MIN,
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: 1,
        ad_repetitions: 0,
        bc_repetitions: 0,
    }
}

fn engine_with(core: Core<Q>) -> Engine {
    Fop1 {
        target: target(),
        core,
    }
}

/// The invariant of the inductive step, from 232.1 §5.1 (the Sent_Queue holds the frames from
/// NN(R) up to V(S) − 1, §5.1.7; V(S) − NN(R) counts them), the Type-BC frame of §5.2.5, the
/// Suspend_State of §5.1.11, and the Initial and Initializing states (§5.2.14: Initialize purges,
/// so nothing is outstanding):
/// 1. the entries are packed from the front of the queue, each with its octets at its own
///    position;
/// 2. the Type-AD entries are N(S) = NN(R), NN(R) + 1, … in order (modulo 256), and their number is
///    (V(S) − NN(R)) mod 256;
/// 3. a Type-BC entry exists only in S5, at most one;
/// 4. in S4 and S5, V(S) = NN(R) (no Type-AD frame is outstanding);
/// 5. in S6 with Suspend_State 0 the queues are empty;
/// 6. Suspend_State ≠ 0 only in S6;
/// 7. 1 ≤ K ≤ Q (COP-015) and `Transmission_Count` ≥ 1.
fn invariant(c: &Core<Q>) -> bool {
    let mut ad = 0u8;
    let mut bc = 0usize;
    let mut packed = true;
    let mut seen_gap = false;
    let mut in_order = true;
    for (p, slot) in c.sent.iter().enumerate() {
        match slot {
            None => seen_gap = true,
            Some(s) => {
                packed &= !seen_gap;
                packed &= s.origin == Origin::Base(p) && s.len <= F;
                match s.kind {
                    SentKind::Ad(_) => {
                        in_order &= s.sequence_number.get() == c.nn_r.get().wrapping_add(ad);
                        ad += 1;
                    }
                    SentKind::Bc => bc += 1,
                }
            }
        }
    }
    let initial = c.state == Fop1State::Initial;
    let entries = c.sent.iter().flatten().count();
    packed
        && in_order
        && ad == c.v_s.get().wrapping_sub(c.nn_r.get())
        && bc <= 1
        && (bc == 0 || c.state == Fop1State::InitializingWithBcFrame)
        && (!matches!(
            c.state,
            Fop1State::InitializingWithoutBcFrame | Fop1State::InitializingWithBcFrame
        ) || c.v_s == c.nn_r)
        && (!(initial && c.suspend_state == SuspendState::NotSuspended)
            || (entries == 0 && c.wait.is_none()))
        && (c.suspend_state == SuspendState::NotSuspended || initial)
        && c.k >= 1
        && usize::from(c.k) <= Q
        && c.transmission_count >= 1
        && c.wait.is_none_or(|w| w.len <= F && !w.fresh)
}

fn any_state() -> Fop1State {
    let i: usize = kani::any();
    kani::assume(i < STATE_COUNT);
    STATES[i]
}

fn any_suspend() -> SuspendState {
    match kani::any::<u8>() % 5 {
        0 => SuspendState::NotSuspended,
        1 => SuspendState::Active,
        2 => SuspendState::RetransmitWithoutWait,
        3 => SuspendState::RetransmitWithWait,
        _ => SuspendState::InitializingWithoutBcFrame,
    }
}

fn any_sent(position: usize) -> Sent {
    Sent {
        origin: Origin::Base(position),
        len: kani::any(),
        sequence_number: FrameSeqNum::new(kani::any()),
        kind: if kani::any() {
            SentKind::Ad(kani::any())
        } else {
            SentKind::Bc
        },
        to_be_retransmitted: kani::any(),
    }
}

impl kani::Arbitrary for Core<Q> {
    /// Any `Core` with `invariant` assumed; the only assumption.
    fn any() -> Core<Q> {
        let core = any_core();
        kani::assume(invariant(&core));
        core
    }
}

/// Any `Core` whatever, without `invariant`: V(S), NN(R), and every other field unrestricted.
fn any_core() -> Core<Q> {
    let t1 = kani::any::<u64>();
    kani::assume(t1 > 0);
    let limit = NonZeroU8::new(kani::any());
    kani::assume(limit.is_some());
    let Some(transmission_limit) = limit else {
        unreachable!()
    };
    let sent = [
        if kani::any() { Some(any_sent(0)) } else { None },
        if kani::any() { Some(any_sent(1)) } else { None },
    ];
    let wait = if kani::any() {
        Some(Waiting {
            handle: kani::any(),
            len: kani::any(),
            fresh: false,
        })
    } else {
        None
    };
    Core {
        t1_initial: Duration::from_micros(t1),
        transmission_limit,
        timeout_type: if kani::any() {
            TimeoutType::Tt0
        } else {
            TimeoutType::Tt1
        },
        k: kani::any(),
        ad_repetitions: kani::any(),
        bc_repetitions: kani::any(),
        state: any_state(),
        v_s: FrameSeqNum::new(kani::any()),
        nn_r: FrameSeqNum::new(kani::any()),
        sent,
        wait,
        ad_out_ready: kani::any(),
        bc_out_ready: kani::any(),
        bd_out: if kani::any() { Some(kani::any()) } else { None },
        pending_initiate: if kani::any() { Some(kani::any()) } else { None },
        deadline: if kani::any() {
            Some(Instant::from_micros(kani::any()))
        } else {
            None
        },
        transmission_count: kani::any(),
        suspend_state: any_suspend(),
    }
}

/// The base case of the induction: a new engine satisfies `invariant` for every K that
/// `Fop1::new` accepts, and `k_fits` accepts exactly K in 1 to min(255, Q) for every `u8`, with
/// Q = 1, 2, 255 (COP-015).
#[kani::proof]
fn new_satisfies_invariant() {
    // req: COP-001, COP-015, API-040
    let k: u8 = kani::any();
    let mut cfg = config();
    cfg.sliding_window_k = k;
    cfg.transmission_limit = NonZeroU8::new(kani::any()).unwrap_or(NonZeroU8::MIN);
    let micros: u64 = kani::any();
    cfg.t1_initial = Duration::from_micros(micros);
    let made = Engine::new(cfg);
    let micros_ok = micros != 0;
    assert!(made.is_ok() == ((1..=Q as u8).contains(&k) && micros_ok));
    if let Ok(fop) = made {
        assert!(invariant(&fop.core));
        assert!(fop.core.state == Fop1State::Initial);
    }
    assert!(Fop1::<1, 1, { fop1_max_actions(1) }>::k_fits(k) == (k == 1));
    assert!(Fop1::<255, 1, { fop1_max_actions(255) }>::k_fits(k) == (k >= 1));
}

fn clcw(report: u8, lockout: bool, wait: bool, retransmit: bool, cop: CopInEffect) -> Clcw {
    Clcw {
        status: ClcwStatus::default(),
        cop_in_effect: cop,
        vc_id: match TcVcId::new(1) {
            Some(v) => v,
            None => unreachable!(),
        },
        no_rf_available: false,
        no_bit_lock: false,
        lockout,
        wait,
        retransmit,
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(report),
    }
}

/// Modulo-256 window arithmetic of the CLCW (§5.3.2.1, Table 5-1, E1 to E15 and E101 to E103):
/// for every V(S), NN(R), N(R), flag, `Transmission_Limit`, and `Transmission_Count`, the event
/// is the one the conditions of the table give. N(R) is valid exactly when it is in
/// NN(R) ≤ N(R) ≤ V(S) on the circle of 256, measured from NN(R); the checks rank as OI-62 says
/// (invalid pattern, Lockout, invalid N(R), then the flags).
#[kani::proof]
fn clcw_event_modulo_256() {
    // req: COP-012, GEN-024
    let fop = engine_with(any_core());
    let (n_r, lockout, wait, retransmit): (u8, bool, bool, bool) =
        (kani::any(), kani::any(), kani::any(), kani::any());
    let cop_ok: bool = kani::any();
    let cop = if cop_ok {
        CopInEffect::COP_1
    } else {
        // The three invalid patterns (0, 2, 3); 1 is COP-1 in effect.
        let pattern = kani::any::<u8>() & 0b11;
        kani::assume(pattern != 1);
        CopInEffect::new(pattern).unwrap_or(CopInEffect::COP_1)
    };
    let c = clcw(n_r, lockout, wait, retransmit, cop);
    let got = fop.clcw_event(&c);
    let (v_s, nn_r) = (
        u32::from(fop.core.v_s.get()),
        u32::from(fop.core.nn_r.get()),
    );
    let outstanding = (v_s + 256 - nn_r) % 256;
    let acked = (u32::from(n_r) + 256 - nn_r) % 256;
    let limit = fop.core.transmission_limit.get();
    let count = fop.core.transmission_count;
    use Fop1Event as E;
    let expected = if cop != CopInEffect::COP_1 {
        E::E15
    } else if lockout {
        E::E14
    } else if acked > outstanding {
        E::E13
    } else if acked == outstanding {
        if retransmit {
            E::E4
        } else if wait {
            E::E3
        } else if acked == 0 {
            E::E1
        } else {
            E::E2
        }
    } else if !retransmit {
        if wait {
            E::E7
        } else if acked == 0 {
            E::E5
        } else {
            E::E6
        }
    } else if limit == 1 {
        if acked == 0 { E::E102 } else { E::E101 }
    } else if acked != 0 {
        if wait { E::E9 } else { E::E8 }
    } else if count < limit {
        if wait { E::E11 } else { E::E10 }
    } else if wait {
        E::E103
    } else {
        E::E12
    };
    assert!(got == Ok(expected));
}
