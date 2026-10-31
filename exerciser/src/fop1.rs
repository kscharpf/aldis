//! Calls every public `aldis-cop1` FOP-1 API (VER-010; M3 WP-4). Inputs pass through
//! `black_box`.

#[path = "../../crates/aldis-cop1/tests/common/mod.rs"]
mod rig;

use core::hint::black_box as bb;
use core::num::NonZeroU8;

use aldis_cop1::{
    Directive, Fop1Channel, Fop1Config, Fop1Event, Fop1Input, Fop1Output, Fop1State, Fop1Store,
    Fop1Target, FrameKind, SuspendState, TimeoutType, fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{Clcw, ClcwStatus, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId};
use aldis_uslp::{
    MapConfig, PhysicalChannelConfig, TfdfHeader, TfdzRule, TransferFrameType, Upid, UslpMapId,
    UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcfCountLen,
};

use crate::exercise::Tally;

/// A small instance: four frames of at most 64 octets.
const Q: usize = 4;
const F: usize = 64;
const N: usize = fop1_max_actions(Q);
type Engine = rig::Rig<Q, F, N>;

/// Steps `f` in place by `decide` and `commit`; `None` if the input failed.
fn step(f: &mut Engine, input: Fop1Input<'_>, at: u64, out: &mut [u8]) -> Option<()> {
    let decision = f.decide(bb(input), Instant::from_micros(at), out).ok()?;
    let _ = f.commit(decision);
    Some(())
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
    t.check(matches!(
        target.channel(),
        Fop1Channel::Tc { spacecraft_id, vc_id } if spacecraft_id.get() == 42 && vc_id.get() == 1
    ));
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
    t.check(Fop1Store::<Q, F>::default() == f.store);
    t.check(f.state() == Fop1State::Initial && f.v_s().get() == 0 && f.nn_r().get() == 0);
    t.check(f.sliding_window_k() == 2 && f.t1_initial().as_micros() == 1000);
    t.check(f.transmission_limit().get() == 2 && f.timeout_type() == TimeoutType::Tt0);
    t.check(f.transmission_count() == 1 && f.suspend_state() == SuspendState::NotSuspended);
    t.check(f.sent_queue_len() == 0 && !f.wait_queue_occupied() && f.next_deadline().is_none());
    t.check(f.out_flag_ready(FrameKind::Ad) && f.target().max_frame_len() == F);
    Some(f)
}

/// Every input kind through `decide` and `commit`, and cells through `decide_event`.
fn inputs(t: &mut Tally, mut f: Engine) -> Option<()> {
    let mut out = [0_u8; F];
    let fdu = bb([0xA0_u8, 1]);
    let initiate = Fop1Input::Directive {
        handle: bb(1),
        directive: Directive::InitiateAdWithoutClcwCheck,
    };
    step(&mut f, initiate, 0, &mut out)?;
    t.check(f.state() == Fop1State::Active);
    let request = Fop1Input::RequestAd {
        handle: bb(2),
        fdu: &fdu,
    };
    let decision = f.decide(request, Instant::from_micros(1), &mut out).ok()?;
    let actions = f.commit(decision);
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
    step(&mut f, done, 2, &mut out)?;
    t.check(f.next_deadline() == Some(Instant::from_micros(1001)));
    step(&mut f, Fop1Input::Timeout, 1001, &mut out)?;
    t.check(f.transmission_count() == 2);
    step(&mut f, clcw(false, false, false, 1)?, 3, &mut out)?;
    t.check(f.sent_queue_len() == 0 && f.nn_r().get() == 1);
    let bd = Fop1Input::RequestBd {
        handle: bb(3),
        fdu: &fdu,
    };
    step(&mut f, bd, 4, &mut out)?;
    t.check(!f.out_flag_ready(FrameKind::Bd));
    let set_k = Fop1Input::Directive {
        handle: bb(4),
        directive: Directive::SetSlidingWindowWidth { width: bb(1) },
    };
    step(&mut f, set_k, 5, &mut out)?;
    t.check(f.sliding_window_k() == 1);
    // An invalid directive is answered and changes no state.
    let before = f.clone();
    step(
        &mut f,
        Fop1Input::InvalidDirective { handle: bb(5) },
        6,
        &mut out,
    )?;
    t.check(f == before);
    let na = f.fop.decide_event(
        &f.store,
        Fop1Event::E31,
        Fop1Input::Timeout,
        Instant::from_micros(8),
        &mut out,
    );
    t.check(na.is_err());
    // Cells through `decide_event` with an event and input `decide` does not pair.
    let limit = Fop1Input::Directive {
        handle: bb(6),
        directive: Directive::SetTransmissionLimit {
            limit: NonZeroU8::new(bb(3))?,
        },
    };
    let decision = f
        .fop
        .decide_event(
            &f.store,
            bb(Fop1Event::E38),
            limit,
            Instant::from_micros(9),
            &mut out,
        )
        .ok()?;
    let _ = f.commit(decision);
    t.check(f.transmission_limit().get() == 3);
    let bd_done = Fop1Input::TransmitDone {
        kind: FrameKind::Ad,
        accepted: bb(true),
    };
    let decision = f
        .fop
        .decide_event(
            &f.store,
            bb(Fop1Event::E45),
            bd_done,
            Instant::from_micros(10),
            &mut out,
        )
        .ok()?;
    let _ = f.commit(decision);
    t.check(f.out_flag_ready(FrameKind::Bd));
    step(&mut f, Fop1Input::InvalidClcw, 7, &mut out)?;
    t.check(f.state() == Fop1State::Initial);
    Some(())
}

/// The USLP target: a channel it refuses, one it accepts, and the frames of an engine on it.
fn uslp_target(t: &mut Tally) -> Option<()> {
    let scid = UslpSpacecraftId::new(bb(0x155))?;
    let vcid = UslpVcId::new(bb(1))?;
    let physical = PhysicalChannelConfig::new(TransferFrameType::Variable, bb(F), 0, true).ok()?;
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(bb(1)).ok()?,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc = VcConfig::new(scid, vcid, params).ok()?;
    let map = MapConfig::new(scid, vcid, UslpMapId::new(bb(2))?, SecurityLayout::NONE);
    let tfdf = TfdfHeader {
        rule: TfdzRule::NoSegmentation,
        upid: Upid::OctetStream,
    };
    let refused = VcConfig::new(
        scid,
        vcid,
        VcParams {
            vcf_count_len_sequence: VcfCountLen::new(bb(2)).ok()?,
            ..VcParams::new(TransferFrameType::Variable)
        },
    )
    .ok()?;
    t.check(Fop1Target::uslp(physical, refused, map, tfdf).is_err());
    let idle = TfdfHeader {
        upid: Upid::OnlyIdleData,
        ..tfdf
    };
    t.check(Fop1Target::uslp(physical, vc, map, idle).is_err());
    let target = Fop1Target::uslp(physical, vc, map, tfdf).ok()?;
    t.check(matches!(target.channel(), Fop1Channel::Uslp { .. }));
    t.check(target.max_frame_len() == F && target.max_fdu_len() == F - 11);
    let f = Engine::new(Fop1Config {
        target,
        t1_initial: Duration::from_micros(bb(1000)),
        transmission_limit: NonZeroU8::new(bb(2))?,
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: bb(2),
        ad_repetitions: bb(1),
        bc_repetitions: bb(1),
    })
    .ok()?;
    uslp_frames(t, f)
}

/// The frames of an engine on the USLP target: Type-BC, Type-AD, and Type-BD.
fn uslp_frames(t: &mut Tally, mut f: Engine) -> Option<()> {
    let mut out = [0_u8; F];
    // A Type-BC frame with the Unlock command, then a Type-AD and a Type-BD frame.
    let decision = f
        .decide(
            bb(Fop1Input::Directive {
                handle: bb(1),
                directive: Directive::InitiateAdWithUnlock,
            }),
            Instant::from_micros(0),
            &mut out,
        )
        .ok()?;
    let actions = f.commit(decision);
    t.check(actions.iter().any(|a| {
        matches!(
            a,
            Fop1Output::Transmit {
                kind: FrameKind::Bc,
                ..
            }
        )
    }));
    step(
        &mut f,
        Fop1Input::TransmitDone {
            kind: FrameKind::Bc,
            accepted: true,
        },
        1,
        &mut out,
    )?;
    step(&mut f, clcw(false, false, false, 0)?, 2, &mut out)?;
    let fdu = bb([0xA0_u8, 1]);
    let decision = f
        .decide(
            bb(Fop1Input::RequestAd {
                handle: bb(2),
                fdu: &fdu,
            }),
            Instant::from_micros(3),
            &mut out,
        )
        .ok()?;
    let actions = f.commit(decision);
    t.check(actions.iter().any(|a| {
        matches!(
            a,
            Fop1Output::Transmit {
                kind: FrameKind::Ad,
                ..
            }
        )
    }));
    let decision = f
        .decide(
            bb(Fop1Input::RequestBd {
                handle: bb(3),
                fdu: &fdu,
            }),
            Instant::from_micros(4),
            &mut out,
        )
        .ok()?;
    t.check(decision.actions().iter().any(|a| {
        matches!(
            a,
            Fop1Output::Transmit {
                kind: FrameKind::Bd,
                ..
            }
        )
    }));
    Some(())
}

/// Every public FOP-1 API.
pub fn run(t: &mut Tally) {
    t.check(fop1_max_actions(bb(Q)) == N);
    let Some(f) = fresh(t) else {
        t.check(false);
        return;
    };
    let ran = inputs(t, f).is_some();
    t.check(ran);
    let ran = uslp_target(t).is_some();
    t.check(ran);
}
