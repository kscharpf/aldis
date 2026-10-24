//! The contract of the TM part of the C ABI, called as a C caller calls it: the virtual channel
//! arrays (CABI-033, M2 plan D14), the security layout variants (API-070), the stateless
//! writers (API-023), and the argument checks (CABI-022) the vectors do not reach.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test-only harness (RS-018)"
)]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_SECURITY_HEADER_LEN, ALDIS_FIELD_SPACECRAFT_ID, ALDIS_FIELD_SYNC_FLAG,
};
use aldis_ffi::oid::*;
use aldis_ffi::tm::*;
use aldis_ffi::{ALDIS_MODE_STRICT, aldis_error_detail_t};

fn size<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap()
}

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        struct_size: size::<aldis_error_detail_t>(),
        ..aldis_error_detail_t::default()
    }
}

/// 40-octet frames with an FECF on SCID 677.
fn mc() -> aldis_tm_mc_config_t {
    aldis_tm_mc_config_t {
        struct_size: size::<aldis_tm_mc_config_t>(),
        scid: 677,
        fsh_len: 0,
        ocf: 0,
        pc: aldis_tm_config_t {
            struct_size: size::<aldis_tm_config_t>(),
            fecf: 1,
            frame_len: 40,
        },
    }
}

/// VC 4 carries packets with a Security Header of 6 octets and a Trailer of 8; VC 5 `VCA_SDUs`.
fn vcs() -> [aldis_tm_vc_config_t; 2] {
    let vc = aldis_tm_vc_config_t {
        struct_size: size::<aldis_tm_vc_config_t>(),
        vcid: 4,
        content: ALDIS_TM_CONTENT_PACKETS,
        security_header_len: 6,
        security_trailer_len: 8,
        ..aldis_tm_vc_config_t::default()
    };
    [
        vc,
        aldis_tm_vc_config_t {
            vcid: 5,
            content: ALDIS_TM_CONTENT_VCA_SDU,
            security_header_len: 0,
            security_trailer_len: 0,
            ..vc
        },
    ]
}

fn packets(vcid: u8) -> aldis_tm_fields_t {
    aldis_tm_fields_t {
        vcid,
        pointer: ALDIS_TM_POINTER_STARTS_AT,
        ..aldis_tm_fields_t::default()
    }
}

/// A frame of VC 4: Security Header `A5…`, data `D0…`, Security Trailer `5A…`.
fn sdls_frame() -> Vec<u8> {
    let (mc, vcs) = (mc(), vcs());
    let fields = packets(4);
    let mut frame = vec![0_u8; 40];
    let mut l = aldis_tm_layout_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_tm_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const fields,
                frame.as_mut_ptr(),
                40,
                &raw mut l,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            (l.security_header_offset, l.data_offset, l.data_len),
            (6, 12, 18)
        );
        assert_eq!(
            (l.security_trailer_offset, l.ocf_len, l.fecf_offset),
            (30, 0, 38)
        );
        frame[6..12].fill(0xA5);
        frame[12..30].fill(0xD0);
        frame[30..38].fill(0x5A);
        let mut n = 0;
        assert_eq!(
            aldis_tm_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const fields,
                frame.as_mut_ptr(),
                40,
                &raw mut n,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(n, 40);
    }
    frame
}

/// A VC struct of a later header, with a member this library does not know.
#[repr(C)]
#[derive(Clone, Copy)]
struct LaterVc {
    vc: aldis_tm_vc_config_t,
    later: u32,
}

#[test]
fn vc_arrays_step_by_their_struct_size() {
    // req: CABI-033, CABI-022, API-070
    let mc = mc();
    let check = |vcs: *const aldis_tm_vc_config_t, n: usize| {
        // SAFETY: `mc` is live; `vcs` is NULL or points to `n` live structs.
        unsafe { aldis_tm_mc_config_check(&raw const mc, vcs, n, null_mut()) }
    };
    let base = vcs();
    assert_eq!(check(base.as_ptr(), 2), ALDIS_OK);
    assert_eq!(check(null(), 0), ALDIS_OK);
    assert_eq!(check(null(), 1), ALDIS_ERR_INVALID_FIELD);
    // A longer struct of a later header: the stride is its struct_size.
    let later = base.map(|vc| LaterVc {
        vc: aldis_tm_vc_config_t {
            struct_size: size::<LaterVc>(),
            ..vc
        },
        later: u32::MAX,
    });
    assert_eq!(check(later.as_ptr().cast(), 2), ALDIS_OK);
    // Every struct repeats the stride.
    let mut mixed = later;
    mixed[1].vc.struct_size = size::<aldis_tm_vc_config_t>();
    assert_eq!(check(mixed.as_ptr().cast(), 2), ALDIS_ERR_INVALID_FIELD);
    // Shorter than the struct this library knows, or not a multiple of its alignment.
    for struct_size in [8, size::<aldis_tm_vc_config_t>() + 2] {
        let mut v = later;
        for e in &mut v {
            e.vc.struct_size = struct_size;
        }
        assert_eq!(check(v.as_ptr().cast(), 2), ALDIS_ERR_INVALID_FIELD);
    }
    // A misaligned array.
    let words = [0_u32; 8];
    let misaligned = words.as_ptr().cast::<u8>().wrapping_add(1).cast();
    assert_eq!(check(misaligned, 1), ALDIS_ERR_INVALID_FIELD);
    // A count whose array would wrap the address space.
    assert_eq!(
        check(base.as_ptr(), usize::MAX / 4),
        ALDIS_ERR_INVALID_FIELD
    );
    // The embedded physical channel carries its own struct_size (CABI-033).
    let mut small = mc;
    small.pc.struct_size = 4;
    // SAFETY: as above.
    let r = unsafe { aldis_tm_mc_config_check(&raw const small, base.as_ptr(), 2, null_mut()) };
    assert_eq!(r, ALDIS_ERR_INVALID_FIELD);
    small = mc;
    small.struct_size = 4;
    // SAFETY: as above.
    let r = unsafe { aldis_tm_mc_config_check(&raw const small, base.as_ptr(), 2, null_mut()) };
    assert_eq!(r, ALDIS_ERR_INVALID_FIELD);
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per decoder and lookup")]
fn the_with_functions_take_the_vc_security_layout() {
    // req: API-070, API-072, TM-005, TM-011, CABI-012, CABI-021
    let (mc, vcs, frame) = (mc(), vcs(), sdls_frame());
    let mut v = aldis_tm_frame_t::default();
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_tm_decode_with(
                frame.as_ptr(),
                40,
                &raw const mc,
                vcs.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut v,
                &raw mut d
            ),
            ALDIS_OK
        );
        let l = v.layout;
        assert_eq!(
            (
                l.security_header_len,
                l.data_offset,
                l.data_len,
                l.security_trailer_len
            ),
            (6, 12, 18, 8)
        );
        assert!(v.has_fecf && v.header.scid == 677 && v.header.vcid == 4);
        assert_eq!(
            aldis_tm_validate_with(
                frame.as_ptr(),
                40,
                &raw const mc,
                vcs.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut d
            ),
            ALDIS_OK
        );
        // Decoded without SDLS, the layout is not VC 4's.
        assert_eq!(
            aldis_tm_validate(
                frame.as_ptr(),
                40,
                &raw const mc,
                vcs.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut d
            ),
            ALDIS_ERR_LENGTH_MISMATCH
        );
        assert_eq!(
            (d.field, d.expected, d.found),
            (ALDIS_FIELD_SECURITY_HEADER_LEN, 6, 0)
        );
        // Another SCID is an unknown channel, with the configured and the found SCID.
        let mut other = mc;
        other.scid = 678;
        assert_eq!(
            aldis_tm_decode_with(
                frame.as_ptr(),
                40,
                &raw const other,
                vcs.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut v,
                &raw mut d
            ),
            ALDIS_ERR_UNKNOWN_CHANNEL
        );
        assert_eq!(
            (d.field, d.expected, d.found),
            (ALDIS_FIELD_SPACECRAFT_ID, 678, 677)
        );
        // A corrupted data octet: only the unchecked decoders accept the frame.
        let mut bad = frame.clone();
        bad[20] ^= 1;
        assert_eq!(
            aldis_tm_decode_with(
                bad.as_ptr(),
                40,
                &raw const mc,
                vcs.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut v,
                null_mut()
            ),
            ALDIS_ERR_CRC_MISMATCH
        );
        assert_eq!(
            aldis_tm_decode_unchecked_with(
                bad.as_ptr(),
                40,
                &raw const mc,
                vcs.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut v,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            (v.layout.data_len, v.fecf),
            (18, u16::from_be_bytes([frame[38], frame[39]]))
        );
        assert_eq!(
            aldis_tm_decode_unchecked(
                bad.as_ptr(),
                40,
                &raw const mc.pc,
                ALDIS_MODE_STRICT,
                &raw mut v,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(v.layout.data_len, 32);
    }
}

#[test]
fn the_header_decodes_alone() {
    // req: TM-002, API-014, CABI-021
    let frame = sdls_frame();
    let mut h = aldis_tm_header_t::default();
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_tm_header_decode(frame.as_ptr(), 6, &raw mut h, &raw mut d),
            ALDIS_OK
        );
        assert_eq!(
            (h.scid, h.vcid, h.fsh_flag, h.first_header_pointer),
            (677, 4, false, 0)
        );
        assert_eq!(
            aldis_tm_header_decode(frame.as_ptr(), 5, &raw mut h, &raw mut d),
            ALDIS_ERR_INCOMPLETE
        );
        assert_eq!((d.detail, d.needed), (ALDIS_DETAIL_NEEDED, 6));
        assert_eq!(
            aldis_tm_header_decode(frame.as_ptr(), 6, null_mut(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per argument rule")]
fn writer_arguments_are_checked() {
    // req: CABI-022, CABI-011, API-022, TM-009
    let (mc, vcs) = (mc(), vcs());
    let fields = packets(4);
    let sh = [0xA5_u8; 6];
    let st = [0x5A_u8; 8];
    let mut frame = [0_u8; 64];
    let mut n = 0_usize;
    let encode = |data: *const u8, data_len: usize, ocf: &[u8], out: *mut u8, cap: usize| {
        let mut n = 0_usize;
        // SAFETY: every pointer is to a live local, or into `frame`.
        unsafe {
            aldis_tm_encode(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const fields,
                null(),
                0,
                sh.as_ptr(),
                6,
                data,
                data_len,
                st.as_ptr(),
                8,
                ocf.as_ptr(),
                ocf.len(),
                out,
                cap,
                &raw mut n,
                null_mut(),
            )
        }
    };
    let data = [0xD0_u8; 18];
    assert_eq!(
        encode(data.as_ptr(), 18, &[], frame.as_mut_ptr(), 40),
        ALDIS_OK
    );
    // An OCF of neither 0 nor 4 octets.
    assert_eq!(
        encode(data.as_ptr(), 18, &[1, 2], frame.as_mut_ptr(), 40),
        ALDIS_ERR_INVALID_FIELD
    );
    // An OCF where none is configured: the frame's OCF Flag (132.0 §4.1.2.4.3).
    assert_eq!(
        encode(data.as_ptr(), 18, &[1, 2, 3, 4], frame.as_mut_ptr(), 40),
        ALDIS_ERR_INVALID_FIELD
    );
    // The output overlapping an input: the data field inside the output buffer.
    let inside = frame.as_ptr().wrapping_add(12);
    assert_eq!(
        encode(inside, 18, &[], frame.as_mut_ptr(), 40),
        ALDIS_ERR_INVALID_FIELD
    );
    // The output too small: nothing written.
    frame.fill(0xEE);
    assert_eq!(
        encode(data.as_ptr(), 18, &[], frame.as_mut_ptr(), 39),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert!(frame.iter().all(|&b| b == 0xEE));
    let mut vca = fields;
    vca.sync_flag = 1;
    let mut layout = aldis_tm_layout_t::default();
    let mut view = aldis_tm_frame_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        let mut d = detail();
        assert_eq!(
            aldis_tm_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const vca,
                frame.as_mut_ptr(),
                40,
                &raw mut n,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            (d.field, d.expected, d.found),
            (ALDIS_FIELD_SYNC_FLAG, 0, 1)
        );
        assert!(
            frame.iter().all(|&b| b == 0xEE),
            "finish rejects without writing"
        );
        assert_eq!(
            aldis_tm_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                2,
                null(),
                frame.as_mut_ptr(),
                40,
                &raw mut layout,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_tm_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const fields,
                frame.as_mut_ptr(),
                40,
                null_mut(),
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        let mut pc = mc.pc;
        pc.fecf = 2;
        assert_eq!(
            aldis_tm_peek_length(frame.as_ptr(), 0, &raw const pc, &raw mut n, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_tm_config_check(&raw const mc.pc, null_mut()),
            ALDIS_OK
        );
        assert_eq!(
            aldis_tm_decode(
                frame.as_ptr(),
                40,
                &raw const mc.pc,
                7,
                &raw mut view,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn the_oid_writer_takes_a_generator_handle() {
    // req: TM-010, CMN-021, CMN-022, CABI-022, CABI-032, GEN-042
    let (mc, vcs) = (mc(), vcs());
    let mut words = vec![0_u64; 64];
    let storage = words.as_mut_ptr();
    let mut h: *mut aldis_oid_generator_t = null_mut();
    let mut frame = [0_u8; 40];
    let mut l = aldis_tm_layout_t::default();
    let begin =
        |vcid: u8, h: *mut aldis_oid_generator_t, out: *mut u8, l: *mut aldis_tm_layout_t| {
            // SAFETY: `mc`, `vcs`, and `l` are live; `out` points to 40 live octets; `h` is a
            // handle into live storage or invalid.
            unsafe {
                aldis_tm_oid_writer_begin(
                    &raw const mc,
                    vcs.as_ptr(),
                    2,
                    vcid,
                    0,
                    0,
                    h,
                    out,
                    40,
                    l,
                    null_mut(),
                )
            }
        };
    // SAFETY: the storage lies in `words`, aligned to 8.
    unsafe {
        assert_eq!(
            aldis_oid_generator_init(
                storage.cast(),
                ALDIS_OID_GENERATOR_STORAGE_SIZE,
                ALDIS_OID_GALOIS,
                &raw mut h,
                null_mut()
            ),
            ALDIS_OK
        );
    }
    // Rejected: VC 5 carries VCA_SDUs (132.0 §4.1.4.6.3), and the generator does not advance.
    assert_eq!(
        begin(5, h, frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_FIELD
    );
    // The output inside the generator's storage.
    let in_storage = storage.cast::<u8>().wrapping_add(16);
    assert_eq!(begin(4, h, in_storage, &raw mut l), ALDIS_ERR_INVALID_FIELD);
    assert_eq!(begin(4, h, frame.as_mut_ptr(), &raw mut l), ALDIS_OK);
    assert_eq!((l.data_offset, l.data_len), (12, 18));
    // The first octets of the sequence of 132.0 Annex D (CMN-020).
    assert_eq!(frame[12..18], [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6]);
    // SAFETY: as above.
    unsafe {
        let mut n = 0;
        assert_eq!(
            aldis_tm_oid_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                2,
                4,
                0,
                0,
                frame.as_mut_ptr(),
                40,
                &raw mut n,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(n, 40);
        assert_eq!(frame[4..6], [0x1F, 0xFE], "Segment Length ID 11, FHP 0x7FE");
        aldis_oid_generator_deinit(h);
    }
    assert_eq!(
        begin(4, h, frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_HANDLE
    );
    assert_eq!(
        begin(4, null_mut(), frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_FIELD
    );
}
