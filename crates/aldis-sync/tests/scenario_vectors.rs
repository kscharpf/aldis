//! Runs the engine scenarios of `test-vectors/sync/scenarios/` against `AsmSearch::step`
//! (SYNC-007…SYNC-016, TV-010…TV-012, TV-023). Every step's outputs and its state must match
//! exactly; an unknown input fails.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::{Error, ErrorDetail};
use aldis_sync::{
    AsmErrors, AsmSearch, SearchInput, SearchState, SyncConfig, SyncProtocol, max_actions,
};
use aldis_testkit::model::ResolvedScenario;
use aldis_testkit::{ScenarioFile, VectorFile, hex};
use serde_json::{Map, Value, json};

const FILES: [&str; 9] = [
    "sync/scenarios/search_offsets_tm.json",
    "sync/scenarios/search_offsets_aos.json",
    "sync/scenarios/search_offsets_uslp.json",
    "sync/scenarios/search_errors.json",
    "sync/scenarios/search_first_match.json",
    "sync/scenarios/search_chunks.json",
    "sync/scenarios/search_units.json",
    "sync/scenarios/search_bounds.json",
    "sync/scenarios/search_capacity.json",
];

/// The capacities every file but `search_capacity.json` needs (the README of the vectors).
const FRAME: usize = 64;
const CHUNK: usize = 128;
/// The instance of `search_capacity.json`.
const SMALL_CHUNK: usize = 16;

fn config(c: &Map<String, Value>) -> SyncConfig {
    let protocol = match c["protocol"].as_str().unwrap() {
        "tm" => SyncProtocol::Tm,
        "aos" => SyncProtocol::Aos,
        "uslp" => SyncProtocol::Uslp,
        other => panic!("protocol {other}"),
    };
    let tolerance = AsmErrors::new(u32::try_from(c["asm_errors"].as_u64().unwrap()).unwrap());
    SyncConfig::new(
        protocol,
        usize::try_from(c["frame_len"].as_u64().unwrap()).unwrap(),
        tolerance.unwrap(),
    )
    .unwrap()
}

fn error_output(e: &Error) -> Value {
    let mut o = Map::new();
    o.insert("error".into(), json!(e.kind().name()));
    if let Some(f) = e.field() {
        o.insert("field".into(), json!(f.name()));
    }
    if let ErrorDetail::Needed(n) = e.detail() {
        o.insert("needed".into(), json!(n));
    }
    Value::Object(o)
}

const fn state_name(s: SearchState) -> &'static str {
    match s {
        SearchState::Searching => "Searching",
        SearchState::Collecting => "Collecting",
    }
}

fn run_scenario<const F: usize, const C: usize, const A: usize>(
    scenario: &ResolvedScenario<'_>,
) -> Vec<String> {
    let mut engine = AsmSearch::<F, C, A>::new(config(scenario.config)).unwrap();
    let id = &scenario.scenario.id;
    let mut failures = Vec::new();
    for (i, step) in scenario.steps.iter().enumerate() {
        let mut area = vec![0xEE_u8; AsmSearch::<F, C, A>::MAX_OUT_LEN];
        let got: Vec<Value> = if let Some(chunk) = step.input.get("chunk") {
            let data = hex::decode(chunk["data_hex"].as_str().unwrap()).unwrap();
            let len = chunk.get("out_len").map_or(area.len(), |n| {
                usize::try_from(n.as_u64().unwrap()).unwrap()
            });
            match engine.step(SearchInput::Chunk(&data), &mut area[..len]) {
                Ok(actions) => actions
                    .iter()
                    .map(|a| {
                        json!({"unit": {
                            "data_hex": hex::encode(a.unit().slice(&area).unwrap()),
                            "errors": a.errors().get(),
                            "skipped_bits": a.skipped_bits(),
                        }})
                    })
                    .collect(),
                Err(e) => vec![error_output(&e)],
            }
        } else if step.input.contains_key("reset") {
            let actions = engine
                .step(SearchInput::Reset { config: None }, &mut area)
                .unwrap();
            assert!(actions.is_empty());
            Vec::new()
        } else if let Some(q) = step.input.get("query") {
            let name = q["counter"].as_str().unwrap();
            let s = engine.status();
            let value = match name {
                "units_returned" => s.units_returned(),
                "partial_octets" => u64::try_from(s.partial_octets()).unwrap(),
                "skipped_bits_total" => s.skipped_bits_total(),
                other => panic!("counter {other}"),
            };
            vec![json!({"counter": {"name": name, "value": value}})]
        } else {
            panic!("{id}: unknown input {:?}", step.input);
        };
        let want: Vec<Value> = step.expect.iter().cloned().map(Value::Object).collect();
        if got != want {
            failures.push(format!("{id} step {i}: expected {want:?}, got {got:?}"));
        }
        if let Some(state) = step
            .expect_state
            .as_deref()
            .filter(|s| *s != state_name(engine.state()))
        {
            failures.push(format!(
                "{id} step {i}: state {:?}, expected {state}",
                engine.state()
            ));
        }
    }
    failures
}

fn run_file(name: &str) -> (usize, Vec<String>) {
    let path = aldis_testkit::vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let Ok(VectorFile::EngineScenario(parsed)) = VectorFile::from_json(&text) else {
        panic!("{name}: not an engine scenario file");
    };
    let file: ScenarioFile = parsed;
    assert_eq!(file.engine, "asm-search");
    let resolved = file.resolve().unwrap();
    let mut failures = Vec::new();
    for scenario in &resolved {
        if scenario.scenario.superseded.is_some() {
            // A superseded case is kept for the record and not run (TV-021).
            continue;
        }
        failures.extend(match scenario.config.get("chunk_capacity") {
            None => run_scenario::<FRAME, CHUNK, { max_actions(CHUNK) }>(scenario),
            Some(n) if n.as_u64() == Some(16) => {
                run_scenario::<FRAME, SMALL_CHUNK, { max_actions(SMALL_CHUNK) }>(scenario)
            }
            Some(other) => panic!("no instance of chunk capacity {other}"),
        });
    }
    (resolved.len(), failures)
}

#[test]
fn the_asm_search_scenarios_match_exactly() {
    // req: SYNC-007, SYNC-008, SYNC-009, SYNC-010, SYNC-011, SYNC-012, SYNC-013, SYNC-014, SYNC-015, SYNC-016, SYNC-017, TV-012
    let mut failures = Vec::new();
    let mut run = 0;
    for name in FILES {
        let (n, f) = run_file(name);
        run += n;
        failures.extend(f);
    }
    assert!(run >= 100, "only {run} scenarios ran");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
