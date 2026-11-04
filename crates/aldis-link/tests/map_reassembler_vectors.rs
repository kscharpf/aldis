//! Runs the `MapReassembler` conformance scenarios of `test-vectors/link/recv/` against
//! `MapReassembler::step` (LNK-003, LNK-070…LNK-072, TV-010…TV-012, TV-023). Every step's outputs
//! must match exactly, a run of packets as one output for each packet; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::ids::{MapId, VcId};
use aldis_link::extractor::PacketRun;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::reassembler::{
    MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap, ReassemblerOutput,
    ReassemblerProtocol, ReassemblerStatus, ReassemblyEvent, TcReassemblerParams, UslpFdu,
};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use serde_json::{Map, Value, json};

const FILES: [&str; 2] = [
    "link/recv/map_reassembler_tc.json",
    "link/recv/map_reassembler_uslp.json",
];

/// The MAPs of the files, and the longest SDU of the engines under test.
const M: usize = 4;
const S: usize = 64;
/// The output area: the longest SDU and the longest FDU of the files.
const AREA: usize = S + 64;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn small(v: &Value) -> u8 {
    u8::try_from(num(v)).unwrap()
}

/// The `max_sdu_len` of a MAP of a configuration (LNK-073): the engine's capacity when absent.
fn max_sdu_len(m: &Value) -> usize {
    m.get("max_sdu_len")
        .map_or(S, |n| usize::try_from(num(n)).unwrap())
}

/// A protocol of the scenarios, with its configuration and inputs from a scenario.
trait Proto: ReassemblerProtocol<Vc = VcId<6>> {
    fn config(c: &Map<String, Value>) -> MapReassemblerConfig<Self, M>;
    /// The `(vc, meta)` of an `fdu` input.
    fn meta(v: &Value) -> Self::Meta;
}

impl Proto for Tc {
    fn config(c: &Map<String, Value>) -> MapReassemblerConfig<Tc, M> {
        let vc = VcId::<6>::new(small(&c["vc"])).unwrap();
        let maps: Vec<_> = c["maps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let id = aldis_tc::TcMapId::new(small(&m["map"])).unwrap();
                match m["service"].as_str().unwrap() {
                    "packets" => ReassemblerMap::<Tc>::packets(vc, id, S).unwrap(),
                    "sdu" => ReassemblerMap::<Tc>::sdu(vc, id, max_sdu_len(m)).unwrap(),
                    other => panic!("unknown service {other}"),
                }
            })
            .collect();
        MapReassemblerConfig::new(TcReassemblerParams::with_segment_header(), &maps).unwrap()
    }

    fn meta(_: &Value) {}
}

impl Proto for Uslp {
    fn config(c: &Map<String, Value>) -> MapReassemblerConfig<Uslp, M> {
        let maps: Vec<_> = c["maps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let vc = VcId::<6>::new(small(&m["vc"])).unwrap();
                let id = MapId::<4>::new(small(&m["map"])).unwrap();
                match m["service"].as_str().unwrap() {
                    "packets" => ReassemblerMap::<Uslp>::packets(vc, id, S).unwrap(),
                    "sdu" => ReassemblerMap::<Uslp>::sdu(vc, id, max_sdu_len(m)).unwrap(),
                    "octet_stream" => ReassemblerMap::<Uslp>::octet_stream(vc, id),
                    other => panic!("unknown service {other}"),
                }
            })
            .collect();
        MapReassemblerConfig::new((), &maps).unwrap()
    }

    fn meta(v: &Value) -> UslpFdu {
        UslpFdu {
            map: MapId::<4>::new(small(&v["map"])).unwrap(),
            gap: v["gap"].as_bool().unwrap(),
        }
    }
}

const fn event_name(e: ReassemblyEvent) -> &'static str {
    match e {
        ReassemblyEvent::GapDiscard => "GapDiscard",
        ReassemblyEvent::SequenceViolation => "SequenceViolation",
        ReassemblyEvent::UnknownMap => "UnknownMap",
        ReassemblyEvent::SduTooLong => "SduTooLong",
    }
}

fn counter(s: &ReassemblerStatus, name: &str) -> u64 {
    match name {
        "GapDiscard" => s.gap_discards.get(),
        "SequenceViolation" => s.sequence_violations.get(),
        "UnknownMap" => s.unknown_maps.get(),
        "SduTooLong" => s.sdus_too_long.get(),
        other => panic!("unknown counter {other}"),
    }
}

/// The outputs of the vectors for an action: a run of packets is one output for each packet.
fn outputs(o: &ReassemblerOutput, area: &[u8]) -> Vec<Value> {
    match *o {
        ReassemblerOutput::Event { event, vc, map } => {
            vec![json!({"reassembly_event": {"kind": event_name(event), "vc": vc, "map": map}})]
        }
        ReassemblerOutput::Sdu { vc, map, sdu } => vec![json!({"deliver": {
            "kind": "Sdu", "vc": vc, "map": map, "data_hex": hex::encode(sdu.slice(area).unwrap()),
        }})],
        ReassemblerOutput::StreamData {
            vc,
            map,
            data,
            after_gap,
        } => {
            let mut d = Map::new();
            d.insert("kind".to_owned(), json!("Sdu"));
            d.insert("vc".to_owned(), json!(vc));
            d.insert("map".to_owned(), json!(map));
            d.insert("octet_stream".to_owned(), json!(true));
            d.insert(
                "data_hex".to_owned(),
                json!(hex::encode(data.slice(area).unwrap())),
            );
            if after_gap {
                d.insert("after_gap".to_owned(), json!(true));
            }
            vec![json!({"deliver": d})]
        }
        ReassemblerOutput::Packets {
            vc,
            map,
            run,
            count,
        } => {
            let packets: Vec<&[u8]> = PacketRun::new(run, area).unwrap().collect();
            assert_eq!(packets.len(), count);
            packets
                .into_iter()
                .map(|p| {
                    json!({"deliver": {
                        "kind": "Packet", "vc": vc, "map": map, "pvn": PacketRun::pvn(p).unwrap(),
                        "data_hex": hex::encode(p), "incomplete": false,
                    }})
                })
                .collect()
        }
        ReassemblerOutput::Discarded { octets } => {
            vec![json!({"notify": {"kind": "Discarded", "octets": octets}})]
        }
    }
}

fn run_scenario<P: Proto>(scenario: &aldis_testkit::ResolvedScenario<'_>) -> Vec<String> {
    let config = P::config(scenario.config);
    let mut engine = MapReassembler::<P, M, S>::new(config).unwrap();
    let mut area = vec![0xEE_u8; AREA];
    for (i, step) in scenario.steps.iter().enumerate() {
        let (key, v) = step.input.iter().next().unwrap();
        let data;
        let got: Vec<Value> = match key.as_str() {
            "fdu" => {
                data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                let input = ReassemblerInput::Fdu(aldis_link::reassembler::FduIn {
                    vc: VcId::<6>::new(small(&v["vc"])).unwrap(),
                    meta: P::meta(v),
                    data: &data,
                });
                area.fill(0xEE);
                match engine.step(input, &mut area) {
                    Ok(actions) => actions.iter().flat_map(|o| outputs(o, &area)).collect(),
                    Err(e) => {
                        let mut v = Map::new();
                        v.insert("error".to_owned(), json!(e.kind().name()));
                        if let Some(f) = e.field() {
                            v.insert("field".to_owned(), json!(f.name()));
                        }
                        vec![Value::Object(v)]
                    }
                }
            }
            "reconfigure" => {
                // The current configuration is applied again (LNK-003).
                let again = ReassemblerInput::Reconfigure(*engine.config());
                engine
                    .step(again, &mut area)
                    .unwrap()
                    .iter()
                    .flat_map(|o| outputs(o, &area))
                    .collect()
            }
            "query" => {
                let name = v["counter"].as_str().unwrap();
                vec![json!({"counter": {"name": name, "value": counter(engine.status(), name)}})]
            }
            other => panic!("unknown input {other}"),
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
    let Ok(VectorFile::EngineScenario(parsed)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = parsed;
    assert_eq!(file.engine, "map-reassembler");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        failures.extend(match scenario.config["protocol"].as_str().unwrap() {
            "tc" => run_scenario::<Tc>(scenario),
            "uslp" => run_scenario::<Uslp>(scenario),
            other => panic!("no engine for {other}"),
        });
    }
    (resolved.len(), failures)
}

#[test]
fn the_map_reassembler_scenarios_match_exactly() {
    // req: LNK-003, LNK-070, LNK-071, LNK-072, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 17, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
