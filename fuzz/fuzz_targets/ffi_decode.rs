//! The C ABI decoders agree with the Rust decoders on the same octets (M2 plan WP-7, D16):
//! `aldis_spp_decode`, `aldis_spp_peek_length`, `aldis_epp_decode`, `aldis_epp_peek_length`,
//! `aldis_time_decode`, `aldis_time_pfield_parse`, and `aldis_time_ascii_parse`, in both modes.
//! A success carries the same fields, offsets, lengths, and anomaly flags; a failure the same
//! status and the same detail: kind, field code, and values (CABI-012, CABI-020, CABI-021,
//! GEN-020). A decoded time code re-encodes through the C ABI to the octets the Rust encoder
//! writes. The C ABI of `aldis-core` agrees with it too: `aldis_crc16`, `aldis_ocf_classify`,
//! `aldis_ocf_check`, and the OID generator in both forms; and so does that of `aldis-tm`:
//! `aldis_tm_decode`, `aldis_tm_decode_unchecked`, `aldis_tm_peek_length`,
//! `aldis_tm_header_decode`, `aldis_tm_decode_with`, and `aldis_tm_validate`; and so does that
//! of `aldis-tc` and the CLCW (`tc_ffi`), and that of `aldis-uslp`: `aldis_uslp_vc_table_check`,
//! `aldis_uslp_decode`, `aldis_uslp_peek_length`, and `aldis_uslp_stream_next`.
//!
//! AOS (M2 WP-4 module 8): every input also runs through the C ABI of `aldis-aos`, which agrees
//! with it: `aldis_aos_fhec_encode`, `aldis_aos_fhec_decode`, `aldis_aos_mpdu_decode`,
//! `aldis_aos_bpdu_decode`, `aldis_aos_peek_length`, `aldis_aos_header_decode`, and
//! `aldis_aos_decode`, on a physical channel the selector configures.
#![no_main]

// req: CABI-012, CABI-020, CABI-021, CABI-022, GEN-020, API-010, VER-022, CMN-001, CMN-010, CMN-022, TM-001, API-070
// req: TC-001, TC-020, TC-021, TC-030
// req: AOS-010, AOS-013, AOS-014, AOS-017
// req: USLP-001, USLP-014, USLP-015

use core::ptr::null_mut;

use aldis_aos::{
    AosEdition, AosFrame, AosPhysicalChannelConfig, AosPrimaryHeader, AosVirtualChannelConfig,
    BitstreamPointer, Bpdu, DataUnit, Mpdu, VcTable,
};
use aldis_core::crc::crc16;
use aldis_core::ids::VcId;
use aldis_core::ocf::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, Error};
use aldis_epp::{EncapsulationPacket, EpiExtension, UserDefined};
use aldis_ffi::aos::{
    ALDIS_AOS_CONTENT_BPDU, ALDIS_AOS_CONTENT_IDLE_DATA, ALDIS_AOS_CONTENT_MPDU,
    ALDIS_AOS_CONTENT_VCA_SDU, ALDIS_AOS_EDITION_ISSUE4, ALDIS_AOS_EDITION_ISSUE5,
    ALDIS_AOS_POINTER_ALL_VALID, ALDIS_AOS_POINTER_NO_PACKET_START, ALDIS_AOS_POINTER_NONE,
    ALDIS_AOS_POINTER_ONLY_IDLE, ALDIS_AOS_POINTER_ONLY_IDLE_DATA, ALDIS_AOS_POINTER_STARTS_AT,
    ALDIS_AOS_POINTER_VALID_BITS, aldis_aos_bpdu_decode, aldis_aos_config_t,
    aldis_aos_corrected_header_t, aldis_aos_decode, aldis_aos_fhec_decode, aldis_aos_fhec_encode,
    aldis_aos_frame_t, aldis_aos_header_decode, aldis_aos_header_t, aldis_aos_mc_config_t,
    aldis_aos_mpdu_decode, aldis_aos_pdu_t, aldis_aos_peek_length, aldis_aos_vc_config_t,
};
use aldis_ffi::crc::aldis_crc16;
use aldis_ffi::epp::{aldis_epp_decode, aldis_epp_packet_t, aldis_epp_peek_length};
use aldis_ffi::error::{ALDIS_ERR_INVALID_FIELD, ALDIS_ERROR_DETAIL_MIN_SIZE};
use aldis_ffi::ocf::{
    ALDIS_OCF_CLCW, ALDIS_OCF_PROJECT_SPECIFIC, ALDIS_OCF_SDLS_FSR, aldis_ocf_check,
    aldis_ocf_classify,
};
use aldis_ffi::oid::{
    ALDIS_OID_FIBONACCI, ALDIS_OID_GALOIS, ALDIS_OID_GENERATOR_STORAGE_SIZE,
    aldis_oid_generator_deinit, aldis_oid_generator_fill, aldis_oid_generator_init,
};
use aldis_ffi::spp::{aldis_spp_decode, aldis_spp_packet_t, aldis_spp_peek_length};
use aldis_ffi::time::{
    aldis_time_ascii_options_t, aldis_time_ascii_parse, aldis_time_ascii_t, aldis_time_decode,
    aldis_time_decoded_t, aldis_time_encode, aldis_time_pfield_parse, aldis_time_pfield_t,
};
use aldis_ffi::tm::{
    ALDIS_TM_CONTENT_PACKETS, ALDIS_TM_POINTER_NO_PACKET_START, ALDIS_TM_POINTER_NONE,
    ALDIS_TM_POINTER_ONLY_IDLE_DATA, ALDIS_TM_POINTER_STARTS_AT, aldis_tm_config_t,
    aldis_tm_decode, aldis_tm_decode_unchecked, aldis_tm_decode_with, aldis_tm_frame_t,
    aldis_tm_header_decode, aldis_tm_header_t, aldis_tm_mc_config_t, aldis_tm_peek_length,
    aldis_tm_validate, aldis_tm_vc_config_t,
};
use aldis_ffi::{ALDIS_MODE_LENIENT, ALDIS_MODE_STRICT, ALDIS_OK, aldis_error_detail_t};
use aldis_spp::SpacePacket;
use aldis_time::{AsciiOptions, AsciiTime, PField, decode_with_pfield};
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FshLen, NoSdls, PacketPointer, TmFrame,
    TmMasterChannelConfig, TmPhysicalChannelConfig, TmPrimaryHeader, TmVirtualChannelConfig,
};
// ---- USLP (M2 WP-5 module 9)
use aldis_ffi::uslp::{
    ALDIS_USLP_FRAME_FIXED, ALDIS_USLP_FRAME_VARIABLE, ALDIS_USLP_KIND_EXPEDITED,
    ALDIS_USLP_KIND_PROTOCOL_CONTROL, ALDIS_USLP_KIND_SEQUENCE_CONTROLLED, ALDIS_USLP_OCF_ABSENT,
    ALDIS_USLP_OCF_ALLOWED, ALDIS_USLP_OCF_REQUIRED, ALDIS_USLP_SECURITY_PER_MAP,
    ALDIS_USLP_SECURITY_VC, ALDIS_USLP_STREAM_ACCEPTED, ALDIS_USLP_STREAM_DISCARDED,
    ALDIS_USLP_STREAM_END, aldis_uslp_config_t, aldis_uslp_decode, aldis_uslp_frame_t,
    aldis_uslp_map_config_t, aldis_uslp_peek_length, aldis_uslp_stream_item_t,
    aldis_uslp_stream_next, aldis_uslp_vc_config_t, aldis_uslp_vc_table_check,
};
use aldis_uslp::{
    FrameKind, FrameStream as UslpFrameStream, IntakeItem as UslpIntakeItem, MapConfig, OcfUse,
    PhysicalChannelConfig, SecurityScope, TransferFrameType, TruncatedFrameLen, Upid, UslpFrame,
    UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable as UslpVcTable, VcfCountLen,
};
// ---- end USLP
use libfuzzer_sys::fuzz_target;

/// The status and detail a C call returns for a Rust result.
fn expect<T>(rust: &Result<T, Error>, status: i32, detail: &aldis_error_detail_t) {
    match rust {
        Ok(_) => assert_eq!(status, ALDIS_OK),
        Err(e) => {
            assert_eq!(status, -i32::from(e.kind().code()));
            assert_eq!(*detail, aldis_error_detail_t::from(*e));
        }
    }
}

#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
fn spp(data: &[u8], mode: DecodeMode, c_mode: u32) {
    let rust = SpacePacket::decode(data, mode);
    let mut view = aldis_spp_packet_t::default();
    let mut detail = aldis_error_detail_t::default();
    // SAFETY: `data`, `view`, and `detail` are live.
    let status = unsafe {
        aldis_spp_decode(
            data.as_ptr(),
            data.len(),
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    // With only the fixed prefix of the detail, the same status and prefix, nothing after it.
    let mut short = aldis_error_detail_t {
        struct_size: ALDIS_ERROR_DETAIL_MIN_SIZE,
        needed: u64::MAX,
        ..aldis_error_detail_t::default()
    };
    // SAFETY: as above.
    let again = unsafe {
        aldis_spp_decode(
            data.as_ptr(),
            data.len(),
            c_mode,
            &raw mut view,
            &raw mut short,
        )
    };
    assert_eq!(again, status);
    assert_eq!(
        (short.kind, short.field, short.detail),
        (detail.kind, detail.field, detail.detail)
    );
    assert_eq!(
        (short.struct_size, short.needed),
        (ALDIS_ERROR_DETAIL_MIN_SIZE, u64::MAX)
    );
    if let Ok(p) = rust {
        assert_eq!(
            (
                view.fields.apid,
                view.fields.sequence_count,
                view.fields.sequence_flags
            ),
            (
                p.apid().get(),
                p.sequence_count().get(),
                p.sequence_flags().bits()
            )
        );
        assert_eq!(
            (view.secondary_header_flag, view.idle, view.anomalies),
            (p.has_secondary_header(), p.is_idle(), p.anomalies().bits())
        );
        assert_eq!(&data[view.data_offset..][..view.data_len], p.data_field());
        assert_eq!(view.packet_len, p.len());
    }
    let rust = SpacePacket::peek_length(data);
    let mut len = 0;
    // SAFETY: as above.
    let status =
        unsafe { aldis_spp_peek_length(data.as_ptr(), data.len(), &raw mut len, &raw mut detail) };
    expect(&rust, status, &detail);
    if let Ok(n) = rust {
        assert_eq!(len, n);
    }
}

#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
fn epp(data: &[u8], mode: DecodeMode, c_mode: u32) {
    let rust = EncapsulationPacket::decode(data, mode);
    let mut view = aldis_epp_packet_t::default();
    let mut detail = aldis_error_detail_t::default();
    // SAFETY: `data`, `view`, and `detail` are live.
    let status = unsafe {
        aldis_epp_decode(
            data.as_ptr(),
            data.len(),
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(p) = rust {
        assert_eq!(view.epi, p.epi().get());
        assert_eq!(usize::from(view.header_len), p.header_len().octets());
        assert_eq!(view.has_extension, p.extension().is_some());
        assert_eq!(
            view.epi_extension,
            p.extension().map_or(0, EpiExtension::get)
        );
        assert_eq!(
            view.user_defined,
            p.user_defined().map_or(0, UserDefined::get)
        );
        assert_eq!(view.ccsds_defined, p.ccsds_defined().unwrap_or(0));
        assert_eq!(
            (view.packet_len, view.anomalies),
            (p.header().packet_len(), p.anomalies().bits())
        );
        assert_eq!(&data[view.data_offset..][..view.data_len], p.data());
    }
    let rust = EncapsulationPacket::peek_length(data);
    let mut len = 0;
    // SAFETY: as above.
    let status =
        unsafe { aldis_epp_peek_length(data.as_ptr(), data.len(), &raw mut len, &raw mut detail) };
    expect(&rust, status, &detail);
    if let Ok(n) = rust {
        assert_eq!(len, n);
    }
}

#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
fn time(data: &[u8], mode: DecodeMode, c_mode: u32) {
    let rust = decode_with_pfield(data, mode);
    let mut d = aldis_time_decoded_t::default();
    let mut detail = aldis_error_detail_t::default();
    // SAFETY: `data`, `d`, and `detail` are live.
    let status = unsafe {
        aldis_time_decode(
            data.as_ptr(),
            data.len(),
            c_mode,
            &raw mut d,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(r) = rust {
        assert_eq!((d.len, d.anomalies), (r.len(), r.anomalies().bits()));
        assert_eq!(
            &d.pfield[..usize::from(d.pfield_len)],
            &data[..r.pfield().map_or(0, |p| p.len())]
        );
        if r.code().format().is_some() {
            // The C format and value encode to what the Rust code encodes.
            let mut rust_out = [0_u8; 32];
            let n = r.code().encode(true, &mut rust_out).unwrap();
            let mut c_out = [0_u8; 32];
            let mut m = 0;
            // SAFETY: every pointer is to a live local.
            let status = unsafe {
                aldis_time_encode(
                    &raw const d.format,
                    &raw const d.value,
                    true,
                    c_out.as_mut_ptr(),
                    c_out.len(),
                    &raw mut m,
                    &raw mut detail,
                )
            };
            assert_eq!(status, ALDIS_OK);
            assert_eq!(&c_out[..m], &rust_out[..n]);
        }
    }
    let rust = PField::parse(data);
    let mut p = aldis_time_pfield_t::default();
    // SAFETY: as above.
    let status =
        unsafe { aldis_time_pfield_parse(data.as_ptr(), data.len(), &raw mut p, &raw mut detail) };
    expect(&rust, status, &detail);
    if let Ok(r) = rust {
        assert_eq!(usize::from(p.len), r.len());
        assert_eq!(
            (p.time_code_id, p.reserved),
            (r.id().bits(), r.id().is_reserved())
        );
    }
    let rust = AsciiTime::parse(data, &AsciiOptions::DEFAULT);
    let options = aldis_time_ascii_options_t {
        struct_size: u32::try_from(size_of::<aldis_time_ascii_options_t>()).unwrap(),
        max_fraction_digits: 12,
    };
    let mut a = aldis_time_ascii_t::default();
    // SAFETY: as above.
    let status = unsafe {
        aldis_time_ascii_parse(
            data.as_ptr(),
            data.len(),
            &raw const options,
            &raw mut a,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(t) = rust {
        assert_eq!(
            (a.year, a.hour, a.minute, a.second),
            (
                t.date.year().get(),
                t.hour.get(),
                t.minute.get(),
                t.second.get()
            )
        );
        assert_eq!(
            (a.fraction, a.fraction_digits, a.utc == 1),
            (t.fraction.value(), t.fraction.digits(), t.utc)
        );
    }
}

/// The C ABI of `aldis-core` (M2 plan D14): `aldis_crc16` agrees with `crc16`;
/// `aldis_ocf_classify` with `Ocf::decode` on four octets and rejects any other length;
/// `aldis_ocf_check` with the checked report constructors, with the same detail; and a fill of
/// the input's length from each OID form with `OidGenerator::fill` (CMN-001, CMN-010, CMN-020,
/// CMN-022, CMN-023).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
fn core(data: &[u8]) {
    let mut crc = 0_u16;
    // SAFETY: `data` and `crc` are live; NULL detail is allowed.
    let status = unsafe { aldis_crc16(data.as_ptr(), data.len(), &raw mut crc, null_mut()) };
    assert_eq!((status, crc), (ALDIS_OK, crc16(data)));

    let octets = <[u8; OCF_LEN]>::try_from(data).ok();
    let mut report = u32::MAX;
    let mut detail = aldis_error_detail_t::default();
    // SAFETY: as above.
    let status =
        unsafe { aldis_ocf_classify(data.as_ptr(), data.len(), &raw mut report, &raw mut detail) };
    match octets.map(Ocf::decode) {
        Some(ocf) => {
            assert_eq!(status, ALDIS_OK);
            let expected = match ocf {
                Ocf::Clcw(_) => ALDIS_OCF_CLCW,
                Ocf::ProjectSpecific(_) => ALDIS_OCF_PROJECT_SPECIFIC,
                Ocf::SdlsFsr(_) => ALDIS_OCF_SDLS_FSR,
            };
            assert_eq!(report, expected);
        }
        None => assert_eq!(
            (status, report, detail.field),
            (ALDIS_ERR_INVALID_FIELD, u32::MAX, 0)
        ),
    }
    for c_report in [
        ALDIS_OCF_CLCW,
        ALDIS_OCF_PROJECT_SPECIFIC,
        ALDIS_OCF_SDLS_FSR,
    ] {
        let mut detail = aldis_error_detail_t::default();
        // SAFETY: as above.
        let status =
            unsafe { aldis_ocf_check(data.as_ptr(), data.len(), c_report, &raw mut detail) };
        match octets {
            Some(o) => {
                let rust = match c_report {
                    ALDIS_OCF_CLCW => Type1Report::try_from(o).map(drop),
                    ALDIS_OCF_PROJECT_SPECIFIC => ProjectSpecificReport::try_from(o).map(drop),
                    _ => FsrReport::try_from(o).map(drop),
                };
                expect(&rust, status, &detail);
            }
            None => assert_eq!((status, detail.field), (ALDIS_ERR_INVALID_FIELD, 0)),
        }
    }

    let mut rust = vec![0_u8; data.len()];
    OidGenerator::new().fill(&mut rust);
    for form in [ALDIS_OID_FIBONACCI, ALDIS_OID_GALOIS] {
        let mut storage = [0_u64; ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
        let mut h = null_mut();
        let mut out = vec![0_u8; data.len()];
        // SAFETY: `storage` is live, aligned, and of the published size; `h` points into it;
        // `out` is live.
        unsafe {
            let status = aldis_oid_generator_init(
                storage.as_mut_ptr().cast(),
                ALDIS_OID_GENERATOR_STORAGE_SIZE,
                form,
                &raw mut h,
                null_mut(),
            );
            assert_eq!(status, ALDIS_OK);
            let status = aldis_oid_generator_fill(h, out.as_mut_ptr(), out.len(), null_mut());
            assert_eq!(status, ALDIS_OK);
            aldis_oid_generator_deinit(h);
        }
        assert_eq!(out, rust);
    }
}

/// The C view of a TM frame agrees with the Rust view: header, pointer, Secondary Header
/// length, FECF, anomalies, and every field's offset and length (CABI-012).
fn tm_same(view: &aldis_tm_frame_t, frame: &TmFrame<'_>) {
    let rust = frame.header();
    let c_header = &view.header;
    assert_eq!(
        (
            c_header.scid,
            c_header.vcid,
            c_header.ocf_flag,
            c_header.mc_frame_count,
            c_header.vc_frame_count
        ),
        (
            rust.scid.get(),
            rust.vcid.get(),
            rust.ocf_flag,
            rust.mc_frame_count,
            rust.vc_frame_count
        )
    );
    assert_eq!(
        (
            c_header.fsh_flag,
            c_header.sync_flag,
            c_header.packet_order_flag
        ),
        (rust.fsh_flag, rust.sync_flag, rust.packet_order_flag)
    );
    assert_eq!(
        (c_header.segment_length_id, c_header.first_header_pointer),
        (
            rust.segment_length_id.get(),
            rust.first_header_pointer.get()
        )
    );
    let pointer = match frame.data_field_status() {
        DataFieldStatus::Packets(PacketPointer::StartsAt(n)) => {
            (ALDIS_TM_POINTER_STARTS_AT, n.get())
        }
        DataFieldStatus::Packets(PacketPointer::NoPacketStart) => {
            (ALDIS_TM_POINTER_NO_PACKET_START, 0)
        }
        DataFieldStatus::Packets(PacketPointer::OnlyIdleData) => {
            (ALDIS_TM_POINTER_ONLY_IDLE_DATA, 0)
        }
        DataFieldStatus::VcaSdu { .. } => (ALDIS_TM_POINTER_NONE, 0),
    };
    assert_eq!((view.pointer, view.packet_offset), pointer);
    assert_eq!(view.fsh_len, frame.fsh_len().map_or(0, FshLen::get));
    assert_eq!(view.has_fecf.then_some(view.fecf), frame.fecf());
    assert_eq!(view.anomalies, frame.anomalies().bits());
    let c_layout = &view.layout;
    let ranges = frame.layout();
    let span = |at: usize, len: usize| at..at + len;
    assert_eq!(c_layout.frame_len, frame.len());
    let fsh = ranges.secondary();
    assert_eq!(
        span(c_layout.fsh_data_offset, c_layout.fsh_data_len),
        (fsh.start + 1).min(fsh.end)..fsh.end
    );
    assert_eq!(
        span(
            c_layout.security_header_offset,
            c_layout.security_header_len
        ),
        ranges.security_header()
    );
    assert_eq!(span(c_layout.data_offset, c_layout.data_len), ranges.data());
    assert_eq!(
        span(
            c_layout.security_trailer_offset,
            c_layout.security_trailer_len
        ),
        ranges.security_trailer()
    );
    assert_eq!(span(c_layout.ocf_offset, c_layout.ocf_len), ranges.ocf());
    assert_eq!(span(c_layout.fecf_offset, c_layout.fecf_len), ranges.fecf());
}

/// The C ABI of `aldis-tm` agrees with `aldis-tm` on a frame of the input's length (M2 plan
/// D14): `aldis_tm_decode` and `aldis_tm_decode_unchecked` without SDLS, `aldis_tm_peek_length`,
/// and `aldis_tm_header_decode`; and, on a master channel of the frame's SCID whose eight VCs
/// carry packets with SDLS layouts of 0 to 2 header and 0 or 1 trailer octets,
/// `aldis_tm_decode_with` and `aldis_tm_validate` (TM-001…TM-008, TM-011, API-070).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one comparison per C function")]
fn tm(data: &[u8], fecf: bool, mode: DecodeMode, c_mode: u32) {
    let pc = aldis_tm_config_t {
        struct_size: u32::try_from(size_of::<aldis_tm_config_t>()).unwrap(),
        fecf: u8::from(fecf),
        frame_len: data.len(),
    };
    let rust_pc = TmPhysicalChannelConfig::new(data.len(), fecf);
    let mut view = aldis_tm_frame_t::default();
    let mut detail = aldis_error_detail_t::default();
    let rust = rust_pc.and_then(|p| TmFrame::decode(data, &p, mode));
    // SAFETY: `data`, `pc`, `view`, and `detail` are live.
    let status = unsafe {
        aldis_tm_decode(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(f) = &rust {
        tm_same(&view, f);
    }
    let rust = rust_pc.and_then(|p| TmFrame::decode_unchecked(data, &p, &NoSdls, mode));
    // SAFETY: as above.
    let status = unsafe {
        aldis_tm_decode_unchecked(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(f) = &rust {
        tm_same(&view, f);
    }
    let rust = rust_pc.and_then(|p| TmFrame::peek_length(data, &p));
    let mut len = 0;
    // SAFETY: as above.
    let status = unsafe {
        aldis_tm_peek_length(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            &raw mut len,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(n) = rust {
        assert_eq!(len, n);
    }
    let rust = TmPrimaryHeader::decode(data);
    let mut header = aldis_tm_header_t::default();
    // SAFETY: as above.
    let status = unsafe {
        aldis_tm_header_decode(data.as_ptr(), data.len(), &raw mut header, &raw mut detail)
    };
    expect(&rust, status, &detail);
    let Ok(h) = rust else {
        return;
    };
    assert_eq!((header.scid, header.vcid), (h.scid.get(), h.vcid.get()));
    // A master channel of the frame's SCID; VC n has a Security Header of n % 3 octets and a
    // Trailer of n % 2.
    let mc = aldis_tm_mc_config_t {
        struct_size: u32::try_from(size_of::<aldis_tm_mc_config_t>()).unwrap(),
        scid: h.scid.get(),
        fsh_len: 0,
        ocf: 0,
        pc,
    };
    let vcs: [aldis_tm_vc_config_t; 8] = core::array::from_fn(|n| aldis_tm_vc_config_t {
        struct_size: u32::try_from(size_of::<aldis_tm_vc_config_t>()).unwrap(),
        vcid: u8::try_from(n).unwrap(),
        content: ALDIS_TM_CONTENT_PACKETS,
        fsh_len: 0,
        ocf: 0,
        security_header_len: u8::try_from(n % 3).unwrap(),
        security_trailer_len: u16::try_from(n % 2).unwrap(),
    });
    let rust_master = rust_pc.and_then(|p| {
        let mut m = TmMasterChannelConfig::new(p, h.scid, None, false);
        for n in 0..8_u8 {
            let security = SecurityLayout::new(usize::from(n % 3), usize::from(n % 2))?;
            let vc = TmVirtualChannelConfig::new(VcId::new(n).unwrap(), DataFieldContent::Packets)
                .with_security(security);
            m = m.with_vc(vc)?;
        }
        Ok(m)
    });
    let rust = rust_master
        .as_ref()
        .map_err(|e| *e)
        .and_then(|m| TmFrame::decode_with(data, m.pc(), m, mode));
    // SAFETY: as above; `vcs` holds 8 structs.
    let status = unsafe {
        aldis_tm_decode_with(
            data.as_ptr(),
            data.len(),
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(f) = &rust {
        tm_same(&view, f);
    }
    let rust = rust_master.and_then(|m| TmFrame::decode_with(data, m.pc(), &m, mode)?.validate(&m));
    // SAFETY: as above.
    let status = unsafe {
        aldis_tm_validate(
            data.as_ptr(),
            data.len(),
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            c_mode,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
}

// ---------------------------------------------------------------- TC (M2 WP-3 module 8)

/// The C ABI of `aldis-tc` agrees with `aldis-tc` on the input (M2 plan D14):
/// `aldis_tc_peek_length` and `aldis_tc_header_decode`; on a master channel of the frame's SCID
/// with eight VCs, the odd ones with Segment Headers and four MAPs each, the even ones with
/// SDLS layouts of `n % 5` header and `n % 3` trailer octets, `aldis_tc_mc_config_check`,
/// `aldis_tc_decode`, and `aldis_tc_stream_next` over the input; `aldis_tc_control_decode`;
/// and `aldis_clcw_decode` on the first four octets (TC-001…TC-006, TC-020…TC-022, TC-030, API-070).
mod tc_ffi {
    use core::ptr::null_mut;

    use aldis_core::security::SecurityLayout;
    use aldis_core::{DecodeMode, Error};
    use aldis_ffi::tc::{
        ALDIS_TC_CONTROL_NONE, ALDIS_TC_CONTROL_SET_VR, ALDIS_TC_CONTROL_UNLOCK, ALDIS_TC_FRAME_AD,
        ALDIS_TC_FRAME_BC, ALDIS_TC_FRAME_BD, ALDIS_TC_STREAM_ACCEPTED, ALDIS_TC_STREAM_DISCARDED,
        ALDIS_TC_STREAM_END, aldis_clcw_decode, aldis_clcw_t, aldis_tc_config_t,
        aldis_tc_control_decode, aldis_tc_control_t, aldis_tc_decode, aldis_tc_frame_t,
        aldis_tc_header_decode, aldis_tc_header_t, aldis_tc_map_config_t, aldis_tc_mc_config_check,
        aldis_tc_mc_config_t, aldis_tc_peek_length, aldis_tc_stream_item_t, aldis_tc_stream_next,
        aldis_tc_vc_config_t,
    };
    use aldis_ffi::{ALDIS_OK, aldis_error_detail_t};
    use aldis_tc::{
        Clcw, ControlCommand, FrameStream, FrameType, IntakeItem, MapConfig, PhysicalChannelConfig,
        SegmentHeaderUse, TcFrame, TcFrameHeader, TcMapId, TcSpacecraftId, TcVcId, VcConfig,
        VcTable,
    };

    use super::expect;

    fn size<T>() -> u32 {
        u32::try_from(size_of::<T>()).unwrap()
    }

    /// The master channel of `scid` as C structs: VC `n` of 0 to 7, VC 0 with frames of at most
    /// 32 octets; MAPs 0 to 3 on each odd VC, MAP `m` with SDLS `m % 3` / `(m + 1) % 2`.
    fn c_channels(
        scid: u16,
        fecf: bool,
    ) -> (
        aldis_tc_mc_config_t,
        Vec<aldis_tc_vc_config_t>,
        Vec<aldis_tc_map_config_t>,
    ) {
        let mc = aldis_tc_mc_config_t {
            struct_size: size::<aldis_tc_mc_config_t>(),
            scid,
            pc: aldis_tc_config_t {
                struct_size: size::<aldis_tc_config_t>(),
                fecf: u8::from(fecf),
                max_frame_len: 1024,
            },
        };
        let vcs = (0..8_u8)
            .map(|n| aldis_tc_vc_config_t {
                struct_size: size::<aldis_tc_vc_config_t>(),
                vcid: n,
                segment_header: n % 2,
                security_header_len: if n % 2 == 0 { n % 5 } else { 0 },
                security_trailer_len: if n % 2 == 0 { u16::from(n % 3) } else { 0 },
                max_frame_len: if n == 0 { 32 } else { 1024 },
            })
            .collect();
        let maps = (0..8_u8)
            .filter(|n| n % 2 == 1)
            .flat_map(|vcid| {
                (0..4_u8).map(move |m| aldis_tc_map_config_t {
                    struct_size: size::<aldis_tc_map_config_t>(),
                    vcid,
                    map_id: m,
                    security_header_len: m % 3,
                    security_trailer_len: u16::from((m + 1) % 2),
                })
            })
            .collect();
        (mc, vcs, maps)
    }

    /// The same master channel in Rust.
    fn rust_channels(scid: u16, fecf: bool) -> Result<VcTable<64, 64>, Error> {
        let pc = PhysicalChannelConfig::new(1024, fecf)?;
        let s = TcSpacecraftId::try_from(scid)?;
        let mut vcs = Vec::new();
        for n in 0..8_u8 {
            let use_ = if n % 2 == 1 {
                SegmentHeaderUse::Present
            } else {
                SegmentHeaderUse::Absent(SecurityLayout::new(
                    usize::from(n % 5),
                    usize::from(n % 3),
                )?)
            };
            let max = if n == 0 { 32 } else { 1024 };
            vcs.push(VcConfig::new(s, TcVcId::try_from(n)?, max, use_)?);
        }
        let mut maps = Vec::new();
        for vcid in (0..8_u8).filter(|n| n % 2 == 1) {
            for m in 0..4_u8 {
                let security = SecurityLayout::new(usize::from(m % 3), usize::from((m + 1) % 2))?;
                maps.push(MapConfig::new(
                    s,
                    TcVcId::try_from(vcid)?,
                    TcMapId::try_from(m)?,
                    security,
                ));
            }
        }
        VcTable::new(pc, &vcs, &maps)
    }

    fn same_header(c: &aldis_tc_header_t, h: &TcFrameHeader) {
        let frame_type = match h.frame_type() {
            FrameType::Ad => ALDIS_TC_FRAME_AD,
            FrameType::Bd => ALDIS_TC_FRAME_BD,
            FrameType::Bc => ALDIS_TC_FRAME_BC,
        };
        assert_eq!(
            (c.frame_type, c.scid, c.vcid, c.sequence_number, c.frame_len),
            (
                frame_type,
                h.spacecraft_id().get(),
                h.vc_id().get(),
                h.sequence_number().get(),
                h.frame_len()
            )
        );
    }

    /// The offset of `part` in `whole`, which holds it.
    fn at(whole: &[u8], part: &[u8]) -> usize {
        part.as_ptr() as usize - whole.as_ptr() as usize
    }

    /// The C view agrees with the Rust view: header, Segment Header, Control Command, FECF,
    /// anomalies, and every field's offset and length (CABI-012).
    fn same(view: &aldis_tc_frame_t, data: &[u8], f: &TcFrame<'_>) {
        same_header(&view.header, &f.header());
        let sh = f.segment_header();
        assert_eq!(view.has_segment_header, sh.is_some());
        assert_eq!(
            (view.sequence_flags, view.map_id),
            sh.map_or((0, 0), |s| (s.sequence_flags.bits(), s.map_id.get()))
        );
        let control = match f.control_command() {
            None => (ALDIS_TC_CONTROL_NONE, 0),
            Some(ControlCommand::Unlock) => (ALDIS_TC_CONTROL_UNLOCK, 0),
            Some(ControlCommand::SetVr(vr)) => (ALDIS_TC_CONTROL_SET_VR, vr.get()),
        };
        assert_eq!((view.control.command, view.control.set_vr_value), control);
        assert_eq!(view.has_fecf.then_some(view.fecf), f.fecf());
        assert_eq!(view.anomalies, f.anomalies().bits());
        let l = &view.layout;
        assert_eq!(l.frame_len, f.len());
        assert_eq!(l.segment_header_len, usize::from(sh.is_some()));
        assert_eq!(
            (l.security_header_offset, l.security_header_len),
            (at(data, f.security_header()), f.security_header().len())
        );
        assert_eq!(
            (l.data_offset, l.data_len),
            (at(data, f.data_field()), f.data_field().len())
        );
        assert_eq!(
            (l.security_trailer_offset, l.security_trailer_len),
            (at(data, f.security_trailer()), f.security_trailer().len())
        );
        assert_eq!(l.fecf_offset + l.fecf_len, f.len());
    }

    #[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
    #[expect(clippy::too_many_lines, reason = "one comparison per C function")]
    pub(super) fn run(data: &[u8], fecf: bool, mode: DecodeMode, c_mode: u32) {
        let mut detail = aldis_error_detail_t::default();
        let rust = TcFrame::peek_length(data);
        let mut len = 0;
        // SAFETY: `data`, `len`, and `detail` are live.
        let status = unsafe {
            aldis_tc_peek_length(data.as_ptr(), data.len(), &raw mut len, &raw mut detail)
        };
        expect(&rust, status, &detail);
        if let Ok(n) = rust {
            assert_eq!(len, n);
        }
        let rust = TcFrame::peek_header(data);
        let mut header = aldis_tc_header_t::default();
        // SAFETY: as above.
        let status = unsafe {
            aldis_tc_header_decode(data.as_ptr(), data.len(), &raw mut header, &raw mut detail)
        };
        expect(&rust, status, &detail);
        if let Ok(h) = &rust {
            same_header(&header, h);
        }
        // A master channel of the frame's SCID, so that the checks after (b) run.
        let scid = rust.map_or(0x155, |h| h.spacecraft_id().get());
        let (mc, vcs, maps) = c_channels(scid, fecf);
        let table = rust_channels(scid, fecf);
        // SAFETY: every pointer is to a live local or vector, with its length.
        let status = unsafe {
            aldis_tc_mc_config_check(
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                &raw mut detail,
            )
        };
        expect(&table, status, &detail);
        let table = table.unwrap();
        let rust = TcFrame::decode(data, &table, mode);
        let mut view = aldis_tc_frame_t::default();
        // SAFETY: as above.
        let status = unsafe {
            aldis_tc_decode(
                data.as_ptr(),
                data.len(),
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                c_mode,
                &raw mut view,
                &raw mut detail,
            )
        };
        expect(&rust, status, &detail);
        if let Ok(f) = &rust {
            same(&view, data, f);
        }

        let mut stream_offset = 0;
        let mut rust_stream = FrameStream::new(data, &table, mode);
        loop {
            let mut item = aldis_tc_stream_item_t::default();
            // SAFETY: as above.
            let status = unsafe {
                aldis_tc_stream_next(
                    data.as_ptr(),
                    data.len(),
                    &raw mut stream_offset,
                    &raw const mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    maps.as_ptr(),
                    maps.len(),
                    c_mode,
                    &raw mut item,
                    null_mut(),
                )
            };
            assert_eq!(status, ALDIS_OK);
            match rust_stream.next() {
                None => {
                    assert_eq!(item.kind, ALDIS_TC_STREAM_END);
                    break;
                }
                Some(IntakeItem::Accepted { frame, .. }) => {
                    assert_eq!(item.kind, ALDIS_TC_STREAM_ACCEPTED);
                    assert_eq!(item.len, frame.len());
                    same(&item.frame, frame.octets(), &frame);
                }
                Some(IntakeItem::Discarded { reason, octets, .. }) => {
                    assert_eq!(item.kind, ALDIS_TC_STREAM_DISCARDED);
                    assert_eq!(item.len, octets.len());
                    assert_eq!(item.discard_reason, u32::from(reason.code()));
                }
            }
        }
        assert_eq!(stream_offset, data.len());
        let rust = ControlCommand::parse(data);
        let mut control = aldis_tc_control_t::default();
        // SAFETY: as above.
        let status = unsafe {
            aldis_tc_control_decode(data.as_ptr(), data.len(), &raw mut control, &raw mut detail)
        };
        assert_eq!(status == ALDIS_OK, rust.is_some());
        if let Some(ControlCommand::SetVr(vr)) = rust {
            assert_eq!(control.set_vr_value, vr.get());
        }
        if let Some(&octets) = data.first_chunk::<4>() {
            let rust = Clcw::decode(octets, mode);
            let mut clcw = aldis_clcw_t::default();
            let mut anomalies = u32::MAX;
            // SAFETY: as above.
            let status = unsafe {
                aldis_clcw_decode(
                    u32::from_be_bytes(octets),
                    c_mode,
                    &raw mut clcw,
                    &raw mut anomalies,
                    &raw mut detail,
                )
            };
            expect(&rust, status, &detail);
            if let Ok((c, a)) = rust {
                assert_eq!(
                    (clcw.vcid, clcw.report_value, clcw.retransmit == 1),
                    (c.vc_id.get(), c.report_value.get(), c.retransmit)
                );
                assert_eq!(anomalies, a.bits());
            }
        }
    }
}

// ---------------------------------------------------------------- AOS (WP-4 module 8)

/// The C view of an AOS data unit agrees with the Rust one (AOS-017).
fn aos_pdu_same(view: &aldis_aos_pdu_t, unit: &DataUnit<'_>) {
    let expected = match unit {
        DataUnit::Mpdu(mpdu) => {
            let (kind, value) = match mpdu.pointer() {
                aldis_aos::PacketPointer::StartsAt(n) => (ALDIS_AOS_POINTER_STARTS_AT, n.get()),
                aldis_aos::PacketPointer::NoPacketStart => (ALDIS_AOS_POINTER_NO_PACKET_START, 0),
                aldis_aos::PacketPointer::OnlyIdleData => (ALDIS_AOS_POINTER_ONLY_IDLE_DATA, 0),
            };
            let header = (mpdu.first_header_pointer(), mpdu.issue4_spare());
            (
                ALDIS_AOS_CONTENT_MPDU,
                kind,
                value,
                header,
                mpdu.anomalies().bits(),
            )
        }
        DataUnit::Bpdu(bpdu) => {
            let (kind, value) = match bpdu.pointer() {
                BitstreamPointer::ValidBits(n) => (ALDIS_AOS_POINTER_VALID_BITS, n.get()),
                BitstreamPointer::AllValid => (ALDIS_AOS_POINTER_ALL_VALID, 0),
                BitstreamPointer::OnlyIdle => (ALDIS_AOS_POINTER_ONLY_IDLE, 0),
            };
            let header = (bpdu.bitstream_data_pointer(), bpdu.spare());
            (
                ALDIS_AOS_CONTENT_BPDU,
                kind,
                value,
                header,
                bpdu.anomalies().bits(),
            )
        }
        DataUnit::VcaSdu(_) => (
            ALDIS_AOS_CONTENT_VCA_SDU,
            ALDIS_AOS_POINTER_NONE,
            0,
            (0, 0),
            0,
        ),
        DataUnit::IdleData(_) => (
            ALDIS_AOS_CONTENT_IDLE_DATA,
            ALDIS_AOS_POINTER_NONE,
            0,
            (0, 0),
            0,
        ),
    };
    let header = (view.pointer_raw, view.spare);
    assert_eq!(
        (
            view.content,
            view.pointer,
            view.pointer_value,
            header,
            view.anomalies
        ),
        expected
    );
}

/// The C form of an AOS Primary Header agrees with the Rust one (API-014).
fn aos_header_same(c: &aldis_aos_header_t, h: &AosPrimaryHeader) {
    assert_eq!(
        (
            c.scid,
            c.vcid,
            c.vc_frame_count,
            c.replay_flag,
            c.cycle_use_flag
        ),
        (
            h.scid().get(),
            h.vcid().get(),
            h.vc_frame_count().get(),
            h.replay_flag(),
            h.cycle_use_flag()
        )
    );
    assert_eq!(
        (c.cycle_field, c.issue4_spare, c.has_fhec.then_some(c.fhec)),
        (h.cycle_field().get(), h.issue4_spare(), h.fhec())
    );
    assert_eq!(
        (c.corrected_symbols, c.octets),
        (h.corrected_symbols().get(), h.to_octets())
    );
}

/// The C view of an AOS frame agrees with the Rust view: header, data unit, FECF, anomalies,
/// and every field's offset and length (CABI-012).
fn aos_same(view: &aldis_aos_frame_t, frame: &AosFrame<'_>) {
    aos_header_same(&view.header, &frame.header());
    aos_pdu_same(&view.pdu, &frame.data_unit());
    assert_eq!(view.has_fecf.then_some(view.fecf), frame.fecf());
    assert_eq!(view.anomalies, frame.anomalies().bits());
    let c = &view.layout;
    let ranges = frame.layout();
    let span = |at: usize, len: usize| at..at + len;
    assert_eq!(c.frame_len, frame.len());
    assert_eq!(
        span(c.insert_zone_offset, c.insert_zone_len),
        ranges.secondary()
    );
    assert_eq!(
        span(c.security_header_offset, c.security_header_len),
        ranges.security_header()
    );
    assert_eq!(span(c.data_offset, c.data_len), ranges.data());
    let zone = frame.data_unit().zone();
    let zone_start = zone.as_ptr().addr() - frame.as_bytes().as_ptr().addr();
    assert_eq!(
        span(c.zone_offset, c.zone_len),
        span(zone_start, zone.len())
    );
    assert_eq!(
        span(c.security_trailer_offset, c.security_trailer_len),
        ranges.security_trailer()
    );
    assert_eq!(span(c.ocf_offset, c.ocf_len), ranges.ocf());
    assert_eq!(span(c.fecf_offset, c.fecf_len), ranges.fecf());
}

/// The C ABI of `aldis-aos` agrees with `aldis-aos` on a frame of the input's length, on a
/// physical channel whose FHEC, FECF, edition, and Insert Zone the selector chooses (M2 plan
/// D14): `aldis_aos_fhec_encode` and `aldis_aos_fhec_decode` on the first octets,
/// `aldis_aos_mpdu_decode` and `aldis_aos_bpdu_decode` on the input, `aldis_aos_peek_length`
/// and `aldis_aos_header_decode`; and, on a master channel of the frame's SCID with every data
/// VCID, VC `n` carrying the content `n % 4` with SDLS layouts of `n % 3` header and `n % 2`
/// trailer octets and an OCF on odd VCIDs, `aldis_aos_decode` (AOS-010…AOS-017, API-070).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
#[expect(clippy::too_many_lines, reason = "one comparison per C function")]
fn aos(data: &[u8], selector: u8, mode: DecodeMode, c_mode: u32) {
    let (fhec, fecf) = (selector & 0x40 != 0, selector & 0x80 != 0);
    let (edition, c_edition) = if selector & 0x20 != 0 {
        (AosEdition::Issue4Compatible, ALDIS_AOS_EDITION_ISSUE4)
    } else {
        (AosEdition::Issue5, ALDIS_AOS_EDITION_ISSUE5)
    };
    let insert_zone_len = usize::from((selector >> 3) & 3);
    let mut detail = aldis_error_detail_t::default();
    // The FHEC on the first six and eight octets (AOS-013, AOS-014).
    if let Some(six) = data.first_chunk::<6>() {
        let mut c = 0_u16;
        // SAFETY: `six`, `c`, and `detail` are live.
        let status = unsafe { aldis_aos_fhec_encode(six.as_ptr(), 6, &raw mut c, &raw mut detail) };
        assert_eq!(
            (status, c.to_be_bytes()),
            (ALDIS_OK, aldis_aos::fhec::encode(six))
        );
    }
    if let Some(eight) = data.first_chunk::<8>() {
        let mut c = aldis_aos_corrected_header_t::default();
        let rust = aldis_aos::fhec::decode(eight);
        // SAFETY: as above.
        let status =
            unsafe { aldis_aos_fhec_decode(eight.as_ptr(), 8, &raw mut c, &raw mut detail) };
        expect(&rust, status, &detail);
        if let Ok(r) = rust {
            assert_eq!(
                (c.octets, c.corrected_symbols),
                (r.octets(), r.corrected_symbols().get())
            );
        }
    }
    // The data units on the whole input (AOS-003, AOS-017).
    let mut pdu = aldis_aos_pdu_t::default();
    let rust = Mpdu::decode(data, edition, mode);
    // SAFETY: as above.
    let status = unsafe {
        aldis_aos_mpdu_decode(
            data.as_ptr(),
            data.len(),
            c_edition,
            c_mode,
            &raw mut pdu,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(m) = rust {
        aos_pdu_same(&pdu, &DataUnit::Mpdu(m));
    }
    let rust = Bpdu::decode(data, mode);
    // SAFETY: as above.
    let status = unsafe {
        aldis_aos_bpdu_decode(
            data.as_ptr(),
            data.len(),
            c_mode,
            &raw mut pdu,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(b) = rust {
        aos_pdu_same(&pdu, &DataUnit::Bpdu(b));
    }
    let pc = aldis_aos_config_t {
        struct_size: u32::try_from(size_of::<aldis_aos_config_t>()).unwrap(),
        fhec: u8::from(fhec),
        fecf: u8::from(fecf),
        edition: c_edition,
        frame_len: data.len(),
        insert_zone_len,
    };
    let rust_pc = AosPhysicalChannelConfig::new(data.len(), fhec, insert_zone_len, fecf, edition);
    let rust = rust_pc.and_then(|p| AosFrame::peek_length(data, &p));
    let mut len = 0;
    // SAFETY: `data`, `pc`, `len`, and `detail` are live.
    let status = unsafe {
        aldis_aos_peek_length(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            &raw mut len,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(n) = rust {
        assert_eq!(len, n);
    }
    let rust = rust_pc.and_then(|p| AosPrimaryHeader::decode(data, &p));
    let mut header = aldis_aos_header_t::default();
    // SAFETY: as above.
    let status = unsafe {
        aldis_aos_header_decode(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            &raw mut header,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    let Ok(h) = rust else {
        return;
    };
    aos_header_same(&header, &h);
    // A master channel of the frame's SCID with every data VCID.
    let mc = aldis_aos_mc_config_t {
        struct_size: u32::try_from(size_of::<aldis_aos_mc_config_t>()).unwrap(),
        scid: h.scid().get(),
        pc,
    };
    let contents = [
        (ALDIS_AOS_CONTENT_MPDU, aldis_aos::DataFieldContent::Mpdu),
        (ALDIS_AOS_CONTENT_BPDU, aldis_aos::DataFieldContent::Bpdu),
        (
            ALDIS_AOS_CONTENT_VCA_SDU,
            aldis_aos::DataFieldContent::VcaSdu,
        ),
        (
            ALDIS_AOS_CONTENT_IDLE_DATA,
            aldis_aos::DataFieldContent::IdleData,
        ),
    ];
    let vcs: [aldis_aos_vc_config_t; 63] = core::array::from_fn(|n| aldis_aos_vc_config_t {
        struct_size: u32::try_from(size_of::<aldis_aos_vc_config_t>()).unwrap(),
        vcid: u8::try_from(n).unwrap(),
        content: contents[n % 4].0,
        ocf: u8::from(n % 2 == 1),
        security_header_len: u8::try_from(n % 3).unwrap(),
        security_trailer_len: u16::try_from(n % 2).unwrap(),
    });
    let rust_table = rust_pc.and_then(|p| {
        let mut list = Vec::new();
        for n in 0..63_u8 {
            let i = usize::from(n);
            let mut vc =
                AosVirtualChannelConfig::new(h.scid(), VcId::new(n).unwrap(), contents[i % 4].1)?;
            if n % 2 == 1 {
                vc = vc.with_ocf();
            }
            list.push(vc.with_security(SecurityLayout::new(i % 3, i % 2)?));
        }
        VcTable::<63>::new(p, &list).map(|t| (p, t))
    });
    let rust = rust_table
        .as_ref()
        .map_err(|e| *e)
        .and_then(|(p, t)| AosFrame::decode(data, p, t, mode));
    let mut view = aldis_aos_frame_t::default();
    // SAFETY: as above; `vcs` holds 63 structs.
    let status = unsafe {
        aldis_aos_decode(
            data.as_ptr(),
            data.len(),
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(f) = &rust {
        aos_same(&view, f);
    }
}

// ---- USLP (M2 WP-5 module 9)

/// The offset of `part`, a subslice of `whole`, in `whole`.
fn uslp_offset(whole: &[u8], part: &[u8]) -> usize {
    part.as_ptr().addr() - whole.as_ptr().addr()
}

/// The C view of a USLP frame agrees with the Rust view: the tag, the shared fields, the TFDF
/// Header, the full frame's header fields, the FECF, the anomalies, and every field's offset
/// and length (CABI-012; M2 plan D12).
fn uslp_same(view: &aldis_uslp_frame_t, frame: &UslpFrame<'_>, data: &[u8]) {
    let ch = frame.channel();
    assert_eq!(view.truncated, frame.is_truncated());
    assert_eq!(
        (
            view.channel.scid,
            view.channel.source_or_destination,
            view.channel.vcid,
            view.channel.map_id
        ),
        (
            ch.spacecraft_id.get(),
            u8::from(ch.source_or_destination.bit()),
            ch.vc_id.get(),
            ch.map_id.get()
        )
    );
    let tfdf = frame.tfdf_header();
    assert_eq!(
        (view.tfdf.rule, view.tfdf.upid, view.tfdf.pointer),
        (
            tfdf.rule.rule().bits(),
            tfdf.upid.bits(),
            tfdf.rule.pointer().unwrap_or(0)
        )
    );
    assert_eq!(view.upid_assigned, !matches!(tfdf.upid, Upid::Other(_)));
    assert_eq!(view.anomalies, frame.anomalies().bits());
    let l = &view.layout;
    let span = |at: usize, len: usize| at..at + len;
    let of = |s: &[u8]| span(uslp_offset(data, s), s.len());
    assert_eq!(l.frame_len, frame.len());
    assert_eq!(
        span(l.security_header_offset, l.security_header_len),
        of(frame.security_header())
    );
    assert_eq!(span(l.tfdz_offset, l.tfdz_len), of(frame.tfdz()));
    assert_eq!(l.tfdf_header_offset + tfdf.len(), l.tfdz_offset);
    assert_eq!(
        span(l.security_trailer_offset, l.security_trailer_len),
        of(frame.security_trailer())
    );
    assert_eq!(
        l.ocf_offset,
        l.security_trailer_offset + l.security_trailer_len
    );
    assert_eq!(l.fecf_offset + l.fecf_len, l.frame_len);
    if let Some(full) = frame.full() {
        let h = full.header();
        assert_eq!(
            (view.ocf_flag, view.vcf_count_len, view.vcf_count),
            (
                h.ocf_flag(),
                h.vcf_count().len().get(),
                h.vcf_count().value()
            )
        );
        let kind = match h.kind() {
            FrameKind::SequenceControlled => ALDIS_USLP_KIND_SEQUENCE_CONTROLLED,
            FrameKind::Expedited => ALDIS_USLP_KIND_EXPEDITED,
            FrameKind::ProtocolControl => ALDIS_USLP_KIND_PROTOCOL_CONTROL,
        };
        assert_eq!(view.kind, kind);
        assert_eq!(
            span(l.insert_zone_offset, l.insert_zone_len),
            of(full.insert_zone())
        );
        assert_eq!(l.ocf_len, if full.ocf().is_some() { OCF_LEN } else { 0 });
        assert_eq!(view.has_fecf.then_some(view.fecf), full.fecf());
    } else {
        assert_eq!(
            (view.kind, view.ocf_flag, view.vcf_count_len, view.vcf_count),
            (ALDIS_USLP_KIND_EXPEDITED, false, 0, 0)
        );
        assert_eq!((l.insert_zone_len, l.ocf_len, l.fecf_len), (0, 0, 0));
        assert!(!view.has_fecf);
    }
}

/// The USLP channels of an input: a physical channel, fixed-length of the input's length or
/// variable-length up to 1024 octets, with the selector's FECF and Insert Zone; one virtual
/// channel on the SCID and VCID the input's first octets name, with the VCF Count Length of
/// its octet 6 for both services, so most frames reach the later checks, and a truncated frame
/// length, an OCF use, and SDLS layouts (per VC or per MAP) drawn from the selector; and its 16
/// MAPs. As C structs and as the `VcTable` the Rust constructors build.
fn uslp_channels(
    data: &[u8],
    selector: u8,
) -> (
    aldis_uslp_config_t,
    aldis_uslp_vc_config_t,
    [aldis_uslp_map_config_t; 16],
) {
    let fixed = selector & 0x80 != 0;
    let fecf = selector & 0x40 != 0;
    let word = data
        .first_chunk::<4>()
        .map_or(0, |b| u32::from_be_bytes(*b));
    let scid = u16::try_from((word >> 12) & 0xFFFF).unwrap();
    let vcid = u8::try_from((word >> 5) & 0x3F).unwrap();
    let count_len = data.get(6).map_or(0, |b| b & 0b111);
    let per_map = selector & 0x20 != 0;
    let frame_type = if fixed {
        ALDIS_USLP_FRAME_FIXED
    } else {
        ALDIS_USLP_FRAME_VARIABLE
    };
    let pc = aldis_uslp_config_t {
        struct_size: u32::try_from(size_of::<aldis_uslp_config_t>()).unwrap(),
        frame_type,
        fecf: u8::from(fecf),
        frame_len: if fixed { data.len() } else { 1024 },
        insert_zone_len: if fixed {
            usize::from((selector >> 3) & 0b11)
        } else {
            0
        },
    };
    let vc = aldis_uslp_vc_config_t {
        struct_size: u32::try_from(size_of::<aldis_uslp_vc_config_t>()).unwrap(),
        scid,
        vcid,
        frame_type,
        vcf_count_len_sequence: count_len,
        vcf_count_len_expedited: count_len,
        ocf: match (fixed, selector & 0x10 != 0) {
            (true, true) => ALDIS_USLP_OCF_REQUIRED,
            (false, true) => ALDIS_USLP_OCF_ALLOWED,
            (_, false) => ALDIS_USLP_OCF_ABSENT,
        },
        truncated_frame_len: if !fixed && selector & 0x08 != 0 {
            6 + vcid % 27
        } else {
            0
        },
        security: if per_map {
            ALDIS_USLP_SECURITY_PER_MAP
        } else {
            ALDIS_USLP_SECURITY_VC
        },
        security_header_len: if per_map { 0 } else { vcid % 3 },
        security_trailer_len: if per_map { 0 } else { u32::from(vcid % 2) },
    };
    let maps = core::array::from_fn(|m| {
        let m = u8::try_from(m).unwrap();
        aldis_uslp_map_config_t {
            struct_size: u32::try_from(size_of::<aldis_uslp_map_config_t>()).unwrap(),
            scid,
            vcid,
            map_id: m,
            security_header_len: if per_map { m % 3 } else { 0 },
            security_trailer_len: if per_map { u32::from(m % 2) } else { 0 },
        }
    });
    (pc, vc, maps)
}

/// The Rust table of the C structs, built through the `aldis-uslp` constructors.
fn uslp_table(
    pc: &aldis_uslp_config_t,
    vc: &aldis_uslp_vc_config_t,
    maps: &[aldis_uslp_map_config_t; 16],
) -> Result<UslpVcTable<1, 16>, Error> {
    let frame_type = |t| {
        if t == ALDIS_USLP_FRAME_FIXED {
            TransferFrameType::Fixed
        } else {
            TransferFrameType::Variable
        }
    };
    let physical = PhysicalChannelConfig::new(
        frame_type(pc.frame_type),
        pc.frame_len,
        pc.insert_zone_len,
        pc.fecf == 1,
    )?;
    let scid = UslpSpacecraftId::try_from(vc.scid)?;
    let vcid = UslpVcId::try_from(vc.vcid)?;
    let params = VcParams {
        frame_type: frame_type(vc.frame_type),
        vcf_count_len_sequence: VcfCountLen::new(vc.vcf_count_len_sequence)?,
        vcf_count_len_expedited: VcfCountLen::new(vc.vcf_count_len_expedited)?,
        ocf: match vc.ocf {
            ALDIS_USLP_OCF_REQUIRED => OcfUse::Required,
            ALDIS_USLP_OCF_ALLOWED => OcfUse::Allowed,
            _ => OcfUse::Absent,
        },
        truncated_frame_len: match vc.truncated_frame_len {
            0 => None,
            n => Some(TruncatedFrameLen::new(usize::from(n))?),
        },
        security: if vc.security == ALDIS_USLP_SECURITY_PER_MAP {
            SecurityScope::PerMap
        } else {
            SecurityScope::Vc(SecurityLayout::new(
                usize::from(vc.security_header_len),
                usize::try_from(vc.security_trailer_len).unwrap(),
            )?)
        },
    };
    let mut rust_maps =
        [MapConfig::new(scid, vcid, UslpMapId::new(0).unwrap(), SecurityLayout::NONE); 16];
    for (slot, m) in rust_maps.iter_mut().zip(maps) {
        *slot = MapConfig::new(
            scid,
            vcid,
            UslpMapId::try_from(m.map_id)?,
            SecurityLayout::new(
                usize::from(m.security_header_len),
                usize::try_from(m.security_trailer_len).unwrap(),
            )?,
        );
    }
    UslpVcTable::new(physical, &[VcConfig::new(scid, vcid, params)?], &rust_maps)
}

/// The C ABI of `aldis-uslp` agrees with `aldis-uslp` on the input as one frame or data unit
/// (M2 plan D14): `aldis_uslp_vc_table_check`, `aldis_uslp_decode`, `aldis_uslp_peek_length`,
/// and `aldis_uslp_stream_next` over the whole input (USLP-001…USLP-007, USLP-014…USLP-016, API-070).
#[expect(unsafe_code, reason = "calls the C ABI with valid pointers")]
fn uslp(data: &[u8], selector: u8, mode: DecodeMode, c_mode: u32) {
    let (pc, vc, maps) = uslp_channels(data, selector);
    let rust_table = uslp_table(&pc, &vc, &maps);
    let mut detail = aldis_error_detail_t::default();
    // SAFETY: every pointer is to a live local.
    let status = unsafe {
        aldis_uslp_vc_table_check(
            &raw const pc,
            &raw const vc,
            1,
            maps.as_ptr(),
            maps.len(),
            &raw mut detail,
        )
    };
    expect(&rust_table, status, &detail);
    let Ok(table) = rust_table else {
        return;
    };
    let mut view = aldis_uslp_frame_t::default();
    let rust = UslpFrame::decode(data, &table, mode);
    // SAFETY: as above.
    let status = unsafe {
        aldis_uslp_decode(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            &raw const vc,
            1,
            maps.as_ptr(),
            maps.len(),
            c_mode,
            &raw mut view,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(f) = &rust {
        uslp_same(&view, f, data);
    }
    let rust = UslpFrame::peek_length(data, &table);
    let mut len = 0;
    // SAFETY: as above.
    let status = unsafe {
        aldis_uslp_peek_length(
            data.as_ptr(),
            data.len(),
            &raw const pc,
            &raw const vc,
            1,
            &raw mut len,
            &raw mut detail,
        )
    };
    expect(&rust, status, &detail);
    if let Ok(n) = rust {
        assert_eq!(len, n);
    }
    let mut stream_at = 0;
    let mut rust_stream = UslpFrameStream::new(data, &table, mode);
    loop {
        let mut item = aldis_uslp_stream_item_t::default();
        // SAFETY: as above.
        let status = unsafe {
            aldis_uslp_stream_next(
                data.as_ptr(),
                data.len(),
                &raw mut stream_at,
                &raw const pc,
                &raw const vc,
                1,
                maps.as_ptr(),
                maps.len(),
                c_mode,
                &raw mut item,
                null_mut(),
            )
        };
        assert_eq!(status, ALDIS_OK);
        match rust_stream.next() {
            None => {
                assert_eq!(item.kind, ALDIS_USLP_STREAM_END);
                break;
            }
            Some(UslpIntakeItem::Accepted { frame, .. }) => {
                assert_eq!(item.kind, ALDIS_USLP_STREAM_ACCEPTED);
                assert_eq!(item.len, frame.len());
                uslp_same(&item.frame, &frame, frame.octets());
            }
            Some(UslpIntakeItem::Discarded { reason, octets, .. }) => {
                assert_eq!(item.kind, ALDIS_USLP_STREAM_DISCARDED);
                assert_eq!(item.len, octets.len());
                assert_eq!(item.discard_reason, u32::from(reason.code()));
            }
        }
    }
    assert_eq!(stream_at, data.len());
}

// ---- end USLP

fuzz_target!(|input: &[u8]| {
    let Some((&selector, data)) = input.split_first() else {
        return;
    };
    let (mode, c_mode) = if selector & 1 == 0 {
        (DecodeMode::Strict, ALDIS_MODE_STRICT)
    } else {
        (DecodeMode::Lenient, ALDIS_MODE_LENIENT)
    };
    // AOS (WP-4 module 8): every input, besides the branch the selector picks.
    aos(data, selector, mode, c_mode);
    match (selector >> 1) % 7 {
        0 => spp(data, mode, c_mode),
        1 => epp(data, mode, c_mode),
        2 => time(data, mode, c_mode),
        3 => tm(data, selector & 0x80 != 0, mode, c_mode),
        4 => tc_ffi::run(data, selector & 0x80 != 0, mode, c_mode),
        // ---- USLP (M2 WP-5 module 9)
        5 => uslp(data, selector, mode, c_mode),
        // ---- end USLP
        _ => core(data),
    }
});
