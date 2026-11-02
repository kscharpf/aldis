//! Runs the `AccessGenerator` conformance scenarios of `test-vectors/link/send/` against
//! `AccessGenerator::step` (LNK-003, LNK-020, TV-010…TV-012, TV-023). Every step's outputs must
//! match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::ids::MapId;
use aldis_link::access::{
    AccessGenerator, AccessGeneratorConfig, AccessInput, AccessOutput, AccessProtocol,
};
use aldis_link::common::Qos;
use aldis_link::protocol::{Aos, Tm, Uslp, UslpParams};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use aldis_uslp::Upid;
use serde_json::{Map, Value, json};

const FILES: [&str; 3] = [
    "link/send/access_generator_tm.json",
    "link/send/access_generator_aos.json",
    "link/send/access_generator_uslp.json",
];

/// The data field and the SDU queue of the engines under test.
const D: usize = 16;
const Q: usize = 128;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

/// A protocol of the scenarios, with its parameters from a scenario's `config`.
trait Proto: AccessProtocol {
    fn params(c: &Map<String, Value>) -> Self::Params;
}

impl Proto for Tm {
    fn params(_: &Map<String, Value>) {}
}

impl Proto for Aos {
    fn params(_: &Map<String, Value>) {}
}

impl Proto for Uslp {
    fn params(c: &Map<String, Value>) -> UslpParams {
        UslpParams::new(
            Upid::from_bits(u8::try_from(num(&c["upid"])).unwrap()).unwrap(),
            MapId::<4>::new(u8::try_from(num(&c["map_id"])).unwrap()).unwrap(),
            match c["qos"].as_str().unwrap() {
                "sequence_controlled" => Qos::SequenceControlled,
                "expedited" => Qos::Expedited,
                other => panic!("unknown qos {other}"),
            },
        )
        .unwrap()
    }
}

fn config<P: Proto>(c: &Map<String, Value>) -> AccessGeneratorConfig<P> {
    let vc = u8::try_from(num(&c["vc"])).unwrap();
    let mut config = AccessGeneratorConfig::<P>::new(
        P::Vc::try_from(vc).unwrap(),
        P::params(c),
        usize::try_from(num(&c["field_len"])).unwrap(),
    )
    .unwrap();
    if let Some(n) = c.get("max_sdu_len") {
        config = config
            .with_max_sdu_len(usize::try_from(num(n)).unwrap())
            .unwrap();
    }
    if let Some(p) = c.get("idle_pattern_hex") {
        config = config.with_idle_pattern(hex::decode(p.as_str().unwrap()).unwrap()[0]);
    }
    config
}

fn output(o: &AccessOutput, area: &[u8]) -> Value {
    match o {
        AccessOutput::DataField(f) => {
            let mut v = Map::new();
            v.insert("vc".to_owned(), json!(f.vc));
            v.insert(
                "field_hex".to_owned(),
                json!(hex::encode(f.field.slice(area).unwrap())),
            );
            if let Some(lvo) = f.pointer {
                v.insert("lvo".to_owned(), json!(lvo));
            }
            if let Some(route) = f.route {
                v.insert("map".to_owned(), json!(route.map.get()));
                v.insert(
                    "qos".to_owned(),
                    json!(match route.qos {
                        Qos::SequenceControlled => "sequence_controlled",
                        Qos::Expedited => "expedited",
                    }),
                );
            }
            json!({ "data_field": v })
        }
        AccessOutput::Discarded { octets } => {
            json!({"notify": {"kind": "Discarded", "octets": octets}})
        }
    }
}

fn run_scenario<P: Proto>(scenario: &aldis_testkit::ResolvedScenario<'_>) -> Vec<String> {
    let mut engine = AccessGenerator::<P, Q>::new(config::<P>(scenario.config)).unwrap();
    let mut area = vec![0xEE_u8; D];
    for (i, step) in scenario.steps.iter().enumerate() {
        let (key, v) = step.input.iter().next().unwrap();
        let data;
        let input = match key.as_str() {
            "submit_sdu" => {
                data = hex::decode(v["sdu_hex"].as_str().unwrap()).unwrap();
                AccessInput::SubmitSdu(&data)
            }
            "release" => AccessInput::Release,
            "reconfigure" => {
                AccessInput::Reconfigure(config::<P>(v["config"].as_object().unwrap()))
            }
            other => panic!("unknown input {other}"),
        };
        area.fill(0xEE);
        let got: Vec<Value> = match engine.step(input, &mut area) {
            Ok(actions) => actions.iter().map(|o| output(o, &area)).collect(),
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
            return vec![format!(
                "{} step {i} {}:\n      got  {}\n      want {}",
                scenario.scenario.id,
                Value::Object(step.input.clone()),
                Value::Array(got),
                Value::Array(want)
            )];
        }
    }
    Vec::new()
}

fn run_file(name: &str) -> (usize, Vec<String>) {
    let path = aldis_testkit::vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = file;
    assert_eq!(file.engine, "access-generator");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        failures.extend(match scenario.config["protocol"].as_str().unwrap() {
            "tm" => run_scenario::<Tm>(scenario),
            "aos" => run_scenario::<Aos>(scenario),
            "uslp" => run_scenario::<Uslp>(scenario),
            other => panic!("no engine for {other}"),
        });
    }
    (resolved.len(), failures)
}

#[test]
fn the_access_generator_scenarios_match_exactly() {
    // req: LNK-003, LNK-020, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 10, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
