//! Runs the shared conformance vectors in `test-vectors/core/` against `aldis-core`
//! (VER-021, TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::counter::{Continuity, ModCounter};
use aldis_core::crc::{crc16, crc16_bitwise};
use aldis_core::error::ErrorKind;
use aldis_core::layout::{FrameLayout, LayoutSpec};
use aldis_core::ocf::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, FieldId, bits};
use aldis_testkit::{CodecCase, Outcome, hex, run_codec_file};
use serde_json::{Map, Value, json};

fn param<'a>(case: &'a CodecCase, name: &str) -> &'a Value {
    case.params
        .as_ref()
        .and_then(|p| p.get(name))
        .unwrap_or_else(|| panic!("{}: missing param {name}", case.id))
}

fn param_u64(case: &CodecCase, name: &str) -> u64 {
    param(case, name)
        .as_u64()
        .unwrap_or_else(|| panic!("{}: param {name} is not a u64", case.id))
}

fn param_usize(case: &CodecCase, name: &str) -> usize {
    usize::try_from(param_u64(case, name)).unwrap()
}

fn param_u8(case: &CodecCase, name: &str) -> u8 {
    u8::try_from(param_u64(case, name)).unwrap()
}

fn input(case: &CodecCase) -> Vec<u8> {
    hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap()
}

fn ok(value: &Value) -> Outcome {
    Outcome::Ok(value.as_object().cloned().unwrap_or_else(Map::new))
}

fn outcome(result: Result<Value, Error>) -> Outcome {
    match result {
        Ok(v) => ok(&v),
        Err(e) => Outcome::Err {
            kind: e.kind().name().to_owned(),
            field: e.field().map(FieldId::code),
        },
    }
}

fn handle(case: &CodecCase) -> Outcome {
    match case.operation.as_str() {
        "crc16" => {
            let data = input(case);
            let val = crc16(&data);
            assert_eq!(crc16_bitwise(&data), val);
            #[cfg(feature = "crc-table")]
            assert_eq!(aldis_core::crc::crc16_table(&data), val);
            ok(&json!({ "value_hex": format!("{val:04X}") }))
        }
        "oid_fill" => {
            let len = param_usize(case, "octets");
            let mut out = vec![0; len];
            OidGenerator::new().fill(&mut out);
            let mut gal = vec![0; len];
            OidGeneratorGalois::new().fill(&mut gal);
            assert_eq!(out, gal);
            ok(&json!({ "output_hex": hex::encode(&out) }))
        }
        "oid_fill_sequence" => {
            let mut g = OidGenerator::new();
            let mut gal = OidGeneratorGalois::new();
            let outputs: Vec<String> = param(case, "fills")
                .as_array()
                .unwrap()
                .iter()
                .map(|n| {
                    let len = usize::try_from(n.as_u64().unwrap()).unwrap();
                    let mut out = vec![0; len];
                    let mut out_gal = vec![0; len];
                    g.fill(&mut out);
                    gal.fill(&mut out_gal);
                    assert_eq!(out, out_gal);
                    hex::encode(&out)
                })
                .collect();
            ok(&json!({ "outputs_hex": outputs }))
        }
        "error_code" => {
            let name = param(case, "kind").as_str().unwrap();
            match ErrorKind::from_name(name) {
                Some(k) => {
                    ok(&json!({ "code": k.code(), "c_abi": i32::from(k.code()).wrapping_neg() }))
                }
                None => Outcome::Err {
                    kind: "UnknownKind".to_owned(),
                    field: None,
                },
            }
        }
        "bits_get" => outcome(
            bits::get(
                &input(case),
                param_usize(case, "bit_offset"),
                param_u8(case, "width"),
            )
            .map(|v| json!({ "value": v })),
        ),
        "bits_set" => {
            let mut buf = input(case);
            outcome(
                bits::set(
                    &mut buf,
                    param_usize(case, "bit_offset"),
                    param_u8(case, "width"),
                    param_u64(case, "value"),
                )
                .map(|()| json!({ "output_hex": hex::encode(&buf) })),
            )
        }
        "counter_increment" => outcome(
            ModCounter::new(param_u8(case, "bits"), param_u64(case, "value"))
                .map(|c| json!({ "value": c.next().value() })),
        ),
        "counter_classify" => outcome(
            ModCounter::new(param_u8(case, "bits"), param_u64(case, "prev"))
                .and_then(|c| c.classify(param_u64(case, "next")))
                .map(|c| match c {
                    Continuity::InSequence => json!({ "class": "in-sequence" }),
                    Continuity::Repeat => json!({ "class": "repeat" }),
                    Continuity::Gap { missing } => json!({ "class": "gap", "missing": missing }),
                    Continuity::Backwards => json!({ "class": "backwards" }),
                }),
        ),
        "frame_layout" => handle_layout(case),
        "ocf_classify" | "ocf_report" => handle_ocf(case),
        _ => Outcome::Unsupported,
    }
}

fn handle_ocf(case: &CodecCase) -> Outcome {
    let octets = ocf_input(case);
    if case.operation == "ocf_classify" {
        let ocf = Ocf::decode(octets);
        assert_eq!(Ocf::from(octets), ocf);
        let report = match ocf {
            Ocf::Clcw(_) => "clcw",
            Ocf::ProjectSpecific(_) => "project_specific",
            Ocf::SdlsFsr(_) => "sdls_fsr",
        };
        return ok(&json!({ "report": report, "octets_hex": hex::encode(&ocf.to_octets()) }));
    }
    // Each report type's checked constructor; a report it accepts is the OCF's variant.
    let result = match param(case, "report").as_str().unwrap() {
        "clcw" => Type1Report::try_from(octets).map(Ocf::from),
        "project_specific" => ProjectSpecificReport::try_from(octets).map(Ocf::from),
        "sdls_fsr" => FsrReport::try_from(octets).map(Ocf::from),
        other => panic!("{}: unknown report {other}", case.id),
    };
    if let Ok(ocf) = result {
        assert_eq!(Ocf::decode(octets), ocf);
    }
    outcome(result.map(|ocf| json!({ "octets_hex": hex::encode(&ocf.to_octets()) })))
}

fn ocf_input(case: &CodecCase) -> [u8; OCF_LEN] {
    <[u8; OCF_LEN]>::try_from(input(case))
        .unwrap_or_else(|v| panic!("{}: {} octets, not an OCF", case.id, v.len()))
}

fn handle_layout(case: &CodecCase) -> Outcome {
    let flag = |name: &str| param(case, name).as_bool().unwrap();
    let spec = SecurityLayout::new(
        param_usize(case, "security_header_len"),
        param_usize(case, "security_trailer_len"),
    )
    .map(|security| LayoutSpec {
        frame_len: param_usize(case, "frame_len"),
        primary_header_len: param_usize(case, "primary_header_len"),
        secondary_len: param_usize(case, "secondary_len"),
        security,
        ocf: flag("ocf"),
        fecf: flag("fecf"),
    });
    outcome(spec.and_then(|s| FrameLayout::compute(&s)).map(|l| {
        let r = |x: &core::ops::Range<usize>| json!([x.start, x.end]);
        json!({
            "primary_header": r(&l.primary_header()), "secondary": r(&l.secondary()),
            "security_header": r(&l.security_header()), "data": r(&l.data()),
            "security_trailer": r(&l.security_trailer()), "ocf": r(&l.ocf()), "fecf": r(&l.fecf()),
        })
    }))
}

#[test]
fn crc16_vectors() {
    // req: CMN-001, CMN-002, CMN-004, TV-022, VER-021
    run_codec_file("core/crc16.json", handle).assert_passed();
}

#[test]
fn oid_vectors() {
    // req: CMN-020, CMN-021, CMN-022, CMN-023, TV-022, VER-021
    run_codec_file("core/oid.json", handle).assert_passed();
}

#[test]
fn error_vectors() {
    // req: GEN-040, VER-021
    run_codec_file("core/errors.json", handle).assert_passed();
}

#[test]
fn bits_vectors() {
    // req: GEN-050, GEN-051, VER-021
    run_codec_file("core/bits.json", handle).assert_passed();
}

#[test]
fn counter_vectors() {
    // req: CMN-030, CMN-031, VER-021
    run_codec_file("core/counters.json", handle).assert_passed();
}

#[test]
fn layout_vectors() {
    // req: CMN-040, CMN-041, VER-021
    run_codec_file("core/layout.json", handle).assert_passed();
}

#[test]
fn ocf_vectors() {
    // req: CMN-010, CMN-011, VER-021
    run_codec_file("core/ocf.json", handle).assert_passed();
}

#[test]
fn corrupted_vector_fails() {
    // req: TV-012
    let path = aldis_testkit::vectors_dir().join("core/crc16.json");
    let aldis_testkit::VectorFile::Codec(mut file) =
        aldis_testkit::VectorFile::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap()
    else {
        panic!("core/crc16.json is not a codec file");
    };
    let case = file
        .cases
        .iter_mut()
        .find(|c| c.superseded.is_none())
        .unwrap();
    let expected_id = case.id.clone();
    case.expect
        .values
        .insert("value_hex".to_owned(), json!("0000"));
    let report = aldis_testkit::run_codec(&file, &path, handle);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].case_id, expected_id);
}
