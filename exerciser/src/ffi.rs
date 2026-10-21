//! Calls every function `aldis-ffi` exports, so that the panic-freedom link check covers the
//! C ABI (VER-010, M2 plan D16). Each call's status is checked on the target. Inputs pass
//! through `black_box`, so every path of each function stays reachable. The vector cases run
//! through the Rust API (`vectors.rs`); the C ABI's own vector run is the `aldis-ffi`
//! integration test on the host, so one call per function suffices here.

use core::hint::black_box as bb;
use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::epp::{
    ALDIS_EPP_HEADER_AUTO, aldis_epp_check, aldis_epp_config_check, aldis_epp_config_t,
    aldis_epp_decode, aldis_epp_encode, aldis_epp_fields_t, aldis_epp_header_encode,
    aldis_epp_idle, aldis_epp_idle_header, aldis_epp_packet_t, aldis_epp_peek_length,
    aldis_epp_validate, aldis_epp_writer_begin, aldis_epp_writer_finish, aldis_epp_writer_layout_t,
};
use aldis_ffi::error::{ALDIS_ERR_INCOMPLETE, ALDIS_ERR_INVALID_HANDLE, ALDIS_OK};
use aldis_ffi::limits::{aldis_limits, aldis_limits_t};
use aldis_ffi::spp::{
    ALDIS_SPP_MAX_PACKET_LEN, ALDIS_SPP_SECONDARY_ABSENT, ALDIS_SPP_SECONDARY_OPAQUE,
    ALDIS_SPP_SEQ_UNSEGMENTED, ALDIS_SPP_TELEMETRY, aldis_spp_config_check, aldis_spp_config_t,
    aldis_spp_decode, aldis_spp_encode, aldis_spp_fields_t, aldis_spp_idle,
    aldis_spp_octet_string_encode, aldis_spp_packet_t, aldis_spp_peek_length,
    aldis_spp_secondary_layout_t, aldis_spp_sequence_count_next, aldis_spp_split,
    aldis_spp_split_t, aldis_spp_writer_begin, aldis_spp_writer_finish, aldis_spp_writer_layout_t,
};
use aldis_ffi::spp_state::{
    ALDIS_SPP_REASSEMBLER_STORAGE_SIZE, ALDIS_SPP_SEGMENTER_STORAGE_SIZE,
    ALDIS_SPP_TRACKER_STORAGE_SIZE, aldis_spp_reassembler_deinit, aldis_spp_reassembler_init,
    aldis_spp_reassembler_is_assembling, aldis_spp_reassembler_push, aldis_spp_reassembler_reset,
    aldis_spp_reassembler_storage_align, aldis_spp_reassembler_storage_size,
    aldis_spp_reassembler_t, aldis_spp_reassembly_t, aldis_spp_segmenter_config_t,
    aldis_spp_segmenter_deinit, aldis_spp_segmenter_init, aldis_spp_segmenter_next,
    aldis_spp_segmenter_remaining, aldis_spp_segmenter_storage_align,
    aldis_spp_segmenter_storage_size, aldis_spp_segmenter_t, aldis_spp_tracked_t,
    aldis_spp_tracker_config_t, aldis_spp_tracker_deinit, aldis_spp_tracker_forget,
    aldis_spp_tracker_init, aldis_spp_tracker_last, aldis_spp_tracker_len,
    aldis_spp_tracker_storage_align, aldis_spp_tracker_storage_size, aldis_spp_tracker_t,
    aldis_spp_tracker_track,
};
use aldis_ffi::time::{
    ALDIS_TIME_ASCII, ALDIS_TIME_CCS, ALDIS_TIME_CDS, ALDIS_TIME_CUC, aldis_time_ascii_encoded_len,
    aldis_time_ascii_format, aldis_time_ascii_options_t, aldis_time_ascii_parse,
    aldis_time_ascii_subset_parse, aldis_time_ascii_subset_t, aldis_time_ascii_t,
    aldis_time_convert, aldis_time_convert_config_t, aldis_time_cuc_from_ns, aldis_time_cuc_to_ns,
    aldis_time_decode, aldis_time_decode_implicit, aldis_time_decoded_t, aldis_time_encode,
    aldis_time_format_pfield, aldis_time_format_t, aldis_time_format_t_len, aldis_time_instant_t,
    aldis_time_pfield_parse, aldis_time_pfield_t, aldis_time_value_t,
};
use aldis_ffi::{ALDIS_MODE_LENIENT, ALDIS_MODE_STRICT};

use crate::exercise::Tally;

/// `size_of::<T>()` as the `struct_size` of a configuration (CABI-033).
fn size<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(0)
}

/// A detail to pass to every call.
/// Its `struct_size` is the size of the struct (CABI-021).
fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        struct_size: size::<aldis_error_detail_t>(),
        ..aldis_error_detail_t::default()
    }
}

const PACKET: [u8; 10] = [0x18, 0x23, 0xC0, 0x07, 0x00, 0x03, 0xA0, 0xB0, 0xB1, 0xEE];

fn spp_cfg() -> aldis_spp_config_t {
    aldis_spp_config_t {
        struct_size: size::<aldis_spp_config_t>(),
        max_packet_len: ALDIS_SPP_MAX_PACKET_LEN,
    }
}

const fn spp_fields() -> aldis_spp_fields_t {
    aldis_spp_fields_t {
        packet_type: ALDIS_SPP_TELEMETRY,
        sequence_flags: ALDIS_SPP_SEQ_UNSEGMENTED,
        apid: bb(0x023),
        sequence_count: bb(7),
    }
}

fn format(code: u32) -> aldis_time_format_t {
    aldis_time_format_t {
        struct_size: size::<aldis_time_format_t>(),
        code,
        coarse_octets: bb(2),
        fine_octets: bb(1),
        ..aldis_time_format_t::default()
    }
}

fn absent_layout() -> aldis_spp_secondary_layout_t {
    aldis_spp_secondary_layout_t {
        struct_size: size::<aldis_spp_secondary_layout_t>(),
        kind: ALDIS_SPP_SECONDARY_ABSENT,
        time_format: format(ALDIS_TIME_CUC),
        ..aldis_spp_secondary_layout_t::default()
    }
}

#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one call per exported function")]
fn spp(t: &mut Tally) {
    let (cfg, fields, mut det) = (spp_cfg(), spp_fields(), detail());
    let packet = bb(PACKET);
    let mut view = aldis_spp_packet_t::default();
    let mut out = [0_u8; 32];
    let (mut written, mut next) = (0_usize, 0_u16);
    // SAFETY: every pointer is to a live local; lengths are those of the buffers.
    unsafe {
        t.check(
            aldis_spp_decode(
                packet.as_ptr(),
                bb(10),
                bb(ALDIS_MODE_STRICT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK
                && view.data_len == 4,
        );
        t.check(
            aldis_spp_peek_length(packet.as_ptr(), bb(3), &raw mut written, &raw mut det)
                == ALDIS_ERR_INCOMPLETE
                && det.needed == 6,
        );
        t.check(
            aldis_spp_encode(
                &raw const cfg,
                &raw const fields,
                null(),
                0,
                packet.as_ptr(),
                bb(4),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK
                && written == 10,
        );
        let mut layout = aldis_spp_writer_layout_t::default();
        t.check(
            aldis_spp_writer_begin(
                &raw const cfg,
                &raw const fields,
                bb(0),
                bb(4),
                out.as_mut_ptr(),
                out.len(),
                &raw mut layout,
                &raw mut det,
            ) == ALDIS_OK
                && layout.user_data_offset == 6,
        );
        t.check(
            aldis_spp_writer_finish(
                &raw const cfg,
                &raw const fields,
                bb(0),
                bb(4),
                bb(3),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK
                && written == 9,
        );
        t.check(
            aldis_spp_idle(
                &raw const cfg,
                bb(9),
                bb(1),
                packet.as_ptr(),
                bb(2),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_spp_octet_string_encode(
                &raw const cfg,
                bb(ALDIS_SPP_TELEMETRY),
                bb(5),
                bb(16_383),
                bb(false),
                packet.as_ptr(),
                bb(3),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut next,
                &raw mut det,
            ) == ALDIS_OK
                && next == 0,
        );
        t.check(
            aldis_spp_sequence_count_next(bb(16_383), &raw mut next, &raw mut det) == ALDIS_OK
                && next == 0,
        );
        t.check(aldis_spp_config_check(&raw const cfg, &raw mut det) == ALDIS_OK);
        // The packet has a secondary header (flag 1): one opaque octet.
        let layout = aldis_spp_secondary_layout_t {
            kind: ALDIS_SPP_SECONDARY_OPAQUE,
            opaque_len: bb(1),
            ..absent_layout()
        };
        let mut split = aldis_spp_split_t::default();
        t.check(
            aldis_spp_split(
                packet.as_ptr(),
                bb(10),
                bb(ALDIS_MODE_LENIENT),
                &raw const layout,
                &raw mut split,
                &raw mut det,
            ) == ALDIS_OK
                && split.user_data_len == 3
                && split.user_data_offset == 7,
        );
        let mut limits = aldis_limits_t {
            struct_size: size::<aldis_limits_t>(),
            ..aldis_limits_t::default()
        };
        t.check(aldis_limits(&raw mut limits) == ALDIS_OK && limits.spp_tracker_max_apids > 0);
    }
}

#[expect(
    unsafe_code,
    reason = "calls the C ABI with valid pointers and storage"
)]
#[expect(clippy::too_many_lines, reason = "one call per exported function")]
fn spp_state(t: &mut Tally) {
    let mut det = detail();
    t.check(aldis_spp_tracker_storage_size() == ALDIS_SPP_TRACKER_STORAGE_SIZE);
    t.check(aldis_spp_tracker_storage_align() == 8 && aldis_spp_segmenter_storage_align() == 8);
    t.check(aldis_spp_segmenter_storage_size() == ALDIS_SPP_SEGMENTER_STORAGE_SIZE);
    t.check(aldis_spp_reassembler_storage_size() == ALDIS_SPP_REASSEMBLER_STORAGE_SIZE);
    t.check(aldis_spp_reassembler_storage_align() == 8);
    let mut tracker_words = [0_u64; ALDIS_SPP_TRACKER_STORAGE_SIZE / 8];
    let cfg = aldis_spp_tracker_config_t {
        struct_size: size::<aldis_spp_tracker_config_t>(),
        capacity: bb(2),
    };
    let mut h: *mut aldis_spp_tracker_t = null_mut();
    let mut tracked = aldis_spp_tracked_t::default();
    let (mut flag, mut count, mut len) = (false, 0_u16, 0_usize);
    // SAFETY: the storage is a live, aligned local of the published size; the handle points
    // into it; every other pointer is to a live local.
    unsafe {
        t.check(
            aldis_spp_tracker_init(
                tracker_words.as_mut_ptr().cast(),
                bb(ALDIS_SPP_TRACKER_STORAGE_SIZE),
                &raw const cfg,
                &raw mut h,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_spp_tracker_track(h, bb(5), bb(1), &raw mut tracked, &raw mut det) == ALDIS_OK,
        );
        t.check(
            aldis_spp_tracker_track(h, bb(5), bb(4), &raw mut tracked, &raw mut det) == ALDIS_OK
                && tracked.missing == 2,
        );
        t.check(
            aldis_spp_tracker_last(h, bb(5), &raw mut flag, &raw mut count, &raw mut det)
                == ALDIS_OK
                && flag
                && count == 4,
        );
        t.check(aldis_spp_tracker_len(h, &raw mut len, &raw mut det) == ALDIS_OK && len == 1);
        t.check(
            aldis_spp_tracker_forget(h, bb(5), &raw mut flag, &raw mut det) == ALDIS_OK && flag,
        );
        aldis_spp_tracker_deinit(h);
        t.check(aldis_spp_tracker_len(h, &raw mut len, &raw mut det) == ALDIS_ERR_INVALID_HANDLE);
    }
    let spp = spp_cfg();
    let seg_cfg = aldis_spp_segmenter_config_t {
        struct_size: size::<aldis_spp_segmenter_config_t>(),
        packet_type: ALDIS_SPP_TELEMETRY,
        apid: bb(9),
        max_user_len: bb(2),
    };
    let sdu = bb([1_u8, 2, 3]);
    let mut seg_words = [0_u64; ALDIS_SPP_SEGMENTER_STORAGE_SIZE / 8];
    let mut seg: *mut aldis_spp_segmenter_t = null_mut();
    let mut ra_words = [0_u64; ALDIS_SPP_REASSEMBLER_STORAGE_SIZE / 8];
    let mut ra: *mut aldis_spp_reassembler_t = null_mut();
    let layout = absent_layout();
    let mut buf = [0_u8; 8];
    let mut out = [0_u8; 16];
    let mut outcome = aldis_spp_reassembly_t::default();
    let (mut written, mut next, mut remaining) = (0_usize, 0_u16, 0_usize);
    // SAFETY: as above.
    unsafe {
        t.check(
            aldis_spp_segmenter_init(
                seg_words.as_mut_ptr().cast(),
                bb(ALDIS_SPP_SEGMENTER_STORAGE_SIZE),
                &raw const spp,
                &raw const seg_cfg,
                sdu.as_ptr(),
                bb(3),
                &raw mut seg,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_spp_reassembler_init(
                ra_words.as_mut_ptr().cast(),
                bb(ALDIS_SPP_REASSEMBLER_STORAGE_SIZE),
                bb(9),
                &raw mut ra,
                &raw mut det,
            ) == ALDIS_OK,
        );
        for _ in 0..2 {
            t.check(
                aldis_spp_segmenter_next(
                    seg,
                    sdu.as_ptr(),
                    bb(3),
                    next,
                    null(),
                    0,
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut written,
                    &raw mut next,
                    &raw mut det,
                ) == ALDIS_OK,
            );
            t.check(
                aldis_spp_reassembler_push(
                    ra,
                    out.as_ptr(),
                    written,
                    bb(ALDIS_MODE_STRICT),
                    &raw const layout,
                    buf.as_mut_ptr(),
                    buf.len(),
                    &raw mut outcome,
                    &raw mut det,
                ) == ALDIS_OK,
            );
        }
        t.check(outcome.sdu_len == 3 && buf.get(..3) == Some(&[1, 2, 3][..]));
        t.check(
            aldis_spp_segmenter_remaining(seg, &raw mut remaining, &raw mut det) == ALDIS_OK
                && remaining == 0,
        );
        t.check(
            aldis_spp_reassembler_is_assembling(ra, &raw mut flag, &raw mut det) == ALDIS_OK
                && !flag,
        );
        t.check(aldis_spp_reassembler_reset(ra, &raw mut det) == ALDIS_OK);
        aldis_spp_segmenter_deinit(seg);
        aldis_spp_reassembler_deinit(ra);
        t.check(aldis_spp_reassembler_reset(ra, &raw mut det) == ALDIS_ERR_INVALID_HANDLE);
    }
}

#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one call per exported function")]
fn time(t: &mut Tally) {
    let mut det = detail();
    let cuc = bb([0x15_u8, 0x01, 0x02, 0x80]);
    let mut decoded = aldis_time_decoded_t::default();
    let mut out = [0_u8; 40];
    let mut written = 0_usize;
    let cuc_format = format(ALDIS_TIME_CUC);
    let options = aldis_time_ascii_options_t {
        struct_size: size::<aldis_time_ascii_options_t>(),
        max_fraction_digits: bb(12),
    };
    let text = bb(*b"2021-05-01T17:20:43.5Z");
    // SAFETY: every pointer is to a live local; lengths are those of the buffers.
    unsafe {
        t.check(
            aldis_time_decode(
                cuc.as_ptr(),
                bb(4),
                bb(ALDIS_MODE_STRICT),
                &raw mut decoded,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(decoded.value.coarse == 0x0102 && decoded.len == 4);
        t.check(
            aldis_time_decode_implicit(
                cuc.as_ptr().add(1),
                bb(3),
                &raw const cuc_format,
                bb(ALDIS_MODE_STRICT),
                &raw mut decoded,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_time_encode(
                &raw const decoded.format,
                &raw const decoded.value,
                bb(true),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK
                && out.get(..4) == Some(&cuc[..]),
        );
        let mut pfield = aldis_time_pfield_t::default();
        t.check(
            aldis_time_pfield_parse(cuc.as_ptr(), bb(4), &raw mut pfield, &raw mut det) == ALDIS_OK
                && pfield.len == 1,
        );
        t.check(
            aldis_time_format_pfield(
                &raw const cuc_format,
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK
                && written == 1,
        );
        t.check(
            aldis_time_format_t_len(&raw const cuc_format, &raw mut written, &raw mut det)
                == ALDIS_OK
                && written == 3,
        );
        let (mut seconds, mut nanos) = (0_u64, 0_u32);
        t.check(
            aldis_time_cuc_to_ns(
                &raw const decoded.value,
                &raw mut seconds,
                &raw mut nanos,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(seconds == 0x0102 && nanos == 500_000_000);
        let mut value = aldis_time_value_t::default();
        t.check(
            aldis_time_cuc_from_ns(
                &raw const cuc_format,
                bb(3),
                bb(500_000_000),
                &raw mut value,
                &raw mut det,
            ) == ALDIS_OK
                && value.fine[0] == 0x80,
        );
        let mut ascii = aldis_time_ascii_t::default();
        t.check(
            aldis_time_ascii_parse(
                text.as_ptr(),
                text.len(),
                &raw const options,
                &raw mut ascii,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_time_ascii_format(
                &raw const ascii,
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(out.get(..written) == Some(&text[..]));
        t.check(
            aldis_time_ascii_encoded_len(&raw const ascii, &raw mut written, &raw mut det)
                == ALDIS_OK
                && written == text.len(),
        );
        let mut subset = aldis_time_ascii_subset_t::default();
        t.check(
            aldis_time_ascii_subset_parse(
                text.as_ptr(),
                bb(10),
                &raw const options,
                &raw mut subset,
                &raw mut det,
            ) == ALDIS_OK,
        );
        let from = aldis_time_instant_t {
            format: format(ALDIS_TIME_ASCII),
            ascii,
            ..aldis_time_instant_t::default()
        };
        let cfg = aldis_time_convert_config_t {
            struct_size: size::<aldis_time_convert_config_t>(),
            to: aldis_time_format_t {
                subms: bb(1),
                ..format(ALDIS_TIME_CDS)
            },
            ..aldis_time_convert_config_t::default()
        };
        let mut converted = aldis_time_instant_t::default();
        t.check(
            aldis_time_convert(
                &raw const from,
                &raw const cfg,
                &raw mut converted,
                &raw mut det,
            ) == ALDIS_OK,
        );
        let ccs = aldis_time_convert_config_t {
            to: aldis_time_format_t {
                subsecond_octets: bb(1),
                ..format(ALDIS_TIME_CCS)
            },
            ..cfg
        };
        t.check(
            aldis_time_convert(
                &raw const converted,
                &raw const ccs,
                &raw mut converted,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(converted.value.year == 2021 && converted.value.subsecond == 50);
    }
}

#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one call per exported function")]
fn epp(t: &mut Tally) {
    let mut det = detail();
    let cfg = aldis_epp_config_t {
        struct_size: size::<aldis_epp_config_t>(),
        min_data_len: 0,
        max_data_len: bb(64),
        valid_epis: bb(0xFF),
        valid_extended_epis: bb(0xFFFF),
    };
    let fields = aldis_epp_fields_t {
        epi: bb(2),
        epi_extension: 0,
        user_defined: 0,
    };
    let data = bb([1_u8, 2, 3]);
    let mut out = [0_u8; 16];
    let (mut written, mut total) = (0_usize, 0_u32);
    let mut view = aldis_epp_packet_t::default();
    // SAFETY: every pointer is to a live local; lengths are those of the buffers.
    unsafe {
        t.check(
            aldis_epp_encode(
                &raw const cfg,
                &raw const fields,
                bb(ALDIS_EPP_HEADER_AUTO),
                data.as_ptr(),
                bb(3),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK
                && written == 5,
        );
        t.check(
            aldis_epp_decode(
                out.as_ptr(),
                written,
                bb(ALDIS_MODE_STRICT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK
                && view.data_len == 3,
        );
        t.check(
            aldis_epp_peek_length(out.as_ptr(), bb(2), &raw mut written, &raw mut det) == ALDIS_OK
                && written == 5,
        );
        t.check(
            aldis_epp_validate(
                out.as_ptr(),
                written,
                bb(ALDIS_MODE_STRICT),
                &raw const cfg,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_epp_check(&raw const cfg, bb(2), bb(false), 0, bb(3), &raw mut det) == ALDIS_OK,
        );
        let mut layout = aldis_epp_writer_layout_t::default();
        t.check(
            aldis_epp_writer_begin(
                &raw const cfg,
                &raw const fields,
                bb(4),
                bb(3),
                out.as_mut_ptr(),
                out.len(),
                &raw mut layout,
                &raw mut det,
            ) == ALDIS_OK
                && layout.data_offset == 4,
        );
        t.check(
            aldis_epp_writer_finish(
                &raw const cfg,
                &raw const fields,
                bb(4),
                bb(3),
                bb(2),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK
                && written == 6,
        );
        t.check(
            aldis_epp_header_encode(
                &raw const cfg,
                &raw const fields,
                bb(8),
                bb(3),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut total,
                &raw mut det,
            ) == ALDIS_OK
                && total == 11,
        );
        t.check(
            aldis_epp_idle(
                bb(4),
                bb(0x55),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_epp_idle_header(
                bb(300),
                out.as_mut_ptr(),
                out.len(),
                &raw mut written,
                &raw mut total,
                &raw mut det,
            ) == ALDIS_OK
                && written == 4,
        );
        t.check(aldis_epp_config_check(&raw const cfg, &raw mut det) == ALDIS_OK);
    }
}

/// Calls every exported function of `aldis-ffi`.
pub fn run(t: &mut Tally) {
    spp(t);
    spp_state(t);
    time(t);
    epp(t);
}
