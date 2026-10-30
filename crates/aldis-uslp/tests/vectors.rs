//! Runs the shared conformance vectors in `test-vectors/uslp/` against `aldis-uslp` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_core::{AnomalySet, DecodeMode, Error, FieldId};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use aldis_uslp::{
    ChannelFields, ConstructionRule, Delimited, FirstHeaderPointer, FrameContent, FrameDelimiter,
    FrameFields, FrameKind, LastValidOctet, MapConfig, OcfUse, PhysicalChannelConfig,
    SecurityScope, SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen,
    Upid, UslpAnomaly, UslpFrame, UslpFrameWriter, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig,
    VcParams, VcTable, VcfCountLen, encode, encode_oid, encode_truncated, validate_frame,
    validate_frame_lenient,
};
use serde_json::{Map, Value, json};

/// The capacities the vectors' configurations assume.
type Table = VcTable<16, 32>;

fn input(case: &CodecCase) -> Vec<u8> {
    hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap()
}

fn mode(case: &CodecCase) -> DecodeMode {
    match case.mode.unwrap_or_default() {
        Mode::Strict => DecodeMode::Strict,
        Mode::Lenient => DecodeMode::Lenient,
    }
}

fn num<T: TryFrom<u64>>(v: &Value) -> T {
    T::try_from(v.as_u64().unwrap()).unwrap_or_else(|_| panic!("{v} out of range"))
}

fn frame_type(v: &Value) -> TransferFrameType {
    match v.as_str().unwrap() {
        "fixed" => TransferFrameType::Fixed,
        "variable" => TransferFrameType::Variable,
        other => panic!("frame type {other}"),
    }
}

/// The channels of a case's `config`, built through the checked constructors in the order the
/// vector contract gives.
fn table(case: &CodecCase) -> Result<Table, Error> {
    let c = Value::Object(case.config.clone().unwrap());
    let pc = PhysicalChannelConfig::new(
        frame_type(&c["frame_type"]),
        num(&c["frame_len"]),
        num(&c["insert_zone_len"]),
        c["fecf"].as_bool().unwrap(),
    )?;
    let mut vcs = Vec::new();
    for v in c["vcs"].as_array().unwrap() {
        let scid = UslpSpacecraftId::try_from(num::<u16>(&v["scid"]))?;
        let vcid = UslpVcId::try_from(num::<u8>(&v["vcid"]))?;
        let seq = VcfCountLen::new(num(&v["vcf_count_len_sequence"]))?;
        let exp = VcfCountLen::new(num(&v["vcf_count_len_expedited"]))?;
        let truncated = match &v["truncated_frame_len"] {
            Value::Null => None,
            t => Some(TruncatedFrameLen::new(num(t))?),
        };
        let security = match &v["security"] {
            Value::String(s) if s == "per_map" => SecurityScope::PerMap,
            s => SecurityScope::Vc(SecurityLayout::new(
                num(&s["header_len"]),
                num(&s["trailer_len"]),
            )?),
        };
        let ocf = match v["ocf"].as_str().unwrap() {
            "absent" => OcfUse::Absent,
            "allowed" => OcfUse::Allowed,
            "required" => OcfUse::Required,
            other => panic!("ocf {other}"),
        };
        let params = VcParams {
            frame_type: frame_type(&v["frame_type"]),
            vcf_count_len_sequence: seq,
            vcf_count_len_expedited: exp,
            ocf,
            truncated_frame_len: truncated,
            security,
        };
        vcs.push(VcConfig::new(scid, vcid, params)?);
    }
    let mut maps = Vec::new();
    for m in c["maps"].as_array().unwrap() {
        let scid = UslpSpacecraftId::try_from(num::<u16>(&m["scid"]))?;
        let vcid = UslpVcId::try_from(num::<u8>(&m["vcid"]))?;
        let map_id = UslpMapId::try_from(num::<u8>(&m["map_id"]))?;
        let security = SecurityLayout::new(
            num(&m["security_header_len"]),
            num(&m["security_trailer_len"]),
        )?;
        maps.push(MapConfig::new(scid, vcid, map_id, security));
    }
    Table::new(pc, &vcs, &maps)
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

fn pointer_kind(rule: TfdzRule) -> Value {
    match rule {
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(_)) => {
            json!("packet_starts_at")
        }
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd) => {
            json!("no_packet_start_or_end")
        }
        TfdzRule::SduStart(LastValidOctet::At(_))
        | TfdzRule::SduContinuation(LastValidOctet::At(_)) => json!("last_valid_octet"),
        TfdzRule::SduStart(LastValidOctet::SduContinues)
        | TfdzRule::SduContinuation(LastValidOctet::SduContinues) => json!("sdu_continues"),
        _ => Value::Null,
    }
}

fn decoded(f: &UslpFrame<'_>, anomalies: AnomalySet<UslpAnomaly>) -> Value {
    let tfdf = f.tfdf_header();
    let reserve_spares = anomalies.contains(UslpAnomaly::ReserveSparesSet);
    let anomalies: Vec<String> = anomalies.iter().map(|a| format!("{a:?}")).collect();
    let mut v = json!({
        "truncated": f.is_truncated(),
        "spacecraft_id": f.spacecraft_id().get(),
        "source_or_destination": sod_name(f.source_or_destination()),
        "vc_id": f.vc_id().get(),
        "map_id": f.map_id().get(),
        "frame_len": f.len(),
        "security_header_hex": hex::encode(f.security_header()),
        "tfdz_construction_rule": tfdf.rule.rule().bits(),
        "upid": tfdf.upid.bits(),
        "upid_assigned": !matches!(tfdf.upid, Upid::Other(_)),
        "tfdz_hex": hex::encode(f.tfdz()),
        "security_trailer_hex": hex::encode(f.security_trailer()),
        "consumed": f.len(),
        "anomalies": anomalies,
    });
    if let Some(full) = f.full() {
        let h = full.header();
        let extra = json!({
            "kind": kind_name(h.kind()),
            "frame_len": h.frame_len(),
            "ocf_flag": h.ocf_flag(),
            "vcf_count_len": h.vcf_count().len().get(),
            "vcf_count": h.vcf_count().value(),
            "insert_zone_hex": hex::encode(full.insert_zone()),
            "pointer": tfdf.rule.pointer(),
            "pointer_kind": pointer_kind(tfdf.rule),
            "ocf_hex": full.ocf().map(|o| hex::encode(&o.to_octets())),
            "fecf_hex": full.fecf().map(|v| format!("{v:04X}")),
        });
        if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
            m.extend(e);
        }
        // The owned header re-encodes to the frame's first octets, with the Reserve Spares
        // `00` (API-014).
        if !reserve_spares {
            assert_eq!(h.octets()[..h.len()], full.octets()[..h.len()]);
        }
    } else {
        let t = f.truncated().unwrap();
        assert_eq!(t.header_octets(), t.octets()[..4]);
    }
    v
}

fn sod(p: &Map<String, Value>) -> SourceOrDestination {
    match p["source_or_destination"].as_str().unwrap() {
        "source" => SourceOrDestination::Source,
        "destination" => SourceOrDestination::Destination,
        other => panic!("source_or_destination {other}"),
    }
}

fn channel(p: &Map<String, Value>) -> Result<ChannelFields, Error> {
    Ok(ChannelFields {
        spacecraft_id: UslpSpacecraftId::try_from(num::<u16>(&p["spacecraft_id"]))?,
        source_or_destination: sod(p),
        vc_id: UslpVcId::try_from(num::<u8>(&p["vc_id"]))?,
        map_id: UslpMapId::try_from(num::<u8>(&p["map_id"]))?,
    })
}

fn fields(p: &Map<String, Value>) -> Result<FrameFields, Error> {
    let ch = channel(p)?;
    Ok(FrameFields {
        spacecraft_id: ch.spacecraft_id,
        source_or_destination: ch.source_or_destination,
        vc_id: ch.vc_id,
        map_id: ch.map_id,
        kind: match p["kind"].as_str().unwrap() {
            "sequence_controlled" => FrameKind::SequenceControlled,
            "expedited" => FrameKind::Expedited,
            "protocol_control" => FrameKind::ProtocolControl,
            other => panic!("kind {other}"),
        },
        vcf_count: num(&p["vcf_count"]),
    })
}

fn tfdf(p: &Map<String, Value>) -> TfdfHeader {
    let rule = ConstructionRule::from_bits(num(&p["tfdz_construction_rule"]));
    let pointer = &p.get("pointer").cloned().unwrap_or(Value::Null);
    assert_eq!(
        rule.has_pointer(),
        !pointer.is_null(),
        "pointer exactly for 000-010"
    );
    TfdfHeader {
        rule: TfdzRule::new(rule, if pointer.is_null() { 0 } else { num(pointer) }),
        upid: Upid::from_bits(num(&p["upid"])).unwrap(),
    }
}

fn ocf(p: &Map<String, Value>) -> Option<Ocf> {
    p["ocf_hex"]
        .as_str()
        .map(|h| Ocf::decode(hex::decode(h).unwrap().try_into().unwrap()))
}

fn bytes_of(p: &Map<String, Value>, key: &str) -> Vec<u8> {
    hex::decode(p[key].as_str().unwrap()).unwrap()
}

fn encode_case(case: &CodecCase) -> Result<Value, Error> {
    let tbl = table(case)?;
    let params = case.params.as_ref().unwrap();
    let frame_fields = fields(params)?;
    let iz = bytes_of(params, "insert_zone_hex");
    let content = FrameContent {
        tfdf: tfdf(params),
        insert_zone: &iz,
        ocf: ocf(params),
    };
    let tfdz = bytes_of(params, "tfdz_hex");
    let mut out = vec![0_u8; 70_000];
    let len = encode(&tbl, frame_fields, content, &tfdz, &mut out)?;
    // The two-phase writer produces the same octets (API-020).
    let mut two = vec![0xA5_u8; len];
    let mut writer =
        UslpFrameWriter::begin(&mut two, &tbl, frame_fields, content, tfdz.len()).unwrap();
    writer.tfdz_mut().unwrap().copy_from_slice(&tfdz);
    let ctx = FrameCtx::Uslp {
        scid: frame_fields.spacecraft_id,
        vcid: frame_fields.vc_id,
        map_id: Some(frame_fields.map_id),
    };
    let done = writer
        .protect(tfdz.len(), &mut PreFilledSecurity, &ctx)?
        .finalize()
        .unwrap();
    assert_eq!(done.len(), len, "{}", case.id);
    assert_eq!(two, out[..len], "{}: two-phase writer differs", case.id);
    Ok(json!({ "output_hex": hex::encode(&out[..len]) }))
}

fn encode_truncated_case(case: &CodecCase) -> Result<Value, Error> {
    let t = table(case)?;
    let p = case.params.as_ref().unwrap();
    let mut out = [0_u8; 64];
    let n = encode_truncated(&t, channel(p)?, tfdf(p), &bytes_of(p, "tfdz_hex"), &mut out)?;
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

fn encode_oid_case(case: &CodecCase) -> Result<Value, Error> {
    let t = table(case)?;
    let p = case.params.as_ref().unwrap();
    let mut oid = OidGenerator::new();
    for _ in 0..num::<usize>(&p["oid_skip_octets"]) {
        oid.next_octet();
    }
    let mut out = vec![0_u8; 70_000];
    let iz = bytes_of(p, "insert_zone_hex");
    let n = encode_oid(&t, fields(p)?, &iz, ocf(p), &mut oid, &mut out)?;
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

fn validate_case(case: &CodecCase) -> Result<Value, Error> {
    let t = table(case)?;
    let bytes = input(case);
    let validated = match mode(case) {
        DecodeMode::Strict => validate_frame(&bytes, &t).map(|f| f.len()),
        DecodeMode::Lenient => validate_frame_lenient(&bytes, &t).map(|f| f.len()),
    };
    Ok(match validated {
        Ok(len) => json!({ "accepted": true, "frame_len": len }),
        Err(r) => json!({
            "accepted": false,
            "reason": format!("{:?}", r.reason()),
            "reason_code": r.reason().code(),
            "error_kind": r.error().kind().name(),
            "error_field": r.error().field().map(FieldId::code),
        }),
    })
}

fn delimit_case(case: &CodecCase) -> Result<Value, Error> {
    let t = table(case)?;
    let unit = input(case);
    let mut at = 0;
    // The released cases hold any fill: the bound of OI-47 is tested in `delimit.rs`.
    let items: Vec<Value> = FrameDelimiter::new(&unit, &t, usize::MAX)
        .map(|item| {
            let (kind, declared) = match item {
                Delimited::Frame(_) => ("frame", Value::Null),
                Delimited::IncompleteFrame { declared_len, .. } => {
                    ("incomplete", json!(declared_len))
                }
                Delimited::NoTruncatedLength(_) => ("no_truncated_length", Value::Null),
                Delimited::NonTruncatedNotAllowed(_) => ("non_truncated_not_allowed", Value::Null),
                Delimited::Fill(_) => ("fill", Value::Null),
                Delimited::LeftoverFill(_) => ("leftover_fill", Value::Null),
            };
            let v = json!({
                "kind": kind,
                "offset": at,
                "octets_hex": hex::encode(item.octets()),
                "declared_len": declared,
            });
            at += item.len();
            v
        })
        .collect();
    Ok(json!({ "items": items }))
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    match case.operation.as_str() {
        "config" => {
            let t = table(case)?;
            Ok(json!({ "vcs": t.vcs().count(), "maps": t.maps().count() }))
        }
        "decode" => {
            let t = table(case)?;
            let bytes = input(case);
            mode(case)
                .dispatch(
                    || UslpFrame::decode(&bytes, &t),
                    || UslpFrame::decode_lenient(&bytes, &t),
                )
                .map(|(f, anomalies)| decoded(&f, anomalies))
        }
        "peek_length" => {
            let t = table(case)?;
            UslpFrame::peek_length(&input(case), &t).map(|n| json!({ "frame_len": n }))
        }
        "encode" => encode_case(case),
        "encode_truncated" => encode_truncated_case(case),
        "encode_oid" => encode_oid_case(case),
        "validate" => validate_case(case),
        "delimit" => delimit_case(case),
        other => panic!("{}: operation {other}", case.id),
    }
}

fn handler(case: &CodecCase) -> Outcome {
    match run(case) {
        Ok(Value::Object(map)) => Outcome::Ok(map),
        Ok(other) => panic!("{}: not an object: {other}", case.id),
        Err(e) => Outcome::Err {
            kind: e.kind().name().to_owned(),
            field: e.field().map(FieldId::code),
        },
    }
}

#[test]
fn fixed_packets_rule000() {
    // req: USLP-003, USLP-005, USLP-006, API-012, API-013, API-070, API-072, VER-021
    run_codec_file("uslp/fixed_packets_rule000.json", handler).assert_passed();
}

#[test]
fn fixed_mapa_rules001_010() {
    // req: USLP-005, USLP-006, USLP-016, VER-021
    run_codec_file("uslp/fixed_mapa_rules001_010.json", handler).assert_passed();
}

#[test]
fn variable_rules011_111() {
    // req: USLP-003, USLP-005, USLP-016, API-011, VER-021
    run_codec_file("uslp/variable_rules011_111.json", handler).assert_passed();
}

#[test]
fn vcf_count_lengths_0_to_7() {
    // req: USLP-004, USLP-010, VER-021
    run_codec_file("uslp/vcf_count_lengths_0_to_7.json", handler).assert_passed();
}

#[test]
fn truncated() {
    // req: USLP-002, USLP-016, API-013, VER-021
    run_codec_file("uslp/truncated.json", handler).assert_passed();
}

#[test]
fn truncated_sdls_layout() {
    // req: USLP-012, API-075, VER-021
    run_codec_file("uslp/truncated_sdls_layout.json", handler).assert_passed();
}

#[test]
fn oid() {
    // req: USLP-009, CMN-020, CMN-021, API-032, VER-021
    run_codec_file("uslp/oid.json", handler).assert_passed();
}

#[test]
fn insert_zone() {
    // req: USLP-008, VER-021
    run_codec_file("uslp/insert_zone.json", handler).assert_passed();
}

#[test]
fn protocol_control_cop1() {
    // req: USLP-001, USLP-013, VER-021
    run_codec_file("uslp/protocol_control_cop1.json", handler).assert_passed();
}

#[test]
fn delimit_multiple_frames_with_fill() {
    // req: USLP-015, VER-021
    run_codec_file("uslp/delimit_multiple_frames_with_fill.json", handler).assert_passed();
}

#[test]
fn validation_failures() {
    // req: USLP-014, VER-021
    run_codec_file("uslp/validation_failures.json", handler).assert_passed();
}

#[test]
fn invalid() {
    // req: USLP-001, USLP-004, USLP-006, API-031, API-032, VER-021
    run_codec_file("uslp/invalid.json", handler).assert_passed();
}

#[test]
fn config() {
    // req: GEN-060, API-040, USLP-002, USLP-008, USLP-012, VER-021
    run_codec_file("uslp/config.json", handler).assert_passed();
}
