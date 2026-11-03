//! Runs the scenarios of the generation engines in `test-vectors/link/send/` (`PacketGenerator`,
//! `AccessGenerator`, `BitstreamGenerator`) through the C ABI handles of `aldis-ffi` (M4 WP-7a;
//! LNK-003, LNK-010…LNK-015, LNK-020, LNK-021, CABI-030…032, CABI-040, TV-010…TV-012).
//!
//! Every step builds the flat input struct from the scenario's input, calls `_step` on the storage
//! of the scenario, and compares the complete, ordered actions, or the error with its field,
//! and the deadline with the expected values, as the Rust runners of `aldis-link` do against the
//! Rust API. The engines of the C ABI have the capacities of `capacities.toml`, not those of the
//! scenarios and ignore their `queue_capacity`. A `PacketGenerator` scenario that expects
//! `WouldBlock` (LNK-015) depends on the capacity of its queue (48 or 64 octets): its steps run
//! up to the first such step, the scenario is counted as cut off there, and the Rust runner
//! covers the refusal. Issue #240 drops this cut-off.

#![expect(
    clippy::borrow_as_ptr,
    clippy::too_many_lines,
    reason = "test-only harness: the C calls take references as pointers, one long loop per engine (RS-018)"
)]

use core::ptr::null;

use aldis::core::{ErrorKind, FieldId};
use aldis_ffi::link::*;
use aldis_ffi::link_gen::*;
use aldis_ffi::{ALDIS_OK, aldis_error_detail_t};
use aldis_testkit::{ScenarioFile, VectorFile, hex, vectors_dir};
use serde_json::{Map, Value, json};

/// The area of a call: the largest field of the scenarios fits it.
const AREA: usize = 64;

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn byte(v: &Value) -> u8 {
    u8::try_from(num(v)).unwrap()
}

fn size(v: &Value) -> usize {
    usize::try_from(num(v)).unwrap()
}

fn octet(c: &Map<String, Value>, key: &str) -> u8 {
    hex::decode(c[key].as_str().unwrap()).unwrap()[0]
}

/// The error of a failed call as the scenarios write it: the kind and the field names.
fn error(status: u32, detail: &aldis_error_detail_t) -> Value {
    assert_eq!(status, detail.kind, "status and detail disagree");
    let kind = ErrorKind::from_code(u16::try_from(status).unwrap()).unwrap();
    let mut v = Map::new();
    v.insert("error".to_owned(), json!(kind.name()));
    if detail.field != 0 {
        v.insert(
            "field".to_owned(),
            json!(FieldId::from_code(detail.field).unwrap().name()),
        );
    }
    Value::Object(v)
}

fn protocol(c: &Map<String, Value>) -> u32 {
    match c["protocol"].as_str().unwrap() {
        "tm" => ALDIS_LINK_PROTOCOL_TM,
        "aos" => ALDIS_LINK_PROTOCOL_AOS,
        "uslp" => ALDIS_LINK_PROTOCOL_USLP,
        other => panic!("unknown protocol {other}"),
    }
}

fn channel(c: &Map<String, Value>) -> aldis_link_channel_t {
    aldis_link_channel_t {
        protocol: protocol(c),
        vc_id: byte(&c["vc"]),
        aos_edition: u8::from(c.get("aos_issue").is_some_and(|v| num(v) == 4)),
        uslp_upid: c.get("upid").map_or(0, byte),
        uslp_map: c.get("map_id").map_or(0, byte),
        uslp_qos: match c.get("qos").and_then(Value::as_str) {
            None | Some("sequence_controlled") => ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
            Some("expedited") => ALDIS_LINK_QOS_EXPEDITED,
            Some(other) => panic!("unknown qos {other}"),
        },
    }
}

fn route(map_id: u8, qos: u32, v: &mut Map<String, Value>) {
    v.insert("map".to_owned(), json!(map_id));
    v.insert(
        "qos".to_owned(),
        json!(match qos {
            ALDIS_LINK_QOS_SEQUENCE_CONTROLLED => "sequence_controlled",
            ALDIS_LINK_QOS_EXPEDITED => "expedited",
            other => panic!("qos {other}"),
        }),
    );
}

fn discarded(octets: usize) -> Value {
    json!({"notify": {"kind": "Discarded", "octets": octets}})
}

fn field(vc: u8, area: &[u8], r: &aldis_ffi::range::aldis_range_t) -> Map<String, Value> {
    let mut v = Map::new();
    v.insert("vc".to_owned(), json!(vc));
    v.insert(
        "field_hex".to_owned(),
        json!(hex::encode(&area[r.offset..r.offset + r.len])),
    );
    v
}

/// Whether a step expects `WouldBlock` (LNK-015).
fn expects_would_block(step: &aldis_testkit::Step) -> bool {
    step.expect
        .iter()
        .any(|e| e.get("error") == Some(&json!("WouldBlock")))
}

fn load(name: &str, engine: &str) -> ScenarioFile {
    let text = std::fs::read_to_string(vectors_dir().join(name)).unwrap();
    let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    assert_eq!(file.engine, engine);
    file
}

mod packet {
    use super::*;

    /// The configuration of a scenario; it ignores the scenario's `queue_capacity`, which the
    /// handle fixes at build time (CABI-030).
    fn config(c: &Map<String, Value>) -> aldis_packet_generator_config_t {
        let pvns: Vec<u8> = c["valid_pvns"]
            .as_array()
            .unwrap()
            .iter()
            .map(byte)
            .collect();
        aldis_packet_generator_config_t {
            channel: channel(c),
            field_len: size(&c["field_len"]),
            space_packets: u8::from(pvns.contains(&0)),
            encapsulation_packets: u8::from(pvns.contains(&7)),
            max_packet_len: size(&c["max_packet_len"]),
            idle_policy: match c["idle_policy"].as_str().unwrap() {
                "oid" => ALDIS_LINK_IDLE_POLICY_OID,
                "idle_field" => ALDIS_LINK_IDLE_POLICY_IDLE_FIELD,
                other => panic!("unknown idle policy {other}"),
            },
            idle_packet_kind: match c.get("idle_packet_kind").and_then(Value::as_str) {
                Some("space_packet") => ALDIS_LINK_IDLE_PACKET_SPACE_PACKET,
                Some("encapsulation") | None => ALDIS_LINK_IDLE_PACKET_ENCAPSULATION,
                Some(other) => panic!("unknown idle packet kind {other}"),
            },
            idle_pattern: octet(c, "idle_pattern_hex"),
            release_deadline_us: c.get("release_deadline_us").map_or(0, num),
        }
    }

    fn output(a: &aldis_packet_generator_action_t, area: &[u8]) -> Value {
        match a.kind {
            ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD => {
                let mut v = field(a.vc_id, area, &a.field);
                v.insert("fhp".to_owned(), json!(a.pointer));
                if a.route_present == 1 {
                    route(a.map_id, a.qos, &mut v);
                }
                json!({ "data_field": v })
            }
            ALDIS_PACKET_GENERATOR_ACTION_DISCARDED => discarded(a.octets),
            other => panic!("action kind {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, usize, Vec<String>) {
        let file = load(name, "packet-generator");
        let (mut run, mut skipped, mut failures) = (0, 0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_packet_generator_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_PACKET_GENERATOR_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_packet_generator_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                if expects_would_block(step) {
                    // The refusal depends on the 48 or 64 octets of the scenario's queue, which
                    // the handle cannot be built with (LNK-015; #240 drops this cut-off): the
                    // steps before it ran, the rest is covered by the Rust runner.
                    skipped += 1;
                    break;
                }
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_packet_generator_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    config: cfg,
                };
                match key.as_str() {
                    "submit_packet" => {
                        data = hex::decode(v["packet_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                    }
                    "release" => input.kind = ALDIS_PACKET_GENERATOR_INPUT_RELEASE,
                    "timeout" => input.kind = ALDIS_PACKET_GENERATOR_INPUT_TIMEOUT,
                    "reconfigure" => {
                        input.kind = ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE;
                        input.config = config(v["config"].as_object().unwrap());
                    }
                    other => panic!("unknown input {other}"),
                }
                let mut area = [0_u8; AREA];
                let mut actions = aldis_packet_generator_actions_t::default();
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_packet_generator_step(
                        &raw mut *storage,
                        &input,
                        step.at_us,
                        area.as_mut_ptr(),
                        area.len(),
                        &mut actions,
                        &mut det,
                    )
                };
                let got: Vec<Value> = if status == ALDIS_OK {
                    actions.items[..actions.count]
                        .iter()
                        .map(|a| output(a, &area))
                        .collect()
                } else {
                    vec![error(status, &det)]
                };
                let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
                let (mut armed, mut at) = (0_u8, 0_u64);
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_packet_generator_next_deadline(
                        &raw mut *storage,
                        &mut armed,
                        &mut at,
                        &mut det,
                    )
                };
                assert_eq!(status, ALDIS_OK);
                let deadline = (armed == 1).then_some(at);
                let mut found = Vec::new();
                if got != want {
                    found.push(format!(
                        "got {} want {}",
                        Value::Array(got),
                        Value::Array(want)
                    ));
                }
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

mod access {
    use super::*;

    fn config(c: &Map<String, Value>) -> aldis_access_generator_config_t {
        aldis_access_generator_config_t {
            channel: channel(c),
            field_len: size(&c["field_len"]),
            max_sdu_len: c.get("max_sdu_len").map_or(0, size),
            idle_pattern: c
                .get("idle_pattern_hex")
                .map_or(0, |_| octet(c, "idle_pattern_hex")),
        }
    }

    fn output(a: &aldis_access_generator_action_t, area: &[u8]) -> Value {
        match a.kind {
            ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD => {
                let mut v = field(a.vc_id, area, &a.field);
                if a.pointer_present == 1 {
                    v.insert("lvo".to_owned(), json!(a.pointer));
                }
                if a.route_present == 1 {
                    route(a.map_id, a.qos, &mut v);
                }
                json!({ "data_field": v })
            }
            ALDIS_ACCESS_GENERATOR_ACTION_DISCARDED => discarded(a.octets),
            other => panic!("action kind {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, Vec<String>) {
        let file = load(name, "access-generator");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_access_generator_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_ACCESS_GENERATOR_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_access_generator_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_access_generator_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    config: cfg,
                };
                match key.as_str() {
                    "submit_sdu" => {
                        data = hex::decode(v["sdu_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                    }
                    "release" => input.kind = ALDIS_ACCESS_GENERATOR_INPUT_RELEASE,
                    "reconfigure" => {
                        input.kind = ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE;
                        input.config = config(v["config"].as_object().unwrap());
                    }
                    other => panic!("unknown input {other}"),
                }
                let mut area = [0xEE_u8; AREA];
                let mut actions = aldis_access_generator_actions_t::default();
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_access_generator_step(
                        &raw mut *storage,
                        &input,
                        area.as_mut_ptr(),
                        area.len(),
                        &mut actions,
                        &mut det,
                    )
                };
                let got: Vec<Value> = if status == ALDIS_OK {
                    actions.items[..actions.count]
                        .iter()
                        .map(|a| output(a, &area))
                        .collect()
                } else {
                    vec![error(status, &det)]
                };
                let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
                if got != want {
                    failures.push(format!(
                        "{} step {i} {}: got {} want {}",
                        scenario.scenario.id,
                        Value::Object(step.input.clone()),
                        Value::Array(got),
                        Value::Array(want)
                    ));
                    break;
                }
            }
        }
        (run, failures)
    }
}

mod bitstream {
    use super::*;

    fn config(c: &Map<String, Value>) -> aldis_bitstream_generator_config_t {
        aldis_bitstream_generator_config_t {
            channel: channel(c),
            field_len: size(&c["field_len"]),
            idle_pattern: octet(c, "idle_pattern_hex"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, Vec<String>) {
        let file = load(name, "bitstream-generator");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_bitstream_generator_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_BITSTREAM_GENERATOR_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status =
                unsafe { aldis_bitstream_generator_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_bitstream_generator_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    bit_len: 0,
                    config: cfg,
                };
                match key.as_str() {
                    "submit_bits" => {
                        data = hex::decode(v["bits_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_BITSTREAM_GENERATOR_INPUT_SUBMIT_BITS;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                        input.bit_len = size(&v["bit_len"]);
                    }
                    "release" => input.kind = ALDIS_BITSTREAM_GENERATOR_INPUT_RELEASE,
                    "reconfigure" => {
                        input.kind = ALDIS_BITSTREAM_GENERATOR_INPUT_RECONFIGURE;
                        input.config = config(v["config"].as_object().unwrap());
                    }
                    other => panic!("unknown input {other}"),
                }
                let mut area = [0xEE_u8; AREA];
                let mut actions = aldis_bitstream_generator_actions_t::default();
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_bitstream_generator_step(
                        &raw mut *storage,
                        &input,
                        area.as_mut_ptr(),
                        area.len(),
                        &mut actions,
                        &mut det,
                    )
                };
                let got: Vec<Value> = if status == ALDIS_OK {
                    actions.items[..actions.count]
                        .iter()
                        .map(|a| match a.kind {
                            ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD => {
                                let mut v = field(a.vc_id, &area, &a.field);
                                v.insert("bdp".to_owned(), json!(a.pointer));
                                json!({ "data_field": v })
                            }
                            ALDIS_BITSTREAM_GENERATOR_ACTION_DISCARDED => discarded(a.octets),
                            other => panic!("action kind {other}"),
                        })
                        .collect()
                } else {
                    vec![error(status, &det)]
                };
                let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
                if got != want {
                    failures.push(format!(
                        "{} step {i} {}: got {} want {}",
                        scenario.scenario.id,
                        Value::Object(step.input.clone()),
                        Value::Array(got),
                        Value::Array(want)
                    ));
                    break;
                }
            }
        }
        (run, failures)
    }
}

/// Runs the scenarios of `name`; the engine is named by the file.
pub(crate) fn run_scenario_file(name: &str) {
    let (run, skipped, failures) = if name.contains("packet_generator") {
        packet::run_file(name)
    } else if name.contains("access_generator") {
        let (run, failures) = access::run_file(name);
        (run, 0, failures)
    } else {
        let (run, failures) = bitstream::run_file(name);
        (run, 0, failures)
    };
    assert!(
        failures.is_empty(),
        "{} of {run} scenarios failed ({skipped} skipped):\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(run > 0, "{name}: no scenario ran");
}
