//! Calls every function `aldis-ffi` exports, so that the panic-freedom link check covers the
//! C ABI (VER-010, M2 plan D16). Each call's status is checked on the target. Inputs pass
//! through `black_box`, so every path of each function stays reachable. The vector cases run
//! through the Rust API (`vectors.rs`); the C ABI's own vector run is the `aldis-ffi`
//! integration test on the host, so one call per function suffices here.

use core::hint::black_box as bb;
use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::crc::aldis_crc16;
use aldis_ffi::epp::{
    ALDIS_EPP_HEADER_AUTO, aldis_epp_check, aldis_epp_config_check, aldis_epp_config_t,
    aldis_epp_decode, aldis_epp_encode, aldis_epp_fields_t, aldis_epp_header_encode,
    aldis_epp_idle, aldis_epp_idle_header, aldis_epp_packet_t, aldis_epp_peek_length,
    aldis_epp_validate, aldis_epp_writer_begin, aldis_epp_writer_finish, aldis_epp_writer_layout_t,
};
use aldis_ffi::error::{
    ALDIS_ERR_INCOMPLETE, ALDIS_ERR_INVALID_FIELD, ALDIS_ERR_INVALID_HANDLE, ALDIS_OK,
};
use aldis_ffi::limits::{aldis_limits, aldis_limits_t};
use aldis_ffi::ocf::{ALDIS_OCF_CLCW, ALDIS_OCF_SDLS_FSR, aldis_ocf_check, aldis_ocf_classify};
use aldis_ffi::oid::{
    ALDIS_OID_FIBONACCI, ALDIS_OID_GALOIS, ALDIS_OID_GENERATOR_STORAGE_SIZE,
    aldis_oid_generator_deinit, aldis_oid_generator_fill, aldis_oid_generator_init,
    aldis_oid_generator_storage_align, aldis_oid_generator_storage_size, aldis_oid_generator_t,
};
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
use aldis_ffi::tm::{
    ALDIS_TM_CONTENT_PACKETS, ALDIS_TM_CONTENT_VCA_SDU, ALDIS_TM_POINTER_STARTS_AT,
    aldis_tm_config_check, aldis_tm_config_t, aldis_tm_content_t, aldis_tm_decode,
    aldis_tm_decode_unchecked, aldis_tm_decode_unchecked_with, aldis_tm_decode_with,
    aldis_tm_encode, aldis_tm_fields_t, aldis_tm_frame_t, aldis_tm_header_decode,
    aldis_tm_header_t, aldis_tm_layout_t, aldis_tm_mc_config_check, aldis_tm_mc_config_t,
    aldis_tm_oid_writer_begin, aldis_tm_oid_writer_finish, aldis_tm_peek_length, aldis_tm_validate,
    aldis_tm_vc_config_t, aldis_tm_writer_begin, aldis_tm_writer_finish,
};
// ---- USLP (M2 WP-5 module 9)
use aldis_ffi::uslp::{
    ALDIS_USLP_DELIMITED_FRAME, ALDIS_USLP_FRAME_FIXED, ALDIS_USLP_FRAME_VARIABLE,
    ALDIS_USLP_KIND_EXPEDITED, ALDIS_USLP_OID_VCID, ALDIS_USLP_REASON_NONE,
    ALDIS_USLP_RULE_NO_SEGMENTATION, ALDIS_USLP_RULE_OCTET_STREAM, ALDIS_USLP_UPID_OCTET_STREAM,
    aldis_uslp_channel_t, aldis_uslp_config_check, aldis_uslp_config_t, aldis_uslp_content_t,
    aldis_uslp_decode, aldis_uslp_delimit, aldis_uslp_delimited_t, aldis_uslp_encode,
    aldis_uslp_encode_oid, aldis_uslp_encode_truncated, aldis_uslp_fields_t, aldis_uslp_frame_t,
    aldis_uslp_layout_t, aldis_uslp_map_config_t, aldis_uslp_peek_length, aldis_uslp_tfdf_t,
    aldis_uslp_validate, aldis_uslp_vc_config_t, aldis_uslp_vc_table_check,
    aldis_uslp_writer_begin, aldis_uslp_writer_finish,
};
// ---- end USLP
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

/// The C ABI of `aldis-core`: the CRC-16, the OCF, and the OID generator in caller storage
/// (CMN-001, CMN-010, CMN-020…023; M2 plan D14).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
fn core(t: &mut Tally) {
    let mut det = detail();
    let check_string = bb(*b"123456789");
    let mut crc = 0_u16;
    let ocf = bb([0xC1_u8, 0x23, 0x45, 0x67]);
    let mut report = 0_u32;
    t.check(aldis_oid_generator_storage_size() == ALDIS_OID_GENERATOR_STORAGE_SIZE);
    t.check(aldis_oid_generator_storage_align() == 8);
    let mut fib_words = [0_u64; ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
    let mut gal_words = [0_u64; ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
    let mut fib: *mut aldis_oid_generator_t = null_mut();
    let mut gal: *mut aldis_oid_generator_t = null_mut();
    let (mut a, mut b) = ([0_u8; 6], [0_u8; 6]);
    // SAFETY: every pointer is to a live local; the storage is aligned and of the published
    // size, and the handles point into it.
    unsafe {
        t.check(
            aldis_crc16(check_string.as_ptr(), bb(9), &raw mut crc, &raw mut det) == ALDIS_OK
                && crc == 0x29B1,
        );
        t.check(
            aldis_ocf_classify(ocf.as_ptr(), bb(4), &raw mut report, &raw mut det) == ALDIS_OK
                && report == ALDIS_OCF_SDLS_FSR,
        );
        t.check(
            aldis_ocf_check(ocf.as_ptr(), bb(4), bb(ALDIS_OCF_CLCW), &raw mut det)
                == ALDIS_ERR_INVALID_FIELD
                && det.expected == 0
                && det.found == 1,
        );
        t.check(
            aldis_oid_generator_init(
                fib_words.as_mut_ptr().cast(),
                bb(ALDIS_OID_GENERATOR_STORAGE_SIZE),
                bb(ALDIS_OID_FIBONACCI),
                &raw mut fib,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_oid_generator_init(
                gal_words.as_mut_ptr().cast(),
                bb(ALDIS_OID_GENERATOR_STORAGE_SIZE),
                bb(ALDIS_OID_GALOIS),
                &raw mut gal,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(aldis_oid_generator_fill(fib, a.as_mut_ptr(), bb(6), &raw mut det) == ALDIS_OK);
        t.check(aldis_oid_generator_fill(gal, b.as_mut_ptr(), bb(6), &raw mut det) == ALDIS_OK);
        t.check(a == b && a == [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6]);
        aldis_oid_generator_deinit(fib);
        aldis_oid_generator_deinit(gal);
        t.check(
            aldis_oid_generator_fill(fib, a.as_mut_ptr(), bb(6), &raw mut det)
                == ALDIS_ERR_INVALID_HANDLE,
        );
    }
}

/// A 32-octet TM master channel with an FECF on SCID 0x0AB: VC 0 carries packets with SDLS 2/1,
/// VC 1 `VCA_SDUs` (132.0 §5; M2 plan D14).
fn tm_channels() -> (aldis_tm_mc_config_t, [aldis_tm_vc_config_t; 2]) {
    let mc = aldis_tm_mc_config_t {
        struct_size: size::<aldis_tm_mc_config_t>(),
        scid: bb(0x0AB),
        fsh_len: 0,
        ocf: 0,
        pc: aldis_tm_config_t {
            struct_size: size::<aldis_tm_config_t>(),
            fecf: bb(1),
            frame_len: bb(32),
        },
    };
    let vc = aldis_tm_vc_config_t {
        struct_size: size::<aldis_tm_vc_config_t>(),
        vcid: 0,
        content: ALDIS_TM_CONTENT_PACKETS,
        fsh_len: 0,
        ocf: 0,
        security_header_len: bb(2),
        security_trailer_len: bb(1),
    };
    let vca = aldis_tm_vc_config_t {
        vcid: 1,
        content: ALDIS_TM_CONTENT_VCA_SDU,
        security_header_len: 0,
        security_trailer_len: 0,
        ..vc
    };
    (mc, [vc, vca])
}

/// The C ABI of `aldis-tm` (TM-001…TM-011; M2 plan D14).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one call per exported function")]
fn tm(t: &mut Tally) {
    let (mc, vcs) = tm_channels();
    let mut det = detail();
    let fields = aldis_tm_fields_t {
        vcid: bb(0),
        pointer: ALDIS_TM_POINTER_STARTS_AT,
        packet_offset: bb(3),
        ..aldis_tm_fields_t::default()
    };
    let (sh, data, st) = ([0xA5_u8; 2], [0xD0_u8; 21], [0x5A_u8; 1]);
    let content = aldis_tm_content_t {
        security_header: sh.as_ptr(),
        security_header_len: bb(2),
        data: data.as_ptr(),
        data_len: bb(21),
        security_trailer: st.as_ptr(),
        security_trailer_len: bb(1),
        ..aldis_tm_content_t::default()
    };
    let mut frame = [0_u8; 32];
    let mut two = [0_u8; 32];
    let mut layout = aldis_tm_layout_t::default();
    let mut view = aldis_tm_frame_t::default();
    let mut header = aldis_tm_header_t::default();
    let mut n = 0_usize;
    let mut words = [0_u64; ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
    let mut oid: *mut aldis_oid_generator_t = null_mut();
    // SAFETY: every pointer is to a live local; lengths are those of the buffers; the
    // generator's storage is aligned and of the published size.
    unsafe {
        t.check(aldis_tm_config_check(&raw const mc.pc, &raw mut det) == ALDIS_OK);
        t.check(
            aldis_tm_mc_config_check(&raw const mc, vcs.as_ptr(), bb(2), &raw mut det) == ALDIS_OK,
        );
        t.check(
            aldis_tm_encode(
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                &raw const fields,
                &raw const content,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 32,
        );
        t.check(
            aldis_tm_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                &raw const fields,
                two.as_mut_ptr(),
                two.len(),
                &raw mut layout,
                &raw mut det,
            ) == ALDIS_OK
                && layout.data_offset == 8
                && layout.data_len == 21,
        );
        two[6..8].fill(0xA5);
        two[8..29].fill(0xD0);
        two[29] = 0x5A;
        t.check(
            aldis_tm_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                &raw const fields,
                two.as_mut_ptr(),
                two.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && two == frame,
        );
        t.check(
            aldis_tm_peek_length(
                frame.as_ptr(),
                bb(1),
                &raw const mc.pc,
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 32,
        );
        t.check(
            aldis_tm_header_decode(frame.as_ptr(), bb(5), &raw mut header, &raw mut det)
                == ALDIS_ERR_INCOMPLETE
                && det.needed == 6,
        );
        t.check(
            aldis_tm_header_decode(frame.as_ptr(), bb(6), &raw mut header, &raw mut det)
                == ALDIS_OK
                && header.scid == 0x0AB,
        );
        t.check(
            aldis_tm_decode(
                frame.as_ptr(),
                bb(32),
                &raw const mc.pc,
                bb(ALDIS_MODE_STRICT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK
                && view.layout.data_len == 24,
        );
        t.check(
            aldis_tm_decode_with(
                frame.as_ptr(),
                bb(32),
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                bb(ALDIS_MODE_LENIENT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK
                && view.layout.data_len == 21
                && view.pointer == ALDIS_TM_POINTER_STARTS_AT,
        );
        frame[10] ^= bb(1);
        t.check(
            aldis_tm_decode_unchecked(
                frame.as_ptr(),
                bb(32),
                &raw const mc.pc,
                bb(ALDIS_MODE_STRICT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_tm_decode_unchecked_with(
                frame.as_ptr(),
                bb(32),
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                bb(ALDIS_MODE_STRICT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK
                && view.has_fecf,
        );
        frame[10] ^= bb(1);
        t.check(
            aldis_tm_validate(
                frame.as_ptr(),
                bb(32),
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                bb(ALDIS_MODE_STRICT),
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_oid_generator_init(
                words.as_mut_ptr().cast(),
                bb(ALDIS_OID_GENERATOR_STORAGE_SIZE),
                bb(ALDIS_OID_FIBONACCI),
                &raw mut oid,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_tm_oid_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                bb(0),
                bb(0),
                bb(0),
                oid,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut layout,
                &raw mut det,
            ) == ALDIS_OK
                && frame.get(8..12) == Some(&[0xFF; 4][..]),
        );
        t.check(
            aldis_tm_oid_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                bb(2),
                bb(0),
                bb(0),
                bb(0),
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 32,
        );
        aldis_oid_generator_deinit(oid);
    }
}

// ---- USLP (M2 WP-5 module 9)

/// USLP channels on SCID 0x155 (732.1 §5; M2 plan D14): a 64-octet variable-length physical
/// channel with an FECF, whose VC 4 has truncated frames of 12 octets and MAP 0; and a 32-octet
/// fixed-length one with VC 63 for OID frames.
fn uslp_channels() -> (
    aldis_uslp_config_t,
    aldis_uslp_vc_config_t,
    aldis_uslp_map_config_t,
    aldis_uslp_config_t,
    aldis_uslp_vc_config_t,
) {
    let pc = aldis_uslp_config_t {
        struct_size: size::<aldis_uslp_config_t>(),
        frame_type: ALDIS_USLP_FRAME_VARIABLE,
        fecf: bb(1),
        frame_len: bb(64),
        insert_zone_len: 0,
    };
    let vc = aldis_uslp_vc_config_t {
        struct_size: size::<aldis_uslp_vc_config_t>(),
        scid: bb(0x155),
        vcid: bb(4),
        frame_type: ALDIS_USLP_FRAME_VARIABLE,
        truncated_frame_len: bb(12),
        ..aldis_uslp_vc_config_t::default()
    };
    let map = aldis_uslp_map_config_t {
        struct_size: size::<aldis_uslp_map_config_t>(),
        scid: bb(0x155),
        vcid: bb(4),
        ..aldis_uslp_map_config_t::default()
    };
    let fixed = aldis_uslp_config_t {
        frame_type: ALDIS_USLP_FRAME_FIXED,
        frame_len: bb(32),
        ..pc
    };
    let oid_vc = aldis_uslp_vc_config_t {
        vcid: ALDIS_USLP_OID_VCID,
        frame_type: ALDIS_USLP_FRAME_FIXED,
        truncated_frame_len: 0,
        ..vc
    };
    (pc, vc, map, fixed, oid_vc)
}

/// The C ABI of `aldis-uslp` (USLP-001…USLP-016; M2 plan D14).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one call per exported function")]
fn uslp(t: &mut Tally) {
    let (pc, vc, map, fixed, oid_vc) = uslp_channels();
    let mut det = detail();
    let fields = aldis_uslp_fields_t {
        channel: aldis_uslp_channel_t {
            scid: bb(0x155),
            vcid: bb(4),
            ..aldis_uslp_channel_t::default()
        },
        kind: ALDIS_USLP_KIND_EXPEDITED,
        vcf_count: 0,
    };
    let tfdz = [0xD0_u8; 3];
    let content = aldis_uslp_content_t {
        tfdf: aldis_uslp_tfdf_t {
            rule: ALDIS_USLP_RULE_OCTET_STREAM,
            upid: ALDIS_USLP_UPID_OCTET_STREAM,
            pointer: 0,
        },
        tfdz: tfdz.as_ptr(),
        tfdz_len: bb(3),
        ..aldis_uslp_content_t::default()
    };
    let in_place = aldis_uslp_content_t {
        tfdz: null(),
        tfdz_len: 0,
        ..content
    };
    let seven = [0x31_u8; 7];
    let truncated = aldis_uslp_content_t {
        tfdf: aldis_uslp_tfdf_t {
            rule: ALDIS_USLP_RULE_NO_SEGMENTATION,
            ..content.tfdf
        },
        tfdz: seven.as_ptr(),
        tfdz_len: bb(7),
        ..aldis_uslp_content_t::default()
    };
    let oid_fields = aldis_uslp_fields_t {
        channel: aldis_uslp_channel_t {
            vcid: ALDIS_USLP_OID_VCID,
            ..fields.channel
        },
        ..fields
    };
    let idle = aldis_uslp_content_t::default();
    let mut frame = [0_u8; 64];
    let mut two = [0_u8; 64];
    let mut layout = aldis_uslp_layout_t::default();
    let mut view = aldis_uslp_frame_t::default();
    let mut item = aldis_uslp_delimited_t::default();
    let mut reason = u8::MAX;
    let mut n = 0_usize;
    let mut words = [0_u64; ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
    let mut oid: *mut aldis_oid_generator_t = null_mut();
    // SAFETY: every pointer is to a live local; lengths are those of the buffers; the
    // generator's storage is aligned and of the published size.
    unsafe {
        t.check(aldis_uslp_config_check(&raw const pc, &raw mut det) == ALDIS_OK);
        t.check(
            aldis_uslp_vc_table_check(
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_uslp_encode(
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                &raw const fields,
                &raw const content,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 13,
        );
        t.check(
            aldis_uslp_writer_begin(
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                &raw const fields,
                &raw const in_place,
                bb(3),
                two.as_mut_ptr(),
                two.len(),
                &raw mut layout,
                &raw mut det,
            ) == ALDIS_OK
                && layout.tfdz_offset == 8,
        );
        two[8..11].fill(0xD0);
        t.check(
            aldis_uslp_writer_finish(
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                &raw const fields,
                &raw const in_place,
                bb(3),
                bb(3),
                two.as_mut_ptr(),
                two.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && two == frame,
        );
        t.check(
            aldis_uslp_peek_length(
                frame.as_ptr(),
                bb(6),
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 13,
        );
        t.check(
            aldis_uslp_decode(
                frame.as_ptr(),
                bb(13),
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                bb(ALDIS_MODE_STRICT),
                &raw mut view,
                &raw mut det,
            ) == ALDIS_OK
                && view.layout.tfdz_len == 3
                && !view.truncated,
        );
        t.check(
            aldis_uslp_validate(
                frame.as_ptr(),
                bb(13),
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                bb(ALDIS_MODE_LENIENT),
                &raw mut view,
                &raw mut reason,
                &raw mut det,
            ) == ALDIS_OK
                && reason == ALDIS_USLP_REASON_NONE,
        );
        t.check(
            aldis_uslp_delimit(
                frame.as_ptr(),
                bb(13),
                bb(0),
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw mut item,
                &raw mut det,
            ) == ALDIS_OK
                && item.kind == ALDIS_USLP_DELIMITED_FRAME
                && item.len == 13,
        );
        t.check(
            aldis_uslp_encode_truncated(
                &raw const pc,
                &raw const vc,
                bb(1),
                &raw const map,
                bb(1),
                &raw const fields.channel,
                &raw const truncated,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 12,
        );
        t.check(
            aldis_oid_generator_init(
                words.as_mut_ptr().cast(),
                bb(ALDIS_OID_GENERATOR_STORAGE_SIZE),
                bb(ALDIS_OID_GALOIS),
                &raw mut oid,
                &raw mut det,
            ) == ALDIS_OK,
        );
        t.check(
            aldis_uslp_encode_oid(
                &raw const fixed,
                &raw const oid_vc,
                bb(1),
                null(),
                0,
                &raw const oid_fields,
                &raw const idle,
                oid,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut n,
                &raw mut det,
            ) == ALDIS_OK
                && n == 32
                && frame.get(10..14) == Some(&[0xFF; 4][..]),
        );
        aldis_oid_generator_deinit(oid);
    }
}

// ---- end USLP

/// Calls every exported function of `aldis-ffi`.
pub fn run(t: &mut Tally) {
    core(t);
    spp(t);
    spp_state(t);
    time(t);
    epp(t);
    tm(t);
    uslp(t);
}
