//! Runs the scenarios of the generation engines in `test-vectors/link/send/` (`PacketGenerator`,
//! `AccessGenerator`, `BitstreamGenerator`) and of the receiving engines in
//! `test-vectors/link/recv/` (`PacketExtractor`, `FrameReceiver`) through the C ABI handles of
//! `aldis-ffi` (M4 WP-7a, WP-7b; LNK-003, LNK-010…LNK-015, LNK-020, LNK-021, LNK-050…LNK-054,
//! LNK-060…LNK-064, LNK-077, LNK-079…LNK-081, CABI-030…032, CABI-040, TV-010…TV-012).
//!
//! Every step builds the flat input struct from the scenario's input, calls `_step` on the storage
//! of the scenario, and compares the complete, ordered actions, or the error with its field,
//! and the deadline with the expected values, as the Rust runners of `aldis-link` do against the
//! Rust API. The engines of the C ABI have the capacities of `capacities.toml`, not those of the
//! scenarios and ignore their `queue_capacity`. A `PacketGenerator` scenario that expects
//! `WouldBlock` (LNK-015) depends on the capacity of its queue (48 or 64 octets), which the
//! handle fixes at `ALDIS_LINK_PACKET_QUEUE_OCTETS` (8 192 since #240) and no configuration
//! lowers: its steps run up to the first such step, the scenario is counted as cut off there, and
//! the Rust runner covers the refusal. The cut-off stays; restoring the sizes does not remove it.

#![expect(
    clippy::borrow_as_ptr,
    clippy::too_many_lines,
    reason = "test-only harness: the C calls take references as pointers, one long loop per engine (RS-018)"
)]

use core::ptr::null;

use aldis::core::engine::OutRange;
use aldis::core::{ErrorKind, FieldId};
use aldis::link::extractor::PacketRun;
use aldis_ffi::link::*;
use aldis_ffi::link_gen::*;
use aldis_ffi::link_rx::*;
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
        vc_id: c.get("vc").map_or(0, byte),
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
                    // the handle cannot be built with (LNK-015; the queue is the build constant): the
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

mod extract {
    use super::*;

    fn config(c: &Map<String, Value>) -> aldis_packet_extractor_config_t {
        let pvns: Vec<u8> = c["valid_pvns"]
            .as_array()
            .unwrap()
            .iter()
            .map(byte)
            .collect();
        aldis_packet_extractor_config_t {
            channel: channel(c),
            space_packets: u8::from(pvns.contains(&0)),
            encapsulation_packets: u8::from(pvns.contains(&7)),
            max_packet_len: size(&c["max_packet_len"]),
            deliver_incomplete: u8::from(c["deliver_incomplete"].as_bool().unwrap()),
            deliver_idle: u8::from(c["deliver_idle"].as_bool().unwrap()),
        }
    }

    fn packet(octets: &[u8], incomplete: bool) -> Value {
        json!({"deliver": {
            "kind": "Packet",
            "pvn": octets.first().map_or(9, |b| b >> 5),
            "data_hex": hex::encode(octets),
            "incomplete": incomplete,
        }})
    }

    fn outputs(a: &aldis_packet_extractor_action_t, area: &[u8]) -> Vec<Value> {
        let r = OutRange::new(a.packets.offset, a.packets.len);
        match a.kind {
            ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS => {
                let packets: Vec<Value> = PacketRun::new(r, area)
                    .unwrap()
                    .map(|p| packet(p, false))
                    .collect();
                assert_eq!(packets.len(), a.count);
                packets
            }
            ALDIS_PACKET_EXTRACTOR_ACTION_INCOMPLETE_PACKET => {
                let octets = r.slice(area).unwrap();
                assert_eq!(a.pvn, octets[0] >> 5);
                vec![packet(octets, true)]
            }
            ALDIS_PACKET_EXTRACTOR_ACTION_DISCARDED => vec![discarded(a.octets)],
            ALDIS_PACKET_EXTRACTOR_ACTION_ERROR => {
                vec![json!({"extraction_error": {"kind": match a.error {
                    ALDIS_PACKET_EXTRACTOR_ERROR_PACKET_TOO_LONG => "PacketTooLong",
                    ALDIS_PACKET_EXTRACTOR_ERROR_INVALID_PVN => "InvalidPvn",
                    ALDIS_PACKET_EXTRACTOR_ERROR_LENGTH_INCONSISTENT => "LengthInconsistent",
                    other => panic!("error {other}"),
                }}})]
            }
            other => panic!("action kind {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, Vec<String>) {
        let file = load(name, "packet-extractor");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_packet_extractor_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_PACKET_EXTRACTOR_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_packet_extractor_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_packet_extractor_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    header_pointer_present: 0,
                    header_pointer: 0,
                    gap: 0,
                    config: cfg,
                };
                let mut counter = None;
                match key.as_str() {
                    "data_field" => {
                        data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                        if let Some(f) = v.get("fhp") {
                            input.header_pointer_present = 1;
                            input.header_pointer = u16::try_from(num(f)).unwrap();
                        }
                        input.gap = u8::from(v["gap"].as_bool().unwrap());
                    }
                    "reconfigure" => input.kind = ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE,
                    "query" => counter = Some(v["counter"].as_str().unwrap()),
                    other => panic!("unknown input {other}"),
                }
                let got: Vec<Value> = if let Some(name) = counter {
                    let mut s = aldis_packet_extractor_status_t::default();
                    // SAFETY: every pointer is to a live local.
                    let status = unsafe {
                        aldis_packet_extractor_status(&raw mut *storage, &mut s, &mut det)
                    };
                    assert_eq!(status, ALDIS_OK);
                    let value = match name {
                        "PacketTooLong" => s.packet_too_long,
                        "InvalidPvn" => s.invalid_pvn,
                        "LengthInconsistent" => s.length_inconsistent,
                        "idle_packets" => s.idle_packets,
                        other => panic!("unknown counter {other}"),
                    };
                    vec![json!({"counter": {"name": name, "value": value}})]
                } else {
                    let mut area = [0_u8; 512];
                    let mut actions = aldis_packet_extractor_actions_t::default();
                    // SAFETY: every pointer is to a live local.
                    let status = unsafe {
                        aldis_packet_extractor_step(
                            &raw mut *storage,
                            &input,
                            area.as_mut_ptr(),
                            area.len(),
                            &mut actions,
                            &mut det,
                        )
                    };
                    if status == ALDIS_OK {
                        actions.items[..actions.count]
                            .iter()
                            .flat_map(|a| outputs(a, &area))
                            .collect()
                    } else {
                        vec![error(status, &det)]
                    }
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

mod receive {
    use super::*;

    fn service(v: &Value) -> u32 {
        match v.as_str().unwrap() {
            "none" => ALDIS_LINK_SERVICE_NONE,
            "vc" => ALDIS_LINK_SERVICE_VC,
            "mc" => ALDIS_LINK_SERVICE_MC,
            other => panic!("unknown service {other}"),
        }
    }

    fn config(c: &Map<String, Value>) -> aldis_frame_receiver_config_t {
        let mut cfg = aldis_frame_receiver_config_t {
            protocol: match c["protocol"].as_str().unwrap() {
                "tc" => ALDIS_LINK_PROTOCOL_TC,
                _ => protocol(c),
            },
            frame_len: size(&c["frame_len"]),
            fecf: u8::from(c["fecf"].as_bool().unwrap()),
            raw_frames: u8::from(
                c.get("accept_rejected_frame_insert_zone")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            insert_zone_len: c.get("insert_zone_len").map_or(0, size),
            ..aldis_frame_receiver_config_t::default()
        };
        if let Some(s) = c.get("ocf_service") {
            cfg.ocf_service = service(s);
        }
        if let Some(s) = c.get("fsh_service") {
            cfg.fsh_service = service(s);
        }
        for (slot, vc) in cfg.vcs.iter_mut().zip(c["vcs"].as_array().unwrap()) {
            let content = match vc.get("role").and_then(Value::as_str) {
                Some("packets") | None => ALDIS_LINK_CONTENT_PACKETS,
                Some("vca") => ALDIS_LINK_CONTENT_VCA,
                Some("bitstream") => ALDIS_LINK_CONTENT_BITSTREAM,
                Some("unserved") => ALDIS_LINK_CONTENT_UNSERVED,
                Some(other) => panic!("unknown role {other}"),
            };
            *slot = aldis_frame_receiver_vc_t {
                vc_id: byte(&vc["vcid"]),
                content,
                ocf: u8::from(vc.get("ocf").and_then(Value::as_bool).unwrap_or(false)),
                fsh_len: vc.get("fsh_len").map_or(0, byte),
                cop: u8::from(vc.get("cop").and_then(Value::as_bool).unwrap_or(false)),
                maps: vc.get("maps").map_or(0, |m| {
                    m.as_array()
                        .unwrap()
                        .iter()
                        .fold(0_u16, |set, n| set | (1 << num(n)))
                }),
            };
            cfg.vc_count += 1;
        }
        cfg
    }

    fn scope(a: &aldis_frame_receiver_action_t) -> (Value, Option<u8>) {
        match a.scope {
            ALDIS_LINK_SCOPE_MASTER_CHANNEL => (json!("mc"), None),
            ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL => (json!("vc"), Some(a.vc_id)),
            other => panic!("scope {other}"),
        }
    }

    fn with(mut m: Map<String, Value>, key: &str, v: Option<Value>) -> Map<String, Value> {
        if let Some(v) = v {
            m.insert(key.to_owned(), v);
        }
        m
    }

    fn replay(a: &aldis_frame_receiver_action_t) -> Option<Value> {
        (a.replay_present == 1).then(|| json!(a.replay == 1))
    }

    fn kind(k: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("kind".to_owned(), json!(k));
        m
    }

    fn data_hex(a: &aldis_frame_receiver_action_t, area: &[u8]) -> String {
        hex::encode(&area[a.data.offset..a.data.offset + a.data.len])
    }

    fn output(a: &aldis_frame_receiver_action_t, area: &[u8]) -> Value {
        match a.kind {
            ALDIS_FRAME_RECEIVER_ACTION_FRAME_GAP => {
                let (sc, vc) = scope(a);
                let m = with(kind("FrameGap"), "vc", vc.map(|v| json!(v)));
                let mut m = with(m, "scope", Some(sc));
                m.insert("missing".to_owned(), json!(a.missing));
                json!({"notify": m})
            }
            ALDIS_FRAME_RECEIVER_ACTION_COUNT_DISCONTINUITY => {
                let (sc, vc) = scope(a);
                let m = with(kind("CountDiscontinuity"), "vc", vc.map(|v| json!(v)));
                json!({"notify": with(m, "scope", Some(sc))})
            }
            ALDIS_FRAME_RECEIVER_ACTION_FRAME_DISCARDED => {
                let mut m = Map::new();
                m.insert(
                    "reason".to_owned(),
                    json!(match a.reason {
                        ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL => "UnknownChannel",
                        ALDIS_LINK_DISCARD_NOT_SERVED => "NotServed",
                        ALDIS_LINK_DISCARD_CRC_MISMATCH => "CrcMismatch",
                        ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE => "HeaderUncorrectable",
                        ALDIS_LINK_DISCARD_SECURITY => "Security",
                        other => panic!("reason {other}"),
                    }),
                );
                let m = with(m, "vc", (a.vc_present == 1).then(|| json!(a.vc_id)));
                let m = with(m, "map", (a.map_present == 1).then(|| json!(a.map_id)));
                json!({"frame_discarded": m})
            }
            ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG => json!({"notify": {"kind": "LossFlag"}}),
            ALDIS_FRAME_RECEIVER_ACTION_DATA_FIELD => {
                let mut m = kind("DataField");
                m.insert("vc".to_owned(), json!(a.vc_id));
                m.insert("data_hex".to_owned(), json!(data_hex(a, area)));
                m.insert("gap".to_owned(), json!(a.gap == 1));
                let m = with(m, "fhp", (a.pointer_present == 1).then(|| json!(a.pointer)));
                let m = with(
                    m,
                    "bit_count",
                    (a.bit_count_present == 1).then(|| json!(a.bit_count)),
                );
                json!({"deliver": with(m, "replay", replay(a))})
            }
            ALDIS_FRAME_RECEIVER_ACTION_OCF => {
                let (sc, vc) = scope(a);
                let mut m = kind("Ocf");
                m.insert("scope".to_owned(), sc);
                // The report type of the vectors: a CLCW is Type-1 (bit 0 of the word is 0).
                m.insert(
                    "report".to_owned(),
                    json!(if a.ocf >> 31 == 0 { "Type1" } else { "Type2" }),
                );
                m.insert(
                    "ocf_hex".to_owned(),
                    json!(hex::encode(&a.ocf.to_be_bytes())),
                );
                let m = with(m, "vc", vc.map(|v| json!(v)));
                json!({"deliver": with(m, "replay", replay(a))})
            }
            ALDIS_FRAME_RECEIVER_ACTION_FSH_SDU => {
                let (sc, vc) = scope(a);
                let mut m = kind("FshSdu");
                m.insert("scope".to_owned(), sc);
                m.insert("data_hex".to_owned(), json!(data_hex(a, area)));
                json!({"deliver": with(m, "vc", vc.map(|v| json!(v)))})
            }
            ALDIS_FRAME_RECEIVER_ACTION_INSERT_SDU => {
                let mut m = kind("InsertSdu");
                m.insert("data_hex".to_owned(), json!(data_hex(a, area)));
                json!({"deliver": with(m, "replay", replay(a))})
            }
            ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME | ALDIS_FRAME_RECEIVER_ACTION_PROTOCOL_CONTROL => {
                let mut m = kind(if a.kind == ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME {
                    "TcFrame"
                } else {
                    "ProtocolControl"
                });
                m.insert("vc".to_owned(), json!(a.vc_id));
                m.insert("data_hex".to_owned(), json!(data_hex(a, area)));
                json!({"deliver": m})
            }
            other => panic!("action kind {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, Vec<String>) {
        let file = load(name, "frame-receiver");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_frame_receiver_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_FRAME_RECEIVER_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_frame_receiver_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}: {det:?}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_frame_receiver_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    reason: 0,
                    config: cfg,
                };
                let mut counter = None;
                match key.as_str() {
                    "frame" | "raw_frame" => {
                        data = hex::decode(v["frame_hex"].as_str().unwrap()).unwrap();
                        input.kind = if key == "frame" {
                            ALDIS_FRAME_RECEIVER_INPUT_FRAME
                        } else {
                            ALDIS_FRAME_RECEIVER_INPUT_RAW_FRAME
                        };
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                    }
                    "caller_discard" => {
                        input.kind = ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD;
                        input.reason = match v["reason"].as_str().unwrap() {
                            "CrcMismatch" => ALDIS_LINK_DISCARD_CRC_MISMATCH,
                            "HeaderUncorrectable" => ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE,
                            "Security" => ALDIS_LINK_DISCARD_SECURITY,
                            other => panic!("unknown reason {other}"),
                        };
                    }
                    "frame_loss" => input.kind = ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS,
                    "reconfigure" => input.kind = ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE,
                    "query" => counter = Some(v["counter"].as_str().unwrap()),
                    other => panic!("unknown input {other}"),
                }
                let got: Vec<Value> = if let Some(name) = counter {
                    let mut s = aldis_frame_receiver_status_t::default();
                    // SAFETY: every pointer is to a live local.
                    let status =
                        unsafe { aldis_frame_receiver_status(&raw mut *storage, &mut s, &mut det) };
                    assert_eq!(status, ALDIS_OK);
                    let value = match name {
                        "UnknownChannel" => s.unknown_channel,
                        "NotServed" => s.not_served,
                        "CrcMismatch" => s.crc_mismatch,
                        "HeaderUncorrectable" => s.header_uncorrectable,
                        "Security" => s.security,
                        "oid_frames" => s.oid_frames,
                        "gaps" => s.gaps,
                        "count_discontinuities" => s.count_discontinuities,
                        other => panic!("unknown counter {other}"),
                    };
                    vec![json!({"counter": {"name": name, "value": value}})]
                } else {
                    let mut area = [0_u8; 2048];
                    let mut actions = aldis_frame_receiver_actions_t::default();
                    // SAFETY: every pointer is to a live local.
                    let status = unsafe {
                        aldis_frame_receiver_step(
                            &raw mut *storage,
                            &input,
                            area.as_mut_ptr(),
                            area.len(),
                            &mut actions,
                            &mut det,
                        )
                    };
                    if status == ALDIS_OK {
                        actions.items[..actions.count]
                            .iter()
                            .map(|a| output(a, &area))
                            .collect()
                    } else {
                        vec![error(status, &det)]
                    }
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
    } else if name.contains("packet_extractor") {
        let (run, failures) = extract::run_file(name);
        (run, 0, failures)
    } else if name.contains("frame_receiver") {
        let (run, failures) = receive::run_file(name);
        (run, 0, failures)
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
