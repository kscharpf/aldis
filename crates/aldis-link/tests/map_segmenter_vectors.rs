//! Runs the `MapSegmenter` conformance scenarios of `test-vectors/link/send/` against
//! `MapSegmenter::step` (LNK-003, LNK-012, LNK-040…LNK-042, TV-010…TV-012, TV-023). Every step's
//! outputs and its deadline must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::common::Qos;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::segmenter::{
    Framing, MapEntry, MapSegmenter, MapSegmenterConfig, SegmenterInput, SegmenterOutput,
    SegmenterProtocol, SegmenterService, TcSegmenterParams, UslpSegmenterParams,
};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use aldis_uslp::{Upid, VcfCountLen};
use serde_json::{Map, Value, json};

const FILES: [&str; 2] = [
    "link/send/map_segmenter_tc.json",
    "link/send/map_segmenter_uslp.json",
];

/// The MAPs and the queue of the engines under test.
const M: usize = 4;
const Q: usize = 256;
/// The FDU area: the longest `max_fdu_len` of the files.
const L: usize = 32;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn usize_of(v: &Value) -> usize {
    usize::try_from(num(v)).unwrap()
}

fn qos_of(v: &Value) -> Qos {
    match v.as_str().unwrap() {
        "sequence_controlled" => Qos::SequenceControlled,
        "expedited" => Qos::Expedited,
        other => panic!("unknown qos {other}"),
    }
}

/// A protocol of the scenarios, with its configuration from a scenario's `config`.
trait Proto: SegmenterProtocol {
    fn config(c: &Map<String, Value>) -> MapSegmenterConfig<Self, M>;
    /// The wire name of the framing: TC Sequence Flags or USLP rule, and the UPID if any.
    fn describe(framing: Framing, header: u8, v: &mut Map<String, Value>);
}

fn maps<P: SegmenterProtocol>(
    c: &Map<String, Value>,
    entry: impl Fn(&Value) -> MapEntry<P>,
) -> Vec<MapEntry<P>> {
    c.get("maps")
        .map(|m| m.as_array().unwrap().iter().map(entry).collect())
        .unwrap_or_default()
}

impl Proto for Tc {
    fn config(c: &Map<String, Value>) -> MapSegmenterConfig<Tc, M> {
        let fecf = c["fecf"].as_bool().unwrap();
        let header = c["segment_header"].as_bool().unwrap();
        let channel = if header {
            TcSegmenterParams::with_segment_header(fecf)
        } else {
            TcSegmenterParams::without_segment_header(fecf, c["blocking"].as_bool().unwrap())
        };
        let entries = maps::<Tc>(c, |m| {
            let id = MapId::<6>::new(u8::try_from(num(&m["map_id"])).unwrap()).unwrap();
            let mut e = MapEntry::<Tc>::new(id, SegmenterService::Any, ());
            if m["segmentation"].as_bool().unwrap() {
                e = e.with_segmentation(usize_of(&m["max_sdu_len"])).unwrap();
            }
            if m["blocking"].as_bool().unwrap() {
                e = e.with_blocking();
            }
            e
        });
        MapSegmenterConfig::new(
            VcId::<6>::new(1).unwrap(),
            channel,
            usize_of(&c["max_fdu_len"]),
            &entries,
        )
        .unwrap()
    }

    fn describe(framing: Framing, _: u8, v: &mut Map<String, Value>) {
        let flags = match framing {
            Framing::Whole => "11",
            Framing::First => "01",
            Framing::Continuing => "00",
            Framing::Last => "10",
            Framing::Stream => panic!("TC has no stream"),
        };
        v.insert("flags".to_owned(), json!(flags));
    }
}

impl Proto for Uslp {
    fn config(c: &Map<String, Value>) -> MapSegmenterConfig<Uslp, M> {
        let count = VcfCountLen::new(2).unwrap();
        let entries = maps::<Uslp>(c, |m| {
            let id = MapId::<4>::new(u8::try_from(num(&m["map_id"])).unwrap()).unwrap();
            let upid = Upid::from_bits(u8::try_from(num(&m["upid"])).unwrap()).unwrap();
            let service = match m["service"].as_str().unwrap() {
                "packets" => SegmenterService::Packets,
                "mapa" => SegmenterService::Sdu,
                "octet_stream" => SegmenterService::OctetStream {
                    release_deadline: Duration::from_micros(num(&m["release_deadline_us"])),
                },
                other => panic!("unknown service {other}"),
            };
            let mut e = MapEntry::<Uslp>::new(id, service, upid);
            if let Some(max) = m.get("max_sdu_len") {
                e = e.with_segmentation(usize_of(max)).unwrap();
            }
            if m.get("blocking").is_some_and(|b| b.as_bool().unwrap()) {
                e = e.with_blocking();
            }
            e
        });
        MapSegmenterConfig::new(
            VcId::<6>::new(1).unwrap(),
            UslpSegmenterParams::new(count, count),
            usize_of(&c["max_tfdf_len"]),
            &entries,
        )
        .unwrap()
    }

    fn describe(framing: Framing, header: u8, v: &mut Map<String, Value>) {
        let rule = match framing {
            Framing::Whole => "111",
            Framing::First => "100",
            Framing::Continuing => "101",
            Framing::Last => "110",
            Framing::Stream => "011",
        };
        v.insert("rule".to_owned(), json!(rule));
        v.insert("upid".to_owned(), json!(header & 0x1F));
    }
}

fn output<P: Proto>(o: &SegmenterOutput, area: &[u8], header: bool) -> Value {
    match o {
        SegmenterOutput::Fdu(f) => {
            let octets = f.fdu.slice(area).unwrap();
            let mut v = Map::new();
            v.insert("handle".to_owned(), json!(f.handle));
            v.insert(
                "qos".to_owned(),
                json!(match f.qos {
                    Qos::SequenceControlled => "sequence_controlled",
                    Qos::Expedited => "expedited",
                }),
            );
            v.insert("fdu_hex".to_owned(), json!(hex::encode(octets)));
            if let Some(map) = f.map {
                v.insert("map".to_owned(), json!(map));
            }
            if header {
                P::describe(f.framing, octets[0], &mut v);
            }
            json!({ "fdu": v })
        }
        SegmenterOutput::Discarded { octets } => {
            json!({"notify": {"kind": "Discarded", "octets": octets}})
        }
    }
}

fn run_scenario<P: Proto>(scenario: &aldis_testkit::ResolvedScenario<'_>) -> Vec<String> {
    let mut engine = MapSegmenter::<P, M, Q>::new(P::config(scenario.config)).unwrap();
    // A TC VC without a Segment Header has no flags to report.
    let header = scenario
        .config
        .get("segment_header")
        .is_none_or(|h| h.as_bool().unwrap());
    let map_of = |v: &Value| {
        let id = v.get("map").map_or(0, num);
        P::Map::try_from(u8::try_from(id).unwrap()).unwrap()
    };
    let mut area = vec![0xEE_u8; L];
    for (i, step) in scenario.steps.iter().enumerate() {
        let now = Instant::from_micros(step.at_us);
        let (key, v) = step.input.iter().next().unwrap();
        let data;
        let input = match key.as_str() {
            "submit_packet" => {
                data = hex::decode(v["packet_hex"].as_str().unwrap()).unwrap();
                SegmenterInput::SubmitPacket {
                    map: map_of(v),
                    packet: &data,
                }
            }
            "submit_sdu" => {
                data = hex::decode(v["sdu_hex"].as_str().unwrap()).unwrap();
                SegmenterInput::SubmitSdu {
                    map: map_of(v),
                    sdu: &data,
                }
            }
            "submit_octets" => {
                data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                SegmenterInput::SubmitOctets {
                    map: map_of(v),
                    octets: &data,
                }
            }
            "next_fdu" => SegmenterInput::NextFdu {
                handle: u32::try_from(num(&v["handle"])).unwrap(),
                qos: qos_of(&v["qos"]),
            },
            "timeout" => SegmenterInput::Timeout,
            "reconfigure" => {
                SegmenterInput::Reconfigure(P::config(v["config"].as_object().unwrap()))
            }
            other => panic!("unknown input {other}"),
        };
        area.fill(0xEE);
        let got: Vec<Value> = match engine.step(input, now, &mut area) {
            Ok(actions) => actions
                .iter()
                .map(|o| output::<P>(o, &area, header))
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
    assert_eq!(file.engine, "map-segmenter");
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
fn the_map_segmenter_scenarios_match_exactly() {
    // req: LNK-003, LNK-012, LNK-040, LNK-041, LNK-042, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 18, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
