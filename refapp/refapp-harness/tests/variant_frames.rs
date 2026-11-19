//! Strict decoding of the AOS and USLP frames of the variants (docs/reference-application.md
//! §4.3, APP-086; A4 WP-2). Each frame is built with the writers of the codecs through the
//! facade, from the values of the scenario file.

use aldis::aos;
use aldis::core::ids::{FrameSeqNum, SpacecraftId, VcId};
use aldis::core::ocf::Ocf;
use aldis::core::oid::OidGenerator;
use aldis::core::security::SecurityLayout;
use aldis::tc::{Clcw, ControlCommand, FrameType};
use aldis::uslp;
use refapp_harness::frame::decode_frame;
use refapp_harness::scenario::{Level, Scenario, Variant};
use refapp_harness::trace::Dir;

/// The initial CLCW of the downlink, as the OCF of a frame.
fn clcw_ocf() -> Ocf {
    Clcw::decode(0x0100_0000_u32.to_be_bytes())
        .unwrap()
        .to_ocf()
}

/// The octets of the CLCW of [`clcw_ocf`], decoded.
fn clcw() -> Clcw {
    Clcw::decode(0x0100_0000_u32.to_be_bytes()).unwrap()
}

/// A packet zone or TFDZ of `len` octets that holds no real packet: the codecs do not read it.
fn zone(len: usize) -> Vec<u8> {
    (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect()
}

// ---- AOS downlink (APP-169…APP-174) ----

struct Aos {
    pc: aos::AosPhysicalChannelConfig,
    table: aos::VcTable<3>,
    scid: SpacecraftId<10>,
}

fn aos_channels(s: &Scenario) -> Aos {
    let a = &s.aos;
    let pc = aos::AosPhysicalChannelConfig::new(
        usize::from(a.frame_length.get()),
        a.fhec,
        0,
        a.fecf,
        aos::AosEdition::Issue5,
    )
    .unwrap();
    let scid = SpacecraftId::<10>::new(s.spacecraft_id.get()).unwrap();
    let vc = |id: u8| {
        let vc = aos::AosVirtualChannelConfig::new(
            scid,
            VcId::<6>::new(id).unwrap(),
            aos::DataFieldContent::Mpdu,
        )
        .unwrap();
        if a.vc_ocf { vc.with_ocf() } else { vc }
    };
    let table = aos::VcTable::new(
        pc,
        &[
            vc(a.vcid_realtime.get()),
            vc(a.vcid_payload.get()),
            vc(a.vcid_idle.get()),
        ],
    )
    .unwrap();
    Aos { pc, table, scid }
}

/// An AOS frame on `vcid` with the count `count` and the pointer `pointer`.
fn aos_frame(s: &Scenario, vcid: u8, count: u32, pointer: aos::PacketPointer, ocf: Ocf) -> Vec<u8> {
    let c = aos_channels(s);
    let fields = aos::AosFrameFields::new(
        c.scid,
        VcId::<6>::new(vcid).unwrap(),
        aos::VcFrameCount::new(count).unwrap(),
        aos::PduHeader::Mpdu(pointer),
    );
    // Packet zone: 223 − header 6 − FHEC 2 − M_PDU Header 2 − VC_OCF 4 − FECF 2 (APP-169).
    let zone = zone(207);
    let content = aos::AosFrameContent {
        ocf: Some(ocf),
        ..aos::AosFrameContent::new(&zone)
    };
    let mut out = vec![0_u8; 223];
    let n = aos::encode(&c.pc, &c.table, &fields, &content, &mut out).unwrap();
    assert_eq!(n, 223);
    out
}

const fn starts_at(offset: u16) -> aos::PacketPointer {
    aos::PacketPointer::StartsAt(aos::PacketOffset::new(offset).unwrap())
}

#[test]
fn an_aos_frame_decodes_with_its_pointer_count_and_clcw() {
    // req: APP-086, APP-169, APP-173, APP-174
    let s = Scenario::load().unwrap();
    for vcid in [0, 1, 7] {
        let frame = aos_frame(&s, vcid, 70_000, starts_at(5), clcw_ocf());
        let d = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame).unwrap();
        assert_eq!(d.vcid, vcid);
        assert_eq!(d.data, zone(207));
        assert_eq!(d.packet_pointer, Some(starts_at(5)));
        assert_eq!(d.count, Some(70_000));
        assert_eq!(d.clcw, Some(clcw()));
        assert!(!d.oid);
        // The fields of the other protocols stay empty.
        assert_eq!(
            (d.pointer, d.tfdf, d.map_id, d.kind),
            (None, None, None, None)
        );
        assert_eq!((d.frame_type, d.sequence), (None, None));
    }
    // The same frames at the frame level.
    let frame = aos_frame(&s, 0, 0, aos::PacketPointer::NoPacketStart, clcw_ocf());
    let d = decode_frame(&s, Level::Frames, Variant::Aos, Dir::Down, &frame).unwrap();
    assert_eq!(d.packet_pointer, Some(aos::PacketPointer::NoPacketStart));
    let idle = aos_frame(&s, 7, 1, aos::PacketPointer::OnlyIdleData, clcw_ocf());
    let d = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &idle).unwrap();
    assert_eq!(d.packet_pointer, Some(aos::PacketPointer::OnlyIdleData));
}

#[test]
fn an_aos_frame_has_its_header_corrected_and_its_fecf_checked() {
    // req: APP-086, APP-169, APP-174
    let s = Scenario::load().unwrap();
    let frame = aos_frame(&s, 1, 9, starts_at(0), clcw_ocf());
    let ok = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame).unwrap();
    // One bit error in the header fields that the FHEC covers (the Master Channel ID and the
    // VCID, octets 0 and 1) is corrected: the frame decodes as it was written (732.0 §4.3.7.2).
    let mut hit = frame.clone();
    hit[1] ^= 0x04;
    let d = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &hit).unwrap();
    assert_eq!(d, ok);
    // A bit error in the frame past the header fails the FECF.
    let mut bad = frame.clone();
    bad[100] ^= 1;
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &bad).is_err());
    let mut bad = frame.clone();
    bad[222] ^= 1;
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &bad).is_err());
    // A header with more errors than the FHEC corrects fails.
    let mut bad = frame.clone();
    for octet in &mut bad[..6] {
        *octet ^= 0xFF;
    }
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &bad).is_err());
    // A frame of another length fails.
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame[..222]).is_err());
}

#[test]
fn an_aos_frame_of_another_channel_or_spacecraft_fails() {
    // req: APP-086, APP-170, APP-174
    let s = Scenario::load().unwrap();
    // A VC that is not configured decodes as opaque octets in the codec; the check refuses it.
    let mut other = s.clone();
    other.aos.vcid_payload = refapp_harness::ranged::Ranged::new(2).unwrap();
    let frame = aos_frame(&other, 2, 0, starts_at(0), clcw_ocf());
    let e = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame).unwrap_err();
    assert!(e.contains("configured"), "{e}");
    // Another Spacecraft ID.
    let mut other = s.clone();
    other.spacecraft_id = refapp_harness::ranged::Ranged::new(342).unwrap();
    let frame = aos_frame(&other, 0, 0, starts_at(0), clcw_ocf());
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame).is_err());
}

#[test]
fn the_vc_ocf_of_an_aos_frame_is_a_clcw() {
    // req: APP-086, APP-173
    let s = Scenario::load().unwrap();
    // Control Word Type 1 (a project-specific report) is no CLCW.
    let frame = aos_frame(&s, 0, 0, starts_at(0), Ocf::decode([0x80, 0, 0, 0]));
    let e = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame).unwrap_err();
    assert!(e.contains("not a CLCW"), "{e}");
    // A Type-1 report whose reserved bits are not a CLCW's fails the strict CLCW decode.
    let frame = aos_frame(&s, 0, 0, starts_at(0), Ocf::decode([0x2F, 0, 0, 0]));
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &frame).is_err());
}

#[test]
fn an_aos_oid_frame_is_rejected() {
    // req: APP-086, APP-170
    let s = Scenario::load().unwrap();
    let c = aos_channels(&s);
    let mut out = vec![0_u8; 223];
    let n = aos::encode_oid(
        &c.pc,
        &c.table,
        &aos::AosFrameFields::oid(c.scid, aos::VcFrameCount::new(0).unwrap()),
        &[],
        &mut OidGenerator::new(),
        &mut out,
    )
    .unwrap();
    assert_eq!(n, 223);
    // The flight node sends no OID frame (APP-170, OI-43).
    let e = decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &out).unwrap_err();
    assert!(e.contains("OID frame"), "{e}");
}

/// A Type-BD TC frame with a Segment Header (232.0 §4.1.2, §4.1.3).
fn tc_frame(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(5 + 1 + data.len() + 2 - 1).unwrap();
    let mut f = vec![0x21, 0x55];
    f.extend_from_slice(&len.to_be_bytes());
    f.push(0);
    f.push(0xC0);
    f.extend_from_slice(data);
    let crc = aldis::core::crc::crc16(&f);
    f.extend_from_slice(&crc.to_be_bytes());
    f
}

#[test]
fn the_aos_uplink_is_the_tc_uplink() {
    // req: APP-086, APP-187
    // A TC frame decodes at `aos` as at `tm_tc`, and an AOS frame is no uplink frame.
    let s = Scenario::load().unwrap();
    let tc = tc_frame(&[0x1A; 9]);
    let d = decode_frame(&s, Level::Frames, Variant::Aos, Dir::Up, &tc).unwrap();
    assert_eq!((d.vcid, d.frame_type), (0, Some(FrameType::Bd)));
    let aos = aos_frame(&s, 0, 0, starts_at(0), clcw_ocf());
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Up, &aos).is_err());
}

// ---- USLP (APP-175…APP-183) ----

const fn uslp_scid(s: &Scenario) -> uslp::UslpSpacecraftId {
    uslp::UslpSpacecraftId::new(s.spacecraft_id.get()).unwrap()
}

const fn uslp_id(id: u8) -> uslp::UslpVcId {
    uslp::UslpVcId::new(id).unwrap()
}

type DownTable = uslp::VcTable<3, 2>;

fn uslp_down_channels(s: &Scenario) -> DownTable {
    let d = &s.uslp_down;
    let scid = uslp_scid(s);
    let physical = uslp::PhysicalChannelConfig::new(
        uslp::TransferFrameType::Fixed,
        usize::from(d.frame_length.get()),
        0,
        d.fecf,
    )
    .unwrap();
    let data = uslp::VcParams {
        vcf_count_len_sequence: uslp::VcfCountLen::new(d.vcf_count_length_sequence.get()).unwrap(),
        vcf_count_len_expedited: uslp::VcfCountLen::new(d.vcf_count_length_expedited.get())
            .unwrap(),
        ocf: uslp::OcfUse::Required,
        ..uslp::VcParams::new(uslp::TransferFrameType::Fixed)
    };
    let oid = uslp::VcParams {
        ocf: uslp::OcfUse::Required,
        ..uslp::VcParams::new(uslp::TransferFrameType::Fixed)
    };
    let map = |vc: u8| {
        uslp::MapConfig::new(
            scid,
            uslp_id(vc),
            uslp::UslpMapId::new(0).unwrap(),
            SecurityLayout::NONE,
        )
    };
    uslp::VcTable::new(
        physical,
        &[
            uslp::VcConfig::new(scid, uslp_id(d.vcid_realtime.get()), data).unwrap(),
            uslp::VcConfig::new(scid, uslp_id(d.vcid_payload.get()), data).unwrap(),
            uslp::VcConfig::new(scid, uslp_id(uslp::OID_VCID), oid).unwrap(),
        ],
        &[map(d.vcid_realtime.get()), map(d.vcid_payload.get())],
    )
    .unwrap()
}

/// The fields of a USLP frame of the scenario.
const fn uslp_fields(
    s: &Scenario,
    sod: uslp::SourceOrDestination,
    vc: u8,
    kind: uslp::FrameKind,
    count: u64,
) -> uslp::FrameFields {
    uslp::FrameFields {
        spacecraft_id: uslp_scid(s),
        source_or_destination: sod,
        vc_id: uslp_id(vc),
        map_id: uslp::UslpMapId::new(0).unwrap(),
        kind,
        vcf_count: count,
    }
}

const fn pointer_header(upid: uslp::Upid, pointer: u16) -> uslp::TfdfHeader {
    uslp::TfdfHeader {
        rule: uslp::TfdzRule::PacketsSpanningFrames(uslp::FirstHeaderPointer::from_raw(pointer)),
        upid,
    }
}

/// A USLP downlink data frame on `vcid`: expedited, a one-octet VCF Count, the MC OCF.
fn uslp_down_frame(s: &Scenario, vcid: u8, count: u64, ocf: Ocf) -> Vec<u8> {
    let table = uslp_down_channels(s);
    let fields = uslp_fields(
        s,
        uslp::SourceOrDestination::Source,
        vcid,
        uslp::FrameKind::Expedited,
        count,
    );
    let content = uslp::FrameContent {
        tfdf: pointer_header(uslp::Upid::Packets, 4),
        insert_zone: &[],
        ocf: Some(ocf),
    };
    let mut out = vec![0_u8; 223];
    // The TFDZ: 223 − header 7 − VCF Count 1 − TFDF Header 3 − OCF 4 − FECF 2 (APP-176).
    let n = uslp::encode(&table, fields, content, &zone(206), &mut out).unwrap();
    assert_eq!(n, 223);
    out
}

fn uslp_oid_frame(s: &Scenario, ocf: Ocf) -> Vec<u8> {
    let table = uslp_down_channels(s);
    let fields = uslp_fields(
        s,
        uslp::SourceOrDestination::Source,
        uslp::OID_VCID,
        uslp::FrameKind::Expedited,
        0,
    );
    let mut out = vec![0_u8; 223];
    let n = uslp::encode_oid(
        &table,
        fields,
        &[],
        Some(ocf),
        &mut OidGenerator::new(),
        &mut out,
    )
    .unwrap();
    assert_eq!(n, 223);
    out
}

#[test]
fn a_uslp_downlink_frame_decodes_with_its_rule_upid_map_and_clcw() {
    // req: APP-086, APP-175, APP-176, APP-179
    let s = Scenario::load().unwrap();
    for vcid in [0, 1] {
        let frame = uslp_down_frame(&s, vcid, 200, clcw_ocf());
        let d = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &frame).unwrap();
        assert_eq!(d.vcid, vcid);
        assert_eq!(d.data, zone(206));
        assert_eq!(
            d.tfdf,
            Some(pointer_header(uslp::Upid::Packets, 4)),
            "rule 000, the UPID of packets, the pointer"
        );
        assert_eq!((d.map_id, d.count), (Some(0), Some(200)));
        assert_eq!(d.kind, Some(uslp::FrameKind::Expedited));
        assert_eq!(d.clcw, Some(clcw()));
        assert!(!d.oid);
        assert_eq!(
            (d.pointer, d.packet_pointer, d.frame_type),
            (None, None, None)
        );
    }
}

#[test]
fn a_uslp_oid_frame_is_accepted_with_its_ocf() {
    // req: APP-086, APP-178, APP-179
    let s = Scenario::load().unwrap();
    let frame = uslp_oid_frame(&s, clcw_ocf());
    let d = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &frame).unwrap();
    assert!(d.oid);
    assert_eq!((d.vcid, d.count, d.clcw), (63, None, Some(clcw())));
    assert_eq!(d.data.len(), 207);
    let tfdf = d.tfdf.unwrap();
    assert_eq!(tfdf.upid, uslp::Upid::OnlyIdleData);
    assert_eq!(tfdf.rule.rule(), uslp::ConstructionRule::SduStart);
    // An OID frame whose OCF Flag is cleared is no frame of the master channel, which requires
    // the OCF (OI-76). The flag is bit 3 of octet 6 (732.1 §4.1.2.10); the FECF then fails too,
    // so the frame is wrong in more than one way, and any one of them is an error.
    let mut no_ocf = frame;
    no_ocf[6] &= !0x08;
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &no_ocf).is_err());
}

#[test]
fn a_uslp_downlink_frame_fails_on_the_fecf_the_spacecraft_and_the_ocf() {
    // req: APP-086, APP-175, APP-179
    let s = Scenario::load().unwrap();
    let frame = uslp_down_frame(&s, 0, 0, clcw_ocf());
    let mut bad = frame.clone();
    bad[100] ^= 1;
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &bad).is_err());
    let mut bad = frame.clone();
    bad[222] ^= 1;
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &bad).is_err());
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &frame[..222]).is_err());
    let mut longer = frame;
    longer.push(0);
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &longer).is_err());
    // Another Spacecraft ID (APP-026).
    let mut other = s.clone();
    other.spacecraft_id = refapp_harness::ranged::Ranged::new(342).unwrap();
    let frame_of_other = uslp_down_frame(&other, 0, 0, clcw_ocf());
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &frame_of_other).is_err());
    // A frame that names the destination is no downlink frame (APP-176).
    let table = uslp_down_channels(&s);
    let content = uslp::FrameContent {
        tfdf: pointer_header(uslp::Upid::Packets, 0),
        insert_zone: &[],
        ocf: Some(clcw_ocf()),
    };
    let fields = uslp_fields(
        &s,
        uslp::SourceOrDestination::Destination,
        0,
        uslp::FrameKind::Expedited,
        0,
    );
    let mut out = vec![0_u8; 223];
    uslp::encode(&table, fields, content, &zone(206), &mut out).unwrap();
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &out).unwrap_err();
    assert!(e.contains("Source-or-Destination"), "{e}");
    // The OCF of every frame is a CLCW.
    let frame = uslp_down_frame(&s, 0, 0, Ocf::decode([0x80, 0, 0, 0]));
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &frame).unwrap_err();
    assert!(e.contains("not a CLCW"), "{e}");
}

#[test]
fn a_uslp_downlink_frame_is_expedited_and_a_data_frame_has_the_rule_000() {
    // req: APP-086, APP-176, APP-178
    let s = Scenario::load().unwrap();
    let table = uslp_down_channels(&s);
    let encode = |kind: uslp::FrameKind, tfdf: uslp::TfdfHeader, body: &[u8]| {
        let fields = uslp_fields(&s, uslp::SourceOrDestination::Source, 0, kind, 0);
        let content = uslp::FrameContent {
            tfdf,
            insert_zone: &[],
            ocf: Some(clcw_ocf()),
        };
        let mut out = vec![0_u8; 223];
        uslp::encode(&table, fields, content, body, &mut out).unwrap();
        out
    };
    // A sequence-controlled frame (flags `00`) has no VCF Count here and is no expedited frame.
    let seq = encode(
        uslp::FrameKind::SequenceControlled,
        pointer_header(uslp::Upid::Packets, 0),
        &zone(207),
    );
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &seq).unwrap_err();
    assert!(e.contains("expedited"), "{e}");
    // A rule other than `000` on a data frame.
    let start = uslp::TfdfHeader {
        rule: uslp::TfdzRule::SduStart(uslp::LastValidOctet::from_raw(205)),
        upid: uslp::Upid::Packets,
    };
    let sdu = encode(uslp::FrameKind::Expedited, start, &zone(206));
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &sdu).unwrap_err();
    assert!(e.contains("rule `000`"), "{e}");
}

type UpTable = uslp::VcTable<1, 1>;

fn uslp_up_channels(s: &Scenario) -> UpTable {
    let u = &s.uslp_up;
    let scid = uslp_scid(s);
    let physical = uslp::PhysicalChannelConfig::new(
        uslp::TransferFrameType::Variable,
        usize::from(u.max_frame_length.get()),
        0,
        u.fecf,
    )
    .unwrap();
    let params = uslp::VcParams {
        vcf_count_len_sequence: uslp::VcfCountLen::new(u.vcf_count_length_sequence.get()).unwrap(),
        vcf_count_len_expedited: uslp::VcfCountLen::new(u.vcf_count_length_expedited.get())
            .unwrap(),
        ..uslp::VcParams::new(uslp::TransferFrameType::Variable)
    };
    let map = uslp::MapConfig::new(
        scid,
        uslp_id(0),
        uslp::UslpMapId::new(0).unwrap(),
        SecurityLayout::NONE,
    );
    uslp::VcTable::new(
        physical,
        &[uslp::VcConfig::new(scid, uslp_id(0), params).unwrap()],
        &[map],
    )
    .unwrap()
}

const fn up_header(upid: uslp::Upid) -> uslp::TfdfHeader {
    uslp::TfdfHeader {
        rule: uslp::TfdzRule::NoSegmentation,
        upid,
    }
}

/// A USLP uplink frame of `kind` with the TFDF Header `header` and the TFDZ `body`.
fn uslp_up_frame(
    s: &Scenario,
    kind: uslp::FrameKind,
    count: u64,
    header: uslp::TfdfHeader,
    body: &[u8],
) -> Vec<u8> {
    let content = uslp::FrameContent {
        tfdf: header,
        insert_zone: &[],
        ocf: None,
    };
    let fields = uslp_fields(s, uslp::SourceOrDestination::Destination, 0, kind, count);
    let mut out = vec![0_u8; 512];
    let n = uslp::encode(&uslp_up_channels(s), fields, content, body, &mut out).unwrap();
    out.truncate(n);
    out
}

/// A Space Packet of 20 octets (PVN 0, APID 1, unsegmented): the codecs do not read it.
const PACKET: [u8; 20] = [
    0x00, 0x01, 0xC0, 0x00, 0x00, 0x0D, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[test]
fn a_uslp_uplink_frame_is_type_ad_bd_or_bc_by_its_flags() {
    // req: APP-086, APP-183
    let s = Scenario::load().unwrap();
    let ad = uslp_up_frame(
        &s,
        uslp::FrameKind::SequenceControlled,
        5,
        up_header(uslp::Upid::Packets),
        &PACKET,
    );
    let d = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &ad).unwrap();
    assert_eq!(d.frame_type, Some(FrameType::Ad));
    assert_eq!((d.sequence, d.vcid, d.map_id), (Some(5), 0, Some(0)));
    assert_eq!(d.data, PACKET);
    assert_eq!(d.control_command, None);
    assert!(!d.is_control());
    assert_eq!(d.kind, Some(uslp::FrameKind::SequenceControlled));
    assert_eq!(d.tfdf, Some(up_header(uslp::Upid::Packets)));
    assert_eq!((d.count, d.clcw, d.oid), (Some(5), None, false));
    // The frame length is 7 + 1 + 1 + 20 + 2 and its Frame Length field is one less (APP-182).
    assert_eq!(ad.len(), 31);
    assert_eq!(u16::from_be_bytes([ad[4], ad[5]]), 30);
    // N(S) wraps modulo 256 in the one-octet VCF Count.
    let last = uslp_up_frame(
        &s,
        uslp::FrameKind::SequenceControlled,
        255,
        up_header(uslp::Upid::Packets),
        &PACKET,
    );
    let d = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &last).unwrap();
    assert_eq!(d.sequence, Some(255));
    // Type-BD: expedited, no count (OI-66).
    let bd = uslp_up_frame(
        &s,
        uslp::FrameKind::Expedited,
        0,
        up_header(uslp::Upid::Packets),
        &PACKET,
    );
    let d = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &bd).unwrap();
    assert_eq!(
        (d.frame_type, d.sequence, d.count),
        (Some(FrameType::Bd), None, None)
    );
    // Type-BC: one Control Command, no count.
    for command in [
        ControlCommand::Unlock,
        ControlCommand::SetVr(FrameSeqNum::new(9)),
    ] {
        let bc = uslp_up_frame(
            &s,
            uslp::FrameKind::ProtocolControl,
            0,
            up_header(uslp::Upid::Cop1ControlCommands),
            command.to_octets().as_slice(),
        );
        let d = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &bc).unwrap();
        assert_eq!(d.frame_type, Some(FrameType::Bc));
        assert!(d.is_control());
        assert_eq!(d.control_command, Some(command));
        assert_eq!((d.sequence, d.count), (None, None));
    }
}

#[test]
fn a_uslp_type_bc_frame_holds_exactly_one_control_command() {
    // req: APP-086, APP-183
    let s = Scenario::load().unwrap();
    let bc = |tfdz: &[u8]| {
        uslp_up_frame(
            &s,
            uslp::FrameKind::ProtocolControl,
            0,
            up_header(uslp::Upid::Cop1ControlCommands),
            tfdz,
        )
    };
    // Reserved patterns, a known pattern with an octet more or less, and two commands.
    for tfdz in [
        &[0x01_u8][..],
        &[0x00, 0x00],
        &[0x82, 0x00],
        &[0x82, 0x00, 0x05, 0x00],
        &[0x00, 0x82, 0x00, 0x05],
        &[0x55; 7],
    ] {
        let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &bc(tfdz)).unwrap_err();
        assert!(
            e.contains("exactly one Control Command"),
            "{tfdz:02x?}: {e}"
        );
    }
}

#[test]
fn a_uslp_uplink_frame_breaks_no_rule_of_app_183_unnoticed() {
    // req: APP-086, APP-181, APP-183, APP-184
    let s = Scenario::load().unwrap();
    let ad = |tfdf: uslp::TfdfHeader| {
        uslp_up_frame(&s, uslp::FrameKind::SequenceControlled, 1, tfdf, &PACKET)
    };
    // The UPID that goes with the type.
    let e = decode_frame(
        &s,
        Level::Cop1,
        Variant::Uslp,
        Dir::Up,
        &ad(up_header(uslp::Upid::OctetStream)),
    )
    .unwrap_err();
    assert!(e.contains("UPID"), "{e}");
    let control = uslp_up_frame(
        &s,
        uslp::FrameKind::ProtocolControl,
        0,
        up_header(uslp::Upid::Packets),
        &[0],
    );
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &control).unwrap_err();
    assert!(e.contains("UPID"), "{e}");
    // A segmentation rule (`100` to `110`) is no rule of the variable-length VC (APP-184); the
    // writer refuses only a rule of the other TFDZ type, so `StartingSegment` encodes.
    let seg = ad(uslp::TfdfHeader {
        rule: uslp::TfdzRule::StartingSegment,
        upid: uslp::Upid::Packets,
    });
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &seg).unwrap_err();
    assert!(e.contains("rule `111`"), "{e}");
    // A frame of the wrong direction: it names the source.
    let table = uslp_up_channels(&s);
    let fields = uslp_fields(
        &s,
        uslp::SourceOrDestination::Source,
        0,
        uslp::FrameKind::SequenceControlled,
        0,
    );
    let content = uslp::FrameContent {
        tfdf: up_header(uslp::Upid::Packets),
        insert_zone: &[],
        ocf: None,
    };
    let mut out = vec![0_u8; 512];
    let n = uslp::encode(&table, fields, content, &PACKET, &mut out).unwrap();
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &out[..n]).unwrap_err();
    assert!(e.contains("Source-or-Destination"), "{e}");
    // The FECF, a frame longer than its Frame Length field says, and a frame cut short.
    let frame = ad(up_header(uslp::Upid::Packets));
    let mut bad = frame.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &bad).is_err());
    let mut longer = frame.clone();
    longer.push(0);
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &longer).is_err());
    assert!(
        decode_frame(
            &s,
            Level::Cop1,
            Variant::Uslp,
            Dir::Up,
            &frame[..frame.len() - 1]
        )
        .is_err()
    );
    // A frame of a VC the master channel does not have: VCID 1, the lowest bit of the VCID is bit 2
    // of octet 3 (732.1 §4.1.2.4); the FECF fails too.
    let mut other_vc = frame;
    other_vc[3] ^= 0x20;
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &other_vc).is_err());
}

#[test]
fn a_uslp_uplink_frame_with_the_reserved_flags_is_rejected() {
    // req: APP-086, APP-185
    let s = Scenario::load().unwrap();
    let frame = uslp_up_frame(
        &s,
        uslp::FrameKind::SequenceControlled,
        1,
        up_header(uslp::Upid::Packets),
        &PACKET,
    );
    // Bypass/Sequence Control Flag 0 with Protocol Control Command Flag 1 is reserved (732.1
    // Table 4-1). They are the two high bits of octet 6, after the Frame Length (§4.1.2.8): set
    // the second, and give the frame the FECF it would have.
    let mut reserved = frame[..frame.len() - 2].to_vec();
    reserved[6] |= 0x40;
    let crc = aldis::core::crc::crc16(&reserved);
    reserved.extend_from_slice(&crc.to_be_bytes());
    let e = decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Up, &reserved).unwrap_err();
    assert!(e.contains("ReservedFlags"), "{e}");
}

#[test]
fn the_variant_must_apply_at_the_level_of_the_frame() {
    // req: APP-086, APP-187
    let s = Scenario::load().unwrap();
    let uslp_frame = uslp_down_frame(&s, 0, 0, clcw_ocf());
    // `uslp` exists at `cop1` only, `aos` not at `packets`.
    for level in [Level::Packets, Level::Frames] {
        let e = decode_frame(&s, level, Variant::Uslp, Dir::Down, &uslp_frame).unwrap_err();
        assert!(e.contains("packet") || e.contains("does not apply"), "{e}");
    }
    let aos = aos_frame(&s, 0, 0, starts_at(0), clcw_ocf());
    assert!(decode_frame(&s, Level::Packets, Variant::Aos, Dir::Down, &aos).is_err());
    // A frame of one protocol is no frame of the other.
    assert!(decode_frame(&s, Level::Cop1, Variant::Aos, Dir::Down, &uslp_frame).is_err());
    assert!(decode_frame(&s, Level::Cop1, Variant::Uslp, Dir::Down, &aos).is_err());
    assert!(decode_frame(&s, Level::Cop1, Variant::TmTc, Dir::Down, &aos).is_err());
}
