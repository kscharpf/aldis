//! Runs the shared conformance vectors in `test-vectors/tc/` against `aldis-tc` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::ids::FrameSeqNum;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_core::{AnomalySet, DecodeMode, Error, FieldId};
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, Delimited, FarmBCounter, FrameData,
    FrameDelimiter, FrameFields, FrameType, MapConfig, PhysicalChannelConfig, SegmentHeader,
    SegmentHeaderUse, SequenceFlags, TcAnomaly, TcFrame, TcFrameWriter, TcMapId, TcSpacecraftId,
    TcVcId, VcConfig, VcTable, encode, validate_frame, validate_frame_lenient,
};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use serde_json::{Map, Value, json};

/// Room for every channel the vectors configure.
type Table = VcTable<8, 8>;

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

fn opt_num(map: &Value, key: &str) -> usize {
    map.get(key).map_or(0, num)
}

fn security(v: &Value) -> Result<SecurityLayout, Error> {
    SecurityLayout::new(
        opt_num(v, "security_header_len"),
        opt_num(v, "security_trailer_len"),
    )
}

/// The channels of a case's `config`, built through the checked constructors.
fn table(case: &CodecCase) -> Result<Table, Error> {
    let c = Value::Object(case.config.clone().unwrap());
    let pc = PhysicalChannelConfig::new(num(&c["max_frame_len"]), c["fecf"].as_bool().unwrap())?;
    let mut vcs = Vec::new();
    for v in c["vcs"].as_array().unwrap() {
        let segment = if v["segment_header"].as_bool().unwrap() {
            SegmentHeaderUse::Present
        } else {
            SegmentHeaderUse::Absent(security(v)?)
        };
        vcs.push(VcConfig::new(
            TcSpacecraftId::try_from(num::<u16>(&v["scid"]))?,
            TcVcId::try_from(num::<u8>(&v["vcid"]))?,
            num(&v["max_frame_len"]),
            segment,
        )?);
    }
    let mut maps = Vec::new();
    for m in c["maps"].as_array().unwrap() {
        maps.push(MapConfig::new(
            TcSpacecraftId::try_from(num::<u16>(&m["scid"]))?,
            TcVcId::try_from(num::<u8>(&m["vcid"]))?,
            TcMapId::try_from(num::<u8>(&m["map_id"]))?,
            security(m)?,
        ));
    }
    Table::new(pc, &vcs, &maps)
}

const fn frame_type_name(t: FrameType) -> &'static str {
    match t {
        FrameType::Ad => "AD",
        FrameType::Bd => "BD",
        FrameType::Bc => "BC",
    }
}

fn decoded(f: &TcFrame<'_>, anomalies: AnomalySet<TcAnomaly>) -> Value {
    let (command, vr) = match f.control_command() {
        None => (Value::Null, Value::Null),
        Some(ControlCommand::Unlock) => (json!("unlock"), Value::Null),
        Some(ControlCommand::SetVr(v)) => (json!("set_vr"), json!(v.get())),
    };
    json!({
        "frame_type": frame_type_name(f.frame_type()),
        "bypass_flag": u8::from(f.frame_type().bypass_flag()),
        "control_command_flag": u8::from(f.frame_type().control_command_flag()),
        "spacecraft_id": f.spacecraft_id().get(),
        "vc_id": f.vc_id().get(),
        "frame_len": f.header().frame_len(),
        "sequence_number": f.sequence_number().get(),
        "segment_header": f.segment_header().map(|sh| json!({
            "sequence_flags": sh.sequence_flags.bits(),
            "map_id": sh.map_id.get(),
        })),
        "security_header_hex": hex::encode(f.security_header()),
        "data_hex": hex::encode(f.data_field()),
        "security_trailer_hex": hex::encode(f.security_trailer()),
        "fecf_hex": f.fecf().map(|v| format!("{v:04X}")),
        "control_command": command,
        "set_vr_value": vr,
        "consumed": f.len(),
        "anomalies": anomalies.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
    })
}

fn fields(p: &Map<String, Value>) -> Result<FrameFields, Error> {
    Ok(FrameFields {
        frame_type: match p["frame_type"].as_str().unwrap() {
            "AD" => FrameType::Ad,
            "BD" => FrameType::Bd,
            "BC" => FrameType::Bc,
            other => panic!("frame type {other}"),
        },
        spacecraft_id: TcSpacecraftId::try_from(num::<u16>(&p["spacecraft_id"]))?,
        vc_id: TcVcId::try_from(num::<u8>(&p["vc_id"]))?,
        sequence_number: FrameSeqNum::new(num(&p["sequence_number"])),
    })
}

fn encode_control(
    channels: &Table,
    frame_fields: FrameFields,
    p: &Map<String, Value>,
    cmd: &str,
    out: &mut [u8],
) -> Result<usize, Error> {
    let command = match cmd {
        "unlock" => ControlCommand::Unlock,
        "set_vr" => ControlCommand::SetVr(FrameSeqNum::new(num(&p["set_vr_value"]))),
        other => panic!("control command {other}"),
    };
    encode(channels, frame_fields, FrameData::Control(command), out)
}

fn encode_fdu(
    case: &CodecCase,
    channels: &Table,
    frame_fields: FrameFields,
    out: &mut [u8],
) -> Result<usize, Error> {
    let p = case.params.as_ref().unwrap();
    let sh = &p["segment_header"];
    let segment = if sh.is_null() {
        None
    } else {
        Some(SegmentHeader {
            sequence_flags: SequenceFlags::from_bits(num(&sh["sequence_flags"])),
            map_id: TcMapId::try_from(num::<u8>(&sh["map_id"]))?,
        })
    };
    let user = hex::decode(p["user_data_hex"].as_str().unwrap()).unwrap();
    let fdu = FrameData::Fdu {
        segment_header: segment,
        user_data: &user,
    };
    let n = encode(channels, frame_fields, fdu, out)?;
    // The two-phase writer must produce the same octets (API-020).
    let mut two = vec![0_u8; n];
    let mut w = TcFrameWriter::begin(&mut two, channels, frame_fields, segment, user.len())?;
    w.user_data_mut().copy_from_slice(&user);
    let ctx = FrameCtx::Tc {
        scid: frame_fields.spacecraft_id,
        vcid: frame_fields.vc_id,
        map_id: segment.map(|s| s.map_id),
    };
    let done = w
        .protect(user.len(), &mut PreFilledSecurity, &ctx)?
        .finalize()
        .unwrap();
    assert_eq!(done.len(), n, "{}", case.id);
    assert_eq!(two, out[..n], "{}: two-phase writer differs", case.id);
    Ok(n)
}

fn encode_case(case: &CodecCase) -> Result<Value, Error> {
    let channels = table(case)?;
    let p = case.params.as_ref().unwrap();
    let frame_fields = fields(p)?;
    let mut out = vec![0_u8; 1100];
    let n = if let Some(cmd) = p.get("control_command").and_then(Value::as_str) {
        encode_control(&channels, frame_fields, p, cmd, &mut out)?
    } else {
        encode_fdu(case, &channels, frame_fields, &mut out)?
    };
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

fn validate_case(case: &CodecCase) -> Result<Value, Error> {
    let t = table(case)?;
    let bytes = input(case);
    let result = match mode(case) {
        DecodeMode::Strict => validate_frame(&bytes, &t).map(|f| f.len()),
        DecodeMode::Lenient => validate_frame_lenient(&bytes, &t).map(|f| f.len()),
    };
    Ok(match result {
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

fn delimit_case(case: &CodecCase) -> Value {
    let unit = input(case);
    let mut at = 0;
    let items: Vec<Value> = FrameDelimiter::new(&unit)
        .map(|item| {
            let (kind, declared) = match item {
                Delimited::Frame(_) => ("frame", Value::Null),
                Delimited::TruncatedFrame { declared_len, .. } => {
                    ("truncated", json!(declared_len))
                }
                Delimited::Fill(_) => ("fill", Value::Null),
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
    json!({ "items": items })
}

fn clcw_value(c: &Clcw, anomalies: &[String]) -> Value {
    json!({
        "status": c.status.get(),
        "cop_in_effect": c.cop_in_effect.get(),
        "vc_id": c.vc_id.get(),
        "no_rf_available": c.no_rf_available,
        "no_bit_lock": c.no_bit_lock,
        "lockout": c.lockout,
        "wait": c.wait,
        "retransmit": c.retransmit,
        "farm_b_counter": c.farm_b_counter.get(),
        "report_value": c.report_value.get(),
        "anomalies": anomalies,
    })
}

fn clcw_encode(case: &CodecCase) -> Result<Value, Error> {
    let p = case.params.as_ref().unwrap();
    let flag = |k: &str| p[k].as_bool().unwrap();
    let c = Clcw {
        status: ClcwStatus::try_from(num::<u8>(&p["status"]))?,
        cop_in_effect: CopInEffect::try_from(num::<u8>(&p["cop_in_effect"]))?,
        vc_id: TcVcId::try_from(num::<u8>(&p["vc_id"]))?,
        no_rf_available: flag("no_rf_available"),
        no_bit_lock: flag("no_bit_lock"),
        lockout: flag("lockout"),
        wait: flag("wait"),
        retransmit: flag("retransmit"),
        farm_b_counter: FarmBCounter::try_from(num::<u8>(&p["farm_b_counter"]))?,
        report_value: FrameSeqNum::new(num(&p["report_value"])),
    };
    assert_eq!(c.to_word().to_be_bytes(), c.to_octets(), "{}", case.id);
    Ok(json!({ "output_hex": hex::encode(&c.to_octets()) }))
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    let bytes = input(case);
    match case.operation.as_str() {
        "config" => {
            let t = table(case)?;
            Ok(json!({ "vcs": t.vcs().count(), "maps": t.maps().count() }))
        }
        "decode" => {
            let t = table(case)?;
            mode(case)
                .dispatch(
                    || TcFrame::decode(&bytes, &t),
                    || TcFrame::decode_lenient(&bytes, &t),
                )
                .map(|(f, anomalies)| decoded(&f, anomalies))
        }
        "peek_length" => TcFrame::peek_length(&bytes).map(|n| json!({ "frame_len": n })),
        "encode" => encode_case(case),
        "validate" => validate_case(case),
        "delimit" => Ok(delimit_case(case)),
        "clcw_decode" => {
            let octets: [u8; 4] = bytes.try_into().unwrap();
            mode(case)
                .dispatch(|| Clcw::decode(octets), || Clcw::decode_lenient(octets))
                .map(|(c, a)| {
                    clcw_value(&c, &a.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>())
                })
        }
        "clcw_encode" => clcw_encode(case),
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
fn ad() {
    // req: TC-001, TC-006, TC-007, API-011, API-013, API-070, API-072, VER-021
    run_codec_file("tc/ad.json", handler).assert_passed();
}

#[test]
fn bd() {
    // req: TC-001, TC-008, VER-021
    run_codec_file("tc/bd.json", handler).assert_passed();
}

#[test]
fn bc_unlock() {
    // req: TC-004, TC-005, VER-021
    run_codec_file("tc/bc_unlock.json", handler).assert_passed();
}

#[test]
fn bc_setvr() {
    // req: TC-005, API-031, API-032, VER-021
    run_codec_file("tc/bc_setvr.json", handler).assert_passed();
}

#[test]
fn segment_header() {
    // req: TC-004, TC-022, API-070, VER-021
    run_codec_file("tc/segment_header.json", handler).assert_passed();
}

#[test]
fn max_length() {
    // req: TC-007, GEN-060, VER-021
    run_codec_file("tc/max_length.json", handler).assert_passed();
}

#[test]
fn delimit_multiple_frames_with_fill() {
    // req: TC-020, VER-021
    run_codec_file("tc/delimit_multiple_frames_with_fill.json", handler).assert_passed();
}

#[test]
fn validation_failures() {
    // req: TC-021, TC-022, TC-023, VER-021
    run_codec_file("tc/validation_failures.json", handler).assert_passed();
}

#[test]
fn decode_errors() {
    // req: TC-001, TC-002, TC-003, TC-006, API-031, API-032, VER-021
    run_codec_file("tc/decode_errors.json", handler).assert_passed();
}

#[test]
fn config() {
    // req: GEN-060, API-040, VER-021
    run_codec_file("tc/config.json", handler).assert_passed();
}

#[test]
fn clcw() {
    // req: TC-030, TC-031, VER-021
    run_codec_file("tc/clcw.json", handler).assert_passed();
}
