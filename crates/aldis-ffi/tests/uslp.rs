//! The contract of the USLP part of the C ABI, called as a C caller calls it: the virtual
//! channel and MAP arrays and their capacities (CABI-030, CABI-033, M2 plan D14), the full and
//! truncated views and their layouts (CABI-012, M2 plan D12), the one-shot encoders and the SDLS
//! fields they leave zero (API-023, API-072), the validation reasons (USLP-014), the
//! stateless delimiter (USLP-015), the OID generator handle (CMN-022), and the argument checks
//! (CABI-022) the vectors do not reach.
#![expect(clippy::unwrap_used, reason = "test-only harness (RS-018)")]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis::core::DecodeMode;
use aldis_ffi::capacities::{ALDIS_USLP_MAX_MAPS, ALDIS_USLP_MAX_VCS};
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_END_OF_FRAME_PRIMARY_HEADER_FLAG, ALDIS_FIELD_MAP_ID, ALDIS_FIELD_UPID,
    ALDIS_FIELD_VC_ID,
};
use aldis_ffi::oid::*;
use aldis_ffi::uslp::*;
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

/// A physical channel with its virtual channels and MAPs, as C structs.
struct Channels {
    pc: aldis_uslp_config_t,
    vcs: Vec<aldis_uslp_vc_config_t>,
    maps: Vec<aldis_uslp_map_config_t>,
}

fn vc(vcid: u8, frame_type: u8) -> aldis_uslp_vc_config_t {
    aldis_uslp_vc_config_t {
        struct_size: size::<aldis_uslp_vc_config_t>(),
        scid: 0x155,
        vcid,
        frame_type,
        ..aldis_uslp_vc_config_t::default()
    }
}

fn map(vcid: u8, map_id: u8, header: u8, trailer: u32) -> aldis_uslp_map_config_t {
    aldis_uslp_map_config_t {
        struct_size: size::<aldis_uslp_map_config_t>(),
        scid: 0x155,
        vcid,
        map_id,
        security_header_len: header,
        security_trailer_len: trailer,
    }
}

/// Variable-length frames of up to 512 octets with an FECF on SCID 0x155: VC 4 with an
/// expedited VCF Count of 1 octet and an OCF allowed; VC 5 with SDLS per MAP; MAPs 4/0, 4/2,
/// and 5/1 with SDLS 6/16. No VC has a truncated frame length, so each carries non-truncated
/// frames only.
fn variable() -> Channels {
    variable_with(0, 0)
}

/// The channels of [`variable`] with truncated frames of 12 octets on VC 4 and of 28 on VC 5,
/// which then carry only truncated frames (Annex D1.2.1; OI-48).
fn variable_truncated() -> Channels {
    variable_with(12, 28)
}

fn variable_with(truncated4: u8, truncated5: u8) -> Channels {
    let pc = aldis_uslp_config_t {
        struct_size: size::<aldis_uslp_config_t>(),
        frame_type: ALDIS_USLP_FRAME_VARIABLE,
        fecf: 1,
        frame_len: 512,
        insert_zone_len: 0,
    };
    let vc4 = aldis_uslp_vc_config_t {
        vcf_count_len_expedited: 1,
        ocf: ALDIS_USLP_OCF_ALLOWED,
        truncated_frame_len: truncated4,
        ..vc(4, ALDIS_USLP_FRAME_VARIABLE)
    };
    let vc5 = aldis_uslp_vc_config_t {
        truncated_frame_len: truncated5,
        security: ALDIS_USLP_SECURITY_PER_MAP,
        ..vc(5, ALDIS_USLP_FRAME_VARIABLE)
    };
    Channels {
        pc,
        vcs: vec![vc4, vc5],
        maps: vec![map(4, 0, 0, 0), map(4, 2, 0, 0), map(5, 1, 6, 16)],
    }
}

/// Fixed-length frames of 64 octets with a 2-octet Insert Zone and an FECF on SCID 0x155: VC 1
/// with an OCF and VCF Counts of 1 and 2 octets; VC 2 with SDLS 4/2; VC 63 for OID frames;
/// MAPs 1/0, 1/3, and 2/1.
fn fixed() -> Channels {
    let pc = aldis_uslp_config_t {
        struct_size: size::<aldis_uslp_config_t>(),
        frame_type: ALDIS_USLP_FRAME_FIXED,
        fecf: 1,
        frame_len: 64,
        insert_zone_len: 2,
    };
    let vc1 = aldis_uslp_vc_config_t {
        vcf_count_len_sequence: 1,
        vcf_count_len_expedited: 2,
        ocf: ALDIS_USLP_OCF_REQUIRED,
        ..vc(1, ALDIS_USLP_FRAME_FIXED)
    };
    let vc2 = aldis_uslp_vc_config_t {
        security_header_len: 4,
        security_trailer_len: 2,
        ..vc(2, ALDIS_USLP_FRAME_FIXED)
    };
    Channels {
        pc,
        vcs: vec![vc1, vc2, vc(63, ALDIS_USLP_FRAME_FIXED)],
        maps: vec![map(1, 0, 0, 0), map(1, 3, 0, 0), map(2, 1, 0, 0)],
    }
}

const fn fields(vcid: u8, map_id: u8, kind: u8, vcf_count: u64) -> aldis_uslp_fields_t {
    aldis_uslp_fields_t {
        channel: aldis_uslp_channel_t {
            scid: 0x155,
            source_or_destination: ALDIS_USLP_SOURCE,
            vcid,
            map_id,
        },
        kind,
        vcf_count,
    }
}

const fn octet_stream() -> aldis_uslp_tfdf_t {
    aldis_uslp_tfdf_t {
        rule: ALDIS_USLP_RULE_OCTET_STREAM,
        upid: ALDIS_USLP_UPID_OCTET_STREAM,
        pointer: 0,
    }
}

impl Channels {
    fn check(&self) -> i32 {
        // SAFETY: every pointer is to a live struct or vector.
        unsafe {
            aldis_uslp_vc_table_check(
                &raw const self.pc,
                self.vcs.as_ptr(),
                self.vcs.len(),
                self.maps.as_ptr(),
                self.maps.len(),
                null_mut(),
            )
        }
    }

    fn encode(
        &self,
        fl: &aldis_uslp_fields_t,
        ct: &aldis_uslp_content_t,
        out: &mut [u8],
        det: &mut aldis_error_detail_t,
    ) -> (i32, usize) {
        let mut len = 0;
        // SAFETY: every pointer is to a live local, struct, or vector, with its length.
        let rc = unsafe {
            aldis_uslp_encode(
                &raw const self.pc,
                self.vcs.as_ptr(),
                self.vcs.len(),
                self.maps.as_ptr(),
                self.maps.len(),
                fl,
                ct,
                out.as_mut_ptr(),
                out.len(),
                &raw mut len,
                det,
            )
        };
        (rc, len)
    }

    fn decode(&self, bytes: &[u8], view: &mut aldis_uslp_frame_t) -> i32 {
        // SAFETY: as for `encode`.
        unsafe {
            aldis_uslp_decode(
                bytes.as_ptr(),
                bytes.len(),
                &raw const self.pc,
                self.vcs.as_ptr(),
                self.vcs.len(),
                self.maps.as_ptr(),
                self.maps.len(),
                ALDIS_MODE_STRICT,
                view,
                null_mut(),
            )
        }
    }
}

/// A virtual channel struct of a later header, with a member this library does not know.
#[repr(C)]
#[derive(Clone, Copy)]
struct LaterVc {
    vc: aldis_uslp_vc_config_t,
    later: u64,
}

/// A MAP struct of a later header.
#[repr(C)]
#[derive(Clone, Copy)]
struct LaterMap {
    map: aldis_uslp_map_config_t,
    later: u64,
}

#[test]
fn vc_and_map_arrays_step_by_their_struct_size() {
    // req: CABI-033, CABI-022, CABI-030, API-040, USLP-017
    let ch = variable();
    let check = |vcs: *const aldis_uslp_vc_config_t,
                 len: usize,
                 maps: *const aldis_uslp_map_config_t,
                 mp: usize| {
        // SAFETY: `ch.pc` is live; `vcs` and `maps` are NULL or point to `n` and `m` live
        // structs.
        unsafe { aldis_uslp_vc_table_check(&raw const ch.pc, vcs, len, maps, mp, null_mut()) }
    };
    assert_eq!(ch.check(), ALDIS_OK);
    // A table with no virtual channel receives nothing (OI-55).
    assert_eq!(check(null(), 0, null(), 0), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(check(null(), 1, null(), 0), ALDIS_ERR_INVALID_FIELD);
    assert_eq!(check(null(), 0, null(), 1), ALDIS_ERR_INVALID_FIELD);
    // Longer structs of a later header: each array's stride is its struct_size.
    let vcs: [LaterVc; 2] = [ch.vcs[0], ch.vcs[1]].map(|vc| LaterVc {
        vc: aldis_uslp_vc_config_t {
            struct_size: size::<LaterVc>(),
            ..vc
        },
        later: u64::MAX,
    });
    let maps: [LaterMap; 3] = [ch.maps[0], ch.maps[1], ch.maps[2]].map(|map| LaterMap {
        map: aldis_uslp_map_config_t {
            struct_size: size::<LaterMap>(),
            ..map
        },
        later: u64::MAX,
    });
    let (vp, mp) = (vcs.as_ptr().cast(), maps.as_ptr().cast());
    assert_eq!(check(vp, 2, mp, 3), ALDIS_OK);
    // Every struct repeats its array's stride.
    let mut mixed = maps;
    mixed[2].map.struct_size = size::<aldis_uslp_map_config_t>();
    assert_eq!(
        check(vp, 2, mixed.as_ptr().cast(), 3),
        ALDIS_ERR_INVALID_FIELD
    );
    let mut mixed = vcs;
    mixed[1].vc.struct_size = size::<aldis_uslp_vc_config_t>();
    assert_eq!(
        check(mixed.as_ptr().cast(), 2, mp, 3),
        ALDIS_ERR_INVALID_FIELD
    );
    // Shorter than the struct this library knows, or not a multiple of its alignment.
    for struct_size in [8, size::<aldis_uslp_vc_config_t>() + 2] {
        let mut short = ch.vcs.clone();
        for e in &mut short {
            e.struct_size = struct_size;
        }
        assert_eq!(
            check(short.as_ptr(), 2, ch.maps.as_ptr(), 3),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // A misaligned array, and a count whose array would wrap the address space.
    let words = [0_u64; 8];
    let misaligned = words.as_ptr().cast::<u8>().wrapping_add(1).cast();
    assert_eq!(check(misaligned, 1, null(), 0), ALDIS_ERR_INVALID_FIELD);
    assert_eq!(
        check(ch.vcs.as_ptr(), usize::MAX / 4, null(), 0),
        ALDIS_ERR_INVALID_FIELD
    );
    // The physical channel's struct_size (CABI-033).
    let small = aldis_uslp_config_t {
        struct_size: 4,
        ..ch.pc
    };
    // SAFETY: every pointer is to a live struct or vector.
    let rc = unsafe { aldis_uslp_config_check(&raw const small, null_mut()) };
    assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);
}

#[test]
fn the_tables_have_the_build_capacities() {
    // req: CABI-030, GEN-011
    let mut ch = variable();
    ch.vcs = (0..=u8::try_from(ALDIS_USLP_MAX_VCS).unwrap())
        .map(|len| vc(len, ALDIS_USLP_FRAME_VARIABLE))
        .collect();
    ch.maps.clear();
    let mut det = detail();
    // SAFETY: every pointer is to a live struct or vector.
    let rc = unsafe {
        aldis_uslp_vc_table_check(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            null(),
            0,
            &raw mut det,
        )
    };
    assert_eq!(
        (rc, det.field),
        (ALDIS_ERR_CAPACITY_EXCEEDED, ALDIS_FIELD_VC_ID)
    );
    ch.vcs.pop();
    assert_eq!(ch.check(), ALDIS_OK, "exactly the capacity");
    ch.vcs.truncate(1);
    ch.maps = (0..=ALDIS_USLP_MAX_MAPS)
        .map(|len| map(0, u8::try_from(len % 16).unwrap(), 0, 0))
        .collect();
    let mut det = detail();
    // SAFETY: as above.
    let rc = unsafe {
        aldis_uslp_vc_table_check(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            1,
            ch.maps.as_ptr(),
            ch.maps.len(),
            &raw mut det,
        )
    };
    assert_eq!(
        (rc, det.field),
        (ALDIS_ERR_CAPACITY_EXCEEDED, ALDIS_FIELD_MAP_ID)
    );
}

#[test]
fn the_encoder_leaves_the_sdls_fields_zero() {
    // req: API-072, USLP-010, USLP-012, CABI-012, CABI-022
    let ch = variable();
    let fl = fields(5, 1, ALDIS_USLP_KIND_SEQUENCE_CONTROLLED, 0);
    let three = [7_u8, 8, 9];
    let ct = aldis_uslp_content_t {
        tfdf: octet_stream(),
        tfdz: three.as_ptr(),
        tfdz_len: 3,
        ..aldis_uslp_content_t::default()
    };
    let mut det = detail();
    let mut buf = [0xEE_u8; 64];
    let (rc, len) = ch.encode(&fl, &ct, &mut buf, &mut det);
    assert_eq!((rc, len), (ALDIS_OK, 35));
    let mut view = aldis_uslp_frame_t::default();
    assert_eq!(ch.decode(&buf[..len], &mut view), ALDIS_OK);
    let lv = view.layout;
    assert_eq!(
        (lv.security_header_offset, lv.security_header_len),
        (7, 6),
        "SDLS 6/16 on VC 5"
    );
    assert_eq!(buf[7..13], [0; 6], "the Security Header is zero");
    assert_eq!(buf[lv.tfdz_offset..][..lv.tfdz_len], [7, 8, 9]);
    assert_eq!(
        (lv.security_trailer_offset, lv.security_trailer_len),
        (17, 16)
    );
    assert_eq!(buf[17..33], [0; 16], "and the Security Trailer");
    assert_eq!((lv.fecf_offset, lv.fecf_len), (33, 2));
    // The same octets as the Rust two-phase writer (typestate) without filling the security fields.
    let tbl = rust_variable();
    let rf = aldis::uslp::FrameFields {
        spacecraft_id: aldis::uslp::UslpSpacecraftId::new(0x155).unwrap(),
        source_or_destination: aldis::uslp::SourceOrDestination::Source,
        vc_id: aldis::uslp::UslpVcId::new(5).unwrap(),
        map_id: aldis::uslp::UslpMapId::new(1).unwrap(),
        kind: aldis::uslp::FrameKind::SequenceControlled,
        vcf_count: 0,
    };
    let rc = aldis::uslp::FrameContent {
        tfdf: aldis::uslp::TfdfHeader {
            rule: aldis::uslp::TfdzRule::OctetStream,
            upid: aldis::uslp::Upid::OctetStream,
        },
        insert_zone: &[],
        ocf: None,
    };
    let mut rust = [0_u8; 64];
    let mut w = aldis::uslp::UslpFrameWriter::begin(&mut rust, &tbl, rf, rc, 3).unwrap();
    w.tfdz_mut().unwrap()[..3].copy_from_slice(&[7, 8, 9]);
    assert_eq!(w.finish(3), Ok(35));
    assert_eq!(rust[..35], buf[..35]);
    // Too small an output: nothing written.
    let mut small = [0xEE_u8; 34];
    let (rc, _) = ch.encode(&fl, &ct, &mut small, &mut det);
    assert_eq!(rc, ALDIS_ERR_BUFFER_TOO_SMALL);
    assert_eq!(small, [0xEE; 34]);
}

/// The Rust table of [`variable`].
fn rust_variable() -> aldis::uslp::VcTable<4, 4> {
    use aldis::core::security::SecurityLayout;
    use aldis::uslp::{
        MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope, TransferFrameType, UslpMapId,
        UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen,
    };
    let scid = UslpSpacecraftId::new(0x155).unwrap();
    let vcid = |len| UslpVcId::new(len).unwrap();
    let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 512, 0, true).unwrap();
    let vc4 = VcParams {
        vcf_count_len_expedited: VcfCountLen::new(1).unwrap(),
        ocf: OcfUse::Allowed,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc5 = VcParams {
        security: SecurityScope::PerMap,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let map = |vcid_n, map_n, header, trailer| {
        MapConfig::new(
            scid,
            vcid(vcid_n),
            UslpMapId::new(map_n).unwrap(),
            SecurityLayout::new(header, trailer).unwrap(),
        )
    };
    VcTable::new(
        pc,
        &[
            VcConfig::new(scid, vcid(4), vc4).unwrap(),
            VcConfig::new(scid, vcid(5), vc5).unwrap(),
        ],
        &[map(4, 0, 0, 0), map(4, 2, 0, 0), map(5, 1, 6, 16)],
    )
    .unwrap()
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per field of both views")]
fn full_and_truncated_frames_cross_as_one_tagged_view() {
    // req: USLP-002, USLP-008, CABI-012, M2 plan D12, OI-52
    let ch = fixed();
    let (iz, ocf, tfdz) = ([0xAA_u8, 0xBB], [1_u8, 2, 3, 4], [0xD0_u8; 44]);
    let ct = aldis_uslp_content_t {
        tfdf: aldis_uslp_tfdf_t {
            rule: ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES,
            upid: ALDIS_USLP_UPID_PACKETS,
            pointer: 2,
        },
        insert_zone: iz.as_ptr(),
        insert_zone_len: 2,
        tfdz: tfdz.as_ptr(),
        tfdz_len: tfdz.len(),
        ocf: ocf.as_ptr(),
        ocf_len: 4,
    };
    let fl = fields(1, 3, ALDIS_USLP_KIND_EXPEDITED, 0x1234);
    let mut frame = [0_u8; 64];
    let mut det = detail();
    assert_eq!(ch.encode(&fl, &ct, &mut frame, &mut det), (ALDIS_OK, 64));
    let mut view = aldis_uslp_frame_t::default();
    assert_eq!(ch.decode(&frame, &mut view), ALDIS_OK);
    assert!(!view.truncated && view.ocf_flag && view.has_fecf && view.upid_assigned);
    assert_eq!(
        (view.kind, view.vcf_count_len, view.vcf_count),
        (ALDIS_USLP_KIND_EXPEDITED, 2, 0x1234)
    );
    assert_eq!(view.channel, fl.channel);
    assert_eq!(view.tfdf, ct.tfdf);
    let lay = view.layout;
    assert_eq!(
        [
            (lay.insert_zone_offset, lay.insert_zone_len),
            (lay.security_header_offset, lay.security_header_len),
            (lay.tfdf_header_offset, lay.tfdf_header_len),
            (lay.tfdz_offset, lay.tfdz_len),
            (lay.security_trailer_offset, lay.security_trailer_len),
            (lay.ocf_offset, lay.ocf_len),
            (lay.fecf_offset, lay.fecf_len),
        ],
        [
            (9, 2),
            (11, 0),
            (11, 3),
            (14, 44),
            (58, 0),
            (58, 4),
            (62, 2)
        ]
    );
    assert_eq!(frame[lay.ocf_offset..][..4], ocf);
    assert_eq!(view.fecf.to_be_bytes(), frame[62..64]);
    // A truncated frame of VC 4 MAP 2: the full frame's members are 0, except the kind, which is
    // expedited (Annex D1.3.1 note 3; OI-52).
    let ch = variable_truncated();
    let seven = [0x31_u8, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37];
    let channel = aldis_uslp_channel_t {
        scid: 0x155,
        source_or_destination: ALDIS_USLP_DESTINATION,
        vcid: 4,
        map_id: 2,
    };
    let tc = aldis_uslp_content_t {
        tfdf: aldis_uslp_tfdf_t {
            rule: ALDIS_USLP_RULE_NO_SEGMENTATION,
            upid: 0b11110,
            pointer: 0,
        },
        tfdz: seven.as_ptr(),
        tfdz_len: 7,
        ..aldis_uslp_content_t::default()
    };
    let mut tr = [0_u8; 12];
    let mut len = 0;
    // SAFETY: every pointer is to a live local or struct, with its length.
    let rc = unsafe {
        aldis_uslp_encode_truncated(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            ch.maps.as_ptr(),
            3,
            &raw const channel,
            &raw const tc,
            tr.as_mut_ptr(),
            12,
            &raw mut len,
            null_mut(),
        )
    };
    assert_eq!((rc, len), (ALDIS_OK, 12));
    let mut view = aldis_uslp_frame_t {
        kind: 9,
        vcf_count: 9,
        ..aldis_uslp_frame_t::default()
    };
    assert_eq!(ch.decode(&tr, &mut view), ALDIS_OK);
    assert!(view.truncated && !view.upid_assigned && !view.ocf_flag && !view.has_fecf);
    assert_eq!(
        (view.kind, view.vcf_count_len, view.vcf_count),
        (ALDIS_USLP_KIND_EXPEDITED, 0, 0)
    );
    assert_eq!(view.channel, channel);
    let lay = view.layout;
    assert_eq!(
        (
            lay.frame_len,
            lay.insert_zone_offset,
            lay.tfdf_header_offset,
            lay.tfdz_offset
        ),
        (12, 4, 4, 5)
    );
    assert_eq!(
        (lay.ocf_offset, lay.ocf_len, lay.fecf_offset, lay.fecf_len),
        (12, 0, 12, 0)
    );
    let mut len = 0;
    // A truncated header is 4 octets; its length comes from the VC (API-013, M2 plan D14).
    // SAFETY: as above.
    let rc = unsafe {
        aldis_uslp_peek_length(
            tr.as_ptr(),
            4,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            &raw mut len,
            null_mut(),
        )
    };
    assert_eq!((rc, len), (ALDIS_OK, 12));
    // A truncated frame has no Insert Zone or OCF: either in the content struct is an
    // argument error.
    for bad in [
        aldis_uslp_content_t {
            insert_zone: seven.as_ptr(),
            insert_zone_len: 1,
            ..tc
        },
        aldis_uslp_content_t {
            ocf: seven.as_ptr(),
            ocf_len: 4,
            ..tc
        },
    ] {
        // SAFETY: as above.
        let rc = unsafe {
            aldis_uslp_encode_truncated(
                &raw const ch.pc,
                ch.vcs.as_ptr(),
                2,
                ch.maps.as_ptr(),
                3,
                &raw const channel,
                &raw const bad,
                tr.as_mut_ptr(),
                12,
                &raw mut len,
                null_mut(),
            )
        };
        assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);
    }
}

#[test]
fn stream_reports_the_discard_reason() {
    // req: USLP-014, CABI-020, CABI-021
    let ch = variable();
    let payload = [1_u8, 2, 3];
    let ct = aldis_uslp_content_t {
        tfdf: octet_stream(),
        tfdz: payload.as_ptr(),
        tfdz_len: 3,
        ..aldis_uslp_content_t::default()
    };
    let mut frame = [0_u8; 64];
    let mut det = detail();
    let (rc, len) = ch.encode(
        &fields(4, 0, ALDIS_USLP_KIND_EXPEDITED, 7),
        &ct,
        &mut frame,
        &mut det,
    );
    assert_eq!(rc, ALDIS_OK);
    let step = |bytes: &[u8], det| {
        let mut item = aldis_uslp_stream_item_t::default();
        let mut offset = 0;
        // SAFETY: every pointer is to a live local, struct, or vector.
        let status = unsafe {
            aldis_uslp_stream_next(
                bytes.as_ptr(),
                bytes.len(),
                &raw mut offset,
                &raw const ch.pc,
                ch.vcs.as_ptr(),
                2,
                ch.maps.as_ptr(),
                3,
                ALDIS_MODE_STRICT,
                &raw mut item,
                det,
            )
        };
        (status, offset, item)
    };
    let (rc, offset, item) = step(&frame[..len], &raw mut det);
    assert_eq!(rc, ALDIS_OK);
    assert_eq!(item.kind, ALDIS_USLP_STREAM_ACCEPTED);
    assert_eq!(item.discard_reason, ALDIS_USLP_DISCARD_NONE);
    assert_eq!(item.len, len);
    assert_eq!(offset, len);

    // Test (e): a wrong FECF discards the frame.
    frame[len - 1] ^= 1;
    let (rc, offset, item) = step(&frame[..len], &raw mut det);
    assert_eq!(rc, ALDIS_OK);
    assert_eq!(item.kind, ALDIS_USLP_STREAM_DISCARDED);
    assert_eq!(item.discard_reason, ALDIS_USLP_DISCARD_FECF_MISMATCH);
    assert_eq!(item.error_code, ALDIS_ERR_CRC_MISMATCH);
    assert_eq!(offset, len);
}

/// A VC with a truncated frame length carries only truncated frames: every C entry point that
/// meets a non-truncated frame on it reports the flag, and the delimiter discards the rest
/// (Annex D1.2.1; USLP-002, USLP-014, USLP-015; OI-48).
#[test]
fn a_vc_with_a_truncated_length_refuses_non_truncated_frames() {
    // req: USLP-002, USLP-014, USLP-015, CABI-020, CABI-021
    let plain = variable();
    let ch = variable_truncated();
    let payload = [1_u8, 2, 3];
    let ct = aldis_uslp_content_t {
        tfdf: octet_stream(),
        tfdz: payload.as_ptr(),
        tfdz_len: 3,
        ..aldis_uslp_content_t::default()
    };
    let fl = fields(4, 0, ALDIS_USLP_KIND_EXPEDITED, 7);
    let mut frame = [0_u8; 64];
    let mut det = detail();
    let (rc, len) = plain.encode(&fl, &ct, &mut frame, &mut det);
    assert_eq!(rc, ALDIS_OK);
    let flag = (
        ALDIS_ERR_INVALID_FIELD,
        ALDIS_FIELD_END_OF_FRAME_PRIMARY_HEADER_FLAG,
    );
    let got = |rc: i32, det: &aldis_error_detail_t| (rc, det.field);
    // The encoder.
    let mut out = [0_u8; 64];
    let mut det = detail();
    let (rc, _) = ch.encode(&fl, &ct, &mut out, &mut det);
    assert_eq!((got(rc, &det), out), (flag, [0; 64]));
    // The decoder.
    let mut view = aldis_uslp_frame_t::default();
    let mut det = detail();
    // SAFETY: every pointer is to a live local, struct, or vector, with its length.
    let rc = unsafe {
        aldis_uslp_decode(
            frame.as_ptr(),
            len,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            ch.maps.as_ptr(),
            3,
            ALDIS_MODE_STRICT,
            &raw mut view,
            &raw mut det,
        )
    };
    assert_eq!(got(rc, &det), flag);
    // The stream intake: discarded, reason NON_TRUNCATED_FRAME_NOT_ALLOWED.
    let mut item = aldis_uslp_stream_item_t::default();
    let mut det = detail();
    let mut offset = 0;
    // SAFETY: as above.
    let rc = unsafe {
        aldis_uslp_stream_next(
            frame.as_ptr(),
            len,
            &raw mut offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            ch.maps.as_ptr(),
            3,
            ALDIS_MODE_STRICT,
            &raw mut item,
            &raw mut det,
        )
    };
    assert_eq!(rc, ALDIS_OK);
    assert_eq!(
        (
            item.kind,
            item.discard_reason,
            item.offset,
            item.len,
            offset
        ),
        (
            ALDIS_USLP_STREAM_DISCARDED,
            ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED,
            0,
            len,
            len,
        )
    );
    // The length of the frame that starts the prefix.
    let mut n = 0;
    let mut det = detail();
    // SAFETY: as above.
    let rc = unsafe {
        aldis_uslp_peek_length(
            frame.as_ptr(),
            len,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            &raw mut n,
            &raw mut det,
        )
    };
    assert_eq!(got(rc, &det), flag);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "both generator forms, against the Rust encoder"
)]
fn encode_oid_takes_a_generator_handle() {
    // req: USLP-009, CMN-021, CMN-022, CABI-022, CABI-032, GEN-042
    let ch = fixed();
    let iz = [0xAA_u8, 0xBB];
    let ct = aldis_uslp_content_t {
        insert_zone: iz.as_ptr(),
        insert_zone_len: 2,
        ..aldis_uslp_content_t::default()
    };
    let mut storage = [
        aldis_oid_generator_t {
            words: [0; ALDIS_OID_GENERATOR_WORDS],
        },
        aldis_oid_generator_t {
            words: [0; ALDIS_OID_GENERATOR_WORDS],
        },
    ];
    let handles = [&raw mut storage[0], &raw mut storage[1]];
    for (h, form) in handles
        .into_iter()
        .zip([ALDIS_OID_FIBONACCI, ALDIS_OID_GALOIS])
    {
        // SAFETY: `h` points to live storage.
        let rc = unsafe { aldis_oid_generator_init(h, form, null_mut()) };
        assert_eq!(rc, ALDIS_OK);
    }
    let oid = |fl: &aldis_uslp_fields_t,
               ct: &aldis_uslp_content_t,
               hd: *mut aldis_oid_generator_t,
               out: *mut u8| {
        let mut len = 0;
        // SAFETY: every pointer is to a live local, struct, or vector; `out` points to 64 live
        // octets; `h` is a handle into live storage or invalid.
        let rc = unsafe {
            aldis_uslp_encode_oid(
                &raw const ch.pc,
                ch.vcs.as_ptr(),
                3,
                ch.maps.as_ptr(),
                3,
                fl,
                ct,
                hd,
                out,
                64,
                &raw mut len,
                null_mut(),
            )
        };
        (rc, len)
    };
    let fl = fields(ALDIS_USLP_OID_VCID, 0, ALDIS_USLP_KIND_EXPEDITED, 0);
    // Rejected, and no generator advances: a MAP ID other than 0, a TFDZ in the content
    // struct, and an output inside the generator's storage.
    let mut frames = [[0_u8; 64]; 2];
    for (i, &hd) in handles.iter().enumerate() {
        assert_eq!(
            oid(
                &fields(63, 1, ALDIS_USLP_KIND_EXPEDITED, 0),
                &ct,
                hd,
                frames[i].as_mut_ptr()
            )
            .0,
            ALDIS_ERR_INVALID_FIELD
        );
        let with_tfdz = aldis_uslp_content_t {
            tfdz: iz.as_ptr(),
            tfdz_len: 2,
            ..ct
        };
        assert_eq!(
            oid(&fl, &with_tfdz, hd, frames[i].as_mut_ptr()).0,
            ALDIS_ERR_INVALID_FIELD
        );
        let in_storage = handles[i].cast::<u8>();
        assert_eq!(oid(&fl, &ct, hd, in_storage).0, ALDIS_ERR_INVALID_FIELD);
    }
    // Both forms give the same frames, from the start of the sequence (CMN-020, CMN-022).
    for _ in 0..2 {
        for (i, &hd) in handles.iter().enumerate() {
            assert_eq!(oid(&fl, &ct, hd, frames[i].as_mut_ptr()), (ALDIS_OK, 64));
        }
        assert_eq!(frames[0], frames[1]);
    }
    let rust = {
        let tbl = aldis::uslp::VcTable::<4, 4>::new(
            aldis::uslp::PhysicalChannelConfig::new(
                aldis::uslp::TransferFrameType::Fixed,
                64,
                2,
                true,
            )
            .unwrap(),
            &[aldis::uslp::VcConfig::new(
                aldis::uslp::UslpSpacecraftId::new(0x155).unwrap(),
                aldis::uslp::UslpVcId::new(63).unwrap(),
                aldis::uslp::VcParams::new(aldis::uslp::TransferFrameType::Fixed),
            )
            .unwrap()],
            &[],
        )
        .unwrap();
        let rf = aldis::uslp::FrameFields {
            spacecraft_id: aldis::uslp::UslpSpacecraftId::new(0x155).unwrap(),
            source_or_destination: aldis::uslp::SourceOrDestination::Source,
            vc_id: aldis::uslp::UslpVcId::new(63).unwrap(),
            map_id: aldis::uslp::UslpMapId::new(0).unwrap(),
            kind: aldis::uslp::FrameKind::Expedited,
            vcf_count: 0,
        };
        let mut g = aldis::core::oid::OidGenerator::new();
        let mut out = [0_u8; 64];
        for _ in 0..2 {
            aldis::uslp::encode_oid(&tbl, rf, &iz, None, &mut g, &mut out).unwrap();
        }
        out
    };
    assert_eq!(frames[0], rust, "the second frame continues the sequence");
    let mut view = aldis_uslp_frame_t::default();
    assert_eq!(ch.decode(&frames[0], &mut view), ALDIS_OK);
    assert_eq!(
        (
            view.tfdf.upid,
            view.tfdf.rule,
            usize::from(view.tfdf.pointer)
        ),
        (
            ALDIS_USLP_UPID_ONLY_IDLE_DATA,
            ALDIS_USLP_RULE_SDU_START,
            view.layout.tfdz_len - 1
        )
    );
    // SAFETY: the handles are those of the live storage.
    unsafe {
        aldis_oid_generator_deinit(handles[0]);
        aldis_oid_generator_deinit(handles[1]);
    }
    assert_eq!(
        oid(&fl, &ct, handles[0], frames[0].as_mut_ptr()).0,
        ALDIS_ERR_INVALID_HANDLE
    );
    assert_eq!(
        oid(&fl, &ct, null_mut(), frames[0].as_mut_ptr()).0,
        ALDIS_ERR_INVALID_FIELD
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per argument")]
fn content_and_fields_are_checked_as_arguments() {
    // req: CABI-022, CABI-011, USLP-007, API-022
    let ch = variable();
    let payload = [1_u8, 2, 3, 4, 5, 6];
    let ct = aldis_uslp_content_t {
        tfdf: octet_stream(),
        tfdz: payload.as_ptr(),
        tfdz_len: 3,
        ..aldis_uslp_content_t::default()
    };
    let fl = fields(4, 0, ALDIS_USLP_KIND_EXPEDITED, 0);
    let mut out = [0xEE_u8; 64];
    let mut det = detail();
    assert_eq!(ch.encode(&fl, &ct, &mut out, &mut det).0, ALDIS_OK);
    // Any 5-bit UPID, assigned or not (USLP-007); not above.
    for upid in [0b01001, 0b11110] {
        let u = aldis_uslp_content_t {
            tfdf: aldis_uslp_tfdf_t { upid, ..ct.tfdf },
            ..ct
        };
        assert_eq!(ch.encode(&fl, &u, &mut out, &mut det).0, ALDIS_OK);
    }
    let mut zone = [0xEE_u8; 64];
    let checks = [
        (
            aldis_uslp_content_t {
                tfdf: aldis_uslp_tfdf_t {
                    upid: ALDIS_USLP_UPID_MAX + 1,
                    ..ct.tfdf
                },
                ..ct
            },
            fl,
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_UPID,
        ),
        (
            aldis_uslp_content_t {
                tfdf: aldis_uslp_tfdf_t { rule: 8, ..ct.tfdf },
                ..ct
            },
            fl,
            ALDIS_ERR_INVALID_FIELD,
            0,
        ),
        (
            aldis_uslp_content_t {
                ocf: payload.as_ptr(),
                ocf_len: 3,
                ..ct
            },
            fl,
            ALDIS_ERR_INVALID_FIELD,
            0,
        ),
        (
            ct,
            fields(4, 16, ALDIS_USLP_KIND_EXPEDITED, 0),
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_MAP_ID,
        ),
        (
            ct,
            fields(64, 0, ALDIS_USLP_KIND_EXPEDITED, 0),
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_VC_ID,
        ),
        (ct, fields(4, 0, 3, 0), ALDIS_ERR_INVALID_FIELD, 0),
    ];
    for (content, fl, status, field) in checks {
        let mut det = detail();
        assert_eq!(ch.encode(&fl, &content, &mut zone, &mut det).0, status);
        assert_eq!(det.field, field);
        assert_eq!(zone, [0xEE; 64], "out unchanged (API-022)");
    }
    // An output overlapping the TFDZ.
    let mut buf = [0_u8; 64];
    let overlapping = aldis_uslp_content_t {
        tfdz: buf.as_ptr().wrapping_add(10),
        tfdz_len: 3,
        ..ct
    };
    assert_eq!(
        ch.encode(&fl, &overlapping, &mut buf, &mut det).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // NULL fields and content.
    let mut len = 0;
    // SAFETY: every pointer is to a live local or vector, or NULL.
    let rc = unsafe {
        aldis_uslp_encode(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            ch.maps.as_ptr(),
            3,
            null(),
            &raw const ct,
            out.as_mut_ptr(),
            64,
            &raw mut len,
            null_mut(),
        )
    };
    assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);
    // SAFETY: as above.
    let rc = unsafe {
        aldis_uslp_encode(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            2,
            ch.maps.as_ptr(),
            3,
            &raw const fl,
            null(),
            out.as_mut_ptr(),
            64,
            &raw mut len,
            null_mut(),
        )
    };
    assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);
    // The decoder agrees with the Rust one on the encoded frame.
    let (rc, len) = ch.encode(&fl, &ct, &mut out, &mut det);
    assert_eq!(rc, ALDIS_OK);
    let rust =
        aldis::uslp::UslpFrame::decode(&out[..len], &rust_variable(), DecodeMode::Strict).unwrap();
    let mut view = aldis_uslp_frame_t::default();
    assert_eq!(ch.decode(&out[..len], &mut view), ALDIS_OK);
    assert_eq!(view.layout.tfdz_len, rust.tfdz().len());
    assert_eq!(
        view.vcf_count_len, 1,
        "the expedited VCF Count Length of VC 4"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "straight-line intake stream scenario"
)]
fn the_uslp_stream_intake_pipeline() {
    // req: USLP-014, USLP-015, CABI-012, CABI-022
    let ch = variable();
    let data = [0x10, 0x20, 0x30];
    let ct = aldis_uslp_content_t {
        tfdf: octet_stream(),
        tfdz: data.as_ptr(),
        tfdz_len: data.len(),
        ..aldis_uslp_content_t::default()
    };
    let fl = fields(4, 0, ALDIS_USLP_KIND_EXPEDITED, 7);
    let mut frame = [0_u8; 64];
    let mut det = detail();
    let (rc, len) = ch.encode(&fl, &ct, &mut frame, &mut det);
    assert_eq!(rc, ALDIS_OK);

    let mut unit = frame[..len].to_vec();
    unit.extend_from_slice(&frame[..len]);
    unit.extend_from_slice(&[0x55, 0x55, 0x55]);

    let mut offset = 0;
    let mut item = aldis_uslp_stream_item_t::default();
    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let rc = unsafe {
        aldis_uslp_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (rc, item.kind, item.offset, item.len, offset),
        (ALDIS_OK, ALDIS_USLP_STREAM_ACCEPTED, 0, len, len)
    );
    assert_eq!(item.frame.channel.scid, 0x155);

    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let rc = unsafe {
        aldis_uslp_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (rc, item.kind, item.offset, item.len, offset),
        (ALDIS_OK, ALDIS_USLP_STREAM_ACCEPTED, len, len, len * 2)
    );

    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let rc = unsafe {
        aldis_uslp_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (
            rc,
            item.kind,
            item.offset,
            item.len,
            item.discard_reason,
            offset
        ),
        (
            ALDIS_OK,
            ALDIS_USLP_STREAM_DISCARDED,
            len * 2,
            3,
            ALDIS_USLP_DISCARD_FILL,
            unit.len()
        )
    );

    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let rc = unsafe {
        aldis_uslp_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (rc, item.kind, item.len, offset),
        (ALDIS_OK, ALDIS_USLP_STREAM_END, 0, unit.len())
    );
}

#[test]
fn stream_rejects_overlapping_pointers() {
    // req: CABI-022
    let ch = variable();
    let unit = [0xC0_u8, 0x15, 0x58, 0x85, 0xE5, 1, 2, 3, 4, 5, 6, 7];
    let mut item = aldis_uslp_stream_item_t::default();

    // 1. offset pointing into unit buffer is rejected
    let mut buffer = [0_usize; 8];
    let unit_ptr = buffer.as_ptr().cast::<u8>();
    let offset_ptr = buffer.as_mut_ptr();
    // SAFETY: pointers point to valid aligned stack buffer
    let rc = unsafe {
        aldis_uslp_stream_next(
            unit_ptr,
            buffer.len().saturating_mul(core::mem::size_of::<usize>()),
            offset_ptr,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);

    // 2. out overlapping offset is rejected (e.g. &item.offset as offset and &item as out)
    // SAFETY: pointers point to valid stack item
    let rc = unsafe {
        aldis_uslp_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut item.offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);
}
