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
//! of `aldis-tc` and the CLCW (`tc_ffi`).
#![no_main]

// req: CABI-012, CABI-020, CABI-021, CABI-022, GEN-020, API-010, VER-022, CMN-001, CMN-010, CMN-022, TM-001, API-070
// req: TC-001, TC-020, TC-021, TC-030

use core::ptr::null_mut;

use aldis_core::crc::crc16;
use aldis_core::ids::VcId;
use aldis_core::ocf::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, Error};
use aldis_epp::{EncapsulationPacket, EpiExtension, UserDefined};
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
        DataFieldStatus::Packets(PacketPointer::StartsAt(n)) => (ALDIS_TM_POINTER_STARTS_AT, n),
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
/// `aldis_tc_decode`, and `aldis_tc_validate` with its reason; `aldis_tc_next_frame` at every
/// item of the input as a data unit; `aldis_tc_control_decode`; and `aldis_clcw_decode` on the
/// first four octets (TC-001…TC-006, TC-020…TC-022, TC-030, API-070).
mod tc_ffi {
    use core::ptr::null_mut;

    use aldis_core::security::SecurityLayout;
    use aldis_core::{DecodeMode, Error};
    use aldis_ffi::tc::{
        ALDIS_TC_CONTROL_NONE, ALDIS_TC_CONTROL_SET_VR, ALDIS_TC_CONTROL_UNLOCK,
        ALDIS_TC_DELIMITED_END, ALDIS_TC_DELIMITED_FILL, ALDIS_TC_DELIMITED_FRAME,
        ALDIS_TC_DELIMITED_TRUNCATED_FRAME, ALDIS_TC_FRAME_AD, ALDIS_TC_FRAME_BC,
        ALDIS_TC_FRAME_BD, ALDIS_TC_REASON_NONE, aldis_clcw_decode, aldis_clcw_t,
        aldis_tc_config_t, aldis_tc_control_decode, aldis_tc_control_t, aldis_tc_decode,
        aldis_tc_delimited_t, aldis_tc_frame_t, aldis_tc_header_decode, aldis_tc_header_t,
        aldis_tc_map_config_t, aldis_tc_mc_config_check, aldis_tc_mc_config_t, aldis_tc_next_frame,
        aldis_tc_peek_length, aldis_tc_validate, aldis_tc_vc_config_t,
    };
    use aldis_ffi::{ALDIS_OK, aldis_error_detail_t};
    use aldis_tc::{
        Clcw, ControlCommand, Delimited, FrameType, MapConfig, PhysicalChannelConfig,
        SegmentHeaderUse, TcFrame, TcFrameHeader, TcMapId, TcSpacecraftId, TcVcId, VcConfig,
        VcTable, next_frame, validate_frame,
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
        let rust = validate_frame(data, &table, mode);
        let mut reason = u32::MAX;
        // SAFETY: as above.
        let status = unsafe {
            aldis_tc_validate(
                data.as_ptr(),
                data.len(),
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                c_mode,
                &raw mut view,
                &raw mut reason,
                &raw mut detail,
            )
        };
        expect(&rust.map_err(Error::from), status, &detail);
        match &rust {
            Ok(f) => {
                assert_eq!(reason, ALDIS_TC_REASON_NONE);
                same(&view, data, f);
            }
            Err(r) => assert_eq!(reason, u32::from(r.reason().code())),
        }
        // The input as one data unit, item by item.
        let mut offset = 0;
        loop {
            let mut item = aldis_tc_delimited_t::default();
            // SAFETY: as above.
            let status = unsafe {
                aldis_tc_next_frame(data.as_ptr(), data.len(), offset, &raw mut item, null_mut())
            };
            assert_eq!((status, item.offset), (ALDIS_OK, offset));
            let rust = next_frame(&data[offset..]);
            let expected = match rust {
                None => (ALDIS_TC_DELIMITED_END, 0, 0),
                Some(Delimited::Frame(f)) => (ALDIS_TC_DELIMITED_FRAME, f.len(), f.len()),
                Some(Delimited::TruncatedFrame {
                    octets,
                    declared_len,
                }) => (
                    ALDIS_TC_DELIMITED_TRUNCATED_FRAME,
                    octets.len(),
                    declared_len,
                ),
                Some(Delimited::Fill(f)) => (ALDIS_TC_DELIMITED_FILL, f.len(), 0),
            };
            assert_eq!((item.kind, item.len, item.declared_len), expected);
            if item.kind == ALDIS_TC_DELIMITED_END {
                break;
            }
            offset += item.len;
        }
        assert_eq!(offset, data.len());
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

fuzz_target!(|input: &[u8]| {
    let Some((&selector, data)) = input.split_first() else {
        return;
    };
    let (mode, c_mode) = if selector & 1 == 0 {
        (DecodeMode::Strict, ALDIS_MODE_STRICT)
    } else {
        (DecodeMode::Lenient, ALDIS_MODE_LENIENT)
    };
    match (selector >> 1) % 6 {
        0 => spp(data, mode, c_mode),
        1 => epp(data, mode, c_mode),
        2 => time(data, mode, c_mode),
        3 => tm(data, selector & 0x80 != 0, mode, c_mode),
        4 => tc_ffi::run(data, selector & 0x80 != 0, mode, c_mode),
        _ => core(data),
    }
});
