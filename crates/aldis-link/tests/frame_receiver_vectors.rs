//! Runs the `FrameReceiver` conformance scenarios of `test-vectors/link/recv/` against
//! `FrameReceiver::step` (LNK-050…LNK-054, LNK-077, LNK-079…LNK-081, LNK-003, TV-010…TV-012,
//! TV-023). Every step's outputs must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

mod support;

use aldis_core::engine::OutRange;
use aldis_core::ids::MapId;
use aldis_core::ocf::Ocf;
use aldis_link::protocol::{Aos, Tc, Tm, Uslp};
use aldis_link::receiver::{
    AosChannel, AosContent, AosVc, CallerDiscardReason, ChannelScope, DiscardReason, FrameReceiver,
    FrameReceiverConfig, MapSet, ReceiveProtocol, ReceiverInput, ReceiverOutput, Service,
    TcChannel, TcVc, TmChannel, TmContent, TmVc, UslpChannel, UslpVc, VcEntry,
};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use serde_json::{Map, Value, json};
use support::Plained;

const FILES: [&str; 4] = [
    "link/recv/frame_receiver_tm.json",
    "link/recv/frame_receiver_tc.json",
    "link/recv/frame_receiver_aos.json",
    "link/recv/frame_receiver_uslp.json",
];

/// The VC capacity of the receivers under test.
const V: usize = 8;

fn num(v: &Value) -> usize {
    usize::try_from(v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))).unwrap()
}

fn service(v: &Value) -> Service {
    match v.as_str().unwrap() {
        "none" => Service::None,
        "vc" => Service::Vc,
        "mc" => Service::Mc,
        other => panic!("unknown service {other}"),
    }
}

/// A protocol of the receiver whose configuration a scenario's `config` gives.
trait Proto: ReceiveProtocol + Plained {
    const NAME: &'static str;
    fn channel(c: &Map<String, Value>) -> Self::Channel;
    fn vc_params(vc: &Value, c: &Map<String, Value>) -> Self::VcParams;
}

impl Proto for Tm {
    const NAME: &'static str = "tm";
    fn channel(c: &Map<String, Value>) -> TmChannel {
        TmChannel::new(service(&c["ocf_service"]), service(&c["fsh_service"]))
    }
    fn vc_params(vc: &Value, _: &Map<String, Value>) -> TmVc {
        let content = match vc["role"].as_str().unwrap() {
            "packets" => TmContent::Packets,
            "vca" => TmContent::Vca,
            "unserved" => TmContent::Unserved,
            other => panic!("unknown role {other}"),
        };
        let fsh = num(&vc["fsh_len"]);
        TmVc::new(
            content,
            vc["ocf"].as_bool().unwrap(),
            (fsh > 0).then_some(fsh),
        )
        .unwrap()
    }
}

impl Proto for Aos {
    const NAME: &'static str = "aos";
    fn channel(c: &Map<String, Value>) -> AosChannel {
        AosChannel::new(num(&c["insert_zone_len"]), false)
    }
    fn vc_params(vc: &Value, _: &Map<String, Value>) -> AosVc {
        let content = match vc["role"].as_str().unwrap() {
            "packets" => AosContent::Packets,
            "vca" => AosContent::Vca,
            "bitstream" => AosContent::Bitstream,
            "unserved" => AosContent::Unserved,
            other => panic!("unknown role {other}"),
        };
        AosVc::new(content, vc["ocf"].as_bool().unwrap())
    }
}

impl Proto for Uslp {
    const NAME: &'static str = "uslp";
    fn channel(c: &Map<String, Value>) -> UslpChannel {
        UslpChannel::new(num(&c["insert_zone_len"]), service(&c["ocf_service"]))
    }
    fn vc_params(vc: &Value, _: &Map<String, Value>) -> UslpVc {
        let maps: Vec<MapId<4>> = vc["maps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| MapId::<4>::new(u8::try_from(num(m)).unwrap()).unwrap())
            .collect();
        UslpVc::new(
            MapSet::new(&maps),
            vc["cop"].as_bool().unwrap(),
            vc.get("role").and_then(Value::as_str) != Some("unserved"),
        )
    }
}

impl Proto for Tc {
    const NAME: &'static str = "tc";
    fn channel(_: &Map<String, Value>) -> TcChannel {
        TcChannel
    }
    fn vc_params(vc: &Value, _: &Map<String, Value>) -> TcVc {
        if vc.get("role").and_then(Value::as_str) == Some("unserved") {
            TcVc::UNSERVED
        } else {
            TcVc::SERVED
        }
    }
}

fn config<P: Proto>(c: &Map<String, Value>) -> FrameReceiverConfig<P, V> {
    let vcs: Vec<VcEntry<P>> = c["vcs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|vc| {
            let id = u8::try_from(num(&vc["vcid"])).unwrap();
            VcEntry::new(P::Vc::try_from(id).unwrap(), P::vc_params(vc, c))
        })
        .collect();
    let config = FrameReceiverConfig::<P, V>::new(
        num(&c["frame_len"]),
        c["fecf"].as_bool().unwrap(),
        P::channel(c),
        &vcs,
    )
    .unwrap();
    config
        .with_raw_frames(
            c.get("accept_rejected_frame_insert_zone")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .unwrap()
}

fn scope<V: Into<u8>>(scope: ChannelScope<V>) -> (Value, Option<u8>) {
    match scope {
        ChannelScope::MasterChannel => (json!("mc"), None),
        ChannelScope::VirtualChannel(vc) => (json!("vc"), Some(vc.into())),
    }
}

fn with(mut m: Map<String, Value>, key: &str, v: Option<Value>) -> Map<String, Value> {
    if let Some(v) = v {
        m.insert(key.to_owned(), v);
    }
    m
}

fn bytes(range: OutRange, area: &[u8]) -> String {
    hex::encode(range.slice(area).unwrap())
}

/// The outputs of the vectors for an action.
fn output<P: Proto>(o: &ReceiverOutput<P>, area: &[u8]) -> Vec<Value>
where
    P::Vc: Into<u8>,
{
    let kind = |k: &str| {
        let mut m = Map::new();
        m.insert("kind".to_owned(), json!(k));
        m
    };
    let v = match o {
        ReceiverOutput::FrameGap { scope: s, missing } => {
            let (sc, vc) = scope(*s);
            let m = with(kind("FrameGap"), "vc", vc.map(|v| json!(v)));
            let mut m = with(m, "scope", Some(sc));
            m.insert("missing".to_owned(), json!(missing));
            json!({"notify": m})
        }
        ReceiverOutput::CountDiscontinuity { scope: s } => {
            let (sc, vc) = scope(*s);
            let m = with(kind("CountDiscontinuity"), "vc", vc.map(|v| json!(v)));
            json!({"notify": with(m, "scope", Some(sc))})
        }
        ReceiverOutput::FrameDiscarded { reason, vc, map } => {
            let name = match reason {
                DiscardReason::UnknownChannel => "UnknownChannel",
                DiscardReason::NotServed => "NotServed",
                DiscardReason::CrcMismatch => "CrcMismatch",
                DiscardReason::HeaderUncorrectable => "HeaderUncorrectable",
                DiscardReason::Security => "Security",
            };
            let mut m = Map::new();
            m.insert("reason".to_owned(), json!(name));
            let m = with(m, "vc", vc.map(|v| json!(v.into())));
            let m = with(m, "map", map.map(|v| json!(v.get())));
            json!({"frame_discarded": m})
        }
        ReceiverOutput::LossFlag => panic!("expanded by the caller"),
        ReceiverOutput::DataField {
            vc,
            data,
            bit_count,
            pointer,
            gap,
            replay,
        } => {
            let mut m = kind("DataField");
            m.insert("vc".to_owned(), json!((*vc).into()));
            m.insert("data_hex".to_owned(), json!(bytes(*data, area)));
            m.insert("gap".to_owned(), json!(gap));
            let m = with(m, "fhp", pointer.map(|p| json!(p)));
            let m = with(m, "bit_count", bit_count.map(|b| json!(b)));
            json!({"deliver": with(m, "replay", replay.map(|r| json!(r)))})
        }
        ReceiverOutput::Ocf {
            scope: s,
            ocf,
            replay,
        } => {
            let (sc, vc) = scope(*s);
            let mut m = kind("Ocf");
            m.insert("scope".to_owned(), sc);
            m.insert(
                "report".to_owned(),
                json!(if matches!(ocf, Ocf::Clcw(_)) {
                    "Type1"
                } else {
                    "Type2"
                }),
            );
            m.insert("ocf_hex".to_owned(), json!(hex::encode(&ocf.to_octets())));
            let m = with(m, "vc", vc.map(|v| json!(v)));
            json!({"deliver": with(m, "replay", replay.map(|r| json!(r)))})
        }
        ReceiverOutput::FshSdu { scope: s, sdu } => {
            let (sc, vc) = scope(*s);
            let mut m = kind("FshSdu");
            m.insert("scope".to_owned(), sc);
            m.insert("data_hex".to_owned(), json!(bytes(*sdu, area)));
            json!({"deliver": with(m, "vc", vc.map(|v| json!(v)))})
        }
        ReceiverOutput::InsertSdu { sdu, replay } => {
            let mut m = kind("InsertSdu");
            m.insert("data_hex".to_owned(), json!(bytes(*sdu, area)));
            json!({"deliver": with(m, "replay", replay.map(|r| json!(r)))})
        }
        ReceiverOutput::TcFrame { vc, frame } | ReceiverOutput::ProtocolControl { vc, frame } => {
            let k = if matches!(o, ReceiverOutput::TcFrame { .. }) {
                "TcFrame"
            } else {
                "ProtocolControl"
            };
            let mut m = kind(k);
            m.insert("vc".to_owned(), json!((*vc).into()));
            m.insert("data_hex".to_owned(), json!(bytes(*frame, area)));
            json!({"deliver": m})
        }
    };
    vec![v]
}

fn counter<P: Proto>(engine: &FrameReceiver<P, V>, name: &str) -> u64 {
    let s = engine.status();
    match name {
        "UnknownChannel" => s.unknown_channel.get(),
        "NotServed" => s.not_served.get(),
        "CrcMismatch" => s.crc_mismatch.get(),
        "HeaderUncorrectable" => s.header_uncorrectable.get(),
        "Security" => s.security.get(),
        "oid_frames" => s.oid_frames.get(),
        "gaps" => s.gaps.get(),
        "count_discontinuities" => s.count_discontinuities.get(),
        other => panic!("unknown counter {other}"),
    }
}

fn run_scenario<P: Proto>(scenario: &aldis_testkit::ResolvedScenario<'_>) -> Option<String>
where
    P::Vc: Into<u8>,
{
    let config = config::<P>(scenario.config);
    let mut engine = FrameReceiver::<P, V>::new(config);
    let mut area = vec![0u8; config.area_len()];
    for (i, step) in scenario.steps.iter().enumerate() {
        let (key, v) = step.input.iter().next().unwrap();
        let got: Vec<Value> = match key.as_str() {
            "frame" | "raw_frame" => {
                let data = hex::decode(v["frame_hex"].as_str().unwrap()).unwrap();
                // A frame reaches the receiver as the codec's `Plain` frame (RS-023); a frame the
                // codec refuses is the step's error.
                let result = if key == "frame" {
                    P::plain(&config, &data)
                        .and_then(|plain| engine.step(ReceiverInput::Frame(plain), &mut area))
                } else {
                    engine.step(ReceiverInput::RawFrame(&data), &mut area)
                };
                match result {
                    Ok(actions) => actions.iter().flat_map(|o| output(o, &area)).collect(),
                    Err(e) => vec![match e.field() {
                        Some(f) => json!({"error": e.kind().name(), "field": f.name()}),
                        None => json!({"error": e.kind().name()}),
                    }],
                }
            }
            "caller_discard" => {
                let reason = match v["reason"].as_str().unwrap() {
                    "CrcMismatch" => CallerDiscardReason::CrcMismatch,
                    "HeaderUncorrectable" => CallerDiscardReason::HeaderUncorrectable,
                    "Security" => CallerDiscardReason::Security,
                    other => panic!("unknown reason {other}"),
                };
                let actions = engine
                    .step(ReceiverInput::CallerDiscard(reason), &mut area)
                    .unwrap();
                actions.iter().flat_map(|o| output(o, &area)).collect()
            }
            "reconfigure" => {
                let actions = engine
                    .step(ReceiverInput::Reconfigure(config), &mut area)
                    .unwrap();
                actions.iter().flat_map(|o| output(o, &area)).collect()
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
    assert_eq!(file.engine, "frame-receiver");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    let mut compared = 0;
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        compared = usize::saturating_add(compared, 1);
        failures.extend(match scenario.config["protocol"].as_str().unwrap() {
            p if p == Tm::NAME => run_scenario::<Tm>(scenario),
            p if p == Tc::NAME => run_scenario::<Tc>(scenario),
            p if p == Aos::NAME => run_scenario::<Aos>(scenario),
            p if p == Uslp::NAME => run_scenario::<Uslp>(scenario),
            other => panic!("no engine for {other}"),
        });
    }
    (compared, failures)
}

#[test]
fn the_frame_receiver_scenarios_match_exactly() {
    // req: LNK-003, LNK-050, LNK-051, LNK-052, LNK-053, LNK-054, LNK-077, LNK-079, LNK-080, LNK-081, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    // 59 scenarios, 3 of them (FrameLoss) superseded by the decision of 2026-11-02.
    assert!(run >= 56, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
