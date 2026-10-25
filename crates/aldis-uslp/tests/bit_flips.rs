//! Bit-flip completeness of the USLP FECF (M2 plan WP-7; 732.1 §4.1.6, Annex B1, §4.3.11.3;
//! USLP-003, USLP-014, CMN-001, VER-023).
//!
//! Every single-bit error and every double-bit error of a set of valid frames with the FECF is
//! rejected by the USLP Frame Validation Checks, [`validate_frame`] (USLP-014), and never
//! accepted as another frame, within the length the CRC-16 guarantees. A truncated frame has no
//! FECF (732.1 Annex D1.2.2 note 1) and is not part of this suite.
//!
//! The outcome depends on where the errors are:
//!
//! - **Outside the Frame Length field** (bits 32–47, §4.1.2.7.1), the frame keeps its length
//!   and the CRC sees the error: [`validate_frame`] and [`UslpFrame::decode`] reject it.
//! - **In the Frame Length field**, on a fixed-length channel the length no longer matches the
//!   channel's (USLP-003), and both reject it. On a variable-length channel [`validate_frame`]
//!   compares it with the octets present (check (d), USLP-014) and rejects it, while
//!   [`UslpFrame::decode`] reads a frame from the start of a longer input and ignores what
//!   follows (API-011): a shorter claimed length makes it check the FECF of a shorter frame,
//!   which the CRC guarantee does not cover. So for those errors `decode` only never returns a
//!   frame of the input's whole length (as for TC, `aldis-tc/tests/bit_flips.rs`).
//! - **Undetected**: the CRC-16 detects every double-bit error only in a frame of at most 4 095
//!   octets ([`aldis_testkit::bitflip`]). USLP frames may be up to 65 536 octets (§4.1.2.7.2,
//!   131.0 §11.3); in a longer frame two errors 32 767 bits apart pass the FECF, and
//!   `a_double_bit_error_32767_bits_apart_passes_the_fecf` shows one decoded as a different
//!   frame. The other double-bit errors of such a frame are still rejected.
//!
//! The frames are fixed, not drawn. On a variable-length channel of frames up to 65 536 octets:
//! the shortest frame with the FECF, a 64-octet frame with an OCF, a frame of 4 095 octets (the
//! longest the double-bit guarantee covers), and a frame of 65 536 octets. On a fixed-length
//! channel: a 64-octet frame with an Insert Zone and an OCF.
//!
//! - Gated: every single-bit and double-bit error of the three short frames; for the two long
//!   ones, the representative single-bit errors of `sampled_bits` and double-bit errors of
//!   `sampled_pairs` (`sampled` below).
//! - Nightly (`#[ignore]`, the job `bit-flips` in `.github/workflows/nightly.yml`, release mode):
//!   every single-bit error of both long frames, and every double-bit error of the 4 095-octet
//!   frame (537 million checks).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly, and bit and octet bookkeeping on lengths the frames bound (RS-018)"
)]

use core::ops::Range;

use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::bitflip::{
    CRC16_DOUBLE_BIT_BITS, CRC16_DOUBLE_BIT_OCTETS, each_bit, each_double, each_pair, each_single,
    filler, find_double, find_single, flip, sampled_bits, sampled_pairs,
};
use aldis_uslp::{
    FirstHeaderPointer, FrameContent, FrameFields, FrameKind, MAX_FRAME_LEN, MapConfig, OcfUse,
    PhysicalChannelConfig, Reason, SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType,
    Upid, UslpFrame, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable,
    VcfCountLen, encode, validate_frame,
};

/// The Frame Length field: bits 32–47 of the Primary Header (732.1 §4.1.2.7.1).
const FRAME_LENGTH_BITS: Range<usize> = 32..48;

/// The channels: one VC with one MAP.
type Channels = VcTable<1, 1>;

/// A valid frame and its channels.
struct Frame {
    table: Channels,
    octets: Vec<u8>,
}

impl Frame {
    fn variable(&self) -> bool {
        self.table.physical().frame_type() == TransferFrameType::Variable
    }
}

/// A table for frames of `len` octets (the maximum, if variable) with the FECF: VC 1 with a
/// 2-octet VCF Count for sequence-controlled frames and MAP 2. Fixed-length: a 4-octet Insert
/// Zone and an OCF in every frame. Variable-length: an OCF allowed.
fn channels(frame_type: TransferFrameType, len: usize) -> Channels {
    let fixed = frame_type == TransferFrameType::Fixed;
    let pc = PhysicalChannelConfig::new(frame_type, len, if fixed { 4 } else { 0 }, true).unwrap();
    let scid = UslpSpacecraftId::new(0xA5A5).unwrap();
    let vc_id = UslpVcId::new(1).unwrap();
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(2).unwrap(),
        ocf: if fixed {
            OcfUse::Required
        } else {
            OcfUse::Allowed
        },
        ..VcParams::new(frame_type)
    };
    let vc = VcConfig::new(scid, vc_id, params).unwrap();
    let map = MapConfig::new(
        scid,
        vc_id,
        UslpMapId::new(2).unwrap(),
        SecurityLayout::NONE,
    );
    Channels::new(pc, &[vc], &[map]).unwrap()
}

/// A frame on `table` with a TFDZ of `tfdz` octets from `seed`, and an OCF if `ocf`.
fn frame(table: Channels, tfdz: usize, ocf: bool, seed: u32) -> Frame {
    let pc = table.physical();
    let fixed = pc.frame_type() == TransferFrameType::Fixed;
    let fields = FrameFields {
        spacecraft_id: UslpSpacecraftId::new(0xA5A5).unwrap(),
        source_or_destination: SourceOrDestination::from_bit(false),
        vc_id: UslpVcId::new(1).unwrap(),
        map_id: UslpMapId::new(2).unwrap(),
        kind: FrameKind::SequenceControlled,
        vcf_count: 0x5A0F,
    };
    let rule = if fixed {
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(0))
    } else {
        TfdzRule::OctetStream
    };
    let insert_zone = filler(pc.insert_zone_len(), seed + 1);
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule,
            upid: Upid::OctetStream,
        },
        insert_zone: &insert_zone,
        ocf: ocf.then(|| Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
    };
    let mut octets = vec![0_u8; MAX_FRAME_LEN];
    let n = encode(&table, fields, content, &filler(tfdz, seed), &mut octets).unwrap();
    octets.truncate(n);
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(validate_frame(&octets, &table, mode).is_ok());
    }
    Frame { table, octets }
}

/// A variable-length frame of exactly `len` octets: the Primary Header with its 2-octet VCF
/// Count (9 octets), the 1-octet TFDF Header, the TFDZ, an OCF if `ocf`, and the FECF.
fn variable(len: usize, ocf: bool, seed: u32) -> Frame {
    let tfdz = len - 9 - 1 - if ocf { 4 } else { 0 } - 2;
    let f = frame(
        channels(TransferFrameType::Variable, MAX_FRAME_LEN),
        tfdz,
        ocf,
        seed,
    );
    assert_eq!(f.octets.len(), len);
    f
}

/// The shortest frame with the FECF (13 octets: one TFDZ octet), a 64-octet frame with an OCF,
/// and a 64-octet fixed-length frame: every single- and double-bit error is gated.
fn short_frames() -> [Frame; 3] {
    // The fixed frame: the header (9), the Insert Zone (4), the TFDF Header (3), the OCF (4),
    // and the FECF (2) leave 42 TFDZ octets.
    let fixed = frame(channels(TransferFrameType::Fixed, 64), 42, true, 5);
    [variable(13, false, 1), variable(64, true, 2), fixed]
}

/// The longest frame within the double-bit guarantee, and the longest frame.
fn long_frames() -> [Frame; 2] {
    [
        variable(CRC16_DOUBLE_BIT_OCTETS, true, 3),
        variable(MAX_FRAME_LEN, false, 4),
    ]
}

/// The gated single- and double-bit errors of the long frames, as for AOS
/// (`aldis-aos/tests/bit_flips.rs`): denser for the 4 095-octet frame than for the 65 536-octet
/// frame, whose decode is 16 times slower.
fn sampled(octets: usize) -> (Vec<usize>, Vec<(usize, usize)>) {
    let n = octets * 8;
    if octets <= CRC16_DOUBLE_BIT_OCTETS {
        (sampled_bits(n, 512, 61), sampled_pairs(n, 32, 997))
    } else {
        (sampled_bits(n, 64, 8191), sampled_pairs(n, 4, 131_072))
    }
}

/// What the receiving end must make of a frame with errors at `bits` (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The Frame Validation Checks reject the frame, and `decode` does too, or on a
    /// variable-length channel reads a shorter frame from a Frame Length error.
    Rejected,
    /// Two errors a multiple of 32 767 bits apart, neither in the Frame Length field: the FECF
    /// passes.
    Undetected,
}

fn expected(bits: &[usize]) -> Outcome {
    match bits {
        [i, j]
            if (j - i) % CRC16_DOUBLE_BIT_BITS == 0
                && !bits.iter().any(|b| FRAME_LENGTH_BITS.contains(b)) =>
        {
            Outcome::Undetected
        }
        _ => Outcome::Rejected,
    }
}

/// Whether `validate_frame` and `decode`, in `mode`, give `bytes`, the frame with errors at
/// `bits`, the expected outcome. An undetected error passes the FECF check; whatever the other
/// checks then make of it is not this property's.
fn as_expected(f: &Frame, bytes: &[u8], bits: &[usize], mode: DecodeMode) -> bool {
    let validated = validate_frame(bytes, &f.table, mode);
    let decoded = UslpFrame::decode(bytes, &f.table, mode);
    match expected(bits) {
        Outcome::Rejected => {
            let in_length = bits.iter().any(|b| FRAME_LENGTH_BITS.contains(b));
            validated.is_err()
                && match decoded {
                    Err(_) => true,
                    Ok(d) => f.variable() && in_length && d.len() < bytes.len(),
                }
        }
        Outcome::Undetected => {
            validated.map_err(|r| r.reason()) != Err(Reason::FecfMismatch)
                && decoded.map_err(|e| e.kind()) != Err(ErrorKind::CrcMismatch)
        }
    }
}

#[test]
fn every_single_bit_error_of_the_short_frames_is_as_expected() {
    // req: USLP-003, USLP-014, CMN-001, VER-023, VER-064
    for f in &short_frames() {
        each_single(&f.octets, |bit, bytes| {
            for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                assert!(
                    as_expected(f, bytes, &[bit], mode),
                    "{} octets, bit {bit}",
                    bytes.len()
                );
            }
        });
    }
}

#[test]
fn every_double_bit_error_of_the_short_frames_is_as_expected() {
    // req: USLP-003, USLP-014, CMN-001, VER-023, VER-064
    for f in &short_frames() {
        each_double(&f.octets, 0..f.octets.len() * 8, |i, j, bytes| {
            for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                let ok = as_expected(f, bytes, &[i, j], mode);
                assert!(ok, "{} octets, bits {i}, {j}, {mode:?}", bytes.len());
            }
        });
    }
}

#[test]
fn sampled_errors_of_the_long_frames_are_as_expected() {
    // req: USLP-003, USLP-014, CMN-001, VER-023, VER-064
    for f in &long_frames() {
        let (bits, pairs) = sampled(f.octets.len());
        each_bit(&f.octets, &bits, |bit, bytes| {
            assert!(
                as_expected(f, bytes, &[bit], DecodeMode::Strict),
                "bit {bit}"
            );
        });
        each_pair(&f.octets, &pairs, |i, j, bytes| {
            let ok = as_expected(f, bytes, &[i, j], DecodeMode::Strict);
            assert!(ok, "{} octets, bits {i}, {j}", bytes.len());
        });
    }
}

#[test]
fn a_double_bit_error_32767_bits_apart_passes_the_fecf() {
    // req: CMN-001, VER-023, VER-064
    // In the longest frame, two TFDZ bits 32 767 apart: the frame passes the Frame Validation
    // Checks with other data. The guarantee ends where the module docs say.
    let [_, longest] = long_frames();
    let (i, j) = (1000, 1000 + CRC16_DOUBLE_BIT_BITS);
    let mut bytes = longest.octets.clone();
    flip(&mut bytes, i);
    flip(&mut bytes, j);
    let sent = validate_frame(&longest.octets, &longest.table, DecodeMode::Strict).unwrap();
    let got = validate_frame(&bytes, &longest.table, DecodeMode::Strict).unwrap();
    assert_eq!(got.len(), sent.len());
    assert_ne!(got.tfdz(), sent.tfdz());
}

#[test]
#[ignore = "exhaustive, 4.7 million checks of up to 65 536 octets: the nightly job bit-flips"]
fn every_single_bit_error_of_the_long_frames_is_as_expected() {
    // req: USLP-003, USLP-014, CMN-001, VER-023, VER-064
    for f in &long_frames() {
        let failure = find_single(&f.octets, |bit, bytes| {
            as_expected(f, bytes, &[bit], DecodeMode::Strict)
        });
        assert_eq!(failure, None, "{} octets", f.octets.len());
    }
}

#[test]
#[ignore = "exhaustive, 537 million checks: the nightly job bit-flips runs it in release mode"]
fn every_double_bit_error_within_the_guarantee_is_as_expected() {
    // req: USLP-003, USLP-014, CMN-001, VER-023, VER-064
    let [f, _] = long_frames();
    let failure = find_double(&f.octets, |i, j, bytes| {
        as_expected(&f, bytes, &[i, j], DecodeMode::Strict)
    });
    assert_eq!(failure, None);
}
