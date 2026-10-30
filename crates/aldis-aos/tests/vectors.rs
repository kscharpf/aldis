//! Runs the shared conformance vectors in `test-vectors/aos/` against `aldis-aos` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_aos::{
    AosAnomaly, AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosFrameWriter,
    AosPhysicalChannelConfig, AosPrimaryHeader, AosVirtualChannelConfig, BitstreamPointer,
    DataFieldContent, DataUnit, FrameCountCycle, PacketPointer, PduHeader, VcFrameCount, VcTable,
    encode, fhec,
};
use aldis_aos::{PacketOffset, ValidBitCount};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_core::{AnomalySet, DecodeMode, Error, FieldId};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use serde_json::{Map, Value, json};

/// The table the runners build: room for eight virtual channels; no case needs more.
type Table = VcTable<8>;

fn get<'a>(map: &'a Map<String, Value>, case: &CodecCase, name: &str) -> &'a Value {
    map.get(name)
        .unwrap_or_else(|| panic!("{}: missing {name}", case.id))
}

fn uint<T: TryFrom<u64>>(v: &Value) -> T {
    T::try_from(v.as_u64().unwrap()).unwrap_or_else(|_| panic!("{v} out of range"))
}

fn flag(map: &Map<String, Value>, name: &str) -> bool {
    map.get(name).and_then(Value::as_bool).unwrap_or(false)
}

fn size(map: &Map<String, Value>, name: &str) -> usize {
    map.get(name).map_or(0, uint::<usize>)
}

fn octets(v: Option<&Value>) -> Vec<u8> {
    hex::decode(v.and_then(Value::as_str).unwrap_or("")).unwrap()
}

fn input(case: &CodecCase) -> Vec<u8> {
    hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap()
}

fn mode(case: &CodecCase) -> DecodeMode {
    match case.mode.unwrap_or_default() {
        Mode::Strict => DecodeMode::Strict,
        Mode::Lenient => DecodeMode::Lenient,
    }
}

/// The physical channel and its virtual channel table, built in the order the model checks
/// them.
fn config(case: &CodecCase) -> Result<(AosPhysicalChannelConfig, Table), Error> {
    let cfg = case.config.as_ref().unwrap();
    let edition = match cfg.get("edition").and_then(Value::as_str) {
        None | Some("issue5") => AosEdition::Issue5,
        Some("issue4") => AosEdition::Issue4Compatible,
        Some(other) => panic!("{}: edition {other}", case.id),
    };
    let pc = AosPhysicalChannelConfig::new(
        uint(get(cfg, case, "frame_len")),
        flag(cfg, "fhec"),
        size(cfg, "insert_zone_len"),
        flag(cfg, "fecf"),
        edition,
    )?;
    let mut vcs = Vec::new();
    for v in cfg
        .get("vcs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let v = v.as_object().unwrap();
        let scid = SpacecraftId::try_from(uint::<u16>(get(v, case, "scid")))?;
        let vcid = VcId::try_from(uint::<u8>(get(v, case, "vcid")))?;
        let content = match get(v, case, "content").as_str().unwrap() {
            "mpdu" => DataFieldContent::Mpdu,
            "bpdu" => DataFieldContent::Bpdu,
            "vca_sdu" => DataFieldContent::VcaSdu,
            "idle_data" => DataFieldContent::IdleData,
            other => panic!("{}: content {other}", case.id),
        };
        let mut vc = AosVirtualChannelConfig::new(scid, vcid, content)?;
        if flag(v, "ocf") {
            vc = vc.with_ocf();
        }
        let security = SecurityLayout::new(
            size(v, "security_header_len"),
            size(v, "security_trailer_len"),
        )?;
        vcs.push(vc.with_security(security));
    }
    Ok((pc, Table::new(pc, &vcs)?))
}

const fn report(ocf: Ocf) -> &'static str {
    match ocf {
        Ocf::Clcw(_) => "clcw",
        Ocf::ProjectSpecific(_) => "project_specific",
        Ocf::SdlsFsr(_) => "sdls_fsr",
    }
}

/// Every field of the Primary Header (TV-002).
fn header_fields(h: &AosPrimaryHeader) -> Map<String, Value> {
    let v = json!({
        "scid": h.scid().get(),
        "vcid": h.vcid().get(),
        "vc_frame_count": h.vc_frame_count().get(),
        "replay_flag": h.replay_flag(),
        "cycle_use_flag": h.cycle_use_flag(),
        "cycle_field": h.cycle_field().get(),
        "vc_frame_count_cycle": h.vc_frame_count_cycle().map(FrameCountCycle::get),
        "issue4_spare": h.issue4_spare(),
        "fhec": h.fhec(),
        "corrected_symbols": h.corrected_symbols().get(),
        "header_hex": hex::encode(&h.to_octets()[..h.len()]),
        "is_oid": h.is_oid(),
    });
    v.as_object().cloned().unwrap()
}

/// Every field the view exposes (TV-002).
fn decoded(f: &AosFrame<'_>, anomalies: AnomalySet<AosAnomaly>) -> Value {
    let mut out = header_fields(&f.header());
    let (kind, raw, pointer, value, spare) = match f.data_unit() {
        DataUnit::Mpdu(m) => {
            let (p, v) = match m.pointer() {
                PacketPointer::StartsAt(n) => ("starts_at", Some(n.get())),
                PacketPointer::NoPacketStart => ("no_packet_start", None),
                PacketPointer::OnlyIdleData => ("only_idle_data", None),
            };
            (
                "mpdu",
                Some(m.first_header_pointer()),
                Some(p),
                v,
                Some(m.issue4_spare()),
            )
        }
        DataUnit::Bpdu(b) => {
            let (p, v) = match b.pointer() {
                BitstreamPointer::ValidBits(n) => ("valid_bits", Some(n.get())),
                BitstreamPointer::AllValid => ("all_valid", None),
                BitstreamPointer::OnlyIdle => ("only_idle", None),
            };
            (
                "bpdu",
                Some(b.bitstream_data_pointer()),
                Some(p),
                v,
                Some(b.spare()),
            )
        }
        DataUnit::VcaSdu(_) => ("vca_sdu", None, None, None, None),
        DataUnit::IdleData(_) => ("idle_data", None, None, None, None),
    };
    let rest = json!({
        "insert_zone_hex": hex::encode(f.insert_zone()),
        "security_header_hex": hex::encode(f.security_header()),
        "data_field_hex": hex::encode(f.data_field()),
        "security_trailer_hex": hex::encode(f.security_trailer()),
        "ocf_hex": f.ocf().map(|o| hex::encode(&o.to_octets())),
        "ocf_report": f.ocf().map(report),
        "fecf": f.fecf(),
        "data_unit": kind,
        "pdu_pointer_raw": raw,
        "pointer": pointer,
        "pointer_value": value,
        "pdu_spare": spare,
        "zone_hex": hex::encode(f.data_unit().zone()),
        "consumed": f.len(),
        "anomalies": anomalies.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
    });
    out.extend(rest.as_object().cloned().unwrap());
    Value::Object(out)
}

fn pdu(case: &CodecCase, p: &Map<String, Value>) -> Result<PduHeader, Error> {
    let value = || uint::<u16>(get(p, case, "value"));
    Ok(
        match (
            get(p, case, "kind").as_str().unwrap(),
            p.get("pointer").and_then(Value::as_str),
        ) {
            ("mpdu", Some("starts_at")) => {
                PduHeader::Mpdu(PacketPointer::StartsAt(PacketOffset::new(value()).unwrap()))
            }
            ("mpdu", Some("no_packet_start")) => PduHeader::Mpdu(PacketPointer::NoPacketStart),
            ("mpdu", Some("only_idle_data")) => PduHeader::Mpdu(PacketPointer::OnlyIdleData),
            ("bpdu", Some("valid_bits")) => PduHeader::Bpdu(BitstreamPointer::ValidBits(
                ValidBitCount::try_from(value())?,
            )),
            ("bpdu", Some("all_valid")) => PduHeader::Bpdu(BitstreamPointer::AllValid),
            ("bpdu", Some("only_idle")) => PduHeader::Bpdu(BitstreamPointer::OnlyIdle),
            ("none", _) => PduHeader::None,
            other => panic!("{}: pdu {other:?}", case.id),
        },
    )
}

fn cycle(p: &Map<String, Value>) -> Result<Option<FrameCountCycle>, Error> {
    match p.get("cycle") {
        None | Some(Value::Null) => Ok(None),
        Some(n) => FrameCountCycle::try_from(uint::<u8>(n)).map(Some),
    }
}

/// Encodes one frame with `encode` and checks that the two-phase writer agrees (API-020).
fn encode_case(case: &CodecCase, pc: &AosPhysicalChannelConfig, t: &Table) -> Result<Value, Error> {
    let p = case.params.as_ref().unwrap();
    let fields = AosFrameFields {
        scid: SpacecraftId::try_from(uint::<u16>(get(p, case, "scid")))?,
        vcid: VcId::try_from(uint::<u8>(get(p, case, "vcid")))?,
        vc_frame_count: VcFrameCount::try_from(uint::<u32>(get(p, case, "vc_frame_count")))?,
        replay_flag: flag(p, "replay_flag"),
        cycle: cycle(p)?,
        pdu: pdu(case, get(p, case, "pdu").as_object().unwrap())?,
    };
    let (iz, sh, zone, st) = (
        octets(p.get("insert_zone_hex")),
        octets(p.get("security_header_hex")),
        octets(p.get("zone_hex")),
        octets(p.get("security_trailer_hex")),
    );
    let ocf = p
        .get("ocf_hex")
        .filter(|v| !v.is_null())
        .map(|v| Ocf::decode(octets(Some(v)).try_into().unwrap()));
    let content = AosFrameContent {
        insert_zone: &iz,
        security_header: &sh,
        zone: &zone,
        security_trailer: &st,
        ocf,
    };
    let out_len = p.get("out_len").map_or(pc.frame_len(), uint::<usize>);
    let mut out = vec![0xEE_u8; out_len];
    let n = encode(pc, t, &fields, &content, &mut out);
    if n.is_err() {
        assert!(out.iter().all(|&b| b == 0xEE), "{}: out changed", case.id);
    }
    let n = n?;
    let mut two = vec![0_u8; n];
    let mut w = AosFrameWriter::begin(&mut two, pc, t, &fields).unwrap();
    w.insert_zone_mut().copy_from_slice(&iz);
    w.security_header_mut().copy_from_slice(&sh);
    w.zone_mut().copy_from_slice(&zone);
    w.security_trailer_mut().copy_from_slice(&st);
    if let Some(ocf) = ocf {
        w.set_ocf(ocf).unwrap();
    }
    let ctx = FrameCtx::Aos {
        scid: fields.scid,
        vcid: fields.vcid,
    };
    let done = w.protect(&mut PreFilledSecurity, &ctx).unwrap().finalize();
    assert_eq!(done.len(), n, "{}", case.id);
    assert_eq!(two, out[..n], "{}: two-phase writer differs", case.id);
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

/// Consecutive OID frames of one SCID from one generator (AOS-016, CMN-021).
fn encode_oid(
    case: &CodecCase,
    pc: &AosPhysicalChannelConfig,
    table: &Table,
) -> Result<Value, Error> {
    let params = case.params.as_ref().unwrap();
    let scid = SpacecraftId::try_from(uint::<u16>(get(params, case, "scid")))?;
    let mut oid = OidGenerator::new();
    let mut frames = Vec::new();
    for f in get(params, case, "frames").as_array().unwrap() {
        let f = f.as_object().unwrap();
        let count = VcFrameCount::try_from(uint::<u32>(get(f, case, "vc_frame_count")))?;
        let fields = AosFrameFields {
            cycle: cycle(f)?,
            ..AosFrameFields::oid(scid, count)
        };
        let mut out = vec![0_u8; pc.frame_len()];
        let mut w = AosFrameWriter::begin_oid(&mut out, pc, table, &fields, &mut oid)?;
        if let Some(iz) = f.get("insert_zone_hex") {
            w.insert_zone_mut().copy_from_slice(&octets(Some(iz)));
        }
        let n = w.unsecured().unwrap().finalize().len();
        frames.push(hex::encode(&out[..n]));
    }
    Ok(json!({ "frames_hex": frames }))
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    let (pc, t) = config(case)?;
    let bytes = input(case);
    match case.operation.as_str() {
        "config" => Ok(json!({})),
        "peek_length" => AosFrame::peek_length(&bytes, &pc).map(|n| json!({ "len": n })),
        "decode_header" => {
            AosPrimaryHeader::decode(&bytes, &pc).map(|h| Value::Object(header_fields(&h)))
        }
        "decode" => mode(case)
            .dispatch(
                || AosFrame::decode(&bytes, &pc, &t),
                || AosFrame::decode_lenient(&bytes, &pc, &t),
            )
            .map(|(f, anomalies)| decoded(&f, anomalies)),
        "encode" => encode_case(case, &pc, &t),
        "encode_oid" => encode_oid(case, &pc, &t),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

/// The FHEC cases of `fhec_*.json` (AOS-013, AOS-014).
fn fhec_case(case: &CodecCase) -> Outcome {
    let bytes = input(case);
    match case.operation.as_str() {
        "fhec_encode" => {
            let six: [u8; 6] = bytes.try_into().unwrap();
            let fhec = fhec::encode(&six);
            let mut header = [six[0], six[1], six[2], six[3], six[4], six[5], 0xA5, 0x5A];
            fhec::write(&mut header);
            Outcome::Ok(
                json!({
                    "fhec_hex": hex::encode(&fhec),
                    "output_hex": hex::encode(&header),
                })
                .as_object()
                .cloned()
                .unwrap(),
            )
        }
        "fhec_decode" => match fhec::decode(&bytes.try_into().unwrap()) {
            Ok(c) => Outcome::Ok(
                json!({
                    "header_hex": hex::encode(&c.octets()),
                    "corrected_symbols": c.corrected_symbols().get(),
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
            Err(e) => Outcome::Err {
                kind: e.kind().name().to_owned(),
                field: e.field().map(FieldId::code),
            },
        },
        _ => Outcome::Unsupported,
    }
}

fn handle(case: &CodecCase) -> Outcome {
    if case.operation.starts_with("fhec_") {
        return fhec_case(case);
    }
    match run(case) {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap()),
        Err(e) => Outcome::Err {
            kind: e.kind().name().to_owned(),
            field: e.field().map(FieldId::code),
        },
    }
}

#[test]
fn fhec_encode() {
    // req: AOS-013, TV-002, VER-021
    run_codec_file("aos/fhec_encode.json", handle).assert_passed();
}

#[test]
fn fhec_correct_1_symbol() {
    // req: AOS-013, AOS-014, VER-021
    run_codec_file("aos/fhec_correct_1_symbol.json", handle).assert_passed();
}

#[test]
fn fhec_correct_2_symbols() {
    // req: AOS-013, AOS-014, VER-021
    run_codec_file("aos/fhec_correct_2_symbols.json", handle).assert_passed();
}

#[test]
fn fhec_miscorrection() {
    // req: AOS-013, AOS-014, VER-021
    run_codec_file("aos/fhec_miscorrection.json", handle).assert_passed();
}

#[test]
fn fhec_uncorrectable() {
    // req: AOS-014, GEN-020, VER-021
    run_codec_file("aos/fhec_uncorrectable.json", handle).assert_passed();
}

#[test]
fn minimal() {
    // req: AOS-010, AOS-011, AOS-012, AOS-019, API-010, API-012, API-013, API-014, API-020, TV-002, VER-021
    run_codec_file("aos/minimal.json", handle).assert_passed();
}

#[test]
fn fhec_frame() {
    // req: AOS-013, AOS-014, AOS-019, API-013, VER-021
    run_codec_file("aos/fhec_frame.json", handle).assert_passed();
}

#[test]
fn insert_zone() {
    // req: AOS-015, AOS-019, VER-021
    run_codec_file("aos/insert_zone.json", handle).assert_passed();
}

#[test]
fn security_layout() {
    // req: API-070, API-071, API-072, VER-021
    run_codec_file("aos/security_layout.json", handle).assert_passed();
}

#[test]
fn mpdu_pointers() {
    // req: AOS-017, AOS-019, VER-021
    run_codec_file("aos/mpdu_pointers.json", handle).assert_passed();
}

#[test]
fn bpdu_pointers() {
    // req: AOS-017, AOS-018, AOS-019, VER-021
    run_codec_file("aos/bpdu_pointers.json", handle).assert_passed();
}

#[test]
fn oid() {
    // req: AOS-016, CMN-020, CMN-021, VER-021
    run_codec_file("aos/oid.json", handle).assert_passed();
}

#[test]
fn issue4_compat() {
    // req: AOS-001, AOS-002, AOS-003, GEN-062, VER-021
    run_codec_file("aos/issue4_compat.json", handle).assert_passed();
}

#[test]
fn scid_10bit() {
    // req: AOS-001, AOS-002, VER-021
    run_codec_file("aos/scid_10bit.json", handle).assert_passed();
}

#[test]
fn config_file() {
    // req: API-040, GEN-060, AOS-018, VER-021
    run_codec_file("aos/config.json", handle).assert_passed();
}

#[test]
fn invalid() {
    // req: AOS-010, AOS-011, API-012, API-022, GEN-041, VER-021
    run_codec_file("aos/invalid.json", handle).assert_passed();
}
