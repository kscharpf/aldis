//! Property tests that drive FOP-1 and FARM-1 with random event sequences (COP-041, VER-023).
//!
//! After every input the engine is in a state that Tables 5-1 and 6-1 reach from the state
//! before it by an event of the class of the input: an `Ok` step goes to the `next_state` of one
//! of the cells that the input's events select in the row of the state before (a CLCW selects
//! E1 to E15 and E101 to E103, a Timeout E16 to E18 and E104, and so on), and no cell the tests
//! reach is "Not applicable", since the step would then be an `Err`. A Timeout before the
//! deadline is no event and leaves the state. An `Err` step returns no engine (`step` takes
//! `&self`), so the caller's engine is untouched by construction (API-058); the test checks that
//! the error is one of the kinds the engines document. The tests also hold the engines to the
//! invariants the tables imply: no Alert other than `Llif` in State S6 (COP-017), the
//! `Sent_Queue` within its capacity (COP-011), the output within its bound (COP-006), and the
//! flags of FARM-1 as functions of its state (OI-64). A step that panics fails the test
//! (GEN-020).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    reason = "tests fail loudly by design; the tables are indexed by the discriminant of the state"
)]

use aldis_cop1::{
    Directive, FARM1_MAX_ACTIONS, FARM1_TABLE, FOP1_TABLE, Farm1, Farm1Config, Farm1Frame,
    Farm1Input, Farm1State, Farm1Window, Fop1, Fop1Config, Fop1Input, Fop1Output, Fop1Target,
    FrameKind, TimeoutType, fop1_max_actions,
};
use aldis_core::ErrorKind;
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId,
};
use aldis_testkit::strategies::common::frame_seq_num;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::tc::control_command;
use aldis_testkit::strategies::{self, octets};
use core::num::NonZeroU8;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/event_sequences.txt"
);

const Q: usize = 4;
const F: usize = 32;
const N: usize = fop1_max_actions(Q);
type Engine = Fop1<Q, F, N>;
const VCID: u8 = 5;

/// One input of the FOP-1, owned, with the microseconds the clock advances before it.
#[derive(Debug, Clone)]
enum FopOp {
    Clcw {
        /// N(R) as an offset from NN(R), or an absolute value.
        n_r: Result<u8, u8>,
        cop: u8,
        other_vc: bool,
        flags: [bool; 3],
    },
    InvalidClcw,
    Timeout,
    RequestAd(Vec<u8>),
    RequestBd(Vec<u8>),
    Directive(DirectiveOp),
    InvalidDirective,
    TransmitDone(u8, bool),
}

#[derive(Debug, Clone, Copy)]
enum DirectiveOp {
    InitiateWithout,
    InitiateWith,
    InitiateUnlock,
    InitiateSetVr(u8),
    Terminate,
    Resume,
    SetVs(u8),
    SetK(u8),
    SetT1(u64),
    SetLimit(u8),
    SetTimeout(bool),
}

fn directive_op() -> impl Strategy<Value = DirectiveOp> {
    prop_oneof![
        3 => Just(DirectiveOp::InitiateWithout),
        2 => Just(DirectiveOp::InitiateWith),
        1 => Just(DirectiveOp::InitiateUnlock),
        1 => any::<u8>().prop_map(DirectiveOp::InitiateSetVr),
        1 => Just(DirectiveOp::Terminate),
        2 => Just(DirectiveOp::Resume),
        1 => any::<u8>().prop_map(DirectiveOp::SetVs),
        1 => (0u8..8).prop_map(DirectiveOp::SetK),
        1 => (0u64..5_000).prop_map(DirectiveOp::SetT1),
        1 => (1u8..6).prop_map(DirectiveOp::SetLimit),
        1 => any::<bool>().prop_map(DirectiveOp::SetTimeout),
    ]
}

fn fop_op() -> impl Strategy<Value = (u64, FopOp)> {
    let op = prop_oneof![
        6 => (
            prop_oneof![3 => (0u8..6).prop_map(Ok), 1 => any::<u8>().prop_map(Err)],
            prop_oneof![9 => Just(1u8), 1 => 0u8..4],
            prop_oneof![19 => Just(false), 1 => Just(true)],
            prop::array::uniform3(any::<bool>()),
        )
            .prop_map(|(n_r, cop, other_vc, flags)| FopOp::Clcw { n_r, cop, other_vc, flags }),
        1 => Just(FopOp::InvalidClcw),
        3 => Just(FopOp::Timeout),
        4 => octets(0..=F).prop_map(FopOp::RequestAd),
        2 => octets(0..=F).prop_map(FopOp::RequestBd),
        5 => directive_op().prop_map(FopOp::Directive),
        1 => Just(FopOp::InvalidDirective),
        5 => (0u8..3, any::<bool>()).prop_map(|(k, a)| FopOp::TransmitDone(k, a)),
    ];
    (prop_oneof![2 => Just(0u64), 1 => 0u64..3_000], op)
}

fn engine(k: u8, limit: u8, tt1: bool) -> Engine {
    let target = Fop1Target::tc(
        TcSpacecraftId::new(7).unwrap(),
        TcVcId::new(VCID).unwrap(),
        F,
        true,
    )
    .unwrap();
    Engine::new(Fop1Config {
        target,
        t1_initial: Duration::from_micros(1_000),
        transmission_limit: NonZeroU8::new(limit).unwrap(),
        timeout_type: if tt1 {
            TimeoutType::Tt1
        } else {
            TimeoutType::Tt0
        },
        sliding_window_k: k,
        ad_repetitions: 0,
        bc_repetitions: 0,
    })
    .unwrap()
}

const fn kind(k: u8) -> FrameKind {
    match k {
        0 => FrameKind::Ad,
        1 => FrameKind::Bc,
        _ => FrameKind::Bd,
    }
}

const fn directive(d: DirectiveOp) -> Directive {
    match d {
        DirectiveOp::InitiateWithout => Directive::InitiateAdWithoutClcwCheck,
        DirectiveOp::InitiateWith => Directive::InitiateAdWithClcwCheck,
        DirectiveOp::InitiateUnlock => Directive::InitiateAdWithUnlock,
        DirectiveOp::InitiateSetVr(v) => Directive::InitiateAdWithSetVr {
            v_r: FrameSeqNum::new(v),
        },
        DirectiveOp::Terminate => Directive::TerminateAd,
        DirectiveOp::Resume => Directive::ResumeAd,
        DirectiveOp::SetVs(v) => Directive::SetVs {
            v_s: FrameSeqNum::new(v),
        },
        DirectiveOp::SetK(width) => Directive::SetSlidingWindowWidth { width },
        DirectiveOp::SetT1(us) => Directive::SetT1Initial {
            duration: Duration::from_micros(us),
        },
        DirectiveOp::SetLimit(limit) => Directive::SetTransmissionLimit {
            limit: NonZeroU8::new(limit).unwrap(),
        },
        DirectiveOp::SetTimeout(tt1) => Directive::SetTimeoutType {
            timeout_type: if tt1 {
                TimeoutType::Tt1
            } else {
                TimeoutType::Tt0
            },
        },
    }
}

/// The indices in the event order of Table 5-1 (E1 to E46 are 0 to 45, E101 to E104 are 46 to 49)
/// of the events that the input of `op` can select.
fn fop_events(op: &FopOp) -> Vec<usize> {
    match op {
        FopOp::Clcw { .. } => (0..=14).chain(46..=48).collect(),
        FopOp::InvalidClcw => vec![14],
        FopOp::Timeout => (15..=17).chain([49]).collect(),
        FopOp::RequestAd(_) => vec![18, 19],
        FopOp::RequestBd(_) => vec![20, 21],
        FopOp::Directive(_) => (22..=39).collect(),
        FopOp::InvalidDirective => vec![39],
        FopOp::TransmitDone(..) => (40..=45).collect(),
    }
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn fop1_stays_within_table_5_1_under_any_event_sequence(
        k in 1u8..=4,
        limit in 1u8..=4,
        tt1 in any::<bool>(),
        ops in prop::collection::vec(fop_op(), 0..60),
    ) {
        // req: COP-041, COP-001, COP-005, COP-006, COP-011, COP-017, API-058, GEN-020, VER-023
        let mut f = engine(k, limit, tt1);
        let mut now = 0u64;
        for (advance, op) in &ops {
            now += advance;
            let at = Instant::from_micros(now);
            let mut area = [0u8; F];
            let n_r = |off: &Result<u8, u8>| match off {
                Ok(o) => f.nn_r().get().wrapping_add(*o),
                Err(abs) => *abs,
            };
            let input = match op {
                FopOp::Clcw { n_r: off, cop, other_vc, flags: [lockout, wait, retransmit] } => {
                    Fop1Input::Clcw(Clcw {
                        status: ClcwStatus::new(0).unwrap(),
                        cop_in_effect: CopInEffect::new(*cop).unwrap(),
                        vc_id: TcVcId::new(if *other_vc { VCID + 1 } else { VCID }).unwrap(),
                        no_rf_available: false,
                        no_bit_lock: false,
                        lockout: *lockout,
                        wait: *wait,
                        retransmit: *retransmit,
                        farm_b_counter: FarmBCounter::default(),
                        report_value: FrameSeqNum::new(n_r(off)),
                    })
                }
                FopOp::InvalidClcw => Fop1Input::InvalidClcw,
                FopOp::Timeout => Fop1Input::Timeout,
                FopOp::RequestAd(fdu) => Fop1Input::RequestAd { handle: 1, fdu },
                FopOp::RequestBd(fdu) => Fop1Input::RequestBd { handle: 2, fdu },
                FopOp::Directive(d) => Fop1Input::Directive { handle: 3, directive: directive(*d) },
                FopOp::InvalidDirective => Fop1Input::InvalidDirective { handle: 4 },
                FopOp::TransmitDone(k, accepted) => Fop1Input::TransmitDone { kind: kind(*k), accepted: *accepted },
            };
            let before = f.state();
            let (after, actions) = match f.step(input, at, &mut area) {
                Ok(ok) => ok,
                Err(e) => {
                    // No engine is returned: the caller keeps `f` (API-058).
                    prop_assert!(matches!(
                        e.kind(),
                        ErrorKind::UnknownChannel
                            | ErrorKind::InvalidField
                            | ErrorKind::OutOfRange
                            | ErrorKind::InvalidState
                    ), "{e:?}");
                    continue;
                }
            };
            prop_assert!(actions.len() <= N);
            // The new state is the `next_state` of a cell that the input's events select in
            // the row of the old state; a Timeout before the deadline selects none.
            let row = &FOP1_TABLE[before as usize];
            let idle = matches!(op, FopOp::Timeout) && after.state() == before;
            prop_assert!(
                idle || fop_events(op).iter().any(|e| row[*e].next_state == Some(after.state())),
                "{before:?} -> {:?} by {op:?} is in no selected cell of Table 5-1", after.state()
            );
            prop_assert!(after.sent_queue_len() <= Q);
            prop_assert!(after.sliding_window_k() >= 1 && usize::from(after.sliding_window_k()) <= Q);
            // COP-017: in S6, an Alert is the Alert[LLIF] of a lower-layer failure only.
            if before == aldis_cop1::Fop1State::Initial {
                for a in actions.iter() {
                    if let Fop1Output::Alert(alert) = a {
                        prop_assert_eq!(alert.reason, aldis_cop1::AlertReason::Llif);
                    }
                }
            }
            // A frame to transmit is in the output area, within the Maximum Transfer Frame Length.
            for a in actions.iter() {
                if let Fop1Output::Transmit { frame, .. } = a {
                    prop_assert!(frame.slice(&area).is_some_and(|b| !b.is_empty() && b.len() <= F));
                }
            }
            f = after;
        }
    }
}

/// One input of the FARM-1, owned.
#[derive(Debug, Clone)]
enum FarmOp {
    Ad(Result<u8, FrameSeqNum>, bool, Vec<u8>),
    Bd(Vec<u8>),
    Bc(Option<ControlCommand>),
    Invalid,
    Release,
    Physical(bool, bool),
}

fn farm_op() -> impl Strategy<Value = FarmOp> {
    prop_oneof![
        6 => (
            prop_oneof![3 => (0u8..5).prop_map(Ok), 1 => frame_seq_num().prop_map(Err)],
            any::<bool>(),
            octets(0..=8),
        ).prop_map(|(n, b, d)| FarmOp::Ad(n, b, d)),
        2 => octets(0..=8).prop_map(FarmOp::Bd),
        1 => control_command().prop_map(|c| FarmOp::Bc(Some(c))),
        1 => Just(FarmOp::Bc(None)),
        1 => Just(FarmOp::Invalid),
        3 => Just(FarmOp::Release),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(a, b)| FarmOp::Physical(a, b)),
    ]
}

/// The indices in the event order of Table 6-1 (E1 to E11 are 0 to 10) of the events the input of
/// `op` can select; a physical-flag input selects none.
fn farm_events(op: &FarmOp) -> Vec<usize> {
    match op {
        FarmOp::Ad(..) => (0..=4).collect(),
        FarmOp::Bd(_) => vec![5],
        FarmOp::Bc(Some(_)) => vec![6, 7],
        FarmOp::Bc(None) | FarmOp::Invalid => vec![8],
        FarmOp::Release => vec![9],
        FarmOp::Physical(..) => vec![],
    }
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn farm1_stays_within_table_6_1_under_any_event_sequence(
        half in 1u16..=127,
        v_r in frame_seq_num(),
        ops in prop::collection::vec(farm_op(), 0..60),
    ) {
        // req: COP-041, COP-002, COP-005, COP-006, API-058, GEN-020, VER-023
        let mut f = Farm1::new(
            Farm1Config {
                vc_id: TcVcId::new(VCID).unwrap(),
                status: ClcwStatus::new(0).unwrap(),
                window: Farm1Window::retransmission_allowed(2 * half).unwrap(),
            },
            v_r,
        );
        for op in &ops {
            let mut area = [0u8; 8];
            let input = match op {
                FarmOp::Ad(n, buffer_available, data) => Farm1Input::Frame {
                    frame: Farm1Frame::Ad {
                        sequence_number: match n {
                            Ok(o) => FrameSeqNum::new(f.v_r().get().wrapping_add(*o)),
                            Err(abs) => *abs,
                        },
                        data,
                    },
                    buffer_available: *buffer_available,
                },
                FarmOp::Bd(data) => Farm1Input::Frame { frame: Farm1Frame::Bd { data }, buffer_available: true },
                FarmOp::Bc(command) => Farm1Input::Frame { frame: Farm1Frame::Bc { command: *command }, buffer_available: true },
                FarmOp::Invalid => Farm1Input::InvalidFrame,
                FarmOp::Release => Farm1Input::BufferReleased,
                FarmOp::Physical(a, b) => Farm1Input::SetPhysicalFlags { no_rf_available: *a, no_bit_lock: *b },
            };
            let before = f.state();
            let (after, actions) = match f.step(input, Instant::from_micros(0), &mut area) {
                Ok(ok) => ok,
                Err(e) => {
                    // The cell "Not applicable" (S2 with E1), or an FDU longer than the area.
                    prop_assert!(matches!(e.kind(), ErrorKind::InvalidState | ErrorKind::BufferTooSmall), "{e:?}");
                    continue;
                }
            };
            prop_assert!(actions.len() <= FARM1_MAX_ACTIONS);
            // A physical-flag input takes no cell and leaves the state; every other input goes
            // to the `next_state` of a cell that its events select in the row of the old state.
            let row = &FARM1_TABLE[before as usize];
            let physical = matches!(op, FarmOp::Physical(..));
            prop_assert!(
                (physical && after.state() == before)
                    || farm_events(op).iter().any(|e| row[*e].next_state == Some(after.state())),
                "{before:?} -> {:?} by {op:?} is in no selected cell of Table 6-1", after.state()
            );
            prop_assert_eq!(after.lockout_flag(), after.state() == Farm1State::Lockout);
            prop_assert_eq!(after.wait_flag(), after.state() == Farm1State::Wait);
            f = after;
        }
    }
}
