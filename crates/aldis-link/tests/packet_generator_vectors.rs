//! Runs the `PacketGenerator` conformance scenarios of `test-vectors/link/send/` against
//! `PacketGenerator::step` (LNK-003, LNK-010…LNK-015, TV-010…TV-012, TV-023). Every step's
//! outputs and deadline must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

#[path = "common/mod.rs"]
mod common;

use aldis_core::time::{Duration, Instant};
use aldis_link::common::{IdlePacketKind, IdlePolicy, Qos, ValidPvns};
use aldis_link::generator::{
    GeneratorInput, GeneratorOutput, PacketGenerator, PacketGeneratorConfig,
};
use aldis_link::protocol::{Aos, Tm, Uslp};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use common::{Proto, num, pvns};
use serde_json::{Map, Value, json};

const FILES: [&str; 4] = [
    "link/send/packet_generator_tm.json",
    "link/send/packet_generator_aos.json",
    "link/send/packet_generator_aos_issue4.json",
    "link/send/packet_generator_uslp.json",
];

/// The data field capacity: the longest field of the files (USLP: a 3-octet header and a
/// 24-octet zone).
const D: usize = 27;

fn config<P: Proto>(c: &Map<String, Value>) -> PacketGeneratorConfig<P> {
    let vc = u8::try_from(num(&c["vc"])).unwrap();
    let mut config = PacketGeneratorConfig::<P>::new(
        P::Vc::try_from(vc).unwrap(),
        P::params(c),
        usize::try_from(num(&c["field_len"])).unwrap(),
        ValidPvns::from_versions(&pvns(c)).unwrap(),
        usize::try_from(num(&c["max_packet_len"])).unwrap(),
    )
    .unwrap();
    let kind = match c.get("idle_packet_kind").and_then(Value::as_str) {
        Some("space_packet") => IdlePacketKind::SpacePacket,
        // USLP: the protocol fixes the Encapsulation Idle Packet, and the file names none.
        Some("encapsulation") | None => IdlePacketKind::Encapsulation,
        Some(other) => panic!("unknown idle packet kind {other}"),
    };
    let policy = match c["idle_policy"].as_str().unwrap() {
        "oid" => IdlePolicy::Oid,
        "idle_field" => IdlePolicy::IdleField,
        other => panic!("unknown idle policy {other}"),
    };
    let pattern = hex::decode(c["idle_pattern_hex"].as_str().unwrap()).unwrap();
    config = config
        .with_idle(policy, kind, *pattern.first().unwrap())
        .unwrap();
    if let Some(us) = c.get("release_deadline_us") {
        config = config.with_release_deadline(Duration::from_micros(num(us)));
    }
    config
}

fn output(o: &GeneratorOutput, area: &[u8]) -> Value {
    match o {
        GeneratorOutput::DataField(f) => {
            let mut v = Map::new();
            v.insert("vc".to_owned(), json!(f.vc));
            v.insert(
                "field_hex".to_owned(),
                json!(hex::encode(f.field.slice(area).unwrap())),
            );
            v.insert("fhp".to_owned(), json!(f.pointer));
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
        GeneratorOutput::Discarded { octets } => {
            json!({"notify": {"kind": "Discarded", "octets": octets}})
        }
    }
}

fn run_scenarios<P: Proto, const Q: usize>(
    scenario: &aldis_testkit::ResolvedScenario<'_>,
) -> Vec<String> {
    let mut engine = PacketGenerator::<P, D, Q>::new(config::<P>(scenario.config)).unwrap();
    let mut area = vec![0u8; D];
    for (i, step) in scenario.steps.iter().enumerate() {
        let now = Instant::from_micros(step.at_us);
        let (key, v) = step.input.iter().next().unwrap();
        let data;
        let input = match key.as_str() {
            "submit_packet" => {
                data = hex::decode(v["packet_hex"].as_str().unwrap()).unwrap();
                GeneratorInput::SubmitPacket(&data)
            }
            "release" => GeneratorInput::Release,
            "timeout" => GeneratorInput::Timeout,
            "reconfigure" => {
                GeneratorInput::Reconfigure(config::<P>(v["config"].as_object().unwrap()))
            }
            other => panic!("unknown input {other}"),
        };
        let got: Vec<Value> = match engine.step(input, now, &mut area) {
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
        let mut problems = Vec::new();
        if got != want {
            problems.push(format!(
                "outputs\n      got  {}\n      want {}",
                Value::Array(got),
                Value::Array(want)
            ));
        }
        let deadline = engine.next_deadline().map(Instant::as_micros);
        if (step.expect_timeout_us.is_some() || step.expect_no_timeout)
            && deadline != step.expect_timeout_us
        {
            problems.push(format!(
                "deadline {deadline:?} want {:?}",
                step.expect_timeout_us
            ));
        }
        if !problems.is_empty() {
            return vec![format!(
                "{} step {i} {}: {}",
                scenario.scenario.id,
                Value::Object(step.input.clone()),
                problems.join("; ")
            )];
        }
    }
    Vec::new()
}

fn run_file(name: &str) -> (usize, Vec<String>) {
    let path = aldis_testkit::vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(parsed)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = parsed;
    assert_eq!(file.engine, "packet-generator");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        let queue = num(&scenario.config["queue_capacity"]);
        let protocol = scenario.config["protocol"].as_str().unwrap();
        failures.extend(match (protocol, queue) {
            ("tm", 64) => run_scenarios::<Tm, 64>(scenario),
            ("tm", 48) => run_scenarios::<Tm, 48>(scenario),
            ("aos", 64) => run_scenarios::<Aos, 64>(scenario),
            ("aos", 48) => run_scenarios::<Aos, 48>(scenario),
            ("uslp", 64) => run_scenarios::<Uslp, 64>(scenario),
            ("uslp", 48) => run_scenarios::<Uslp, 48>(scenario),
            other => panic!("no engine for {other:?}"),
        });
    }
    (resolved.len(), failures)
}

#[test]
fn the_packet_generator_scenarios_match_exactly() {
    // req: LNK-003, LNK-010, LNK-011, LNK-012, LNK-013, LNK-014, LNK-015, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 50, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
