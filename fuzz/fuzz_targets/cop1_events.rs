//! Arbitrary event sequences for FOP-1 and FARM-1 (COP-041): after every accepted input the
//! engine is in a state that Table 5-1 or Table 6-1 reaches from the state before it by an event
//! of the class of the input (the cells that event class selects in the row of the old state),
//! the step is a pure function (the same state and input give the same result, also for an
//! error), and the engines hold their own bounds (the `Sent_Queue` capacity, the action bound,
//! the flags of FARM-1 as functions of its state; COP-006, COP-017, OI-64). A refused step
//! returns no engine, so the caller's engine is untouched by construction (API-058). No input
//! panics (GEN-020).
#![no_main]

#[path = "../../crates/aldis-cop1/tests/common/mod.rs"]
mod common;

use common::Rig;

// req: COP-041, COP-001, COP-002, COP-005, COP-006, COP-017, API-058, GEN-020, VER-022

use aldis_cop1::{
    AlertReason, Directive, FARM1_MAX_ACTIONS, FARM1_TABLE, FOP1_TABLE, Farm1, Farm1Config,
    Farm1Frame, Farm1Input, Farm1State, Farm1Window, Fop1Config, Fop1Decision, Fop1Input,
    Fop1Output, Fop1State, Fop1Target, FrameKind, TimeoutType, fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId,
};
use core::num::NonZeroU8;
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const Q: usize = 4;
const F: usize = 32;
const N: usize = fop1_max_actions(Q);
const VCID: u8 = 5;

fn directive(u: &mut Unstructured<'_>) -> Result<Directive> {
    Ok(match u.int_in_range(0..=10)? {
        0 => Directive::InitiateAdWithoutClcwCheck,
        1 => Directive::InitiateAdWithClcwCheck,
        2 => Directive::InitiateAdWithUnlock,
        3 => Directive::InitiateAdWithSetVr {
            v_r: FrameSeqNum::new(u.arbitrary()?),
        },
        4 => Directive::TerminateAd,
        5 => Directive::ResumeAd,
        6 => Directive::SetVs {
            v_s: FrameSeqNum::new(u.arbitrary()?),
        },
        7 => Directive::SetSlidingWindowWidth {
            width: u.arbitrary()?,
        },
        8 => Directive::SetT1Initial {
            duration: Duration::from_micros(u.int_in_range(0..=5_000)?),
        },
        9 => Directive::SetTransmissionLimit {
            limit: NonZeroU8::new(u.int_in_range(1..=255)?).unwrap(),
        },
        _ => Directive::SetTimeoutType {
            timeout_type: if u.arbitrary()? {
                TimeoutType::Tt1
            } else {
                TimeoutType::Tt0
            },
        },
    })
}

/// The event indices of Table 5-1 (E1 to E46 are 0 to 45, E101 to E104 are 46 to 49) that the
/// class of the input can select; a Timeout before the deadline selects none.
fn fop1_events(class: u8) -> Vec<usize> {
    match class {
        0 => (0..=14).chain(46..=48).collect(),
        1 => vec![14],
        2 => (15..=17).chain([49]).collect(),
        3 => vec![18, 19],
        4 => vec![20, 21],
        5 => (22..=39).collect(),
        6 => vec![39],
        _ => (40..=45).collect(),
    }
}

/// A CLCW with N(R) `n_r`, perhaps of another Virtual Channel or COP in Effect.
fn clcw(u: &mut Unstructured<'_>, n_r: u8) -> Result<Fop1Input<'static>> {
    Ok(Fop1Input::Clcw(Clcw {
        status: ClcwStatus::new(0).unwrap(),
        cop_in_effect: CopInEffect::new(u.int_in_range(0..=3)?).unwrap(),
        vc_id: TcVcId::new(if u.ratio(1, 16)? { VCID + 1 } else { VCID }).unwrap(),
        no_rf_available: u.arbitrary()?,
        no_bit_lock: u.arbitrary()?,
        lockout: u.arbitrary()?,
        wait: u.arbitrary()?,
        retransmit: u.arbitrary()?,
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(n_r),
    }))
}

#[expect(
    clippy::too_many_lines,
    reason = "one pass of the loop draws the configuration, the input, and its checks"
)]
fn fop1(u: &mut Unstructured<'_>) -> Result<()> {
    let target = Fop1Target::tc(
        TcSpacecraftId::new(7).unwrap(),
        TcVcId::new(VCID).unwrap(),
        F,
        u.arbitrary()?,
    )
    .unwrap();
    let mut f = Rig::<Q, F, N>::new(Fop1Config {
        target,
        t1_initial: Duration::from_micros(u.int_in_range(1..=2_000)?),
        transmission_limit: NonZeroU8::new(u.int_in_range(1..=5)?).unwrap(),
        timeout_type: if u.arbitrary()? {
            TimeoutType::Tt1
        } else {
            TimeoutType::Tt0
        },
        sliding_window_k: u.int_in_range(1..=4)?,
        ad_repetitions: u.arbitrary()?,
        bc_repetitions: u.arbitrary()?,
    })
    .unwrap();
    let mut now = 0_u64;
    // bound: each pass consumes at least one octet of the input, and `u` is finite
    while !u.is_empty() {
        now += u.int_in_range(0..=3_000)?;
        let at = Instant::from_micros(now);
        let fdu_len = u.int_in_range(0..=F)?;
        let fdu = u.bytes(fdu_len.min(u.len()))?.to_vec();
        let n_r = if u.arbitrary()? {
            f.nn_r().get().wrapping_add(u.int_in_range(0..=5)?)
        } else {
            u.arbitrary()?
        };
        let class: u8 = u.int_in_range(0..=7)?;
        let input = match class {
            0 => clcw(u, n_r)?,
            1 => Fop1Input::InvalidClcw,
            2 => Fop1Input::Timeout,
            3 => Fop1Input::RequestAd {
                handle: 1,
                fdu: &fdu,
            },
            4 => Fop1Input::RequestBd {
                handle: 2,
                fdu: &fdu,
            },
            5 => Fop1Input::Directive {
                handle: 3,
                directive: directive(u)?,
            },
            6 => Fop1Input::InvalidDirective { handle: 4 },
            _ => Fop1Input::TransmitDone {
                kind: match u.int_in_range(0..=2)? {
                    0 => FrameKind::Ad,
                    1 => FrameKind::Bc,
                    _ => FrameKind::Bd,
                },
                accepted: u.arbitrary()?,
            },
        };
        let before = f.state();
        let mut area = [0_u8; F];
        let mut again = [0_u8; F];
        let first = f.decide(input, at, &mut area);
        // The decision is a pure function of the state, the input, and the time.
        assert_eq!(
            first.as_ref().map(Fop1Decision::actions),
            f.decide(input, at, &mut again)
                .as_ref()
                .map(Fop1Decision::actions)
        );
        let Ok(decision) = first else {
            continue;
        };
        let mut after = f.clone();
        let actions = after.commit(decision);
        assert_eq!(area, again);
        assert!(actions.len() <= N);
        let events = fop1_events(class);
        let row = &FOP1_TABLE[before as usize];
        assert!(
            (class == 2 && after.state() == before)
                || events
                    .iter()
                    .any(|e| row[*e].next_state == Some(after.state()))
        );
        assert!(after.sent_queue_len() <= Q);
        for a in actions.iter() {
            match a {
                Fop1Output::Alert(alert) if before == Fop1State::Initial => {
                    assert_eq!(alert.reason, AlertReason::Llif);
                }
                Fop1Output::Transmit { frame, .. } => {
                    assert!(
                        frame
                            .slice(&area)
                            .is_some_and(|b| !b.is_empty() && b.len() <= F)
                    );
                }
                _ => {}
            }
        }
        f = after;
    }
    Ok(())
}

fn farm1(u: &mut Unstructured<'_>) -> Result<()> {
    let window = Farm1Window::retransmission_allowed(2 * u.int_in_range(1..=127)?).unwrap();
    let mut f = Farm1::new(
        Farm1Config {
            vc_id: TcVcId::new(VCID).unwrap(),
            status: ClcwStatus::new(0).unwrap(),
            window,
        },
        FrameSeqNum::new(u.arbitrary()?),
    );
    // bound: each pass consumes at least one octet of the input, and `u` is finite
    while !u.is_empty() {
        let data_len = u.int_in_range(0..=8)?;
        let data = u.bytes(data_len.min(u.len()))?.to_vec();
        let n_s = if u.arbitrary()? {
            f.v_r().get().wrapping_add(u.int_in_range(0..=4)?)
        } else {
            u.arbitrary()?
        };
        let buffer_available = u.arbitrary()?;
        let class: u8 = u.int_in_range(0..=6)?;
        let mut bc_known = false;
        let input = match class {
            0 | 1 => Farm1Input::Frame {
                frame: Farm1Frame::Ad {
                    sequence_number: FrameSeqNum::new(n_s),
                    data: &data,
                },
                buffer_available,
            },
            2 => Farm1Input::Frame {
                frame: Farm1Frame::Bd { data: &data },
                buffer_available,
            },
            3 => Farm1Input::Frame {
                frame: Farm1Frame::Bc {
                    command: match u.int_in_range(0..=2)? {
                        0 => Some(ControlCommand::Unlock),
                        1 => Some(ControlCommand::SetVr(FrameSeqNum::new(u.arbitrary()?))),
                        _ => None,
                    }
                    .inspect(|_| bc_known = true),
                },
                buffer_available,
            },
            4 => Farm1Input::InvalidFrame,
            5 => Farm1Input::BufferReleased,
            _ => Farm1Input::SetPhysicalFlags {
                no_rf_available: u.arbitrary()?,
                no_bit_lock: u.arbitrary()?,
            },
        };
        let before = f.state();
        let mut area = [0_u8; 8];
        let mut again = [0_u8; 8];
        let at = Instant::from_micros(0);
        let first = f.step(input, at, &mut area);
        assert_eq!(first, f.step(input, at, &mut again));
        let Ok((after, actions)) = first else {
            continue;
        };
        assert_eq!(area, again);
        assert!(actions.len() <= FARM1_MAX_ACTIONS);
        // The event indices of Table 6-1 (E1 to E11 are 0 to 10) that the class can select.
        let events: &[usize] = match (class, bc_known) {
            (0 | 1, _) => &[0, 1, 2, 3, 4],
            (2, _) => &[5],
            (3, true) => &[6, 7],
            (3, false) | (4, _) => &[8],
            (5, _) => &[9],
            _ => &[],
        };
        let row = &FARM1_TABLE[before as usize];
        assert!(
            (class == 6 && after.state() == before)
                || events
                    .iter()
                    .any(|e| row[*e].next_state == Some(after.state()))
        );
        assert_eq!(after.lockout_flag(), after.state() == Farm1State::Lockout);
        assert_eq!(after.wait_flag(), after.state() == Farm1State::Wait);
        f = after;
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = if u.arbitrary().unwrap_or(false) {
        fop1(&mut u)
    } else {
        farm1(&mut u)
    };
});
