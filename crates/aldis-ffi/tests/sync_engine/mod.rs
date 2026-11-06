//! Runs the vectors of `test-vectors/sync/` through the C ABI of `aldis-sync`: the codec files
//! (`aldis_sync_attach`, `aldis_sync_randomize`, and the configuration checks of
//! `aldis_sync_search_init`) and the engine scenarios of `scenarios/` (`aldis_sync_search_*`)
//! (M5 WP-4; SYNC-001…SYNC-020, CABI-030…032, CABI-040, TV-010…TV-012, TV-023).
//!
//! The values of the C choices are the encodings of OI-83 (d): the frame protocol is an
//! `ALDIS_LINK_PROTOCOL_*` constant (a protocol that is none, `"none"` in the vectors, is a value
//! no constant has), and the randomizer sequence is its length in bits.
//!
//! The handle has the capacities of `capacities.toml`, not those of the vectors, and a runner
//! cannot lower them (they are compile-time constants, GEN-011). The vectors that state a
//! `frame_capacity` or `chunk_capacity` that the handle does not have, and expect the refusal that
//! the capacity causes, are therefore counted and skipped, and the Rust runner of `aldis-sync`
//! covers them (a scenario that states the handle's own capacity runs): the two `CapacityExceeded` cases of `config.json` and the scenarios of
//! `search_capacity.json`. The refusal of a chunk above the handle's capacity is checked by
//! `tests/sync.rs`.

#![expect(
    clippy::large_stack_arrays,
    reason = "test-only harness: the storage is the 64 KB a C caller declares (RS-018)"
)]

use std::num::NonZeroU16;

use aldis::core::{ErrorKind, FieldId};
use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::capacities::ALDIS_SYNC_MAX_CHUNK_OCTETS;
use aldis_ffi::error::{ALDIS_DETAIL_NEEDED, ALDIS_OK};
use aldis_ffi::link::{ALDIS_LINK_PROTOCOL_AOS, ALDIS_LINK_PROTOCOL_TM, ALDIS_LINK_PROTOCOL_USLP};
use aldis_ffi::sync::*;
use aldis_testkit::model::CodecCase;
use aldis_testkit::runner::{Outcome, run_codec};
use aldis_testkit::{VectorFile, hex, vectors_dir};
use serde_json::{Map, Value, json};

/// The output area of a step that gives none: the frames of the scenarios are at most 64 octets
/// and the chunks at most 128 (the README of the vectors).
const AREA: usize = 192;

fn num(c: &Map<String, Value>, key: &str) -> Option<u64> {
    c.get(key).map(|v| v.as_u64().unwrap())
}

fn usize_of(n: u64) -> usize {
    usize::try_from(n).unwrap()
}

/// The `ALDIS_LINK_PROTOCOL_*` value of a protocol name; `none` is a value that is no constant.
fn protocol(c: &Map<String, Value>) -> u32 {
    match c["protocol"].as_str().unwrap() {
        "tm" => ALDIS_LINK_PROTOCOL_TM,
        "aos" => ALDIS_LINK_PROTOCOL_AOS,
        "uslp" => ALDIS_LINK_PROTOCOL_USLP,
        "none" => u32::MAX,
        other => panic!("protocol {other}"),
    }
}

fn config(c: &Map<String, Value>) -> aldis_sync_search_config_t {
    aldis_sync_search_config_t {
        protocol: protocol(c),
        frame_len: usize_of(num(c, "frame_len").unwrap()),
        asm_errors: u32::try_from(num(c, "asm_errors").unwrap_or(0)).unwrap(),
    }
}

/// The error of a failed call: the kind, the field code, and the `needed` detail.
struct Failed {
    kind: ErrorKind,
    field: Option<NonZeroU16>,
    needed: Option<u64>,
}

fn call(f: impl FnOnce(*mut aldis_error_detail_t) -> u32) -> Result<(), Failed> {
    let mut d = aldis_error_detail_t::default();
    let status = f(&raw mut d);
    if status == ALDIS_OK {
        return Ok(());
    }
    assert_eq!(status, d.kind, "status and detail disagree");
    let field = NonZeroU16::new(d.field).map(|code| {
        FieldId::from_code(code.get())
            .unwrap_or_else(|| panic!("field code {code} is not catalogued"))
            .code()
    });
    Err(Failed {
        kind: ErrorKind::from_code(u16::try_from(d.kind).unwrap()).unwrap(),
        field,
        needed: (d.detail == ALDIS_DETAIL_NEEDED).then_some(d.needed),
    })
}

fn outcome(result: Result<Value, Failed>) -> Outcome {
    match result {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap_or_default()),
        Err(f) => Outcome::Err {
            kind: f.kind.name().to_owned(),
            field: f.field,
        },
    }
}

fn storage() -> Box<aldis_sync_search_t> {
    Box::new(aldis_sync_search_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_SYNC_SEARCH_WORDS],
    })
}

fn attach(case: &CodecCase) -> Outcome {
    let cfg = config(case.config.as_ref().unwrap());
    let frame = hex::decode(case.input_hex.as_deref().unwrap()).unwrap();
    let area = case
        .params
        .as_ref()
        .and_then(|p| num(p, "out_len"))
        .map_or(frame.len() + 4, usize_of);
    let mut out = vec![0xEE_u8; area];
    let mut len = 0_usize;
    let result = call(|d| {
        // SAFETY: every pointer is to a live local or vector, valid for the length given.
        unsafe {
            aldis_sync_attach(
                &raw const cfg,
                frame.as_ptr(),
                frame.len(),
                out.as_mut_ptr(),
                out.len(),
                &raw mut len,
                d,
            )
        }
    })
    .map(|()| json!({ "cadu_hex": hex::encode(&out[..len]) }));
    if let Err(f) = &result {
        assert!(
            out.iter().all(|&b| b == 0xEE),
            "nothing is written on error"
        );
        if let Some(needed) = case.expect.values.get("needed") {
            assert_eq!(f.needed, needed.as_u64());
        }
    }
    outcome(result)
}

fn randomize(case: &CodecCase) -> Outcome {
    let sequence =
        u32::try_from(num(case.config.as_ref().unwrap(), "randomizer_sequence").unwrap()).unwrap();
    let mut buffer = hex::decode(case.input_hex.as_deref().unwrap()).unwrap();
    let before = buffer.clone();
    let result = call(|d| {
        // SAFETY: `buffer` is a live vector, valid for its length.
        unsafe { aldis_sync_randomize(sequence, buffer.as_mut_ptr(), buffer.len(), d) }
    })
    .map(|()| json!({ "output_hex": hex::encode(&buffer) }));
    if result.is_err() {
        assert_eq!(buffer, before, "the buffer is not changed on error");
    }
    outcome(result)
}

/// The configuration cases: `aldis_sync_search_init` is the configuration check.
fn configuration(case: &CodecCase) -> Outcome {
    let cfg = config(case.config.as_ref().unwrap());
    let mut s = storage();
    let result = call(|d| {
        // SAFETY: `s` is a live storage object and `cfg` a live config.
        unsafe { aldis_sync_search_init(&raw mut *s, &raw const cfg, d) }
    })
    .map(|()| json!({}));
    if result.is_ok() {
        // SAFETY: `s` holds the engine that `init` placed.
        unsafe { aldis_sync_search_deinit(&raw mut *s) };
    }
    outcome(result)
}

fn handler(case: &CodecCase) -> Outcome {
    match case.operation.as_str() {
        "attach" => attach(case),
        "config" => configuration(case),
        "randomize" => randomize(case),
        _ => Outcome::Unsupported,
    }
}

/// Whether the case expects the refusal of a capacity that the handle does not have.
fn capacity_case(case: &CodecCase) -> bool {
    case.config
        .as_ref()
        .is_some_and(|c| c.contains_key("frame_capacity"))
        && case.expect.error.as_deref() == Some("CapacityExceeded")
}

/// Runs the codec file `name` (relative to `test-vectors/`) and returns the cases skipped.
pub(crate) fn run_codec_file(name: &str) -> usize {
    let path = vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::Codec(mut file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not a codec file");
    };
    let all = file.cases.len();
    file.cases.retain(|c| !capacity_case(c));
    let skipped = all - file.cases.len();
    run_codec(&file, &path, handler).assert_passed();
    skipped
}

fn error_output(f: &Failed) -> Value {
    let mut o = Map::new();
    o.insert("error".into(), json!(f.kind.name()));
    if let Some(code) = f.field {
        o.insert(
            "field".into(),
            json!(FieldId::from_code(code.get()).unwrap().name()),
        );
    }
    if let Some(n) = f.needed {
        o.insert("needed".into(), json!(n));
    }
    Value::Object(o)
}

fn state_name(state: u32) -> &'static str {
    match state {
        ALDIS_SYNC_SEARCH_STATE_SEARCHING => "Searching",
        ALDIS_SYNC_SEARCH_STATE_COLLECTING => "Collecting",
        other => panic!("state {other}"),
    }
}

fn status_of(h: &mut aldis_sync_search_t) -> aldis_sync_search_status_t {
    let mut status = aldis_sync_search_status_t::default();
    // SAFETY: `h` holds a live engine and `status` is a live local.
    let result = call(|d| unsafe { aldis_sync_search_status(h, &raw mut status, d) });
    assert!(result.is_ok());
    status
}

fn step(
    h: &mut aldis_sync_search_t,
    kind: u32,
    data: &[u8],
    area: &mut [u8],
) -> Result<aldis_sync_search_actions_t, Failed> {
    let input = aldis_sync_search_input_t {
        kind,
        data: data.as_ptr(),
        data_len: data.len(),
        config: aldis_sync_search_config_t::default(),
    };
    let mut actions = aldis_sync_search_actions_t {
        count: 0,
        items: [aldis_sync_search_action_t::default(); ALDIS_SYNC_SEARCH_MAX_ACTIONS],
    };
    // SAFETY: `h` holds a live engine; `input`, `actions`, and `area` are live locals, and the
    // input's `data` is valid for `data_len` octets.
    call(|d| unsafe {
        aldis_sync_search_step(
            h,
            &raw const input,
            area.as_mut_ptr(),
            area.len(),
            &raw mut actions,
            d,
        )
    })
    .map(|()| actions)
}

/// Runs the scenario file `name` and returns the scenarios skipped.
pub(crate) fn run_scenario_file(name: &str) -> usize {
    let path = vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    assert_eq!(file.engine, "asm-search");
    let resolved = file.resolve().unwrap();
    let (mut run, mut skipped) = (0, 0);
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            continue;
        }
        // The handle has the capacity of `capacities.toml`; a scenario that states another
        // chunk capacity tests that capacity and is run by the Rust runner (module comment).
        if num(scenario.config, "chunk_capacity")
            .is_some_and(|n| usize_of(n) != ALDIS_SYNC_MAX_CHUNK_OCTETS)
        {
            skipped += 1;
            continue;
        }
        run += 1;
        let id = &scenario.scenario.id;
        let cfg = config(scenario.config);
        let mut s = storage();
        // SAFETY: `s` is a live storage object and `cfg` a live config.
        let init = call(|d| unsafe { aldis_sync_search_init(&raw mut *s, &raw const cfg, d) });
        assert!(init.is_ok(), "{id}: init failed");
        for (i, st) in scenario.steps.iter().enumerate() {
            let got: Vec<Value> = if let Some(chunk) = st.input.get("chunk") {
                let data = hex::decode(chunk["data_hex"].as_str().unwrap()).unwrap();
                assert!(
                    data.len() <= 128,
                    "{id}: a chunk of the scenarios is at most 128 octets"
                );
                let len = chunk
                    .get("out_len")
                    .map_or(AREA, |n| usize_of(n.as_u64().unwrap()));
                let mut area = vec![0xEE_u8; len];
                match step(&mut s, ALDIS_SYNC_SEARCH_INPUT_CHUNK, &data, &mut area) {
                    Ok(actions) => actions.items[..actions.count]
                        .iter()
                        .map(|a| {
                            assert_eq!(a.kind, ALDIS_SYNC_SEARCH_ACTION_UNIT);
                            json!({"unit": {
                                "data_hex": hex::encode(&area[a.unit.offset..a.unit.offset + a.unit.len]),
                                "errors": a.errors,
                                "skipped_bits": a.skipped_bits,
                            }})
                        })
                        .collect(),
                    Err(f) => vec![error_output(&f)],
                }
            } else if st.input.contains_key("reset") {
                let mut area = [0_u8; 0];
                let actions = step(&mut s, ALDIS_SYNC_SEARCH_INPUT_RESET, &[], &mut area);
                assert_eq!(actions.ok().map(|a| a.count), Some(0), "{id}: reset");
                Vec::new()
            } else if let Some(q) = st.input.get("query") {
                let name = q["counter"].as_str().unwrap();
                let status = status_of(&mut s);
                let value = match name {
                    "units_returned" => status.units_returned,
                    "partial_octets" => u64::try_from(status.partial_octets).unwrap(),
                    "skipped_bits_total" => status.skipped_bits_total,
                    other => panic!("counter {other}"),
                };
                vec![json!({"counter": {"name": name, "value": value}})]
            } else {
                panic!("{id}: unknown input {:?}", st.input);
            };
            let want: Vec<Value> = st.expect.iter().cloned().map(Value::Object).collect();
            if got != want {
                failures.push(format!("{id} step {i}: expected {want:?}, got {got:?}"));
            }
            if let Some(state) = st.expect_state.as_deref() {
                let actual = state_name(status_of(&mut s).state);
                if actual != state {
                    failures.push(format!("{id} step {i}: state {actual}, expected {state}"));
                }
            }
        }
        // SAFETY: `s` holds the engine that `init` placed.
        unsafe { aldis_sync_search_deinit(&raw mut *s) };
    }
    assert!(
        failures.is_empty(),
        "{name}: {} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(run + skipped > 0, "{name}: no scenario");
    skipped
}
