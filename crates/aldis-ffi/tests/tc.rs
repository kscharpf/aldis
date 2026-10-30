//! The contract of the TC part of the C ABI, called as a C caller calls it: the virtual channel
//! and MAP arrays (CABI-033, M2 plan D14), the security layouts of a VC or a MAP (API-070), the
//! one-shot encoder and delimiter (API-023, TC-020), the validation reasons (TC-021), the
//! Control Commands and the CLCW (TC-005, TC-030), and the argument checks (CABI-022) the
//! vectors do not reach.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test-only harness (RS-018)"
)]
#![expect(unsafe_code, reason = "the tests call the C ABI")]

use core::ptr::{null, null_mut};

use aldis_ffi::capacities::ALDIS_TC_MAX_MAPS;
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_CONTROL_COMMAND, ALDIS_FIELD_CONTROL_COMMAND_FLAG, ALDIS_FIELD_FRAME_DATA_FIELD,
    ALDIS_FIELD_MAP_ID, ALDIS_FIELD_SEGMENT_HEADER, ALDIS_FIELD_SPACECRAFT_ID, ALDIS_FIELD_VC_ID,
};
use aldis_ffi::tc::*;
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

/// Frames of at most 64 octets with an FECF on SCID 0x155.
fn mc() -> aldis_tc_mc_config_t {
    aldis_tc_mc_config_t {
        struct_size: size::<aldis_tc_mc_config_t>(),
        scid: 0x155,
        pc: aldis_tc_config_t {
            struct_size: size::<aldis_tc_config_t>(),
            fecf: 1,
            max_frame_len: 64,
        },
    }
}

/// VC 1 without SDLS, VC 2 with Segment Headers, VC 3 with a Security Header of 4 octets and a
/// Trailer of 2.
fn vcs() -> [aldis_tc_vc_config_t; 3] {
    let vc = aldis_tc_vc_config_t {
        struct_size: size::<aldis_tc_vc_config_t>(),
        vcid: 1,
        segment_header: 0,
        security_header_len: 0,
        security_trailer_len: 0,
        max_frame_len: 64,
    };
    [
        vc,
        aldis_tc_vc_config_t {
            vcid: 2,
            segment_header: 1,
            ..vc
        },
        aldis_tc_vc_config_t {
            vcid: 3,
            security_header_len: 4,
            security_trailer_len: 2,
            ..vc
        },
    ]
}

/// MAP 0 of VC 2 without SDLS, MAP 9 with a Security Header of 3 octets and a Trailer of 1.
fn maps() -> [aldis_tc_map_config_t; 2] {
    let map = aldis_tc_map_config_t {
        struct_size: size::<aldis_tc_map_config_t>(),
        vcid: 2,
        map_id: 0,
        security_header_len: 0,
        security_trailer_len: 0,
    };
    [
        map,
        aldis_tc_map_config_t {
            map_id: 9,
            security_header_len: 3,
            security_trailer_len: 1,
            ..map
        },
    ]
}

/// A Type-AD frame of `vcid`, with a Segment Header for `map` if given.
fn ad(vcid: u8, map: Option<u8>) -> aldis_tc_fields_t {
    aldis_tc_fields_t {
        frame_type: ALDIS_TC_FRAME_AD,
        scid: 0x155,
        vcid,
        sequence_number: 7,
        segment_header: u8::from(map.is_some()),
        sequence_flags: ALDIS_TC_SEQ_UNSEGMENTED,
        map_id: map.unwrap_or(0),
    }
}

fn check_mc(
    mc: &aldis_tc_mc_config_t,
    vcs: *const aldis_tc_vc_config_t,
    vc_count: usize,
    maps: *const aldis_tc_map_config_t,
    map_count: usize,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: `mc` and `err` are live; `vcs` and `maps` are NULL or point to their counts of
    // live structs.
    unsafe { aldis_tc_mc_config_check(mc, vcs, vc_count, maps, map_count, err) }
}

fn decode(bytes: &[u8], mode: u32, out: &mut aldis_tc_frame_t) -> u32 {
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    // SAFETY: every pointer is to a live local, with its length.
    unsafe {
        aldis_tc_decode(
            bytes.as_ptr(),
            bytes.len(),
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            mode,
            out,
            null_mut(),
        )
    }
}

/// A frame from `aldis_tc_encode` with `user` as the User Data, its Security Header (zeros from
/// the encoder) filled in place with `A5`, its Trailer with `5A`, and the FECF computed again
/// as a security layer does (API-072); with its layout from the decoder.
fn written(fields: &aldis_tc_fields_t, user: &[u8]) -> (Vec<u8>, aldis_tc_layout_t) {
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let mut frame = vec![0xEE_u8; 64];
    let content = aldis_tc_content_t {
        user_data: user.as_ptr(),
        user_data_len: user.len(),
        control: aldis_tc_control_t::default(),
    };
    let mut n = 0_usize;
    // SAFETY: every pointer is to a live local or vector, with its length.
    let encoded = unsafe {
        aldis_tc_encode(
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            fields,
            &raw const content,
            frame.as_mut_ptr(),
            frame.len(),
            &raw mut n,
            null_mut(),
        )
    };
    assert_eq!(encoded, ALDIS_OK);
    frame.truncate(n);
    let mut view = aldis_tc_frame_t::default();
    assert_eq!(decode(&frame, ALDIS_MODE_STRICT, &mut view), ALDIS_OK);
    let l = view.layout;
    assert!(
        frame[l.security_header_offset..][..l.security_header_len]
            .iter()
            .chain(&frame[l.security_trailer_offset..][..l.security_trailer_len])
            .all(|&b| b == 0),
        "the encoder leaves the security fields zero"
    );
    frame[l.security_header_offset..][..l.security_header_len].fill(0xA5);
    frame[l.security_trailer_offset..][..l.security_trailer_len].fill(0x5A);
    aldis::core::crc::write_fecf(&mut frame).unwrap();
    (frame, l)
}

/// A VC struct of a later header, with a member this library does not know.
#[repr(C)]
#[derive(Clone, Copy)]
struct LaterVc {
    vc: aldis_tc_vc_config_t,
    later: u64,
}

/// A MAP struct of a later header.
#[repr(C)]
#[derive(Clone, Copy)]
struct LaterMap {
    map: aldis_tc_map_config_t,
    later: u32,
}

#[test]
fn channel_arrays_step_by_their_struct_size() {
    // req: CABI-033, CABI-022, CABI-030, GEN-011, API-040, TC-024
    let mc = mc();
    let (base, base_maps) = (vcs(), maps());
    let mut e = detail();
    assert_eq!(
        check_mc(&mc, base.as_ptr(), 3, base_maps.as_ptr(), 2, &mut e),
        ALDIS_OK
    );
    // A master channel with no virtual channel receives nothing (OI-55).
    assert_eq!(
        check_mc(&mc, null(), 0, null(), 0, &mut e),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(e.field, ALDIS_FIELD_VC_ID);
    assert_eq!(
        check_mc(&mc, null(), 1, null(), 0, &mut e),
        ALDIS_ERR_INVALID_FIELD
    );
    // Longer structs of a later header: each array's stride is its own struct_size.
    let later = base.map(|vc| LaterVc {
        vc: aldis_tc_vc_config_t {
            struct_size: size::<LaterVc>(),
            ..vc
        },
        later: u64::MAX,
    });
    let later_maps = base_maps.map(|map| LaterMap {
        map: aldis_tc_map_config_t {
            struct_size: size::<LaterMap>(),
            ..map
        },
        later: u32::MAX,
    });
    let (v, m) = (later.as_ptr().cast(), later_maps.as_ptr().cast());
    assert_eq!(check_mc(&mc, v, 3, m, 2, &mut e), ALDIS_OK);
    // Every struct repeats its array's stride.
    let mut mixed = later_maps;
    mixed[1].map.struct_size = size::<aldis_tc_map_config_t>();
    assert_eq!(
        check_mc(&mc, v, 3, mixed.as_ptr().cast(), 2, &mut e),
        ALDIS_ERR_INVALID_FIELD
    );
    // The embedded physical channel carries its own struct_size.
    let mut small = mc;
    small.pc.struct_size = 4;
    assert_eq!(
        check_mc(&small, base.as_ptr(), 3, null(), 0, &mut e),
        ALDIS_ERR_INVALID_FIELD
    );
    // A VC with Segment Headers takes its SDLS lengths from its MAPs (232.0 Table 6-3).
    let mut sdls = base;
    sdls[1].security_header_len = 2;
    assert_eq!(
        check_mc(&mc, sdls.as_ptr(), 3, null(), 0, &mut e),
        ALDIS_ERR_INVALID_FIELD
    );
    // More than the 64 VCIDs of a master channel, or than ALDIS_TC_MAX_MAPS MAPs.
    let many = [base[0]; ALDIS_TC_MAX_VCS + 1];
    assert_eq!(
        check_mc(&mc, many.as_ptr(), many.len(), null(), 0, &mut e),
        ALDIS_ERR_CAPACITY_EXCEEDED
    );
    assert_eq!(e.field, ALDIS_FIELD_VC_ID);
    let too_many = [base_maps[0]; ALDIS_TC_MAX_MAPS + 1];
    assert_eq!(
        check_mc(
            &mc,
            base.as_ptr(),
            3,
            too_many.as_ptr(),
            too_many.len(),
            &mut e
        ),
        ALDIS_ERR_CAPACITY_EXCEEDED
    );
    assert_eq!(e.field, ALDIS_FIELD_MAP_ID);
    // All 64 VCIDs fit; a VCID twice is the codec's InvalidConfig.
    let all: [aldis_tc_vc_config_t; ALDIS_TC_MAX_VCS] =
        core::array::from_fn(|n| aldis_tc_vc_config_t {
            vcid: u8::try_from(n).unwrap(),
            ..base[0]
        });
    assert_eq!(
        check_mc(&mc, all.as_ptr(), all.len(), null(), 0, &mut e),
        ALDIS_OK
    );
    assert_eq!(
        check_mc(&mc, many.as_ptr(), 2, null(), 0, &mut e),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(e.field, ALDIS_FIELD_VC_ID);
    // A MAP of a VC without Segment Headers.
    let mut stray = base_maps;
    stray[0].vcid = 1;
    assert_eq!(
        check_mc(&mc, base.as_ptr(), 3, stray.as_ptr(), 2, &mut e),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(e.field, ALDIS_FIELD_MAP_ID);
}

#[test]
fn security_layouts_come_from_the_vc_or_its_map() {
    // req: API-070, API-072, TC-004, CABI-012
    // VC 3: Security Header 4, Trailer 2; two octets of User Data.
    let (frame, l) = written(&ad(3, None), &[1, 2]);
    assert_eq!(
        (l.security_header_offset, l.data_offset, l.data_len),
        (5, 9, 2)
    );
    assert_eq!((l.security_trailer_offset, l.frame_len), (11, 15));
    assert_eq!(frame.len(), 5 + 4 + 2 + 2 + 2);
    let mut view = aldis_tc_frame_t::default();
    assert_eq!(decode(&frame, ALDIS_MODE_STRICT, &mut view), ALDIS_OK);
    let v = view.layout;
    assert_eq!(
        &frame[v.security_header_offset..][..v.security_header_len],
        [0xA5; 4]
    );
    assert_eq!(&frame[v.data_offset..][..v.data_len], [1, 2]);
    assert_eq!(
        &frame[v.security_trailer_offset..][..v.security_trailer_len],
        [0x5A; 2]
    );
    assert!(!view.has_segment_header && view.has_fecf && v.fecf_len == 2);
    // VC 2, MAP 9: the Segment Header, then the MAP's Security Header 3 and Trailer 1.
    let (frame, l) = written(&ad(2, Some(9)), &[7, 8, 9]);
    assert_eq!((l.segment_header_offset, l.segment_header_len), (5, 1));
    assert_eq!((l.security_header_offset, l.security_header_len), (6, 3));
    assert_eq!(frame[5], 0xC9, "Sequence Flags 11, MAP 9");
    assert_eq!(decode(&frame, ALDIS_MODE_STRICT, &mut view), ALDIS_OK);
    assert!(view.has_segment_header);
    assert_eq!(
        (view.sequence_flags, view.map_id),
        (ALDIS_TC_SEQ_UNSEGMENTED, 9)
    );
    assert_eq!(view.layout, l);
    // MAP 0 of the same VC has no SDLS.
    let (frame, _) = written(&ad(2, Some(0)), &[7, 8, 9]);
    assert_eq!(decode(&frame, ALDIS_MODE_STRICT, &mut view), ALDIS_OK);
    assert_eq!(
        (view.layout.security_header_len, view.layout.data_offset),
        (0, 6)
    );
}

#[test]
fn the_header_and_length_peek_alone() {
    // req: API-013, API-014, TC-001, TC-002
    // A frame of SCID 0x2AA, which the master channel does not have: the header still decodes.
    let frame = [0x22, 0xAA, 0x08, 0x05, 0x03, 0x00];
    let mut h = aldis_tc_header_t::default();
    let mut n = 0_usize;
    let mut e = detail();
    // SAFETY: every pointer is to a live local, with its length.
    unsafe {
        assert_eq!(
            aldis_tc_header_decode(frame.as_ptr(), 6, &raw mut h, &raw mut e),
            ALDIS_OK
        );
        assert_eq!(
            aldis_tc_peek_length(frame.as_ptr(), 5, &raw mut n, &raw mut e),
            ALDIS_OK
        );
        assert_eq!(
            aldis_tc_header_decode(frame.as_ptr(), 4, &raw mut h, &raw mut e),
            ALDIS_ERR_INCOMPLETE
        );
        assert_eq!((e.detail, e.needed), (ALDIS_DETAIL_NEEDED, 5));
        assert_eq!(
            aldis_tc_header_decode(frame.as_ptr(), 6, null_mut(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    assert_eq!(
        (h.frame_type, h.scid, h.vcid, h.sequence_number, h.frame_len),
        (ALDIS_TC_FRAME_BD, 0x2AA, 2, 3, 6)
    );
    assert_eq!(n, 6);
    let mut view = aldis_tc_frame_t::default();
    assert_eq!(
        decode(&frame, ALDIS_MODE_STRICT, &mut view),
        ALDIS_ERR_UNKNOWN_CHANNEL
    );
}

#[test]
fn stream_reports_the_discard_reason() {
    // req: TC-021, TC-022, TC-023, CABI-020, CABI-021
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let step = |frame: &[u8], mc: &aldis_tc_mc_config_t| {
        let mut item = aldis_tc_stream_item_t::default();
        let mut offset = 0;
        let mut e = detail();
        // SAFETY: every pointer is to a live local, with its length.
        let status = unsafe {
            aldis_tc_stream_next(
                frame.as_ptr(),
                frame.len(),
                &raw mut offset,
                mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                ALDIS_MODE_STRICT,
                &raw mut item,
                &raw mut e,
            )
        };
        (status, offset, item)
    };
    let (frame, _) = written(&ad(1, None), &[1, 2]);
    let (status, offset, item) = step(&frame, &mc);
    assert_eq!(status, ALDIS_OK);
    assert_eq!(item.kind, ALDIS_TC_STREAM_ACCEPTED);
    assert_eq!(item.discard_reason, ALDIS_TC_DISCARD_NONE);
    assert_eq!(item.len, frame.len());
    assert_eq!(item.frame.layout.frame_len, frame.len());
    assert_eq!(offset, frame.len());

    // Another Spacecraft ID: check (b).
    let other = aldis_tc_mc_config_t { scid: 0x156, ..mc };
    let (status, offset, item) = step(&frame, &other);
    assert_eq!(status, ALDIS_OK);
    assert_eq!(item.kind, ALDIS_TC_STREAM_DISCARDED);
    assert_eq!(item.discard_reason, ALDIS_TC_DISCARD_UNEXPECTED_SPACECRAFT);
    assert_eq!(item.error_field, ALDIS_FIELD_SPACECRAFT_ID);
    assert_eq!(item.len, frame.len());
    assert_eq!(offset, frame.len());

    // A configuration error fails the call with an error code and writes no item.
    let bad = aldis_tc_mc_config_t { scid: 0x400, ..mc };
    let (status, _, _) = step(&frame, &bad);
    assert_eq!(status, ALDIS_ERR_OUT_OF_RANGE);
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per argument rule")]
fn encode_arguments_are_checked() {
    // req: CABI-022, CABI-011, API-022, TC-004, TC-005, TC-007
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let mut frame = [0xEE_u8; 64];
    let user = [0xD0_u8; 4];
    let encode = |fields: &aldis_tc_fields_t,
                  content: *const aldis_tc_content_t,
                  out: *mut u8,
                  cap: usize| {
        let mut n = 0_usize;
        let mut e = detail();
        // SAFETY: every pointer is to a live local or into `frame`; `content` is NULL or points
        // to a live struct whose buffer is live or into `frame`.
        let status = unsafe {
            aldis_tc_encode(
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                fields,
                content,
                out,
                cap,
                &raw mut n,
                &raw mut e,
            )
        };
        (status, e)
    };
    let content = aldis_tc_content_t {
        user_data: user.as_ptr(),
        user_data_len: user.len(),
        control: aldis_tc_control_t::default(),
    };
    let out = frame.as_mut_ptr();
    let fields = ad(1, None);
    assert_eq!(encode(&fields, &raw const content, out, 64).0, ALDIS_OK);
    assert_eq!(encode(&fields, null(), out, 64).0, ALDIS_ERR_INVALID_FIELD);
    // Too small an output: `needed`, and the output unchanged (API-022).
    frame.fill(0xEE);
    let (status, e) = encode(&fields, &raw const content, frame.as_mut_ptr(), 10);
    assert_eq!((status, e.needed), (ALDIS_ERR_BUFFER_TOO_SMALL, 11));
    assert!(frame.iter().all(|&b| b == 0xEE));
    // The output overlapping the User Data.
    let inside = aldis_tc_content_t {
        user_data: frame.as_ptr().wrapping_add(8),
        ..content
    };
    assert_eq!(
        encode(&fields, &raw const inside, frame.as_mut_ptr(), 64).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // A Control Command with User Data, or with a Segment Header; an unknown command.
    let unlock = aldis_tc_control_t {
        command: ALDIS_TC_CONTROL_UNLOCK,
        set_vr_value: 0,
    };
    let bc = aldis_tc_fields_t {
        frame_type: ALDIS_TC_FRAME_BC,
        ..fields
    };
    let both = aldis_tc_content_t {
        control: unlock,
        ..content
    };
    assert_eq!(
        encode(&bc, &raw const both, frame.as_mut_ptr(), 64).0,
        ALDIS_ERR_INVALID_FIELD
    );
    let command = aldis_tc_content_t {
        control: unlock,
        ..aldis_tc_content_t::default()
    };
    assert_eq!(
        encode(&bc, &raw const command, frame.as_mut_ptr(), 64).0,
        ALDIS_OK
    );
    let segmented = aldis_tc_fields_t {
        segment_header: 1,
        ..bc
    };
    assert_eq!(
        encode(&segmented, &raw const command, frame.as_mut_ptr(), 64).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // The codec's own checks: a frame type that does not match the content, a Segment Header
    // on a VC without them.
    let (status, e) = encode(&fields, &raw const command, frame.as_mut_ptr(), 64);
    assert_eq!(
        (status, e.field),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_CONTROL_COMMAND_FLAG)
    );
    let (status, e) = encode(&ad(1, Some(0)), &raw const content, frame.as_mut_ptr(), 64);
    assert_eq!(
        (status, e.field),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_SEGMENT_HEADER)
    );
    // A frame above the channel's maximum of 64 octets (TC-007).
    let big = [0_u8; 60];
    let too_big = aldis_tc_content_t {
        user_data: big.as_ptr(),
        user_data_len: 60,
        ..content
    };
    let mut roomy = [0xEE_u8; 128];
    let (status, _) = encode(&fields, &raw const too_big, roomy.as_mut_ptr(), 128);
    assert_eq!(status, ALDIS_ERR_OUT_OF_RANGE);
    assert!(roomy.iter().all(|&b| b == 0xEE));
    // An empty User Data on a Type-D frame (TC-004).
    let empty = aldis_tc_content_t {
        user_data_len: 0,
        ..content
    };
    let (status, e) = encode(&fields, &raw const empty, frame.as_mut_ptr(), 64);
    assert_eq!(
        (status, e.field),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_FRAME_DATA_FIELD)
    );
}

#[test]
fn control_commands() {
    // req: TC-005, CABI-022
    let mut c = aldis_tc_control_t::default();
    let mut e = detail();
    let mut octets = [0xEE_u8; 3];
    let mut n = 0_usize;
    let set_vr = aldis_tc_control_t {
        command: ALDIS_TC_CONTROL_SET_VR,
        set_vr_value: 0x2A,
    };
    // SAFETY: every pointer is to a live local, with its length.
    unsafe {
        assert_eq!(
            aldis_tc_control_decode([0x82, 0, 0x2A].as_ptr(), 3, &raw mut c, &raw mut e),
            ALDIS_OK
        );
        assert_eq!(c, set_vr);
        assert_eq!(
            aldis_tc_control_decode([0x82, 0].as_ptr(), 2, &raw mut c, &raw mut e),
            ALDIS_ERR_RESERVED_VALUE
        );
        assert_eq!(e.field, ALDIS_FIELD_CONTROL_COMMAND);
        assert_eq!(
            aldis_tc_control_encode(
                &raw const set_vr,
                octets.as_mut_ptr(),
                2,
                &raw mut n,
                &raw mut e
            ),
            ALDIS_ERR_BUFFER_TOO_SMALL
        );
        assert_eq!((e.needed, octets), (3, [0xEE; 3]));
        assert_eq!(
            aldis_tc_control_encode(
                &raw const set_vr,
                octets.as_mut_ptr(),
                3,
                &raw mut n,
                &raw mut e
            ),
            ALDIS_OK
        );
        assert_eq!((n, octets), (3, [0x82, 0, 0x2A]));
        let none = aldis_tc_control_t::default();
        assert_eq!(
            aldis_tc_control_encode(
                &raw const none,
                octets.as_mut_ptr(),
                3,
                &raw mut n,
                &raw mut e
            ),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn the_clcw_crosses_as_a_word() {
    // req: TC-030, TC-031, CABI-022, API-032
    let mut e = detail();
    // A CLCW round trip as a word, and its spares in lenient mode.
    let clcw = aldis_clcw_t {
        cop_in_effect: ALDIS_CLCW_COP_1,
        vcid: 5,
        retransmit: 1,
        farm_b_counter: 2,
        report_value: 42,
        ..aldis_clcw_t::default()
    };
    let mut word = 0_u32;
    let mut back = aldis_clcw_t::default();
    let mut anomalies = u32::MAX;
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_clcw_encode(&raw const clcw, &raw mut word, &raw mut e),
            ALDIS_OK
        );
        assert_eq!(word, 0x0114_0C2A);
        let decoded = aldis_clcw_decode(
            word,
            ALDIS_MODE_STRICT,
            &raw mut back,
            &raw mut anomalies,
            &raw mut e,
        );
        assert_eq!((decoded, back, anomalies), (ALDIS_OK, clcw, 0));
        let spare = aldis_clcw_decode(
            word | 0x0100,
            ALDIS_MODE_STRICT,
            &raw mut back,
            &raw mut anomalies,
            &raw mut e,
        );
        assert_eq!(spare, ALDIS_ERR_RESERVED_VALUE);
        let lenient = aldis_clcw_decode(
            word | 0x0100,
            ALDIS_MODE_LENIENT,
            &raw mut back,
            &raw mut anomalies,
            &raw mut e,
        );
        assert_eq!(
            (lenient, anomalies),
            (ALDIS_OK, ALDIS_TC_ANOMALY_CLCW_SPARE_SET)
        );
        assert_eq!(
            aldis_clcw_decode(word, 2, &raw mut back, &raw mut anomalies, &raw mut e),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_clcw_decode(
                word,
                ALDIS_MODE_STRICT,
                &raw mut back,
                null_mut(),
                &raw mut e
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        let flag = aldis_clcw_t { lockout: 2, ..clcw };
        assert_eq!(
            aldis_clcw_encode(&raw const flag, &raw mut word, &raw mut e),
            ALDIS_ERR_INVALID_FIELD
        );
        let vcid = aldis_clcw_t { vcid: 64, ..clcw };
        assert_eq!(
            aldis_clcw_encode(&raw const vcid, &raw mut word, &raw mut e),
            ALDIS_ERR_OUT_OF_RANGE
        );
        assert_eq!(e.field, ALDIS_FIELD_VC_ID);
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "straight-line intake stream scenario"
)]
fn the_stream_intake_pipeline() {
    // req: TC-020, TC-021, CABI-012, CABI-022
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let (frame, _) = written(&ad(1, None), &[1, 2]);
    let mut unit = frame.clone();
    unit.extend_from_slice(&frame);
    unit.extend_from_slice(&[0x55, 0x55, 0x55]);

    let mut offset = 0;
    let mut item = aldis_tc_stream_item_t::default();
    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let status = unsafe {
        aldis_tc_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (status, item.kind, item.offset, item.len, offset),
        (
            ALDIS_OK,
            ALDIS_TC_STREAM_ACCEPTED,
            0,
            frame.len(),
            frame.len()
        )
    );
    assert_eq!(item.frame.header.scid, 0x155);

    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let status = unsafe {
        aldis_tc_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (status, item.kind, item.offset, item.len, offset),
        (
            ALDIS_OK,
            ALDIS_TC_STREAM_ACCEPTED,
            frame.len(),
            frame.len(),
            frame.len() * 2
        )
    );

    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let status = unsafe {
        aldis_tc_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (
            status,
            item.kind,
            item.offset,
            item.len,
            item.discard_reason,
            offset
        ),
        (
            ALDIS_OK,
            ALDIS_TC_STREAM_DISCARDED,
            frame.len() * 2,
            3,
            ALDIS_TC_DISCARD_FILL,
            unit.len()
        )
    );

    // SAFETY: pointers point to valid stack and slice buffers and counts match.
    let status = unsafe {
        aldis_tc_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut offset,
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(
        (status, item.kind, item.len, offset),
        (ALDIS_OK, ALDIS_TC_STREAM_END, 0, unit.len())
    );
}

#[test]
fn stream_rejects_overlapping_pointers() {
    // req: CABI-022
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let unit = [0x31_u8, 0x55, 0x04, 0x05, 0x00, 0x00];
    let mut item = aldis_tc_stream_item_t::default();

    // 1. offset pointing into unit buffer is rejected
    let mut buffer = [0_usize; 8];
    let unit_ptr = buffer.as_ptr().cast::<u8>();
    let offset_ptr = buffer.as_mut_ptr();
    // SAFETY: pointers point to valid aligned stack buffer
    let status = unsafe {
        aldis_tc_stream_next(
            unit_ptr,
            buffer.len().saturating_mul(core::mem::size_of::<usize>()),
            offset_ptr,
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(status, ALDIS_ERR_INVALID_FIELD);

    // 2. out overlapping offset is rejected (e.g. &item.offset as offset and &item as out)
    // SAFETY: pointers point to valid stack item
    let status = unsafe {
        aldis_tc_stream_next(
            unit.as_ptr(),
            unit.len(),
            &raw mut item.offset,
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            ALDIS_MODE_STRICT,
            &raw mut item,
            null_mut(),
        )
    };
    assert_eq!(status, ALDIS_ERR_INVALID_FIELD);
}
