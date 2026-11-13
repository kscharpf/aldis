//! Runs the scenarios of the generation engines in `test-vectors/link/send/` (`PacketGenerator`,
//! `AccessGenerator`, `BitstreamGenerator`) and of the receiving engines in
//! `test-vectors/link/recv/` (`PacketExtractor`, `FrameReceiver`), the `ChannelMux`, the
//! `MapSegmenter`, and the `MapReassembler` through the C ABI handles of `aldis-ffi` (M4 WP-7a to
//! WP-7d; LNK-003, LNK-010…LNK-015, LNK-020, LNK-021, LNK-050…LNK-054,
//! LNK-040…LNK-043, LNK-060…LNK-064, LNK-070…LNK-073, LNK-077, LNK-079…LNK-081, CABI-030…032, CABI-040, TV-010…TV-012).
//!
//! Every step builds the flat input struct from the scenario's input, calls `_step` on the storage
//! of the scenario, and compares the complete, ordered actions, or the error with its field,
//! and the deadline with the expected values, as the Rust runners of `aldis-link` do against the
//! Rust API. The engines of the C ABI have the capacities of `capacities.toml`, not those of the
//! scenarios and ignore their `queue_capacity`. A `PacketGenerator` or `MapSegmenter` scenario that expects
//! `WouldBlock` (LNK-015) depends on the capacity of its queue (48 or 64 octets), which the
//! handle fixes at `ALDIS_LINK_PACKET_QUEUE_OCTETS` (8 192 since #240) and no configuration
//! lowers: its steps run up to the first such step, the scenario is counted as cut off there, and
//! the Rust runner covers the refusal. The cut-off stays; restoring the sizes does not remove it.

#![expect(
    clippy::borrow_as_ptr,
    clippy::large_stack_arrays,
    clippy::too_many_lines,
    reason = "test-only harness: the C calls take references as pointers, one long loop per engine (RS-018)"
)]

use core::ptr::null;

use aldis::core::engine::OutRange;
use aldis::core::{ErrorKind, FieldId};
use aldis::link::extractor::PacketRun;
use aldis_ffi::link::access::*;
use aldis_ffi::link::bitstream::*;
use aldis_ffi::link::extractor::*;
use aldis_ffi::link::generator::*;
use aldis_ffi::link::mux::*;
use aldis_ffi::link::common::*;
use aldis_ffi::link::reassembler::*;
use aldis_ffi::link::receiver::*;
use aldis_ffi::link::segmenter::*;
use aldis_ffi::link::*;
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
        "tc" => ALDIS_LINK_PROTOCOL_TC,
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

mod mux {
    use super::*;

    fn ocf_word(hex_octets: &str) -> u32 {
        u32::from_be_bytes(hex::decode(hex_octets).unwrap().try_into().unwrap())
    }

    fn qos_of(name: Option<&str>) -> u32 {
        match name {
            None | Some("sequence_controlled") => ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
            Some("expedited") => ALDIS_LINK_QOS_EXPEDITED,
            Some(other) => panic!("unknown qos {other}"),
        }
    }

    fn vc(v: &Value) -> aldis_channel_mux_vc_t {
        let o = v.as_object().unwrap();
        let mut fsh_init = [0_u8; ALDIS_CHANNEL_MUX_MAX_FSH_DATA];
        let fsh = o
            .get("fsh_init_hex")
            .map(|h| hex::decode(h.as_str().unwrap()).unwrap());
        if let Some(f) = &fsh {
            fsh_init[..f.len()].copy_from_slice(f);
        }
        aldis_channel_mux_vc_t {
            vc_id: byte(&o["vcid"]),
            priority: byte(&o["priority"]),
            weight: u16::try_from(num(&o["weight"])).unwrap(),
            sec_header_len: o.get("sec_header_len").map_or(0, size),
            sec_trailer_len: o.get("sec_trailer_len").map_or(0, size),
            role: match o["role"].as_str().unwrap() {
                "packets" => ALDIS_LINK_CONTENT_PACKETS,
                // The USLP roles are the generators' business; the mux does not read them.
                "vca" | "mapa" => ALDIS_LINK_CONTENT_VCA,
                "bitstream" => ALDIS_LINK_CONTENT_BITSTREAM,
                other => panic!("unknown role {other}"),
            },
            ocf: u8::from(o.contains_key("ocf_init_hex")),
            ocf_init: o
                .get("ocf_init_hex")
                .map_or(0, |h| ocf_word(h.as_str().unwrap())),
            fsh_len: u8::try_from(fsh.map_or(0, |f| f.len())).unwrap(),
            fsh_init,
            uslp_map: o.get("map_id").map_or(0, byte),
            uslp_qos: qos_of(o.get("qos").and_then(Value::as_str)),
            uslp_vcf_len_sc: o.get("vcf_len_sc").map_or(0, byte),
            uslp_vcf_len_exp: o.get("vcf_len_exp").map_or(0, byte),
            uslp_cop: u8::from(o.get("cop").and_then(Value::as_bool).unwrap_or(false)),
        }
    }

    pub(super) fn config(c: &Map<String, Value>) -> aldis_channel_mux_config_t {
        let mut mc_fsh_init = [0_u8; ALDIS_CHANNEL_MUX_MAX_FSH_DATA];
        let mc_fsh = c
            .get("mc_fsh_init_hex")
            .map(|h| hex::decode(h.as_str().unwrap()).unwrap());
        if let Some(f) = &mc_fsh {
            mc_fsh_init[..f.len()].copy_from_slice(f);
        }
        let mut vcs = [aldis_channel_mux_vc_t::default();
            aldis_ffi::capacities::ALDIS_LINK_MAX_VIRTUAL_CHANNELS];
        let listed = c["vcs"].as_array().unwrap();
        for (slot, v) in vcs.iter_mut().zip(listed) {
            *slot = vc(v);
        }
        aldis_channel_mux_config_t {
            protocol: protocol(c),
            scid: u16::try_from(num(&c["scid"])).unwrap(),
            frame_len: size(&c["frame_len"]),
            fecf: u8::from(c["fecf"].as_bool().unwrap()),
            policy: match c["policy"]["kind"].as_str().unwrap() {
                "fixed_priority" => ALDIS_LINK_MUX_POLICY_FIXED_PRIORITY,
                "round_robin" => ALDIS_LINK_MUX_POLICY_ROUND_ROBIN,
                "weighted_round_robin" => ALDIS_LINK_MUX_POLICY_WEIGHTED_ROUND_ROBIN,
                other => panic!("unknown policy {other}"),
            },
            insert_zone_len: c.get("insert_zone_len").map_or(0, size),
            insert_idle_pattern: c
                .get("insert_init_hex")
                .map_or(0, |_| octet(c, "insert_init_hex")),
            tm_oid_vc: c.get("oid_vc").map_or(0, byte),
            mc_ocf: u8::from(c.contains_key("mc_ocf_init_hex")),
            mc_ocf_init: c
                .get("mc_ocf_init_hex")
                .map_or(0, |h| ocf_word(h.as_str().unwrap())),
            mc_fsh_len: u8::try_from(mc_fsh.map_or(0, |f| f.len())).unwrap(),
            mc_fsh_init,
            aos_edition: 0,
            aos_fhec: u8::from(c.get("fhec").and_then(Value::as_bool).unwrap_or(false)),
            aos_cycle_in_use: u8::from(
                c.get("cycle_in_use")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            uslp_destination: c.get("source_or_destination").map_or(0, byte),
            uslp_oid_vcf_len: c.get("oid").map_or(0, |o| byte(&o["vcf_len"])),
            vc_count: listed.len(),
            vcs,
        }
    }

    fn range_of(r: &aldis_ffi::range::aldis_range_t) -> Value {
        json!([r.offset, r.offset + r.len])
    }

    fn output(a: &aldis_channel_mux_action_t, area: &[u8]) -> Value {
        match a.kind {
            ALDIS_CHANNEL_MUX_ACTION_TRANSMIT => json!({"transmit": {
                "kind": match a.class {
                    ALDIS_LINK_FRAME_CLASS_DATA => "data",
                    ALDIS_LINK_FRAME_CLASS_OID => "oid",
                    other => panic!("class {other}"),
                },
                "vc": a.vc_id,
                "frame_hex": hex::encode(&area[a.frame.offset..a.frame.offset + a.frame.len]),
                "security_header": range_of(&a.security_header),
                "data": range_of(&a.data),
                "security_trailer": range_of(&a.security_trailer),
            }}),
            ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED => {
                json!({"notify": {"kind": "InsertSduRepeated"}})
            }
            ALDIS_CHANNEL_MUX_ACTION_DISCARDED => discarded(a.octets),
            other => panic!("action kind {other}"),
        }
    }

    fn scope(v: &Value, input: &mut aldis_channel_mux_input_t) {
        match v["scope"].as_str().unwrap() {
            "mc" => input.scope = ALDIS_LINK_SCOPE_MASTER_CHANNEL,
            "vc" => {
                input.scope = ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL;
                input.vc_id = byte(&v["vc"]);
            }
            other => panic!("unknown scope {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, Vec<String>) {
        let file = load(name, "channel-mux");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_channel_mux_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_CHANNEL_MUX_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_channel_mux_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            // The configuration in force, for the default route of a VC.
            let mut current = scenario.config.clone();
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_channel_mux_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    vc_id: 0,
                    scope: 0,
                    ocf: 0,
                    pointer_present: 0,
                    pointer: 0,
                    route_present: 0,
                    map_id: 0,
                    qos: 0,
                    cop_present: 0,
                    cop_vcf_count: 0,
                    cop_bypass: 0,
                    cop_protocol_control: 0,
                    config: cfg,
                };
                match key.as_str() {
                    "offer" => {
                        data = hex::decode(v["field_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_CHANNEL_MUX_INPUT_OFFER;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                        input.vc_id = byte(&v["vc"]);
                        if let Some(p) = v.get("fhp") {
                            input.pointer_present = 1;
                            input.pointer = u16::try_from(num(p)).unwrap();
                        }
                        if v.get("map").is_some() || v.get("qos").is_some() {
                            // The default of a VC is its MAP and QoS (USLP); TM and AOS have none.
                            let entry = current["vcs"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|e| byte(&e["vcid"]) == input.vc_id)
                                .unwrap();
                            input.route_present = 1;
                            input.map_id = v
                                .get("map")
                                .map_or_else(|| entry.get("map_id").map_or(0, byte), byte);
                            input.qos = qos_of(
                                v.get("qos")
                                    .or_else(|| entry.get("qos"))
                                    .and_then(Value::as_str),
                            );
                        }
                        if let Some(c) = v.get("cop") {
                            input.cop_present = 1;
                            input.cop_vcf_count = num(&c["vcf_count"]);
                            input.cop_bypass = byte(&c["bypass"]);
                            input.cop_protocol_control = byte(&c["protocol_control"]);
                        }
                    }
                    "opportunity" => input.kind = ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY,
                    "set_ocf" => {
                        input.kind = ALDIS_CHANNEL_MUX_INPUT_SET_OCF;
                        scope(v, &mut input);
                        input.ocf = ocf_word(v["ocf_hex"].as_str().unwrap());
                    }
                    "set_fsh" => {
                        data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_CHANNEL_MUX_INPUT_SET_FSH;
                        scope(v, &mut input);
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                    }
                    "set_insert" => {
                        data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_CHANNEL_MUX_INPUT_SET_INSERT;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                    }
                    "reconfigure" => {
                        input.kind = ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE;
                        current.clone_from(v["config"].as_object().unwrap());
                        input.config = config(&current);
                    }
                    other => panic!("unknown input {other}"),
                }
                let mut area = [0xEE_u8; 2048];
                let mut actions = aldis_channel_mux_actions_t::default();
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_channel_mux_step(
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

mod segment {
    use super::*;

    /// The area of a call: the longest `max_fdu_len` of the files, as in the Rust runner.
    const AREA: usize = 32;

    fn map_entry(m: &Value, usl: bool) -> aldis_map_segmenter_map_t {
        let o = m.as_object().unwrap();
        let mut e = aldis_map_segmenter_map_t {
            map_id: byte(&o["map_id"]),
            service: ALDIS_MAP_SERVICE_ANY,
            segmentation: u8::from(
                o.get("segmentation")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            max_sdu_len: o.get("max_sdu_len").map_or(0, size),
            blocking: u8::from(o.get("blocking").and_then(Value::as_bool).unwrap_or(false)),
            uslp_upid: o.get("upid").map_or(0, byte),
            release_deadline_us: o.get("release_deadline_us").map_or(0, num),
        };
        // A USLP MAP with a `max_sdu_len` segments.
        if usl {
            e.segmentation = u8::from(o.contains_key("max_sdu_len"));
            e.service = match o["service"].as_str().unwrap() {
                "packets" => ALDIS_MAP_SERVICE_PACKETS,
                "mapa" => ALDIS_MAP_SERVICE_SDU,
                "octet_stream" => ALDIS_MAP_SERVICE_OCTET_STREAM,
                other => panic!("unknown service {other}"),
            };
        }
        e
    }

    pub(super) fn config(c: &Map<String, Value>) -> aldis_map_segmenter_config_t {
        let usl = protocol(c) == ALDIS_LINK_PROTOCOL_USLP;
        let mut maps = [aldis_map_segmenter_map_t::default();
            aldis_ffi::capacities::ALDIS_LINK_MAX_MAP_CHANNELS];
        let listed = c.get("maps").map_or(&[][..], |m| m.as_array().unwrap());
        for (slot, m) in maps.iter_mut().zip(listed) {
            *slot = map_entry(m, usl);
        }
        let mut out = aldis_map_segmenter_config_t {
            channel: aldis_link_channel_t {
                protocol: protocol(c),
                vc_id: 1,
                ..aldis_link_channel_t::default()
            },
            max_fdu_len: size(if usl {
                &c["max_tfdf_len"]
            } else {
                &c["max_fdu_len"]
            }),
            map_count: listed.len(),
            maps,
            ..aldis_map_segmenter_config_t::default()
        };
        if usl {
            out.uslp_vcf_len_sc = 2;
            out.uslp_vcf_len_exp = 2;
        } else {
            out.tc_segment_header = u8::from(c["segment_header"].as_bool().unwrap());
            out.tc_fecf = u8::from(c["fecf"].as_bool().unwrap());
            out.tc_blocking = u8::from(c.get("blocking").and_then(Value::as_bool).unwrap_or(false));
        }
        out
    }

    fn output(a: &aldis_map_segmenter_action_t, area: &[u8], usl: bool, header: bool) -> Value {
        match a.kind {
            ALDIS_MAP_SEGMENTER_ACTION_FDU => {
                let octets = &area[a.fdu.offset..a.fdu.offset + a.fdu.len];
                let mut v = Map::new();
                v.insert("handle".to_owned(), json!(a.handle));
                v.insert(
                    "qos".to_owned(),
                    json!(match a.qos {
                        ALDIS_LINK_QOS_SEQUENCE_CONTROLLED => "sequence_controlled",
                        ALDIS_LINK_QOS_EXPEDITED => "expedited",
                        other => panic!("qos {other}"),
                    }),
                );
                v.insert("fdu_hex".to_owned(), json!(hex::encode(octets)));
                assert_eq!(a.map_present == 1, header || usl);
                if a.map_present == 1 {
                    v.insert("map".to_owned(), json!(a.map_id));
                }
                assert_eq!(a.vc_id, 1);
                if header {
                    if usl {
                        let rule = match a.framing {
                            ALDIS_MAP_FRAMING_WHOLE => "111",
                            ALDIS_MAP_FRAMING_FIRST => "100",
                            ALDIS_MAP_FRAMING_CONTINUING => "101",
                            ALDIS_MAP_FRAMING_LAST => "110",
                            ALDIS_MAP_FRAMING_STREAM => "011",
                            other => panic!("framing {other}"),
                        };
                        v.insert("rule".to_owned(), json!(rule));
                        v.insert("upid".to_owned(), json!(octets[0] & 0x1F));
                    } else {
                        let flags = match a.framing {
                            ALDIS_MAP_FRAMING_WHOLE => "11",
                            ALDIS_MAP_FRAMING_FIRST => "01",
                            ALDIS_MAP_FRAMING_CONTINUING => "00",
                            ALDIS_MAP_FRAMING_LAST => "10",
                            other => panic!("framing {other}"),
                        };
                        v.insert("flags".to_owned(), json!(flags));
                    }
                }
                json!({ "fdu": v })
            }
            ALDIS_MAP_SEGMENTER_ACTION_DISCARDED => discarded(a.octets),
            other => panic!("action kind {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, usize, Vec<String>) {
        let file = load(name, "map-segmenter");
        let (mut run, mut skipped, mut failures) = (0, 0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let usl = protocol(scenario.config) == ALDIS_LINK_PROTOCOL_USLP;
            // A TC VC without a Segment Header has no flags to report.
            let header = scenario
                .config
                .get("segment_header")
                .is_none_or(|h| h.as_bool().unwrap());
            let mut storage = Box::new(aldis_map_segmenter_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_MAP_SEGMENTER_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_map_segmenter_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                if expects_would_block(step) {
                    // The refusal depends on the 256 octets of the scenario's queue, which the
                    // handle cannot be built with (LNK-015; the queue is the build constant):
                    // the steps before it ran, the rest is covered by the Rust runner.
                    skipped += 1;
                    break;
                }
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_map_segmenter_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    map_id: v.get("map").map_or(0, byte),
                    handle: 0,
                    qos: 0,
                    config: cfg,
                };
                match key.as_str() {
                    "submit_packet" | "submit_sdu" | "submit_octets" => {
                        let (kind, field) = match key.as_str() {
                            "submit_packet" => {
                                (ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_PACKET, "packet_hex")
                            }
                            "submit_sdu" => (ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, "sdu_hex"),
                            _ => (ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS, "data_hex"),
                        };
                        data = hex::decode(v[field].as_str().unwrap()).unwrap();
                        input.kind = kind;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                    }
                    "next_fdu" => {
                        input.kind = ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU;
                        input.handle = u32::try_from(num(&v["handle"])).unwrap();
                        input.qos = match v["qos"].as_str().unwrap() {
                            "sequence_controlled" => ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
                            "expedited" => ALDIS_LINK_QOS_EXPEDITED,
                            other => panic!("unknown qos {other}"),
                        };
                    }
                    "timeout" => input.kind = ALDIS_MAP_SEGMENTER_INPUT_TIMEOUT,
                    "reconfigure" => {
                        input.kind = ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE;
                        input.config = config(v["config"].as_object().unwrap());
                    }
                    other => panic!("unknown input {other}"),
                }
                let mut area = [0xEE_u8; AREA];
                let mut actions = aldis_map_segmenter_actions_t::default();
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_map_segmenter_step(
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
                        .map(|a| output(a, &area, usl, header))
                        .collect()
                } else {
                    vec![error(status, &det)]
                };
                let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
                let (mut armed, mut at) = (0_u8, 0_u64);
                // SAFETY: every pointer is to a live local.
                let status = unsafe {
                    aldis_map_segmenter_next_deadline(
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

mod reassemble {
    use super::*;

    /// The longest SDU of the MAPs of the files that name none, as in the Rust runner.
    const SDU: usize = 64;
    /// The output area: the longest SDU and the longest FDU of the files.
    const AREA: usize = SDU + 64;

    fn config(c: &Map<String, Value>) -> aldis_map_reassembler_config_t {
        let usl = protocol(c) == ALDIS_LINK_PROTOCOL_USLP;
        let mut maps = [aldis_map_reassembler_map_t::default();
            aldis_ffi::capacities::ALDIS_LINK_MAX_MAP_CHANNELS];
        let listed = c["maps"].as_array().unwrap();
        for (slot, m) in maps.iter_mut().zip(listed) {
            *slot = aldis_map_reassembler_map_t {
                vc_id: if usl { byte(&m["vc"]) } else { byte(&c["vc"]) },
                map_id: byte(&m["map"]),
                service: match m["service"].as_str().unwrap() {
                    "packets" => ALDIS_MAP_SERVICE_PACKETS,
                    "sdu" => ALDIS_MAP_SERVICE_SDU,
                    "octet_stream" => ALDIS_MAP_SERVICE_OCTET_STREAM,
                    other => panic!("unknown service {other}"),
                },
                max_sdu_len: m.get("max_sdu_len").map_or(SDU, size),
            };
        }
        aldis_map_reassembler_config_t {
            channel: aldis_link_channel_t {
                protocol: protocol(c),
                ..aldis_link_channel_t::default()
            },
            tc_segment_header: u8::from(!usl),
            map_count: listed.len(),
            maps,
        }
    }

    fn event_name(event: u32) -> &'static str {
        match event {
            ALDIS_MAP_REASSEMBLY_EVENT_GAP_DISCARD => "GapDiscard",
            ALDIS_MAP_REASSEMBLY_EVENT_SEQUENCE_VIOLATION => "SequenceViolation",
            ALDIS_MAP_REASSEMBLY_EVENT_UNKNOWN_MAP => "UnknownMap",
            ALDIS_MAP_REASSEMBLY_EVENT_SDU_TOO_LONG => "SduTooLong",
            other => panic!("event {other}"),
        }
    }

    /// The outputs of the vectors for an action: a run of packets is one output for each packet.
    fn outputs(a: &aldis_map_reassembler_action_t, area: &[u8]) -> Vec<Value> {
        let r = OutRange::new(a.data.offset, a.data.len);
        match a.kind {
            ALDIS_MAP_REASSEMBLER_ACTION_EVENT => vec![json!({"reassembly_event": {
                "kind": event_name(a.event), "vc": a.vc_id, "map": a.map_id,
            }})],
            ALDIS_MAP_REASSEMBLER_ACTION_SDU => vec![json!({"deliver": {
                "kind": "Sdu", "vc": a.vc_id, "map": a.map_id,
                "data_hex": hex::encode(r.slice(area).unwrap()),
            }})],
            ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA => {
                let mut d = Map::new();
                d.insert("kind".to_owned(), json!("Sdu"));
                d.insert("vc".to_owned(), json!(a.vc_id));
                d.insert("map".to_owned(), json!(a.map_id));
                d.insert("octet_stream".to_owned(), json!(true));
                d.insert(
                    "data_hex".to_owned(),
                    json!(hex::encode(r.slice(area).unwrap())),
                );
                if a.after_gap == 1 {
                    d.insert("after_gap".to_owned(), json!(true));
                }
                vec![json!({"deliver": d})]
            }
            ALDIS_MAP_REASSEMBLER_ACTION_PACKETS => {
                let packets: Vec<&[u8]> = PacketRun::new(r, area).unwrap().collect();
                assert_eq!(packets.len(), a.count);
                packets
                    .into_iter()
                    .map(|p| {
                        json!({"deliver": {
                            "kind": "Packet", "vc": a.vc_id, "map": a.map_id,
                            "pvn": PacketRun::pvn(p).unwrap(),
                            "data_hex": hex::encode(p), "incomplete": false,
                        }})
                    })
                    .collect()
            }
            ALDIS_MAP_REASSEMBLER_ACTION_DISCARDED => vec![discarded(a.octets)],
            other => panic!("action kind {other}"),
        }
    }

    pub(super) fn run_file(name: &str) -> (usize, Vec<String>) {
        let file = load(name, "map-reassembler");
        let (mut run, mut failures) = (0, Vec::new());
        for scenario in &file.resolve().unwrap() {
            if scenario.scenario.superseded.is_some() {
                continue;
            }
            run += 1;
            let mut storage = Box::new(aldis_map_reassembler_t {
                words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_MAP_REASSEMBLER_WORDS],
            });
            let mut det = aldis_error_detail_t::default();
            let cfg = config(scenario.config);
            // SAFETY: every pointer is to a live local.
            let status = unsafe { aldis_map_reassembler_init(&raw mut *storage, &cfg, &mut det) };
            assert_eq!(status, ALDIS_OK, "{}", scenario.scenario.id);
            for (i, step) in scenario.steps.iter().enumerate() {
                let (key, v) = step.input.iter().next().unwrap();
                let data: Vec<u8>;
                let mut input = aldis_map_reassembler_input_t {
                    kind: 0,
                    data: null(),
                    data_len: 0,
                    vc_id: 0,
                    map_id: 0,
                    gap: 0,
                    config: cfg,
                };
                let mut counter = None;
                match key.as_str() {
                    "fdu" => {
                        data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
                        input.kind = ALDIS_MAP_REASSEMBLER_INPUT_FDU;
                        input.data = data.as_ptr();
                        input.data_len = data.len();
                        input.vc_id = byte(&v["vc"]);
                        input.map_id = v.get("map").map_or(0, byte);
                        input.gap =
                            u8::from(v.get("gap").and_then(Value::as_bool).unwrap_or(false));
                    }
                    // The current configuration is applied again (LNK-003).
                    "reconfigure" => input.kind = ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE,
                    "query" => counter = Some(v["counter"].as_str().unwrap()),
                    other => panic!("unknown input {other}"),
                }
                let got: Vec<Value> = if let Some(name) = counter {
                    let mut s = aldis_map_reassembler_status_t::default();
                    // SAFETY: every pointer is to a live local.
                    let status = unsafe {
                        aldis_map_reassembler_status(&raw mut *storage, &mut s, &mut det)
                    };
                    assert_eq!(status, ALDIS_OK);
                    let value = match name {
                        "GapDiscard" => s.gap_discards,
                        "SequenceViolation" => s.sequence_violations,
                        "UnknownMap" => s.unknown_maps,
                        "SduTooLong" => s.sdus_too_long,
                        other => panic!("unknown counter {other}"),
                    };
                    vec![json!({"counter": {"name": name, "value": value}})]
                } else {
                    let mut area = [0xEE_u8; AREA];
                    let mut actions = aldis_map_reassembler_actions_t::default();
                    // SAFETY: every pointer is to a live local.
                    let status = unsafe {
                        aldis_map_reassembler_step(
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
    } else if name.contains("channel_mux") {
        let (run, failures) = mux::run_file(name);
        (run, 0, failures)
    } else if name.contains("map_segmenter") {
        segment::run_file(name)
    } else if name.contains("map_reassembler") {
        let (run, failures) = reassemble::run_file(name);
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
