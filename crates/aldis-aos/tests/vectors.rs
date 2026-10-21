//! Runs the shared conformance vectors in `test-vectors/aos/` against `aldis-aos` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_aos::fhec;
use aldis_core::FieldId;
use aldis_testkit::{CodecCase, Outcome, hex, run_codec_file};
use serde_json::{Map, Value, json};

fn input<const N: usize>(case: &CodecCase) -> [u8; N] {
    let bytes = hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap();
    bytes
        .try_into()
        .unwrap_or_else(|_| panic!("{}: input is not {N} octets", case.id))
}

fn outputs(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn handler(case: &CodecCase) -> Outcome {
    match case.operation.as_str() {
        "fhec_encode" => {
            let six: [u8; 6] = input(case);
            let fhec = fhec::encode(&six);
            let mut header = [six[0], six[1], six[2], six[3], six[4], six[5], 0xA5, 0x5A];
            fhec::write(&mut header);
            Outcome::Ok(outputs(json!({
                "fhec_hex": hex::encode(&fhec),
                "output_hex": hex::encode(&header),
            })))
        }
        "fhec_decode" => match fhec::decode(&input(case)) {
            Ok(c) => Outcome::Ok(outputs(json!({
                "header_hex": hex::encode(&c.octets()),
                "corrected_symbols": c.corrected_symbols().get(),
            }))),
            Err(e) => Outcome::Err {
                kind: e.kind().name().to_owned(),
                field: e.field().map(FieldId::code),
            },
        },
        _ => Outcome::Unsupported,
    }
}

#[test]
fn fhec_encode() {
    // req: AOS-013, TV-002, VER-021
    run_codec_file("aos/fhec_encode.json", handler).assert_passed();
}

#[test]
fn fhec_correct_1_symbol() {
    // req: AOS-013, AOS-014, VER-021
    run_codec_file("aos/fhec_correct_1_symbol.json", handler).assert_passed();
}

#[test]
fn fhec_correct_2_symbols() {
    // req: AOS-013, AOS-014, VER-021
    run_codec_file("aos/fhec_correct_2_symbols.json", handler).assert_passed();
}

#[test]
fn fhec_miscorrection() {
    // req: AOS-013, AOS-014, VER-021
    run_codec_file("aos/fhec_miscorrection.json", handler).assert_passed();
}

#[test]
fn fhec_uncorrectable() {
    // req: AOS-014, GEN-020, VER-021
    run_codec_file("aos/fhec_uncorrectable.json", handler).assert_passed();
}
