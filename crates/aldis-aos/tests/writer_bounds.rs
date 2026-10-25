//! The AOS writers write inside the frame and nowhere else (732.0 §4.1; API-022, API-023,
//! GEN-014, GEN-020; the same property as `aldis-tc/tests/writer_bounds.rs`, hazard HZ-TC-16,
//! proposal P-TC-06).
//!
//! The buffer is filled with a pattern and is longer than the frame. After `encode`, and after
//! `AosFrameWriter::begin` and `finish`, every octet from the end of the frame on is the
//! pattern; a rejected `encode` or `begin` changes no octet.
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly (RS-018)"
)]

use aldis_aos::{
    AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosFrameWriter,
    AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent, PacketOffset,
    PacketPointer, PduHeader, VcFrameCount, VcTable, encode,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::{DecodeMode, ErrorKind};

const FRAME: usize = 64;
const BUFFER: usize = 200;

fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from((i * 37 + 11) % 251).unwrap())
        .collect()
}

/// A 64-octet frame with the FECF, an Insert Zone of 4 octets, and an OCF, on an `M_PDU` VC.
fn channel() -> (AosPhysicalChannelConfig, VcTable<1>, AosFrameFields, usize) {
    let scid = SpacecraftId::<10>::new(0x2A5).unwrap();
    let vcid = VcId::<6>::new(9).unwrap();
    let pc = AosPhysicalChannelConfig::new(FRAME, true, 4, true, AosEdition::Issue5).unwrap();
    let vc = AosVirtualChannelConfig::new(scid, vcid, DataFieldContent::Mpdu)
        .unwrap()
        .with_ocf();
    let table = VcTable::<1>::new(pc, &[vc]).unwrap();
    let zone = table.frame_layout(scid, vcid).unwrap().data().len() - 2;
    let fields = AosFrameFields {
        scid,
        vcid,
        vc_frame_count: VcFrameCount::new(0x00A5_5A0F).unwrap(),
        replay_flag: false,
        cycle: None,
        pdu: PduHeader::Mpdu(PacketPointer::StartsAt(PacketOffset::new(0).unwrap())),
    };
    (pc, table, fields, zone)
}

#[test]
fn encode_writes_only_the_frame() {
    // req: API-022, GEN-014, GEN-020, AOS-019
    let (pc, table, fields, zone_len) = channel();
    let (zone, insert_zone) = (vec![0x33_u8; zone_len], [0x44_u8; 4]);
    let content = AosFrameContent {
        insert_zone: &insert_zone,
        security_header: &[],
        zone: &zone,
        security_trailer: &[],
        ocf: Some(Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
    };
    let before = pattern(BUFFER);
    let mut out = before.clone();
    assert_eq!(encode(&pc, &table, &fields, &content, &mut out), Ok(FRAME));
    assert_eq!(out[FRAME..], before[FRAME..]);
    assert!(AosFrame::decode(&out[..FRAME], &pc, &table, DecodeMode::Strict).is_ok());
    // One octet short, and a zone of the wrong length: nothing is written.
    let mut short = before[..FRAME - 1].to_vec();
    let e = encode(&pc, &table, &fields, &content, &mut short).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(short, before[..FRAME - 1]);
    let wrong = AosFrameContent {
        zone: &zone[1..],
        ..content
    };
    let mut out = before.clone();
    let e = encode(&pc, &table, &fields, &wrong, &mut out).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::LengthMismatch);
    assert_eq!(out, before);
}

#[test]
fn begin_and_finish_write_only_the_frame() {
    // req: API-020, API-022, API-023, GEN-014, GEN-020
    let (pc, table, fields, _) = channel();
    let before = pattern(BUFFER);
    let mut out = before.clone();
    let w = AosFrameWriter::begin(&mut out, &pc, &table, &fields).unwrap();
    assert_eq!(w.finish(), FRAME);
    assert_eq!(out[FRAME..], before[FRAME..]);
    // A rejected `begin` changes nothing.
    let mut short = before[..FRAME - 1].to_vec();
    let e = AosFrameWriter::begin(&mut short, &pc, &table, &fields).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(short, before[..FRAME - 1]);
}
