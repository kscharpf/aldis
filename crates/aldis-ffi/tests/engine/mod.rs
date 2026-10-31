//! Runs the engine scenarios of `test-vectors/cop1/scenarios/` through the C ABI of the FOP-1 and
//! FARM-1 handles (M3 WP-6; COP-001, COP-002, COP-005, COP-006, CABI-030…032, CABI-040, TV-010…TV-012).
//!
//! Every step builds the flat input struct from the scenario's input, calls `aldis_fop1_step` or
//! `aldis_farm1_step` on the storage of the scenario, and compares the complete, ordered actions,
//! the state, and the deadline with the expected values, exactly as the Rust runner of
//! `aldis-cop1` does against the Rust API. The scenarios whose trigger is a `table_event` feed an
//! event number to the table without classification, a conformance hook of the Rust engine that
//! the C ABI does not expose (CABI-040): they are counted and skipped, and the Rust runner
//! covers them.

use aldis::core::ErrorKind;
use aldis_ffi::farm1::*;
use aldis_ffi::fop1::*;
use aldis_ffi::{ALDIS_OK, aldis_error_detail_t};
use aldis_testkit::{ScenarioFile, VectorFile, hex, vectors_dir};
use serde_json::{Map, Value, json};

/// A frame buffer of the scenarios: the largest FOP-1 frame is `FRAME` octets.
const FRAME: usize = 64;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn byte(v: &Value) -> u8 {
    u8::try_from(num(v)).unwrap()
}

/// The name of the error with the code `status`.
fn error_name(status: u32) -> String {
    ErrorKind::from_code(u16::try_from(status).unwrap())
        .unwrap()
        .name()
        .to_owned()
}

/// Checks that a failed call reports the same code in the status and in the detail (CABI-020,
/// CABI-021).
fn checked(status: u32, detail: &aldis_error_detail_t) -> Option<String> {
    if status == ALDIS_OK {
        return None;
    }
    assert_eq!(status, detail.kind, "status and detail disagree");
    Some(error_name(status))
}

fn snake_notice(status: u32) -> &'static str {
    match status {
        ALDIS_FOP1_NOTICE_ACCEPT => "accept",
        ALDIS_FOP1_NOTICE_REJECT => "reject",
        ALDIS_FOP1_NOTICE_POSITIVE_CONFIRM => "positive_confirm",
        ALDIS_FOP1_NOTICE_NEGATIVE_CONFIRM => "negative_confirm",
        other => panic!("notice {other}"),
    }
}

fn frame_name(kind: u32) -> &'static str {
    match kind {
        ALDIS_FOP1_FRAME_AD => "AD",
        ALDIS_FOP1_FRAME_BC => "BC",
        ALDIS_FOP1_FRAME_BD => "BD",
        other => panic!("frame kind {other}"),
    }
}

fn alert_name(reason: u32) -> &'static str {
    match reason {
        ALDIS_FOP1_ALERT_LIMIT => "limit",
        ALDIS_FOP1_ALERT_T1 => "t1",
        ALDIS_FOP1_ALERT_LOCKOUT => "lockout",
        ALDIS_FOP1_ALERT_SYNCH => "synch",
        ALDIS_FOP1_ALERT_NN_R => "nn_r",
        ALDIS_FOP1_ALERT_CLCW => "clcw",
        ALDIS_FOP1_ALERT_LLIF => "llif",
        ALDIS_FOP1_ALERT_TERM => "term",
        other => panic!("alert reason {other}"),
    }
}

/// The scenario file `name` (relative to `test-vectors/`) for `engine`.
fn load(name: &str, engine: &str) -> ScenarioFile {
    let path = vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    assert_eq!(file.engine, engine);
    file
}

/// Compares one step with the C results and returns the problems.
fn problems(
    got: &[Value],
    want: &[Map<String, Value>],
    state: &str,
    want_state: Option<&String>,
) -> Vec<String> {
    let want: Vec<Value> = want.iter().cloned().map(Value::Object).collect();
    let mut problems = Vec::new();
    if got != want {
        problems.push(format!(
            "outputs\n      got  {}\n      want {}",
            Value::Array(got.to_vec()),
            Value::Array(want)
        ));
    }
    if let Some(s) = want_state
        && state != s
    {
        problems.push(format!("state {state} want {s}"));
    }
    problems
}

/// FOP-1: storage, step, and queries through `aldis_fop1_*`.
mod fop1_runner {
    use super::*;

    /// A FOP-1 in caller storage, released when dropped.
    struct Engine {
        storage: Box<aldis_fop1_t>,
    }

    impl Engine {
        fn new(c: &Map<String, Value>) -> Engine {
            assert_eq!(c["frame_target"], "tc");
            let cfg = aldis_fop1_config_t {
                scid: u16::try_from(num(&c["scid"])).unwrap(),
                vc_id: byte(&c["vcid"]),
                fecf: u8::from(c["fecf"].as_bool().unwrap()),
                max_frame_len: FRAME,
                t1_initial_us: num(&c["t1_initial_us"]),
                transmission_limit: byte(&c["transmission_limit"]),
                timeout_type: u32::try_from(num(&c["timeout_type"])).unwrap(),
                sliding_window_k: byte(&c["k"]),
                ad_repetitions: 1,
                bc_repetitions: 1,
            };
            let mut storage = Box::new(aldis_fop1_t {
                words: [0; ALDIS_FOP1_WORDS],
            });
            // SAFETY: `storage` is a live `aldis_fop1_t`, `cfg` a live config, and the detail is
            // NULL.
            let status = unsafe {
                aldis_fop1_init(&raw mut *storage, &raw const cfg, core::ptr::null_mut())
            };
            assert_eq!(status, ALDIS_OK);
            Engine { storage }
        }

        fn step(&mut self, input: &aldis_fop1_input_t, now: u64, out: &mut [u8]) -> Step {
            let mut actions = aldis_fop1_actions_t::default();
            let mut detail = aldis_error_detail_t::default();
            // SAFETY: every pointer is to a live object of the type, `out` is valid for its
            // length, and the input's `data` is valid for `data_len` octets.
            let status = unsafe {
                aldis_fop1_step(
                    &raw mut *self.storage,
                    input,
                    now,
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut actions,
                    &raw mut detail,
                )
            };
            match checked(status, &detail) {
                Some(kind) => Step::Err(kind),
                None => Step::Ok(actions),
            }
        }

        fn state(&mut self) -> String {
            let mut s = aldis_fop1_status_t::default();
            // SAFETY: as above.
            let status = unsafe {
                aldis_fop1_status(&raw mut *self.storage, &raw mut s, core::ptr::null_mut())
            };
            assert_eq!(status, ALDIS_OK);
            format!("S{}", s.state)
        }

        fn deadline(&mut self) -> Option<u64> {
            let (mut armed, mut at) = (9_u8, 9_u64);
            // SAFETY: as above.
            let status = unsafe {
                aldis_fop1_next_deadline(
                    &raw mut *self.storage,
                    &raw mut armed,
                    &raw mut at,
                    core::ptr::null_mut(),
                )
            };
            assert_eq!(status, ALDIS_OK);
            assert!(armed <= 1 && (armed == 1 || at == 0));
            (armed == 1).then_some(at)
        }
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            // SAFETY: the storage is live; releasing it is what `deinit` is for.
            unsafe { aldis_fop1_deinit(&raw mut *self.storage) };
        }
    }

    enum Step {
        Ok(aldis_fop1_actions_t),
        Err(String),
    }

    fn blank() -> aldis_fop1_input_t {
        aldis_fop1_input_t {
            kind: ALDIS_FOP1_INPUT_TIMEOUT,
            handle: 0,
            clcw: 0,
            directive: 0,
            value: 0,
            frame_kind: 0,
            accepted: 0,
            data: core::ptr::null(),
            data_len: 0,
        }
    }

    /// The C input of `input` (one key), with the octets it borrows in `data`; `None` for the
    /// conformance hook `table_event`.
    fn input(
        input: &Map<String, Value>,
        vcid: u8,
        data: &mut Vec<u8>,
    ) -> Option<aldis_fop1_input_t> {
        let (key, v) = input.iter().next().unwrap();
        let empty = Map::new();
        let v = v.as_object().unwrap_or(&empty);
        let handle = || u32::try_from(num(&v["handle"])).unwrap();
        Some(match key.as_str() {
            "clcw" => {
                let field = |k: &str, default: u64| v.get(k).map_or(default, num);
                // Octets: status/cop (byte 0), VCID<<2 (byte 1), flags (byte 2), N(R) (byte 3).
                let b0 = byte(&json!(field("cop_in_effect", 1)));
                let b1 = byte(&json!(field("vcid", u64::from(vcid)))) << 2;
                let b2 = (u8::from(v["lockout"].as_bool().unwrap()) << 5)
                    | (u8::from(v["wait"].as_bool().unwrap()) << 4)
                    | (u8::from(v["retransmit"].as_bool().unwrap()) << 3);
                let b3 = byte(&v["n_r"]);
                aldis_fop1_input_t {
                    kind: ALDIS_FOP1_INPUT_CLCW,
                    clcw: u32::from_be_bytes([
                        b0 | (u8::try_from(field("control_word_type", 0)).unwrap() << 7),
                        b1,
                        b2,
                        b3,
                    ]),
                    ..blank()
                }
            }
            "timeout" => blank(),
            "request_ad" | "request_bd" => {
                *data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                aldis_fop1_input_t {
                    kind: if key == "request_ad" {
                        ALDIS_FOP1_INPUT_REQUEST_AD
                    } else {
                        ALDIS_FOP1_INPUT_REQUEST_BD
                    },
                    handle: handle(),
                    data: data.as_ptr(),
                    data_len: data.len(),
                    ..blank()
                }
            }
            "directive" => directive(v),
            "transmit_done" => aldis_fop1_input_t {
                kind: ALDIS_FOP1_INPUT_TRANSMIT_DONE,
                frame_kind: match v["kind"].as_str().unwrap() {
                    "AD" => ALDIS_FOP1_FRAME_AD,
                    "BC" => ALDIS_FOP1_FRAME_BC,
                    "BD" => ALDIS_FOP1_FRAME_BD,
                    other => panic!("unknown kind {other}"),
                },
                accepted: u8::from(v["response"] == "accept"),
                ..blank()
            },
            "table_event" => return None,
            other => panic!("unknown input {other}"),
        })
    }

    /// The `directive` input; one the caller cannot parse (an unknown type, or no value for a
    /// directive that has one) is `INVALID_DIRECTIVE`, which is E40.
    fn directive(v: &Map<String, Value>) -> aldis_fop1_input_t {
        let handle = u32::try_from(num(&v["handle"])).unwrap();
        let (kind, valued) = match v["type"].as_str().unwrap() {
            "initiate_ad_without_clcw_check" => {
                (ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK, false)
            }
            "initiate_ad_with_clcw_check" => {
                (ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_CLCW_CHECK, false)
            }
            "initiate_ad_with_unlock" => (ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_UNLOCK, false),
            "initiate_ad_with_set_vr" => (ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_SET_VR, true),
            "terminate_ad" => (ALDIS_FOP1_DIRECTIVE_TERMINATE_AD, false),
            "resume_ad" => (ALDIS_FOP1_DIRECTIVE_RESUME_AD, false),
            "set_v_s" => (ALDIS_FOP1_DIRECTIVE_SET_VS, true),
            "set_k" => (ALDIS_FOP1_DIRECTIVE_SET_K, true),
            "set_t1_initial" => (ALDIS_FOP1_DIRECTIVE_SET_T1_INITIAL, true),
            "set_transmission_limit" => (ALDIS_FOP1_DIRECTIVE_SET_TRANSMISSION_LIMIT, true),
            "set_timeout_type" => (ALDIS_FOP1_DIRECTIVE_SET_TIMEOUT_TYPE, true),
            _ => {
                return aldis_fop1_input_t {
                    kind: ALDIS_FOP1_INPUT_INVALID_DIRECTIVE,
                    handle,
                    ..blank()
                };
            }
        };
        if valued && !v.contains_key("value") {
            return aldis_fop1_input_t {
                kind: ALDIS_FOP1_INPUT_INVALID_DIRECTIVE,
                handle,
                ..blank()
            };
        }
        aldis_fop1_input_t {
            kind: ALDIS_FOP1_INPUT_DIRECTIVE,
            handle,
            directive: kind,
            value: v.get("value").map_or(0, num),
            ..blank()
        }
    }

    fn output(a: &aldis_fop1_action_t, area: &[u8]) -> Value {
        match a.kind {
            ALDIS_FOP1_ACTION_TRANSMIT => json!({"transmit": {
                "kind": frame_name(a.frame_kind),
                "ns": a.sequence_number,
                "retransmission": a.retransmission == 1,
                "frame_hex": hex::encode(&area[a.frame.offset..a.frame.offset + a.frame.len]),
            }}),
            ALDIS_FOP1_ACTION_ABORT => json!({"event": "abort"}),
            ALDIS_FOP1_ACTION_SUSPEND => json!({"event": "suspend"}),
            ALDIS_FOP1_ACTION_TRANSFER_NOTIFY => {
                json!({"event": "transfer_notify", "handle": a.handle, "type": snake_notice(a.status)})
            }
            ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY => {
                json!({"event": "directive_notify", "handle": a.handle, "type": snake_notice(a.status)})
            }
            ALDIS_FOP1_ACTION_ALERT => json!({
                "event": "alert",
                "reason": alert_name(a.reason),
                "event_number": a.event,
                "prior_state": format!("S{}", a.state),
            }),
            other => panic!("action kind {other}"),
        }
    }

    /// Runs every scenario of `name`; returns the number run, the number skipped, and the
    /// failures.
    pub(super) fn run_file(name: &str) -> (usize, usize, Vec<String>) {
        let file = load(name, "fop1");
        let (mut run, mut skipped, mut failures) = (0, 0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            let vcid = byte(&scenario.config["vcid"]);
            let mut data = Vec::new();
            let hooks = scenario
                .steps
                .iter()
                .any(|s| input(&s.input, vcid, &mut data).is_none());
            if hooks {
                skipped += 1;
                continue;
            }
            run += 1;
            let mut engine = Engine::new(scenario.config);
            for (i, step) in scenario.steps.iter().enumerate() {
                let mut data = Vec::new();
                let given = input(&step.input, vcid, &mut data).unwrap();
                let mut area = vec![0_u8; FRAME];
                let got: Vec<Value> = match engine.step(&given, step.at_us, &mut area) {
                    Step::Ok(actions) => actions.items[..actions.count]
                        .iter()
                        .map(|a| output(a, &area))
                        .collect(),
                    Step::Err(kind) => vec![json!({"error": kind})],
                };
                let mut found = problems(
                    &got,
                    &step.expect,
                    &engine.state(),
                    step.expect_state.as_ref(),
                );
                let deadline = engine.deadline();
                if (step.expect_timeout_us.is_some() || step.expect_no_timeout)
                    && deadline != step.expect_timeout_us
                {
                    found.push(format!(
                        "deadline {deadline:?} want {:?}",
                        step.expect_timeout_us
                    ));
                }
                if !found.is_empty() {
                    failures.push(format!(
                        "{} step {i} {}: {}",
                        scenario.scenario.id,
                        Value::Object(step.input.clone()),
                        found.join("; ")
                    ));
                    break;
                }
            }
        }
        (run, skipped, failures)
    }
}

/// FARM-1: storage, step, and queries through `aldis_farm1_*`.
mod farm1_runner {
    use super::*;

    struct Engine {
        storage: Box<aldis_farm1_t>,
    }

    impl Engine {
        fn new(c: &Map<String, Value>) -> Engine {
            assert_eq!(c["frame_target"], "tc");
            let w = c["window"].as_object().unwrap();
            let allowed = w["retransmission_allowed"].as_bool().unwrap();
            let cfg = aldis_farm1_config_t {
                vc_id: byte(&c["vcid"]),
                status: byte(&c["status_field"]),
                retransmission_allowed: u8::from(allowed),
                window: u16::try_from(num(&w["w"])).unwrap(),
                positive_window: if allowed {
                    0
                } else {
                    u16::try_from(num(&w["pw"])).unwrap()
                },
                v_r: byte(&c["v_r"]),
            };
            let mut storage = Box::new(aldis_farm1_t {
                words: [0; ALDIS_FARM1_WORDS],
            });
            // SAFETY: `storage` is a live `aldis_farm1_t`, `cfg` a live config, and the detail is
            // NULL.
            let status = unsafe {
                aldis_farm1_init(&raw mut *storage, &raw const cfg, core::ptr::null_mut())
            };
            assert_eq!(status, ALDIS_OK);
            Engine { storage }
        }

        fn step(
            &mut self,
            input: &aldis_farm1_input_t,
            now: u64,
            out: &mut [u8],
        ) -> Result<aldis_farm1_actions_t, String> {
            let mut actions = aldis_farm1_actions_t::default();
            let mut detail = aldis_error_detail_t::default();
            // SAFETY: every pointer is to a live object of the type, `out` is valid for its
            // length, and the input's `data` is valid for `data_len` octets.
            let status = unsafe {
                aldis_farm1_step(
                    &raw mut *self.storage,
                    input,
                    now,
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut actions,
                    &raw mut detail,
                )
            };
            checked(status, &detail).map_or(Ok(actions), Err)
        }

        fn status(&mut self) -> aldis_farm1_status_t {
            let mut s = aldis_farm1_status_t::default();
            // SAFETY: as above.
            let status = unsafe {
                aldis_farm1_status(&raw mut *self.storage, &raw mut s, core::ptr::null_mut())
            };
            assert_eq!(status, ALDIS_OK);
            s
        }
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            // SAFETY: the storage is live; releasing it is what `deinit` is for.
            unsafe { aldis_farm1_deinit(&raw mut *self.storage) };
        }
    }

    fn blank() -> aldis_farm1_input_t {
        aldis_farm1_input_t {
            kind: ALDIS_FARM1_INPUT_BUFFER_RELEASED,
            sequence_number: 0,
            v_star: 0,
            buffer_available: 0,
            no_rf_available: 0,
            no_bit_lock: 0,
            data: core::ptr::null(),
            data_len: 0,
        }
    }

    /// The C input of a TC Transfer Frame that passed the Frame Validation Checks, read from its
    /// octets (232.0 §4.1.2, §4.1.3.3, §4.1.4): the bypass and control command flags give the
    /// type, and the data field follows the five-octet header, before the FECF if there is one.
    fn frame(
        config: &Map<String, Value>,
        octets: &[u8],
        buffer_available: bool,
    ) -> (aldis_farm1_input_t, Vec<u8>) {
        let trailer = if config["fecf"].as_bool().unwrap() {
            2
        } else {
            0
        };
        let data = octets[5..octets.len() - trailer].to_vec();
        let (bypass, control) = (octets[0] & 0x20 != 0, octets[0] & 0x10 != 0);
        let mut input = aldis_farm1_input_t {
            buffer_available: u8::from(buffer_available),
            ..blank()
        };
        match (bypass, control) {
            (false, _) => {
                input.kind = ALDIS_FARM1_INPUT_FRAME_AD;
                input.sequence_number = octets[4];
            }
            (true, false) => input.kind = ALDIS_FARM1_INPUT_FRAME_BD,
            (true, true) => match data.as_slice() {
                [0x00] => input.kind = ALDIS_FARM1_INPUT_FRAME_BC_UNLOCK,
                [0x82, 0x00, v_star] => {
                    input.kind = ALDIS_FARM1_INPUT_FRAME_BC_SET_VR;
                    input.v_star = *v_star;
                }
                _ => input.kind = ALDIS_FARM1_INPUT_FRAME_BC_OTHER,
            },
        }
        (input, data)
    }

    fn state_name(n: u8) -> String {
        format!("S{n}")
    }

    fn output(a: &aldis_farm1_action_t, area: &[u8]) -> Value {
        match a.kind {
            ALDIS_FARM1_ACTION_DELIVER => json!({"deliver": {
                "kind": match a.fdu_kind {
                    ALDIS_FARM1_FDU_AD => "AD",
                    ALDIS_FARM1_FDU_BD => "BD",
                    other => panic!("fdu kind {other}"),
                },
                "ns": a.sequence_number,
                "data_hex": hex::encode(&area[a.data.offset..a.data.offset + a.data.len]),
            }}),
            ALDIS_FARM1_ACTION_FRAME_DISCARDED => json!({
                "event": "frame_discarded",
                "event_number": a.event,
                "state": state_name(a.state),
            }),
            ALDIS_FARM1_ACTION_STATE_CHANGED => json!({
                "event": "state_changed",
                "event_number": a.event,
                "from": state_name(a.state),
                "to": state_name(a.to_state),
            }),
            ALDIS_FARM1_ACTION_UNLOCKED => json!({"event": "unlocked"}),
            other => panic!("action kind {other}"),
        }
    }

    /// The `query` output: the CLCW of the snapshot, as the scenario writes it.
    fn clcw_output(s: &aldis_farm1_status_t) -> Value {
        let [b0, b1, b2, b3] = s.clcw.to_be_bytes();
        json!({
            "clcw": {
                "lockout": b2 & 0x20 != 0,
                "wait": b2 & 0x10 != 0,
                "retransmit": b2 & 0x08 != 0,
                "farm_b_counter": (b2 >> 1) & 3,
                "n_r": b3,
                "cop_in_effect": b0 & 3,
                "vcid": b1 >> 2,
                "status_field": (b0 >> 2) & 7,
                "no_rf_available": b2 & 0x80 != 0,
                "no_bit_lock": b2 & 0x40 != 0,
            },
            "clcw_hex": hex::encode(&[b0, b1, b2, b3]),
        })
    }

    pub(super) fn run_file(name: &str) -> (usize, usize, Vec<String>) {
        let file = load(name, "farm1");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            run += 1;
            let mut engine = Engine::new(scenario.config);
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let empty = Map::new();
                let v = v.as_object().unwrap_or(&empty);
                let mut area = vec![0_u8; 2048];
                // The octets of the frame: kept alive for the call.
                let mut keep = Vec::new();
                let got: Vec<Value> = if key == "query" {
                    let status = engine.status();
                    assert_eq!(status.lockout, u8::from(status.state == 3));
                    assert_eq!(status.wait, u8::from(status.state == 2));
                    vec![clcw_output(&status)]
                } else {
                    let input = match key.as_str() {
                        "frame" => {
                            let octets = hex::decode(v["frame_hex"].as_str().unwrap()).unwrap();
                            let (input, data) = frame(
                                scenario.config,
                                &octets,
                                v["buffer_available"].as_bool().unwrap(),
                            );
                            keep = data;
                            aldis_farm1_input_t {
                                data: keep.as_ptr(),
                                data_len: keep.len(),
                                ..input
                            }
                        }
                        "invalid_frame" => aldis_farm1_input_t {
                            kind: ALDIS_FARM1_INPUT_INVALID_FRAME,
                            ..blank()
                        },
                        "buffer_released" => blank(),
                        "set_physical_flags" => aldis_farm1_input_t {
                            kind: ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS,
                            no_rf_available: u8::from(v["no_rf_available"].as_bool().unwrap()),
                            no_bit_lock: u8::from(v["no_bit_lock"].as_bool().unwrap()),
                            ..blank()
                        },
                        other => panic!("unknown input {other}"),
                    };
                    match engine.step(&input, step.at_us, &mut area) {
                        Ok(actions) => actions.items[..actions.count]
                            .iter()
                            .map(|a| output(a, &area))
                            .collect(),
                        Err(kind) => vec![json!({"error": kind})],
                    }
                };
                drop(keep);
                let state = state_name(engine.status().state);
                let found = problems(&got, &step.expect, &state, step.expect_state.as_ref());
                if !found.is_empty() {
                    failures.push(format!(
                        "{} step {i} {}: {}",
                        scenario.scenario.id,
                        Value::Object(step.input.clone()),
                        found.join("; ")
                    ));
                    break;
                }
            }
        }
        (run, 0, failures)
    }
}

/// Runs every scenario of the FOP-1 or FARM-1 file `name` (relative to `test-vectors/`), and fails with all the failures.
pub(crate) fn run_scenario_file(name: &str) {
    let engine = if name.contains("fop1") {
        "fop1"
    } else {
        "farm1"
    };
    let (run, skipped, failures) = if engine == "fop1" {
        fop1_runner::run_file(name)
    } else {
        farm1_runner::run_file(name)
    };
    assert!(
        failures.is_empty(),
        "{} of {run} scenarios failed ({skipped} skipped):\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(run > 0, "{name}: no scenario ran");
}
