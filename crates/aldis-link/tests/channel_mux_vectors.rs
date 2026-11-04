//! Runs the `ChannelMux` conformance scenarios of `test-vectors/link/send/` against
//! `ChannelMux::step` (LNK-003, LNK-030…LNK-036, LNK-076, LNK-082, TV-010…TV-012, TV-023).
//! Every step's outputs must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_aos::AosEdition;
use aldis_core::ids::{MapId, SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;
use aldis_link::common::Qos;
use aldis_link::mux::{
    AosChannel, AosVc, ChannelMux, ChannelMuxConfig, Cop, FrameClass, Fsh, MuxInput, MuxOutput,
    MuxProtocol, Offer, Scope, TmChannel, TmRole, TmVc, UslpChannel, UslpVc, VcConfig,
};
use aldis_link::policy::{MuxPolicy, Weight};
use aldis_link::protocol::{Aos, Route, Tm, Uslp};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use aldis_uslp::{SourceOrDestination, VcfCountLen};
use serde_json::{Map, Value, json};

const FILES: [&str; 3] = [
    "link/send/channel_mux_tm.json",
    "link/send/channel_mux_aos.json",
    "link/send/channel_mux_uslp.json",
];

/// The VCs and the frame length of the engines under test.
const V: usize = 8;
const F: usize = 64;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn small(v: &Value) -> u8 {
    u8::try_from(num(v)).unwrap()
}

fn octets(c: &Map<String, Value>, key: &str) -> Option<Vec<u8>> {
    c.get(key)
        .map(|v| hex::decode(v.as_str().unwrap()).unwrap())
}

fn ocf(c: &Map<String, Value>, key: &str) -> Option<[u8; 4]> {
    octets(c, key).map(|o| o.try_into().unwrap())
}

fn policy(c: &Map<String, Value>) -> MuxPolicy {
    match c["policy"]["kind"].as_str().unwrap() {
        "fixed_priority" => MuxPolicy::FixedPriority,
        "round_robin" => MuxPolicy::RoundRobin,
        "weighted_round_robin" => MuxPolicy::WeightedRoundRobin,
        other => panic!("unknown policy {other}"),
    }
}

/// The VC of `c`'s entry `v`, with the parameters every protocol has.
fn vc<P: MuxProtocol>(v: &Value, params: P::VcParams) -> VcConfig<P> {
    let c = v.as_object().unwrap();
    let get = |k: &str| c.get(k).map_or(0, |n| usize::try_from(num(n)).unwrap());
    VcConfig::<P>::new(P::Vc::try_from(small(&c["vcid"])).unwrap(), params)
        .with_priority(small(&c["priority"]))
        .with_weight(Weight::new(u16::try_from(num(&c["weight"])).unwrap()).unwrap())
        .with_security(SecurityLayout::new(get("sec_header_len"), get("sec_trailer_len")).unwrap())
}

/// A protocol of the scenarios, with its configuration from a scenario's `config`.
trait Proto: MuxProtocol {
    /// The configuration; COP-1 is in effect on the USLP VCs whose entry has `"cop": true`.
    fn config(c: &Map<String, Value>) -> ChannelMuxConfig<Self, V>;
    /// The default MAP and `QoS` of VC `vcid` (USLP).
    fn route(_: &Map<String, Value>, _: u8) -> Option<Route> {
        None
    }
}

fn finish<P: MuxProtocol>(
    c: &Map<String, Value>,
    channel: P::Channel,
    vcs: &[VcConfig<P>],
) -> ChannelMuxConfig<P, V> {
    let config = ChannelMuxConfig::<P, V>::new(
        channel,
        usize::try_from(num(&c["frame_len"])).unwrap(),
        c["fecf"].as_bool().unwrap(),
        policy(c),
        vcs,
    )
    .unwrap();
    match c.get("insert_zone_len").map(num) {
        Some(len) if len > 0 => {
            let fill = octets(c, "insert_init_hex").unwrap();
            config
                .with_insert_zone(usize::try_from(len).unwrap(), fill[0])
                .unwrap()
        }
        _ => config,
    }
}

impl Proto for Tm {
    fn config(c: &Map<String, Value>) -> ChannelMuxConfig<Tm, V> {
        let mut channel = TmChannel::new(
            SpacecraftId::<10>::new(u16::try_from(num(&c["scid"])).unwrap()).unwrap(),
            VcId::<3>::new(small(&c["oid_vc"])).unwrap(),
        );
        channel.mc_ocf = ocf(c, "mc_ocf_init_hex");
        channel.mc_fsh = octets(c, "mc_fsh_init_hex").map(|d| Fsh::new(&d).unwrap());
        let vcs: Vec<_> = c["vcs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let o = v.as_object().unwrap();
                let mut params = TmVc::new(match o["role"].as_str().unwrap() {
                    "packets" => TmRole::Packets,
                    "vca" => TmRole::Vca,
                    other => panic!("unknown role {other}"),
                });
                params.ocf = ocf(o, "ocf_init_hex");
                params.fsh = octets(o, "fsh_init_hex").map(|d| Fsh::new(&d).unwrap());
                vc::<Tm>(v, params)
            })
            .collect();
        finish(c, channel, &vcs)
    }
}

impl Proto for Aos {
    fn config(c: &Map<String, Value>) -> ChannelMuxConfig<Aos, V> {
        let channel = AosChannel::new(
            SpacecraftId::<10>::new(u16::try_from(num(&c["scid"])).unwrap()).unwrap(),
            AosEdition::Issue5,
            c["fhec"].as_bool().unwrap(),
            c["cycle_in_use"].as_bool().unwrap(),
        )
        .unwrap();
        let vcs: Vec<_> = c["vcs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let params = AosVc {
                    ocf: ocf(v.as_object().unwrap(), "ocf_init_hex"),
                };
                vc::<Aos>(v, params)
            })
            .collect();
        finish(c, channel, &vcs)
    }
}

fn vc_entry(c: &Map<String, Value>, vcid: u8) -> &Map<String, Value> {
    c["vcs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_object().unwrap())
        .find(|v| small(&v["vcid"]) == vcid)
        .unwrap()
}

fn qos(name: &str) -> Qos {
    match name {
        "sequence_controlled" => Qos::SequenceControlled,
        "expedited" => Qos::Expedited,
        other => panic!("unknown qos {other}"),
    }
}

impl Proto for Uslp {
    fn config(c: &Map<String, Value>) -> ChannelMuxConfig<Uslp, V> {
        let channel = UslpChannel {
            scid: SpacecraftId::<16>::new(u16::try_from(num(&c["scid"])).unwrap()).unwrap(),
            source_or_destination: SourceOrDestination::from_bit(
                num(&c["source_or_destination"]) == 1,
            ),
            oid_vcf_len: VcfCountLen::new(small(&c["oid"]["vcf_len"])).unwrap(),
            mc_ocf: ocf(c, "mc_ocf_init_hex"),
        };
        let vcs: Vec<_> = c["vcs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let o = v.as_object().unwrap();
                let params = UslpVc::new(
                    MapId::<4>::new(small(&o["map_id"])).unwrap(),
                    qos(o["qos"].as_str().unwrap()),
                    VcfCountLen::new(small(&o["vcf_len_sc"])).unwrap(),
                    VcfCountLen::new(small(&o["vcf_len_exp"])).unwrap(),
                    o.get("cop").and_then(Value::as_bool).unwrap_or(false),
                )
                .unwrap();
                vc::<Uslp>(v, params)
            })
            .collect();
        finish(c, channel, &vcs)
    }

    fn route(c: &Map<String, Value>, vcid: u8) -> Option<Route> {
        let o = vc_entry(c, vcid);
        Some(Route {
            map: MapId::<4>::new(small(&o["map_id"])).unwrap(),
            qos: qos(o["qos"].as_str().unwrap()),
        })
    }
}

fn scope(v: &Value) -> Scope {
    match v["scope"].as_str().unwrap() {
        "mc" => Scope::Mc,
        "vc" => Scope::Vc(small(&v["vc"])),
        other => panic!("unknown scope {other}"),
    }
}

fn range(r: aldis_core::engine::OutRange) -> Value {
    json!([r.start(), r.end()])
}

fn output(o: &MuxOutput, area: &[u8]) -> Value {
    match o {
        MuxOutput::Transmit(t) => json!({"transmit": {
            "kind": match t.class { FrameClass::Data => "data", FrameClass::Oid => "oid" },
            "vc": t.vc,
            "frame_hex": hex::encode(t.frame.slice(area).unwrap()),
            "security_header": range(t.security_header),
            "data": range(t.data),
            "security_trailer": range(t.security_trailer),
        }}),
        MuxOutput::InsertSduRepeated => json!({"notify": {"kind": "InsertSduRepeated"}}),
        MuxOutput::Discarded { octets } => {
            json!({"notify": {"kind": "Discarded", "octets": octets}})
        }
    }
}

fn run_scenario<P: Proto>(scenario: &aldis_testkit::ResolvedScenario<'_>) -> Vec<String> {
    let mut engine = ChannelMux::<P, V, F>::new(P::config(scenario.config)).unwrap();
    let mut config = scenario.config.clone();
    let mut area = vec![0xEE_u8; F];
    for (i, step) in scenario.steps.iter().enumerate() {
        let (key, v) = step.input.iter().next().unwrap();
        let field;
        let data;
        let mut early = None;
        let input = match key.as_str() {
            "offer" => {
                field = hex::decode(v["field_hex"].as_str().unwrap()).unwrap();
                let vcid = small(&v["vc"]);
                let named = v.get("map").is_some() || v.get("qos").is_some();
                // TM and AOS have no default route: a `map` key is a route they refuse.
                let route = named.then(|| {
                    let d = P::route(&config, vcid);
                    Route {
                        map: v
                            .get("map")
                            .map_or_else(|| d.unwrap().map, |m| MapId::<4>::new(small(m)).unwrap()),
                        qos: v.get("qos").map_or_else(
                            || d.map_or(Qos::SequenceControlled, |d| d.qos),
                            |q| qos(q.as_str().unwrap()),
                        ),
                    }
                });
                // A combination `Cop::new` refuses (732.1 Table 4-1) is the step's error.
                let cop = v.get("cop").and_then(|c| {
                    Cop::new(
                        num(&c["vcf_count"]),
                        num(&c["bypass"]) == 1,
                        num(&c["protocol_control"]) == 1,
                    )
                    .map_err(|e| early = Some(e))
                    .ok()
                });
                MuxInput::Offer(Offer {
                    vc: vcid,
                    field: &field,
                    pointer: v.get("fhp").map(|p| u16::try_from(num(p)).unwrap()),
                    route,
                    cop,
                })
            }
            "opportunity" => MuxInput::Opportunity,
            "set_ocf" => MuxInput::SetOcf {
                scope: scope(v),
                ocf: hex::decode(v["ocf_hex"].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            },
            "set_fsh" => {
                data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                MuxInput::SetFsh {
                    scope: scope(v),
                    data: &data,
                }
            }
            "set_insert" => {
                data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                MuxInput::SetInsert(&data)
            }
            "reconfigure" => {
                config.clone_from(v["config"].as_object().unwrap());
                MuxInput::Reconfigure(P::config(&config))
            }
            other => panic!("unknown input {other}"),
        };
        area.fill(0xEE);
        let stepped = early.map_or_else(|| engine.step(input, &mut area), Err);
        let got: Vec<Value> = match stepped {
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
    let Ok(VectorFile::EngineScenario(parsed)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = parsed;
    assert_eq!(file.engine, "channel-mux");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    let mut run = 0;
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        run = usize::saturating_add(run, 1);
        failures.extend(match scenario.config["protocol"].as_str().unwrap() {
            "tm" => run_scenario::<Tm>(scenario),
            "aos" => run_scenario::<Aos>(scenario),
            "uslp" => run_scenario::<Uslp>(scenario),
            other => panic!("no engine for {other}"),
        });
    }
    (run, failures)
}

#[test]
fn the_channel_mux_scenarios_match_exactly() {
    // req: LNK-003, LNK-030, LNK-031, LNK-032, LNK-033, LNK-034, LNK-035, LNK-036, LNK-076, LNK-082, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 40, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
