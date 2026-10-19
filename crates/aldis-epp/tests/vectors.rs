//! Runs the shared conformance vectors in `test-vectors/epp/` against `aldis-epp` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::{DecodeMode, Error};
use aldis_epp::{
    EncapsulationFields, EncapsulationHeader, EncapsulationPacket, EncapsulationWriter, Epi,
    EpiExtension, EpiSet, EppConfig, ExtendedEpiSet, HeaderLen, HeaderSize, UserDefined, encode,
    idle,
};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use serde_json::{Map, Value, json};

fn value<'a>(map: Option<&'a Map<String, Value>>, case: &CodecCase, name: &str) -> &'a Value {
    map.and_then(|m| m.get(name))
        .unwrap_or_else(|| panic!("{}: missing {name}", case.id))
}

fn param<'a>(case: &'a CodecCase, name: &str) -> &'a Value {
    value(case.params.as_ref(), case, name)
}

fn u64_of(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

fn usize_param(case: &CodecCase, name: &str) -> usize {
    usize::try_from(u64_of(param(case, name))).unwrap()
}

fn u8_param(case: &CodecCase, name: &str) -> u8 {
    u8::try_from(u64_of(param(case, name))).unwrap()
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

fn fields(case: &CodecCase) -> EncapsulationFields {
    EncapsulationFields {
        epi: Epi::new(u8_param(case, "epi")).unwrap(),
        extension: EpiExtension::new(u8_param(case, "epi_extension")).unwrap(),
        user_defined: UserDefined::new(u8_param(case, "user_defined")).unwrap(),
    }
}

fn header_size(case: &CodecCase) -> HeaderSize {
    match param(case, "header_size") {
        Value::String(s) if s == "auto" => HeaderSize::Auto,
        v => HeaderSize::Fixed(match u64_of(v) {
            1 => HeaderLen::One,
            2 => HeaderLen::Two,
            4 => HeaderLen::Four,
            8 => HeaderLen::Eight,
            n => panic!("{}: header_size {n}", case.id),
        }),
    }
}

fn config(case: &CodecCase) -> Result<EppConfig, Error> {
    let cfg = case.config.as_ref();
    let len = |name| u32::try_from(u64_of(value(cfg, case, name))).unwrap();
    let list = |name| -> Vec<u8> {
        value(cfg, case, name)
            .as_array()
            .unwrap()
            .iter()
            .map(|v| u8::try_from(u64_of(v)).unwrap())
            .collect()
    };
    let epis = list("valid_epis")
        .into_iter()
        .fold(EpiSet::EMPTY, |s, e| s.with(Epi::new(e).unwrap()));
    let ext = list("valid_extended_epis")
        .into_iter()
        .fold(ExtendedEpiSet::EMPTY, |s, e| {
            s.with(EpiExtension::new(e).unwrap())
        });
    EppConfig::new(len("min_data_len"), len("max_data_len"), epis, ext)
}

fn header_hex(header: &EncapsulationHeader) -> String {
    let mut buf = [0_u8; 8];
    let n = header.encode(&mut buf).unwrap();
    hex::encode(&buf[..n])
}

fn decoded(p: &EncapsulationPacket<'_>) -> Value {
    json!({
        "epi": p.epi().get(),
        "user_defined": p.user_defined().map(UserDefined::get),
        "epi_extension": p.extension().map(EpiExtension::get),
        "ccsds_defined": p.ccsds_defined(),
        "header_len": p.header_len().octets(),
        "packet_len": p.header().packet_len(),
        "data_hex": hex::encode(p.data()),
        "consumed": p.len(),
        "anomalies": p.anomalies().iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
    })
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    match case.operation.as_str() {
        "decode" => {
            let bytes = input(case);
            EncapsulationPacket::decode(&bytes, mode(case)).map(|p| decoded(&p))
        }
        "peek_length" => {
            EncapsulationPacket::peek_length(&input(case)).map(|n| json!({ "len": n }))
        }
        "encode" => {
            let data = hex::decode(param(case, "data_hex").as_str().unwrap()).unwrap();
            let mut out = vec![0_u8; data.len() + 8];
            let n = encode(fields(case), header_size(case), &data, &mut out)?;
            // The two-phase writer must produce the same octets (API-020).
            let mut two = vec![0_u8; data.len() + 8];
            let mut w =
                EncapsulationWriter::begin(&mut two, fields(case), header_size(case), data.len())
                    .unwrap();
            w.data_mut().copy_from_slice(&data);
            assert_eq!(w.finish(data.len()), Ok(n), "{}", case.id);
            assert_eq!(two[..n], out[..n], "{}: two-phase writer differs", case.id);
            Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
        }
        "encode_header" => {
            let h = EncapsulationHeader::for_data(
                fields(case),
                header_size(case),
                usize_param(case, "data_len"),
            )?;
            Ok(json!({ "header_hex": header_hex(&h), "total_len": h.packet_len() }))
        }
        "idle" => {
            let total = usize_param(case, "total_len");
            let mut out = vec![0_u8; total];
            let n = idle(total, u8_param(case, "fill"), &mut out)?;
            Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
        }
        "idle_header" => {
            let h = EncapsulationHeader::idle(usize_param(case, "total_len"))?;
            Ok(json!({ "header_hex": header_hex(&h), "total_len": h.packet_len() }))
        }
        "validate" => {
            let cfg = config(case)?;
            let bytes = input(case);
            let packet = EncapsulationPacket::decode(&bytes, DecodeMode::Strict)?;
            packet.validate(&cfg).map(|()| json!({}))
        }
        "config" => config(case).map(|_| json!({})),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

fn handle(case: &CodecCase) -> Outcome {
    match run(case) {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap()),
        Err(e) => Outcome::Err(e.kind().name().to_owned()),
    }
}

macro_rules! vector_test {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            // req: VER-021, TV-001, TV-002, TV-003
            run_codec_file(concat!("epp/", $file, ".json"), handle).assert_passed();
        }
    };
}

vector_test!(header_1, "header_1");
vector_test!(header_2, "header_2");
vector_test!(header_4, "header_4");
vector_test!(header_8, "header_8");
vector_test!(idle_packets, "idle");
vector_test!(auto_size_boundaries, "auto_size_boundaries");
vector_test!(invalid, "invalid");
vector_test!(config_and_validate, "config");
