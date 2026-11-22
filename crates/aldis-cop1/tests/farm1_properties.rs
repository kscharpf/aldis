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
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::Instant;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_tc::{
    ClcwStatus, ControlCommand, FrameType, PhysicalChannelConfig, SegmentHeaderUse, TcSpacecraftId,
    TcVcId, VcConfig, VcTable, check_frame,
};
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
        6 => (ns, any::<bool>(), octets(1..=16)).prop_map(|(n, b, d)| Op::Ad(n, b, d)),
        2 => octets(1..=16).prop_map(Op::Bd),
        1 => control_command().prop_map(|c| Op::Bc(Some(c))),
        1 => Just(Op::Bc(None)),
        1 => Just(Op::Invalid),
        2 => Just(Op::Release),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(a, b)| Op::Physical(a, b)),
    ]
}

fn tc_table(vc_id: TcVcId) -> VcTable<1, 0> {
    let scid = TcSpacecraftId::new(0).unwrap();
    let pc = PhysicalChannelConfig::new(1024, false).unwrap();
    let vc = VcConfig::new(
        scid,
        vc_id,
        1024,
        SegmentHeaderUse::Absent(SecurityLayout::NONE),
    )
    .unwrap();
    VcTable::new(pc, &[vc], &[]).unwrap()
}

fn build_tc(frame_type: FrameType, vc_id: TcVcId, n_s: u8, payload: &[u8]) -> Vec<u8> {
    let flags = match frame_type {
        FrameType::Ad => 0x00,
        FrameType::Bd => 0x20,
        FrameType::Bc => 0x30,
    };
    let len_minus_1 = u16::try_from(5 + payload.len() - 1).unwrap();
    let vc_len = (u16::from(vc_id.get()) << 10) | len_minus_1;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(flags);
    buf.push(0);
    buf.extend_from_slice(&vc_len.to_be_bytes());
    buf.push(n_s);
    buf.extend_from_slice(payload);
    buf
}

fn with_input<R>(op: &Op, f: impl FnOnce(Farm1Input<'_>) -> R) -> R {
    let vc = TcVcId::new(2).unwrap();
    let table = tc_table(vc);
    match op {
        Op::Ad(n, buffer_available, data) => {
            let wire = build_tc(FrameType::Ad, vc, n.get(), data);
            let checked = check_frame(&wire, &table, DecodeMode::Lenient).unwrap();
            f(Farm1Input::from_tc(&checked, *buffer_available))
        }
        Op::Bd(data) => {
            let wire = build_tc(FrameType::Bd, vc, 0, data);
            let checked = check_frame(&wire, &table, DecodeMode::Lenient).unwrap();
            f(Farm1Input::from_tc(&checked, true))
        }
        Op::Bc(command) => {
            let cmd = command.map(ControlCommand::to_octets);
            let payload = cmd.as_ref().map_or(&[0xFF][..], |c| c.as_slice());
            let wire = build_tc(FrameType::Bc, vc, 0, payload);
            let checked = check_frame(&wire, &table, DecodeMode::Lenient).unwrap();
            f(Farm1Input::from_tc(&checked, true))
        }
        Op::Invalid => f(Farm1Input::InvalidFrame),
        Op::Release => f(Farm1Input::BufferReleased),
        Op::Physical(no_rf_available, no_bit_lock) => f(Farm1Input::SetPhysicalFlags {
            no_rf_available: *no_rf_available,
            no_bit_lock: *no_bit_lock,
        }),
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
            let result = with_input(op, |inp| f.step(inp, NOW, &mut out));
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
            prop_assert_eq!(with_input(op, |inp| before.step(inp, NOW, &mut again)).unwrap(), (after, actions));
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
                    Op::Bc(_) | Op::Invalid | Op::Release | Op::Physical(..) | Op::Ad(..) => {
                        prop_assert!(false, "delivery for {op:?}");
                    }
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
        let vc = TcVcId::new(2).unwrap();
        let table = tc_table(vc);
        let wire = build_tc(FrameType::Ad, vc, n_s.get(), &data);
        let checked = check_frame(&wire, &table, DecodeMode::Strict).unwrap();
        let ad = |buffer_available| Farm1Input::from_tc(&checked, buffer_available);
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
    fn a_tc_frame_is_refused_by_the_farm1_of_another_vc(f in tc_frame(), w in window()) {
        // req: COP-037, COP-002, API-058, VER-023
        let frame = check_frame(&f.octets, &f.channels.table, DecodeMode::Strict).unwrap();
        let input = Farm1Input::from_tc(&frame, true);
        let vc = frame.as_frame().vc_id();
        let mut out = [0u8; 4096];
        let other = TcVcId::new((vc.get() + 1) % 64).unwrap();
        let farm = |vc_id| Farm1::new(Farm1Config { vc_id, ..config(w) }, FrameSeqNum::new(0));
        let err = farm(other).step(input, NOW, &mut out).unwrap_err();
        prop_assert_eq!(err.kind(), ErrorKind::UnknownChannel);
        prop_assert!(out.iter().all(|b| *b == 0));
        // Its own channel's FARM-1 does not refuse it for the channel.
        let own = farm(vc).step(input, NOW, &mut out);
        prop_assert!(own.map_or_else(|e| e.kind() != ErrorKind::UnknownChannel, |_| true));
    }

    #[test]
    fn tc_frames_map_to_farm1_frames(f in tc_frame()) {
        // req: COP-031, COP-002, VER-023
        let frame = check_frame(&f.octets, &f.channels.table, DecodeMode::Strict).unwrap();
        let mapped = Farm1Frame::from_tc(&frame);
        prop_assert_eq!(mapped.kind(), frame.frame_type());
        match frame.frame_type() {
            FrameType::Ad => {
                prop_assert_eq!(mapped.sequence_number(), Some(frame.sequence_number()));
                prop_assert_eq!(mapped.data(), Some(f.user_data.as_slice()));
                prop_assert_eq!(mapped.control_command(), None);
            }
            FrameType::Bd => {
                prop_assert_eq!(mapped.sequence_number(), None);
                prop_assert_eq!(mapped.data(), Some(f.user_data.as_slice()));
                prop_assert_eq!(mapped.control_command(), None);
            }
            FrameType::Bc => {
                prop_assert_eq!(mapped.sequence_number(), None);
                prop_assert_eq!(mapped.data(), None);
                prop_assert_eq!(mapped.control_command(), f.control);
                prop_assert!(mapped.control_command().is_some());
            }
        }
    }
}
