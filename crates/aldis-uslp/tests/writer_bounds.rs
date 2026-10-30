//! The USLP writers write inside the frame or reservation and nowhere else (732.1 §4.1;
//! API-022, API-023, GEN-014, GEN-020; the same property as `aldis-tc/tests/writer_bounds.rs`,
//! hazard HZ-TC-16, proposal P-TC-06).
//!
//! The buffer is filled with a pattern and is longer than the frame. After `encode`, and after
//! `UslpFrameWriter::begin` and `finish` (also below the reservation, with and without a
//! Security Trailer, which `finish` moves), every octet from the end of the reservation on is
//! the pattern; a rejected `encode` or `begin`, and a failed `finish`, change no octet of the
//! frame or the reservation.
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test fixtures fail loudly, and bookkeeping on lengths the fixtures bound (RS-018)"
)]

use aldis_core::security::{FrameCtx, NoSecurity, SecurityLayout};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_uslp::{
    FrameContent, FrameFields, FrameKind, MapConfig, PhysicalChannelConfig, SecurityScope,
    SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, Upid, UslpFrame, UslpFrameWriter,
    UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, encode, validate_frame,
};

const BUFFER: usize = 300;

fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from((i * 37 + 11) % 251).unwrap())
        .collect()
}

type Channels = VcTable<2, 2>;

const fn scid() -> UslpSpacecraftId {
    UslpSpacecraftId::new(0xA5A5).unwrap()
}

/// A variable-length channel with the FECF: VC 1 without SDLS and VC 2 with a 4-octet Security
/// Header and 8-octet Security Trailer; MAP 2 on both.
fn channels() -> Channels {
    let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 128, 0, true).unwrap();
    let plain = VcParams::new(TransferFrameType::Variable);
    let sdls = VcParams {
        security: SecurityScope::Vc(SecurityLayout::new(4, 8).unwrap()),
        ..plain
    };
    let vc = |id: u8, params| VcConfig::new(scid(), UslpVcId::new(id).unwrap(), params).unwrap();
    let map = |id: u8| {
        MapConfig::new(
            scid(),
            UslpVcId::new(id).unwrap(),
            UslpMapId::new(2).unwrap(),
            SecurityLayout::NONE,
        )
    };
    Channels::new(pc, &[vc(1, plain), vc(2, sdls)], &[map(1), map(2)]).unwrap()
}

const fn fields(vc: u8) -> FrameFields {
    FrameFields {
        spacecraft_id: scid(),
        source_or_destination: SourceOrDestination::from_bit(false),
        vc_id: UslpVcId::new(vc).unwrap(),
        map_id: UslpMapId::new(2).unwrap(),
        kind: FrameKind::Expedited,
        vcf_count: 0,
    }
}

const CONTENT: FrameContent<'static> = FrameContent {
    tfdf: TfdfHeader {
        rule: TfdzRule::OctetStream,
        upid: Upid::OctetStream,
    },
    insert_zone: &[],
    ocf: None,
};

/// The channel a hook is applied to, for `vc` (API-073).
const fn ctx(vc: u8) -> FrameCtx {
    let f = fields(vc);
    FrameCtx::Uslp {
        scid: f.spacecraft_id,
        vcid: f.vc_id,
        map_id: Some(f.map_id),
    }
}

#[test]
fn encode_writes_only_the_frame() {
    // req: API-022, GEN-014, GEN-020, USLP-010
    let t = channels();
    for vc in [1, 2] {
        let tfdz = [0x55_u8; 30];
        let before = pattern(BUFFER);
        let mut out = before.clone();
        let n = encode(&t, fields(vc), CONTENT, &tfdz, &mut out).unwrap();
        assert!(n < BUFFER);
        assert_eq!(out[n..], before[n..], "vc {vc}");
        assert!(validate_frame(&out[..n], &t, DecodeMode::Strict).is_ok());
        // One octet short, and an empty TFDZ: nothing is written.
        let mut short = before[..n - 1].to_vec();
        let e = encode(&t, fields(vc), CONTENT, &tfdz, &mut short).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(short, before[..n - 1]);
        let mut out = before.clone();
        encode(&t, fields(vc), CONTENT, &[], &mut out).unwrap_err();
        assert_eq!(out, before);
    }
    // An unknown VC.
    let before = pattern(BUFFER);
    let mut out = before.clone();
    encode(&t, fields(3), CONTENT, &[1, 2, 3], &mut out).unwrap_err();
    assert_eq!(out, before);
}

#[test]
fn begin_and_finish_write_only_the_reservation() {
    // req: API-020, API-022, API-023, GEN-014, GEN-020
    let t = channels();
    let max_tfdz = 40;
    for vc in [1, 2] {
        for tfdz_len in [max_tfdz, 25, 1] {
            let before = pattern(BUFFER);
            let mut out = before.clone();
            let mut w =
                UslpFrameWriter::begin(&mut out, &t, fields(vc), CONTENT, max_tfdz).unwrap();
            assert_eq!(w.tfdz_mut().unwrap().len(), max_tfdz);
            let security =
                w.security_header_mut().unwrap().len() + w.security_trailer_mut().unwrap().len();
            // The Primary Header (7), the TFDF Header (1), the reservation, and the FECF (2).
            let total = 7 + 1 + security + max_tfdz + 2;
            w.tfdz_mut().unwrap()[..tfdz_len].fill(0x5C);
            let ctx = ctx(vc);
            let n = w
                .protect(tfdz_len, &mut NoSecurity, &ctx)
                .unwrap()
                .finalize()
                .len();
            assert_eq!(n, total - (max_tfdz - tfdz_len));
            assert_eq!(out[total..], before[total..], "vc {vc}, {tfdz_len}");
            assert!(UslpFrame::decode(&out[..n], &t, DecodeMode::Strict).is_ok());
            assert!(validate_frame(&out[..n], &t, DecodeMode::Strict).is_ok());
        }
    }
}

#[test]
fn a_failed_begin_or_finish_changes_nothing_of_the_frame() {
    // req: API-022, API-023, GEN-020
    let t = channels();
    let before = pattern(BUFFER);
    // Too small for the reservation: 7 + 1 + 40 + 2 = 50 octets.
    let mut out = before[..49].to_vec();
    let e = UslpFrameWriter::begin(&mut out, &t, fields(1), CONTENT, 40).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(out, before[..49]);
    // A failed finish leaves the unfinished frame as `begin` wrote it.
    let mut begun = before.clone();
    {
        let _begun = UslpFrameWriter::begin(&mut begun, &t, fields(1), CONTENT, 40).unwrap();
    }
    for tfdz_len in [0, 41] {
        let mut out = before.clone();
        let w = UslpFrameWriter::begin(&mut out, &t, fields(1), CONTENT, 40).unwrap();
        w.protect(tfdz_len, &mut NoSecurity, &ctx(1)).unwrap_err();
        assert_eq!(out, begun, "tfdz_len {tfdz_len}");
        assert_eq!(out[50..], before[50..]);
    }
}
