//! The contract of the C ABI itself, called as a C caller calls it: argument checks
//! (CABI-022), error detail (CABI-020, CABI-021), configuration sizes (CABI-033), and the
//! storage and handles of the stateful helpers (CABI-031, CABI-032).
#![expect(clippy::unwrap_used, reason = "test-only harness (RS-018)")]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis_ffi::capacities::ALDIS_SPP_TRACKER_MAX_APIDS;
use aldis_ffi::error::*;
use aldis_ffi::spp::*;
use aldis_ffi::spp_state::*;
use aldis_ffi::time::*;
use aldis_ffi::{ALDIS_MODE_STRICT, aldis_error_detail_t};

fn size<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap()
}

fn spp_cfg() -> aldis_spp_config_t {
    aldis_spp_config_t {
        struct_size: size::<aldis_spp_config_t>(),
        max_packet_len: ALDIS_SPP_MAX_PACKET_LEN,
    }
}

const fn fields(apid: u16) -> aldis_spp_fields_t {
    aldis_spp_fields_t {
        packet_type: ALDIS_SPP_TELEMETRY,
        sequence_flags: ALDIS_SPP_SEQ_UNSEGMENTED,
        apid,
        sequence_count: 0,
    }
}

const PACKET: [u8; 7] = [0x00, 0x05, 0xC0, 0x00, 0x00, 0x00, 0xAA];

#[test]
fn null_pointers_are_invalid_arguments() {
    // req: CABI-022, CABI-021
    let mut view = aldis_spp_packet_t::default();
    let mut n = 0;
    let mut d = aldis_error_detail_t::default();
    // SAFETY: every pointer is NULL or to a live local.
    unsafe {
        assert_eq!(
            aldis_spp_decode(null(), 7, ALDIS_MODE_STRICT, &raw mut view, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!((d.kind, d.field, d.detail), (6, 0, ALDIS_DETAIL_NONE));
        assert_eq!(
            aldis_spp_decode(
                PACKET.as_ptr(),
                7,
                ALDIS_MODE_STRICT,
                null_mut(),
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // An empty buffer may be NULL: the decoder then reports what it needs.
        assert_eq!(
            aldis_spp_peek_length(null(), 0, &raw mut n, &raw mut d),
            ALDIS_ERR_INCOMPLETE
        );
        assert_eq!((d.kind, d.detail, d.needed), (2, ALDIS_DETAIL_NEEDED, 6));
        let cfg = spp_cfg();
        let f = fields(5);
        assert_eq!(
            aldis_spp_encode(
                null(),
                &raw const f,
                null(),
                0,
                PACKET.as_ptr(),
                1,
                null_mut(),
                0,
                &raw mut n,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_spp_encode(
                &raw const cfg,
                null(),
                null(),
                0,
                PACKET.as_ptr(),
                1,
                null_mut(),
                0,
                &raw mut n,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // The output buffer of 0 octets may be NULL: too small, with `needed`.
        assert_eq!(
            aldis_spp_encode(
                &raw const cfg,
                &raw const f,
                null(),
                0,
                PACKET.as_ptr(),
                1,
                null_mut(),
                0,
                &raw mut n,
                &raw mut d
            ),
            ALDIS_ERR_BUFFER_TOO_SMALL
        );
        assert_eq!((d.kind, d.needed), (1, 7));
        // An unknown mode constant.
        assert_eq!(
            aldis_spp_decode(PACKET.as_ptr(), 7, 2, &raw mut view, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn outputs_must_not_overlap_inputs() {
    // req: CABI-022
    let cfg = spp_cfg();
    let f = fields(5);
    let mut buf = [0_u8; 32];
    let mut n = 0;
    let base = buf.as_mut_ptr();
    // SAFETY: both ranges lie in `buf`; the overlap is rejected before any slice is made.
    unsafe {
        assert_eq!(
            aldis_spp_encode(
                &raw const cfg,
                &raw const f,
                null(),
                0,
                base.cast_const(),
                4,
                base.add(2),
                16,
                &raw mut n,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_spp_encode(
                &raw const cfg,
                &raw const f,
                null(),
                0,
                base.cast_const(),
                4,
                base.add(4),
                16,
                &raw mut n,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(n, 10);
    }
}

#[test]
fn errors_carry_their_field_code_and_values() {
    // req: CABI-021, API-082
    let cfg = spp_cfg();
    let f = fields(4096);
    let mut out = [0_u8; 16];
    let mut n = 0;
    let mut d = aldis_error_detail_t::default();
    // SAFETY: every pointer is to a live local.
    let status = unsafe {
        aldis_spp_encode(
            &raw const cfg,
            &raw const f,
            null(),
            0,
            PACKET.as_ptr(),
            1,
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            &raw mut d,
        )
    };
    assert_eq!(status, ALDIS_ERR_OUT_OF_RANGE);
    assert_eq!(
        d,
        aldis_error_detail_t {
            kind: 5,
            field: aldis_ffi::field::ALDIS_FIELD_APID,
            detail: ALDIS_DETAIL_EXPECTED,
            needed: 0,
            expected: 2047,
            found: 4096,
        }
    );
    // The detail is written only on failure.
    let ok_fields = fields(5);
    // SAFETY: as above.
    let ok = unsafe {
        aldis_spp_encode(
            &raw const cfg,
            &raw const ok_fields,
            null(),
            0,
            PACKET.as_ptr(),
            1,
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            &raw mut d,
        )
    };
    assert_eq!((ok, d.found), (ALDIS_OK, 4096));
}

#[test]
fn configurations_need_a_struct_size() {
    // req: CABI-033, CABI-022
    let small = aldis_spp_config_t {
        struct_size: 4,
        max_packet_len: ALDIS_SPP_MAX_PACKET_LEN,
    };
    let larger = aldis_spp_config_t {
        struct_size: 64,
        ..spp_cfg()
    };
    // SAFETY: the configs are live locals; `larger` claims more than it has, which the
    // library never reads (it reads only the struct it knows).
    unsafe {
        assert_eq!(
            aldis_spp_config_check(&raw const small, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_spp_config_check(&raw const larger, null_mut()),
            ALDIS_OK
        );
        let bad = aldis_spp_config_t {
            max_packet_len: 6,
            ..spp_cfg()
        };
        let mut d = aldis_error_detail_t::default();
        assert_eq!(
            aldis_spp_config_check(&raw const bad, &raw mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
        assert_eq!(d.field, aldis_ffi::field::ALDIS_FIELD_MAX_PACKET_LEN);
    }
    let format = aldis_time_format_t {
        struct_size: 8,
        code: ALDIS_TIME_CDS,
        ..aldis_time_format_t::default()
    };
    let mut t_len = 0;
    // SAFETY: as above.
    unsafe {
        assert_eq!(
            aldis_time_format_t_len(&raw const format, &raw mut t_len, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        let format = aldis_time_format_t {
            struct_size: size::<aldis_time_format_t>(),
            ..format
        };
        assert_eq!(
            aldis_time_format_t_len(&raw const format, &raw mut t_len, null_mut()),
            ALDIS_OK
        );
        assert_eq!(t_len, 6);
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per storage rule")]
fn storage_is_checked_at_init() {
    // req: CABI-031, CABI-030
    let cfg = aldis_spp_tracker_config_t {
        struct_size: size::<aldis_spp_tracker_config_t>(),
        capacity: 2,
    };
    let mut words = vec![0_u64; ALDIS_SPP_TRACKER_STORAGE_SIZE / 8 + 1];
    let mut h: *mut aldis_spp_tracker_t = null_mut();
    let mut d = aldis_error_detail_t::default();
    assert_eq!(
        aldis_spp_tracker_storage_size(),
        ALDIS_SPP_TRACKER_STORAGE_SIZE
    );
    assert_eq!(
        aldis_spp_tracker_storage_align(),
        ALDIS_SPP_TRACKER_STORAGE_ALIGN
    );
    assert_eq!(
        aldis_spp_segmenter_storage_size(),
        ALDIS_SPP_SEGMENTER_STORAGE_SIZE
    );
    assert_eq!(
        aldis_spp_segmenter_storage_align(),
        ALDIS_SPP_SEGMENTER_STORAGE_ALIGN
    );
    assert_eq!(
        aldis_spp_reassembler_storage_size(),
        ALDIS_SPP_REASSEMBLER_STORAGE_SIZE
    );
    assert_eq!(
        aldis_spp_reassembler_storage_align(),
        ALDIS_SPP_REASSEMBLER_STORAGE_ALIGN
    );
    let base = words.as_mut_ptr().cast::<core::ffi::c_void>();
    // SAFETY: every pointer is NULL or into `words`, which has room for the slot at either
    // offset.
    unsafe {
        assert_eq!(
            aldis_spp_tracker_init(
                base,
                ALDIS_SPP_TRACKER_STORAGE_SIZE - 1,
                &raw const cfg,
                &raw mut h,
                &raw mut d
            ),
            ALDIS_ERR_BUFFER_TOO_SMALL
        );
        assert_eq!(
            d.needed,
            u64::try_from(ALDIS_SPP_TRACKER_STORAGE_SIZE).unwrap()
        );
        let odd = base.cast::<u8>().add(4).cast::<core::ffi::c_void>();
        assert_eq!(
            aldis_spp_tracker_init(
                odd,
                ALDIS_SPP_TRACKER_STORAGE_SIZE,
                &raw const cfg,
                &raw mut h,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_spp_tracker_init(
                null_mut(),
                ALDIS_SPP_TRACKER_STORAGE_SIZE,
                &raw const cfg,
                &raw mut h,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        let zero = aldis_spp_tracker_config_t { capacity: 0, ..cfg };
        assert_eq!(
            aldis_spp_tracker_init(
                base,
                ALDIS_SPP_TRACKER_STORAGE_SIZE,
                &raw const zero,
                &raw mut h,
                null_mut()
            ),
            ALDIS_ERR_INVALID_CONFIG
        );
        let big = aldis_spp_tracker_config_t {
            capacity: u32::try_from(ALDIS_SPP_TRACKER_MAX_APIDS).unwrap() + 1,
            ..cfg
        };
        assert_eq!(
            aldis_spp_tracker_init(
                base,
                ALDIS_SPP_TRACKER_STORAGE_SIZE,
                &raw const big,
                &raw mut h,
                null_mut()
            ),
            ALDIS_ERR_CAPACITY_EXCEEDED
        );
        assert!(h.is_null());
        assert_eq!(
            aldis_spp_tracker_init(
                base,
                ALDIS_SPP_TRACKER_STORAGE_SIZE,
                &raw const cfg,
                &raw mut h,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(h.cast::<core::ffi::c_void>(), base);
    }
}

#[test]
fn handles_are_tagged() {
    // req: CABI-032
    let cfg = aldis_spp_tracker_config_t {
        struct_size: size::<aldis_spp_tracker_config_t>(),
        capacity: 1,
    };
    let mut words = vec![0_u64; ALDIS_SPP_TRACKER_STORAGE_SIZE / 8];
    let mut h: *mut aldis_spp_tracker_t = null_mut();
    let mut t = aldis_spp_tracked_t::default();
    let mut len = 0;
    let base = words.as_mut_ptr().cast::<core::ffi::c_void>();
    // SAFETY: every handle points into `words` or is NULL.
    unsafe {
        // Never initialized.
        assert_eq!(
            aldis_spp_tracker_len(base.cast(), &raw mut len, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_spp_tracker_init(
                base,
                words.len() * 8,
                &raw const cfg,
                &raw mut h,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            aldis_spp_tracker_track(h, 5, 1, &raw mut t, null_mut()),
            ALDIS_OK
        );
        assert_eq!(t.continuity, ALDIS_SPP_TRACK_FIRST);
        // The capacity the caller chose.
        let mut d = aldis_error_detail_t::default();
        assert_eq!(
            aldis_spp_tracker_track(h, 6, 1, &raw mut t, &raw mut d),
            ALDIS_ERR_CAPACITY_EXCEEDED
        );
        assert_eq!(
            (d.field, d.expected, d.found),
            (aldis_ffi::field::ALDIS_FIELD_APID, 1, 2)
        );
        let (mut known, mut count) = (false, 0_u16);
        assert_eq!(
            aldis_spp_tracker_last(h, 5, &raw mut known, &raw mut count, null_mut()),
            ALDIS_OK
        );
        assert_eq!((known, count), (true, 1));
        // Foreign: a tracker is not a segmenter or a reassembler.
        let mut remaining = 0;
        assert_eq!(
            aldis_spp_segmenter_remaining(h.cast(), &raw mut remaining, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        let mut assembling = false;
        assert_eq!(
            aldis_spp_reassembler_is_assembling(h.cast(), &raw mut assembling, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // A NULL handle is an invalid argument (CABI-022).
        assert_eq!(
            aldis_spp_tracker_len(null_mut(), &raw mut len, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        // Deinitialized.
        aldis_spp_tracker_deinit(h);
        assert_eq!(
            aldis_spp_tracker_len(h, &raw mut len, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        let mut forgot = true;
        assert_eq!(
            aldis_spp_tracker_forget(h, 5, &raw mut forgot, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        aldis_spp_tracker_deinit(null_mut());
    }
}

#[test]
fn a_moved_storage_is_foreign() {
    // req: CABI-032
    let mut words = vec![0_u64; 2 * ALDIS_SPP_REASSEMBLER_STORAGE_SIZE / 8];
    let half = ALDIS_SPP_REASSEMBLER_STORAGE_SIZE / 8;
    let mut h: *mut aldis_spp_reassembler_t = null_mut();
    let mut assembling = true;
    // SAFETY: both halves of `words` hold a slot; the copy is made with `copy_within`.
    unsafe {
        assert_eq!(
            aldis_spp_reassembler_init(
                words.as_mut_ptr().cast(),
                half * 8,
                7,
                &raw mut h,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            aldis_spp_reassembler_is_assembling(h, &raw mut assembling, null_mut()),
            ALDIS_OK
        );
        assert!(!assembling);
    }
    words.copy_within(..half, half);
    let moved = words[half..].as_mut_ptr().cast::<aldis_spp_reassembler_t>();
    // SAFETY: `moved` points to the copy in `words`.
    unsafe {
        assert_eq!(
            aldis_spp_reassembler_is_assembling(moved, &raw mut assembling, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_spp_reassembler_reset(moved, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        let h = words.as_mut_ptr().cast::<aldis_spp_reassembler_t>();
        assert_eq!(aldis_spp_reassembler_reset(h, null_mut()), ALDIS_OK);
        aldis_spp_reassembler_deinit(h);
        assert_eq!(
            aldis_spp_reassembler_reset(h, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn helpers_never_keep_a_buffer() {
    // req: API-060, SPP-013, CABI-022
    let spp = spp_cfg();
    let cfg = aldis_spp_segmenter_config_t {
        struct_size: size::<aldis_spp_segmenter_config_t>(),
        packet_type: ALDIS_SPP_TELEMETRY,
        apid: 9,
        max_user_len: 2,
    };
    let sdu = [1_u8, 2, 3];
    let mut words = vec![0_u64; ALDIS_SPP_SEGMENTER_STORAGE_SIZE / 8];
    let mut h: *mut aldis_spp_segmenter_t = null_mut();
    let mut out = [0_u8; 16];
    let (mut n, mut next) = (0, 0_u16);
    let mut d = aldis_error_detail_t::default();
    // SAFETY: every pointer is to a live local or into `words`.
    unsafe {
        assert_eq!(
            aldis_spp_segmenter_init(
                words.as_mut_ptr().cast(),
                words.len() * 8,
                &raw const spp,
                &raw const cfg,
                sdu.as_ptr(),
                3,
                &raw mut h,
                null_mut()
            ),
            ALDIS_OK
        );
        // The same SDU, moved elsewhere, is fine; one of another length is not.
        let moved = sdu;
        assert_eq!(
            aldis_spp_segmenter_next(
                h,
                moved.as_ptr(),
                3,
                0,
                null(),
                0,
                out.as_mut_ptr(),
                16,
                &raw mut n,
                &raw mut next,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!((n, next), (8, 1));
        assert_eq!(
            aldis_spp_segmenter_next(
                h,
                sdu.as_ptr(),
                2,
                1,
                null(),
                0,
                out.as_mut_ptr(),
                16,
                &raw mut n,
                &raw mut next,
                &raw mut d
            ),
            ALDIS_ERR_LENGTH_MISMATCH
        );
        assert_eq!(d.field, aldis_ffi::field::ALDIS_FIELD_SDU);
        // The output may not overlap the helper's storage.
        assert_eq!(
            aldis_spp_segmenter_next(
                h,
                sdu.as_ptr(),
                3,
                1,
                null(),
                0,
                words.as_mut_ptr().cast(),
                16,
                &raw mut n,
                &raw mut next,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        let mut remaining = 0;
        assert_eq!(
            aldis_spp_segmenter_remaining(h, &raw mut remaining, null_mut()),
            ALDIS_OK
        );
        assert_eq!(remaining, 1);
        aldis_spp_segmenter_deinit(h);
    }
}

#[test]
fn reassembly_takes_the_buffer_on_every_push() {
    // req: API-060, SPP-013
    let first = [0x00_u8, 0x07, 0x40, 0x00, 0x00, 0x01, 0xA1, 0xA2];
    let last = [0x00_u8, 0x07, 0x80, 0x01, 0x00, 0x00, 0xA3];
    let layout = aldis_spp_secondary_layout_t {
        struct_size: size::<aldis_spp_secondary_layout_t>(),
        kind: ALDIS_SPP_SECONDARY_ABSENT,
        time_format: aldis_time_format_t {
            struct_size: size::<aldis_time_format_t>(),
            ..aldis_time_format_t::default()
        },
        ..aldis_spp_secondary_layout_t::default()
    };
    let mut words = vec![0_u64; ALDIS_SPP_REASSEMBLER_STORAGE_SIZE / 8];
    let mut h: *mut aldis_spp_reassembler_t = null_mut();
    let mut buf = [0_u8; 8];
    let mut r = aldis_spp_reassembly_t::default();
    let mut d = aldis_error_detail_t::default();
    // SAFETY: every pointer is to a live local or into `words`.
    unsafe {
        assert_eq!(
            aldis_spp_reassembler_init(
                words.as_mut_ptr().cast(),
                words.len() * 8,
                7,
                &raw mut h,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            aldis_spp_reassembler_push(
                h,
                first.as_ptr(),
                8,
                ALDIS_MODE_STRICT,
                &raw const layout,
                buf.as_mut_ptr(),
                8,
                &raw mut r,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(r.status, ALDIS_SPP_REASSEMBLY_IN_PROGRESS);
        // A buffer too short for the SDU in progress.
        assert_eq!(
            aldis_spp_reassembler_push(
                h,
                last.as_ptr(),
                7,
                ALDIS_MODE_STRICT,
                &raw const layout,
                buf.as_mut_ptr(),
                1,
                &raw mut r,
                &raw mut d
            ),
            ALDIS_ERR_BUFFER_TOO_SMALL
        );
        assert_eq!(d.needed, 2);
        // Another APID.
        let other = [0x00_u8, 0x08, 0xC0, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(
            aldis_spp_reassembler_push(
                h,
                other.as_ptr(),
                7,
                ALDIS_MODE_STRICT,
                &raw const layout,
                buf.as_mut_ptr(),
                8,
                &raw mut r,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.field, aldis_ffi::field::ALDIS_FIELD_APID);
        let mut assembling = false;
        assert_eq!(
            aldis_spp_reassembler_is_assembling(h, &raw mut assembling, null_mut()),
            ALDIS_OK
        );
        assert!(assembling);
        assert_eq!(
            aldis_spp_reassembler_push(
                h,
                last.as_ptr(),
                7,
                ALDIS_MODE_STRICT,
                &raw const layout,
                buf.as_mut_ptr(),
                8,
                &raw mut r,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!((r.status, r.sdu_len), (ALDIS_SPP_REASSEMBLY_COMPLETE, 3));
        assert_eq!(&buf[..3], &[0xA1, 0xA2, 0xA3]);
        assert_eq!(aldis_spp_reassembler_reset(h, null_mut()), ALDIS_OK);
    }
}

#[test]
fn counts_and_epi_checks() {
    // req: SPP-011, EPP-010, CABI-020
    let mut next = 0_u16;
    let mut d = aldis_error_detail_t::default();
    let cfg = aldis_ffi::epp::aldis_epp_config_t {
        struct_size: size::<aldis_ffi::epp::aldis_epp_config_t>(),
        min_data_len: 1,
        max_data_len: 8,
        valid_epis: 1 << 2,
        valid_extended_epis: 0,
    };
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_spp_sequence_count_next(16_383, &raw mut next, null_mut()),
            ALDIS_OK
        );
        assert_eq!(next, 0);
        assert_eq!(
            aldis_spp_sequence_count_next(16_384, &raw mut next, &raw mut d),
            ALDIS_ERR_OUT_OF_RANGE
        );
        assert_eq!(d.field, aldis_ffi::field::ALDIS_FIELD_SEQUENCE_COUNT);
        assert_eq!(
            aldis_ffi::epp::aldis_epp_check(&raw const cfg, 2, false, 0, 4, null_mut()),
            ALDIS_OK
        );
        assert_eq!(
            aldis_ffi::epp::aldis_epp_check(&raw const cfg, 3, false, 0, 4, &raw mut d),
            ALDIS_ERR_UNKNOWN_CHANNEL
        );
        assert_eq!(d.field, aldis_ffi::field::ALDIS_FIELD_EPI);
        assert_eq!(
            aldis_ffi::epp::aldis_epp_check(&raw const cfg, 2, true, 16, 4, &raw mut d),
            ALDIS_ERR_OUT_OF_RANGE
        );
        assert_eq!(
            aldis_ffi::epp::aldis_epp_check(&raw const cfg, 2, false, 0, 9, &raw mut d),
            ALDIS_ERR_OUT_OF_RANGE
        );
        assert_eq!(d.field, aldis_ffi::field::ALDIS_FIELD_DATA_LEN);
    }
}
