//! Calls every public `aldis-cop1` FOP-1 API (VER-010; M3 WP-4). Inputs pass through
//! `black_box`.

use core::hint::black_box as bb;
use core::num::NonZeroU8;

use aldis_cop1::{
    Directive, Fop1, Fop1Config, Fop1Event, Fop1Input, Fop1Output, Fop1State, Fop1Target,
    FrameKind, SuspendState, TimeoutType, fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{Clcw, ClcwStatus, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId};

use crate::exercise::Tally;

/// A small instance: four frames of at most 64 octets.
const Q: usize = 4;
const F: usize = 64;
const N: usize = fop1_max_actions(Q);
type Engine = Fop1<Q, F, N>;

/// The result of a step that succeeded, or `None`.
fn step(f: &Engine, input: Fop1Input<'_>, at: u64, out: &mut [u8]) -> Option<Engine> {
    f.step(bb(input), Instant::from_micros(at), out)
        .ok()
        .map(|(next, _)| next)
}

/// A CLCW of VCID 1 with the given flags and N(R).
fn clcw(lockout: bool, wait: bool, retransmit: bool, n_r: u8) -> Option<Fop1Input<'static>> {
    Some(Fop1Input::Clcw(Clcw {
        status: ClcwStatus::new(0)?,
        cop_in_effect: CopInEffect::COP_1,
        vc_id: TcVcId::new(1)?,
        no_rf_available: false,
        no_bit_lock: false,
        lockout,
        wait,
        retransmit,
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(bb(n_r)),
    }))
}

/// The engine and its queries before any input.
fn fresh(t: &mut Tally) -> Option<Engine> {
    let target = Fop1Target::tc(
        TcSpacecraftId::new(bb(42))?,
        TcVcId::new(bb(1))?,
        bb(F),
        bb(true),
    )
    .ok()?;
    t.check(target.max_frame_len() == F && target.max_fdu_len() == F - 7);
    t.check(target.spacecraft_id().get() == 42 && target.vc_id().get() == 1);
    let config = Fop1Config {
        target,
        t1_initial: Duration::from_micros(bb(1000)),
        transmission_limit: NonZeroU8::new(bb(2))?,
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: bb(2),
        ad_repetitions: bb(1),
        bc_repetitions: bb(1),
    };
    t.check(
        Engine::new(Fop1Config {
            sliding_window_k: bb(9),
            ..config.clone()
        })
        .is_err(),
    );
    let f = Engine::new(config).ok()?;
    t.check(f.state() == Fop1State::Initial && f.v_s().get() == 0 && f.nn_r().get() == 0);
    t.check(f.sliding_window_k() == 2 && f.t1_initial().as_micros() == 1000);
    t.check(f.transmission_limit().get() == 2 && f.timeout_type() == TimeoutType::Tt0);
    t.check(f.transmission_count() == 1 && f.suspend_state() == SuspendState::NotSuspended);
    t.check(f.sent_queue_len() == 0 && !f.wait_queue_occupied() && f.next_deadline().is_none());
    t.check(f.out_flag_ready(FrameKind::Ad) && f.target().vc_id().get() == 1);
    Some(f)
}

/// Every input kind through `step`, and one cell through `step_event`.
fn inputs(t: &mut Tally, f: &Engine) -> Option<()> {
    let mut out = [0_u8; F];
    let fdu = bb([0xA0_u8, 1]);
    let initiate = Fop1Input::Directive {
        handle: bb(1),
        directive: Directive::InitiateAdWithoutClcwCheck,
    };
    let f = step(f, initiate, 0, &mut out)?;
    t.check(f.state() == Fop1State::Active);
    let request = Fop1Input::RequestAd {
        handle: bb(2),
        fdu: &fdu,
    };
    let (f, actions) = f.step(request, Instant::from_micros(1), &mut out).ok()?;
    t.check(matches!(
        actions.get(1),
        Some(Fop1Output::Transmit {
            kind: FrameKind::Ad,
            ..
        })
    ));
    let done = Fop1Input::TransmitDone {
        kind: FrameKind::Ad,
        accepted: bb(true),
    };
    let f = step(&f, done, 2, &mut out)?;
    t.check(f.next_deadline() == Some(Instant::from_micros(1001)));
    let f = step(&f, Fop1Input::Timeout, 1001, &mut out)?;
    t.check(f.transmission_count() == 2);
    let f = step(&f, clcw(false, false, false, 1)?, 3, &mut out)?;
    t.check(f.sent_queue_len() == 0 && f.nn_r().get() == 1);
    let bd = Fop1Input::RequestBd {
        handle: bb(3),
        fdu: &fdu,
    };
    let f = step(&f, bd, 4, &mut out)?;
    t.check(!f.out_flag_ready(FrameKind::Bd));
    let set_k = Fop1Input::Directive {
        handle: bb(4),
        directive: Directive::SetSlidingWindowWidth { width: bb(1) },
    };
    let f = step(&f, set_k, 5, &mut out)?;
    t.check(f.sliding_window_k() == 1);
    let invalid = Fop1Input::InvalidDirective { handle: bb(5) };
    t.check(step(&f, invalid, 6, &mut out).is_some_and(|g| g == f));
    t.check(
        step(&f, Fop1Input::InvalidClcw, 7, &mut out)
            .is_some_and(|g| g.state() == Fop1State::Initial),
    );
    let na = f.step_event(
        Fop1Event::E31,
        Fop1Input::Timeout,
        Instant::from_micros(8),
        &mut out,
    );
    t.check(na.is_err());
    // Cells through `step_event` with an event and input `step` does not pair.
    let limit = Fop1Input::Directive {
        handle: bb(6),
        directive: Directive::SetTransmissionLimit {
            limit: NonZeroU8::new(bb(3))?,
        },
    };
    let (g, _) = f
        .step_event(bb(Fop1Event::E38), limit, Instant::from_micros(9), &mut out)
        .ok()?;
    t.check(g.transmission_limit().get() == 3);
    let bd_done = Fop1Input::TransmitDone {
        kind: FrameKind::Ad,
        accepted: bb(true),
    };
    let bd = f.step_event(
        bb(Fop1Event::E45),
        bd_done,
        Instant::from_micros(10),
        &mut out,
    );
    t.check(bd.is_ok_and(|(g, _)| g.out_flag_ready(FrameKind::Bd)));
    Some(())
}

/// Every public FOP-1 API.
pub fn run(t: &mut Tally) {
    t.check(fop1_max_actions(bb(Q)) == N);
    let Some(f) = fresh(t) else {
        t.check(false);
        return;
    };
    let ran = inputs(t, &f).is_some();
    t.check(ran);
}
