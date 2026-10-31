//! Property tests of the FARM-1 engine (VER-023), drawing frames and Control Commands from the
//! shared strategies of `aldis-testkit` (VER-054).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "tests fail loudly by design"
)]

use aldis_cop1::{
    FARM1_MAX_ACTIONS, Farm1, Farm1Config, Farm1Frame, Farm1Input, Farm1Output, Farm1State,
    Farm1Window, FduKind,
};
use aldis_core::ErrorKind;
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::Instant;
use aldis_tc::{ClcwStatus, ControlCommand, FrameType, TcVcId, validate_frame};
use aldis_testkit::strategies::common::frame_seq_num;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::tc::{control_command, tc_frame};
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/farm1_properties.txt"
);

const NOW: Instant = Instant::from_micros(0);

/// A window of Table 7-2: retransmission allowed, or not allowed with any PW.
fn window() -> impl Strategy<Value = Farm1Window> {
    prop_oneof![
        (1u16..=127).prop_map(|h| Farm1Window::retransmission_allowed(2 * h).unwrap()),
        (1u16..=256, any::<prop::sample::Index>()).prop_filter_map(
            "NW within Table 7-2",
            |(w, i)| {
                let pw = 1 + u16::try_from(i.index(usize::from(w))).unwrap();
                Farm1Window::retransmission_not_allowed(w, pw).ok()
            }
        ),
    ]
}

const fn config(window: Farm1Window) -> Farm1Config {
    Farm1Config {
        vc_id: TcVcId::new(2).unwrap(),
        status: ClcwStatus::new(1).unwrap(),
        window,
    }
}

/// One input, owned.
#[derive(Debug, Clone)]
enum Op {
    Ad(FrameSeqNum, bool, Vec<u8>),
    Bd(Vec<u8>),
    Bc(Option<ControlCommand>),
    Invalid,
    Release,
    Physical(bool, bool),
}

fn op(near: FrameSeqNum) -> impl Strategy<Value = Op> {
    // N(S) near V(R) most of the time, so that frames are accepted, and anywhere otherwise.
    let ns = prop_oneof![
        3 => (0u8..4).prop_map(move |d| FrameSeqNum::new(near.get().wrapping_add(d))),
        1 => frame_seq_num(),
    ];
    prop_oneof![
        6 => (ns, any::<bool>(), octets(0..=16)).prop_map(|(n, b, d)| Op::Ad(n, b, d)),
        2 => octets(0..=16).prop_map(Op::Bd),
        1 => control_command().prop_map(|c| Op::Bc(Some(c))),
        1 => Just(Op::Bc(None)),
        1 => Just(Op::Invalid),
        2 => Just(Op::Release),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(a, b)| Op::Physical(a, b)),
    ]
}

fn input(op: &Op) -> Farm1Input<'_> {
    match op {
        Op::Ad(n, buffer_available, data) => Farm1Input::Frame {
            vc_id: TcVcId::new(2).unwrap(),
            frame: Farm1Frame::Ad {
                sequence_number: *n,
                data,
            },
            buffer_available: *buffer_available,
        },
        Op::Bd(data) => Farm1Input::Frame {
            vc_id: TcVcId::new(2).unwrap(),
            frame: Farm1Frame::Bd { data },
            buffer_available: true,
        },
        Op::Bc(command) => Farm1Input::Frame {
            vc_id: TcVcId::new(2).unwrap(),
            frame: Farm1Frame::Bc { command: *command },
            buffer_available: true,
        },
        Op::Invalid => Farm1Input::InvalidFrame,
        Op::Release => Farm1Input::BufferReleased,
        Op::Physical(no_rf_available, no_bit_lock) => Farm1Input::SetPhysicalFlags {
            no_rf_available: *no_rf_available,
            no_bit_lock: *no_bit_lock,
        },
    }
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn any_input_sequence_keeps_the_invariants(
        (w, v_r, ops) in (window(), frame_seq_num()).prop_flat_map(|(w, v_r)| {
            (Just(w), Just(v_r), prop::collection::vec(op(v_r), 0..40))
        }),
    ) {
        // req: COP-002, COP-030, COP-033, COP-034, COP-035, API-057, API-058, GEN-020, VER-023
        let mut f = Farm1::new(config(w), v_r);
        for op in &ops {
            let mut out = [0u8; 16];
            let before = f;
            let result = f.step(input(op), NOW, &mut out);
            // The only error with an area of 16 octets is the "not applicable" cell S2 E1.
            let na = matches!(op, Op::Ad(n, true, _) if *n == before.v_r())
                && before.state() == Farm1State::Wait;
            let (after, actions) = match result {
                Ok(ok) => { prop_assert!(!na); ok }
                Err(e) => {
                    prop_assert!(na);
                    prop_assert_eq!(e.kind(), ErrorKind::InvalidState);
                    continue;
                }
            };
            prop_assert!(actions.len() <= FARM1_MAX_ACTIONS);
            // The pure function: the same state and input give the same result.
            let mut again = [0u8; 16];
            prop_assert_eq!(before.step(input(op), NOW, &mut again).unwrap(), (after, actions));
            prop_assert_eq!(out, again);

            // State and flags (232.1 §6.1.2 to §6.1.5, Table 6-1).
            match after.state() {
                Farm1State::Open => prop_assert!(!after.lockout_flag() && !after.wait_flag()),
                Farm1State::Wait => prop_assert!(after.wait_flag() && !after.lockout_flag()),
                Farm1State::Lockout => prop_assert!(after.lockout_flag()),
            }
            // V(R) moves only for an accepted Type-AD frame (+1) and a Set V(R) outside Lockout.
            let delivered = actions.iter().find_map(|a| match a {
                Farm1Output::Deliver { kind, sequence_number, data } => Some((*kind, *sequence_number, *data)),
                Farm1Output::Notify(_) => None,
            });
            let expected_vr = match (op, before.state()) {
                (Op::Ad(..), _) if delivered.is_some() => before.v_r().next(),
                (Op::Bc(Some(ControlCommand::SetVr(v))), Farm1State::Open | Farm1State::Wait) => *v,
                _ => before.v_r(),
            };
            prop_assert_eq!(after.v_r(), expected_vr);
            if let Some((kind, n_s, range)) = delivered {
                match op {
                    Op::Ad(n, true, data) => {
                        prop_assert_eq!((kind, n_s, *n), (FduKind::Ad, before.v_r(), before.v_r()));
                        prop_assert_eq!(range.slice(&out), Some(data.as_slice()));
                    }
                    Op::Bd(data) => {
                        prop_assert_eq!((kind, n_s.get()), (FduKind::Bd, 0));
                        prop_assert_eq!(range.slice(&out), Some(data.as_slice()));
                    }
                    _ => prop_assert!(false, "delivery for {op:?}"),
                }
            }
            // The FARM-B Counter counts valid Type-B frames (COP-034).
            let counted = matches!(op, Op::Bd(_) | Op::Bc(Some(_)));
            prop_assert_eq!(
                after.farm_b_counter().get(),
                (before.farm_b_counter().get() + u8::from(counted)) % 4
            );
            // The CLCW reports the state (COP-035).
            let c = after.clcw();
            prop_assert_eq!(
                (c.report_value, c.lockout, c.wait, c.retransmit, c.farm_b_counter),
                (after.v_r(), after.lockout_flag(), after.wait_flag(), after.retransmit_flag(), after.farm_b_counter())
            );
            f = after;
        }
    }

    #[test]
    fn window_classification_matches_the_inequalities(
        w in window(),
        v_r in frame_seq_num(),
        n_s in frame_seq_num(),
    ) {
        // req: COP-030, VER-023
        let f = Farm1::new(config(w), v_r);
        let data = [1u8, 2];
        let mut out = [0u8; 2];
        let ad = |buffer_available| Farm1Input::Frame { vc_id: TcVcId::new(2).unwrap(),
            frame: Farm1Frame::Ad { sequence_number: n_s, data: &data },
            buffer_available,
        };
        // 232.1 §6.1.8.3 on N(S) read in the circular sequence space around V(R).
        let ahead = (i32::from(n_s.get()) - i32::from(v_r.get())).rem_euclid(256);
        let (pw, nw) = (i32::from(w.positive()), i32::from(w.negative()));
        let (g, a) = f.step(ad(true), NOW, &mut out).unwrap();
        if ahead == 0 {
            prop_assert_eq!(g.v_r(), v_r.next());
            prop_assert_eq!(a.len(), 1);
            let (h, _) = f.step(ad(false), NOW, &mut out).unwrap();
            prop_assert_eq!((h.state(), h.wait_flag(), h.retransmit_flag()), (Farm1State::Wait, true, true));
        } else if ahead < pw {
            // N(S) <= V(R) + PW - 1
            prop_assert_eq!((g.state(), g.retransmit_flag(), g.v_r()), (Farm1State::Open, true, v_r));
        } else if 256 - ahead <= nw {
            prop_assert_eq!((g.state(), g.retransmit_flag(), g.v_r()), (Farm1State::Open, false, v_r));
        } else {
            prop_assert_eq!((g.state(), g.lockout_flag(), g.v_r()), (Farm1State::Lockout, true, v_r));
        }
    }

    #[test]
    fn tc_frames_map_to_farm1_frames(f in tc_frame()) {
        // req: COP-031, COP-002, VER-023
        let frame = validate_frame(&f.octets, &f.channels.table).unwrap();
        match (frame.frame_type(), Farm1Frame::from_tc(&frame)) {
            (FrameType::Ad, Farm1Frame::Ad { sequence_number, data }) => {
                prop_assert_eq!(sequence_number, frame.sequence_number());
                prop_assert_eq!(data, f.user_data.as_slice());
            }
            (FrameType::Bd, Farm1Frame::Bd { data }) => prop_assert_eq!(data, f.user_data.as_slice()),
            (FrameType::Bc, Farm1Frame::Bc { command }) => {
                prop_assert_eq!(command, f.control);
                prop_assert!(command.is_some());
            }
            (t, other) => prop_assert!(false, "{t:?} became {other:?}"),
        }
    }
}
