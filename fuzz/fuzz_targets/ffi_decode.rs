//! The C ABI decoders agree with the Rust decoders on the same octets (M2 plan WP-7, D16):
//! `aldis_spp_decode`, `aldis_spp_peek_length`, `aldis_epp_decode`, `aldis_epp_peek_length`,
//! `aldis_time_decode`, `aldis_time_pfield_parse`, and `aldis_time_ascii_parse`, in both modes.
//! A success carries the same fields, offsets, lengths, and anomaly flags; a failure the same
//! status and the same detail: kind, field code, and values (CABI-012, CABI-020, CABI-021,
//! GEN-020). A decoded time code re-encodes through the C ABI to the octets the Rust encoder
//! writes.
#![no_main]

// req: CABI-012, CABI-020, CABI-021, CABI-022, GEN-020, API-010, VER-022

use aldis_core::{DecodeMode, Error};
use aldis_epp::{EncapsulationPacket, EpiExtension, UserDefined};
use aldis_ffi::epp::{aldis_epp_decode, aldis_epp_packet_t, aldis_epp_peek_length};
use aldis_ffi::error::ALDIS_ERROR_DETAIL_MIN_SIZE;
use aldis_ffi::spp::{aldis_spp_decode, aldis_spp_packet_t, aldis_spp_peek_length};
use aldis_ffi::time::{
    aldis_time_ascii_options_t, aldis_time_ascii_parse, aldis_time_ascii_t, aldis_time_decode,
    aldis_time_decoded_t, aldis_time_encode, aldis_time_pfield_parse, aldis_time_pfield_t,
};
use aldis_ffi::{ALDIS_MODE_LENIENT, ALDIS_MODE_STRICT, ALDIS_OK, aldis_error_detail_t};
use aldis_spp::SpacePacket;
use aldis_time::{AsciiOptions, AsciiTime, PField, decode_with_pfield};
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

fuzz_target!(|input: &[u8]| {
    let Some((&selector, data)) = input.split_first() else {
        return;
    };
    let (mode, c_mode) = if selector & 1 == 0 {
        (DecodeMode::Strict, ALDIS_MODE_STRICT)
    } else {
        (DecodeMode::Lenient, ALDIS_MODE_LENIENT)
    };
    match (selector >> 1) % 3 {
        0 => spp(data, mode, c_mode),
        1 => epp(data, mode, c_mode),
        _ => time(data, mode, c_mode),
    }
});
