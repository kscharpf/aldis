//! The contract of the TM part of the C ABI, called as a C caller calls it: the virtual channel
//! arrays (CABI-033, M2 plan D14), the security layout variants (API-070), the one-shot
//! encoders (API-023), and the argument checks (CABI-022) the vectors do not reach.
#![expect(clippy::unwrap_used, reason = "test-only harness (RS-018)")]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_FIRST_HEADER_POINTER, ALDIS_FIELD_FRAME_LEN, ALDIS_FIELD_FSH_LEN,
    ALDIS_FIELD_OCF_FLAG, ALDIS_FIELD_SPACECRAFT_ID, ALDIS_FIELD_SYNC_FLAG, ALDIS_FIELD_VC_ID,
};
use aldis_ffi::oid::*;
use aldis_ffi::tm::*;
use aldis_ffi::{ALDIS_MODE_LENIENT, ALDIS_MODE_STRICT, aldis_error_detail_t};

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
    let (sh, data, st) = ([0xA5_u8; 6], [0xD0_u8; 18], [0x5A_u8; 8]);
    let content = aldis_tm_content_t {
        security_header: sh.as_ptr(),
        security_header_len: 6,
        data: data.as_ptr(),
        data_len: 18,
        security_trailer: st.as_ptr(),
        security_trailer_len: 8,
        ..aldis_tm_content_t::default()
    };
    let mut n = 0;
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_tm_encode(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const fields,
                &raw const content,
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
        // Validation decodes with VC 4's layout, so the frame matches its channel (c-abi.md §11).
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
            ALDIS_OK
        );
        // With VC 4 configured for VCA_SDUs, the Synchronization Flag disagrees.
        let mut vca = vcs;
        vca[0].content = ALDIS_TM_CONTENT_VCA_SDU;
        assert_eq!(
            aldis_tm_validate(
                frame.as_ptr(),
                40,
                &raw const mc,
                vca.as_ptr(),
                2,
                ALDIS_MODE_STRICT,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            (d.field, d.expected, d.found),
            (ALDIS_FIELD_SYNC_FLAG, 1, 0)
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
fn encode_arguments_are_checked() {
    // req: CABI-022, CABI-011, API-022, TM-009
    let (mc, vcs) = (mc(), vcs());
    let fields = packets(4);
    let sh = [0xA5_u8; 6];
    let st = [0x5A_u8; 8];
    let mut frame = [0_u8; 64];
    let mut n = 0_usize;
    let encode = |content: *const aldis_tm_content_t, out: *mut u8, cap: usize| {
        let mut n = 0_usize;
        // SAFETY: every pointer is to a live local, or into `frame`; `content` is NULL or
        // points to a live struct whose buffers are live or into `frame`.
        unsafe {
            aldis_tm_encode(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const fields,
                content,
                out,
                cap,
                &raw mut n,
                null_mut(),
            )
        }
    };
    let data = [0xD0_u8; 18];
    let content = aldis_tm_content_t {
        security_header: sh.as_ptr(),
        security_header_len: 6,
        data: data.as_ptr(),
        data_len: 18,
        security_trailer: st.as_ptr(),
        security_trailer_len: 8,
        ..aldis_tm_content_t::default()
    };
    let out = frame.as_mut_ptr();
    assert_eq!(encode(&raw const content, out, 40), ALDIS_OK);
    assert_eq!(encode(null(), out, 40), ALDIS_ERR_INVALID_FIELD);
    // A NULL buffer with a length (CABI-022).
    let no_data = aldis_tm_content_t {
        data: null(),
        ..content
    };
    assert_eq!(encode(&raw const no_data, out, 40), ALDIS_ERR_INVALID_FIELD);
    // An OCF of neither 0 nor 4 octets, and one where none is configured: the frame's OCF Flag
    // (132.0 §4.1.2.4.3).
    let ocf = [1_u8, 2, 3, 4];
    for len in [2, 4] {
        let with_ocf = aldis_tm_content_t {
            ocf: ocf.as_ptr(),
            ocf_len: len,
            ..content
        };
        assert_eq!(
            encode(&raw const with_ocf, out, 40),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // The output overlapping any buffer of the content (CABI-022).
    let inside = |at: usize| frame.as_ptr().wrapping_add(at);
    for overlapping in [
        aldis_tm_content_t {
            security_header: inside(6),
            ..content
        },
        aldis_tm_content_t {
            data: inside(12),
            ..content
        },
        aldis_tm_content_t {
            security_trailer: inside(30),
            ..content
        },
        aldis_tm_content_t {
            fsh_data: inside(0),
            fsh_data_len: 1,
            ..content
        },
        aldis_tm_content_t {
            ocf: inside(36),
            ocf_len: 4,
            ..content
        },
    ] {
        assert_eq!(
            encode(&raw const overlapping, out, 40),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // The output too small: nothing written.
    frame.fill(0xEE);
    assert_eq!(
        encode(&raw const content, frame.as_mut_ptr(), 39),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert!(frame.iter().all(|&b| b == 0xEE));
    let mut vca = fields;
    vca.sync_flag = 1;
    let mut view = aldis_tm_frame_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        let mut d = detail();
        assert_eq!(
            aldis_tm_encode(
                &raw const mc,
                vcs.as_ptr(),
                2,
                &raw const vca,
                &raw const content,
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
            "encode rejects without writing"
        );
        // A NULL `fields` and a NULL `out_len` (CABI-022).
        for (f, l) in [(null(), &raw mut n), (&raw const fields, null_mut())] {
            assert_eq!(
                aldis_tm_encode(
                    &raw const mc,
                    vcs.as_ptr(),
                    2,
                    f,
                    &raw const content,
                    frame.as_mut_ptr(),
                    40,
                    l,
                    null_mut()
                ),
                ALDIS_ERR_INVALID_FIELD
            );
        }
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

/// A frame length above 4 095 octets is refused with the maximum (TM-001, OI-51).
#[test]
fn config_check_refuses_a_frame_length_above_4095() {
    // req: TM-001, CABI-022, VER-064
    let mut pc = mc().pc;
    for (len, ok) in [(4095, true), (4096, false)] {
        pc.frame_len = len;
        let mut d = detail();
        // SAFETY: both pointers are to live locals.
        let status = unsafe { aldis_tm_config_check(&raw const pc, &raw mut d) };
        if ok {
            assert_eq!(status, ALDIS_OK);
        } else {
            assert_eq!(status, ALDIS_ERR_INVALID_CONFIG);
            assert_eq!((d.field, d.expected), (ALDIS_FIELD_FRAME_LEN, 4095));
        }
    }
}

/// VCs 4 (SDLS packets), 5 (`VCA_SDUs`), and 6 (packets without SDLS, the only kind of channel
/// an OID frame may use).
fn oid_setup() -> (aldis_tm_mc_config_t, [aldis_tm_vc_config_t; 3]) {
    let (mc, [sdls, vca]) = (mc(), vcs());
    let vcs = [
        sdls,
        vca,
        aldis_tm_vc_config_t {
            vcid: 6,
            security_header_len: 0,
            security_trailer_len: 0,
            ..sdls
        },
    ];
    (mc, vcs)
}

#[test]
fn encode_oid_takes_a_generator_handle() {
    // req: TM-012, TM-010, CMN-021, CMN-022, CABI-022, CABI-032, GEN-042, API-020
    let (mc, vcs) = oid_setup();
    let mut storage = aldis_oid_generator_t {
        words: [0; ALDIS_OID_GENERATOR_WORDS],
    };
    let h = &raw mut storage;
    let mut frame = [0_u8; 40];
    let content = aldis_tm_content_t::default();
    let encode = |vcid: u8,
                  content: *const aldis_tm_content_t,
                  h: *mut aldis_oid_generator_t,
                  out: *mut u8,
                  cap: usize| {
        let mut n = 0_usize;
        // SAFETY: `mc`, `vcs`, and `content` are live; `out` points to `cap` live octets; `h` is
        // a handle into live storage or invalid.
        let status = unsafe {
            aldis_tm_encode_oid(
                &raw const mc,
                vcs.as_ptr(),
                3,
                vcid,
                0,
                0,
                content,
                h,
                out,
                cap,
                &raw mut n,
                null_mut(),
            )
        };
        (status, n)
    };
    // SAFETY: `h` points to live storage.
    unsafe {
        assert_eq!(
            aldis_oid_generator_init(h, ALDIS_OID_GALOIS, null_mut()),
            ALDIS_OK
        );
    }
    let c = &raw const content;
    // Rejected: VC 5 carries VCA_SDUs (132.0 §4.1.4.6.3), and the generator does not advance.
    assert_eq!(
        encode(5, c, h, frame.as_mut_ptr(), 40).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // Rejected: VC 4 uses SDLS (132.0 §6.4.6.2), and nothing is written.
    assert_eq!(
        encode(4, c, h, frame.as_mut_ptr(), 40).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        encode(3, c, h, frame.as_mut_ptr(), 40).0,
        ALDIS_ERR_UNKNOWN_CHANNEL
    );
    assert_eq!(
        encode(6, c, h, frame.as_mut_ptr(), 39).0,
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert_eq!(
        encode(6, null(), h, frame.as_mut_ptr(), 40).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(frame, [0; 40]);
    // The output inside the generator's storage.
    let in_storage = h.cast::<u8>().wrapping_add(16);
    assert_eq!(encode(6, c, h, in_storage, 40).0, ALDIS_ERR_INVALID_FIELD);
    // None of the failures advanced the generator: this is the start of the sequence.
    assert_eq!(encode(6, c, h, frame.as_mut_ptr(), 40), (ALDIS_OK, 40));
    // The first octets of the sequence of 132.0 Annex D (CMN-020).
    assert_eq!(frame[6..12], [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6]);
    assert_eq!(frame[4..6], [0x1F, 0xFE], "Segment Length ID 11, FHP 0x7FE");
    // SAFETY: as above.
    unsafe { aldis_oid_generator_deinit(h) };
    assert_eq!(
        encode(6, c, h, frame.as_mut_ptr(), 40).0,
        ALDIS_ERR_INVALID_HANDLE
    );
    assert_eq!(
        encode(6, c, null_mut(), frame.as_mut_ptr(), 40).0,
        ALDIS_ERR_INVALID_FIELD
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per argument rule")]
fn encode_oid_forms_agree_and_check_the_content() {
    // req: TM-012, TM-010, CMN-020, CMN-022, CABI-022, GEN-042, API-020
    let (mut mc, mut vcs) = oid_setup();
    mc.ocf = 1;
    vcs[2].fsh_len = 4;
    let mut storage = [
        aldis_oid_generator_t {
            words: [0; ALDIS_OID_GENERATOR_WORDS],
        },
        aldis_oid_generator_t {
            words: [0; ALDIS_OID_GENERATOR_WORDS],
        },
    ];
    let handles = [&raw mut storage[0], &raw mut storage[1]];
    for (h, kind) in handles
        .into_iter()
        .zip([ALDIS_OID_FIBONACCI, ALDIS_OID_GALOIS])
    {
        // SAFETY: `h` points to live storage.
        unsafe {
            assert_eq!(aldis_oid_generator_init(h, kind, null_mut()), ALDIS_OK);
        }
    }
    let (fsh, ocf) = ([1_u8, 2, 3], [0x01_u8, 0, 0, 0]);
    let good = aldis_tm_content_t {
        fsh_data: fsh.as_ptr(),
        fsh_data_len: 3,
        ocf: ocf.as_ptr(),
        ocf_len: 4,
        ..aldis_tm_content_t::default()
    };
    let call = |content: &aldis_tm_content_t, h, out: &mut [u8]| {
        let (mut n, mut d) = (0_usize, detail());
        // SAFETY: every pointer is to a live local or into `out`.
        let s = unsafe {
            aldis_tm_encode_oid(
                &raw const mc,
                vcs.as_ptr(),
                3,
                6,
                1,
                2,
                content,
                h,
                out.as_mut_ptr(),
                out.len(),
                &raw mut n,
                &raw mut d,
            )
        };
        (s, n, d)
    };
    let (mut a, mut b) = ([0_u8; 40], [0_u8; 40]);
    assert_eq!(call(&good, handles[0], &mut a).0, ALDIS_OK);
    assert_eq!(call(&good, handles[1], &mut b).0, ALDIS_OK);
    assert_eq!(a, b, "the Fibonacci and Galois forms give the same frame");
    assert_eq!(a[6..10], [0x03, 1, 2, 3], "Secondary Header");
    assert_eq!(a[34..38], ocf);
    // Rejected content leaves the buffer and the generator untouched.
    let mut out = [0xEE_u8; 40];
    let short = aldis_tm_content_t {
        fsh_data_len: 2,
        ..good
    };
    let (s, _, d) = call(&short, handles[0], &mut out);
    assert_eq!(
        (s, d.field),
        (ALDIS_ERR_LENGTH_MISMATCH, ALDIS_FIELD_FSH_LEN)
    );
    let no_ocf = aldis_tm_content_t {
        ocf: null(),
        ocf_len: 0,
        ..good
    };
    let (s, _, d) = call(&no_ocf, handles[0], &mut out);
    assert_eq!(
        (s, d.field),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_OCF_FLAG)
    );
    let bad_ocf = aldis_tm_content_t { ocf_len: 2, ..good };
    assert_eq!(
        call(&bad_ocf, handles[0], &mut out).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // A buffer the call does not take must be empty (CABI-022).
    let junk = [0_u8; 4];
    for forbidden in [
        aldis_tm_content_t {
            data: junk.as_ptr(),
            data_len: 4,
            ..good
        },
        aldis_tm_content_t {
            security_header: junk.as_ptr(),
            security_header_len: 4,
            ..good
        },
        aldis_tm_content_t {
            security_trailer: junk.as_ptr(),
            security_trailer_len: 4,
            ..good
        },
    ] {
        assert_eq!(
            call(&forbidden, handles[0], &mut out).0,
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // The output overlapping the content.
    let overlapping = aldis_tm_content_t {
        fsh_data: out.as_ptr().wrapping_add(7),
        ..good
    };
    assert_eq!(
        call(&overlapping, handles[0], &mut out).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(out, [0xEE; 40]);
    // The second frame continues the sequence, equal in both forms.
    let first = a;
    assert_eq!(call(&good, handles[0], &mut a).0, ALDIS_OK);
    assert_eq!(call(&good, handles[1], &mut b).0, ALDIS_OK);
    assert_eq!(a, b);
    assert_ne!(a[10..34], first[10..34]);
    // SAFETY: both handles are live.
    unsafe {
        aldis_oid_generator_deinit(handles[0]);
        aldis_oid_generator_deinit(handles[1]);
    }
}

/// One call of `aldis_tm_encode` with `fields` on `mc`, `vcs` into a fresh 2100-octet buffer:
/// the status and the detail, with the buffer unchanged.
fn write_all(
    mc: &aldis_tm_mc_config_t,
    vcs: &[aldis_tm_vc_config_t],
    fields: &aldis_tm_fields_t,
    data_len: usize,
) -> (u32, aldis_error_detail_t) {
    let data = vec![0xD0_u8; data_len];
    let content = aldis_tm_content_t {
        data: data.as_ptr(),
        data_len,
        ..aldis_tm_content_t::default()
    };
    let mut frame = vec![0xEE_u8; 2100];
    let (mut d, mut n) = (detail(), 0_usize);
    // SAFETY: every pointer is to a live local.
    let status = unsafe {
        aldis_tm_encode(
            mc,
            vcs.as_ptr(),
            vcs.len(),
            fields,
            &raw const content,
            frame.as_mut_ptr(),
            frame.len(),
            &raw mut n,
            &raw mut d,
        )
    };
    assert!(frame.iter().all(|&b| b == 0xEE), "encode wrote on error");
    (status, d)
}

#[test]
fn an_only_idle_data_frame_on_an_sdls_vc_is_refused_or_flagged() {
    // req: TM-010, TM-011, API-031, API-032, CABI-012 (OI-54)
    let (mc, vcs) = (mc(), vcs());
    // The frame of VC 4 with FHP 0x7FE (Synchronization Flag 0, Segment Length ID 11).
    let mut frame = sdls_frame();
    frame[4..6].copy_from_slice(&[0x1F, 0xFE]);
    aldis::core::crc::write_fecf(&mut frame).unwrap();
    let mut v = aldis_tm_frame_t::default();
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    unsafe {
        for decode in [aldis_tm_decode_with, aldis_tm_decode_unchecked_with] {
            assert_eq!(
                decode(
                    frame.as_ptr(),
                    40,
                    &raw const mc,
                    vcs.as_ptr(),
                    2,
                    ALDIS_MODE_STRICT,
                    &raw mut v,
                    &raw mut d
                ),
                ALDIS_ERR_INVALID_FIELD
            );
            assert_eq!(d.field, ALDIS_FIELD_VC_ID);
            assert_eq!(
                decode(
                    frame.as_ptr(),
                    40,
                    &raw const mc,
                    vcs.as_ptr(),
                    2,
                    ALDIS_MODE_LENIENT,
                    &raw mut v,
                    &raw mut d
                ),
                ALDIS_OK
            );
            assert_eq!(v.anomalies, ALDIS_TM_ANOMALY_ONLY_IDLE_DATA_ON_SDLS_VC);
            assert_eq!(v.pointer, ALDIS_TM_POINTER_ONLY_IDLE_DATA);
        }
        // Validation refuses it in both modes: the managed parameters forbid it.
        for mode in [ALDIS_MODE_STRICT, ALDIS_MODE_LENIENT] {
            assert_eq!(
                aldis_tm_validate(
                    frame.as_ptr(),
                    40,
                    &raw const mc,
                    vcs.as_ptr(),
                    2,
                    mode,
                    &raw mut d
                ),
                ALDIS_ERR_INVALID_FIELD
            );
            assert_eq!(d.field, ALDIS_FIELD_VC_ID);
        }
        // The decoder without the VC array has no SDLS knowledge, so it reads the layout of a
        // channel without SDLS and flags nothing.
        assert_eq!(
            aldis_tm_decode(
                frame.as_ptr(),
                40,
                &raw const mc.pc,
                ALDIS_MODE_LENIENT,
                &raw mut v,
                &raw mut d
            ),
            ALDIS_OK
        );
        assert_eq!(v.anomalies, 0);
    }
}

#[test]
fn packet_offsets_above_0x7fd_fail_as_the_encoder_checks_them() {
    // req: TM-009, CABI-020, CABI-022, RS-020
    let (mc, vcs) = (mc(), vcs());
    let beyond = |vcid| aldis_tm_fields_t {
        packet_offset: 0x7FE,
        ..packets(vcid)
    };
    // Inside the 18-octet data field of VC 4: the largest valid offset, 17.
    let (status, d) = write_all(&mc, &vcs, &beyond(4), 18);
    assert_eq!(
        (status, d.field, d.detail, d.expected, d.found),
        (
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_FIRST_HEADER_POINTER,
            ALDIS_DETAIL_EXPECTED,
            17,
            0x7FE
        )
    );
    for offset in [0x7FF, u16::MAX] {
        let f = aldis_tm_fields_t {
            packet_offset: offset,
            ..packets(4)
        };
        let (status, d) = write_all(&mc, &vcs, &f, 18);
        assert_eq!(
            (status, d.expected, d.found),
            (ALDIS_ERR_OUT_OF_RANGE, 17, u64::from(offset))
        );
    }
    // A data field longer than the pointer reaches: the largest offset, 0x7FD.
    let big = aldis_tm_mc_config_t {
        pc: aldis_tm_config_t {
            fecf: 0,
            frame_len: 2100,
            ..mc.pc
        },
        ..mc
    };
    let plain = [aldis_tm_vc_config_t {
        security_header_len: 0,
        security_trailer_len: 0,
        ..vcs[0]
    }];
    let (status, d) = write_all(&big, &plain, &beyond(4), 2094);
    assert_eq!(
        (status, d.field, d.expected, d.found),
        (
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_FIRST_HEADER_POINTER,
            0x7FD,
            0x7FE
        )
    );
    // The checks before the pointer's come first: the VC, then the Synchronization Flag.
    let (status, d) = write_all(&mc, &vcs, &beyond(3), 18);
    assert_eq!(
        (status, d.field),
        (ALDIS_ERR_UNKNOWN_CHANNEL, ALDIS_FIELD_VC_ID)
    );
    let (status, d) = write_all(&mc, &vcs, &beyond(5), 18);
    assert_eq!(
        (status, d.field, d.expected, d.found),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_SYNC_FLAG, 1, 0)
    );
}
