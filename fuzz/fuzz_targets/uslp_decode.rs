//! Arbitrary octets through `UslpFrame::decode`, `peek_length`, `validate_frame`, and the
//! delimiter on channels drawn from the input: no panic; the input unchanged; strict ⊆ lenient;
//! `peek_length` agrees with `decode`; the USLP Frame Validation Checks accept exactly the
//! decoded frame; the delimiter's first frame is the decoded one; and a strict decode re-encodes,
//! with its Security Header and Trailer, to the same octets (USLP-001…USLP-007, USLP-014…USLP-016,
//! API-020, GEN-020, GEN-042).
#![no_main]

// req: USLP-001, USLP-002, USLP-003, USLP-004, USLP-005, USLP-006, USLP-014, USLP-015, USLP-016, API-020, GEN-020, GEN-042, VER-022

use aldis_core::crc::crc16;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_uslp::{
    Delimited, FrameContent, FrameFields, MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope,
    TransferFrameType, TruncatedFrameLen, UslpFrame, UslpFrameWriter, UslpMapId, UslpSpacecraftId,
    UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen, encode_truncated, next_frame,
    validate_frame,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// SCID 0x155 on a fixed-length or variable-length channel: VC 1 (the channel's type, VCF
/// Counts and OCF from `u`, MAPs 0 and 3), VC 2 (variable only: SDLS per MAP, MAP 1 with 6/16,
/// truncated frames of 28), VC 3 (a fixed-length TFDZ with SDLS from `u`, MAP 0), and VC 63
/// (fixed only).
fn channels(u: &mut Unstructured<'_>) -> Result<Option<VcTable<4, 4>>> {
    let scid = UslpSpacecraftId::new(0x155).unwrap();
    let vc = |n: u8| UslpVcId::new(n).unwrap();
    let map = |n: u8| UslpMapId::new(n).unwrap();
    let fixed: bool = u.arbitrary()?;
    let frame_type = if fixed {
        TransferFrameType::Fixed
    } else {
        TransferFrameType::Variable
    };
    let iz = if fixed { u.int_in_range(0..=3)? } else { 0 };
    let Ok(pc) =
        PhysicalChannelConfig::new(frame_type, u.int_in_range(9..=300)?, iz, u.arbitrary()?)
    else {
        return Ok(None);
    };
    let len = |u: &mut Unstructured<'_>| -> Result<VcfCountLen> {
        Ok(VcfCountLen::new(u.int_in_range(0..=7)?).unwrap())
    };
    let ocf = match (u.arbitrary::<bool>()?, fixed) {
        (false, _) => OcfUse::Absent,
        (true, true) => OcfUse::Required,
        (true, false) => OcfUse::Allowed,
    };
    let truncated = if fixed {
        None
    } else {
        TruncatedFrameLen::new(u.int_in_range(5..=32)?).ok()
    };
    let vc1 = VcParams {
        vcf_count_len_sequence: len(u)?,
        vcf_count_len_expedited: len(u)?,
        ocf,
        truncated_frame_len: truncated,
        ..VcParams::new(frame_type)
    };
    let sdls = SecurityLayout::new(u.int_in_range(0..=6)?, u.int_in_range(0..=4)?).unwrap();
    let vc3 = VcParams {
        security: SecurityScope::Vc(sdls),
        ..VcParams::new(TransferFrameType::Fixed)
    };
    let mut vcs = vec![VcConfig::new(scid, vc(1), vc1).unwrap()];
    let mut maps = vec![
        MapConfig::new(scid, vc(1), map(0), SecurityLayout::NONE),
        MapConfig::new(scid, vc(1), map(3), SecurityLayout::NONE),
        MapConfig::new(scid, vc(3), map(0), SecurityLayout::NONE),
    ];
    vcs.push(VcConfig::new(scid, vc(3), vc3).unwrap());
    if fixed {
        let oid = VcParams {
            vcf_count_len_expedited: len(u)?,
            ..VcParams::new(TransferFrameType::Fixed)
        };
        vcs.push(VcConfig::new(scid, vc(63), oid).unwrap());
    } else {
        let vc2 = VcParams {
            truncated_frame_len: TruncatedFrameLen::new(28).ok(),
            security: SecurityScope::PerMap,
            ..VcParams::new(TransferFrameType::Variable)
        };
        vcs.push(VcConfig::new(scid, vc(2), vc2).unwrap());
        maps.push(MapConfig::new(
            scid,
            vc(2),
            map(1),
            SecurityLayout::USLP_TRUNCATED,
        ));
    }
    Ok(VcTable::new(pc, &vcs, &maps).ok())
}

/// A strict decode re-encodes to the same octets (API-020).
fn re_encode(table: &VcTable<4, 4>, f: &UslpFrame<'_>) {
    let mut out = vec![0_u8; f.len()];
    match (f.full(), f.truncated()) {
        (Some(full), _) if full.header().vc_id().get() != 63 => {
            let h = full.header();
            let fields = FrameFields {
                spacecraft_id: h.spacecraft_id(),
                source_or_destination: h.source_or_destination(),
                vc_id: h.vc_id(),
                map_id: h.map_id(),
                kind: h.kind(),
                vcf_count: h.vcf_count().value(),
            };
            let content = FrameContent {
                tfdf: full.tfdf_header(),
                insert_zone: full.insert_zone(),
                ocf: full.ocf(),
            };
            let zone = full.tfdz();
            let mut w =
                UslpFrameWriter::begin(&mut out, table, fields, content, zone.len()).unwrap();
            w.tfdz_mut().copy_from_slice(zone);
            w.security_header_mut()
                .copy_from_slice(full.security_header());
            w.security_trailer_mut()
                .copy_from_slice(full.security_trailer());
            assert_eq!(w.finish(zone.len()), Ok(f.len()));
            assert_eq!(out, f.octets());
        }
        (_, Some(t)) => {
            let n = encode_truncated(table, t.channel(), t.tfdf_header(), t.tfdz(), &mut out);
            assert_eq!(n, Ok(f.len()));
            // The writer leaves the security fields zero for the SDLS layer (API-072).
            let mut expect = f.octets().to_vec();
            let h = t.security_header().len();
            expect[4..4 + h].fill(0);
            let tl = t.security_trailer().len();
            let end = expect.len();
            expect[end - tl..].fill(0);
            assert_eq!(out, expect);
        }
        _ => {}
    }
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let Some(table) = channels(u)? else {
        return Ok(());
    };
    let (aim, fix_len, fix_fecf): (bool, bool, bool) =
        (u.arbitrary()?, u.arbitrary()?, u.arbitrary()?);
    let vcid: u8 = *u.choose(&[1, 2, 3, 63])?;
    let mut bytes = u.bytes(u.len())?.to_vec();
    // Often aim the header at a configured channel, give it the length it has, and a correct
    // FECF.
    if aim && bytes.len() >= 4 {
        bytes[0] = 0xC0;
        bytes[1] = 0x15;
        bytes[2] = 0x50 | (vcid >> 3);
        bytes[3] = (bytes[3] & 0x1F) | ((vcid & 7) << 5);
    }
    if fix_len && bytes.len() >= 7 && bytes[3] & 1 == 0 {
        let n = u16::try_from(bytes.len() - 1).unwrap_or(u16::MAX);
        bytes[4..6].copy_from_slice(&n.to_be_bytes());
    }
    if fix_fecf && table.physical().fecf() && bytes.len() >= 9 && bytes[3] & 1 == 0 {
        let n = (usize::from(u16::from_be_bytes([bytes[4], bytes[5]])) + 1).min(bytes.len());
        if n >= 9 {
            let crc = crc16(&bytes[..n - 2]).to_be_bytes();
            bytes[n - 2..n].copy_from_slice(&crc);
        }
    }
    let input = bytes.clone();
    let strict = UslpFrame::decode(&bytes, &table, DecodeMode::Strict);
    let lenient = UslpFrame::decode(&bytes, &table, DecodeMode::Lenient);
    assert_eq!(bytes, input, "decode changed its input");
    if let Ok(s) = &strict {
        assert!(s.anomalies().is_empty());
        let l = lenient.as_ref().unwrap();
        assert_eq!((s.octets(), s.tfdz()), (l.octets(), l.tfdz()));
    }
    match &lenient {
        Ok(f) => {
            assert_eq!(f.octets(), &bytes[..f.len()]);
            assert_eq!(UslpFrame::peek_length(&bytes, &table), Ok(f.len()));
            let v = validate_frame(f.octets(), &table, DecodeMode::Lenient).unwrap();
            assert_eq!(&v, f);
            if f.len() < bytes.len() {
                assert!(validate_frame(&bytes, &table, DecodeMode::Lenient).is_err());
            }
            assert!(
                matches!(next_frame(&bytes, &table), Some(Delimited::Frame(d)) if d == f.octets())
            );
        }
        Err(e) => {
            assert!(
                validate_frame(&bytes, &table, DecodeMode::Lenient).is_err(),
                "{e:?}"
            );
            if e.kind() == ErrorKind::Incomplete
                && let Ok(n) = UslpFrame::peek_length(&bytes, &table)
            {
                assert!(bytes.len() < n.max(7));
            }
        }
    }
    if let Ok(f) = strict {
        re_encode(&table, &f);
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
