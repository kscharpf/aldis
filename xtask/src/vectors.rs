//! `xtask vectors validate` and `xtask vectors schema [--check]` (TV-001…003, TV-020…021).

use std::collections::BTreeMap;

use aldis_testkit::{CodecFile, Provenance, ScenarioFile, VectorFile, hex};
use serde_json::Value;

use crate::md::scan_ids;
use crate::repo;
use crate::spec::Spec;

const SCHEMA_DIR: &str = "test-vectors/schema";

fn needs_note(p: Provenance) -> bool {
    matches!(p, Provenance::HandDerived | Provenance::CrossChecked)
}

/// Checks every string value under a key ending in `_hex` (also inside arrays).
fn check_hex(value: &Value, key: &str, at: &str, problems: &mut Vec<String>) {
    match value {
        Value::String(s) if key.ends_with("_hex") => {
            if let Err(e) = hex::decode(s) {
                problems.push(format!("{at}: {key}: {e}"));
            }
        }
        Value::Array(items) => items.iter().for_each(|v| check_hex(v, key, at, problems)),
        Value::Object(map) => map.iter().for_each(|(k, v)| check_hex(v, k, at, problems)),
        _ => {}
    }
}

fn check_requirements(ids: &[String], spec: &Spec, at: &str, problems: &mut Vec<String>) {
    if ids.is_empty() {
        problems.push(format!("{at}: lists no requirements (TV-003)"));
    }
    let areas = spec.area_set();
    for id in ids {
        let refs = scan_ids(id, &areas);
        if refs.len() != 1 || refs[0].id() != *id || !spec.requirements.contains_key(id) {
            problems.push(format!("{at}: unknown requirement {id:?}"));
        }
    }
}

/// Validates all vector files; returns the number of problems.
pub fn validate() -> Result<usize, String> {
    let spec = Spec::load()?;
    let mut problems = Vec::new();
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut files = 0;
    let mut cases = 0;

    for path in repo::files("test-vectors", &["json"]) {
        let rel = repo::rel(&path);
        if rel.starts_with(&format!("{SCHEMA_DIR}/")) {
            continue;
        }
        files += 1;
        let mut unique = |id: &str, at: &str, problems: &mut Vec<String>| {
            if let Some(prev) = ids.insert(id.to_owned(), at.to_owned()) {
                problems.push(format!("{at}: duplicate id {id:?} (also {prev})"));
            }
        };
        match VectorFile::from_json(&repo::read(&path)?) {
            Err(e) => problems.push(format!("{rel}: {e}")),
            Ok(VectorFile::Codec(CodecFile {
                provenance,
                provenance_note,
                cases: list,
                ..
            })) => {
                if needs_note(provenance) && provenance_note.is_none() {
                    problems.push(format!(
                        "{rel}: provenance {provenance:?} requires provenance_note (TV-020)"
                    ));
                }
                for c in &list {
                    cases += 1;
                    let at = format!("{rel}#{}", c.id);
                    unique(&c.id, &at, &mut problems);
                    check_requirements(&c.requirements, &spec, &at, &mut problems);
                    if let Some(h) = &c.input_hex
                        && let Err(e) = hex::decode(h)
                    {
                        problems.push(format!("{at}: input_hex: {e}"));
                    }
                    match (c.expect.ok, c.expect.error.as_deref()) {
                        (true, Some(_)) => {
                            problems.push(format!("{at}: expect.ok is true but an error is named"));
                        }
                        (false, None) => problems.push(format!(
                            "{at}: expect.ok is false but no error kind is named"
                        )),
                        (false, Some(kind)) if !spec.error_kinds.contains(kind) => {
                            problems.push(format!(
                                "{at}: error {kind:?} is not in the catalogue (03-api-model.md §9)"
                            ));
                        }
                        _ => {}
                    }
                    for (k, v) in c.expect.values.iter().chain(c.params.iter().flatten()) {
                        check_hex(v, k, &at, &mut problems);
                    }
                }
            }
            Ok(VectorFile::EngineScenario(ScenarioFile {
                id,
                requirements,
                provenance,
                provenance_note,
                steps,
                ..
            })) => {
                cases += 1;
                let at = format!("{rel}#{id}");
                unique(&id, &at, &mut problems);
                check_requirements(&requirements, &spec, &at, &mut problems);
                if needs_note(provenance) && provenance_note.is_none() {
                    problems.push(format!(
                        "{at}: provenance {provenance:?} requires provenance_note (TV-020)"
                    ));
                }
                if steps.windows(2).any(|w| w[1].at_us < w[0].at_us) {
                    problems.push(format!("{at}: step instants must not decrease (TV-010)"));
                }
            }
        }
    }

    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "vectors validate: {files} file(s), {cases} case(s), {} problem(s)",
        problems.len()
    );
    Ok(problems.len())
}

/// Generates the JSON Schemas from the testkit model, or checks the committed ones.
pub fn schema(check: bool) -> Result<usize, String> {
    let dir = repo::root().join(SCHEMA_DIR);
    let generated = [
        (
            "codec.schema.json",
            aldis_testkit::schemars::schema_for!(CodecFile),
        ),
        (
            "engine-scenario.schema.json",
            aldis_testkit::schemars::schema_for!(ScenarioFile),
        ),
    ];
    let mut stale = 0;
    for (name, schema) in generated {
        let body = serde_json::to_string_pretty(&schema).map_err(|e| e.to_string())? + "\n";
        let path = dir.join(name);
        if check {
            if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
                println!(
                    "error: {SCHEMA_DIR}/{name} is out of date; run `cargo xtask vectors schema`"
                );
                stale += 1;
            }
        } else {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, body).map_err(|e| e.to_string())?;
            println!("wrote {SCHEMA_DIR}/{name}");
        }
    }
    if check && stale == 0 {
        println!("vectors schema: up to date");
    }
    Ok(stale)
}
