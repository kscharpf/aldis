//! Bit-flip completeness of the TC FECF (M2 plan WP-7; 232.0 §4.1.4, §4.4.9.3; TC-006, TC-021,
//! CMN-001, VER-023).
//!
//! Every single-bit error and every double-bit error of a set of valid frames with the FECF is
//! rejected by the Frame Validation Checks, [`validate_frame`] (TC-021), and never accepted as
//! another frame. A TC frame is at most 1 024 octets (232.0 §4.1.2.7.3), within the 4 095 octets
//! up to which the CRC-16 detects every double-bit error ([`aldis_testkit::bitflip`]), so the
//! double-bit property holds for every TC frame.
//!
//! The outcome depends on where the error is:
//!
//! - **Outside the Frame Length field** (bits 22–31, 232.0 §4.1.2.7.1) the frame keeps its
//!   length, so the CRC sees the error: both [`validate_frame`] and [`TcFrame::decode`] reject
//!   it (the FECF or an earlier header check, 232.0 §4.4.9.3).
//! - **In the Frame Length field** the frame claims another length. [`validate_frame`] compares
//!   it with the octets present (check (d), TC-021) and rejects it. [`TcFrame::decode`] reads a
//!   frame from the start of a longer input and ignores what follows (API-011): a shorter claimed
//!   length makes it check the FECF of a shorter frame, whose last two octets are data, so the
//!   CRC guarantee does not apply, and such a prefix passes when those octets happen to be its
//!   CRC (`decode_may_accept_a_frame_length_error_as_a_shorter_frame` builds one). So for those
//!   errors the property is only that `decode` never returns a frame of the input's whole
//!   length; a receiver of delimited frames must use [`validate_frame`] or compare
//!   [`TcFrame::len`] with the octets it gave.
//!
//! The frames are fixed, not drawn: the shortest Type-BD frame with the FECF, a Type-BC Unlock
//! frame, a Type-AD frame with a Segment Header, and a frame of the longest length.
//!
//! - Gated: every single-bit error of each frame; every double-bit error of the three short
//!   frames; the representative double-bit errors of `sampled_pairs` for the longest.
//! - Nightly (`#[ignore]`, the job `bit-flips` in `.github/workflows/nightly.yml`, release mode):
//!   every double-bit error of the longest frame, 33.5 million checks.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly, and bit and octet bookkeeping on lengths the frames bound (RS-018)"
)]

use core::ops::Range;

use aldis_core::crc::crc16;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, Error, Lenient};
use aldis_tc::delimit::{Delimited, FrameDelimiter, next_frame};
use aldis_tc::{
    ControlCommand, FrameData, FrameFields, FrameType, MAX_FRAME_LEN, MapConfig,
    PhysicalChannelConfig, Reason, Rejection, SegmentHeader, SegmentHeaderUse, SequenceFlags,
    TcFrame, TcMapId, TcSpacecraftId, TcVcId, ValidatedTcFrame, VcConfig, VcTable, encode,
    validate_frame, validate_frame_lenient,
};
use aldis_testkit::bitflip::{
    CRC16_DOUBLE_BIT_OCTETS, each_double, each_pair, each_single, filler, find_double,
    sampled_pairs,
};

/// `validate_frame` or `validate_frame_lenient`, as `mode` says (API-030).
fn validated<'a>(
    bytes: &'a [u8],
    t: &Channels,
    mode: DecodeMode,
) -> Result<ValidatedTcFrame<'a>, Rejection> {
    match mode {
        DecodeMode::Strict => validate_frame(bytes, t),
        DecodeMode::Lenient => validate_frame_lenient(bytes, t).map(Lenient::into_view),
    }
}

/// `TcFrame::decode` or `TcFrame::decode_lenient`, as `mode` says (API-030).
fn decoded<'a>(bytes: &'a [u8], t: &Channels, mode: DecodeMode) -> Result<TcFrame<'a>, Error> {
    match mode {
        DecodeMode::Strict => TcFrame::decode(bytes, t),
        DecodeMode::Lenient => TcFrame::decode_lenient(bytes, t).map(Lenient::into_view),
    }
}

// Every TC frame lies within the double-bit guarantee.
const _: () = assert!(MAX_FRAME_LEN <= CRC16_DOUBLE_BIT_OCTETS);

/// The Frame Length field: bits 22–31 of the Primary Header (232.0 §4.1.2.7.1).
const FRAME_LENGTH_BITS: Range<usize> = 22..32;

/// The channels: frames of up to 1 024 octets with the FECF; VC 1 without Segment Headers, VC 2
/// with them and MAP 3.
type Channels = VcTable<2, 1>;

const fn scid() -> TcSpacecraftId {
    TcSpacecraftId::new(0x2A5).unwrap()
}

const fn vc(id: u8) -> TcVcId {
    TcVcId::new(id).unwrap()
}

const fn map() -> TcMapId {
    TcMapId::new(3).unwrap()
}

fn channels() -> Channels {
    channels_with(true)
}

/// The same channels with or without the FECF.
fn channels_with(fecf: bool) -> Channels {
    let pc = PhysicalChannelConfig::new(MAX_FRAME_LEN, fecf).unwrap();
    let vcs = [
        VcConfig::new(
            scid(),
            vc(1),
            MAX_FRAME_LEN,
            SegmentHeaderUse::Absent(SecurityLayout::NONE),
        )
        .unwrap(),
        VcConfig::new(scid(), vc(2), MAX_FRAME_LEN, SegmentHeaderUse::Present).unwrap(),
    ];
    let maps = [MapConfig::new(scid(), vc(2), map(), SecurityLayout::NONE)];
    Channels::new(pc, &vcs, &maps).unwrap()
}

/// A frame of `frame_type` on VC `vc_id` holding `data`, with N(S) 7.
fn frame(t: &Channels, frame_type: FrameType, vc_id: u8, data: FrameData<'_>) -> Vec<u8> {
    let fields = FrameFields {
        frame_type,
        spacecraft_id: scid(),
        vc_id: vc(vc_id),
        sequence_number: FrameSeqNum::new(7),
    };
    let mut out = vec![0_u8; MAX_FRAME_LEN];
    let n = encode(t, fields, data, &mut out).unwrap();
    out.truncate(n);
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(validated(&out, t, mode).is_ok());
    }
    out
}

/// A Type-AD frame of `len` octets on VC 2, with a Segment Header for MAP 3 and data from `seed`.
fn segmented(t: &Channels, len: usize, seed: u32) -> Vec<u8> {
    // The Primary Header, the Segment Header, and the FECF if the channel has one.
    let overhead = if t.physical().fecf() { 8 } else { 6 };
    let user = filler(len - overhead, seed);
    let segment_header = Some(SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id: map(),
    });
    frame(
        t,
        FrameType::Ad,
        2,
        FrameData::Fdu {
            segment_header,
            user_data: &user,
        },
    )
}

/// The shortest Type-BD frame with the FECF (8 octets), a Type-BC Unlock frame, and a 64-octet
/// Type-AD frame with a Segment Header: every double-bit error is gated.
fn short_frames(t: &Channels) -> [Vec<u8>; 3] {
    let one = [0x5A];
    [
        frame(
            t,
            FrameType::Bd,
            1,
            FrameData::Fdu {
                segment_header: None,
                user_data: &one,
            },
        ),
        frame(
            t,
            FrameType::Bc,
            1,
            FrameData::Control(ControlCommand::Unlock),
        ),
        segmented(t, 64, 1),
    ]
}

/// Whether `bytes`, a valid frame with bit errors, is rejected as the module docs state: by
/// `validate_frame` in both modes, and by `decode` unless it returns a frame shorter than
/// `bytes` from an error in the Frame Length field.
fn rejected(t: &Channels, bytes: &[u8], bits: &[usize]) -> bool {
    let in_length = bits.iter().any(|b| FRAME_LENGTH_BITS.contains(b));
    [DecodeMode::Strict, DecodeMode::Lenient]
        .into_iter()
        .all(|mode| {
            validated(bytes, t, mode).is_err()
                && match decoded(bytes, t, mode) {
                    Err(_) => true,
                    Ok(f) => in_length && f.len() < bytes.len(),
                }
        })
}

#[test]
fn every_single_bit_error_is_rejected() {
    // req: TC-006, TC-021, CMN-001, VER-023, VER-064
    let t = channels();
    let longest = segmented(&t, MAX_FRAME_LEN, 4);
    for f in short_frames(&t).iter().chain([&longest]) {
        each_single(f, |bit, bytes| {
            assert!(
                rejected(&t, bytes, &[bit]),
                "{} octets, bit {bit}",
                bytes.len()
            );
        });
    }
}

#[test]
fn every_double_bit_error_of_the_short_frames_is_rejected() {
    // req: TC-006, TC-021, CMN-001, VER-023, VER-064
    let t = channels();
    for f in &short_frames(&t) {
        each_double(f, 0..f.len() * 8, |i, j, bytes| {
            assert!(
                rejected(&t, bytes, &[i, j]),
                "{} octets, bits {i}, {j}",
                bytes.len()
            );
        });
    }
}

#[test]
fn sampled_double_bit_errors_of_the_longest_frame_are_rejected() {
    // req: TC-006, TC-021, CMN-001, VER-023, VER-064
    let t = channels();
    let f = segmented(&t, MAX_FRAME_LEN, 4);
    each_pair(&f, &sampled_pairs(f.len() * 8, 32, 97), |i, j, bytes| {
        assert!(rejected(&t, bytes, &[i, j]), "bits {i}, {j}");
    });
}

#[test]
fn decode_may_accept_a_frame_length_error_as_a_shorter_frame() {
    // req: TC-021, VER-023, VER-064
    // A 64-octet frame whose octets 30 and 31 are the FECF of its first 30 octets with the
    // Frame Length bit of value 32 (bit 26) cleared: with that bit flipped, it reads as a valid
    // 32-octet frame followed by other octets.
    let t = channels();
    let mut f = segmented(&t, 64, 1);
    let mut header = f[..30].to_vec();
    header[3] ^= 0x20;
    let crc = crc16(&header).to_be_bytes();
    let mut user = f[6..62].to_vec();
    user[24..26].copy_from_slice(&crc);
    let segment_header = Some(SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id: map(),
    });
    f = frame(
        &t,
        FrameType::Ad,
        2,
        FrameData::Fdu {
            segment_header,
            user_data: &user,
        },
    );
    f[3] ^= 0x20;
    let short = TcFrame::decode(&f, &t).unwrap();
    assert_eq!(short.len(), 32);
    let rejection = validate_frame(&f, &t).unwrap_err();
    assert_eq!(rejection.reason(), Reason::LengthMismatch);
}

#[test]
#[ignore = "exhaustive, 33.5 million checks: the nightly job bit-flips runs it in release mode"]
fn every_double_bit_error_of_the_longest_frame_is_rejected() {
    // req: TC-006, TC-021, CMN-001, VER-023, VER-064
    let t = channels();
    let f = segmented(&t, MAX_FRAME_LEN, 4);
    assert_eq!(
        find_double(&f, |i, j, bytes| rejected(&t, bytes, &[i, j])),
        None
    );
}

/// The Frame Length field value of `bytes` (the length minus one, 232.0 §4.1.2.7.1).
fn length_field(bytes: &[u8]) -> usize {
    (usize::from(bytes[2] & 0x03) << 8) | usize::from(bytes[3])
}

/// `bytes` with the Frame Length field bit of value `1 << k` (k in 0..10) flipped.
fn flip_length_bit(bytes: &[u8], k: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[2] ^= u8::try_from((1_usize << k) >> 8).unwrap();
    out[3] ^= u8::try_from((1_usize << k) & 0xFF).unwrap();
    out
}

/// The first item of `unit` as a frame, or `None` when it is fill or a truncated frame.
fn first_frame(unit: &[u8]) -> Option<&[u8]> {
    match next_frame(unit) {
        Some(Delimited::Frame(f)) => Some(f),
        _ => None,
    }
}

#[test]
fn without_the_fecf_a_shortened_frame_is_delimited_and_accepted() {
    // req: TC-001, TC-006, TC-020, TC-021, API-011, VER-064
    // hazard: HZ-TC-01 (P-TC-03). Without the FECF, the Frame Length error that lowers the
    // length yields a frame of the shorter length from `next_frame`, and `validate_frame` of
    // those octets accepts it in both modes (check (d) reads the same field).
    let t = channels_with(false);
    let f = segmented(&t, 64, 1);
    assert_eq!(length_field(&f), 63);
    let mut lowered = 0;
    for k in 0..10 {
        if length_field(&f) & (1 << k) == 0 {
            continue;
        }
        let bad = flip_length_bit(&f, k);
        let claimed = length_field(&bad) + 1;
        assert!(claimed < f.len());
        let unit = FrameDelimiter::new(&bad).next().unwrap();
        let Delimited::Frame(short) = unit else {
            panic!("bit {k}: {unit:?}");
        };
        assert_eq!(short.len(), claimed, "bit {k}");
        assert_eq!(short, &bad[..claimed]);
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            assert!(validated(short, &t, mode).is_ok(), "bit {k}");
            assert_eq!(decoded(short, &t, mode).unwrap().len(), claimed);
        }
        // The whole unit as given, bounded by something other than the field, is rejected.
        assert_eq!(
            validate_frame(&bad, &t).unwrap_err().reason(),
            Reason::LengthMismatch
        );
        lowered += 1;
    }
    // Bits of value 32, 16, 8, 4, 2, and 1.
    assert_eq!(lowered, 6);
}

#[test]
fn with_the_fecf_a_shortened_frame_is_delimited_and_rejected() {
    // req: TC-001, TC-006, TC-020, TC-021, API-011, VER-064
    // hazard: HZ-TC-01 (P-TC-03). With the FECF the delimiter still yields the shorter frame,
    // and validate_frame rejects each one of this frame (the CRC guarantee does not apply to
    // a moved codeword end, so this pins the outcome for these octets, not a bound).
    let t = channels_with(true);
    let f = segmented(&t, 64, 1);
    for k in (0..10).filter(|k| length_field(&f) & (1 << k) != 0) {
        let bad = flip_length_bit(&f, k);
        let short = first_frame(&bad).unwrap();
        assert_eq!(short.len(), length_field(&bad) + 1);
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            assert_eq!(
                validated(short, &t, mode).unwrap_err().reason(),
                Reason::FecfMismatch,
                "bit {k}"
            );
        }
    }
}

/// A unit of a first frame of 32 octets and a second of 64.
fn two_frames(t: &Channels) -> Vec<u8> {
    let mut unit = segmented(t, 32, 1);
    unit.extend(segmented(t, 64, 2));
    unit
}

#[test]
fn without_the_fecf_a_lengthened_frame_is_delimited_and_accepted() {
    // req: TC-001, TC-006, TC-020, TC-021, API-011, VER-064
    // hazard: HZ-TC-04 (P-TC-07). Without the FECF, each Frame Length error of the first of
    // two frames that raises the length to a value within the unit yields a frame that holds
    // the first frame and the start of the second, and validate_frame accepts it.
    let t = channels_with(false);
    let unit = two_frames(&t);
    assert_eq!(unit.len(), 96);
    let mut raised = Vec::new();
    for k in 0..10 {
        if length_field(&unit) & (1 << k) != 0 {
            continue;
        }
        let bad = flip_length_bit(&unit, k);
        let claimed = length_field(&bad) + 1;
        if claimed > unit.len() {
            // The unit cannot complete it: a truncated frame (hz:HZ-TC-05).
            assert!(matches!(
                next_frame(&bad),
                Some(Delimited::TruncatedFrame { .. })
            ));
            continue;
        }
        let long = first_frame(&bad).unwrap();
        assert_eq!(long.len(), claimed, "bit {k}");
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            assert!(validated(long, &t, mode).is_ok(), "bit {k}");
        }
        raised.push(claimed);
    }
    // Bits of value 32 and 64 of the length 31.
    assert_eq!(raised, [64, 96]);
}

#[test]
fn with_the_fecf_a_lengthened_frame_is_delimited_and_rejected() {
    // req: TC-001, TC-006, TC-020, TC-021, API-011, VER-064
    // hazard: HZ-TC-04 (P-TC-07). With the FECF the raised length is delimited and each such
    // frame of this unit is rejected by the FECF; a pass would be the 2^-16 chance of the
    // moved codeword end, and would be listed here.
    let t = channels_with(true);
    let unit = two_frames(&t);
    let mut passed = Vec::new();
    let mut checked = 0;
    for k in (0..10).filter(|k| length_field(&unit) & (1 << k) == 0) {
        let bad = flip_length_bit(&unit, k);
        let Some(long) = first_frame(&bad) else {
            continue;
        };
        checked += 1;
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            if validated(long, &t, mode).is_ok() {
                passed.push((k, mode));
            }
        }
    }
    assert_eq!(checked, 2);
    assert!(passed.is_empty(), "accepted: {passed:?}");
}
