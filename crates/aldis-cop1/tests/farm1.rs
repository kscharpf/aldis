//! FARM-1 engine tests through the public `step` and `clcw` (COP-002, COP-006, COP-030…COP-036).
//!
//! Every one of the 33 cells of Table 6-1 is executed from every state reachable by inputs, and
//! compared with an expectation transcribed here from CCSDS 232.1-B-2 Table 6-1 (independent of
//! `FARM1_TABLE`, which the engine reads).

#![expect(
    clippy::many_single_char_names,
    clippy::unwrap_used,
    reason = "tests fail loudly by design"
)]

use aldis_cop1::{
    FARM1_MAX_ACTIONS, Farm1, Farm1Config, Farm1Event, Farm1Frame, Farm1Input, Farm1Notify,
    Farm1Output, Farm1State, Farm1Window, FduKind, OutRange,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::Instant;
use aldis_core::{ErrorDetail, ErrorKind, field};
use aldis_tc::{ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, TcVcId};

const DATA: [u8; 3] = [0xA1, 0xB2, 0xC3];
const NOW: Instant = Instant::from_micros(0);

const fn seq(v: u8) -> FrameSeqNum {
    FrameSeqNum::new(v)
}

const fn config(window: Farm1Window) -> Farm1Config {
    Farm1Config {
        vc_id: TcVcId::new(5).unwrap(),
        status: ClcwStatus::new(3).unwrap(),
        window,
    }
}

/// W = 16, PW = NW = 8 (retransmission allowed).
fn window16() -> Farm1Window {
    Farm1Window::retransmission_allowed(16).unwrap()
}

fn fresh(v_r: u8) -> Farm1 {
    Farm1::new(config(window16()), seq(v_r))
}

const fn ad(n_s: u8, buffer_available: bool) -> Farm1Input<'static> {
    Farm1Input::Frame {
        frame: Farm1Frame::Ad {
            sequence_number: seq(n_s),
            data: &DATA,
        },
        buffer_available,
    }
}

const fn bd() -> Farm1Input<'static> {
    Farm1Input::Frame {
        frame: Farm1Frame::Bd { data: &DATA },
        buffer_available: true,
    }
}

const fn bc(command: Option<ControlCommand>) -> Farm1Input<'static> {
    Farm1Input::Frame {
        frame: Farm1Frame::Bc { command },
        buffer_available: true,
    }
}

/// A successful step: the next state and the actions as a vector.
fn go(f: &Farm1, input: Farm1Input<'_>) -> (Farm1, Vec<Farm1Output>) {
    let mut out = [0u8; 16];
    let (next, actions) = f.step(input, NOW, &mut out).unwrap();
    assert!(actions.len() <= FARM1_MAX_ACTIONS);
    (next, actions.iter().copied().collect())
}

fn apply(f: &Farm1, inputs: &[Farm1Input<'_>]) -> Farm1 {
    inputs.iter().fold(*f, |f, i| go(&f, *i).0)
}

const STATES: [Farm1State; 3] = [Farm1State::Open, Farm1State::Wait, Farm1State::Lockout];

const EVENTS: [Farm1Event; 10] = [
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
];

/// The input that raises `event` (E1 to E10) in `f`, with V*(R) = 77 for Set V(R).
#[expect(clippy::unreachable, reason = "E11 is a query, not an input")]
fn input_for(f: &Farm1, event: Farm1Event) -> Farm1Input<'static> {
    let v = f.v_r().get();
    match event {
        Farm1Event::E1 => ad(v, true),
        Farm1Event::E2 => ad(v, false),
        Farm1Event::E3 => ad(v.wrapping_add(3), true),
        Farm1Event::E4 => ad(v.wrapping_sub(3), true),
        Farm1Event::E5 => ad(v.wrapping_add(100), true),
        Farm1Event::E6 => bd(),
        Farm1Event::E7 => bc(Some(ControlCommand::Unlock)),
        Farm1Event::E8 => bc(Some(ControlCommand::SetVr(seq(77)))),
        Farm1Event::E9 => Farm1Input::InvalidFrame,
        Farm1Event::E10 => Farm1Input::BufferReleased,
        Farm1Event::E11 => unreachable!("E11 is the query clcw"),
    }
}

/// Every flag combination FARM-1 reaches in `state` from a fresh instance with V(R) = 10.
fn starts(state: Farm1State) -> Vec<Farm1> {
    let f = fresh(10);
    match state {
        Farm1State::Open => vec![f, apply(&f, &[ad(12, true)])],
        Farm1State::Wait => vec![apply(&f, &[ad(10, false)])],
        Farm1State::Lockout => {
            let waiting = apply(&f, &[ad(10, false), ad(110, true)]);
            vec![
                apply(&f, &[ad(110, true)]),
                waiting,
                apply(&waiting, &[Farm1Input::BufferReleased]),
            ]
        }
    }
}

#[derive(Clone, Copy)]
enum Vr {
    Same,
    Inc,
    Set,
}

#[derive(Clone, Copy)]
enum Flag {
    Keep,
    To(bool),
}

/// What Table 6-1 says one cell does.
#[derive(Clone, Copy)]
struct Cell {
    next: Option<Farm1State>,
    vr: Vr,
    retransmit: Flag,
    wait: Flag,
    lockout: Flag,
    farm_b: bool,
    deliver: Option<FduKind>,
    discard: bool,
}

const NONE: Cell = Cell {
    next: None,
    vr: Vr::Same,
    retransmit: Flag::Keep,
    wait: Flag::Keep,
    lockout: Flag::Keep,
    farm_b: false,
    deliver: None,
    discard: false,
};

/// Table 6-1 of CCSDS 232.1-B-2, transcribed cell by cell (COP-002).
#[expect(
    clippy::too_many_lines,
    reason = "the 33 cells of Table 6-1, one arm each"
)]
fn table(state: Farm1State, event: Farm1Event) -> Cell {
    use Farm1Event::{E1, E2, E3, E4, E5, E6, E7, E8, E9, E10, E11};
    use Farm1State::{Lockout as S3, Open as S1, Wait as S2};
    let discard = |next| Cell {
        next: Some(next),
        discard: true,
        ..NONE
    };
    let accept_bd = |next| Cell {
        next: Some(next),
        farm_b: true,
        deliver: Some(FduKind::Bd),
        ..NONE
    };
    let lockout_out = |next| Cell {
        next: Some(next),
        discard: true,
        lockout: Flag::To(true),
        ..NONE
    };
    let ignore = |next| Cell {
        next: Some(next),
        ..NONE
    };
    match (state, event) {
        (S1, E1) => Cell {
            next: Some(S1),
            vr: Vr::Inc,
            retransmit: Flag::To(false),
            deliver: Some(FduKind::Ad),
            ..NONE
        },
        (S2, E1) => NONE,
        (S3, E1 | E2 | E3 | E4 | E5 | E9) => discard(S3),
        (S1, E2) => Cell {
            next: Some(S2),
            retransmit: Flag::To(true),
            wait: Flag::To(true),
            discard: true,
            ..NONE
        },
        (S2, E2 | E3 | E4 | E9) => discard(S2),
        (S1, E3) => Cell {
            retransmit: Flag::To(true),
            ..discard(S1)
        },
        (S1, E4 | E9) => discard(S1),
        (S1 | S2, E5) => lockout_out(S3),
        (S1, E6) => accept_bd(S1),
        (S2, E6) => accept_bd(S2),
        (S3, E6) => accept_bd(S3),
        (S1, E7) => Cell {
            next: Some(S1),
            retransmit: Flag::To(false),
            farm_b: true,
            ..NONE
        },
        (S2, E7) => Cell {
            next: Some(S1),
            retransmit: Flag::To(false),
            wait: Flag::To(false),
            farm_b: true,
            ..NONE
        },
        (S3, E7) => Cell {
            next: Some(S1),
            retransmit: Flag::To(false),
            wait: Flag::To(false),
            lockout: Flag::To(false),
            farm_b: true,
            ..NONE
        },
        (S1, E8) => Cell {
            next: Some(S1),
            vr: Vr::Set,
            retransmit: Flag::To(false),
            farm_b: true,
            ..NONE
        },
        (S2, E8) => Cell {
            next: Some(S1),
            vr: Vr::Set,
            retransmit: Flag::To(false),
            wait: Flag::To(false),
            farm_b: true,
            ..NONE
        },
        (S3, E8) => Cell {
            next: Some(S3),
            farm_b: true,
            ..NONE
        },
        (S1, E10) => ignore(S1),
        (S2, E10) => Cell {
            wait: Flag::To(false),
            ..ignore(S1)
        },
        (S3, E10) => Cell {
            wait: Flag::To(false),
            ..ignore(S3)
        },
        // E11 reports and changes nothing; it is the query `clcw`, not an input.
        (_, E11) => ignore(state),
    }
}

const fn flag(old: bool, change: Flag) -> bool {
    match change {
        Flag::Keep => old,
        Flag::To(v) => v,
    }
}

#[test]
fn every_cell_of_table_6_1_through_step() {
    // req: COP-002, COP-003, COP-004, COP-005, COP-006, COP-033, COP-034, COP-036, API-057, API-058, API-083, RS-021
    let mut cells = 0;
    for state in STATES {
        for event in EVENTS {
            for before in starts(state) {
                assert_eq!(before.state(), state);
                let input = input_for(&before, event);
                let want = table(state, event);
                let mut out = [0u8; 16];
                let Some(next) = want.next else {
                    // Not applicable: InvalidState, nothing changed, nothing written (COP-005).
                    let e = before.step(input, NOW, &mut out).unwrap_err();
                    assert_eq!(e.kind(), ErrorKind::InvalidState, "{state:?} {event:?}");
                    assert_eq!(out, [0u8; 16]);
                    cells += 1;
                    continue;
                };
                let (after, actions) = before.step(input, NOW, &mut out).unwrap();
                let at = format!("{state:?} {event:?} from {before:?}");
                assert_eq!(after.state(), next, "{at}");
                let v_star = 77;
                assert_eq!(
                    after.v_r(),
                    match want.vr {
                        Vr::Same => before.v_r(),
                        Vr::Inc => before.v_r().next(),
                        Vr::Set => seq(v_star),
                    },
                    "{at}"
                );
                assert_eq!(
                    after.retransmit_flag(),
                    flag(before.retransmit_flag(), want.retransmit),
                    "{at}"
                );
                // The Wait and Lockout Flags are functions of the State (232.1 §6.1.3, §6.1.4;
                // OI-64), so (S2, E5), which leaves Wait_Flag set in the table, reports it clear.
                assert_eq!(after.wait_flag(), next == Farm1State::Wait, "{at}");
                assert_eq!(after.lockout_flag(), next == Farm1State::Lockout, "{at}");
                // The printed flag actions agree with that, from the flags the state implies.
                let was = (state == Farm1State::Wait, state == Farm1State::Lockout);
                let printed = (flag(was.0, want.wait), flag(was.1, want.lockout));
                let exception = state == Farm1State::Wait && event == Farm1Event::E5;
                assert_eq!(
                    printed,
                    (
                        next == Farm1State::Wait || exception,
                        next == Farm1State::Lockout
                    ),
                    "{at}"
                );
                let b = before.farm_b_counter().get();
                assert_eq!(
                    after.farm_b_counter().get(),
                    if want.farm_b { (b + 1) % 4 } else { b },
                    "{at}"
                );

                // The actions in the order of the cell: Accept or Discard, Unlocked, State change.
                let mut expect = Vec::new();
                if let Some(kind) = want.deliver {
                    expect.push(Farm1Output::Deliver {
                        kind,
                        sequence_number: match kind {
                            FduKind::Ad => before.v_r(),
                            FduKind::Bd => seq(0),
                        },
                        data: OutRange::new(0, DATA.len()),
                    });
                    assert_eq!(out[..DATA.len()], DATA, "{at}");
                }
                if want.discard {
                    expect.push(Farm1Output::Notify(Farm1Notify::FrameDiscarded {
                        event,
                        state,
                    }));
                }
                if event == Farm1Event::E7 {
                    expect.push(Farm1Output::Notify(Farm1Notify::Unlocked));
                }
                if next != state {
                    expect.push(Farm1Output::Notify(Farm1Notify::StateChanged {
                        event,
                        from: state,
                        to: next,
                    }));
                }
                assert_eq!(actions.iter().copied().collect::<Vec<_>>(), expect, "{at}");
                cells += 1;
            }
        }
    }
    // Ten events from every reachable start of S1 (2), S2 (1), and S3 (3).
    assert_eq!(cells, 10 * (2 + 1 + 3));
}

#[test]
fn event_e11_reports_the_state_without_changing_it() {
    // req: COP-002, COP-035, API-056
    for state in STATES {
        for f in starts(state) {
            let c = f.clcw();
            assert_eq!(c.cop_in_effect, CopInEffect::COP_1);
            assert_eq!(c.vc_id, TcVcId::new(5).unwrap());
            assert_eq!(c.status, ClcwStatus::new(3).unwrap());
            assert_eq!(
                (c.lockout, c.wait, c.retransmit),
                (f.lockout_flag(), f.wait_flag(), f.retransmit_flag())
            );
            assert_eq!(c.farm_b_counter, f.farm_b_counter());
            assert_eq!(c.report_value, f.v_r());
            assert_eq!(c.to_octets(), f.clcw().to_octets());
            assert_eq!(f.config(), config(window16()));
        }
    }
}

#[test]
fn clcw_follows_the_state_after_every_input() {
    // req: COP-035, OI-64
    let f = fresh(10);
    assert_eq!(f.state(), Farm1State::Open);
    let c = f.clcw();
    assert_eq!(
        (c.lockout, c.wait, c.retransmit, c.farm_b_counter.get()),
        (false, false, false, 0)
    );
    assert_eq!(c.report_value, seq(10));
    // An accepted frame moves N(R); a gap sets Retransmit; no buffer sets Wait too.
    let f = go(&f, ad(10, true)).0;
    assert_eq!(f.clcw().report_value, seq(11));
    let f = go(&f, ad(13, true)).0;
    assert!(f.clcw().retransmit && !f.clcw().wait);
    let f = go(&f, ad(11, false)).0;
    assert!(f.clcw().retransmit && f.clcw().wait);
    let f = go(&f, ad(111, true)).0;
    // Wait_Flag is 0 in Lockout, although the cell does not clear it (OI-64).
    assert!(f.clcw().lockout && !f.clcw().wait);
    assert_eq!(f.state(), Farm1State::Lockout);
    let f = go(&f, bc(Some(ControlCommand::Unlock))).0;
    let c = f.clcw();
    assert_eq!(
        (c.lockout, c.wait, c.retransmit, c.farm_b_counter.get()),
        (false, false, false, 1)
    );
}

#[test]
fn physical_flags_reach_the_clcw_and_nothing_else() {
    // req: COP-035
    let f = fresh(3);
    let (g, actions) = go(
        &f,
        Farm1Input::SetPhysicalFlags {
            no_rf_available: true,
            no_bit_lock: false,
        },
    );
    assert_eq!(actions, []);
    assert!(g.clcw().no_rf_available && !g.clcw().no_bit_lock);
    let (h, _) = go(
        &g,
        Farm1Input::SetPhysicalFlags {
            no_rf_available: false,
            no_bit_lock: true,
        },
    );
    assert!(!h.clcw().no_rf_available && h.clcw().no_bit_lock);
    let mut same = h.clcw();
    same.no_bit_lock = false;
    assert_eq!(same, f.clcw());
    assert_eq!(
        (h.state(), h.v_r(), h.wait_flag()),
        (f.state(), f.v_r(), f.wait_flag())
    );
}

#[test]
fn window_classification_with_modular_sequence_numbers() {
    // req: COP-030
    // V(R) = 250, PW = NW = 8: 250 is expected, 251..=257 (mod 256) are ahead in the positive
    // window, 242..=249 behind in the negative window, everything else is the lockout area.
    let f = fresh(250);
    let event = |n_s: u8| {
        let (g, out) = go(&f, ad(n_s, true));
        match (g.state(), out.first()) {
            (Farm1State::Lockout, _) => Farm1Event::E5,
            (_, Some(Farm1Output::Deliver { .. })) => Farm1Event::E1,
            (_, _) if g.retransmit_flag() => Farm1Event::E3,
            (_, _) => Farm1Event::E4,
        }
    };
    for n in 0..=255u8 {
        let ahead = (i32::from(n) - 250).rem_euclid(256);
        let want = match ahead {
            0 => Farm1Event::E1,
            1..=7 => Farm1Event::E3,
            248..=255 => Farm1Event::E4,
            _ => Farm1Event::E5,
        };
        assert_eq!(event(n), want, "N(S) = {n}");
    }
    // The edges named in the Blue Book inequalities of 6.1.8.3.1.
    assert_eq!(event(1), Farm1Event::E3); // V(R) + PW - 1
    assert_eq!(event(2), Farm1Event::E5); // V(R) + PW
    assert_eq!(event(242), Farm1Event::E4); // V(R) - NW
    assert_eq!(event(241), Farm1Event::E5); // V(R) - NW - 1
}

#[test]
fn window_classification_with_an_asymmetric_window() {
    // req: COP-030
    // Retransmission not allowed: W = 10, PW = 7, NW = 3.
    let w = Farm1Window::retransmission_not_allowed(10, 7).unwrap();
    assert_eq!((w.width(), w.positive(), w.negative()), (10, 7, 3));
    let f = Farm1::new(config(w), seq(0));
    let class = |n_s: u8| {
        let (g, out) = go(&f, ad(n_s, true));
        if g.state() == Farm1State::Lockout {
            "E5"
        } else if matches!(out.first(), Some(Farm1Output::Deliver { .. })) {
            "E1"
        } else if g.retransmit_flag() {
            "E3"
        } else {
            "E4"
        }
    };
    assert_eq!(class(0), "E1");
    assert_eq!([class(1), class(6), class(7)], ["E3", "E3", "E5"]);
    assert_eq!([class(255), class(253), class(252)], ["E4", "E4", "E5"]);

    // PW = W = 256, NW = 0: everything but N(S) = V(R) is in the positive window.
    let all = Farm1::new(
        config(Farm1Window::retransmission_not_allowed(256, 256).unwrap()),
        seq(200),
    );
    for n in 0..=255u8 {
        let (g, _) = go(&all, ad(n, true));
        assert_eq!(g.state(), Farm1State::Open, "N(S) = {n}");
        assert_eq!(g.retransmit_flag(), n != 200, "N(S) = {n}");
    }
    // PW = 1: the positive window is V(R) alone, so V(R) + 1 is in the lockout area.
    let one = Farm1::new(
        config(Farm1Window::retransmission_not_allowed(128, 1).unwrap()),
        seq(0),
    );
    assert_eq!(go(&one, ad(1, true)).0.state(), Farm1State::Lockout);
    assert_eq!(go(&one, ad(255, true)).0.state(), Farm1State::Open);
}

#[test]
fn window_constructors_enforce_table_7_2() {
    // req: COP-030, API-040
    let w = Farm1Window::retransmission_allowed(2).unwrap();
    assert_eq!((w.width(), w.positive(), w.negative()), (2, 1, 1));
    let w = Farm1Window::retransmission_allowed(254).unwrap();
    assert_eq!((w.positive(), w.negative()), (127, 127));
    for bad in [0, 1, 3, 253, 255, 256, 1000] {
        let e = Farm1Window::retransmission_allowed(bad).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::FARM_SLIDING_WINDOW_W)),
            "W = {bad}"
        );
    }
    let ok = Farm1Window::retransmission_not_allowed(256, 256).unwrap();
    assert_eq!((ok.width(), ok.positive(), ok.negative()), (256, 256, 0));
    let ok = Farm1Window::retransmission_not_allowed(256, 129).unwrap();
    assert_eq!(ok.negative(), 127);
    let ok = Farm1Window::retransmission_not_allowed(1, 1).unwrap();
    assert_eq!((ok.positive(), ok.negative()), (1, 0));
    for (w, pw, bad) in [
        (0, 1, field::FARM_SLIDING_WINDOW_W),
        (257, 1, field::FARM_SLIDING_WINDOW_W),
        (10, 0, field::FARM_POSITIVE_WINDOW_PW),
        (10, 11, field::FARM_POSITIVE_WINDOW_PW),
        (256, 128, field::FARM_NEGATIVE_WINDOW_NW),
        (200, 1, field::FARM_NEGATIVE_WINDOW_NW),
    ] {
        let e = Farm1Window::retransmission_not_allowed(w, pw).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(bad)),
            "W = {w}, PW = {pw}"
        );
    }
}

#[test]
fn farm_b_counter_counts_valid_type_b_frames_modulo_four() {
    // req: COP-034
    let mut f = fresh(0);
    for expected in [1, 2, 3, 0, 1, 2] {
        f = go(&f, bd()).0;
        assert_eq!(f.farm_b_counter(), FarmBCounter::new(expected).unwrap());
    }
    // Type-BC frames count too, Set V(R) in Lockout included (E8 in S3); Type-AD frames, invalid
    // frames, and a Type-BC frame with an unknown command do not.
    let mut f = fresh(0);
    f = go(&f, bc(Some(ControlCommand::Unlock))).0;
    f = go(&f, bc(Some(ControlCommand::SetVr(seq(9))))).0;
    assert_eq!(f.farm_b_counter().get(), 2);
    f = go(&f, ad(9, true)).0;
    f = go(&f, Farm1Input::InvalidFrame).0;
    f = go(&f, bc(None)).0;
    f = go(&f, Farm1Input::BufferReleased).0;
    assert_eq!(f.farm_b_counter().get(), 2);
    f = go(&f, ad(100, true)).0;
    assert_eq!(f.state(), Farm1State::Lockout);
    f = go(&f, bc(Some(ControlCommand::SetVr(seq(1))))).0;
    assert_eq!((f.farm_b_counter().get(), f.v_r()), (3, seq(10)));
}

#[test]
fn bc_frame_without_a_valid_command_is_an_invalid_frame() {
    // req: COP-031
    for state in STATES {
        for f in starts(state) {
            let (g, actions) = go(&f, bc(None));
            let (h, expected) = go(&f, Farm1Input::InvalidFrame);
            assert_eq!((g, &actions), (h, &expected));
            assert_eq!(g.state(), state);
            assert_eq!(
                actions,
                [Farm1Output::Notify(Farm1Notify::FrameDiscarded {
                    event: Farm1Event::E9,
                    state
                })]
            );
        }
    }
}

#[test]
fn buffer_availability_is_the_callers_input() {
    // req: COP-033, API-052
    let f = fresh(0);
    // No buffer: Wait, Retransmit; the frame is not delivered.
    let (w, a) = go(&f, ad(0, false));
    assert_eq!(w.state(), Farm1State::Wait);
    assert!(w.wait_flag() && w.retransmit_flag());
    assert!(!a.iter().any(|x| matches!(x, Farm1Output::Deliver { .. })));
    // Further frames are discarded until the Buffer Release Signal, whatever they carry.
    for input in [ad(0, false), ad(2, true), ad(255, true)] {
        let (g, _) = go(&w, input);
        assert_eq!(g.state(), Farm1State::Wait);
        assert_eq!(g.v_r(), seq(0));
    }
    // A buffer the caller has not announced with E10 is the "not applicable" cell (COP-005).
    let mut out = [0u8; 8];
    let e = w.step(ad(0, true), NOW, &mut out).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::InvalidState);
    // A Type-BD frame is accepted in Wait.
    let (_, a) = go(&w, bd());
    assert!(matches!(
        a.first(),
        Some(Farm1Output::Deliver {
            kind: FduKind::Bd,
            ..
        })
    ));
    // E10 reopens; the retransmitted frame is accepted and clears Retransmit.
    let (o, _) = go(&w, Farm1Input::BufferReleased);
    assert_eq!(
        (o.state(), o.wait_flag(), o.retransmit_flag()),
        (Farm1State::Open, false, true)
    );
    let (g, a) = go(&o, ad(0, true));
    assert_eq!((g.v_r(), g.retransmit_flag()), (seq(1), false));
    assert_eq!(a.len(), 1);
    // A caller without flow control never reaches Wait.
    let mut f = fresh(250);
    for n in 250..=255u8 {
        f = go(&f, ad(n, true)).0;
        assert_eq!(f.state(), Farm1State::Open);
    }
    assert_eq!(f.v_r(), seq(0));
}

#[test]
fn lockout_is_left_only_by_unlock() {
    // req: COP-036, COP-002
    let f = apply(&fresh(5), &[ad(5, true), ad(150, true)]);
    assert_eq!((f.state(), f.v_r()), (Farm1State::Lockout, seq(6)));
    // Frames, Set V(R) and the Buffer Release Signal leave the engine in Lockout.
    for input in [
        ad(6, true),
        ad(6, false),
        ad(7, true),
        bc(Some(ControlCommand::SetVr(seq(40)))),
        Farm1Input::BufferReleased,
        Farm1Input::InvalidFrame,
        bd(),
    ] {
        let (g, _) = go(&f, input);
        assert_eq!((g.state(), g.v_r()), (Farm1State::Lockout, seq(6)));
        assert!(g.lockout_flag());
    }
    // Unlock reports Unlocked so that the Higher Procedures can purge their buffers, resets the
    // flags, and does not touch V(R).
    let (g, a) = go(&f, bc(Some(ControlCommand::Unlock)));
    assert_eq!(
        a,
        [
            Farm1Output::Notify(Farm1Notify::Unlocked),
            Farm1Output::Notify(Farm1Notify::StateChanged {
                event: Farm1Event::E7,
                from: Farm1State::Lockout,
                to: Farm1State::Open
            })
        ]
    );
    assert_eq!((g.state(), g.v_r()), (Farm1State::Open, seq(6)));
    assert!(!g.lockout_flag() && !g.wait_flag() && !g.retransmit_flag());
}

#[test]
fn set_vr_sets_the_expected_sequence_number() {
    // req: COP-002, COP-036
    let f = apply(&fresh(5), &[ad(7, true)]);
    assert!(f.retransmit_flag());
    let (g, _) = go(&f, bc(Some(ControlCommand::SetVr(seq(200)))));
    assert_eq!((g.v_r(), g.retransmit_flag()), (seq(200), false));
    assert_eq!(go(&g, ad(200, true)).0.v_r(), seq(201));
}

/// Whether `actions` deliver a Type-AD FDU with sequence number `n`.
fn delivers_ad(actions: &[Farm1Output], n: u8) -> bool {
    actions.iter().any(|a| {
        matches!(a, Farm1Output::Deliver { kind: FduKind::Ad, sequence_number, .. } if *sequence_number == seq(n))
    })
}

#[test]
fn a_stale_set_vr_sets_v_r_back_and_accepts_a_delivered_frame_again() {
    // req: COP-002 (hz:HZ-COP1-18)
    // FARM-1 has no sequence number on a Type-BC frame, so a Set V(R) that arrives after the
    // Type-AD frames sent after it is executed like a current one (Table 6-1, E8).
    let f = apply(&fresh(0), &[ad(0, true), ad(1, true), ad(2, true)]);
    assert_eq!(f.v_r(), seq(3));
    let (g, _) = go(&f, bc(Some(ControlCommand::SetVr(seq(1)))));
    assert_eq!((g.state(), g.v_r()), (Farm1State::Open, seq(1)));
    let (h, a) = go(&g, ad(1, true));
    assert!(delivers_ad(&a, 1));
    assert_eq!(h.v_r(), seq(2));
}

#[test]
fn a_stale_unlock_reports_unlocked_and_clears_wait_after_frames_were_accepted() {
    // req: COP-002, COP-036 (hz:HZ-COP1-18)
    // The Unlocked action makes the caller purge its buffers (ASM-17), and E7 clears Wait, so an
    // Unlock that arrives after the Type-AD frames sent after it acts on frames already accepted.
    let f = apply(&fresh(0), &[ad(0, true), ad(1, true), ad(2, false)]);
    assert_eq!((f.state(), f.v_r()), (Farm1State::Wait, seq(2)));
    let (g, a) = go(&f, bc(Some(ControlCommand::Unlock)));
    assert!(a.contains(&Farm1Output::Notify(Farm1Notify::Unlocked)));
    assert_eq!((g.state(), g.v_r()), (Farm1State::Open, seq(2)));
    assert!(!g.wait_flag());
}

#[test]
fn errors_leave_the_state_unchanged_and_write_nothing() {
    // req: API-058, API-061, COP-005
    let f = fresh(0);
    let before = f;
    // BufferTooSmall: a Type-AD and a Type-BD FDU of three octets in an area of two.
    for input in [ad(0, true), bd()] {
        let mut out = [0xEE; 2];
        let e = f.step(input, NOW, &mut out).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(e.detail(), ErrorDetail::Needed(3));
        assert_eq!(out, [0xEE; 2]);
        assert_eq!(f, before);
    }
    // An empty area does for inputs that deliver nothing.
    for input in [
        ad(0, false),
        ad(5, true),
        bc(None),
        Farm1Input::BufferReleased,
    ] {
        assert!(f.step(input, NOW, &mut []).is_ok());
    }
    // An FDU exactly as long as the area fits.
    let mut out = [0u8; 3];
    assert!(f.step(ad(0, true), NOW, &mut out).is_ok());
    assert_eq!(out, DATA);
    // Not applicable: the same state, nothing written.
    let w = go(&f, ad(0, false)).0;
    let mut out = [0xEE; 3];
    assert!(w.step(ad(0, true), NOW, &mut out).is_err());
    assert_eq!((out, w), ([0xEE; 3], go(&f, ad(0, false)).0));
}

#[test]
fn step_is_deterministic_and_independent_of_the_clock() {
    // req: API-057, GEN-030, GEN-032
    let f = apply(&fresh(9), &[ad(11, true)]);
    for input in [ad(9, true), bd(), Farm1Input::BufferReleased] {
        let mut a = [0u8; 8];
        let mut b = [0u8; 8];
        let x = f.step(input, Instant::from_micros(0), &mut a).unwrap();
        let y = f
            .step(input, Instant::from_micros(u64::MAX), &mut b)
            .unwrap();
        assert_eq!((x, a), (y, b));
    }
}

#[test]
fn delivered_range_addresses_the_output_area() {
    // req: API-061
    let f = fresh(0);
    let mut out = [0u8; 8];
    let (_, actions) = f.step(ad(0, true), NOW, &mut out).unwrap();
    let Some(Farm1Output::Deliver {
        data,
        kind,
        sequence_number,
    }) = actions.get(0).copied()
    else {
        panic!("no Deliver");
    };
    assert_eq!((kind, sequence_number), (FduKind::Ad, seq(0)));
    assert_eq!(data.slice(&out), Some(&DATA[..]));
}
