//! The contract of the `aldis_sync_*` functions, called as a C caller calls them: storage and tags
//! (CABI-031, CABI-032), argument checks (CABI-022), the choices of the C ABI (SYNC-002,
//! SYNC-020), the capacities of the header (CABI-030, SYNC-014), and a frame of the largest
//! length through attach, randomize, and search (SYNC-017). The rules of the codec and the engine
//! are checked by `crates/aldis-sync/tests/`, and the vectors by `vectors.rs`.
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::large_stack_arrays,
    clippy::too_many_lines,
    reason = "test-only harness: one long call sequence per contract, and the storage a C caller declares (RS-018)"
)]

use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::capacities::{ALDIS_SYNC_MAX_CHUNK_OCTETS, ALDIS_SYNC_MAX_FRAME_OCTETS};
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_ASM_ERRORS, ALDIS_FIELD_CHUNK_LEN, ALDIS_FIELD_FRAME_LEN,
    ALDIS_FIELD_RANDOMIZER_SEQUENCE, ALDIS_FIELD_SYNC_PROTOCOL,
};
use aldis_ffi::link::*;
use aldis_ffi::sync::*;

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

fn storage() -> Box<aldis_sync_search_t> {
    Box::new(aldis_sync_search_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_SYNC_SEARCH_WORDS],
    })
}

const fn config(protocol: u32, frame_len: usize) -> aldis_sync_search_config_t {
    aldis_sync_search_config_t {
        protocol,
        frame_len,
        asm_errors: 0,
    }
}

const fn input(kind: u32, data: &[u8]) -> aldis_sync_search_input_t {
    aldis_sync_search_input_t {
        kind,
        data: data.as_ptr(),
        data_len: data.len(),
        config: config(ALDIS_LINK_PROTOCOL_TM, 4),
    }
}

fn empty_actions() -> aldis_sync_search_actions_t {
    aldis_sync_search_actions_t {
        count: 0,
        items: [aldis_sync_search_action_t::default(); ALDIS_SYNC_SEARCH_MAX_ACTIONS],
    }
}

#[test]
fn capacities_are_published_and_match_the_engine() {
    // req: CABI-030, CABI-031, SYNC-013, SYNC-014
    assert_eq!(ALDIS_SYNC_MAX_CHUNK_OCTETS, 256);
    assert_eq!(ALDIS_SYNC_MAX_FRAME_OCTETS, 65536);
    assert_eq!(
        ALDIS_SYNC_SEARCH_MAX_ACTIONS,
        aldis::sync::max_actions(ALDIS_SYNC_MAX_CHUNK_OCTETS)
    );
    assert_eq!(ALDIS_SYNC_SEARCH_MAX_ACTIONS, 52);
    assert_eq!(
        size_of::<aldis_sync_search_t>(),
        ALDIS_SYNC_SEARCH_WORDS * 8
    );
    assert_eq!(align_of::<aldis_sync_search_t>(), 8);
    // The frame capacity is the largest frame of any protocol, so no valid length exceeds it.
    assert_eq!(
        ALDIS_SYNC_MAX_FRAME_OCTETS,
        aldis::sync::SyncProtocol::Aos.max_frame_len()
    );
    // The values of the randomizer choice are the lengths of the sequences.
    assert_eq!(
        ALDIS_SYNC_RANDOMIZER_131071,
        aldis::sync::RandomizerSequence::Period131071.period()
    );
    assert_eq!(
        ALDIS_SYNC_RANDOMIZER_255,
        aldis::sync::RandomizerSequence::Period255.period()
    );
}

#[test]
fn the_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = config(ALDIS_LINK_PROTOCOL_TM, 4);
    let mut d = detail();
    let mut s = storage();
    let mut status = aldis_sync_search_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_sync_search_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_sync_search_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_sync_search_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_sync_search_status(null_mut(), &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_sync_search_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_sync_search_status_t::default());
        // A copy of live storage is foreign: the tag holds its own address.
        let mut copy = storage();
        copy.words = s.words;
        assert_eq!(
            aldis_sync_search_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // The status output must not overlap the storage.
        let inside = (&raw mut *s).cast::<aldis_sync_search_status_t>();
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_sync_search_deinit(&raw mut *s);
        aldis_sync_search_deinit(null_mut());
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_choices_and_the_configuration_are_checked_in_order() {
    // req: SYNC-002, SYNC-016, SYNC-020, CABI-022
    let mut s = storage();
    let mut d = detail();
    let bad = |protocol, frame_len, asm_errors| aldis_sync_search_config_t {
        protocol,
        frame_len,
        asm_errors,
    };
    // SAFETY: every pointer is to a live local.
    unsafe {
        for (cfg, field) in [
            (bad(ALDIS_LINK_PROTOCOL_TC, 4, 0), ALDIS_FIELD_SYNC_PROTOCOL),
            (bad(99, 4, 0), ALDIS_FIELD_SYNC_PROTOCOL),
            (bad(99, 0, 16), ALDIS_FIELD_SYNC_PROTOCOL),
            (bad(ALDIS_LINK_PROTOCOL_TM, 0, 16), ALDIS_FIELD_FRAME_LEN),
            (bad(ALDIS_LINK_PROTOCOL_TM, 2049, 0), ALDIS_FIELD_FRAME_LEN),
            (
                bad(ALDIS_LINK_PROTOCOL_AOS, 65537, 0),
                ALDIS_FIELD_FRAME_LEN,
            ),
            (bad(ALDIS_LINK_PROTOCOL_USLP, 4, 16), ALDIS_FIELD_ASM_ERRORS),
        ] {
            assert_eq!(
                aldis_sync_search_init(&raw mut *s, &raw const cfg, &raw mut d),
                ALDIS_ERR_INVALID_CONFIG
            );
            assert_eq!(d.field, field);
        }
        // The largest frame of the largest protocol fits the capacity.
        let cfg = bad(ALDIS_LINK_PROTOCOL_AOS, ALDIS_SYNC_MAX_FRAME_OCTETS, 15);
        assert_eq!(
            aldis_sync_search_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        // The randomizer choice: only the two sequence lengths.
        let mut data = [1_u8, 2, 3];
        for sequence in [0, 1, 254, 256, 131_070, 131_072, u32::MAX] {
            assert_eq!(
                aldis_sync_randomize(sequence, data.as_mut_ptr(), data.len(), &raw mut d),
                ALDIS_ERR_INVALID_CONFIG
            );
            assert_eq!(d.field, ALDIS_FIELD_RANDOMIZER_SEQUENCE);
            assert_eq!(data, [1, 2, 3]);
        }
    }
}

#[test]
fn the_codec_functions_check_their_pointers() {
    // req: CABI-022, SYNC-003, SYNC-004
    let cfg = config(ALDIS_LINK_PROTOCOL_TM, 3);
    let frame = [1_u8, 2, 3];
    let mut out = [0_u8; 7];
    let mut len = 0_usize;
    // SAFETY: every pointer is NULL, to a live local, or overlaps another argument on purpose and
    // is rejected before any access.
    unsafe {
        let attach = |cfg: *const aldis_sync_search_config_t,
                      frame: *const u8,
                      frame_len: usize,
                      out: *mut u8,
                      cap: usize,
                      out_len: *mut usize| {
            aldis_sync_attach(cfg, frame, frame_len, out, cap, out_len, null_mut())
        };
        assert_eq!(
            attach(
                &raw const cfg,
                frame.as_ptr(),
                3,
                out.as_mut_ptr(),
                7,
                &raw mut len
            ),
            0
        );
        assert_eq!(
            (len, out[..4].to_vec(), out[4..].to_vec()),
            (7, vec![0x1A, 0xCF, 0xFC, 0x1D], frame.to_vec())
        );
        assert_eq!(
            attach(null(), frame.as_ptr(), 3, out.as_mut_ptr(), 7, &raw mut len),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            attach(&raw const cfg, null(), 3, out.as_mut_ptr(), 7, &raw mut len),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            attach(
                &raw const cfg,
                frame.as_ptr(),
                3,
                null_mut(),
                7,
                &raw mut len
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            attach(
                &raw const cfg,
                frame.as_ptr(),
                3,
                out.as_mut_ptr(),
                7,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // The CADU may not be built over the frame.
        assert_eq!(
            attach(
                &raw const cfg,
                out.as_ptr().wrapping_add(4),
                3,
                out.as_mut_ptr(),
                7,
                &raw mut len
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // An empty frame is a length mismatch, and NULL is allowed for it.
        assert_eq!(
            attach(&raw const cfg, null(), 0, out.as_mut_ptr(), 7, &raw mut len),
            ALDIS_ERR_LENGTH_MISMATCH
        );
        // The randomizer: a buffer of any length, including none, with NULL for none.
        assert_eq!(
            aldis_sync_randomize(ALDIS_SYNC_RANDOMIZER_255, null_mut(), 0, null_mut()),
            0
        );
        assert_eq!(
            aldis_sync_randomize(ALDIS_SYNC_RANDOMIZER_255, null_mut(), 1, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn a_step_checks_its_arguments_and_the_chunk_capacity() {
    // req: SYNC-014, SYNC-015, CABI-022, CABI-040, API-058
    let cfg = config(ALDIS_LINK_PROTOCOL_TM, 4);
    let mut s = storage();
    let mut d = detail();
    let mut area = vec![0xEE_u8; ALDIS_SYNC_MAX_CHUNK_OCTETS + ALDIS_SYNC_MAX_FRAME_OCTETS];
    let mut actions = empty_actions();
    let mut status = aldis_sync_search_status_t::default();
    let junk = [0_u8; ALDIS_SYNC_MAX_CHUNK_OCTETS + 1];
    let chunk = input(
        ALDIS_SYNC_SEARCH_INPUT_CHUNK,
        &junk[..ALDIS_SYNC_MAX_CHUNK_OCTETS],
    );
    let too_long = input(ALDIS_SYNC_SEARCH_INPUT_CHUNK, &junk);
    // SAFETY: every pointer is to a live local or vector, or rejected before any access.
    unsafe {
        assert_eq!(
            aldis_sync_search_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        let (d_ptr, actions_ptr) = (&raw mut d, &raw mut actions);
        let step =
            |h: *mut aldis_sync_search_t,
             i: &aldis_sync_search_input_t,
             out: *mut u8,
             cap: usize| { aldis_sync_search_step(h, i, out, cap, actions_ptr, d_ptr) };
        // A chunk above the capacity: `chunk_len`, the limit and the value, nothing changed.
        assert_eq!(
            step(&raw mut *s, &too_long, area.as_mut_ptr(), area.len()),
            ALDIS_ERR_OUT_OF_RANGE
        );
        assert_eq!(
            (d.field, d.detail, d.expected, d.found),
            (ALDIS_FIELD_CHUNK_LEN, ALDIS_DETAIL_EXPECTED, 256, 257)
        );
        assert_eq!(step(&raw mut *s, &chunk, area.as_mut_ptr(), area.len()), 0);
        // A kind that is none.
        let none = input(99, &[]);
        assert_eq!(
            step(&raw mut *s, &none, area.as_mut_ptr(), area.len()),
            ALDIS_ERR_INVALID_FIELD
        );
        // NULL handle, NULL input, data inside the storage, `out` over the data.
        assert_eq!(
            step(null_mut(), &chunk, area.as_mut_ptr(), area.len()),
            ALDIS_ERR_INVALID_FIELD
        );
        let inside = aldis_sync_search_input_t {
            data: (&raw const *s).cast::<u8>(),
            data_len: 8,
            ..chunk
        };
        assert_eq!(
            step(&raw mut *s, &inside, area.as_mut_ptr(), area.len()),
            ALDIS_ERR_INVALID_FIELD
        );
        let over = aldis_sync_search_input_t {
            data: area.as_ptr(),
            data_len: 8,
            ..chunk
        };
        assert_eq!(
            step(&raw mut *s, &over, area.as_mut_ptr(), area.len()),
            ALDIS_ERR_INVALID_FIELD
        );
        // The actions may not overlap the area.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "the call rejects the overlap before it reads or writes"
        )]
        let overlap = area.as_mut_ptr().cast::<aldis_sync_search_actions_t>();
        assert_eq!(
            aldis_sync_search_step(
                &raw mut *s,
                &raw const chunk,
                area.as_mut_ptr(),
                area.len(),
                overlap,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // A reconfiguration that is refused changes nothing; one that is accepted resets.
        let head = [0x1A_u8, 0xCF, 0xFC, 0x1D, 0xAA];
        let feed = input(ALDIS_SYNC_SEARCH_INPUT_CHUNK, &head);
        assert_eq!(step(&raw mut *s, &feed, area.as_mut_ptr(), area.len()), 0);
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(
            (status.state, status.partial_octets),
            (ALDIS_SYNC_SEARCH_STATE_COLLECTING, 1)
        );
        let refused = aldis_sync_search_input_t {
            kind: ALDIS_SYNC_SEARCH_INPUT_RECONFIGURE,
            data: null(),
            data_len: 0,
            config: config(ALDIS_LINK_PROTOCOL_TC, 4),
        };
        assert_eq!(
            step(&raw mut *s, &refused, area.as_mut_ptr(), area.len()),
            ALDIS_ERR_INVALID_CONFIG
        );
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status.partial_octets, 1);
        let accepted = aldis_sync_search_input_t {
            config: config(ALDIS_LINK_PROTOCOL_AOS, 2),
            ..refused
        };
        assert_eq!(
            step(&raw mut *s, &accepted, area.as_mut_ptr(), area.len()),
            0
        );
        assert_eq!(
            aldis_sync_search_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(
            (status.state, status.partial_octets),
            (ALDIS_SYNC_SEARCH_STATE_SEARCHING, 0)
        );
        // The new frame length is in force: a CADU of two octets gives a unit of two.
        let cadu = [0x1A_u8, 0xCF, 0xFC, 0x1D, 0xAB, 0xCD];
        let feed = input(ALDIS_SYNC_SEARCH_INPUT_CHUNK, &cadu);
        assert_eq!(step(&raw mut *s, &feed, area.as_mut_ptr(), area.len()), 0);
        assert_eq!(actions.count, 1);
        assert_eq!(area[..2], [0xAB, 0xCD]);
    }
}

#[test]
fn a_frame_of_the_largest_length_goes_through_attach_randomize_and_search() {
    // req: SYNC-017, SYNC-014, CABI-030, CABI-040
    let len = ALDIS_SYNC_MAX_FRAME_OCTETS;
    let cfg = config(ALDIS_LINK_PROTOCOL_AOS, len);
    let frame: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect();
    let mut sent = frame.clone();
    let mut cadu = vec![0_u8; len + 4];
    let mut cadu_len = 0_usize;
    let mut s = storage();
    let mut area = vec![0_u8; ALDIS_SYNC_MAX_CHUNK_OCTETS + ALDIS_SYNC_MAX_FRAME_OCTETS];
    let mut units = Vec::new();
    // SAFETY: every pointer is to a live local or vector, valid for the length given.
    unsafe {
        assert_eq!(
            aldis_sync_randomize(
                ALDIS_SYNC_RANDOMIZER_131071,
                sent.as_mut_ptr(),
                len,
                null_mut()
            ),
            0
        );
        assert_ne!(sent, frame);
        assert_eq!(
            aldis_sync_attach(
                &raw const cfg,
                sent.as_ptr(),
                len,
                cadu.as_mut_ptr(),
                len + 4,
                &raw mut cadu_len,
                null_mut()
            ),
            0
        );
        assert_eq!(cadu_len, len + 4);
        assert_eq!(
            aldis_sync_search_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        // Three junk octets, then the CADU, in chunks of the capacity.
        let mut stream = vec![0x55_u8; 3];
        stream.extend_from_slice(&cadu);
        for chunk in stream.chunks(ALDIS_SYNC_MAX_CHUNK_OCTETS) {
            let i = input(ALDIS_SYNC_SEARCH_INPUT_CHUNK, chunk);
            let mut actions = empty_actions();
            assert_eq!(
                aldis_sync_search_step(
                    &raw mut *s,
                    &raw const i,
                    area.as_mut_ptr(),
                    area.len(),
                    &raw mut actions,
                    null_mut()
                ),
                0
            );
            for a in &actions.items[..actions.count] {
                assert_eq!((a.errors, a.skipped_bits), (0, 24));
                units.push(area[a.unit.offset..a.unit.offset + a.unit.len].to_vec());
            }
        }
        assert_eq!(units.len(), 1);
        assert_eq!(units[0], sent);
        assert_eq!(
            aldis_sync_randomize(
                ALDIS_SYNC_RANDOMIZER_131071,
                units[0].as_mut_ptr(),
                len,
                null_mut()
            ),
            0
        );
        assert_eq!(units[0], frame);
        aldis_sync_search_deinit(&raw mut *s);
    }
}
