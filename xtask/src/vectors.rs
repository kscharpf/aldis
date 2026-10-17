//! `xtask vectors validate` and `xtask vectors schema [--check]` (TV-001…003, TV-020…021).

use std::collections::BTreeMap;
use std::process::Command;

use aldis_testkit::{CodecCase, CodecFile, Provenance, ScenarioFile, Superseded, VectorFile, hex};
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

fn check_superseded(
    superseded: Option<&Superseded>,
    at: &str,
    by_links: &mut Vec<(String, String)>,
    problems: &mut Vec<String>,
) {
    let Some(s) = superseded else {
        return;
    };
    if s.reason.trim().is_empty() {
        problems.push(format!("{at}: superseded.reason is empty (TV-021)"));
    }
    if let Some(by) = &s.by {
        by_links.push((at.to_owned(), by.clone()));
    }
}

fn check_by_links(
    ids: &BTreeMap<String, (String, bool)>,
    by_links: &[(String, String)],
    problems: &mut Vec<String>,
) {
    for (at, by) in by_links {
        match ids.get(by) {
            None => problems.push(format!(
                "{at}: superseded.by {by:?} does not name an existing case (TV-021)"
            )),
            Some((target_at, true)) => problems.push(format!(
                "{at}: superseded.by {by:?} ({target_at}) is itself superseded (TV-021)"
            )),
            Some((_, false)) => {}
        }
    }
}

fn check_against_released(
    rel: &str,
    tag: &str,
    released: &VectorFile,
    current: &VectorFile,
    problems: &mut Vec<String>,
) {
    match (released, current) {
        (VectorFile::Codec(old_file), VectorFile::Codec(cur_file)) => {
            let cur_by_id: BTreeMap<&str, &CodecCase> =
                cur_file.cases.iter().map(|c| (c.id.as_str(), c)).collect();
            for old in &old_file.cases {
                let at = format!("{rel}#{}", old.id);
                let Some(&now) = cur_by_id.get(old.id.as_str()) else {
                    problems.push(format!("{at}: case released in {tag} was removed (TV-021)"));
                    continue;
                };
                let mut now_base = now.clone();
                now_base.superseded.clone_from(&old.superseded);
                if now_base != *old
                    || (old.superseded.is_some() && now.superseded != old.superseded)
                {
                    problems.push(format!(
                        "{at}: case released in {tag} was modified in place (TV-021)"
                    ));
                }
            }
        }
        (VectorFile::EngineScenario(old), VectorFile::EngineScenario(now)) => {
            let at = format!("{rel}#{}", old.id);
            let mut now_base = now.clone();
            now_base.superseded.clone_from(&old.superseded);
            if now_base != *old || (old.superseded.is_some() && now.superseded != old.superseded) {
                problems.push(format!(
                    "{at}: scenario released in {tag} was modified in place (TV-021)"
                ));
            }
        }
        _ => problems.push(format!(
            "{rel}: vector file kind changed since {tag} (TV-021)"
        )),
    }
}

fn release_tags() -> Vec<String> {
    let Ok(out) = Command::new("git")
        .args(["tag", "--list", "v*"])
        .current_dir(repo::root())
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn check_released_tags(
    rel: &str,
    tags: &[String],
    current: &VectorFile,
    problems: &mut Vec<String>,
) {
    for tag in tags {
        let Ok(out) = Command::new("git")
            .args(["show", &format!("{tag}:{rel}")])
            .current_dir(repo::root())
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if let Ok(released) = VectorFile::from_json(&text) {
            check_against_released(rel, tag, &released, current, problems);
        }
    }
}

fn check_codec_case(
    c: &CodecCase,
    spec: &Spec,
    at: &str,
    by_links: &mut Vec<(String, String)>,
    problems: &mut Vec<String>,
) {
    check_requirements(&c.requirements, spec, at, problems);
    check_superseded(c.superseded.as_ref(), at, by_links, problems);
    if let Some(h) = &c.input_hex
        && let Err(e) = hex::decode(h)
    {
        problems.push(format!("{at}: input_hex: {e}"));
    }
    match (c.expect.ok, c.expect.error.as_deref()) {
        (true, Some(_)) => {
            problems.push(format!("{at}: expect.ok is true but an error is named"));
        }
        (false, None) => {
            problems.push(format!(
                "{at}: expect.ok is false but no error kind is named"
            ));
        }
        (false, Some(kind)) if !spec.error_kinds.contains(kind) => {
            problems.push(format!(
                "{at}: error {kind:?} is not in the catalogue (03-api-model.md §9)"
            ));
        }
        _ => {}
    }
    for (k, v) in c
        .expect
        .values
        .iter()
        .chain(c.params.iter().flatten())
        .chain(c.config.iter().flatten())
    {
        check_hex(v, k, at, problems);
    }
}

/// Validates all vector files; returns the number of problems.
pub fn validate() -> Result<usize, String> {
    let spec = Spec::load()?;
    let tags = release_tags();
    let mut problems = Vec::new();
    let mut ids: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut by_links: Vec<(String, String)> = Vec::new();
    let mut files = 0;
    let mut cases = 0;

    for path in repo::files("test-vectors", &["json"]) {
        let rel = repo::rel(&path);
        if rel.starts_with(&format!("{SCHEMA_DIR}/")) {
            continue;
        }
        files += 1;
        let mut unique = |id: &str, at: &str, is_superseded: bool, problems: &mut Vec<String>| {
            if let Some((prev, _)) = ids.insert(id.to_owned(), (at.to_owned(), is_superseded)) {
                problems.push(format!("{at}: duplicate id {id:?} (also {prev})"));
            }
        };
        let parsed = VectorFile::from_json(&repo::read(&path)?);
        if let Ok(ref vf) = parsed {
            check_released_tags(&rel, &tags, vf, &mut problems);
        }
        match parsed {
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
                    unique(&c.id, &at, c.superseded.is_some(), &mut problems);
                    check_codec_case(c, &spec, &at, &mut by_links, &mut problems);
                }
            }
            Ok(VectorFile::EngineScenario(ScenarioFile {
                id,
                requirements,
                provenance,
                provenance_note,
                config,
                steps,
                superseded,
                ..
            })) => {
                cases += 1;
                let at = format!("{rel}#{id}");
                unique(&id, &at, superseded.is_some(), &mut problems);
                check_requirements(&requirements, &spec, &at, &mut problems);
                check_superseded(superseded.as_ref(), &at, &mut by_links, &mut problems);
                if needs_note(provenance) && provenance_note.is_none() {
                    problems.push(format!(
                        "{at}: provenance {provenance:?} requires provenance_note (TV-020)"
                    ));
                }
                if steps.windows(2).any(|w| w[1].at_us < w[0].at_us) {
                    problems.push(format!("{at}: step instants must not decrease (TV-010)"));
                }
                for (k, v) in &config {
                    check_hex(v, k, &at, &mut problems);
                }
                for step in &steps {
                    for (k, v) in step.input.iter().chain(step.expect.iter().flatten()) {
                        check_hex(v, k, &at, &mut problems);
                    }
                }
            }
        }
    }

    check_by_links(&ids, &by_links, &mut problems);

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

#[cfg(test)]
mod tests {
    use super::{check_against_released, check_by_links};
    use aldis_testkit::{Superseded, VectorFile};
    use std::collections::BTreeMap;

    #[test]
    fn superseded_links_and_released_immutability() {
        // req: TV-021
        let mut ids = BTreeMap::new();
        ids.insert("old-001".to_owned(), ("f.json#old-001".to_owned(), true));
        ids.insert("new-002".to_owned(), ("f.json#new-002".to_owned(), false));

        let mut problems = Vec::new();
        check_by_links(
            &ids,
            &[
                ("f.json#old-001".to_owned(), "new-002".to_owned()),
                ("f.json#old-001".to_owned(), "typo-002".to_owned()),
                ("f.json#old-001".to_owned(), "old-001".to_owned()),
            ],
            &mut problems,
        );
        assert_eq!(problems.len(), 2);

        let base_json = r#"{
            "kind": "codec",
            "standard": "132.0-B-3",
            "provenance": "blue-book-example",
            "cases": [{
                "id": "c-001",
                "requirements": ["CMN-001"],
                "operation": "crc16",
                "input_hex": "3132",
                "expect": { "ok": true }
            }]
        }"#;
        let released = VectorFile::from_json(base_json).unwrap();
        let VectorFile::Codec(mut superseded_ok) = released.clone() else {
            panic!("expected codec");
        };
        superseded_ok.cases[0].superseded = Some(Superseded {
            reason: "expanded".to_owned(),
            by: Some("c-002".to_owned()),
        });
        let mut p_ok = Vec::new();
        check_against_released(
            "f.json",
            "v0.1.0-m0",
            &released,
            &VectorFile::Codec(superseded_ok.clone()),
            &mut p_ok,
        );
        assert!(p_ok.is_empty(), "{p_ok:?}");

        let mut edited = superseded_ok;
        edited.cases[0].requirements.push("CMN-002".to_owned());
        let mut p_edit = Vec::new();
        check_against_released(
            "f.json",
            "v0.1.0-m0",
            &released,
            &VectorFile::Codec(edited),
            &mut p_edit,
        );
        assert_eq!(p_edit.len(), 1);
    }
}
