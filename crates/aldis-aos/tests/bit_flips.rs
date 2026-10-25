//! Bit-flip completeness of the AOS FECF and FHEC (M2 plan WP-7; 732.0 §4.1.2.6, §4.1.6,
//! §4.3.7.2–3; AOS-013, AOS-014, CMN-001, VER-023).
//!
//! The receiving end corrects the Primary Header with the FHEC first and then checks the FECF
//! over the frame with the corrected header (§4.3.7.2–3, AOS-014). So the right outcome of an
//! error depends on where its bits are:
//!
//! - **In the FHEC's symbols** (header bits 0–15 and 40–63, AOS-014), with the FHEC configured:
//!   the Reed-Solomon code corrects up to two symbol errors (AOS-013), and one or two bit errors
//!   touch at most two symbols. The frame is **corrected**: it decodes to exactly the frame
//!   sent, with `corrected_symbols()` 1 or 2.
//! - **Anywhere else** (every bit without the FHEC; the VC Frame Count, bits 16–39, and the rest
//!   of the frame with it): the error reaches the FECF, and the frame is **rejected** with
//!   `CrcMismatch`, in both modes. Bit errors in the FHEC's symbols beside it are corrected
//!   first and do not change this.
//! - **Undetected**: the CRC-16 detects every double-bit error only in a frame of at most 4 095
//!   octets ([`aldis_testkit::bitflip`]). AOS frames may be up to 65 536 octets (131.0 §11.3);
//!   in a longer frame two errors outside the FHEC's symbols that are 32 767 bits apart pass the
//!   FECF. `a_double_bit_error_32767_bits_apart_passes_the_fecf` shows one decoded as a
//!   different frame; the other double-bit errors of such a frame are still rejected.
//!
//! The frames are fixed, not drawn: the shortest frame with the FECF and no FHEC, a 64-octet
//! frame with the FHEC, an Insert Zone, and an OCF, a frame of 4 095 octets (the longest the
//! double-bit guarantee covers) with the FHEC, and a frame of the longest length, 65 536 octets.
//!
//! - Gated: every single-bit and double-bit error of the two short frames; for the two long
//!   ones, the representative single-bit errors of `sampled_bits` and double-bit errors of
//!   `sampled_pairs` (`sampled` below).
//! - Nightly (`#[ignore]`, the job `bit-flips` in `.github/workflows/nightly.yml`, release mode):
//!   every single-bit error of both long frames, and every double-bit error of the 4 095-octet
//!   frame (537 million decodes).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly, and bit and octet bookkeeping on lengths the frames bound (RS-018)"
)]

use aldis_aos::{
    AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig,
    AosVirtualChannelConfig, CorrectedSymbols, DataFieldContent, PacketOffset, PacketPointer,
    PduHeader, VcFrameCount, VcTable, encode,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::bitflip::{
    CRC16_DOUBLE_BIT_BITS, CRC16_DOUBLE_BIT_OCTETS, each_bit, each_double, each_pair, each_single,
    filler, find_double, find_single, flip, sampled_bits, sampled_pairs,
};

/// The longest AOS Transfer Frame: 65 536 octets (131.0 §11.3, set by the 16-bit First Header
/// Pointer).
const LONGEST: usize = 65_536;

/// The channels: one VC on the physical channel.
type Table = VcTable<1>;

/// A valid frame and its channels.
struct Frame {
    table: Table,
    octets: Vec<u8>,
}

impl Frame {
    const fn pc(&self) -> AosPhysicalChannelConfig {
        self.table.physical_channel()
    }

    fn decode<'a>(&self, bytes: &'a [u8], mode: DecodeMode) -> aldis_core::Result<AosFrame<'a>> {
        AosFrame::decode(bytes, &self.pc(), &self.table, mode)
    }

    /// Whether bit `bit` is in one of the FHEC's symbols: header bits 0–15 and 40–63, when the
    /// FHEC is configured (AOS-014).
    fn in_fhec(&self, bit: usize) -> bool {
        self.pc().fhec() && (bit < 16 || (40..64).contains(&bit))
    }
}

/// A frame of `len` octets with the FECF on VC 9 of SCID 0x2A5 (Issue 5): with the FHEC, a
/// 4-octet Insert Zone, and an OCF on an `M_PDU` VC if `extras`, else a `VCA_SDU` VC; data from
/// `seed`.
fn frame(len: usize, extras: bool, seed: u32) -> Frame {
    let scid = SpacecraftId::<10>::new(0x2A5).unwrap();
    let vcid = VcId::<6>::new(9).unwrap();
    let iz = if extras { 4 } else { 0 };
    let pc = AosPhysicalChannelConfig::new(len, extras, iz, true, AosEdition::Issue5).unwrap();
    let content = if extras {
        DataFieldContent::Mpdu
    } else {
        DataFieldContent::VcaSdu
    };
    let mut vc = AosVirtualChannelConfig::new(scid, vcid, content).unwrap();
    if extras {
        vc = vc.with_ocf();
    }
    let table = Table::new(pc, &[vc]).unwrap();
    let layout = table.frame_layout(scid, vcid).unwrap();
    // The M_PDU Header takes 2 octets of the data field.
    let zone = filler(layout.data().len() - if extras { 2 } else { 0 }, seed);
    let insert_zone = filler(iz, seed + 1);
    let pdu = if extras {
        PduHeader::Mpdu(PacketPointer::StartsAt(PacketOffset::new(0).unwrap()))
    } else {
        PduHeader::None
    };
    let fields = AosFrameFields {
        scid,
        vcid,
        vc_frame_count: VcFrameCount::new(0x00A5_5A0F).unwrap(),
        replay_flag: false,
        cycle: None,
        pdu,
    };
    let content = AosFrameContent {
        insert_zone: &insert_zone,
        security_header: &[],
        zone: &zone,
        security_trailer: &[],
        ocf: extras.then(|| Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
    };
    let mut octets = vec![0_u8; len];
    assert_eq!(encode(&pc, &table, &fields, &content, &mut octets), Ok(len));
    let f = Frame { table, octets };
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(f.decode(&f.octets, mode).is_ok());
    }
    f
}

/// What the receiving end must make of a frame with errors at `bits` (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The FHEC corrects every error: the frame sent.
    Corrected,
    /// The FECF detects the errors left after the FHEC.
    Rejected,
    /// Two errors left after the FHEC lie a multiple of 32 767 bits apart: the FECF passes.
    Undetected,
}

fn expected(f: &Frame, bits: &[usize]) -> Outcome {
    let left: Vec<usize> = bits.iter().copied().filter(|&b| !f.in_fhec(b)).collect();
    match left.as_slice() {
        [] => Outcome::Corrected,
        [i, j] if (j - i) % CRC16_DOUBLE_BIT_BITS == 0 => Outcome::Undetected,
        _ => Outcome::Rejected,
    }
}

/// Whether decoding `bytes`, the frame with errors at `bits`, in `mode` has the expected
/// outcome. An undetected error passes the FECF; whatever the decoder makes of it then is not
/// this property's.
fn as_expected(f: &Frame, bytes: &[u8], bits: &[usize], mode: DecodeMode) -> bool {
    let got = f.decode(bytes, mode);
    match expected(f, bits) {
        Outcome::Corrected => got.is_ok_and(|g| {
            let sent = f.decode(&f.octets, mode).unwrap();
            let symbols = if bits.len() == 1 || bits[0] / 4 == bits[1] / 4 {
                CorrectedSymbols::One
            } else {
                CorrectedSymbols::Two
            };
            g.header().to_octets() == sent.header().to_octets()
                && g.data_unit() == sent.data_unit()
                && (g.insert_zone(), g.ocf(), g.fecf())
                    == (sent.insert_zone(), sent.ocf(), sent.fecf())
                && g.corrected_symbols() == symbols
        }),
        Outcome::Rejected => got.map_err(|e| e.kind()) == Err(ErrorKind::CrcMismatch),
        Outcome::Undetected => got.map_err(|e| e.kind()) != Err(ErrorKind::CrcMismatch),
    }
}

/// The shortest frame with the FECF and no FHEC (9 octets) and a 64-octet frame with the FHEC:
/// every single- and double-bit error is gated.
fn short_frames() -> [Frame; 2] {
    [frame(9, false, 1), frame(64, true, 2)]
}

/// The longest frame within the double-bit guarantee, and the longest frame, both with the FHEC.
fn long_frames() -> [Frame; 2] {
    [
        frame(CRC16_DOUBLE_BIT_OCTETS, true, 3),
        frame(LONGEST, true, 4),
    ]
}

/// The gated single- and double-bit errors of the long frames: for the 4 095-octet frame the
/// first and last 512 bits, every 61st bit, and `sampled_pairs` with 32 near bits and a stride of
/// 997; for the 65 536-octet frame, whose decode is 16 times slower, the first and last 64 bits,
/// every 8 191st bit, and `sampled_pairs` with 4 near bits and a stride of 131 072.
fn sampled(octets: usize) -> (Vec<usize>, Vec<(usize, usize)>) {
    let n = octets * 8;
    if octets <= CRC16_DOUBLE_BIT_OCTETS {
        (sampled_bits(n, 512, 61), sampled_pairs(n, 32, 997))
    } else {
        (sampled_bits(n, 64, 8191), sampled_pairs(n, 4, 131_072))
    }
}

#[test]
fn every_single_bit_error_of_the_short_frames_is_corrected_or_rejected() {
    // req: AOS-013, AOS-014, CMN-001, VER-023, VER-064
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
fn every_double_bit_error_of_the_short_frames_is_corrected_or_rejected() {
    // req: AOS-013, AOS-014, CMN-001, VER-023, VER-064
    for f in &short_frames() {
        each_double(&f.octets, 0..f.octets.len() * 8, |i, j, bytes| {
            for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                assert!(
                    as_expected(f, bytes, &[i, j], mode),
                    "{} octets, bits {i}, {j}",
                    bytes.len()
                );
            }
        });
    }
}

#[test]
fn sampled_errors_of_the_long_frames_are_corrected_or_rejected() {
    // req: AOS-013, AOS-014, CMN-001, VER-023, VER-064
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
    // Within 4 095 octets no two bits are 32 767 apart; in the longest frame, two data bits
    // are, and the frame decodes with other data: the guarantee ends where the module docs say.
    let [within, longest] = long_frames();
    assert!(within.octets.len() * 8 <= CRC16_DOUBLE_BIT_BITS);
    let (i, j) = (1000, 1000 + CRC16_DOUBLE_BIT_BITS);
    let mut bytes = longest.octets.clone();
    flip(&mut bytes, i);
    flip(&mut bytes, j);
    let sent = longest.decode(&longest.octets, DecodeMode::Strict).unwrap();
    let got = longest.decode(&bytes, DecodeMode::Strict).unwrap();
    assert_ne!(got.data_unit(), sent.data_unit());
}

#[test]
#[ignore = "exhaustive, 4.7 million decodes of up to 65 536 octets: the nightly job bit-flips"]
fn every_single_bit_error_of_the_long_frames_is_corrected_or_rejected() {
    // req: AOS-013, AOS-014, CMN-001, VER-023, VER-064
    for f in &long_frames() {
        let failure = find_single(&f.octets, |bit, bytes| {
            as_expected(f, bytes, &[bit], DecodeMode::Strict)
        });
        assert_eq!(failure, None, "{} octets", f.octets.len());
    }
}

#[test]
#[ignore = "exhaustive, 537 million decodes: the nightly job bit-flips runs it in release mode"]
fn every_double_bit_error_within_the_guarantee_is_corrected_or_rejected() {
    // req: AOS-013, AOS-014, CMN-001, VER-023, VER-064
    let [f, _] = long_frames();
    let failure = find_double(&f.octets, |i, j, bytes| {
        as_expected(&f, bytes, &[i, j], DecodeMode::Strict)
    });
    assert_eq!(failure, None);
}
