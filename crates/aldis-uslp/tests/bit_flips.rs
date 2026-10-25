//! Bit-flip completeness of the USLP FECF (M2 plan WP-7; 732.1 §4.1.6, Annex B1, §4.3.11.3;
//! USLP-003, USLP-014, CMN-001, VER-023).
//!
//! Every single-bit error and every double-bit error of a set of valid frames with the FECF is
//! rejected by the USLP Frame Validation Checks, [`validate_frame`] (USLP-014), and never
//! accepted as another frame, within the length the CRC-16 guarantees. A truncated frame has no
//! FECF (732.1 Annex D1.2.2 note 1): it is not part of this FECF property, and its own tests
//! (the section on truncated frames below) pin which single-bit errors the structural checks
//! reject. The last section pins the outcome of flag and VCID errors on channels without the
//! FECF.
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

use aldis_core::crc::crc16;
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::bitflip::{
    CRC16_DOUBLE_BIT_BITS, CRC16_DOUBLE_BIT_OCTETS, each_bit, each_double, each_pair, each_single,
    filler, find_double, find_single, flip, sampled_bits, sampled_pairs,
};
use aldis_uslp::delimit::{Delimited, next_frame};
use aldis_uslp::{
    ChannelFields, FirstHeaderPointer, FrameContent, FrameFields, FrameKind, MAX_FRAME_LEN,
    MapConfig, OcfUse, PhysicalChannelConfig, Reason, SecurityScope, SourceOrDestination,
    TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen, Upid, UslpAnomaly, UslpFrame,
    UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen, encode,
    encode_oid, encode_truncated, validate_frame,
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

#[test]
fn decode_may_accept_a_frame_length_error_as_a_shorter_frame() {
    // req: USLP-003, USLP-014, USLP-015, API-011, VER-023, VER-064
    // hazard: HZ-USLP-01 (P-USLP-02). A 64-octet frame whose octets 30 and 31 are the FECF of
    // its first 30 octets with the Frame Length bit of value 32 cleared: with that bit flipped
    // (bit 42), it reads as a valid 32-octet frame followed by other octets.
    let f = variable(64, false, 1);
    let mut octets = f.octets.clone();
    let mut head = octets[..30].to_vec();
    head[5] ^= 0x20;
    octets[30..32].copy_from_slice(&crc16(&head).to_be_bytes());
    let fecf = crc16(&octets[..62]).to_be_bytes();
    octets[62..].copy_from_slice(&fecf);
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(validate_frame(&octets, &f.table, mode).is_ok());
    }
    octets[5] ^= 0x20;
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        let short = UslpFrame::decode(&octets, &f.table, mode).unwrap();
        assert_eq!(short.len(), 32);
        // The delimiter reads the same field, so the unit yields the shorter frame, and
        // validate_frame of those octets accepts it: only a caller that compares the length
        // with the octets it was given sees the error.
        let Some(Delimited::Frame(item)) = next_frame(&octets, &f.table) else {
            panic!("not a frame");
        };
        assert_eq!(item.len(), 32);
        assert!(validate_frame(item, &f.table, mode).is_ok());
        // Given the 64 octets it was sent as, it is rejected.
        let rejection = validate_frame(&octets, &f.table, mode).unwrap_err();
        assert_eq!(rejection.reason(), Reason::LengthMismatch);
    }
}

// ---------------------------------------------------------------------------------------------
// Truncated frames (P-USLP-05, P-USLP-06; OI-48).
// ---------------------------------------------------------------------------------------------

/// One variable-length VC (VC 1, MAP 2) with a truncated frame length of `len` octets, with SDLS
/// if `sdls`, on a Physical Channel with the FECF (which truncated frames lack).
fn truncated_table(len: usize, sdls: bool) -> Channels {
    let pc =
        PhysicalChannelConfig::new(TransferFrameType::Variable, MAX_FRAME_LEN, 0, true).unwrap();
    let scid = UslpSpacecraftId::new(0xA5A5).unwrap();
    let vc_id = UslpVcId::new(1).unwrap();
    let params = VcParams {
        truncated_frame_len: Some(TruncatedFrameLen::new(len).unwrap()),
        security: SecurityScope::Vc(if sdls {
            SecurityLayout::USLP_TRUNCATED
        } else {
            SecurityLayout::NONE
        }),
        ..VcParams::new(TransferFrameType::Variable)
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

const fn truncated_channel() -> ChannelFields {
    ChannelFields {
        spacecraft_id: UslpSpacecraftId::new(0xA5A5).unwrap(),
        source_or_destination: SourceOrDestination::from_bit(false),
        vc_id: UslpVcId::new(1).unwrap(),
        map_id: UslpMapId::new(2).unwrap(),
    }
}

/// A truncated frame of `len` octets on `table` (from [`truncated_table`]) with the TFDZ octets
/// all `fill`, rule `011`.
fn truncated(table: &Channels, len: usize, sdls: bool, fill: u8) -> Vec<u8> {
    let room = len - 4 - 1 - if sdls { 6 + 16 } else { 0 };
    let header = TfdfHeader {
        rule: TfdzRule::OctetStream,
        upid: Upid::OctetStream,
    };
    let mut out = vec![0_u8; 64];
    let n = encode_truncated(
        table,
        truncated_channel(),
        header,
        &vec![fill; room],
        &mut out,
    )
    .unwrap();
    assert_eq!(n, len);
    out.truncate(n);
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(validate_frame(&out, table, mode).is_ok());
    }
    out
}

/// What a single-bit error of a truncated frame gives (732.1 Annex D1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flip {
    Accepted,
    Rejected(Reason),
}

/// The outcome of the error at `bit` of a truncated frame of `len` octets, with SDLS if `sdls`,
/// on the VC of [`truncated_table`], whose TFDF Header holds the rule `011`, in `mode`. The
/// frame's TFDZ is all zero, so that the End of Frame Primary Header Flag error is read as a
/// non-truncated header with clean flags. This is the model of the Blue Book's fields and the
/// order of checks of `docs/standards/uslp-frame.md` §3.1, written from them.
fn expected_flip(bit: usize, len: usize, sdls: bool) -> Flip {
    let tfdf_header_bit = (4 + if sdls { 6 } else { 0 }) * 8;
    match bit {
        // The Transfer Frame Version Number.
        0..4 => Flip::Rejected(Reason::UnexpectedVersion),
        // The Spacecraft ID: the table has one Master Channel.
        4..20 => Flip::Rejected(Reason::UnexpectedSpacecraft),
        // The Source-or-Destination Identifier.
        20 => Flip::Accepted,
        // The VCID: the table has VC 1 only.
        21..27 => Flip::Rejected(Reason::InvalidVcid),
        // The MAP ID: the table has MAP 2 only.
        27..31 => Flip::Rejected(Reason::InvalidMapId),
        // The End of Frame Primary Header Flag: the frame becomes a non-truncated header on a
        // VC that has a truncated frame length (OI-48). A 6-octet input has no room for the
        // 7-octet header.
        31 if len < 7 => Flip::Rejected(Reason::LengthMismatch),
        31 => Flip::Rejected(Reason::NonTruncatedFrameNotAllowed),
        // The construction rule `011` of the TFDF Header: `111` is also a variable-length
        // rule; `001` and `010` have pointers that a truncated frame lacks (USLP-016).
        b if (tfdf_header_bit..tfdf_header_bit + 3).contains(&b) => {
            if b == tfdf_header_bit {
                Flip::Accepted
            } else {
                Flip::Rejected(Reason::ConstructionRuleMismatch)
            }
        }
        // The UPID, the Security Header and Trailer, and the TFDZ: a truncated frame has no
        // field that the receiver checks (Annex D1.2.2 note 1).
        _ => Flip::Accepted,
    }
}

#[test]
fn every_single_bit_error_of_a_truncated_frame_is_as_pinned() {
    // req: USLP-002, USLP-012, USLP-014, USLP-016, CMN-001, VER-023, VER-064
    // hazard: HZ-USLP-04 (P-USLP-06). A truncated frame has no error control: the errors
    // that the receiver rejects are those of the fields it checks, and the others are
    // accepted, in both modes. `decode` agrees with `validate_frame` on acceptance.
    for (len, sdls) in [(6, false), (12, false), (32, false), (28, true)] {
        let table = truncated_table(len, sdls);
        let f = truncated(&table, len, sdls, 0);
        let mut accepted = 0;
        each_single(&f, |bit, bytes| {
            let want = expected_flip(bit, len, sdls);
            for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                let got = match validate_frame(bytes, &table, mode) {
                    Ok(_) => Flip::Accepted,
                    Err(r) => Flip::Rejected(r.reason()),
                };
                assert_eq!(got, want, "{len} octets, sdls {sdls}, bit {bit}, {mode:?}");
                assert_eq!(
                    UslpFrame::decode(bytes, &table, mode).is_ok(),
                    want == Flip::Accepted,
                    "{len} octets, sdls {sdls}, bit {bit}, {mode:?}"
                );
            }
        });
        for bit in 0..len * 8 {
            if expected_flip(bit, len, sdls) == Flip::Accepted {
                accepted += 1;
            }
        }
        // A truncated frame is not protected (Annex D1.2.2 note 1): errors are accepted.
        assert!(accepted > 0, "{len} octets");
    }
}

#[test]
fn a_flipped_end_of_frame_flag_of_a_truncated_frame_is_rejected_whatever_follows() {
    // req: USLP-002, USLP-014, USLP-015, API-011, VER-064
    // hazard: HZ-USLP-02 (P-USLP-05; OI-48). The flag error reads the truncated frame as a
    // non-truncated header on a VC that carries only truncated frames: rejected with reason
    // 18, at any input length (the check precedes the Frame Length), with and without SDLS,
    // and the delimiter does not trust its Frame Length either. Reason 3 or 4 come first
    // when the octet that the flags now occupy has them set (strict mode for the spares).
    for (len, sdls) in [(12, false), (32, false), (28, true)] {
        let table = truncated_table(len, sdls);
        // The flags octet is octet 6: TFDZ octet 1 without SDLS, Security Header octet 3 with.
        let at = 6;
        for (octet, strict, lenient) in [
            (
                0x00,
                Reason::NonTruncatedFrameNotAllowed,
                Reason::NonTruncatedFrameNotAllowed,
            ),
            (
                0x20,
                Reason::ReservedSpare,
                Reason::NonTruncatedFrameNotAllowed,
            ),
            (0x40, Reason::ReservedFlags, Reason::ReservedFlags),
        ] {
            let mut f = truncated(&table, len, sdls, 0);
            f[at] = octet;
            f[3] ^= 0x01;
            // Followed by enough octets for the largest Frame Length that rule bits make.
            for trailing in [0, 40_000] {
                let mut input = f.clone();
                input.resize(len + trailing, 0xA5);
                for (mode, reason) in [(DecodeMode::Strict, strict), (DecodeMode::Lenient, lenient)]
                {
                    let r = validate_frame(&input[..len], &table, mode).unwrap_err();
                    assert_eq!(r.reason(), reason, "{len} octets, sdls {sdls}, {octet:#x}");
                    if trailing != 0 {
                        let e = UslpFrame::decode(&input, &table, mode).unwrap_err();
                        assert_eq!(e.kind(), r.error().kind());
                    }
                }
            }
            if octet == 0x00 {
                // Octets 4 and 5 are now the Frame Length: the TFDF Header and the first TFDZ
                // octet (a length far above the frame) without SDLS, which the delimiter does
                // not trust (OI-48); two zero Security Header octets (a length of 1, below the
                // shortest frame) with SDLS, which it takes for fill. Both discard the rest.
                let item = next_frame(&f, &table).unwrap();
                if sdls {
                    assert!(matches!(item, Delimited::Fill(_)));
                } else {
                    assert!(matches!(item, Delimited::NonTruncatedNotAllowed(_)));
                }
                assert_eq!(item.len(), len);
            }
        }
    }
}

#[test]
fn a_flipped_end_of_frame_flag_of_a_non_truncated_frame_is_rejected() {
    // req: USLP-002, USLP-014, CMN-001, VER-064
    // hazard: HZ-USLP-02 (P-USLP-05). The reverse: a frame on a VC without a truncated frame
    // length read as truncated is rejected with reason 7 (Annex D1.2.1), in every frame of
    // the suite, fixed-length ones included.
    for f in &short_frames() {
        let mut bytes = f.octets.clone();
        bytes[3] ^= 0x01;
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            let r = validate_frame(&bytes, &f.table, mode).unwrap_err();
            assert_eq!(r.reason(), Reason::TruncatedFrameNotAllowed);
            assert!(UslpFrame::decode(&bytes, &f.table, mode).is_err());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Channels without the FECF (P-USLP-10; HZ-USLP-08, HZ-USLP-09).
// ---------------------------------------------------------------------------------------------

/// A variable-length Physical Channel without the FECF: VC 1 with MAP 2, equal VCF Count
/// Lengths (2 octets) for both qualities of service, and an OCF allowed.
fn no_fecf_variable() -> Channels {
    let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 64, 0, false).unwrap();
    let scid = UslpSpacecraftId::new(0xA5A5).unwrap();
    let vc_id = UslpVcId::new(1).unwrap();
    let two = VcfCountLen::new(2).unwrap();
    let params = VcParams {
        vcf_count_len_sequence: two,
        vcf_count_len_expedited: two,
        ocf: OcfUse::Allowed,
        ..VcParams::new(TransferFrameType::Variable)
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

/// A frame of `kind` on [`no_fecf_variable`] with a 20-octet TFDZ and rule `111` (which fits
/// both user data and protocol control frames), and an OCF if `ocf`.
fn no_fecf_frame(table: &Channels, kind: FrameKind, ocf: bool) -> Vec<u8> {
    let fields = FrameFields {
        spacecraft_id: UslpSpacecraftId::new(0xA5A5).unwrap(),
        source_or_destination: SourceOrDestination::from_bit(false),
        vc_id: UslpVcId::new(1).unwrap(),
        map_id: UslpMapId::new(2).unwrap(),
        kind,
        vcf_count: 9,
    };
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid: Upid::OctetStream,
        },
        insert_zone: &[],
        ocf: ocf.then(|| Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
    };
    let mut out = vec![0_u8; 64];
    let n = encode(table, fields, content, &filler(20, 7), &mut out).unwrap();
    out.truncate(n);
    out
}

/// `frame` with the bit `bit` (0 = most significant bit of octet 0) flipped.
fn flipped(frame: &[u8], bit: usize) -> Vec<u8> {
    let mut b = frame.to_vec();
    flip(&mut b, bit);
    b
}

#[test]
fn without_the_fecf_most_flag_errors_are_accepted() {
    // req: USLP-001, USLP-004, USLP-013, USLP-014, VER-064
    // hazard: HZ-USLP-09 (P-USLP-10). Source-or-Destination Identifier (bit 20), Bypass/Sequence
    // Control Flag (48), Protocol Control Command Flag (49), OCF Flag (52). Pinned in both
    // modes: accepted with another meaning, which the assertions state, but for the flags `01`
    // (reason 3).
    let table = no_fecf_variable();
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        for kind in [FrameKind::SequenceControlled, FrameKind::Expedited] {
            for ocf in [false, true] {
                let octets = no_fecf_frame(&table, kind, ocf);
                let sent = validate_frame(&octets, &table, mode).unwrap();
                // The identifier is read as the other end's.
                let b = flipped(&octets, 20);
                let got = validate_frame(&b, &table, mode).unwrap();
                assert_ne!(got.source_or_destination(), sent.source_or_destination());
                // The Bypass flag: the quality of service changes (equal count lengths).
                let b = flipped(&octets, 48);
                let got = validate_frame(&b, &table, mode).unwrap();
                let other = if kind == FrameKind::Expedited {
                    FrameKind::SequenceControlled
                } else {
                    FrameKind::Expedited
                };
                assert_eq!(got.full().unwrap().kind(), other);
                // The OCF Flag: the last four TFDZ octets become the OCF, or the reverse.
                let b = flipped(&octets, 52);
                let got = validate_frame(&b, &table, mode).unwrap();
                let got = got.full().unwrap();
                assert_eq!(got.ocf().is_some(), !ocf);
                assert_eq!(got.tfdz().len(), if ocf { 24 } else { 16 });
            }
        }
        // The Protocol Control Command Flag: an expedited frame becomes protocol control,
        // accepted; a sequence-controlled one becomes the reserved `01`, reason 3.
        let sent = no_fecf_frame(&table, FrameKind::Expedited, false);
        let b = flipped(&sent, 49);
        let got = validate_frame(&b, &table, mode).unwrap();
        assert_eq!(got.full().unwrap().kind(), FrameKind::ProtocolControl);
        let sent = no_fecf_frame(&table, FrameKind::SequenceControlled, false);
        let b = flipped(&sent, 49);
        let r = validate_frame(&b, &table, mode).unwrap_err();
        assert_eq!(r.reason(), Reason::ReservedFlags);
        // A protocol control frame whose Bypass flag flips is `01`: reason 3.
        let sent = no_fecf_frame(&table, FrameKind::ProtocolControl, false);
        let b = flipped(&sent, 48);
        let r = validate_frame(&b, &table, mode).unwrap_err();
        assert_eq!(r.reason(), Reason::ReservedFlags);
        assert!(validate_frame(&flipped(&sent, 49), &table, mode).is_ok());
    }
}

/// A fixed-length Physical Channel of 64-octet frames without the FECF: VC 31 with MAP 0 and a
/// fixed-length TFDZ. VCID 63 is valid without an entry, and VC 31 is one bit from it.
fn no_fecf_fixed() -> Channels {
    let pc = PhysicalChannelConfig::new(TransferFrameType::Fixed, 64, 0, false).unwrap();
    let scid = UslpSpacecraftId::new(0xA5A5).unwrap();
    let vc_id = UslpVcId::new(31).unwrap();
    let vc = VcConfig::new(scid, vc_id, VcParams::new(TransferFrameType::Fixed)).unwrap();
    let map = MapConfig::new(
        scid,
        vc_id,
        UslpMapId::new(0).unwrap(),
        SecurityLayout::NONE,
    );
    Channels::new(pc, &[vc], &[map]).unwrap()
}

const fn no_fecf_fields(vc: u8) -> FrameFields {
    FrameFields {
        spacecraft_id: UslpSpacecraftId::new(0xA5A5).unwrap(),
        source_or_destination: SourceOrDestination::from_bit(false),
        vc_id: UslpVcId::new(vc).unwrap(),
        map_id: UslpMapId::new(0).unwrap(),
        kind: FrameKind::Expedited,
        vcf_count: 0,
    }
}

#[test]
fn without_the_fecf_a_vcid_error_between_a_data_frame_and_an_oid_frame_is_pinned() {
    // req: USLP-009, USLP-014, USLP-016, API-031, API-032, VER-064
    // hazard: HZ-USLP-08 (P-USLP-10). Every single-bit error of the VCID (bits 21–26) of a
    // frame of VC 31 and of an OID frame (VC 63) on a fixed-length channel without the FECF:
    // the one error between the two is accepted as the other, and the others name no VC.
    let table = no_fecf_fixed();
    let mut data = vec![0_u8; 64];
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(0)),
            upid: Upid::Packets,
        },
        insert_zone: &[],
        ocf: None,
    };
    let n = encode(
        &table,
        no_fecf_fields(31),
        content,
        &filler(54, 3),
        &mut data,
    )
    .unwrap();
    assert_eq!(n, 64);
    let mut oid = vec![0_u8; 64];
    let n = encode_oid(
        &table,
        no_fecf_fields(63),
        &[],
        None,
        &mut OidGenerator::new(),
        &mut oid,
    )
    .unwrap();
    assert_eq!(n, 64);
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(validate_frame(&data, &table, mode).is_ok());
        assert!(validate_frame(&oid, &table, mode).is_ok());
    }
    for bit in 21..27 {
        // Only the most significant bit of the VCID (bit 21) leads from 31 (011111) to 63
        // (111111) and back.
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            let (d, o) = (flipped(&data, bit), flipped(&oid, bit));
            let got = validate_frame(&d, &table, mode);
            let from_oid = validate_frame(&o, &table, mode);
            if bit == 21 {
                // A data frame read as an OID frame: its rule, UPID, or pointer deviate.
                // Strict mode rejects it (reason 17); lenient mode accepts it and reports the
                // anomaly.
                match mode {
                    DecodeMode::Strict => {
                        assert_eq!(got.unwrap_err().reason(), Reason::OidHeaderMismatch);
                    }
                    DecodeMode::Lenient => {
                        let got = got.unwrap();
                        assert_eq!(got.vc_id().get(), 63);
                        assert!(got.anomalies().contains(UslpAnomaly::OidHeaderMismatch));
                    }
                }
                // An OID frame read as a data frame of VC 31: accepted in both modes with no
                // anomaly, and its UPID is "Only Idle Data".
                let sent = from_oid.unwrap();
                assert_eq!(sent.vc_id().get(), 31);
                assert!(sent.anomalies().is_empty());
                assert_eq!(sent.tfdf_header().upid, Upid::OnlyIdleData);
            } else {
                assert_eq!(got.unwrap_err().reason(), Reason::InvalidVcid, "bit {bit}");
                assert_eq!(
                    from_oid.unwrap_err().reason(),
                    Reason::InvalidVcid,
                    "bit {bit}"
                );
            }
        }
    }
}
