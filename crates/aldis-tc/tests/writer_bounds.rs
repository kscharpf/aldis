//! The TC writers write inside their frame or reservation and nowhere else (232.0 §4.1;
//! API-022, API-023, GEN-014, GEN-020; hazard HZ-TC-16, proposal P-TC-06).
//!
//! The buffer is filled with a pattern and is longer than the frame. After a successful
//! `encode`, `TcFrameWriter::begin`, and `finish`, every octet from the end of the frame (or,
//! before `finish`, from the end of the reservation) on is the pattern; an `encode` rejected
//! for any reason, and a `begin` or `finish` that fails, change no octet of the frame or the
//! reservation either (API-022). A `finish` below the reservation leaves the reserved octets
//! after the frame unspecified (its doc comment): the test does not look at them.
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly, and bookkeeping on lengths the fixtures bound (RS-018)"
)]

use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_tc::{
    ControlCommand, FrameData, FrameFields, FrameType, MapConfig, PhysicalChannelConfig,
    SegmentHeader, SegmentHeaderUse, SequenceFlags, TcFrame, TcFrameWriter, TcMapId,
    TcSpacecraftId, TcVcId, VcConfig, VcTable, encode, validate_frame,
};

/// Channels with the FECF: VC 1 without Segment Headers, VC 2 with them and MAP 3, and VC 4
/// without Segment Headers and with a 4-octet Security Header and 8-octet Security Trailer.
type Channels = VcTable<3, 1>;

const fn scid() -> TcSpacecraftId {
    TcSpacecraftId::new(0x2A5).unwrap()
}

const fn vc(id: u8) -> TcVcId {
    TcVcId::new(id).unwrap()
}

fn channels() -> Channels {
    let pc = PhysicalChannelConfig::new(128, true).unwrap();
    let vcs = [
        VcConfig::new(
            scid(),
            vc(1),
            128,
            SegmentHeaderUse::Absent(SecurityLayout::NONE),
        )
        .unwrap(),
        VcConfig::new(scid(), vc(2), 128, SegmentHeaderUse::Present).unwrap(),
        VcConfig::new(
            scid(),
            vc(4),
            128,
            SegmentHeaderUse::Absent(SecurityLayout::new(4, 8).unwrap()),
        )
        .unwrap(),
    ];
    let maps = [MapConfig::new(
        scid(),
        vc(2),
        TcMapId::new(3).unwrap(),
        SecurityLayout::NONE,
    )];
    Channels::new(pc, &vcs, &maps).unwrap()
}

const fn fields(frame_type: FrameType, vc_id: u8) -> FrameFields {
    FrameFields {
        frame_type,
        spacecraft_id: scid(),
        vc_id: vc(vc_id),
        sequence_number: FrameSeqNum::new(7),
    }
}

/// A buffer of `len` octets of a pattern with no repeated octet next to another.
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from((i * 37 + 11) % 251).unwrap())
        .collect()
}

const BUFFER: usize = 300;

#[test]
fn encode_writes_only_the_frame() {
    // req: API-022, GEN-014, GEN-020, TC-007
    // hazard: HZ-TC-16 (P-TC-06).
    let t = channels();
    let segment = Some(SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id: TcMapId::new(3).unwrap(),
    });
    let user = [0x11_u8; 20];
    let cases: [(FrameFields, FrameData<'_>); 4] = [
        (
            fields(FrameType::Ad, 1),
            FrameData::Fdu {
                segment_header: None,
                user_data: &user[..1],
            },
        ),
        (
            fields(FrameType::Bd, 2),
            FrameData::Fdu {
                segment_header: segment,
                user_data: &user,
            },
        ),
        (
            fields(FrameType::Ad, 4),
            FrameData::Fdu {
                segment_header: None,
                user_data: &user,
            },
        ),
        (
            fields(FrameType::Bc, 1),
            FrameData::Control(ControlCommand::Unlock),
        ),
    ];
    for (f, data) in cases {
        let before = pattern(BUFFER);
        let mut out = before.clone();
        let n = encode(&t, f, data, &mut out).unwrap();
        assert!(n < BUFFER);
        assert_eq!(out[n..], before[n..], "{f:?}");
        assert!(validate_frame(&out[..n], &t, DecodeMode::Strict).is_ok());
        // The buffer exactly as long as the frame.
        let mut exact = before[..n].to_vec();
        assert_eq!(encode(&t, f, data, &mut exact), Ok(n));
        assert_eq!(exact, out[..n]);
    }
}

#[test]
fn a_rejected_encode_changes_nothing() {
    // req: API-022, GEN-020, TC-004, TC-007
    // hazard: HZ-TC-16 (P-TC-06). Each kind of rejection of `encode`, with the pattern in the
    // whole buffer.
    let t = channels();
    let user = [0x22_u8; 20];
    let segment = Some(SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id: TcMapId::new(3).unwrap(),
    });
    let fdu = |segment_header, user_data| FrameData::Fdu {
        segment_header,
        user_data,
    };
    let cases = [
        // An unknown VC.
        (fields(FrameType::Ad, 5), fdu(None, &user[..]), BUFFER),
        // A Segment Header on a VC without them, and none on a VC with them.
        (fields(FrameType::Ad, 1), fdu(segment, &user[..]), BUFFER),
        (fields(FrameType::Ad, 2), fdu(None, &user[..]), BUFFER),
        // Empty User Data.
        (fields(FrameType::Ad, 1), fdu(None, &[][..]), BUFFER),
        // Above the maximum frame length.
        (
            fields(FrameType::Ad, 1),
            fdu(None, &[0_u8; 200][..]),
            BUFFER,
        ),
        // A Control Command on a Type-AD frame, and User Data on a Type-BC frame.
        (
            fields(FrameType::Ad, 1),
            FrameData::Control(ControlCommand::Unlock),
            BUFFER,
        ),
        (fields(FrameType::Bc, 1), fdu(None, &user[..]), BUFFER),
        // A buffer one octet short of the frame (5 + 20 + 2 = 27).
        (fields(FrameType::Ad, 1), fdu(None, &user[..]), 26),
    ];
    for (f, data, len) in cases {
        let before = pattern(len);
        let mut out = before.clone();
        let e = encode(&t, f, data, &mut out).unwrap_err();
        assert_eq!(out, before, "{f:?}: {:?}", e.kind());
    }
    // The last case is the buffer error.
    let mut out = pattern(26);
    let e = encode(&t, fields(FrameType::Ad, 1), fdu(None, &user[..]), &mut out).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
}

#[test]
fn begin_and_finish_write_only_the_reservation() {
    // req: API-020, API-022, API-023, GEN-014, GEN-020
    // hazard: HZ-TC-16 (P-TC-06). Full and short `finish`, with and without a Security Trailer
    // (which `finish` moves in front of the unused User Data).
    let t = channels();
    let max_user = 40;
    for vc_id in [1, 4] {
        let f = fields(FrameType::Ad, vc_id);
        for user_len in [max_user, 25, 1] {
            let before = pattern(BUFFER);
            let mut out = before.clone();
            let mut w = TcFrameWriter::begin(&mut out, &t, f, None, max_user).unwrap();
            let reserved = w.user_data_mut().len();
            assert_eq!(reserved, max_user);
            let security = w.security_header_mut().len() + w.security_trailer_mut().len();
            let total = 5 + security + max_user + 2;
            w.user_data_mut()[..user_len].fill(0x5C);
            let n = w.finish(user_len).unwrap();
            assert_eq!(n, total - (max_user - user_len));
            // Beyond the reservation nothing changes; inside it, the frame is a valid frame.
            assert_eq!(out[total..], before[total..], "vc {vc_id}, {user_len}");
            let frame = TcFrame::decode(&out[..n], &t, DecodeMode::Strict).unwrap();
            assert_eq!(frame.len(), n);
            assert!(validate_frame(&out[..n], &t, DecodeMode::Strict).is_ok());
        }
        // `begin` alone writes within the reservation.
        let before = pattern(BUFFER);
        let mut out = before.clone();
        let total = {
            let mut w = TcFrameWriter::begin(&mut out, &t, f, None, max_user).unwrap();
            5 + w.security_header_mut().len() + max_user + w.security_trailer_mut().len() + 2
        };
        assert_eq!(out[total..], before[total..]);
    }
}

#[test]
fn a_failed_begin_or_finish_changes_nothing_of_the_frame() {
    // req: API-022, API-023, GEN-020
    // hazard: HZ-TC-16 (P-TC-06). `begin` that fails leaves the buffer as it was; `finish`
    // that fails (a User Data length of 0 or above the reservation) leaves the unfinished
    // frame as `begin` wrote it.
    let t = channels();
    let f = fields(FrameType::Ad, 1);
    let before = pattern(BUFFER);
    // Too small for the reservation (5 + 40 + 2 = 47).
    let mut out = before[..46].to_vec();
    let e = TcFrameWriter::begin(&mut out, &t, f, None, 40).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(out, before[..46]);
    // An unknown VC.
    let mut out = before.clone();
    TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 5), None, 40).unwrap_err();
    assert_eq!(out, before);
    // A failed finish.
    for user_len in [0, 41] {
        let mut out = before.clone();
        let w = TcFrameWriter::begin(&mut out, &t, f, None, 40).unwrap();
        let begun = {
            // The writer borrows the buffer: compare after it is gone.
            w.finish(user_len).unwrap_err();
            out.clone()
        };
        let mut again = before.clone();
        {
            let _begun = TcFrameWriter::begin(&mut again, &t, f, None, 40).unwrap();
        }
        assert_eq!(begun, again, "user_len {user_len}");
        assert_eq!(begun[47..], before[47..]);
    }
}
