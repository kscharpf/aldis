//! Runs the `BitstreamGenerator` conformance scenarios of `test-vectors/link/send/` against
//! `BitstreamGenerator::step` (LNK-003, LNK-021, TV-010…TV-012, TV-023). Every step's outputs
//! must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_link::bitstream::{
    BitstreamGenerator, BitstreamGeneratorConfig, BitstreamInput, BitstreamOutput,
};
use aldis_link::protocol::Aos;
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use serde_json::{Map, Value, json};

const FILE: &str = "link/send/bitstream_generator_aos.json";

/// The bit queue of the engine under test, in octets.
const Q: usize = 16;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn config(c: &Map<String, Value>) -> BitstreamGeneratorConfig<Aos> {
    BitstreamGeneratorConfig::<Aos>::new(
        aldis_core::ids::VcId::<6>::new(u8::try_from(num(&c["vc"])).unwrap()).unwrap(),
        usize::try_from(num(&c["field_len"])).unwrap(),
        hex::decode(c["idle_pattern_hex"].as_str().unwrap()).unwrap()[0],
    )
    .unwrap()
}

#[test]
fn the_bitstream_generator_scenarios_match_exactly() {
    // req: LNK-003, LNK-021, TV-012
    let path = aldis_testkit::vectors_dir().join(FILE);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(parsed)) = VectorFile::from_json(&text) else {
        panic!("{FILE}: not an engine scenario file");
    };
    let file: ScenarioFile = parsed;
    assert_eq!(file.engine, "bitstream-generator");
    let resolved = file.resolve().unwrap();
    assert!(resolved.len() >= 4);
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            continue;
        }
        let mut engine = BitstreamGenerator::<Aos, Q>::new(config(scenario.config));
        let mut area = vec![0xEE_u8; 16];
        for (i, step) in scenario.steps.iter().enumerate() {
            let (key, v) = step.input.iter().next().unwrap();
            let bits;
            let input = match key.as_str() {
                "submit_bits" => {
                    bits = hex::decode(v["bits_hex"].as_str().unwrap()).unwrap();
                    BitstreamInput::SubmitBits {
                        bits: &bits,
                        bit_len: usize::try_from(num(&v["bit_len"])).unwrap(),
                    }
                }
                "release" => BitstreamInput::Release,
                "reconfigure" => {
                    BitstreamInput::Reconfigure(config(v["config"].as_object().unwrap()))
                }
                other => panic!("unknown input {other}"),
            };
            area.fill(0xEE);
            let got: Vec<Value> = match engine.step(input, &mut area) {
                Ok(actions) => actions
                    .iter()
                    .map(|o| match o {
                        BitstreamOutput::DataField(f) => json!({"data_field": {
                            "vc": f.vc,
                            "field_hex": hex::encode(f.field.slice(&area).unwrap()),
                            "bdp": f.pointer,
                        }}),
                        BitstreamOutput::Discarded { octets } => {
                            json!({"notify": {"kind": "Discarded", "octets": octets}})
                        }
                    })
                    .collect(),
                Err(e) => {
                    let mut v = Map::new();
                    v.insert("error".to_owned(), json!(e.kind().name()));
                    if let Some(f) = e.field() {
                        v.insert("field".to_owned(), json!(f.name()));
                    }
                    vec![Value::Object(v)]
                }
            };
            let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
            if got != want {
                failures.push(format!(
                    "{} step {i} {}:\n      got  {}\n      want {}",
                    scenario.scenario.id,
                    Value::Object(step.input.clone()),
                    Value::Array(got),
                    Value::Array(want)
                ));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
