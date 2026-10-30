//! Structured USLP frames through the writers: `decode(encode(x)) == x` field by field for
//! user data and protocol control frames on fixed-length and variable-length channels; the
//! two-phase writer agrees with `encode`; the frame passes the USLP Frame Validation Checks; a
//! rejected encode leaves the buffer unchanged; and an OID frame advances the generator by
//! exactly its TFDZ, or not at all when rejected (USLP-003…USLP-006, USLP-009, USLP-010, API-020,
//! API-022, GEN-042).
#![no_main]

// req: USLP-003, USLP-004, USLP-005, USLP-006, USLP-009, USLP-010, USLP-014, API-020, API-021, API-022, GEN-042, VER-022

use aldis_core::DecodeMode;
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_uslp::{
    ConstructionRule, FrameContent, FrameFields, FrameKind, MapConfig, OcfUse,
    PhysicalChannelConfig, SecurityScope, SourceOrDestination, TfdfHeader, TfdzRule,
    TransferFrameType, Upid, UslpFrame, UslpFrameWriter, UslpMapId, UslpSpacecraftId, UslpVcId,
    VcConfig, VcParams, VcTable, VcfCountLen, encode, encode_oid, validate_frame,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// `decode(encode(x)) == x`, the two-phase writer, validation, and a rejected encode.
fn check_frame(
    table: &VcTable<2, 1>,
    fields: FrameFields,
    content: FrameContent<'_>,
    zone: &[u8],
    fill: u8,
) {
    let (scid, vcid, map, kind, tfdf, ocf) = (
        fields.spacecraft_id,
        fields.vc_id,
        fields.map_id,
        fields.kind,
        content.tfdf,
        content.ocf,
    );
    let mut out = vec![fill; 420];
    match encode(table, fields, content, zone, &mut out) {
        Ok(frame_len_out) => {
            let frame = &out[..frame_len_out];
            let dec = UslpFrame::decode(frame, table, DecodeMode::Strict).unwrap();
            let view = dec.full().unwrap();
            let hdr = view.header();
            assert_eq!(
                (hdr.spacecraft_id(), hdr.vc_id(), hdr.map_id()),
                (scid, vcid, map)
            );
            assert_eq!(
                (hdr.kind(), hdr.vcf_count().value()),
                (kind, fields.vcf_count)
            );
            assert_eq!(hdr.source_or_destination(), fields.source_or_destination);
            assert_eq!((view.tfdf_header(), view.tfdz()), (tfdf, zone));
            assert_eq!((view.insert_zone(), view.ocf()), (content.insert_zone, ocf));
            assert_eq!(hdr.frame_len(), frame_len_out);
            assert!(validate_frame(frame, table, DecodeMode::Strict).is_ok());
            let mut two = vec![!fill; frame_len_out];
            let mut writer =
                UslpFrameWriter::begin(&mut two, table, fields, content, zone.len()).unwrap();
            writer.tfdz_mut().unwrap().copy_from_slice(zone);
            let ctx = FrameCtx::Uslp {
                scid,
                vcid,
                map_id: Some(map),
            };
            let done = writer
                .protect(zone.len(), &mut PreFilledSecurity, &ctx)
                .unwrap()
                .finalize()
                .unwrap();
            assert_eq!(done.len(), frame_len_out);
            assert_eq!(two, frame);
        }
        Err(_) => assert!(out.iter().all(|&b| b == fill), "a rejected encode wrote"),
    }
}

/// OID frames on a fixed-length channel advance the generator by exactly their TFDZ (USLP-009,
/// GEN-042).
fn check_oid(
    u: &mut Unstructured<'_>,
    table: &VcTable<2, 1>,
    fields: FrameFields,
    insert_zone: &[u8],
    fill: u8,
) -> Result<()> {
    let mut oid = OidGenerator::new();
    let mut skip = vec![0_u8; u.int_in_range(0..=20)?];
    oid.fill(&mut skip);
    let before = oid.clone();
    let oid_fields = FrameFields {
        vc_id: UslpVcId::new(63).unwrap(),
        map_id: UslpMapId::new(u.int_in_range(0..=1)?).unwrap(),
        ..fields
    };
    let mut out = vec![fill; 420];
    if let Ok(frame_len_out) = encode_oid(table, oid_fields, insert_zone, None, &mut oid, &mut out)
    {
        let dec = UslpFrame::decode(&out[..frame_len_out], table, DecodeMode::Strict).unwrap();
        let mut reference = before;
        let mut expect = vec![0_u8; dec.tfdz().len()];
        reference.fill(&mut expect);
        assert_eq!(dec.tfdz(), expect.as_slice());
        assert_eq!(oid, reference);
        assert_eq!(dec.tfdf_header().upid, Upid::OnlyIdleData);
    } else {
        assert_eq!(oid, before);
        assert!(out.iter().all(|&b| b == fill));
    }
    Ok(())
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let scid = UslpSpacecraftId::new(u.arbitrary()?).unwrap();
    let vcid = UslpVcId::new(u.int_in_range(0..=62)?).unwrap();
    let map = UslpMapId::new(u.int_in_range(0..=15)?).unwrap();
    let fixed: bool = u.arbitrary()?;
    let frame_type = if fixed {
        TransferFrameType::Fixed
    } else {
        TransferFrameType::Variable
    };
    let vc_type = if fixed || u.arbitrary()? {
        TransferFrameType::Fixed
    } else {
        TransferFrameType::Variable
    };
    let iz = if fixed { u.int_in_range(0..=4)? } else { 0 };
    let Ok(pc) =
        PhysicalChannelConfig::new(frame_type, u.int_in_range(9..=400)?, iz, u.arbitrary()?)
    else {
        return Ok(());
    };
    let len = |u: &mut Unstructured<'_>| -> Result<VcfCountLen> {
        Ok(VcfCountLen::new(u.int_in_range(0..=7)?).unwrap())
    };
    let ocf_use = match (u.arbitrary::<bool>()?, vc_type) {
        (false, _) => OcfUse::Absent,
        (true, TransferFrameType::Fixed) => OcfUse::Required,
        (true, TransferFrameType::Variable) => OcfUse::Allowed,
    };
    let security = SecurityLayout::new(u.int_in_range(0..=8)?, u.int_in_range(0..=8)?).unwrap();
    let params = VcParams {
        vcf_count_len_sequence: len(u)?,
        vcf_count_len_expedited: len(u)?,
        ocf: ocf_use,
        security: SecurityScope::Vc(security),
        ..VcParams::new(vc_type)
    };
    let oid_vc = VcParams {
        vcf_count_len_sequence: len(u)?,
        vcf_count_len_expedited: len(u)?,
        ..VcParams::new(TransferFrameType::Fixed)
    };
    let mut vcs = vec![VcConfig::new(scid, vcid, params).unwrap()];
    if fixed {
        vcs.push(VcConfig::new(scid, UslpVcId::new(63).unwrap(), oid_vc).unwrap());
    }
    let maps = [MapConfig::new(scid, vcid, map, SecurityLayout::NONE)];
    let Ok(table) = VcTable::<2, 1>::new(pc, &vcs, &maps) else {
        return Ok(());
    };
    let kind = *u.choose(&[
        FrameKind::SequenceControlled,
        FrameKind::Expedited,
        FrameKind::ProtocolControl,
    ])?;
    let fields = FrameFields {
        spacecraft_id: scid,
        source_or_destination: SourceOrDestination::from_bit(u.arbitrary()?),
        vc_id: vcid,
        map_id: map,
        kind,
        vcf_count: u.arbitrary()?,
    };
    let rule = ConstructionRule::from_bits(u.arbitrary()?);
    let tfdf = TfdfHeader {
        rule: TfdzRule::new(rule, u.arbitrary()?),
        upid: Upid::from_bits(u.int_in_range(0..=31)?).unwrap(),
    };
    let ocf = if u.arbitrary()? {
        Some(Ocf::decode(u.arbitrary()?))
    } else {
        None
    };
    let iz_len = u.int_in_range(0..=4)?;
    let insert_zone = u.bytes(iz_len)?.to_vec();
    let tfdz_len = u.int_in_range(0..=400)?;
    let zone = u.bytes(tfdz_len)?.to_vec();
    let content = FrameContent {
        tfdf,
        insert_zone: &insert_zone,
        ocf,
    };
    let fill: u8 = u.arbitrary()?;
    check_frame(&table, fields, content, &zone, fill);
    if fixed {
        check_oid(u, &table, fields, &insert_zone, fill)?;
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
