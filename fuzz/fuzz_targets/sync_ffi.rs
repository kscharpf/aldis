//! The C ABI of `aldis-sync` against the Rust crate (VER-022, SYNC-003, SYNC-004, SYNC-007,
//! SYNC-009…SYNC-011, SYNC-014, SYNC-015, SYNC-018, CABI-031, CABI-040, API-058, GEN-020): for
//! an arbitrary configuration, stream, division into chunks, and output area, the search handle
//! returns the status, the actions, and the status snapshot of the Rust engine at every step, a
//! refused step changes neither, and `aldis_sync_attach` and `aldis_sync_randomize` write what
//! `attach_asm` and `randomize` write.
#![no_main]
#![expect(unsafe_code, reason = "the target calls the C ABI")]

// req: VER-022, SYNC-003, SYNC-004, SYNC-007, SYNC-009, SYNC-010, SYNC-011, SYNC-014, SYNC-015, SYNC-018, CABI-031, CABI-040, API-058, GEN-020

use core::ptr::null_mut;

use aldis_core::ErrorKind;
use aldis_ffi::capacities::{ALDIS_SYNC_MAX_CHUNK_OCTETS, ALDIS_SYNC_MAX_FRAME_OCTETS};
use aldis_ffi::error::{ALDIS_ERR_BUFFER_TOO_SMALL, ALDIS_ERR_OUT_OF_RANGE};
use aldis_ffi::link::{ALDIS_LINK_PROTOCOL_AOS, ALDIS_LINK_PROTOCOL_TM, ALDIS_LINK_PROTOCOL_USLP};
use aldis_ffi::sync::{
    ALDIS_SYNC_RANDOMIZER_255, ALDIS_SYNC_RANDOMIZER_131071, ALDIS_SYNC_SEARCH_INPUT_CHUNK,
    ALDIS_SYNC_SEARCH_INPUT_RECONFIGURE, ALDIS_SYNC_SEARCH_INPUT_RESET,
    ALDIS_SYNC_SEARCH_MAX_ACTIONS, ALDIS_SYNC_SEARCH_STATE_COLLECTING, ALDIS_SYNC_SEARCH_WORDS,
    aldis_sync_attach, aldis_sync_randomize, aldis_sync_search_action_t,
    aldis_sync_search_actions_t, aldis_sync_search_config_t, aldis_sync_search_deinit,
    aldis_sync_search_init, aldis_sync_search_input_t, aldis_sync_search_status,
    aldis_sync_search_status_t, aldis_sync_search_step, aldis_sync_search_t,
};
use aldis_ffi::{ALDIS_OK, aldis_error_detail_t};
use aldis_sync::{
    AsmErrors, AsmSearch, RandomizerSequence, SearchInput, SearchState, SyncConfig, SyncProtocol,
    attach_asm, max_actions, randomize,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const FRAME: usize = ALDIS_SYNC_MAX_FRAME_OCTETS;
const CHUNK: usize = ALDIS_SYNC_MAX_CHUNK_OCTETS;
type Engine = AsmSearch<FRAME, CHUNK, ALDIS_SYNC_SEARCH_MAX_ACTIONS>;

const _: () = assert!(ALDIS_SYNC_SEARCH_MAX_ACTIONS == max_actions(CHUNK));

/// The usual cap on the frame length this target draws, for the initial configuration and every
/// reconfiguration. A Reset or reconfiguration is drawn on about one step in eight, and a frame
/// of up to 65536 octets on each made the target slow. Measured over 60 s: about 33 runs/s with
/// that draw, about 590 runs/s with the cap of 512 alone, about 51 runs/s with a full-length
/// draw on 1 configuration in 64, and about 91 runs/s on 1 in 1024. The remaining cost is not
/// explained; untested candidates are the per-run init or reconfigure of a handle built at the
/// 65536-octet capacity and the per-step output buffers. About one configuration in 1024 draws a
/// length up to the protocol's real maximum (TM 2048, AOS and USLP 65536), kept so that the
/// target still reaches the capacity of the C handle. The `sync_search` target uses a small
/// instance (`FRAME` of 64) and does not cover those lengths; the SYNC-014 largest-frame test of
/// `crates/aldis-ffi/tests/sync.rs` covers one fixed case.
const LONG_FRAME_CAP: usize = 512;

fn draw_config(u: &mut Unstructured<'_>) -> Result<(u32, aldis_sync_search_config_t, SyncConfig)> {
    let (raw, protocol, max) = match u.int_in_range(0..=2)? {
        0 => (ALDIS_LINK_PROTOCOL_TM, SyncProtocol::Tm, 2048),
        1 => (ALDIS_LINK_PROTOCOL_AOS, SyncProtocol::Aos, 65536),
        _ => (ALDIS_LINK_PROTOCOL_USLP, SyncProtocol::Uslp, 65536),
    };
    // Mostly short frames, so that units complete inside a chunk; sometimes one up to
    // `LONG_FRAME_CAP`; rarely one up to the protocol's maximum.
    let len = if u.ratio(1, 1024)? {
        u.int_in_range(1..=max)?
    } else if u.ratio(1, 8)? {
        u.int_in_range(1..=LONG_FRAME_CAP)?
    } else {
        u.int_in_range(1..=40)?
    };
    let tolerance = u.int_in_range(0..=15)?;
    let c = aldis_sync_search_config_t {
        protocol: raw,
        frame_len: len,
        asm_errors: tolerance,
    };
    let rust = SyncConfig::new(protocol, len, AsmErrors::new(tolerance).unwrap()).unwrap();
    Ok((raw, c, rust))
}

fn empty_actions() -> aldis_sync_search_actions_t {
    aldis_sync_search_actions_t {
        count: 0,
        items: [aldis_sync_search_action_t::default(); ALDIS_SYNC_SEARCH_MAX_ACTIONS],
    }
}

fn snapshot(h: &mut aldis_sync_search_t) -> aldis_sync_search_status_t {
    let mut s = aldis_sync_search_status_t::default();
    // SAFETY: `h` holds a live engine and `s` is a live local; no pointer overlaps another.
    let code = unsafe { aldis_sync_search_status(h, &raw mut s, null_mut()) };
    assert_eq!(code, ALDIS_OK);
    s
}

fn same_status(h: &mut aldis_sync_search_t, rust: &Engine) {
    let c = snapshot(h);
    let r = rust.status();
    assert_eq!(c.units_returned, r.units_returned());
    assert_eq!(c.skipped_bits_total, r.skipped_bits_total());
    assert_eq!(c.partial_octets, r.partial_octets());
    assert_eq!(
        c.state == ALDIS_SYNC_SEARCH_STATE_COLLECTING,
        r.state() == SearchState::Collecting
    );
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok((_, mut c_config, mut config)) = draw_config(&mut u) else {
        return;
    };

    // The codec functions: the CADU and the randomizer.
    let frame_len = config.frame_len();
    let frame: Vec<u8> = (0..frame_len)
        .map(|_| u.arbitrary::<u8>().unwrap_or(0))
        .collect();
    let mut want = vec![0_u8; 4 + frame_len];
    assert_eq!(attach_asm(&config, &frame, &mut want), Ok(4 + frame_len));
    let mut got = vec![0_u8; 4 + frame_len];
    let mut got_len = 0_usize;
    // SAFETY: every pointer is to a live local or vector of the stated length.
    let code = unsafe {
        aldis_sync_attach(
            &raw const c_config,
            frame.as_ptr(),
            frame.len(),
            got.as_mut_ptr(),
            got.len(),
            &raw mut got_len,
            null_mut(),
        )
    };
    assert_eq!(code, ALDIS_OK);
    assert_eq!((got_len, &got), (4 + frame_len, &want));
    for (raw, sequence) in [
        (
            ALDIS_SYNC_RANDOMIZER_131071,
            RandomizerSequence::Period131071,
        ),
        (ALDIS_SYNC_RANDOMIZER_255, RandomizerSequence::Period255),
    ] {
        let mut a = frame.clone();
        let mut b = frame.clone();
        randomize(sequence, &mut a);
        // SAFETY: `b` is a live vector of the stated length.
        let code = unsafe { aldis_sync_randomize(raw, b.as_mut_ptr(), b.len(), null_mut()) };
        assert_eq!(code, ALDIS_OK);
        assert_eq!(a, b);
    }

    // The handle against the engine.
    let mut rust = Box::new(Engine::new(config).unwrap());
    let mut storage = Box::new(aldis_sync_search_t {
        words: [0; ALDIS_SYNC_SEARCH_WORDS],
    });
    // SAFETY: `storage` is a live, aligned `aldis_sync_search_t` and `c_config` a live local.
    let code =
        unsafe { aldis_sync_search_init(&raw mut *storage, &raw const c_config, null_mut()) };
    assert_eq!(code, ALDIS_OK);
    same_status(&mut storage, &rust);

    let Ok(stream_len) = u.int_in_range(0..=1200) else {
        return;
    };
    let stream: Vec<u8> = (0..stream_len)
        .map(|_| u.arbitrary::<u8>().unwrap_or(0))
        .collect();
    // Half of the streams carry a CADU of the configured length at a drawn bit offset, so that
    // units are returned.
    let mut stream = stream;
    if u.ratio(1, 2).unwrap_or(false) {
        let mut cadu = vec![0_u8; 4 + frame_len];
        let _ = attach_asm(&config, &frame, &mut cadu);
        let shift = u.int_in_range(0..=7).unwrap_or(0_u32);
        let mut shifted = vec![0_u8; cadu.len() + 1];
        for (i, &b) in cadu.iter().enumerate() {
            shifted[i] |= b >> shift;
            shifted[i + 1] |= b.checked_shl(8 - shift).unwrap_or(0);
        }
        stream.extend_from_slice(&shifted);
    }

    let mut at = 0;
    let mut last_empty = false;
    while at < stream.len() {
        // Now and then a Reset or a reconfiguration, on both sides (SYNC-015).
        if u.ratio(1, 8).unwrap_or(false) {
            let reconfigure = u.ratio(1, 2).unwrap_or(false);
            let mut kind = ALDIS_SYNC_SEARCH_INPUT_RESET;
            let mut new = None;
            if reconfigure && let Ok((_, c, r)) = draw_config(&mut u) {
                kind = ALDIS_SYNC_SEARCH_INPUT_RECONFIGURE;
                new = Some((c, r));
            }
            let rust_reset = rust.step(
                SearchInput::Reset {
                    config: new.map(|(_, r)| r),
                },
                &mut [],
            );
            let input = aldis_sync_search_input_t {
                kind,
                data: core::ptr::null(),
                data_len: 0,
                config: new.map_or(c_config, |(c, _)| c),
            };
            let mut actions = empty_actions();
            // SAFETY: the storage holds a live engine; `input` and `actions` are live locals;
            // a null area has length 0.
            let code = unsafe {
                aldis_sync_search_step(
                    &raw mut *storage,
                    &raw const input,
                    null_mut(),
                    0,
                    &raw mut actions,
                    null_mut(),
                )
            };
            assert_eq!(code, ALDIS_OK);
            assert_eq!(rust_reset.unwrap().len(), 0);
            assert_eq!(actions.count, 0);
            if let Some((c, r)) = new {
                c_config = c;
                config = r;
            }
            same_status(&mut storage, &rust);
        }
        let frame_len = config.frame_len();
        let mut want_len = u.int_in_range(0..=CHUNK + 20).unwrap_or(CHUNK);
        if last_empty {
            want_len = want_len.max(1);
        }
        last_empty = want_len == 0;
        let end = stream.len().min(at + want_len);
        let chunk = &stream[at..end];
        let area_len = u
            .int_in_range(0..=Engine::MAX_OUT_LEN.min(4 * frame_len + 8))
            .unwrap_or(0);

        let before_rust = rust.status();
        let before_c = snapshot(&mut storage);
        let mut rust_area = vec![0xEE_u8; area_len];
        let mut c_area = vec![0xEE_u8; area_len];
        let rust_step = rust.step(SearchInput::Chunk(chunk), &mut rust_area);

        let input = aldis_sync_search_input_t {
            kind: ALDIS_SYNC_SEARCH_INPUT_CHUNK,
            data: chunk.as_ptr(),
            data_len: chunk.len(),
            config: c_config,
        };
        let mut actions = empty_actions();
        let mut detail = aldis_error_detail_t::default();
        // SAFETY: the storage holds a live engine; `input`, the area, and `actions` are live
        // locals or vectors that do not overlap each other or the storage.
        let code = unsafe {
            aldis_sync_search_step(
                &raw mut *storage,
                &raw const input,
                c_area.as_mut_ptr(),
                c_area.len(),
                &raw mut actions,
                &raw mut detail,
            )
        };
        match rust_step {
            Ok(list) => {
                assert_eq!(code, ALDIS_OK);
                assert_eq!(actions.count, list.len());
                assert_eq!(rust_area, c_area);
                for (c, r) in actions.items.iter().zip(list.iter()) {
                    assert_eq!(c.errors, u32::from(r.errors().get()));
                    assert_eq!(c.skipped_bits, r.skipped_bits());
                    let unit = r.unit().slice(&rust_area).unwrap();
                    assert_eq!(c.unit.len, unit.len());
                    assert_eq!(&c_area[c.unit.offset..c.unit.offset + c.unit.len], unit);
                }
                assert!(
                    actions.items[list.len()..]
                        .iter()
                        .all(|a| *a == aldis_sync_search_action_t::default())
                );
            }
            Err(e) => {
                // A refused step changes nothing, on both sides (API-058).
                match e.kind() {
                    ErrorKind::BufferTooSmall => assert_eq!(code, ALDIS_ERR_BUFFER_TOO_SMALL),
                    ErrorKind::OutOfRange => assert_eq!(code, ALDIS_ERR_OUT_OF_RANGE),
                    other => panic!("unexpected error {other:?}"),
                }
                assert_eq!(rust.status(), before_rust);
                assert_eq!(snapshot(&mut storage), before_c);
            }
        }
        same_status(&mut storage, &rust);
        at = end;
    }
    // SAFETY: the storage holds a live engine, deinitialised once.
    unsafe { aldis_sync_search_deinit(&raw mut *storage) };
});
