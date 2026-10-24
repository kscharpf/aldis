//! Calls every public `aldis-uslp` API (VER-010, M2 D16) and runs every `test-vectors/uslp/`
//! case (`vector`). Inputs pass through `black_box`.

use core::fmt::Write as _;
use core::hint::black_box as bb;

use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::{Anomaly, DecodeMode};
use aldis_uslp::{
    ChannelFields, ConstructionRule, Delimited, FirstHeaderPointer, FrameContent, FrameDelimiter,
    FrameFields, FrameKind, LastValidOctet, MapConfig, OcfUse, PhysicalChannelConfig, Reason,
    SecurityScope, SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen,
    UnassignedUpid, Upid, UslpAnomaly, UslpFrame, UslpFrameWriter, UslpMapId, UslpSpacecraftId,
    UslpVcId, VcConfig, VcLookup, VcParams, VcTable, VcfCount, VcfCountLen, encode, encode_oid,
    encode_truncated, next_frame, validate_frame,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Text, Val, need};

/// Room for every channel the vectors configure.
type Table = VcTable<16, 32>;

const SCID: u16 = 0x155;

const fn scid() -> Option<UslpSpacecraftId> {
    UslpSpacecraftId::new(bb(SCID))
}

/// A variable-length channel of 128 octets with FECF: VC 4 (OCF allowed, VCF 1/1, truncated
/// 12, MAP 0) and VC 5 (SDLS per MAP: MAP 1 with 6/16, truncated 28).
fn variable() -> Option<VcTable<2, 2>> {
    let s = scid()?;
    let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, bb(128), 0, true).ok()?;
    let one = VcfCountLen::new(1).ok()?;
    let vc4 = VcParams {
        vcf_count_len_sequence: one,
        vcf_count_len_expedited: one,
        ocf: OcfUse::Allowed,
        truncated_frame_len: TruncatedFrameLen::new(bb(12)).ok(),
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc5 = VcParams {
        truncated_frame_len: TruncatedFrameLen::new(28).ok(),
        security: SecurityScope::PerMap,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vcs = [
        VcConfig::new(s, UslpVcId::new(4)?, vc4).ok()?,
        VcConfig::new(s, UslpVcId::new(5)?, vc5).ok()?,
    ];
    let maps = [
        MapConfig::new(
            s,
            UslpVcId::new(4)?,
            UslpMapId::new(0)?,
            SecurityLayout::NONE,
        ),
        MapConfig::new(
            s,
            UslpVcId::new(5)?,
            UslpMapId::new(1)?,
            SecurityLayout::USLP_TRUNCATED,
        ),
    ];
    VcTable::new(pc, &vcs, &maps).ok()
}

/// A fixed-length channel of 40 octets with FECF and a 2-octet Insert Zone: VC 1 (rule 000,
/// OCF required, MAP 3) and VC 63.
fn fixed() -> Option<VcTable<2, 1>> {
    let s = scid()?;
    let pc = PhysicalChannelConfig::new(TransferFrameType::Fixed, bb(40), 2, true).ok()?;
    let vc1 = VcParams {
        ocf: OcfUse::Required,
        ..VcParams::new(TransferFrameType::Fixed)
    };
    let vcs = [
        VcConfig::new(s, UslpVcId::new(1)?, vc1).ok()?,
        VcConfig::new(
            s,
            UslpVcId::new(63)?,
            VcParams::new(TransferFrameType::Fixed),
        )
        .ok()?,
    ];
    let map = MapConfig::new(
        s,
        UslpVcId::new(1)?,
        UslpMapId::new(3)?,
        SecurityLayout::NONE,
    );
    VcTable::new(pc, &vcs, &[map]).ok()
}

fn fields(vc: u8, map: u8, kind: FrameKind) -> Option<FrameFields> {
    Some(FrameFields {
        spacecraft_id: scid()?,
        source_or_destination: SourceOrDestination::from_bit(bb(true)),
        vc_id: UslpVcId::new(vc)?,
        map_id: UslpMapId::new(map)?,
        kind,
        vcf_count: bb(1),
    })
}

fn config_group(t: &mut Tally) {
    let (Some(v), Some(f)) = (variable(), fixed()) else {
        t.check(false);
        return;
    };
    let pc = v.physical();
    t.check(pc.frame_type() == TransferFrameType::Variable && pc.frame_len() == 128);
    t.check(pc.fecf() && pc.insert_zone_len() == 0 && v.physical_channel() == pc);
    t.check(v.vcs().count() == 2 && v.maps().count() == 2 && f.vcs().count() == 2);
    let s = scid();
    let vc4 = s.zip(UslpVcId::new(4)).and_then(|(s, c)| v.vc(s, c));
    t.check(vc4.is_some_and(|vc| {
        vc.ocf() == OcfUse::Allowed
            && vc.frame_type() == TransferFrameType::Variable
            && vc.vcf_count_len(true).get() == 1
            && vc.truncated_frame_len().map(TruncatedFrameLen::get) == Some(12)
            && vc.security() == SecurityScope::Vc(SecurityLayout::NONE)
            && vc.spacecraft_id().get() == SCID
            && vc.vc_id().get() == 4
            && vc.params().ocf == OcfUse::Allowed
    }));
    let map = s
        .zip(UslpVcId::new(5))
        .zip(UslpMapId::new(1))
        .and_then(|((s, c), m)| v.map(s, c, m));
    t.check(map.is_some_and(|m| {
        m.map_id().get() == 1
            && m.vc_id().get() == 5
            && m.spacecraft_id().get() == SCID
            && m.security().trailer_len() == 16
    }));
    t.check(s.is_some_and(|s| v.has_spacecraft(s)));
    t.check(PhysicalChannelConfig::new(TransferFrameType::Variable, bb(9), 1, false).is_err());
    t.check(TruncatedFrameLen::new(bb(33)).is_err() && VcfCountLen::new(bb(8)).is_err());
    let len = VcfCountLen::new(bb(2));
    t.check(len.is_ok_and(|l| l.max_count() == 0xFFFF && l.octets() == 2));
    let count = len.ok().and_then(|l| VcfCount::new(l, bb(7)));
    t.check(count.is_some_and(|c| c.value() == 7 && c.len().get() == 2));
}

fn tfdf_group(t: &mut Tally) {
    for bits in 0..8_u8 {
        let r = ConstructionRule::from_bits(bb(bits));
        t.check(r.bits() == bits && r.is_fixed_length() == (bits < 3));
        t.check(r.has_pointer() == r.is_fixed_length());
        let rule = TfdzRule::new(r, bb(5));
        t.check(rule.rule() == r && rule.pointer().is_some() == r.has_pointer());
    }
    t.check(FirstHeaderPointer::from_raw(bb(0xFFFF)) == FirstHeaderPointer::NoPacketStartOrEnd);
    t.check(FirstHeaderPointer::PacketStartsAt(3).raw() == 3);
    t.check(LastValidOctet::from_raw(bb(2)).raw() == 2);
    let h = TfdfHeader {
        rule: TfdzRule::SduStart(LastValidOctet::SduContinues),
        upid: Upid::OnlyIdleData,
    };
    t.check(h.len() == 3 && !h.is_empty() && h.octets() == [0x3F, 0xFF, 0xFF]);
    for bits in 0..32_u8 {
        let u = Upid::from_bits(bb(bits));
        t.check(u.map(Upid::bits) == Some(bits));
        let other = UnassignedUpid::new(bb(bits));
        t.check(other.map(UnassignedUpid::get).is_none_or(|v| v == bits));
    }
    t.check(Upid::from_bits(bb(32)).is_none());
    for k in [
        FrameKind::SequenceControlled,
        FrameKind::Expedited,
        FrameKind::ProtocolControl,
    ] {
        let flags = (k.bypass_flag(), k.protocol_control_flag());
        t.check(FrameKind::from_flags(bb(flags.0), flags.1) == Some(k));
    }
    t.check(FrameKind::from_flags(bb(false), true).is_none());
    t.check(SourceOrDestination::Destination.bit());
    t.check(UslpAnomaly::ALL.iter().map(|a| a.bit()).eq([0, 1]));
    t.check(UslpAnomaly::from_bit(bb(1)) == Some(UslpAnomaly::OidHeaderMismatch));
    t.check(Reason::from_code(bb(17)).map(Reason::code) == Some(17));
}

fn frame_group(t: &mut Tally) {
    let (Some(vt), Some(full)) = (variable(), fields(4, 0, FrameKind::Expedited)) else {
        t.check(false);
        return;
    };
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid: Upid::Packets,
        },
        insert_zone: &[],
        ocf: Some(Ocf::decode(bb([0, 0, 0, 1]))),
    };
    let mut out = [0_u8; 64];
    let len = encode(&vt, full, content, bb(&[1, 2, 3]), &mut out);
    t.check(len == Ok(18));
    let frame = out.get(..18).unwrap_or_default();
    match UslpFrame::decode(bb(frame), &vt, DecodeMode::Strict) {
        Ok(dec) => {
            t.check(!dec.is_truncated() && dec.truncated().is_none() && !dec.is_empty());
            t.check(dec.spacecraft_id().get() == SCID && dec.vc_id().get() == 4);
            t.check(dec.map_id().get() == 0 && dec.source_or_destination().bit());
            t.check(dec.tfdz() == [1, 2, 3] && dec.len() == 18 && dec.octets() == frame);
            t.check(dec.security_header().is_empty() && dec.security_trailer().is_empty());
            t.check(dec.anomalies().is_empty() && dec.channel().vc_id.get() == 4);
            t.check(dec.tfdf_header().rule == TfdzRule::NoSegmentation);
            if let Some(view) = dec.full() {
                let hdr = view.header();
                t.check(view.kind() == FrameKind::Expedited && view.vcf_count().value() == 1);
                t.check(
                    view.ocf().is_some() && view.fecf().is_some() && view.insert_zone().is_empty(),
                );
                t.check(view.tfdz().len() == 3 && view.len() == 18 && !view.is_empty());
                t.check(view.security_header().is_empty() && view.security_trailer().is_empty());
                t.check(view.octets() == frame && view.anomalies().is_empty());
                t.check(view.tfdf_header() == dec.tfdf_header());
                t.check(
                    hdr.len() == 8 && !hdr.is_empty() && hdr.frame_len() == 18 && hdr.ocf_flag(),
                );
                t.check(hdr.kind() == FrameKind::Expedited && hdr.vcf_count().len().get() == 1);
                t.check(hdr.spacecraft_id().get() == SCID && hdr.vc_id().get() == 4);
                t.check(hdr.map_id().get() == 0 && hdr.source_or_destination().bit());
                t.check(hdr.channel().map_id.get() == 0);
                t.check(hdr.octets().get(..8) == frame.get(..8));
            } else {
                t.check(false);
            }
        }
        Err(_) => t.check(false),
    }
    t.check(UslpFrame::peek_length(bb(frame), &vt) == Ok(18));
    t.check(validate_frame(bb(frame), &vt, DecodeMode::Lenient).is_ok());
    let mut two = [0_u8; 64];
    match UslpFrameWriter::begin(&mut two, &vt, full, content, bb(10)) {
        Ok(mut w) => {
            t.check(w.security_header_mut().is_empty() && w.security_trailer_mut().is_empty());
            let ok =
                crate::vectors::copy(w.tfdz_mut().get_mut(..3).unwrap_or_default(), &[1, 2, 3]);
            t.check(ok && w.finish(bb(3)) == Ok(18));
            t.check(two.get(..18) == Some(frame));
        }
        Err(_) => t.check(false),
    }
    truncated_and_oid(t, &vt, full);
}

/// Truncated frames (USLP-002, USLP-012) and OID frames (USLP-009).
fn truncated_and_oid(t: &mut Tally, vt: &VcTable<2, 2>, full: FrameFields) {
    let Some(ft) = fixed() else {
        t.check(false);
        return;
    };
    let mut out = [0_u8; 64];
    let channel = full.vc_id;
    let tr = ChannelFields {
        spacecraft_id: full.spacecraft_id,
        source_or_destination: full.source_or_destination,
        vc_id: channel,
        map_id: full.map_id,
    };
    let hdr = TfdfHeader {
        rule: TfdzRule::OctetStream,
        upid: Upid::MissionSpecificInformation1,
    };
    let len = encode_truncated(vt, tr, hdr, bb(&[9; 7]), &mut out);
    t.check(len == Ok(12));
    match UslpFrame::decode(
        bb(out.get(..12).unwrap_or_default()),
        vt,
        DecodeMode::Strict,
    ) {
        Ok(dec) => match dec.truncated() {
            Some(view) => {
                t.check(view.tfdz() == [9; 7] && view.len() == 12 && !view.is_empty());
                t.check(
                    view.channel() == tr
                        && view.tfdf_header() == hdr
                        && view.anomalies().is_empty(),
                );
                t.check(view.security_header().is_empty() && view.security_trailer().is_empty());
                t.check(view.octets().len() == 12 && view.header_octets().first() == Some(&0xC0));
            }
            None => t.check(false),
        },
        Err(_) => t.check(false),
    }
    // OID frames (USLP-009).
    let mut oid = OidGenerator::new();
    let mut fixed_out = [0_u8; 40];
    if let Some(of) = fields(63, 0, FrameKind::Expedited) {
        let of = FrameFields { vcf_count: 0, ..of };
        let len = encode_oid(&ft, of, bb(&[1, 2]), None, &mut oid, &mut fixed_out);
        t.check(len == Ok(40));
        let dec = UslpFrame::decode(bb(&fixed_out), &ft, DecodeMode::Strict);
        t.check(dec.is_ok_and(|dec| dec.tfdf_header().upid == Upid::OnlyIdleData));
    }
}

fn delimit_group(t: &mut Tally) {
    let Some(v) = variable() else {
        t.check(false);
        return;
    };
    // A truncated frame on VC 4 MAP 0 (12 octets), then fill.
    let mut unit = [0x55_u8; 16];
    let start = [0xC0, 0x15, 0x50, 0x81, 0xE0];
    for (d, s) in unit.iter_mut().zip(start) {
        *d = s;
    }
    let mut d = FrameDelimiter::new(bb(&unit), &v);
    t.check(
        d.next()
            .is_some_and(|i| matches!(i, Delimited::Frame(f) if f.len() == 12)),
    );
    t.check(d.remaining().len() == 4);
    t.check(
        d.next()
            .is_some_and(|i| matches!(i, Delimited::Fill(_)) && i.len() == 4),
    );
    t.check(d.next().is_none());
    let cut = unit.get(..6).unwrap_or_default();
    t.check(next_frame(bb(cut), &v).is_some_and(|i| {
        matches!(
            i,
            Delimited::IncompleteFrame {
                declared_len: 12,
                ..
            }
        ) && i.octets().len() == 6
            && !i.is_empty()
    }));
    let unknown = [0xC0, 0x15, 0x50, 0xC1];
    t.check(matches!(
        next_frame(bb(&unknown), &v),
        Some(Delimited::NoTruncatedLength(_))
    ));
}

/// Runs the `aldis-uslp` group.
pub fn run(t: &mut Tally) {
    config_group(t);
    tfdf_group(t);
    frame_group(t);
    delimit_group(t);
}

// ----- the vector handler, as `crates/aldis-uslp/tests/vectors.rs` runs it -----

const fn mode(case: &Case) -> DecodeMode {
    if case.lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    }
}

fn frame_type(map: Map, key: &str) -> Result<TransferFrameType, Fail> {
    match map.text(key)? {
        "fixed" => Ok(TransferFrameType::Fixed),
        "variable" => Ok(TransferFrameType::Variable),
        _ => Err(Fail::Vector),
    }
}

fn scid_of(map: Map) -> Result<UslpSpacecraftId, Fail> {
    Ok(UslpSpacecraftId::try_from(
        map.num::<u16>("spacecraft_id")
            .or_else(|_| map.num::<u16>("scid"))?,
    )?)
}

fn vcid_of(map: Map, key: &str) -> Result<UslpVcId, Fail> {
    Ok(UslpVcId::try_from(map.num::<u8>(key)?)?)
}

fn mapid_of(map: Map) -> Result<UslpMapId, Fail> {
    Ok(UslpMapId::try_from(map.num::<u8>("map_id")?)?)
}

fn vc_config(v: Map) -> Result<VcConfig, Fail> {
    let scid = scid_of(v)?;
    let vcid = vcid_of(v, "vcid")?;
    let seq = VcfCountLen::new(v.num("vcf_count_len_sequence")?)?;
    let exp = VcfCountLen::new(v.num("vcf_count_len_expedited")?)?;
    let truncated_frame_len = match v.val("truncated_frame_len")? {
        Val::Null => None,
        t => Some(TruncatedFrameLen::new(
            usize::try_from(need(t.num())?).map_err(|_| Fail::Vector)?,
        )?),
    };
    let security = match v.val("security")? {
        Val::Str("per_map") => SecurityScope::PerMap,
        s => {
            let s = need(s.obj())?;
            SecurityScope::Vc(SecurityLayout::new(
                s.size("header_len")?,
                s.size("trailer_len")?,
            )?)
        }
    };
    let ocf = match v.text("ocf")? {
        "absent" => OcfUse::Absent,
        "allowed" => OcfUse::Allowed,
        "required" => OcfUse::Required,
        _ => return Err(Fail::Vector),
    };
    let params = VcParams {
        frame_type: frame_type(v, "frame_type")?,
        vcf_count_len_sequence: seq,
        vcf_count_len_expedited: exp,
        ocf,
        truncated_frame_len,
        security,
    };
    Ok(VcConfig::new(scid, vcid, params)?)
}

/// The channels of a case's `config`, built through the checked constructors in the order of
/// the vector contract.
fn table(case: &Case) -> Result<Table, Fail> {
    let c = case.config;
    let pc = PhysicalChannelConfig::new(
        frame_type(c, "frame_type")?,
        c.size("frame_len")?,
        c.size("insert_zone_len")?,
        c.flag("fecf")?,
    )?;
    let (s, any_vc, any_map) = (
        need(UslpSpacecraftId::new(0))?,
        need(UslpVcId::new(0))?,
        need(UslpMapId::new(0))?,
    );
    let placeholder = VcConfig::new(s, any_vc, VcParams::new(TransferFrameType::Fixed))?;
    let mut vcs = [placeholder; 17];
    let vc_list = c.list("vcs")?;
    if vc_list.len() > vcs.len() {
        return Err(Fail::Vector);
    }
    for (slot, v) in vcs.iter_mut().zip(vc_list) {
        *slot = vc_config(need(v.obj())?)?;
    }
    let mut maps = [MapConfig::new(s, any_vc, any_map, SecurityLayout::NONE); 33];
    let map_list = c.list("maps")?;
    if map_list.len() > maps.len() {
        return Err(Fail::Vector);
    }
    for (slot, m) in maps.iter_mut().zip(map_list) {
        let m = need(m.obj())?;
        let security = SecurityLayout::new(
            m.size("security_header_len")?,
            m.size("security_trailer_len")?,
        )?;
        *slot = MapConfig::new(scid_of(m)?, vcid_of(m, "vcid")?, mapid_of(m)?, security);
    }
    let vcs = need(vcs.get(..vc_list.len()))?;
    let maps = need(maps.get(..map_list.len()))?;
    Ok(Table::new(pc, vcs, maps)?)
}

/// `usize` → `u64` for a comparison.
fn wide(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

const fn kind_name(k: FrameKind) -> &'static str {
    match k {
        FrameKind::SequenceControlled => "sequence_controlled",
        FrameKind::Expedited => "expedited",
        FrameKind::ProtocolControl => "protocol_control",
    }
}

const fn sod_name(s: SourceOrDestination) -> &'static str {
    match s {
        SourceOrDestination::Source => "source",
        SourceOrDestination::Destination => "destination",
    }
}

const fn pointer_kind(rule: TfdzRule) -> Option<&'static str> {
    match rule {
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(_)) => {
            Some("packet_starts_at")
        }
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd) => {
            Some("no_packet_start_or_end")
        }
        TfdzRule::SduStart(LastValidOctet::At(_))
        | TfdzRule::SduContinuation(LastValidOctet::At(_)) => Some("last_valid_octet"),
        TfdzRule::SduStart(LastValidOctet::SduContinues)
        | TfdzRule::SduContinuation(LastValidOctet::SduContinues) => Some("sdu_continues"),
        _ => None,
    }
}

fn decoded(o: &mut Out, f: &UslpFrame<'_>) {
    let tfdf = f.tfdf_header();
    o.flag("truncated", f.is_truncated());
    o.num("spacecraft_id", f.spacecraft_id().get());
    o.text("source_or_destination", sod_name(f.source_or_destination()));
    o.num("vc_id", f.vc_id().get());
    o.num("map_id", f.map_id().get());
    o.num("frame_len", wide(f.len()));
    o.bytes("security_header_hex", f.security_header());
    o.num("tfdz_construction_rule", tfdf.rule.rule().bits());
    o.num("upid", tfdf.upid.bits());
    o.flag("upid_assigned", !matches!(tfdf.upid, Upid::Other(_)));
    o.bytes("tfdz_hex", f.tfdz());
    o.bytes("security_trailer_hex", f.security_trailer());
    o.num("consumed", wide(f.len()));
    o.names("anomalies", f.anomalies().iter());
    if let Some(full) = f.full() {
        let h = full.header();
        o.text("kind", kind_name(h.kind()));
        o.flag("ocf_flag", h.ocf_flag());
        o.num("vcf_count_len", h.vcf_count().len().get());
        o.num("vcf_count", h.vcf_count().value());
        o.bytes("insert_zone_hex", full.insert_zone());
        o.opt_num("pointer", tfdf.rule.pointer());
        o.opt_text("pointer_kind", pointer_kind(tfdf.rule));
        match full.ocf() {
            Some(c) => o.bytes("ocf_hex", &c.to_octets()),
            None => o.null("ocf_hex"),
        }
        match full.fecf() {
            Some(v) => o.bytes("fecf_hex", &v.to_be_bytes()),
            None => o.null("fecf_hex"),
        }
    }
}

fn channel(p: Map) -> Result<ChannelFields, Fail> {
    Ok(ChannelFields {
        spacecraft_id: scid_of(p)?,
        source_or_destination: match p.text("source_or_destination")? {
            "source" => SourceOrDestination::Source,
            "destination" => SourceOrDestination::Destination,
            _ => return Err(Fail::Vector),
        },
        vc_id: vcid_of(p, "vc_id")?,
        map_id: mapid_of(p)?,
    })
}

fn frame_fields(p: Map) -> Result<FrameFields, Fail> {
    let ch = channel(p)?;
    Ok(FrameFields {
        spacecraft_id: ch.spacecraft_id,
        source_or_destination: ch.source_or_destination,
        vc_id: ch.vc_id,
        map_id: ch.map_id,
        kind: match p.text("kind")? {
            "sequence_controlled" => FrameKind::SequenceControlled,
            "expedited" => FrameKind::Expedited,
            "protocol_control" => FrameKind::ProtocolControl,
            _ => return Err(Fail::Vector),
        },
        vcf_count: p.num("vcf_count")?,
    })
}

fn tfdf(p: Map) -> Result<TfdfHeader, Fail> {
    let rule = ConstructionRule::from_bits(p.num("tfdz_construction_rule")?);
    let pointer = match crate::vectors::get(p, "pointer") {
        None | Some(Val::Null) => None,
        Some(v) => Some(u16::try_from(need(v.num())?).map_err(|_| Fail::Vector)?),
    };
    if rule.has_pointer() != pointer.is_some() {
        return Err(Fail::Vector);
    }
    Ok(TfdfHeader {
        rule: TfdzRule::new(rule, pointer.unwrap_or(0)),
        upid: need(Upid::from_bits(p.num("upid")?))?,
    })
}

fn ocf(p: Map) -> Result<Option<Ocf>, Fail> {
    match p.val("ocf_hex")? {
        Val::Null => Ok(None),
        v => {
            let octets = need(need(v.bytes())?.first_chunk::<4>())?;
            Ok(Some(Ocf::decode(*octets)))
        }
    }
}

fn encode_case(case: &Case, o: &mut Out, scratch: &mut Scratch) -> Run {
    let tbl = table(case)?;
    let params = case.params;
    let fields = frame_fields(params)?;
    let content = FrameContent {
        tfdf: tfdf(params)?,
        insert_zone: params.bytes("insert_zone_hex")?,
        ocf: ocf(params)?,
    };
    let zone = params.bytes("tfdz_hex")?;
    let n = encode(&tbl, fields, content, bb(zone), &mut scratch.a)?;
    // The two-phase writer must produce the same octets (API-020).
    let two = need(scratch.b.get_mut(..n))?;
    let mut w = UslpFrameWriter::begin(two, &tbl, fields, content, zone.len())?;
    if !crate::vectors::copy(w.tfdz_mut(), zone) || w.finish(zone.len()) != Ok(n) {
        return Err(Fail::Vector);
    }
    if scratch.b.get(..n) != scratch.a.get(..n) {
        return Err(Fail::Vector);
    }
    o.bytes("output_hex", need(scratch.a.get(..n))?);
    Ok(())
}

fn encode_truncated_case(case: &Case, o: &mut Out, scratch: &mut Scratch) -> Run {
    let t = table(case)?;
    let p = case.params;
    let out = need(scratch.a.get_mut(..64))?;
    let n = encode_truncated(&t, channel(p)?, tfdf(p)?, bb(p.bytes("tfdz_hex")?), out)?;
    o.bytes("output_hex", need(scratch.a.get(..n))?);
    Ok(())
}

fn encode_oid_case(case: &Case, o: &mut Out, scratch: &mut Scratch) -> Run {
    let t = table(case)?;
    let p = case.params;
    let mut oid = OidGenerator::new();
    for _ in 0..p.size("oid_skip_octets")? {
        oid.next_octet();
    }
    let iz = p.bytes("insert_zone_hex")?;
    let n = encode_oid(
        &t,
        frame_fields(p)?,
        bb(iz),
        ocf(p)?,
        &mut oid,
        &mut scratch.a,
    )?;
    o.bytes("output_hex", need(scratch.a.get(..n))?);
    Ok(())
}

fn validate_case(case: &Case, o: &mut Out) -> Run {
    let t = table(case)?;
    match validate_frame(bb(case.input), &t, mode(case)) {
        Ok(f) => {
            o.flag("accepted", true);
            o.num("frame_len", wide(f.len()));
        }
        Err(r) => {
            o.flag("accepted", false);
            let mut name = Text::new();
            if write!(name, "{:?}", r.reason()).is_err() {
                return Err(Fail::Vector);
            }
            o.bytes("reason", name.as_bytes());
            o.num("reason_code", r.reason().code());
            o.text("error_kind", r.error().kind().name());
            o.opt_num("error_field", r.error().field().map(|f| f.code().get()));
        }
    }
    Ok(())
}

fn delimit_case(case: &Case, o: &mut Out) -> Run {
    let t = table(case)?;
    let mut list = o.list("items");
    let mut at = 0_usize;
    for item in FrameDelimiter::new(bb(case.input), &t) {
        let (kind, declared) = match item {
            Delimited::Frame(_) => ("frame", None),
            Delimited::IncompleteFrame { declared_len, .. } => {
                ("incomplete", Some(wide(declared_len)))
            }
            Delimited::NoTruncatedLength(_) => ("no_truncated_length", None),
            Delimited::Fill(_) => ("fill", None),
        };
        list.obj(|i| {
            i.text("kind", kind);
            i.num("offset", wide(at));
            i.bytes("octets_hex", item.octets());
            i.opt_num("declared_len", declared);
        });
        at = at.saturating_add(item.len());
    }
    o.end(&list);
    Ok(())
}

/// One `test-vectors/uslp/` case, as `crates/aldis-uslp/tests/vectors.rs` runs it.
pub fn vector(case: &Case, o: &mut Out, scratch: &mut Scratch) -> Run {
    match case.op {
        "config" => {
            let t = table(case)?;
            o.num("vcs", wide(t.vcs().count()));
            o.num("maps", wide(t.maps().count()));
        }
        "decode" => {
            let t = table(case)?;
            decoded(o, &UslpFrame::decode(bb(case.input), &t, mode(case))?);
        }
        "peek_length" => {
            let t = table(case)?;
            o.num(
                "frame_len",
                wide(UslpFrame::peek_length(bb(case.input), &t)?),
            );
        }
        "encode" => encode_case(case, o, scratch)?,
        "encode_truncated" => encode_truncated_case(case, o, scratch)?,
        "encode_oid" => encode_oid_case(case, o, scratch)?,
        "validate" => validate_case(case, o)?,
        "delimit" => delimit_case(case, o)?,
        _ => return Err(Fail::Vector),
    }
    Ok(())
}
