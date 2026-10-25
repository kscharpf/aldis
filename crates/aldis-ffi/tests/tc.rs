//! The contract of the TC part of the C ABI, called as a C caller calls it: the virtual channel
//! and MAP arrays (CABI-033, M2 plan D14), the security layouts of a VC or a MAP (API-070), the
//! stateless writers and delimiter (API-023, TC-020), the validation reasons (TC-021), the
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
    ALDIS_FIELD_MAP_ID, ALDIS_FIELD_SEGMENT_HEADER, ALDIS_FIELD_SPACECRAFT_ID,
    ALDIS_FIELD_USER_LEN, ALDIS_FIELD_VC_ID,
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
) -> i32 {
    // SAFETY: `mc` and `err` are live; `vcs` and `maps` are NULL or point to their counts of
    // live structs.
    unsafe { aldis_tc_mc_config_check(mc, vcs, vc_count, maps, map_count, err) }
}

fn decode(bytes: &[u8], mode: u32, out: &mut aldis_tc_frame_t) -> i32 {
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

/// A frame through the two-phase writer with `max` octets reserved and `user` written, the
/// Security Header filled with `A5` and the Trailer with `5A`.
fn written(fields: &aldis_tc_fields_t, max: usize, user: &[u8]) -> (Vec<u8>, aldis_tc_layout_t) {
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let mut frame = vec![0xEE_u8; 64];
    let mut l = aldis_tc_layout_t::default();
    // SAFETY: every pointer is to a live local or vector, with its length.
    let begun = unsafe {
        aldis_tc_writer_begin(
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            fields,
            max,
            frame.as_mut_ptr(),
            frame.len(),
            &raw mut l,
            null_mut(),
        )
    };
    assert_eq!(begun, ALDIS_OK);
    // The unfinished header is version `11`, which every decoder rejects (API-023).
    let mut view = aldis_tc_frame_t::default();
    assert_eq!(
        decode(&frame, ALDIS_MODE_LENIENT, &mut view),
        ALDIS_ERR_UNSUPPORTED_VERSION
    );
    frame[l.security_header_offset..][..l.security_header_len].fill(0xA5);
    frame[l.data_offset..][..user.len()].copy_from_slice(user);
    frame[l.security_trailer_offset..][..l.security_trailer_len].fill(0x5A);
    let mut n = 0_usize;
    // SAFETY: as above.
    let finished = unsafe {
        aldis_tc_writer_finish(
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            maps.as_ptr(),
            maps.len(),
            fields,
            max,
            user.len(),
            frame.as_mut_ptr(),
            frame.len(),
            &raw mut n,
            null_mut(),
        )
    };
    assert_eq!(finished, ALDIS_OK);
    frame.truncate(n);
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
    // req: CABI-033, CABI-022, CABI-030, GEN-011, API-040
    let mc = mc();
    let (base, base_maps) = (vcs(), maps());
    let mut e = detail();
    assert_eq!(
        check_mc(&mc, base.as_ptr(), 3, base_maps.as_ptr(), 2, &mut e),
        ALDIS_OK
    );
    assert_eq!(check_mc(&mc, null(), 0, null(), 0, &mut e), ALDIS_OK);
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
    // req: API-070, API-072, API-023, TC-004, CABI-012
    // VC 3: Security Header 4, Trailer 2; four octets reserved, two used, so the Trailer moves.
    let (frame, l) = written(&ad(3, None), 4, &[1, 2]);
    assert_eq!(
        (l.security_header_offset, l.data_offset, l.data_len),
        (5, 9, 4)
    );
    assert_eq!((l.security_trailer_offset, l.frame_len), (13, 17));
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
    let (frame, l) = written(&ad(2, Some(9)), 3, &[7, 8, 9]);
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
    let (frame, _) = written(&ad(2, Some(0)), 3, &[7, 8, 9]);
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
fn validation_reports_the_reason() {
    // req: TC-021, TC-022, TC-023, CABI-020, CABI-021
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let validate = |frame: &[u8], mc: &aldis_tc_mc_config_t, reason: *mut u32| {
        let mut view = aldis_tc_frame_t::default();
        let mut e = detail();
        // SAFETY: every pointer is to a live local, with its length; `reason` is NULL or live.
        let status = unsafe {
            aldis_tc_validate(
                frame.as_ptr(),
                frame.len(),
                mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                ALDIS_MODE_STRICT,
                &raw mut view,
                reason,
                &raw mut e,
            )
        };
        (status, e.field, view)
    };
    let (frame, _) = written(&ad(1, None), 2, &[1, 2]);
    let mut reason = u32::MAX;
    let (status, _, view) = validate(&frame, &mc, &raw mut reason);
    assert_eq!((status, reason), (ALDIS_OK, ALDIS_TC_REASON_NONE));
    assert_eq!(view.layout.frame_len, frame.len());
    // One octet more than the Frame Length: check (d), as the decoder does not check it.
    let mut longer = frame.clone();
    longer.push(0);
    let (status, _, _) = validate(&longer, &mc, &raw mut reason);
    assert_eq!(
        (status, reason),
        (ALDIS_ERR_LENGTH_MISMATCH, ALDIS_TC_REASON_LENGTH_MISMATCH)
    );
    let mut view = aldis_tc_frame_t::default();
    assert_eq!(decode(&longer, ALDIS_MODE_STRICT, &mut view), ALDIS_OK);
    // Another Spacecraft ID: check (b).
    let other = aldis_tc_mc_config_t { scid: 0x156, ..mc };
    let (status, field, _) = validate(&frame, &other, &raw mut reason);
    assert_eq!(
        (status, field, reason),
        (
            ALDIS_ERR_UNKNOWN_CHANNEL,
            ALDIS_FIELD_SPACECRAFT_ID,
            ALDIS_TC_REASON_UNEXPECTED_SPACECRAFT
        )
    );
    // A configuration error is not a rejection: `reason` is left as it was.
    let bad = aldis_tc_mc_config_t { scid: 0x400, ..mc };
    reason = u32::MAX;
    let (status, _, _) = validate(&frame, &bad, &raw mut reason);
    assert_eq!((status, reason), (ALDIS_ERR_OUT_OF_RANGE, u32::MAX));
    let (status, _, _) = validate(&frame, &mc, null_mut());
    assert_eq!(status, ALDIS_ERR_INVALID_FIELD);
}

#[test]
#[expect(clippy::too_many_lines, reason = "one check per argument rule")]
fn writer_arguments_are_checked() {
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
    // The two-phase writer: a Type-BC frame, and User Data above the reservation or empty.
    let begin = |fields: &aldis_tc_fields_t, max: usize, frame: &mut [u8]| {
        let mut l = aldis_tc_layout_t::default();
        let mut e = detail();
        // SAFETY: every pointer is to a live local or into `frame`, with its length.
        let status = unsafe {
            aldis_tc_writer_begin(
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                fields,
                max,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut l,
                &raw mut e,
            )
        };
        (status, e)
    };
    let finish = |fields: &aldis_tc_fields_t, max: usize, used: usize, frame: &mut [u8]| {
        let mut n = 0_usize;
        let mut e = detail();
        // SAFETY: as above.
        let status = unsafe {
            aldis_tc_writer_finish(
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                fields,
                max,
                used,
                frame.as_mut_ptr(),
                frame.len(),
                &raw mut n,
                &raw mut e,
            )
        };
        (status, e, n)
    };
    let (status, e) = begin(&bc, 1, &mut frame);
    assert_eq!(
        (status, e.field),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_CONTROL_COMMAND_FLAG)
    );
    assert_eq!(begin(&fields, 4, &mut frame).0, ALDIS_OK);
    let before = frame;
    let (status, e, _) = finish(&fields, 4, 5, &mut frame);
    assert_eq!(
        (status, e.field),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_USER_LEN)
    );
    let (status, e, _) = finish(&fields, 4, 0, &mut frame);
    assert_eq!(
        (status, e.field),
        (ALDIS_ERR_INVALID_FIELD, ALDIS_FIELD_FRAME_DATA_FIELD)
    );
    assert_eq!(frame, before, "a failed finish leaves the frame unchanged");
    let (status, _, n) = finish(&fields, 4, 4, &mut frame);
    assert_eq!((status, n), (ALDIS_OK, 11));
    // A frame above the channel's maximum of 64 octets (TC-007).
    let (status, _) = begin(&fields, 60, &mut frame);
    assert_eq!(status, ALDIS_ERR_OUT_OF_RANGE);
}

#[test]
fn finish_checks_the_buffer_was_begun_with_these_arguments() {
    // req: API-023, CABI-022, TC-004 (OI-50)
    let (mc, vcs, maps) = (mc(), vcs(), maps());
    let call = |begin: bool, fields: &aldis_tc_fields_t, max: usize, frame: &mut [u8]| {
        let mut l = aldis_tc_layout_t::default();
        let mut n = 0_usize;
        // SAFETY: every pointer is to a live local or into `frame`, with its length.
        unsafe {
            if begin {
                aldis_tc_writer_begin(
                    &raw const mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    maps.as_ptr(),
                    maps.len(),
                    fields,
                    max,
                    frame.as_mut_ptr(),
                    frame.len(),
                    &raw mut l,
                    null_mut(),
                )
            } else {
                aldis_tc_writer_finish(
                    &raw const mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    maps.as_ptr(),
                    maps.len(),
                    fields,
                    max,
                    1,
                    frame.as_mut_ptr(),
                    frame.len(),
                    &raw mut n,
                    null_mut(),
                )
            }
        }
    };
    // Another MAP, VC, frame type, sequence number, sequence flags, and reservation.
    let begun = ad(2, Some(0));
    let mut frame = [0xEE_u8; 64];
    assert_eq!(call(true, &begun, 4, &mut frame), ALDIS_OK);
    let written = frame;
    let other = [
        (ad(2, Some(9)), 4),
        (ad(3, None), 4),
        (
            aldis_tc_fields_t {
                frame_type: ALDIS_TC_FRAME_BD,
                ..begun
            },
            4,
        ),
        (
            aldis_tc_fields_t {
                sequence_number: 8,
                ..begun
            },
            4,
        ),
        (
            aldis_tc_fields_t {
                sequence_flags: ALDIS_TC_SEQ_FIRST,
                ..begun
            },
            4,
        ),
        (begun, 3),
        (begun, 5),
    ];
    for (f, max) in other {
        assert_eq!(
            call(false, &f, max, &mut frame),
            ALDIS_ERR_INVALID_STATE,
            "{f:?} {max}"
        );
        assert_eq!(frame, written, "a failed finish leaves the frame unchanged");
    }
    // The same arguments finish it.
    assert_eq!(call(false, &begun, 4, &mut frame), ALDIS_OK);
    // A buffer never begun, and a finished frame.
    for fill in [0x00_u8, 0xA5] {
        let mut never = [fill; 64];
        assert_eq!(call(false, &begun, 4, &mut never), ALDIS_ERR_INVALID_STATE);
        assert_eq!(never, [fill; 64]);
    }
    assert_eq!(call(false, &begun, 4, &mut frame), ALDIS_ERR_INVALID_STATE);
}

#[test]
fn the_delimiter_is_stateless() {
    // req: TC-020, CABI-012, CABI-022
    // An Unlock frame without FECF, then three octets of fill.
    let unit = [0x31, 0x55, 0x04, 0x05, 0x00, 0x00, 0x55, 0x55, 0x55];
    let next = |unit: *const u8, len: usize, offset: usize| {
        let mut item = aldis_tc_delimited_t::default();
        // SAFETY: `unit` is NULL or live for `len` octets; `item` is live.
        let status = unsafe { aldis_tc_next_frame(unit, len, offset, &raw mut item, null_mut()) };
        (status, item)
    };
    let (status, item) = next(unit.as_ptr(), unit.len(), 0);
    assert_eq!(
        (status, item.kind, item.len, item.declared_len),
        (ALDIS_OK, ALDIS_TC_DELIMITED_FRAME, 6, 6)
    );
    // The same call gives the same item: nothing is kept.
    assert_eq!(next(unit.as_ptr(), unit.len(), 0).1, item);
    let (_, item) = next(unit.as_ptr(), unit.len(), 6);
    assert_eq!(
        (item.kind, item.offset, item.len),
        (ALDIS_TC_DELIMITED_FILL, 6, 3)
    );
    assert_eq!(
        next(unit.as_ptr(), unit.len(), 9).1.kind,
        ALDIS_TC_DELIMITED_END
    );
    assert_eq!(next(null(), 0, 0).1.kind, ALDIS_TC_DELIMITED_END);
    assert_eq!(
        next(unit.as_ptr(), unit.len(), 10).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // A valid header declaring more octets than remain.
    let (_, item) = next(unit.as_ptr(), 5, 0);
    assert_eq!(
        (item.kind, item.len, item.declared_len),
        (ALDIS_TC_DELIMITED_TRUNCATED_FRAME, 5, 6)
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
