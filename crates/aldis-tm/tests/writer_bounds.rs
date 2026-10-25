//! The TM writers write inside the frame and nowhere else (132.0 §4.1; API-022, API-023,
//! GEN-014, GEN-020; the same property as `aldis-tc/tests/writer_bounds.rs`, hazard HZ-TC-16,
//! proposal P-TC-06).
//!
//! The buffer is filled with a pattern and is longer than the frame. After `encode`, and after
//! `TmFrameWriter::begin` and `finish`, every octet from the end of the frame on is the pattern;
//! a rejected `encode` or `begin` changes no octet.
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly (RS-018)"
)]

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FshLen, PacketOffset, PacketPointer, TmFrame,
    TmFrameContent, TmFrameFields, TmFrameWriter, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig, encode,
};

const FRAME: usize = 64;
const BUFFER: usize = 200;

fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from((i * 37 + 11) % 251).unwrap())
        .collect()
}

fn channel() -> (TmMasterChannelConfig, TmFrameFields) {
    let vcid = VcId::<3>::new(5).unwrap();
    let pc = TmPhysicalChannelConfig::new(FRAME, true).unwrap();
    let vc = TmVirtualChannelConfig::new(vcid, DataFieldContent::Packets)
        .with_fsh(FshLen::new(4).unwrap())
        .with_ocf();
    let scid = SpacecraftId::<10>::new(0x2A5).unwrap();
    let mc = TmMasterChannelConfig::new(pc, scid, None, false)
        .with_vc(vc)
        .unwrap();
    let pointer = PacketPointer::StartsAt(PacketOffset::new(0).unwrap());
    let fields = TmFrameFields::new(vcid, 0xA5, 0x5A, DataFieldStatus::Packets(pointer));
    (mc, fields)
}

#[test]
fn encode_writes_only_the_frame() {
    // req: API-022, GEN-014, GEN-020, TM-009
    let (mc, fields) = channel();
    let layout = mc.frame_layout(fields.vcid).unwrap().clone();
    let data = vec![0x33_u8; layout.data().len()];
    let fsh = vec![0x44_u8; layout.secondary().len() - 1];
    let content = TmFrameContent {
        secondary_header_data: &fsh,
        security_header: &[],
        data: &data,
        security_trailer: &[],
        ocf: Some(aldis_core::ocf::Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
    };
    let before = pattern(BUFFER);
    let mut out = before.clone();
    assert_eq!(encode(&mc, &fields, &content, &mut out), Ok(FRAME));
    assert_eq!(out[FRAME..], before[FRAME..]);
    assert!(TmFrame::decode(&out[..FRAME], mc.pc(), DecodeMode::Strict).is_ok());
    // One octet short, and a data field of the wrong length: nothing is written.
    let mut short = before[..FRAME - 1].to_vec();
    let e = encode(&mc, &fields, &content, &mut short).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(short, before[..FRAME - 1]);
    let wrong = TmFrameContent {
        data: &data[1..],
        ..content
    };
    let mut out = before.clone();
    let e = encode(&mc, &fields, &wrong, &mut out).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::LengthMismatch);
    assert_eq!(out, before);
}

#[test]
fn begin_and_finish_write_only_the_frame() {
    // req: API-020, API-022, API-023, GEN-014, GEN-020
    let (mc, fields) = channel();
    let before = pattern(BUFFER);
    let mut out = before.clone();
    let w = TmFrameWriter::begin(&mut out, &mc, &fields).unwrap();
    assert_eq!(w.finish(), FRAME);
    assert_eq!(out[FRAME..], before[FRAME..]);
    // A rejected `begin` changes nothing.
    let mut short = before[..FRAME - 1].to_vec();
    let e = TmFrameWriter::begin(&mut short, &mc, &fields).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(short, before[..FRAME - 1]);
}
