//! The contract of the FOP-1 and FARM-1 handles, called as a C caller calls them: storage and
//! tags (CABI-031, CABI-032), argument checks (CABI-022), the step call and the read-only calls
//! (CABI-040), and the capacities of the header (CABI-030). The engines' behaviour is checked by
//! the scenarios of `test-vectors/cop1/` (`vectors.rs`).
#![expect(clippy::unwrap_used, reason = "test-only harness (RS-018)")]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis_ffi::capacities::{ALDIS_FOP1_MAX_FRAME_OCTETS, ALDIS_FOP1_SENT_QUEUE_FRAMES};
use aldis_ffi::error::*;
use aldis_ffi::farm1::*;
use aldis_ffi::fop1::*;

/// A detail that a failed call must overwrite.
fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

use aldis_ffi::aldis_error_detail_t;

fn farm1_storage() -> Box<aldis_farm1_t> {
    Box::new(aldis_farm1_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_FARM1_WORDS],
    })
}

fn fop1_storage() -> Box<aldis_fop1_t> {
    Box::new(aldis_fop1_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_FOP1_WORDS],
    })
}

const fn farm1_config() -> aldis_farm1_config_t {
    aldis_farm1_config_t {
        vc_id: 1,
        status: 0,
        retransmission_allowed: 1,
        window: 8,
        positive_window: 0,
        v_r: 0,
    }
}

const fn fop1_config() -> aldis_fop1_config_t {
    aldis_fop1_config_t {
        scid: 42,
        vc_id: 1,
        fecf: 1,
        max_frame_len: 64,
        t1_initial_us: 1000,
        transmission_limit: 2,
        timeout_type: ALDIS_FOP1_TIMEOUT_TT0,
        sliding_window_k: 3,
        ad_repetitions: 1,
        bc_repetitions: 1,
    }
}

const fn farm1_ad(n_s: u8, data: &[u8]) -> aldis_farm1_input_t {
    aldis_farm1_input_t {
        kind: ALDIS_FARM1_INPUT_FRAME_AD,
        sequence_number: n_s,
        v_star: 0,
        buffer_available: 1,
        no_rf_available: 0,
        no_bit_lock: 0,
        data: data.as_ptr(),
        data_len: data.len(),
    }
}

const fn fop1_input(kind: u32) -> aldis_fop1_input_t {
    aldis_fop1_input_t {
        kind,
        handle: 7,
        clcw: 0,
        directive: 0,
        value: 0,
        frame_kind: 0,
        accepted: 0,
        data: null(),
        data_len: 0,
    }
}

#[test]
fn capacities_are_published_and_match_the_engines() {
    // req: CABI-030, COP-006
    assert_eq!(ALDIS_FARM1_MAX_ACTIONS, aldis::cop1::FARM1_MAX_ACTIONS);
    assert_eq!(
        ALDIS_FOP1_MAX_ACTIONS,
        aldis::cop1::fop1_max_actions(ALDIS_FOP1_SENT_QUEUE_FRAMES)
    );
    assert_eq!(ALDIS_FOP1_SENT_QUEUE_FRAMES, 16);
    assert_eq!(ALDIS_FOP1_MAX_FRAME_OCTETS, 1024);
    assert_eq!(size_of::<aldis_farm1_t>(), ALDIS_FARM1_WORDS * 8);
    assert_eq!(size_of::<aldis_fop1_t>(), ALDIS_FOP1_WORDS * 8);
    assert_eq!(align_of::<aldis_fop1_t>(), 8);
}

#[test]
fn storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = farm1_config();
    let mut d = detail();
    let mut s = farm1_storage();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_farm1_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_farm1_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_farm1_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        // An uninitialized object is garbage; the tag does not match.
        let mut status = aldis_farm1_status_t::default();
        assert_eq!(
            aldis_farm1_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_farm1_status(null_mut(), &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(aldis_farm1_init(&raw mut *s, &raw const cfg, null_mut()), 0);
        assert_eq!(
            aldis_farm1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!((status.state, status.v_r, status.clcw), (1, 0, 0x0104_0000));
        // A copy of live storage is foreign: the tag holds its own address.
        let mut copy = farm1_storage();
        copy.words = s.words;
        assert_eq!(
            aldis_farm1_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // Storage of another engine: the magic differs.
        let foreign = (&raw mut *s).cast::<aldis_fop1_t>();
        let mut deadline = (0_u8, 0_u64);
        assert_eq!(
            aldis_fop1_next_deadline(
                foreign,
                &raw mut deadline.0,
                &raw mut deadline.1,
                null_mut()
            ),
            ALDIS_ERR_INVALID_HANDLE
        );
        // `init` replaces whatever the storage holds, live or not.
        assert_eq!(aldis_farm1_init(&raw mut *s, &raw const cfg, null_mut()), 0);
        aldis_farm1_deinit(&raw mut *s);
        aldis_farm1_deinit(&raw mut *s);
        aldis_farm1_deinit(null_mut());
        assert_eq!(
            aldis_farm1_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn configurations_are_checked_once() {
    // req: CABI-022, COP-015, COP-030, API-040
    let mut s = farm1_storage();
    let mut d = detail();
    let farm = |f: fn(&mut aldis_farm1_config_t)| {
        let mut cfg = farm1_config();
        f(&mut cfg);
        cfg
    };
    // SAFETY: every pointer is to a live local.
    unsafe {
        for (cfg, want) in [
            (farm(|c| c.vc_id = 64), ALDIS_ERR_OUT_OF_RANGE),
            (farm(|c| c.status = 8), ALDIS_ERR_OUT_OF_RANGE),
            (farm(|c| c.window = 7), ALDIS_ERR_INVALID_CONFIG),
            (
                farm(|c| c.retransmission_allowed = 2),
                ALDIS_ERR_INVALID_FIELD,
            ),
            (
                farm(|c| {
                    c.retransmission_allowed = 0;
                    c.window = 4;
                    c.positive_window = 5;
                }),
                ALDIS_ERR_INVALID_CONFIG,
            ),
        ] {
            assert_eq!(
                aldis_farm1_init(&raw mut *s, &raw const cfg, &raw mut d),
                want
            );
            assert_eq!(d.kind, want);
        }
        let ok = farm(|c| {
            c.retransmission_allowed = 0;
            c.window = 4;
            c.positive_window = 3;
        });
        assert_eq!(aldis_farm1_init(&raw mut *s, &raw const ok, null_mut()), 0);
    }
    let mut s = fop1_storage();
    let fop = |f: fn(&mut aldis_fop1_config_t)| {
        let mut cfg = fop1_config();
        f(&mut cfg);
        cfg
    };
    // SAFETY: every pointer is to a live local.
    unsafe {
        for (cfg, want) in [
            (fop(|c| c.scid = 1024), ALDIS_ERR_OUT_OF_RANGE),
            (fop(|c| c.vc_id = 64), ALDIS_ERR_OUT_OF_RANGE),
            (fop(|c| c.fecf = 2), ALDIS_ERR_INVALID_FIELD),
            (fop(|c| c.max_frame_len = 1025), ALDIS_ERR_INVALID_CONFIG),
            (fop(|c| c.max_frame_len = 5), ALDIS_ERR_INVALID_CONFIG),
            (fop(|c| c.t1_initial_us = 0), ALDIS_ERR_INVALID_CONFIG),
            (fop(|c| c.transmission_limit = 0), ALDIS_ERR_INVALID_CONFIG),
            (fop(|c| c.timeout_type = 2), ALDIS_ERR_INVALID_FIELD),
            (fop(|c| c.sliding_window_k = 0), ALDIS_ERR_INVALID_CONFIG),
            (fop(|c| c.sliding_window_k = 17), ALDIS_ERR_INVALID_CONFIG),
        ] {
            assert_eq!(
                aldis_fop1_init(&raw mut *s, &raw const cfg, &raw mut d),
                want,
                "{cfg:?}"
            );
            assert_eq!(d.kind, want);
        }
        let ok = fop1_config();
        assert_eq!(aldis_fop1_init(&raw mut *s, &raw const ok, null_mut()), 0);
        aldis_fop1_deinit(&raw mut *s);
    }
}

#[test]
fn a_farm1_step_returns_its_actions_and_leaves_the_state_on_error() {
    // req: CABI-040, COP-002, COP-006, API-058, API-061
    let mut s = farm1_storage();
    let cfg = farm1_config();
    let mut d = detail();
    let mut out = [0_u8; 8];
    let data = [9_u8, 8, 7];
    let mut actions = aldis_farm1_actions_t::default();
    // SAFETY: every pointer is to a live local; `data` is valid for its length.
    unsafe {
        assert_eq!(aldis_farm1_init(&raw mut *s, &raw const cfg, null_mut()), 0);
        // E1: Accept, V(R) := 1.
        let input = farm1_ad(0, &data);
        assert_eq!(
            aldis_farm1_step(
                &raw mut *s,
                &raw const input,
                5,
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                null_mut()
            ),
            0
        );
        assert_eq!(actions.count, 1);
        let a = actions.items[0];
        assert_eq!(
            (a.kind, a.fdu_kind, a.sequence_number),
            (ALDIS_FARM1_ACTION_DELIVER, ALDIS_FARM1_FDU_AD, 0)
        );
        assert_eq!((a.data.offset, a.data.len), (0, 3));
        assert_eq!(out[..3], data);
        assert!(actions.items[1..].iter().all(|x| *x == Default::default()));
        // A buffer too small: nothing written, the state unchanged.
        actions = aldis_farm1_actions_t {
            count: 77,
            ..Default::default()
        };
        let input = farm1_ad(1, &data);
        assert_eq!(
            aldis_farm1_step(
                &raw mut *s,
                &raw const input,
                6,
                out.as_mut_ptr(),
                2,
                &raw mut actions,
                &raw mut d
            ),
            ALDIS_ERR_BUFFER_TOO_SMALL
        );
        assert_eq!((d.kind, d.needed), (ALDIS_ERR_BUFFER_TOO_SMALL, 3));
        assert_eq!(actions.count, 77);
        let mut status = aldis_farm1_status_t::default();
        assert_eq!(
            aldis_farm1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status.v_r, 1);
        // The same frame with a buffer: accepted.
        assert_eq!(
            aldis_farm1_step(
                &raw mut *s,
                &raw const input,
                6,
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                null_mut()
            ),
            0
        );
        assert_eq!(actions.count, 1);
        // Set the physical flags: no action, in the CLCW.
        let flags = aldis_farm1_input_t {
            kind: ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS,
            no_rf_available: 1,
            ..farm1_ad(0, &[])
        };
        assert_eq!(
            aldis_farm1_step(
                &raw mut *s,
                &raw const flags,
                7,
                null_mut(),
                0,
                &raw mut actions,
                null_mut()
            ),
            0
        );
        assert_eq!(actions.count, 0);
        assert_eq!(
            aldis_farm1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status.clcw, 0x0104_8002);
    }
}

#[test]
fn a_farm1_step_checks_every_argument() {
    // req: CABI-022, CABI-040, API-058
    let mut s = farm1_storage();
    let cfg = farm1_config();
    let mut out = [0_u8; 8];
    let data = [1_u8, 2];
    let mut actions = aldis_farm1_actions_t::default();
    let mut d = detail();
    // SAFETY: every pointer is NULL, to a live local, or (the overlap cases) into live storage
    // that is rejected before any access through it.
    unsafe {
        assert_eq!(aldis_farm1_init(&raw mut *s, &raw const cfg, null_mut()), 0);
        let good = farm1_ad(0, &data);
        let call = |s: &mut aldis_farm1_t,
                    input: *const aldis_farm1_input_t,
                    out: *mut u8,
                    cap: usize,
                    actions: *mut aldis_farm1_actions_t| {
            aldis_farm1_step(s, input, 0, out, cap, actions, null_mut())
        };
        let (o, n) = (out.as_mut_ptr(), out.len());
        let a = &raw mut actions;
        assert_eq!(call(&mut s, null(), o, n, a), ALDIS_ERR_INVALID_FIELD);
        assert_eq!(
            call(&mut s, &raw const good, o, n, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            call(&mut s, &raw const good, null_mut(), 1, a),
            ALDIS_ERR_INVALID_FIELD
        );
        // Enumeration with no constant, flag that is not 0 or 1.
        for bad in [
            aldis_farm1_input_t { kind: 99, ..good },
            aldis_farm1_input_t {
                buffer_available: 2,
                ..good
            },
            aldis_farm1_input_t {
                kind: ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS,
                no_bit_lock: 3,
                ..good
            },
            aldis_farm1_input_t {
                data: null(),
                data_len: 1,
                ..good
            },
        ] {
            assert_eq!(
                call(&mut s, &raw const bad, o, n, a),
                ALDIS_ERR_INVALID_FIELD
            );
        }
        // An input buffer inside the engine's storage would alias the state (c-abi.md §12.2).
        let inside = (&raw const *s).cast::<u8>().wrapping_add(16);
        let aliased = aldis_farm1_input_t {
            data: inside,
            data_len: 2,
            ..good
        };
        assert_eq!(
            call(&mut s, &raw const aliased, o, n, a),
            ALDIS_ERR_INVALID_FIELD
        );
        // The output buffer or the actions inside the storage, the actions in the output.
        let inside_mut = (&raw mut *s).cast::<u8>().wrapping_add(16);
        assert_eq!(
            call(&mut s, &raw const good, inside_mut, 4, a),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            call(&mut s, &raw const good, o, n, inside_mut.cast()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            call(&mut s, &raw const good, a.cast::<u8>(), 8, a),
            ALDIS_ERR_INVALID_FIELD
        );
        // The output buffer overlapping the input buffer.
        let both = aldis_farm1_input_t {
            data: o.cast_const(),
            data_len: 4,
            ..good
        };
        assert_eq!(
            call(&mut s, &raw const both, o, n, a),
            ALDIS_ERR_INVALID_FIELD
        );
        // A misaligned detail fails before anything happens.
        let odd = (&raw mut d).wrapping_byte_add(1);
        assert_eq!(
            aldis_farm1_step(&mut *s, &raw const good, 0, o, n, a, odd),
            ALDIS_ERR_INVALID_FIELD
        );
        // None of it changed the engine.
        let mut status = aldis_farm1_status_t::default();
        assert_eq!(
            aldis_farm1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status.v_r, 0);
        assert_eq!(
            aldis_farm1_status(&raw mut *s, null_mut(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        // The one cell that is not applicable: S2, E1.
        let wait = aldis_farm1_input_t {
            buffer_available: 0,
            ..good
        };
        assert_eq!(call(&mut s, &raw const wait, o, n, a), 0);
        assert_eq!(
            aldis_farm1_step(&mut *s, &raw const good, 0, o, n, a, &raw mut d),
            ALDIS_ERR_INVALID_STATE
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_STATE);
    }
}

#[test]
fn a_fop1_run_from_initial_to_a_retransmission() {
    // req: CABI-040, COP-001, COP-010, COP-011, COP-013, API-053, API-061
    let mut s = fop1_storage();
    let cfg = fop1_config();
    let mut out = [0_u8; 64];
    let mut actions = aldis_fop1_actions_t::default();
    let mut d = detail();
    let data = [0xDE_u8, 0xAD];
    // SAFETY: every pointer is to a live local; `data` is valid for its length.
    unsafe {
        let step = |s: &mut aldis_fop1_t,
                    input: &aldis_fop1_input_t,
                    now: u64,
                    out: &mut [u8],
                    actions: *mut aldis_fop1_actions_t| {
            aldis_fop1_step(
                s,
                input,
                now,
                out.as_mut_ptr(),
                out.len(),
                actions,
                null_mut(),
            )
        };
        assert_eq!(aldis_fop1_init(&raw mut *s, &raw const cfg, null_mut()), 0);
        let mut status = aldis_fop1_status_t::default();
        assert_eq!(
            aldis_fop1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(
            (status.state, status.sliding_window_k, status.sent_queue_len),
            (6, 3, 0)
        );
        assert_eq!(status.timeout_type, ALDIS_FOP1_TIMEOUT_TT0);
        assert_eq!(status.t1_initial_us, 1000);
        // E23: Initiate AD without CLCW check.
        let initiate = aldis_fop1_input_t {
            directive: ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK,
            ..fop1_input(ALDIS_FOP1_INPUT_DIRECTIVE)
        };
        assert_eq!(step(&mut s, &initiate, 10, &mut out, &raw mut actions), 0);
        assert_eq!(
            aldis_fop1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status.state, 1);
        // E19: a request is transmitted and the timer starts.
        let request = aldis_fop1_input_t {
            data: data.as_ptr(),
            data_len: data.len(),
            ..fop1_input(ALDIS_FOP1_INPUT_REQUEST_AD)
        };
        assert_eq!(step(&mut s, &request, 20, &mut out, &raw mut actions), 0);
        let transmit = actions
            .items
            .iter()
            .take(actions.count)
            .find(|a| a.kind == ALDIS_FOP1_ACTION_TRANSMIT)
            .unwrap();
        assert_eq!(
            (
                transmit.frame_kind,
                transmit.sequence_number,
                transmit.retransmission
            ),
            (ALDIS_FOP1_FRAME_AD, 0, 0)
        );
        // Header (5), data (2), FECF (2).
        assert_eq!((transmit.frame.offset, transmit.frame.len), (0, 9));
        assert_eq!(out[5..7], data);
        let (mut armed, mut at) = (0_u8, 0_u64);
        assert_eq!(
            aldis_fop1_next_deadline(&raw mut *s, &raw mut armed, &raw mut at, null_mut()),
            0
        );
        assert_eq!((armed, at), (1, 1020));
        // The lower procedure accepts the frame, the timer runs out: a retransmission.
        let done = aldis_fop1_input_t {
            frame_kind: ALDIS_FOP1_FRAME_AD,
            accepted: 1,
            ..fop1_input(ALDIS_FOP1_INPUT_TRANSMIT_DONE)
        };
        assert_eq!(step(&mut s, &done, 30, &mut out, &raw mut actions), 0);
        out.fill(0);
        let timeout = fop1_input(ALDIS_FOP1_INPUT_TIMEOUT);
        assert_eq!(step(&mut s, &timeout, 1019, &mut out, &raw mut actions), 0);
        assert_eq!(actions.count, 0);
        assert_eq!(step(&mut s, &timeout, 1020, &mut out, &raw mut actions), 0);
        assert!(
            actions
                .items
                .iter()
                .take(actions.count)
                .any(|a| a.kind == ALDIS_FOP1_ACTION_TRANSMIT && a.retransmission == 1)
        );
        assert_eq!(out[5..7], data);
        // A failed call writes no actions and keeps the state.
        actions.count = 77;
        let empty = fop1_input(ALDIS_FOP1_INPUT_REQUEST_AD);
        assert_eq!(
            aldis_fop1_step(
                &raw mut *s,
                &raw const empty,
                1021,
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!((d.kind, actions.count), (ALDIS_ERR_INVALID_FIELD, 77));
        aldis_fop1_deinit(&raw mut *s);
        assert_eq!(
            aldis_fop1_step(
                &raw mut *s,
                &raw const empty,
                1022,
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                null_mut()
            ),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn a_fop1_step_checks_every_argument() {
    // req: CABI-022, CABI-040, COP-015, API-058
    let mut s = fop1_storage();
    let cfg = fop1_config();
    let mut out = [0_u8; 64];
    let mut actions = aldis_fop1_actions_t::default();
    let data = [1_u8, 2, 3];
    let ap = &raw mut actions;
    // SAFETY: every pointer is NULL, to a live local, or (the overlap cases) into live storage
    // that is rejected before any access through it.
    unsafe {
        assert_eq!(aldis_fop1_init(&raw mut *s, &raw const cfg, null_mut()), 0);
        let call = |s: &mut aldis_fop1_t, input: *const aldis_fop1_input_t, out: &mut [u8]| {
            aldis_fop1_step(s, input, 0, out.as_mut_ptr(), out.len(), ap, null_mut())
        };
        let request = aldis_fop1_input_t {
            data: data.as_ptr(),
            data_len: data.len(),
            ..fop1_input(ALDIS_FOP1_INPUT_REQUEST_AD)
        };
        assert_eq!(call(&mut s, null(), &mut out), ALDIS_ERR_INVALID_FIELD);
        for (bad, want) in [
            (fop1_input(99), ALDIS_ERR_INVALID_FIELD),
            (
                aldis_fop1_input_t {
                    directive: 99,
                    ..fop1_input(ALDIS_FOP1_INPUT_DIRECTIVE)
                },
                ALDIS_ERR_INVALID_FIELD,
            ),
            (
                aldis_fop1_input_t {
                    frame_kind: 99,
                    ..fop1_input(ALDIS_FOP1_INPUT_TRANSMIT_DONE)
                },
                ALDIS_ERR_INVALID_FIELD,
            ),
            (
                aldis_fop1_input_t {
                    accepted: 2,
                    ..fop1_input(ALDIS_FOP1_INPUT_TRANSMIT_DONE)
                },
                ALDIS_ERR_INVALID_FIELD,
            ),
            // Control Word Type 0 but a version that is not 0: no CLCW to decode.
            (
                aldis_fop1_input_t {
                    clcw: 0x4000_0000,
                    ..fop1_input(ALDIS_FOP1_INPUT_CLCW)
                },
                ALDIS_ERR_UNSUPPORTED_VERSION,
            ),
            (
                aldis_fop1_input_t {
                    data: null(),
                    data_len: 1,
                    ..request
                },
                ALDIS_ERR_INVALID_FIELD,
            ),
        ] {
            assert_eq!(call(&mut s, &raw const bad, &mut out), want, "{bad:?}");
        }
        // An input buffer inside the storage, and an output buffer inside it.
        let inside = (&raw const *s).cast::<u8>().wrapping_add(64);
        let aliased = aldis_fop1_input_t {
            data: inside,
            data_len: 2,
            ..request
        };
        assert_eq!(
            call(&mut s, &raw const aliased, &mut out),
            ALDIS_ERR_INVALID_FIELD
        );
        let inside_mut = (&raw mut *s).cast::<u8>().wrapping_add(64);
        assert_eq!(
            aldis_fop1_step(
                &mut *s,
                &raw const request,
                0,
                inside_mut,
                8,
                ap,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // A CLCW of another Virtual Channel, and the one of Control Word Type 1 (E15).
        let other = aldis_fop1_input_t {
            clcw: u32::from_be_bytes([0x01, 9 << 2, 0, 0]),
            ..fop1_input(ALDIS_FOP1_INPUT_CLCW)
        };
        assert_eq!(
            call(&mut s, &raw const other, &mut out),
            ALDIS_ERR_UNKNOWN_CHANNEL
        );
        let own = u32::from_be_bytes([0x01, 1 << 2, 0, 0]);
        let ok = aldis_fop1_input_t {
            clcw: own,
            ..fop1_input(ALDIS_FOP1_INPUT_CLCW)
        };
        assert_eq!(call(&mut s, &raw const ok, &mut out), 0);
        let invalid = aldis_fop1_input_t {
            clcw: own | 0x8000_0000,
            ..fop1_input(ALDIS_FOP1_INPUT_CLCW)
        };
        assert_eq!(call(&mut s, &raw const invalid, &mut out), 0);
        // A directive value that does not fit is E40, a notification and no error.
        let too_big = aldis_fop1_input_t {
            directive: ALDIS_FOP1_DIRECTIVE_SET_K,
            value: 300,
            ..fop1_input(ALDIS_FOP1_INPUT_DIRECTIVE)
        };
        assert_eq!(call(&mut s, &raw const too_big, &mut out), 0);
        assert_eq!(actions.count, 1);
        let notice = actions.items[0];
        assert_eq!(
            (notice.kind, notice.handle, notice.status),
            (
                ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY,
                7,
                ALDIS_FOP1_NOTICE_REJECT
            )
        );
        let unparsed = fop1_input(ALDIS_FOP1_INPUT_INVALID_DIRECTIVE);
        assert_eq!(call(&mut s, &raw const unparsed, &mut out), 0);
        assert_eq!(actions.items[0].status, ALDIS_FOP1_NOTICE_REJECT);
        // A frame buffer shorter than the frame.
        let initiate = aldis_fop1_input_t {
            directive: ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK,
            ..fop1_input(ALDIS_FOP1_INPUT_DIRECTIVE)
        };
        assert_eq!(call(&mut s, &raw const initiate, &mut out), 0);
        assert_eq!(
            call(&mut s, &raw const request, &mut out[..4]),
            ALDIS_ERR_BUFFER_TOO_SMALL
        );
        // The state survived all of it.
        let mut status = aldis_fop1_status_t::default();
        assert_eq!(
            aldis_fop1_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!((status.state, status.sent_queue_len), (1, 0));
        assert_eq!(
            aldis_fop1_status(&raw mut *s, null_mut(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        let mut armed = 0_u8;
        assert_eq!(
            aldis_fop1_next_deadline(&raw mut *s, &raw mut armed, null_mut(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}
