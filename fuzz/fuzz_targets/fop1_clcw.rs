//! Arbitrary CLCW words fed to an active FOP-1 (COP-010, COP-012, COP-041): each 32-bit word is
//! decoded by `Clcw::decode` and, when it decodes, enters the FOP-1 as `Fop1Input::Clcw`, else as
//! `Fop1Input::InvalidClcw` (E1 to E15, E101 to E103 of Table 5-1). A CLCW of another Virtual
//! Channel is refused with `UnknownChannel` and nothing else; every accepted word leaves a state
//! that Table 5-1 reaches by a CLCW event from the state before; the decision is a pure function
//! of the state, the input, and the time; the `Sent_Queue` never grows on a CLCW, and N(R) moves
//! forward by at most the number of frames in it unless the CLCW raised an Alert (232.1 §5.2.2).
//! The CLCW events are interleaved with Type-AD requests, Transmit Responses, and Timeouts, so
//! that the `Sent_Queue` holds frames to acknowledge. No input panics (GEN-020).
#![no_main]

#[path = "../../crates/aldis-cop1/tests/common/mod.rs"]
mod common;

use common::Rig;

// req: COP-010, COP-012, COP-041, API-058, GEN-020, VER-022

use aldis_cop1::{
    Directive, FOP1_TABLE, Fop1Config, Fop1Decision, Fop1Input, Fop1Output, Fop1State, Fop1Target,
    FrameKind, TimeoutType, fop1_max_actions,
};
use aldis_core::ErrorKind;
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{Clcw, TcSpacecraftId, TcVcId};
use core::num::NonZeroU8;
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const Q: usize = 8;
const F: usize = 16;
const N: usize = fop1_max_actions(Q);
const VCID: u8 = 5;

/// Applies `input` as the harness of a lower layer would: decides, commits, and answers each
/// Transmit with an accepted Transmit Response. A refused input changes nothing (API-058).
fn drive(f: &mut Rig<Q, F, N>, input: Fop1Input<'_>, at: Instant) {
    let mut area = [0_u8; F];
    let Ok(decision) = f.decide(input, at, &mut area) else {
        return;
    };
    let kinds: Vec<FrameKind> = f
        .commit(decision)
        .iter()
        .filter_map(|a| match a {
            Fop1Output::Transmit { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    for kind in kinds {
        let mut area = [0_u8; F];
        let done = Fop1Input::TransmitDone {
            kind,
            accepted: true,
        };
        if let Ok(d) = f.decide(done, at, &mut area) {
            let _ = f.commit(d);
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one loop over the operations of the harness, each a short arm"
)]
fn run(u: &mut Unstructured<'_>) -> Result<()> {
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
        ad_repetitions: 1,
        bc_repetitions: 1,
    })
    .unwrap();
    let mut now = 0_u64;
    let at = |now: u64| Instant::from_micros(now);
    drive(
        &mut f,
        Fop1Input::Directive {
            handle: 0,
            directive: Directive::InitiateAdWithoutClcwCheck,
        },
        at(now),
    );
    assert_ne!(f.state(), Fop1State::Initial);
    let mut handle = 1_u32;
    // bound: each pass consumes at least one octet of the input, and `u` is finite
    while !u.is_empty() {
        now += u.int_in_range(0..=3_000)?;
        match u.int_in_range(0..=5)? {
            0 | 1 if f.state() != Fop1State::Initial => {
                let len = u.int_in_range(1..=F)?;
                let fdu = u.bytes(len.min(u.len()).max(1))?.to_vec();
                handle += 1;
                drive(&mut f, Fop1Input::RequestAd { handle, fdu: &fdu }, at(now));
            }
            2 => {
                drive(&mut f, Fop1Input::Timeout, at(now));
            }
            3 => {
                handle += 1;
                // Initiate AD with and without the CLCW check and with Set V(R) reach S4 and S5
                // as well as S1; the cells refused in the current state change nothing.
                let directive = match u.int_in_range(0..=3)? {
                    0 => Directive::InitiateAdWithoutClcwCheck,
                    1 => Directive::InitiateAdWithClcwCheck,
                    2 => Directive::InitiateAdWithSetVr {
                        v_r: FrameSeqNum::new(u.arbitrary()?),
                    },
                    _ => Directive::ResumeAd,
                };
                drive(&mut f, Fop1Input::Directive { handle, directive }, at(now));
            }
            _ => {
                let word: [u8; 4] = u.arbitrary()?;
                let (input, decoded) = match Clcw::decode(word) {
                    Ok(c) => (Fop1Input::Clcw(c), Some(c)),
                    Err(_) => (Fop1Input::InvalidClcw, None),
                };
                let before = f.state();
                let (queue, nn_r) = (f.sent_queue_len(), f.nn_r().get());
                let mut area = [0_u8; F];
                let mut again = [0_u8; F];
                let first = f.decide(input, at(now), &mut area);
                // The decision is a pure function of the state, the input, and the time.
                assert_eq!(
                    first.as_ref().map(Fop1Decision::actions),
                    f.decide(input, at(now), &mut again)
                        .as_ref()
                        .map(Fop1Decision::actions)
                );
                if decoded.is_some_and(|c| c.vc_id.get() != VCID) {
                    assert_eq!(first.unwrap_err().kind(), ErrorKind::UnknownChannel);
                    continue;
                }
                let Ok(decision) = first else {
                    continue;
                };
                let mut after = f.clone();
                let actions = after.commit(decision);
                assert!(actions.len() <= N);
                let events: &[usize] = if decoded.is_some() {
                    &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 46, 47, 48]
                } else {
                    &[14]
                };
                let row = &FOP1_TABLE[before as usize];
                assert!(
                    events
                        .iter()
                        .any(|e| row[*e].next_state == Some(after.state()))
                );
                assert!(after.sent_queue_len() <= queue);
                if after.state() != Fop1State::Initial && before != Fop1State::Initial {
                    let advance = usize::from(after.nn_r().get().wrapping_sub(nn_r));
                    assert!(advance <= queue);
                }
                f = after;
            }
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = run(&mut u);
});
