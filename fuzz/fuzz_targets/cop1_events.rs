//! Arbitrary event sequences for FOP-1 and FARM-1 (COP-041): after every input the engine is in
//! a state that Table 5-1 or Table 6-1 reaches from the state before it, the step is a pure
//! function (the same state and input give the same result), a refused step leaves the engine
//! as it was, and the engines hold their own bounds (the `Sent_Queue` capacity, the action
//! bound, the flags of FARM-1 as functions of its state; COP-006, COP-017, OI-64). No input
//! panics (GEN-020).
#![no_main]

// req: COP-041, COP-001, COP-002, COP-005, COP-006, COP-017, API-058, GEN-020, VER-022

use aldis_cop1::{
    AlertReason, Directive, FARM1_MAX_ACTIONS, FARM1_TABLE, FOP1_TABLE, Farm1, Farm1Config,
    Farm1Frame, Farm1Input, Farm1State, Farm1Window, Fop1, Fop1Config, Fop1Input, Fop1Output,
    Fop1State, Fop1Target, FrameKind, TimeoutType, fop1_max_actions,
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

fn fop1(u: &mut Unstructured<'_>) -> Result<()> {
    let target = Fop1Target::tc(
        TcSpacecraftId::new(7).unwrap(),
        TcVcId::new(VCID).unwrap(),
        F,
        u.arbitrary()?,
    )
    .unwrap();
    let mut f = Fop1::<Q, F, N>::new(Fop1Config {
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
        let input = match u.int_in_range(0..=7)? {
            0 => Fop1Input::Clcw(Clcw {
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
            }),
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
        let first = f.step(input, at, &mut area);
        // The step is a pure function of the state, the input, and the time.
        assert_eq!(first, f.step(input, at, &mut again));
        let Ok((after, actions)) = first else {
            continue;
        };
        assert_eq!(area, again);
        assert!(actions.len() <= N);
        assert!(
            FOP1_TABLE[before as usize]
                .iter()
                .any(|cell| cell.next_state == Some(after.state()))
        );
        assert!(after.sent_queue_len() <= Q);
        for a in actions.iter() {
            match a {
                Fop1Output::Alert(alert) if before == Fop1State::Initial => {
                    assert_eq!(alert.reason, AlertReason::Llif);
                }
                Fop1Output::Transmit { frame, .. } => {
                    assert!(frame.slice(&area).is_some_and(|b| !b.is_empty() && b.len() <= F));
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
        let input = match u.int_in_range(0..=6)? {
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
                    },
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
        assert!(
            after.state() == before
                || FARM1_TABLE[before as usize]
                    .iter()
                    .any(|cell| cell.next_state == Some(after.state()))
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
