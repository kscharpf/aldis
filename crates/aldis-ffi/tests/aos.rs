//! The contract of the AOS part of the C ABI, called as a C caller calls it: the virtual
//! channel arrays of one master channel (CABI-033, M2 plan D14, c-abi.md §11), the frame
//! decoder with the master channel's layouts (API-070), the FHEC and data unit decoders, the
//! stateless writers (API-023), and the argument checks (CABI-022) the vectors do not reach.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test-only harness (RS-018)"
)]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis::aos::{AosEdition, Bpdu, Mpdu};
use aldis::core::DecodeMode;
use aldis_ffi::aos::*;
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_BITSTREAM_DATA_POINTER, ALDIS_FIELD_DATA_FIELD_CONTENT,
    ALDIS_FIELD_FIRST_HEADER_POINTER, ALDIS_FIELD_SPACECRAFT_ID, ALDIS_FIELD_VC_ID,
};
use aldis_ffi::oid::*;
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

/// 40-octet frames with the FHEC, a 4-octet Insert Zone, and an FECF, on SCID 0x0AB.
fn mc() -> aldis_aos_mc_config_t {
    aldis_aos_mc_config_t {
        struct_size: size::<aldis_aos_mc_config_t>(),
        scid: 0x0AB,
        pc: aldis_aos_config_t {
            struct_size: size::<aldis_aos_config_t>(),
            fhec: 1,
            fecf: 1,
            edition: ALDIS_AOS_EDITION_ISSUE5,
            frame_len: 40,
            insert_zone_len: 4,
        },
    }
}

/// VC 0 carries `M_PDUs` with an OCF and SDLS 2/1, VC 1 `B_PDUs`, VC 2 `VCA_SDUs`.
fn vcs() -> [aldis_aos_vc_config_t; 3] {
    let vc = aldis_aos_vc_config_t {
        struct_size: size::<aldis_aos_vc_config_t>(),
        vcid: 0,
        content: ALDIS_AOS_CONTENT_MPDU,
        ocf: 1,
        security_header_len: 2,
        security_trailer_len: 1,
    };
    let plain = aldis_aos_vc_config_t {
        ocf: 0,
        security_header_len: 0,
        security_trailer_len: 0,
        ..vc
    };
    [
        vc,
        aldis_aos_vc_config_t {
            vcid: 1,
            content: ALDIS_AOS_CONTENT_BPDU,
            ..plain
        },
        aldis_aos_vc_config_t {
            vcid: 2,
            content: ALDIS_AOS_CONTENT_VCA_SDU,
            ..plain
        },
    ]
}

const fn fields(vcid: u8, pointer: u8, pointer_value: u16) -> aldis_aos_fields_t {
    aldis_aos_fields_t {
        vcid,
        vc_frame_count: 0x12_3456,
        replay_flag: 1,
        cycle_use_flag: 1,
        cycle: 9,
        pointer,
        pointer_value,
    }
}

/// A frame of VC 0: Insert Zone `11…`, Security Header `A5…`, Packet Zone `D0…`, Security
/// Trailer `5A`, OCF `01000000`, written in two stateless steps.
fn sdls_frame() -> Vec<u8> {
    let (mc, vcs) = (mc(), vcs());
    let f = fields(0, ALDIS_AOS_POINTER_STARTS_AT, 3);
    let mut frame = vec![0xEE_u8; 40];
    let mut l = aldis_aos_layout_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_aos_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const f,
                frame.as_mut_ptr(),
                40,
                &raw mut l,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            (
                l.insert_zone_offset,
                l.security_header_offset,
                l.data_offset
            ),
            (8, 12, 14)
        );
        assert_eq!(
            (
                l.data_len,
                l.zone_offset,
                l.zone_len,
                l.security_trailer_offset
            ),
            (19, 16, 17, 33)
        );
        assert_eq!((l.ocf_offset, l.ocf_len, l.fecf_offset), (34, 4, 38));
        assert!(frame[16..38].iter().all(|&b| b == 0), "zero-filled");
        frame[8..12].fill(0x11);
        frame[12..14].fill(0xA5);
        frame[16..33].fill(0xD0);
        frame[33] = 0x5A;
        frame[34..38].copy_from_slice(&[0x01, 0, 0, 0]);
        let mut n = 0;
        assert_eq!(
            aldis_aos_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const f,
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
    vc: aldis_aos_vc_config_t,
    later: u32,
}

#[test]
fn vc_arrays_step_by_their_struct_size() {
    // req: CABI-033, CABI-022, API-040, AOS-020
    let mc = mc();
    let check = |vcs: *const aldis_aos_vc_config_t, n: usize| {
        // SAFETY: `mc` is live; `vcs` is NULL or points to `n` live structs.
        unsafe { aldis_aos_mc_config_check(&raw const mc, vcs, n, null_mut()) }
    };
    let base = vcs();
    assert_eq!(check(base.as_ptr(), 3), ALDIS_OK);
    // A master channel with no virtual channel receives nothing (OI-55).
    assert_eq!(check(null(), 0), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(check(null(), 1), ALDIS_ERR_INVALID_FIELD);
    // A longer struct of a later header: the stride is its struct_size.
    let later = base.map(|vc| LaterVc {
        vc: aldis_aos_vc_config_t {
            struct_size: size::<LaterVc>(),
            ..vc
        },
        later: u32::MAX,
    });
    assert_eq!(check(later.as_ptr().cast(), 3), ALDIS_OK);
    // Every struct repeats the stride.
    let mut mixed = later;
    mixed[2].vc.struct_size = size::<aldis_aos_vc_config_t>();
    assert_eq!(check(mixed.as_ptr().cast(), 3), ALDIS_ERR_INVALID_FIELD);
    // Shorter than the struct this library knows, or not a multiple of its alignment.
    for struct_size in [4, size::<aldis_aos_vc_config_t>() + 2] {
        let mut v = later;
        for e in &mut v {
            e.vc.struct_size = struct_size;
        }
        assert_eq!(check(v.as_ptr().cast(), 3), ALDIS_ERR_INVALID_FIELD);
    }
    // A misaligned array, and a count whose array would wrap the address space.
    let words = [0_u32; 8];
    let misaligned = words.as_ptr().cast::<u8>().wrapping_add(1).cast();
    assert_eq!(check(misaligned, 1), ALDIS_ERR_INVALID_FIELD);
    assert_eq!(
        check(base.as_ptr(), usize::MAX / 4),
        ALDIS_ERR_INVALID_FIELD
    );
    // The embedded physical channel carries its own struct_size (CABI-033), and its edition
    // and flags take only their constants (CABI-010).
    for small in [
        aldis_aos_mc_config_t {
            struct_size: 4,
            ..mc
        },
        aldis_aos_mc_config_t {
            pc: aldis_aos_config_t {
                struct_size: 4,
                ..mc.pc
            },
            ..mc
        },
        aldis_aos_mc_config_t {
            pc: aldis_aos_config_t {
                edition: 2,
                ..mc.pc
            },
            ..mc
        },
        aldis_aos_mc_config_t {
            pc: aldis_aos_config_t { fhec: 2, ..mc.pc },
            ..mc
        },
    ] {
        // SAFETY: as above.
        let r =
            unsafe { aldis_aos_mc_config_check(&raw const small, base.as_ptr(), 3, null_mut()) };
        assert_eq!(r, ALDIS_ERR_INVALID_FIELD);
    }
}

#[test]
fn a_master_channel_holds_vcids_0_to_62() {
    // req: API-040, GEN-011, GEN-060, CABI-030
    let mut mc = mc();
    mc.pc.fhec = 0;
    mc.pc.insert_zone_len = 0;
    let mc = mc;
    let one = aldis_aos_vc_config_t {
        struct_size: size::<aldis_aos_vc_config_t>(),
        content: ALDIS_AOS_CONTENT_VCA_SDU,
        ..aldis_aos_vc_config_t::default()
    };
    let mut all: Vec<aldis_aos_vc_config_t> = (0..=62_u8)
        .map(|vcid| aldis_aos_vc_config_t { vcid, ..one })
        .collect();
    let check = |vcs: &[aldis_aos_vc_config_t]| {
        let mut d = detail();
        // SAFETY: `mc`, `vcs`, and `d` are live.
        let r = unsafe {
            aldis_aos_mc_config_check(&raw const mc, vcs.as_ptr(), vcs.len(), &raw mut d)
        };
        (r, d.field)
    };
    assert_eq!(
        check(&all).0,
        ALDIS_OK,
        "every data VCID: no capacity limit"
    );
    // One more names a VCID twice, however long the array (732.0 Table 5-3).
    all.push(aldis_aos_vc_config_t { vcid: 7, ..one });
    assert_eq!(check(&all), (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID));
    all.extend((0..10_u8).map(|vcid| aldis_aos_vc_config_t { vcid, ..one }));
    assert_eq!(check(&all), (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID));
    // A struct that does not parse fails first, wherever it lies.
    all.push(aldis_aos_vc_config_t { content: 9, ..one });
    assert_eq!(check(&all).0, ALDIS_ERR_INVALID_FIELD);
    // VCID 63 is the OID frames' (Table 5-3 note 2), 64 does not fit six bits.
    let oid = [aldis_aos_vc_config_t { vcid: 63, ..one }];
    assert_eq!(check(&oid), (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID));
    let wide = [aldis_aos_vc_config_t { vcid: 64, ..one }];
    assert_eq!(check(&wide), (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_VC_ID));
    // An SCID above 255 in Issue 4 (AOS-002), or above 1023 in any edition.
    let mut issue4 = mc;
    issue4.pc.edition = ALDIS_AOS_EDITION_ISSUE4;
    issue4.scid = 256;
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    let r = unsafe { aldis_aos_mc_config_check(&raw const issue4, oid.as_ptr(), 0, &raw mut d) };
    assert_eq!(
        (r, d.field),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID),
        "no VC: rejected before the SCID is held to the edition"
    );
    let first = [one];
    // SAFETY: as above.
    let r = unsafe { aldis_aos_mc_config_check(&raw const issue4, first.as_ptr(), 1, &raw mut d) };
    assert_eq!(
        (r, d.field),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_SPACECRAFT_ID)
    );
    let wide_scid = aldis_aos_mc_config_t { scid: 1024, ..mc };
    // SAFETY: as above.
    let r =
        unsafe { aldis_aos_mc_config_check(&raw const wide_scid, first.as_ptr(), 1, &raw mut d) };
    assert_eq!(
        (r, d.field),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_SPACECRAFT_ID)
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per lookup rule")]
fn the_decoder_takes_the_master_channels_vcs() {
    // req: AOS-010, AOS-014, AOS-016, AOS-017, API-070, API-072, CABI-012
    let (mc, vcs) = (mc(), vcs());
    let frame = sdls_frame();
    let decode =
        |bytes: &[u8], mc: &aldis_aos_mc_config_t, vcs: &[aldis_aos_vc_config_t], mode: u32| {
            let mut view = aldis_aos_frame_t::default();
            let mut d = detail();
            // SAFETY: every pointer is to a live local or slice.
            let status = unsafe {
                aldis_aos_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    mode,
                    &raw mut view,
                    &raw mut d,
                )
            };
            (status, d.field, view)
        };
    let (status, _, view) = decode(&frame, &mc, &vcs, ALDIS_MODE_STRICT);
    assert_eq!(status, ALDIS_OK);
    assert_eq!(
        (
            view.header.scid,
            view.header.vcid,
            view.header.vc_frame_count
        ),
        (0x0AB, 0, 0x12_3456)
    );
    assert!(view.header.replay_flag && view.header.cycle_use_flag && view.header.has_fhec);
    assert_eq!(
        (view.header.cycle_field, view.header.corrected_symbols),
        (9, 0)
    );
    assert_eq!(view.header.octets[..6], frame[..6]);
    assert_eq!(view.header.fhec, u16::from_be_bytes([frame[6], frame[7]]));
    assert_eq!(
        (
            view.pdu.content,
            view.pdu.pointer,
            view.pdu.pointer_value,
            view.pdu.pointer_raw
        ),
        (ALDIS_AOS_CONTENT_MPDU, ALDIS_AOS_POINTER_STARTS_AT, 3, 3)
    );
    assert_eq!(
        (
            view.layout.security_header_len,
            view.layout.zone_offset,
            view.layout.ocf_len
        ),
        (2, 16, 4)
    );
    assert!(view.has_fecf);
    assert_eq!(view.fecf, u16::from_be_bytes([frame[38], frame[39]]));
    // A symbol hit in the header is corrected; the FECF is checked over the corrected header.
    let mut hit = frame.clone();
    hit[1] ^= 0x01;
    let (status, _, again) = decode(&hit, &mc, &vcs, ALDIS_MODE_LENIENT);
    assert_eq!(status, ALDIS_OK);
    assert_eq!((again.header.vcid, again.header.corrected_symbols), (0, 1));
    assert_eq!(
        again.header.octets, view.header.octets,
        "the corrected octets"
    );
    // Without SDLS on VC 0 the M_PDU Header would be the Security Header `A5A5`, a pointer
    // beyond the Packet Zone: the layouts are the master channel's (API-070).
    let mut no_sdls = vcs;
    no_sdls[0].security_header_len = 0;
    no_sdls[0].security_trailer_len = 0;
    let (status, field, _) = decode(&frame, &mc, &no_sdls, ALDIS_MODE_STRICT);
    assert_eq!(
        (status, field),
        (ALDIS_ERR_MALFORMED, ALDIS_FIELD_FIRST_HEADER_POINTER)
    );
    // As a VCA_SDU without SDLS, the data field starts after the Insert Zone.
    no_sdls[0].content = ALDIS_AOS_CONTENT_VCA_SDU;
    let (status, _, again) = decode(&frame, &mc, &no_sdls, ALDIS_MODE_STRICT);
    assert_eq!(status, ALDIS_OK);
    assert_eq!(
        (
            again.layout.data_offset,
            again.layout.data_len,
            again.layout.zone_offset,
            again.pdu.pointer
        ),
        (12, 22, 12, ALDIS_AOS_POINTER_NONE)
    );
    // Another SCID, an unknown VC.
    let other = aldis_aos_mc_config_t { scid: 0x0AC, ..mc };
    let (status, field, _) = decode(&frame, &other, &vcs, ALDIS_MODE_STRICT);
    assert_eq!(
        (status, field),
        (ALDIS_ERR_UNKNOWN_CHANNEL, ALDIS_FIELD_SPACECRAFT_ID)
    );
    let (status, field, _) = decode(&frame, &mc, &vcs[1..], ALDIS_MODE_STRICT);
    assert_eq!(
        (status, field),
        (ALDIS_ERR_UNKNOWN_CHANNEL, ALDIS_FIELD_VC_ID)
    );
    // An OID frame needs a master channel with a VC, as a `VcTable` does (Table 5-1).
    let mut oid = [0_u8; 40];
    let oid_fields = fields(ALDIS_AOS_OID_VCID, ALDIS_AOS_POINTER_NONE, 0);
    let mut words = [0_u64; ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
    let mut generator: *mut aldis_oid_generator_t = null_mut();
    let mut layout = aldis_aos_layout_t::default();
    // SAFETY: every pointer is to a live local; the storage is aligned and of the published
    // size.
    unsafe {
        assert_eq!(
            aldis_oid_generator_init(
                words.as_mut_ptr().cast(),
                ALDIS_OID_GENERATOR_STORAGE_SIZE,
                ALDIS_OID_FIBONACCI,
                &raw mut generator,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            aldis_aos_oid_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const oid_fields,
                generator,
                oid.as_mut_ptr(),
                40,
                &raw mut layout,
                null_mut()
            ),
            ALDIS_OK
        );
        let mut n = 0;
        assert_eq!(
            aldis_aos_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const oid_fields,
                oid.as_mut_ptr(),
                40,
                &raw mut n,
                null_mut()
            ),
            ALDIS_OK
        );
        aldis_oid_generator_deinit(generator);
    }
    let (status, _, again) = decode(&oid, &mc, &vcs, ALDIS_MODE_STRICT);
    assert_eq!(status, ALDIS_OK);
    assert_eq!(
        (again.header.vcid, again.pdu.content, again.pdu.pointer),
        (63, ALDIS_AOS_CONTENT_IDLE_DATA, ALDIS_AOS_POINTER_NONE)
    );
    assert_eq!(
        (
            again.layout.data_offset,
            again.layout.zone_offset,
            again.layout.ocf_len
        ),
        (12, 12, 0)
    );
    assert_eq!(oid[12..16], [0xFF; 4]);
    // A master channel with no VC is not a configuration (AOS-020, OI-55).
    let (status, field, _) = decode(&oid, &mc, &[], ALDIS_MODE_STRICT);
    assert_eq!(
        (status, field),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID)
    );
    // The mode takes only its constants.
    assert_eq!(decode(&frame, &mc, &vcs, 7).0, ALDIS_ERR_INVALID_FIELD);
}

#[test]
fn the_header_decodes_alone() {
    // req: AOS-002, AOS-013, AOS-014, API-013, API-014, CABI-012
    let (mc, frame) = (mc(), sdls_frame());
    let mut h = aldis_aos_header_t::default();
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_aos_header_decode(frame.as_ptr(), 7, &raw const mc.pc, &raw mut h, &raw mut d),
            ALDIS_ERR_INCOMPLETE
        );
        assert_eq!(d.needed, 8, "the header with its FHEC");
        assert_eq!(
            aldis_aos_header_decode(frame.as_ptr(), 8, &raw const mc.pc, &raw mut h, &raw mut d),
            ALDIS_OK
        );
        assert_eq!(
            (h.scid, h.vcid, &h.octets[..], h.has_fhec),
            (0x0AB, 0, &frame[..8], true)
        );
        let mut n = 0;
        assert_eq!(
            aldis_aos_peek_length(frame.as_ptr(), 1, &raw const mc.pc, &raw mut n, null_mut()),
            ALDIS_OK
        );
        assert_eq!(n, 40);
        // Three symbols hit: beyond what the FHEC corrects (AOS-014).
        let mut bad = frame.clone();
        bad[0] ^= 0x01;
        bad[1] ^= 0x10;
        bad[5] ^= 0x01;
        assert_eq!(
            aldis_aos_header_decode(bad.as_ptr(), 8, &raw const mc.pc, &raw mut h, null_mut()),
            ALDIS_ERR_HEADER_UNCORRECTABLE
        );
        assert_eq!(
            aldis_aos_peek_length(bad.as_ptr(), 8, &raw const mc.pc, &raw mut n, null_mut()),
            ALDIS_ERR_HEADER_UNCORRECTABLE
        );
        assert_eq!(
            aldis_aos_header_decode(frame.as_ptr(), 8, &raw const mc.pc, null_mut(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn the_fhec_takes_whole_headers() {
    // req: AOS-013, AOS-014, CABI-022
    let frame = sdls_frame();
    let mut fhec = 0_u16;
    let mut c = aldis_aos_corrected_header_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_aos_fhec_encode(frame.as_ptr(), 6, &raw mut fhec, null_mut()),
            ALDIS_OK
        );
        assert_eq!(fhec.to_be_bytes(), [frame[6], frame[7]]);
        for len in [5, 8] {
            assert_eq!(
                aldis_aos_fhec_encode(frame.as_ptr(), len, &raw mut fhec, null_mut()),
                ALDIS_ERR_INVALID_FIELD
            );
        }
        let mut hit = frame[..8].to_vec();
        hit[5] ^= 0x80;
        hit[7] ^= 0x01;
        assert_eq!(
            aldis_aos_fhec_decode(hit.as_ptr(), 8, &raw mut c, null_mut()),
            ALDIS_OK
        );
        assert_eq!((&c.octets[..], c.corrected_symbols), (&frame[..8], 2));
        assert_eq!(hit[5], frame[5] ^ 0x80, "the input is not changed");
        for len in [6, 9] {
            assert_eq!(
                aldis_aos_fhec_decode(frame.as_ptr(), len, &raw mut c, null_mut()),
                ALDIS_ERR_INVALID_FIELD
            );
        }
        assert_eq!(
            aldis_aos_fhec_decode(null(), 0, &raw mut c, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn the_data_units_decode_alone() {
    // req: AOS-003, AOS-017, API-032, GEN-043, CABI-012
    let mut p = aldis_aos_pdu_t::default();
    let run = |field: &[u8], edition: Option<u8>, mode: u32, p: &mut aldis_aos_pdu_t| {
        // SAFETY: every pointer is to a live local or slice.
        unsafe {
            match edition {
                Some(e) => {
                    aldis_aos_mpdu_decode(field.as_ptr(), field.len(), e, mode, p, null_mut())
                }
                None => aldis_aos_bpdu_decode(field.as_ptr(), field.len(), mode, p, null_mut()),
            }
        }
    };
    // An Issue 4 M_PDU with its spare set: rejected strictly, flagged leniently.
    let issue4 = [0x08_u8, 0x02, 0xAA, 0xBB, 0xCC];
    assert_eq!(
        run(
            &issue4,
            Some(ALDIS_AOS_EDITION_ISSUE4),
            ALDIS_MODE_STRICT,
            &mut p
        ),
        ALDIS_ERR_RESERVED_VALUE
    );
    assert_eq!(
        run(
            &issue4,
            Some(ALDIS_AOS_EDITION_ISSUE4),
            ALDIS_MODE_LENIENT,
            &mut p
        ),
        ALDIS_OK
    );
    let rust = Mpdu::decode(&issue4, AosEdition::Issue4Compatible, DecodeMode::Lenient).unwrap();
    assert_eq!(
        (
            p.pointer,
            p.pointer_value,
            p.pointer_raw,
            p.spare,
            p.anomalies
        ),
        (
            ALDIS_AOS_POINTER_STARTS_AT,
            2,
            rust.first_header_pointer(),
            rust.issue4_spare(),
            rust.anomalies().bits()
        )
    );
    assert_eq!(p.anomalies, ALDIS_AOS_ANOMALY_ISSUE4_SPARE_SET);
    // In Issue 5 the same octets are a pointer beyond the zone.
    assert_eq!(
        run(
            &issue4,
            Some(ALDIS_AOS_EDITION_ISSUE5),
            ALDIS_MODE_LENIENT,
            &mut p
        ),
        ALDIS_ERR_MALFORMED
    );
    assert_eq!(
        run(&issue4, Some(2), ALDIS_MODE_STRICT, &mut p),
        ALDIS_ERR_INVALID_FIELD
    );
    // A B_PDU: 2 valid bits of a 1-octet zone, with the spare set in lenient mode.
    let bpdu = [0x40_u8, 0x01, 0xC0];
    assert_eq!(run(&bpdu, None, ALDIS_MODE_LENIENT, &mut p), ALDIS_OK);
    let rust = Bpdu::decode(&bpdu, DecodeMode::Lenient).unwrap();
    assert_eq!(
        (p.content, p.pointer, p.pointer_value, p.spare, p.anomalies),
        (
            ALDIS_AOS_CONTENT_BPDU,
            ALDIS_AOS_POINTER_VALID_BITS,
            2,
            rust.spare(),
            ALDIS_AOS_ANOMALY_BPDU_SPARE_SET
        )
    );
    assert_eq!(
        run(&bpdu, None, ALDIS_MODE_STRICT, &mut p),
        ALDIS_ERR_RESERVED_VALUE
    );
    assert_eq!(
        run(&bpdu[..2], None, ALDIS_MODE_STRICT, &mut p),
        ALDIS_ERR_MALFORMED
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per argument rule")]
fn writer_arguments_are_checked() {
    // req: CABI-022, CABI-011, API-022, AOS-017, AOS-019
    let (mc, vcs) = (mc(), vcs());
    let mpdu_fields = fields(0, ALDIS_AOS_POINTER_STARTS_AT, 3);
    let (iz, sh, zone, st, ocf) = (
        [0x11_u8; 4],
        [0xA5_u8; 2],
        [0xD0_u8; 17],
        [0x5A_u8; 1],
        [1_u8, 0, 0, 0],
    );
    let mut frame = [0_u8; 64];
    let encode = |mpdu_fields: *const aldis_aos_fields_t,
                  content: *const aldis_aos_content_t,
                  out: *mut u8,
                  cap: usize| {
        let mut len = 0_usize;
        let mut det = detail();
        // SAFETY: every pointer is to a live local, or into `frame`; `content` is NULL or
        // points to a live struct whose buffers are live or into `frame`.
        let r = unsafe {
            aldis_aos_encode(
                &raw const mc,
                vcs.as_ptr(),
                3,
                mpdu_fields,
                content,
                out,
                cap,
                &raw mut len,
                &raw mut det,
            )
        };
        (r, det.field)
    };
    let content = aldis_aos_content_t {
        insert_zone: iz.as_ptr(),
        insert_zone_len: 4,
        security_header: sh.as_ptr(),
        security_header_len: 2,
        zone: zone.as_ptr(),
        zone_len: 17,
        security_trailer: st.as_ptr(),
        security_trailer_len: 1,
        ocf: ocf.as_ptr(),
        ocf_len: 4,
    };
    let out = frame.as_mut_ptr();
    assert_eq!(
        encode(&raw const mpdu_fields, &raw const content, out, 40).0,
        ALDIS_OK
    );
    assert_eq!(
        frame[..40],
        sdls_frame()[..],
        "the one-shot and two-phase writers agree"
    );
    assert_eq!(
        encode(&raw const mpdu_fields, null(), out, 40).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        encode(null(), &raw const content, out, 40).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // A NULL buffer with a length, and an OCF of neither 0 nor 4 octets (CABI-022).
    for bad in [
        aldis_aos_content_t {
            zone: null(),
            ..content
        },
        aldis_aos_content_t {
            ocf_len: 2,
            ..content
        },
    ] {
        assert_eq!(
            encode(&raw const mpdu_fields, &raw const bad, out, 40).0,
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // The output overlapping any buffer of the content (CABI-022).
    let inside = frame.as_ptr().wrapping_add(20);
    for overlapping in [
        aldis_aos_content_t {
            insert_zone: inside,
            ..content
        },
        aldis_aos_content_t {
            security_header: inside,
            ..content
        },
        aldis_aos_content_t {
            zone: inside,
            ..content
        },
        aldis_aos_content_t {
            security_trailer: inside,
            ..content
        },
        aldis_aos_content_t {
            ocf: inside,
            ..content
        },
    ] {
        assert_eq!(
            encode(&raw const mpdu_fields, &raw const overlapping, out, 40).0,
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // The fields: flags 0 or 1, a pointer constant, a count of valid bits of 1 to 0x3FFE,
    // checked as the `ValidBitCount` they become (AOS-017).
    for bad in [
        aldis_aos_fields_t {
            replay_flag: 2,
            ..mpdu_fields
        },
        aldis_aos_fields_t {
            cycle_use_flag: 2,
            ..mpdu_fields
        },
        aldis_aos_fields_t {
            pointer: 7,
            ..mpdu_fields
        },
    ] {
        assert_eq!(
            encode(&raw const bad, &raw const content, out, 40).0,
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // A packet offset above 0xFFFD, checked as the `PacketOffset` it becomes (AOS-003).
    let wide = fields(0, ALDIS_AOS_POINTER_STARTS_AT, 0xFFFE);
    assert_eq!(
        encode(&raw const wide, &raw const content, out, 40),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_FIRST_HEADER_POINTER)
    );
    for bits in [0, ALDIS_AOS_VALID_BITS_MAX + 1] {
        let bad = fields(1, ALDIS_AOS_POINTER_VALID_BITS, bits);
        assert_eq!(
            encode(&raw const bad, &raw const content, out, 40),
            (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_BITSTREAM_DATA_POINTER)
        );
    }
    // A pointer not of the VC's data unit.
    let bpdu_on_mpdu = fields(0, ALDIS_AOS_POINTER_ALL_VALID, 0);
    assert_eq!(
        encode(&raw const bpdu_on_mpdu, &raw const content, out, 40),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_DATA_FIELD_CONTENT)
    );
    // The output too small: nothing written.
    frame.fill(0xEE);
    assert_eq!(
        encode(
            &raw const mpdu_fields,
            &raw const content,
            frame.as_mut_ptr(),
            39
        )
        .0,
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert!(frame.iter().all(|&b| b == 0xEE));
    let mut layout = aldis_aos_layout_t::default();
    let mut len = 0_usize;
    // SAFETY: every pointer is to a live local.
    unsafe {
        let mut det = detail();
        assert_eq!(
            aldis_aos_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const bpdu_on_mpdu,
                frame.as_mut_ptr(),
                40,
                &raw mut len,
                &raw mut det
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(det.field, ALDIS_FIELD_DATA_FIELD_CONTENT);
        assert!(
            frame.iter().all(|&b| b == 0xEE),
            "finish rejects without writing"
        );
        // VCID 63 is the OID writer's.
        let oid = fields(63, ALDIS_AOS_POINTER_NONE, 0);
        assert_eq!(
            aldis_aos_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const oid,
                frame.as_mut_ptr(),
                40,
                &raw mut layout,
                &raw mut det
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(det.field, ALDIS_FIELD_VC_ID);
        assert_eq!(
            aldis_aos_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const mpdu_fields,
                frame.as_mut_ptr(),
                40,
                null_mut(),
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // A B_PDU frame: the zone follows the 2-octet header.
        let b = fields(1, ALDIS_AOS_POINTER_VALID_BITS, 9);
        assert_eq!(
            aldis_aos_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const b,
                frame.as_mut_ptr(),
                40,
                &raw mut layout,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(
            (layout.data_offset, layout.zone_offset, layout.zone_len),
            (12, 14, 24)
        );
        assert_eq!(
            frame[12..14],
            [0x00, 0x08],
            "pointer 8, the index of the ninth bit"
        );
    }
}

#[test]
fn finish_checks_the_buffer_was_begun_with_these_arguments() {
    // req: AOS-019, API-023, CABI-022 (OI-50)
    let (mc, vcs) = (mc(), vcs());
    let call = |begin: bool, f: &aldis_aos_fields_t, frame: &mut [u8]| {
        let mut l = aldis_aos_layout_t::default();
        let mut n = 0_usize;
        // SAFETY: every pointer is to a live local or into `frame`, with its length.
        unsafe {
            if begin {
                aldis_aos_writer_begin(
                    &raw const mc,
                    vcs.as_ptr(),
                    3,
                    f,
                    frame.as_mut_ptr(),
                    frame.len(),
                    &raw mut l,
                    null_mut(),
                )
            } else {
                aldis_aos_writer_finish(
                    &raw const mc,
                    vcs.as_ptr(),
                    3,
                    f,
                    frame.as_mut_ptr(),
                    frame.len(),
                    &raw mut n,
                    null_mut(),
                )
            }
        }
    };
    let begun = fields(0, ALDIS_AOS_POINTER_STARTS_AT, 3);
    let mut frame = [0xEE_u8; 40];
    assert_eq!(call(true, &begun, &mut frame), ALDIS_OK);
    let written = frame;
    let other = [
        aldis_aos_fields_t {
            vc_frame_count: 0x12_3457,
            ..begun
        },
        aldis_aos_fields_t {
            replay_flag: 0,
            ..begun
        },
        aldis_aos_fields_t { cycle: 8, ..begun },
        aldis_aos_fields_t {
            cycle_use_flag: 0,
            ..begun
        },
        fields(0, ALDIS_AOS_POINTER_STARTS_AT, 4),
        fields(0, ALDIS_AOS_POINTER_NO_PACKET_START, 0),
        fields(1, ALDIS_AOS_POINTER_ALL_VALID, 0),
    ];
    for f in other {
        assert_eq!(
            call(false, &f, &mut frame),
            ALDIS_ERR_INVALID_STATE,
            "{f:?}"
        );
        assert_eq!(frame, written, "a failed finish leaves the frame unchanged");
    }
    assert_eq!(call(false, &begun, &mut frame), ALDIS_OK);
    // A buffer never begun, for a data VC and for the OID VC.
    let oid = fields(ALDIS_AOS_OID_VCID, ALDIS_AOS_POINTER_NONE, 0);
    for fill in [0x00_u8, 0xA5] {
        for f in [&begun, &oid] {
            let mut never = [fill; 40];
            assert_eq!(call(false, f, &mut never), ALDIS_ERR_INVALID_STATE);
            assert_eq!(never, [fill; 40]);
        }
    }
}

#[test]
fn the_oid_writer_takes_a_generator_handle() {
    // req: AOS-016, CMN-021, CMN-022, CABI-022, CABI-032, GEN-042
    let (mc, vcs) = (mc(), vcs());
    let f = fields(ALDIS_AOS_OID_VCID, ALDIS_AOS_POINTER_NONE, 0);
    let mut words = vec![0_u64; 64];
    let storage = words.as_mut_ptr();
    let mut h: *mut aldis_oid_generator_t = null_mut();
    let mut frame = [0_u8; 40];
    let mut l = aldis_aos_layout_t::default();
    let begin = |f: &aldis_aos_fields_t,
                 h: *mut aldis_oid_generator_t,
                 out: *mut u8,
                 l: *mut aldis_aos_layout_t| {
        // SAFETY: `mc`, `vcs`, `f`, and `l` are live; `out` points to 40 live octets; `h` is
        // a handle into live storage or invalid.
        unsafe {
            aldis_aos_oid_writer_begin(&raw const mc, vcs.as_ptr(), 3, f, h, out, 40, l, null_mut())
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
    // Rejected without advancing the generator: another VCID, a data unit header.
    let data_vc = fields(2, ALDIS_AOS_POINTER_NONE, 0);
    assert_eq!(
        begin(&data_vc, h, frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_FIELD
    );
    let with_header = aldis_aos_fields_t {
        pointer: ALDIS_AOS_POINTER_ONLY_IDLE_DATA,
        ..f
    };
    assert_eq!(
        begin(&with_header, h, frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_FIELD
    );
    // The output inside the generator's storage.
    let in_storage = storage.cast::<u8>().wrapping_add(16);
    assert_eq!(
        begin(&f, h, in_storage, &raw mut l),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(begin(&f, h, frame.as_mut_ptr(), &raw mut l), ALDIS_OK);
    assert_eq!(
        (l.data_offset, l.data_len, l.ocf_len, l.security_header_len),
        (12, 26, 0, 0)
    );
    // The first octets of the sequence of 132.0 Annex D (CMN-020), in the Galois form.
    assert_eq!(frame[12..18], [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6]);
    // SAFETY: as above.
    unsafe {
        let mut n = 0;
        assert_eq!(
            aldis_aos_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                3,
                &raw const f,
                frame.as_mut_ptr(),
                40,
                &raw mut n,
                null_mut()
            ),
            ALDIS_OK
        );
        assert_eq!(n, 40);
        assert_eq!(frame[1] & 0x3F, 63, "VCID 63");
        aldis_oid_generator_deinit(h);
    }
    assert_eq!(
        begin(&f, h, frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_HANDLE
    );
    assert_eq!(
        begin(&f, null_mut(), frame.as_mut_ptr(), &raw mut l),
        ALDIS_ERR_INVALID_FIELD
    );
}
