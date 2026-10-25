//! Bit-flip completeness of the TM FECF (M2 plan WP-7; 132.0 §4.1.6; TM-005, CMN-001, VER-023).
//!
//! Every single-bit error and every double-bit error of a set of valid frames with the FECF is
//! rejected. The decoder checks the FECF before it reads any field (TM-005), so the error is
//! always `CrcMismatch`, in both modes, and no error is ever decoded as another frame. A TM
//! frame is at most 2 048 octets (131.0 §11.3), within the 4 095 octets up to which the CRC-16
//! detects every double-bit error ([`aldis_testkit::bitflip`]), so the double-bit property holds
//! for every TM frame.
//!
//! The frames are fixed, not drawn: the shortest frame with the FECF, a frame with a Secondary
//! Header and an OCF, a 256-octet frame, and a frame of the longest length.
//!
//! - Gated: every single-bit error of each frame; every double-bit error of the 9- and 64-octet
//!   frames; the representative double-bit errors of `sampled_pairs` for the other two.
//! - Nightly (`#[ignore]`, the job `bit-flips` in `.github/workflows/nightly.yml`, release mode):
//!   every double-bit error of the longest frame, 134 million decodes.
#![expect(clippy::unwrap_used, reason = "test fixtures fail loudly (RS-018)")]

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::bitflip::{
    CRC16_DOUBLE_BIT_OCTETS, each_double, each_pair, each_single, filler, find_double,
    sampled_pairs,
};
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FshLen, PacketOffset, PacketPointer, TmFrame,
    TmFrameContent, TmFrameFields, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig, encode,
};

/// The longest TM Transfer Frame: 2 048 octets (131.0 §11.3, set by the 11-bit First Header
/// Pointer).
const LONGEST: usize = 2048;

// Every TM frame lies within the double-bit guarantee.
const _: () = assert!(LONGEST <= CRC16_DOUBLE_BIT_OCTETS);

/// A valid frame and the master channel it was encoded on.
struct Frame {
    mc: TmMasterChannelConfig,
    octets: Vec<u8>,
}

/// A frame of `len` octets with the FECF, on VC 5 of SCID 0x2A5, with a 4-octet Secondary
/// Header and an OCF if `extras`, and data from `seed`.
fn frame(len: usize, extras: bool, seed: u32) -> Frame {
    let vcid = VcId::<3>::new(5).unwrap();
    let pc = TmPhysicalChannelConfig::new(len, true).unwrap();
    let mut vc = TmVirtualChannelConfig::new(vcid, DataFieldContent::Packets);
    if extras {
        vc = vc.with_fsh(FshLen::new(4).unwrap()).with_ocf();
    }
    let scid = SpacecraftId::<10>::new(0x2A5).unwrap();
    let mc = TmMasterChannelConfig::new(pc, scid, None, false)
        .with_vc(vc)
        .unwrap();
    let layout = mc.frame_layout(vcid).unwrap().clone();
    let data = filler(layout.data().len(), seed);
    let fsh = filler(
        layout.secondary().len().saturating_sub(1),
        seed.wrapping_add(1),
    );
    let pointer = PacketPointer::StartsAt(PacketOffset::new(0).unwrap());
    let fields = TmFrameFields::new(vcid, 0xA5, 0x5A, DataFieldStatus::Packets(pointer));
    let content = TmFrameContent {
        secondary_header_data: &fsh,
        security_header: &[],
        data: &data,
        security_trailer: &[],
        ocf: extras.then(|| Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
    };
    let mut octets = vec![0_u8; len];
    assert_eq!(encode(&mc, &fields, &content, &mut octets), Ok(len));
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        assert!(TmFrame::decode(&octets, mc.pc(), mode).is_ok());
    }
    Frame { mc, octets }
}

/// Whether `bytes`, a valid frame with bit errors, is rejected with `CrcMismatch` (TM-005).
fn rejected(f: &Frame, bytes: &[u8], mode: DecodeMode) -> bool {
    TmFrame::decode(bytes, f.mc.pc(), mode).map_err(|e| e.kind()) == Err(ErrorKind::CrcMismatch)
}

/// The shortest frame with the FECF (9 octets: the header, one data octet, the FECF) and a
/// 64-octet frame with a Secondary Header and an OCF: every double-bit error is gated.
fn short_frames() -> [Frame; 2] {
    [frame(9, false, 1), frame(64, true, 2)]
}

/// A 256-octet frame and the longest frame, with a Secondary Header and an OCF.
fn long_frames() -> [Frame; 2] {
    [frame(256, false, 3), frame(LONGEST, true, 4)]
}

#[test]
fn every_single_bit_error_is_rejected() {
    // req: TM-005, CMN-001, VER-023
    for f in short_frames().iter().chain(&long_frames()) {
        each_single(&f.octets, |bit, bytes| {
            assert!(
                rejected(f, bytes, DecodeMode::Strict),
                "{} octets, bit {bit}",
                bytes.len()
            );
        });
    }
    for f in &short_frames() {
        each_single(&f.octets, |bit, bytes| {
            assert!(
                rejected(f, bytes, DecodeMode::Lenient),
                "{} octets, bit {bit}",
                bytes.len()
            );
        });
    }
}

#[test]
fn every_double_bit_error_of_the_short_frames_is_rejected() {
    // req: TM-005, CMN-001, VER-023
    for f in &short_frames() {
        let bits = f.octets.len() * 8;
        each_double(&f.octets, 0..bits, |i, j, bytes| {
            for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                assert!(
                    rejected(f, bytes, mode),
                    "{} octets, bits {i}, {j}",
                    bytes.len()
                );
            }
        });
    }
}

#[test]
fn sampled_double_bit_errors_of_the_long_frames_are_rejected() {
    // req: TM-005, CMN-001, VER-023
    for f in &long_frames() {
        each_pair(
            &f.octets,
            &sampled_pairs(f.octets.len() * 8, 32, 97),
            |i, j, bytes| {
                assert!(
                    rejected(f, bytes, DecodeMode::Strict),
                    "{} octets, bits {i}, {j}",
                    bytes.len()
                );
            },
        );
    }
}

#[test]
#[ignore = "exhaustive, 134 million decodes: the nightly job bit-flips runs it in release mode"]
fn every_double_bit_error_of_the_longest_frame_is_rejected() {
    // req: TM-005, CMN-001, VER-023
    let [_, f] = long_frames();
    let failure = find_double(&f.octets, |_, _, bytes| {
        rejected(&f, bytes, DecodeMode::Strict)
    });
    assert_eq!(failure, None);
}
