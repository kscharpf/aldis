//! Runs the FOP-1 conformance scenarios of `test-vectors/cop1/scenarios/` against
//! `Fop1::step` (COP-001, COP-005, COP-010…COP-017, TV-010…TV-012, TV-023). Every step's
//! outputs, state, and deadline must match exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

#[path = "common/mod.rs"]
mod common;

use aldis_cop1::{
    Directive, Fop1Config, Fop1Event, Fop1Input, Fop1Output, Fop1State, Fop1Target, FrameKind,
    TimeoutType, fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{Clcw, ClcwStatus, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId};
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use core::num::NonZeroU8;
use serde_json::{Map, Value, json};

/// Four frames of at most 64 octets: room for every scenario (K is at most 3), and the same
/// instance as the unit tests, so that coverage counts one instantiation (VER-024).
use common::Rig;

const Q: usize = 4;
const F: usize = 64;
const N: usize = fop1_max_actions(Q);
type Engine = Rig<Q, F, N>;

const FILES: [&str; 5] = [
    "fop1_table.json",
    "fop1_ad_service.json",
    "fop1_window_timer.json",
    "fop1_wrap_directives.json",
    "fop1_clcw.json",
];

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn byte(v: &Value) -> u8 {
    u8::try_from(num(v)).unwrap()
}

fn config(c: &Map<String, Value>) -> Engine {
    assert_eq!(c["frame_target"], "tc");
    let target = Fop1Target::tc(
        TcSpacecraftId::new(u16::try_from(num(&c["scid"])).unwrap()).unwrap(),
        TcVcId::new(byte(&c["vcid"])).unwrap(),
        F,
        c["fecf"].as_bool().unwrap(),
    )
    .unwrap();
    Engine::new(Fop1Config {
        target,
        t1_initial: Duration::from_micros(num(&c["t1_initial_us"])),
        transmission_limit: NonZeroU8::new(byte(&c["transmission_limit"])).unwrap(),
        timeout_type: if num(&c["timeout_type"]) == 0 {
            TimeoutType::Tt0
        } else {
            TimeoutType::Tt1
        },
        sliding_window_k: byte(&c["k"]),
        ad_repetitions: 1,
        bc_repetitions: 1,
    })
    .unwrap()
}

fn handle(v: &Map<String, Value>) -> u32 {
    u32::try_from(num(&v["handle"])).unwrap()
}

fn kind(v: &Value) -> FrameKind {
    match v.as_str().unwrap() {
        "AD" => FrameKind::Ad,
        "BC" => FrameKind::Bc,
        "BD" => FrameKind::Bd,
        other => panic!("unknown kind {other}"),
    }
}

/// The directive of a `directive` input, or `None` for one the caller cannot parse (E40).
fn directive(v: &Map<String, Value>) -> Option<Directive> {
    let value = v.get("value").map(num);
    let fits = |max: u64| {
        value
            .filter(|x| *x <= max)
            .map(|x| u8::try_from(x).unwrap())
    };
    Some(match v["type"].as_str().unwrap() {
        "initiate_ad_without_clcw_check" => Directive::InitiateAdWithoutClcwCheck,
        "initiate_ad_with_clcw_check" => Directive::InitiateAdWithClcwCheck,
        "initiate_ad_with_unlock" => Directive::InitiateAdWithUnlock,
        "initiate_ad_with_set_vr" => Directive::InitiateAdWithSetVr {
            v_r: FrameSeqNum::new(fits(255)?),
        },
        "terminate_ad" => Directive::TerminateAd,
        "resume_ad" => Directive::ResumeAd,
        "set_v_s" => Directive::SetVs {
            v_s: FrameSeqNum::new(fits(255)?),
        },
        "set_k" => Directive::SetSlidingWindowWidth { width: fits(255)? },
        "set_t1_initial" => Directive::SetT1Initial {
            duration: Duration::from_micros(value?),
        },
        "set_transmission_limit" => Directive::SetTransmissionLimit {
            limit: NonZeroU8::new(fits(255)?)?,
        },
        "set_timeout_type" => Directive::SetTimeoutType {
            timeout_type: match value? {
                0 => TimeoutType::Tt0,
                1 => TimeoutType::Tt1,
                _ => return None,
            },
        },
        _ => return None,
    })
}

fn clcw(v: &Map<String, Value>, vcid: u8) -> Fop1Input<'static> {
    let field = |k: &str, default: u64| v.get(k).map_or(default, num);
    if field("control_word_type", 0) != 0 {
        return Fop1Input::InvalidClcw;
    }
    Fop1Input::Clcw(Clcw {
        status: ClcwStatus::new(0).unwrap(),
        cop_in_effect: CopInEffect::new(u8::try_from(field("cop_in_effect", 1)).unwrap()).unwrap(),
        vc_id: TcVcId::new(u8::try_from(field("vcid", u64::from(vcid))).unwrap()).unwrap(),
        no_rf_available: false,
        no_bit_lock: false,
        lockout: v["lockout"].as_bool().unwrap(),
        wait: v["wait"].as_bool().unwrap(),
        retransmit: v["retransmit"].as_bool().unwrap(),
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(byte(&v["n_r"])),
    })
}

/// The input of `input` (one key), with the octets it borrows in `data`.
fn input<'a>(input: &Map<String, Value>, vcid: u8, data: &'a mut Vec<u8>) -> Fop1Input<'a> {
    let (key, v) = input.iter().next().unwrap();
    let empty = Map::new();
    let v = v.as_object().unwrap_or(&empty);
    match key.as_str() {
        "clcw" => clcw(v, vcid),
        "timeout" => Fop1Input::Timeout,
        "request_ad" | "request_bd" => {
            *data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
            if key == "request_ad" {
                Fop1Input::RequestAd {
                    handle: handle(v),
                    fdu: data,
                }
            } else {
                Fop1Input::RequestBd {
                    handle: handle(v),
                    fdu: data,
                }
            }
        }
        "directive" => match directive(v) {
            Some(directive) => Fop1Input::Directive {
                handle: handle(v),
                directive,
            },
            None => Fop1Input::InvalidDirective { handle: handle(v) },
        },
        "transmit_done" => Fop1Input::TransmitDone {
            kind: kind(&v["kind"]),
            accepted: v["response"] == "accept",
        },
        other => panic!("unknown input {other}"),
    }
}

/// Table 5-1's events in table order: E1 to E46, then E101 to E104.
const EVENTS: [Fop1Event; 50] = {
    use Fop1Event as E;
    [
        E::E1,
        E::E2,
        E::E3,
        E::E4,
        E::E5,
        E::E6,
        E::E7,
        E::E8,
        E::E9,
        E::E10,
        E::E11,
        E::E12,
        E::E13,
        E::E14,
        E::E15,
        E::E16,
        E::E17,
        E::E18,
        E::E19,
        E::E20,
        E::E21,
        E::E22,
        E::E23,
        E::E24,
        E::E25,
        E::E26,
        E::E27,
        E::E28,
        E::E29,
        E::E30,
        E::E31,
        E::E32,
        E::E33,
        E::E34,
        E::E35,
        E::E36,
        E::E37,
        E::E38,
        E::E39,
        E::E40,
        E::E41,
        E::E42,
        E::E43,
        E::E44,
        E::E45,
        E::E46,
        E::E101,
        E::E102,
        E::E103,
        E::E104,
    ]
};

/// The event numbered `n` in Table 5-1.
fn event(n: u64) -> Fop1Event {
    *EVENTS
        .iter()
        .zip((1..=46).chain(101..=104))
        .find(|(_, number)| *number == n)
        .unwrap()
        .0
}

/// The number of `e` in Table 5-1.
fn event_number(e: Fop1Event) -> u64 {
    EVENTS
        .iter()
        .zip((1..=46).chain(101..=104))
        .find(|(x, _)| **x == e)
        .unwrap()
        .1
}

const fn state_name(s: Fop1State) -> &'static str {
    match s {
        Fop1State::Active => "S1",
        Fop1State::RetransmitWithoutWait => "S2",
        Fop1State::RetransmitWithWait => "S3",
        Fop1State::InitializingWithoutBcFrame => "S4",
        Fop1State::InitializingWithBcFrame => "S5",
        Fop1State::Initial => "S6",
    }
}

fn snake<T: core::fmt::Debug>(v: T) -> String {
    let name = format!("{v:?}");
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn output(o: &Fop1Output, area: &[u8]) -> Value {
    match o {
        Fop1Output::Transmit {
            kind,
            sequence_number,
            retransmission,
            frame,
            ..
        } => json!({"transmit": {
            "kind": format!("{kind:?}").to_uppercase(),
            "ns": sequence_number.get(),
            "retransmission": retransmission,
            "frame_hex": hex::encode(frame.slice(area).unwrap()),
        }}),
        Fop1Output::Abort => json!({"event": "abort"}),
        Fop1Output::Suspend => json!({"event": "suspend"}),
        Fop1Output::Transfer(n) => {
            json!({"event": "transfer_notify", "handle": n.handle, "type": snake(n.status)})
        }
        Fop1Output::Directive(n) => {
            json!({"event": "directive_notify", "handle": n.handle, "type": snake(n.status)})
        }
        Fop1Output::Alert(a) => json!({
            "event": "alert",
            "reason": match snake(a.reason).as_str() { "nn_r" => "nn_r".to_owned(), s => s.to_owned() },
            "event_number": event_number(a.event),
            "prior_state": state_name(a.prior_state),
        }),
    }
}

fn run_file(name: &str) -> (usize, Vec<String>) {
    let path = aldis_testkit::vectors_dir()
        .join("cop1/scenarios")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = file;
    assert_eq!(file.engine, "fop1");
    let mut failures = Vec::new();
    let resolved = file.resolve().unwrap();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        let vcid = byte(&scenario.config["vcid"]);
        let mut engine = config(scenario.config);
        let mut area = vec![0u8; F];
        for (i, step) in scenario.steps.iter().enumerate() {
            let now = Instant::from_micros(step.at_us);
            let mut data = Vec::new();
            let result = if let Some(t) = step.input.get("table_event") {
                let t = t.as_object().unwrap();
                let e = event(num(&t["event"]));
                let inner = t.get("input").and_then(Value::as_object);
                let given = inner.map_or(Fop1Input::Timeout, |m| input(m, vcid, &mut data));
                engine
                    .fop
                    .decide_event(&engine.store, e, given, now, &mut area)
            } else {
                let given = input(&step.input, vcid, &mut data);
                engine.decide(given, now, &mut area)
            };
            let got: Vec<Value> = match result {
                Ok(decision) => {
                    let actions = engine.commit(decision);
                    actions.iter().map(|o| output(o, &area)).collect()
                }
                Err(e) => vec![json!({"error": e.kind().name()})],
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
            if let Some(s) = &step.expect_state
                && state_name(engine.state()) != s
            {
                problems.push(format!("state {} want {s}", state_name(engine.state())));
            }
            let deadline = engine.next_deadline().map(Instant::as_micros);
            let want_deadline = step.expect_timeout_us;
            if (want_deadline.is_some() || step.expect_no_timeout) && deadline != want_deadline {
                problems.push(format!("deadline {deadline:?} want {want_deadline:?}"));
            }
            if !problems.is_empty() {
                failures.push(format!(
                    "{} step {i} {}: {}",
                    scenario.scenario.id,
                    Value::Object(step.input.clone()),
                    problems.join("; ")
                ));
                break;
            }
        }
    }
    (resolved.len(), failures)
}

#[test]
fn the_action_bound_target_and_classification_at_run_time() {
    // req: COP-006, COP-010, API-058
    assert_eq!(fop1_max_actions(core::hint::black_box(Q)), N);
    let plain = Fop1Target::tc(
        TcSpacecraftId::new(1).unwrap(),
        TcVcId::new(0).unwrap(),
        F,
        core::hint::black_box(false),
    )
    .unwrap();
    assert_eq!(plain.max_fdu_len(), F - 5);
    assert!(matches!(
        plain.channel(),
        aldis_cop1::Fop1Channel::Tc { .. }
    ));
    let f = Engine::new(Fop1Config {
        target: plain,
        t1_initial: Duration::from_micros(10),
        transmission_limit: NonZeroU8::MIN,
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: 1,
        ad_repetitions: 0,
        bc_repetitions: 0,
    })
    .unwrap();
    let mut out = [0; F];
    let at = Instant::from_micros(0);
    // No timer: a Timeout is no event.
    let mut g = f.clone();
    let decision = g.decide(Fop1Input::Timeout, at, &mut out).unwrap();
    let actions = g.commit(decision);
    assert_eq!((g, actions.len()), (f.clone(), 0));
    // An FDU longer than the frame allows is refused before any cell runs.
    let long = [0; F];
    for input in [
        Fop1Input::RequestAd {
            handle: 1,
            fdu: &long,
        },
        Fop1Input::RequestBd {
            handle: 1,
            fdu: &long,
        },
    ] {
        assert!(f.decide(input, at, &mut out).is_err());
    }
}

#[test]
fn fop1_scenarios() {
    // req: COP-001, COP-004, COP-005, COP-006, COP-010, COP-011, COP-012, COP-013, COP-014, COP-015, COP-017, TV-010, TV-011, TV-012
    let mut all = Vec::new();
    let mut total = 0;
    for name in FILES {
        let (count, failures) = run_file(name);
        total += count;
        all.extend(failures.into_iter().map(|f| format!("{name}: {f}")));
    }
    assert!(
        all.is_empty(),
        "{} of {total} scenarios failed:\n{}",
        all.len(),
        all.join("\n")
    );
}
