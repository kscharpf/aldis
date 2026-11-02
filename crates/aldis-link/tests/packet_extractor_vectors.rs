//! Runs the `PacketExtractor` conformance scenarios of `test-vectors/link/recv/` against
//! `PacketExtractor::step` (LNK-003, LNK-060…LNK-064, TV-010…TV-012, TV-023). Every step's
//! outputs must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

#[path = "common/mod.rs"]
mod common;

use aldis_link::common::ValidPvns;
use aldis_link::extractor::{
    ExtractionError, ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor,
    PacketExtractorConfig, extractor_max_actions,
};
use aldis_link::protocol::{Aos, Tm, Uslp};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use common::{Proto, num, pvns};
use serde_json::{Map, Value, json};

const FILES: [&str; 3] = [
    "link/recv/packet_extractor_tm.json",
    "link/recv/packet_extractor_aos.json",
    "link/recv/packet_extractor_uslp.json",
];

/// The assembly buffer: the longest Maximum Packet Length of the files.
const M: usize = 100;
/// The packet zone: the longest of the files is 24 octets.
const D: usize = 32;
/// The bound on the actions of one input, for the shortest packet delivered (idle packets of
/// 1 octet are delivered by one file).
const N: usize = extractor_max_actions(D, 1);

type Engine<P> = PacketExtractor<P, M, D, N>;

fn config<P: Proto>(c: &Map<String, Value>) -> PacketExtractorConfig<P> {
    PacketExtractorConfig::<P>::new(
        P::params(c),
        ValidPvns::from_versions(&pvns(c)).unwrap(),
        usize::try_from(num(&c["max_packet_len"])).unwrap(),
    )
    .unwrap()
    .with_delivery(
        c["deliver_incomplete"].as_bool().unwrap(),
        c["deliver_idle"].as_bool().unwrap(),
    )
}

fn output(o: &ExtractorOutput, area: &[u8]) -> Value {
    match o {
        ExtractorOutput::Packet {
            pvn,
            packet,
            incomplete,
        } => json!({"deliver": {
            "kind": "Packet",
            "pvn": pvn,
            "data_hex": hex::encode(packet.slice(area).unwrap()),
            "incomplete": incomplete,
        }}),
        ExtractorOutput::Discarded { octets } => {
            json!({"notify": {"kind": "Discarded", "octets": octets}})
        }
        ExtractorOutput::Error(e) => json!({"extraction_error": {"kind": match e {
            ExtractionError::PacketTooLong => "PacketTooLong",
            ExtractionError::InvalidPvn => "InvalidPvn",
            ExtractionError::LengthInconsistent => "LengthInconsistent",
        }}}),
    }
}

/// The value of the counter `name` of the vectors (04 §5, and `idle_packets`).
fn counter<P: Proto>(engine: &Engine<P>, name: &str) -> u64 {
    let s = engine.status();
    match name {
        "PacketTooLong" => s.packet_too_long.get(),
        "InvalidPvn" => s.invalid_pvn.get(),
        "LengthInconsistent" => s.length_inconsistent.get(),
        "idle_packets" => s.idle_packets.get(),
        other => panic!("unknown counter {other}"),
    }
}

fn run_scenario<P: Proto>(scenario: &aldis_testkit::ResolvedScenario<'_>) -> Option<String> {
    let mut engine = Engine::<P>::new(config::<P>(scenario.config)).unwrap();
    let mut area = vec![0u8; M + D];
    for (i, step) in scenario.steps.iter().enumerate() {
        let (key, v) = step.input.iter().next().unwrap();
        let data;
        let got: Vec<Value> = match key.as_str() {
            "data_field" => {
                data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                let input = ExtractorInput::DataField(FieldIn {
                    field: &data,
                    header_pointer: v.get("fhp").map(|f| u16::try_from(num(f)).unwrap()),
                    gap: v["gap"].as_bool().unwrap(),
                });
                match engine.step(input, &mut area) {
                    Ok(actions) => actions.iter().map(|o| output(o, &area)).collect(),
                    Err(e) => vec![json!({"error": e.kind().name()})],
                }
            }
            "reconfigure" => {
                let same = *engine.config();
                let actions = engine
                    .step(ExtractorInput::Reconfigure(same), &mut area)
                    .unwrap();
                actions.iter().map(|o| output(o, &area)).collect()
            }
            "query" => {
                let name = v["counter"].as_str().unwrap();
                vec![json!({"counter": {"name": name, "value": counter(&engine, name)}})]
            }
            other => panic!("unknown input {other}"),
        };
        let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
        if got != want {
            return Some(format!(
                "{} step {i} {}:\n      got  {}\n      want {}",
                scenario.scenario.id,
                Value::Object(step.input.clone()),
                Value::Array(got),
                Value::Array(want)
            ));
        }
    }
    None
}

fn run_file(name: &str) -> (usize, Vec<String>) {
    let path = aldis_testkit::vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = file;
    assert_eq!(file.engine, "packet-extractor");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        failures.extend(match scenario.config["protocol"].as_str().unwrap() {
            p if p == Tm::NAME => run_scenario::<Tm>(scenario),
            p if p == Aos::NAME => run_scenario::<Aos>(scenario),
            p if p == Uslp::NAME => run_scenario::<Uslp>(scenario),
            other => panic!("no engine for {other}"),
        });
    }
    (resolved.len(), failures)
}

#[test]
fn the_packet_extractor_scenarios_match_exactly() {
    // req: LNK-003, LNK-060, LNK-061, LNK-062, LNK-063, LNK-064, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 63, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
