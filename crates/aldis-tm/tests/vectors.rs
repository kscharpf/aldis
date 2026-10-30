//! Runs the shared conformance vectors in `test-vectors/tm/` against `aldis-tm` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_core::{AnomalySet, DecodeMode, Error, FieldId};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use aldis_tm::TmAnomaly;
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FirstHeaderPointer, FshLen, NoSdls, PacketOffset,
    PacketPointer, SegmentLengthId, TmFrame, TmFrameContent, TmFrameFields, TmFrameWriter,
    TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig, UncheckedTmFrame,
    encode,
};
use serde_json::{Map, Value, json};

fn get<'a>(map: &'a Map<String, Value>, case: &CodecCase, name: &str) -> &'a Value {
    map.get(name)
        .unwrap_or_else(|| panic!("{}: missing {name}", case.id))
}

fn uint<T: TryFrom<u64>>(v: &Value) -> T {
    T::try_from(v.as_u64().unwrap()).unwrap_or_else(|_| panic!("{v} out of range"))
}

fn octets(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap_or("")).unwrap()
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

/// The FSH length of a configuration value, `null` for none (Tables 5-2, 5-3).
fn fsh(v: Option<&Value>) -> Result<Option<FshLen>, Error> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(n) => FshLen::try_from(uint::<u8>(n)).map(Some),
    }
}

/// The physical channel, and the master channel when the configuration has an SCID, built in
/// the order the model checks them.
fn config(
    case: &CodecCase,
) -> Result<(TmPhysicalChannelConfig, Option<TmMasterChannelConfig>), Error> {
    let cfg = case.config.as_ref().unwrap();
    let pc = TmPhysicalChannelConfig::new(
        uint(get(cfg, case, "frame_len")),
        get(cfg, case, "fecf").as_bool().unwrap(),
    )?;
    let Some(scid) = cfg.get("scid") else {
        return Ok((pc, None));
    };
    let scid = SpacecraftId::try_from(uint::<u16>(scid))?;
    let mc_ocf = cfg.get("mc_ocf").and_then(Value::as_bool).unwrap_or(false);
    let mut mc = TmMasterChannelConfig::new(pc, scid, fsh(cfg.get("mc_fsh"))?, mc_ocf);
    for v in cfg
        .get("vcs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let v = v.as_object().unwrap();
        let vcid = VcId::try_from(uint::<u8>(get(v, case, "vcid")))?;
        let content = match get(v, case, "content").as_str().unwrap() {
            "packets" => DataFieldContent::Packets,
            "vca_sdu" => DataFieldContent::VcaSdu,
            other => panic!("{}: content {other}", case.id),
        };
        let mut vc = TmVirtualChannelConfig::new(vcid, content);
        if let Some(len) = fsh(v.get("fsh"))? {
            vc = vc.with_fsh(len);
        }
        if v.get("ocf").and_then(Value::as_bool).unwrap_or(false) {
            vc = vc.with_ocf();
        }
        let sh = v.get("security_header_len").map_or(0, uint::<usize>);
        let st = v.get("security_trailer_len").map_or(0, uint::<usize>);
        vc = vc.with_security(SecurityLayout::new(sh, st)?);
        mc = mc.with_vc(vc)?;
    }
    Ok((pc, Some(mc)))
}

fn status(case: &CodecCase, s: &Map<String, Value>) -> Result<DataFieldStatus, Error> {
    Ok(if get(s, case, "sync").as_bool().unwrap() {
        DataFieldStatus::VcaSdu {
            packet_order_flag: get(s, case, "packet_order_flag").as_bool().unwrap(),
            segment_length_id: SegmentLengthId::new(uint(get(s, case, "segment_length_id")))
                .unwrap(),
            first_header_pointer: FirstHeaderPointer::new(uint(get(
                s,
                case,
                "first_header_pointer",
            )))
            .unwrap(),
        }
    } else {
        DataFieldStatus::Packets(match get(s, case, "pointer").as_str().unwrap() {
            "starts_at" => PacketPointer::StartsAt(PacketOffset::try_from(uint::<u16>(get(
                s,
                case,
                "packet_offset",
            )))?),
            "no_packet_start" => PacketPointer::NoPacketStart,
            "only_idle_data" => PacketPointer::OnlyIdleData,
            other => panic!("{}: pointer {other}", case.id),
        })
    })
}

fn ocf_of(v: Option<&Value>) -> Option<Ocf> {
    v.filter(|v| !v.is_null())
        .map(|v| Ocf::decode(octets(v).try_into().unwrap()))
}

const fn report(ocf: Ocf) -> &'static str {
    match ocf {
        Ocf::Clcw(_) => "clcw",
        Ocf::ProjectSpecific(_) => "project_specific",
        Ocf::SdlsFsr(_) => "sdls_fsr",
    }
}

/// Every field the view exposes (TV-002).
fn decoded(f: &UncheckedTmFrame<'_>, anomalies: AnomalySet<TmAnomaly>) -> Value {
    let h = f.header();
    let (pointer, offset) = match f.data_field_status() {
        DataFieldStatus::Packets(PacketPointer::StartsAt(n)) => (Some("starts_at"), Some(n.get())),
        DataFieldStatus::Packets(PacketPointer::NoPacketStart) => (Some("no_packet_start"), None),
        DataFieldStatus::Packets(PacketPointer::OnlyIdleData) => (Some("only_idle_data"), None),
        DataFieldStatus::VcaSdu { .. } => (None, None),
    };
    json!({
        "scid": f.scid().get(),
        "vcid": f.vcid().get(),
        "ocf_flag": h.ocf_flag,
        "mc_frame_count": f.mc_frame_count(),
        "vc_frame_count": f.vc_frame_count(),
        "fsh_flag": h.fsh_flag,
        "sync_flag": h.sync_flag,
        "packet_order_flag": h.packet_order_flag,
        "segment_length_id": h.segment_length_id.get(),
        "first_header_pointer": h.first_header_pointer.get(),
        "pointer": pointer,
        "packet_offset": offset,
        "fsh_len": f.fsh_len().map(FshLen::get),
        "fsh_data_hex": f.secondary_header_data().map(hex::encode),
        "security_header_hex": hex::encode(f.security_header()),
        "data_hex": hex::encode(f.data_field()),
        "security_trailer_hex": hex::encode(f.security_trailer()),
        "ocf_hex": f.ocf().map(|o| hex::encode(&o.to_octets())),
        "ocf_report": f.ocf().map(report),
        "fecf": f.fecf(),
        "consumed": f.len(),
        "anomalies": anomalies.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
    })
}

/// Encodes one frame with `encode` and checks that the two-phase writer agrees (API-020).
fn encode_case(case: &CodecCase, mc: &TmMasterChannelConfig) -> Result<Value, Error> {
    let p = case.params.as_ref().unwrap();
    let fields = TmFrameFields::new(
        VcId::try_from(uint::<u8>(get(p, case, "vcid")))?,
        uint(get(p, case, "mc_frame_count")),
        uint(get(p, case, "vc_frame_count")),
        status(case, get(p, case, "status").as_object().unwrap())?,
    );
    let (fsh_data, sh, data, st) = (
        octets(get(p, case, "fsh_data_hex")),
        octets(get(p, case, "security_header_hex")),
        octets(get(p, case, "data_hex")),
        octets(get(p, case, "security_trailer_hex")),
    );
    let content = TmFrameContent {
        secondary_header_data: &fsh_data,
        security_header: &sh,
        data: &data,
        security_trailer: &st,
        ocf: ocf_of(p.get("ocf_hex")),
    };
    let out_len = p.get("out_len").map_or(mc.pc().frame_len(), uint::<usize>);
    let mut out = vec![0xEE_u8; out_len];
    let n = encode(mc, &fields, &content, &mut out);
    if n.is_err() {
        assert!(out.iter().all(|&b| b == 0xEE), "{}: out changed", case.id);
    }
    let n = n?;
    let mut two = vec![0_u8; n];
    let mut w = TmFrameWriter::begin(&mut two, mc, &fields).unwrap();
    w.secondary_header_data_mut().copy_from_slice(&fsh_data);
    w.security_header_mut().copy_from_slice(&sh);
    w.data_field_mut().copy_from_slice(&data);
    w.security_trailer_mut().copy_from_slice(&st);
    if let Some(ocf) = content.ocf {
        w.set_ocf(ocf).unwrap();
    }
    let ctx = FrameCtx::Tm {
        scid: mc.scid(),
        vcid: fields.vcid,
    };
    let done = w.protect(&mut PreFilledSecurity, &ctx).unwrap().finalize();
    assert_eq!(done.len(), n, "{}", case.id);
    assert_eq!(two, out[..n], "{}: two-phase writer differs", case.id);
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

/// Consecutive OID frames from one generator (TM-010, CMN-021).
fn encode_oid(case: &CodecCase, mc: &TmMasterChannelConfig) -> Result<Value, Error> {
    let p = case.params.as_ref().unwrap();
    let vcid = VcId::try_from(uint::<u8>(get(p, case, "vcid")))?;
    let mut oid = OidGenerator::new();
    let mut frames = Vec::new();
    for f in get(p, case, "frames").as_array().unwrap() {
        let f = f.as_object().unwrap();
        let mut out = vec![0_u8; mc.pc().frame_len()];
        let mut w = TmFrameWriter::begin_oid(
            &mut out,
            mc,
            vcid,
            uint(get(f, case, "mc_frame_count")),
            uint(get(f, case, "vc_frame_count")),
            &mut oid,
        )?;
        if let Some(data) = f.get("fsh_data_hex") {
            w.secondary_header_data_mut().copy_from_slice(&octets(data));
        }
        if let Some(ocf) = ocf_of(f.get("ocf_hex")) {
            w.set_ocf(ocf)?;
        }
        let n = w.unsecured().unwrap().finalize().len();
        frames.push(hex::encode(&out[..n]));
    }
    Ok(json!({ "frames_hex": frames }))
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    let (pc, mc) = config(case)?;
    let bytes = input(case);
    let mc = || {
        mc.as_ref()
            .unwrap_or_else(|| panic!("{}: no master channel", case.id))
    };
    match case.operation.as_str() {
        "config" => Ok(json!({})),
        "peek_length" => TmFrame::peek_length(&bytes, &pc).map(|n| json!({ "len": n })),
        "decode" => mode(case)
            .dispatch(
                || TmFrame::decode(&bytes, &pc).map(UncheckedTmFrame::from),
                || TmFrame::decode_lenient(&bytes, &pc).map(|l| l.map(UncheckedTmFrame::from)),
            )
            .map(|(f, a)| decoded(&f, a)),
        "decode_with" => mode(case)
            .dispatch(
                || TmFrame::decode_with(&bytes, &pc, mc()).map(UncheckedTmFrame::from),
                || {
                    TmFrame::decode_with_lenient(&bytes, &pc, mc())
                        .map(|l| l.map(UncheckedTmFrame::from))
                },
            )
            .map(|(f, a)| decoded(&f, a)),
        "decode_unchecked" => mode(case)
            .dispatch(
                || TmFrame::decode_unchecked(&bytes, &pc, &NoSdls),
                || TmFrame::decode_unchecked_lenient(&bytes, &pc, &NoSdls),
            )
            .map(|(f, a)| decoded(&f, a)),
        // Decoded with the master channel's security layouts, which it is checked against
        // (c-abi.md §11).
        "validate" => {
            let (f, _) = mode(case).dispatch(
                || TmFrame::decode_with(&bytes, &pc, mc()),
                || TmFrame::decode_with_lenient(&bytes, &pc, mc()),
            )?;
            f.validate(mc()).map(|_| json!({}))
        }
        "encode" => encode_case(case, mc()),
        "encode_oid" => encode_oid(case, mc()),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

fn handle(case: &CodecCase) -> Outcome {
    match run(case) {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap()),
        Err(e) => Outcome::Err {
            kind: e.kind().name().to_owned(),
            field: e.field().map(FieldId::code),
        },
    }
}

macro_rules! vector_test {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            // req: VER-021, TV-001, TV-002, TV-003
            run_codec_file(concat!("tm/", $file, ".json"), handle).assert_passed();
        }
    };
}

vector_test!(minimal, "minimal");
vector_test!(fsh_ocf_fecf, "fsh_ocf_fecf");
vector_test!(fhp_values, "fhp_values");
vector_test!(oid_sequence, "oid_sequence");
vector_test!(security_layout, "security_layout");
vector_test!(invalid, "invalid");
