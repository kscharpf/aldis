//! `xtask vectors validate` and `xtask vectors schema [--check]` (TV-001…003, TV-020…021).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use aldis_testkit::{CodecCase, CodecFile, Provenance, ScenarioFile, Superseded, VectorFile, hex};
use serde_json::{Map, Value};

use crate::md::scan_ids;
use crate::repo;
use crate::spec::Spec;

const SCHEMA_DIR: &str = "test-vectors/schema";

const fn needs_note(p: Provenance) -> bool {
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
    match s.by.as_deref().map(str::trim) {
        Some(by) if !by.is_empty() => by_links.push((at.to_owned(), by.to_owned())),
        _ => problems.push(format!("{at}: superseded.by is required (TV-021)")),
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

fn check_superseded_transition(
    at: &str,
    tag: &str,
    old_obj: &Map<String, Value>,
    now_obj: &Map<String, Value>,
    problems: &mut Vec<String>,
) {
    let mut old_base = old_obj.clone();
    let mut now_base = now_obj.clone();
    let old_sup = old_base.remove("superseded");
    let now_sup = now_base.remove("superseded");
    if old_base != now_base || (old_sup.is_some() && old_sup != now_sup) {
        problems.push(format!(
            "{at}: released vector in {tag} was modified in place (TV-021)"
        ));
    }
    if old_sup.is_none()
        && let Some(sup) = now_sup
    {
        let has_by = sup
            .get("by")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        let has_reason = sup
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_by || !has_reason {
            problems.push(format!(
                "{at}: vector released in {tag} was superseded without reason and by (TV-021)"
            ));
        }
    }
}

fn check_against_released(
    rel: &str,
    tag: &str,
    released: &Value,
    current: &Value,
    problems: &mut Vec<String>,
) {
    let (Some(old_top), Some(cur_top)) = (released.as_object(), current.as_object()) else {
        problems.push(format!(
            "{rel}: non-object JSON in {tag} comparison (TV-021)"
        ));
        return;
    };
    if old_top.get("kind").and_then(Value::as_str) == Some("codec") {
        let mut old_meta = old_top.clone();
        let mut cur_meta = cur_top.clone();
        let old_cases = old_meta.remove("cases");
        let cur_cases = cur_meta.remove("cases");
        if old_meta != cur_meta {
            problems.push(format!(
                "{rel}: file-level metadata changed since {tag} (TV-021)"
            ));
        }
        let cur_by_id: BTreeMap<&str, &Map<String, Value>> = cur_cases
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter_map(|c| Some((c.get("id")?.as_str()?, c)))
            .collect();
        for old_case in old_cases
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(old_obj) = old_case.as_object() else {
                continue;
            };
            let id = old_obj.get("id").and_then(Value::as_str).unwrap_or("?");
            let at = format!("{rel}#{id}");
            let Some(&now_obj) = cur_by_id.get(id) else {
                problems.push(format!("{at}: case released in {tag} was removed (TV-021)"));
                continue;
            };
            check_superseded_transition(&at, tag, old_obj, now_obj, problems);
        }
    } else {
        let id = old_top.get("id").and_then(Value::as_str).unwrap_or("?");
        let at = format!("{rel}#{id}");
        check_superseded_transition(&at, tag, old_top, cur_top, problems);
    }
}

fn release_tags() -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(["tag", "--list", "v*"])
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("git tag --list: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git tag --list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let tags: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if tags.is_empty() {
        return Err(
            "no release tags (`v*`) found; fetch tags before running `vectors validate` (TV-021)"
                .to_owned(),
        );
    }
    Ok(tags)
}

fn tag_blobs(tag: &str) -> Result<BTreeMap<String, Value>, String> {
    let ls = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", tag, "test-vectors"])
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("git ls-tree {tag}: {e}"))?;
    if !ls.status.success() {
        return Err(format!(
            "git ls-tree {tag} failed: {}",
            String::from_utf8_lossy(&ls.stderr)
        ));
    }
    let rels: Vec<String> = String::from_utf8_lossy(&ls.stdout)
        .lines()
        .map(str::trim)
        .filter(|p| {
            std::path::Path::new(p)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                && !p.starts_with(&format!("{SCHEMA_DIR}/"))
        })
        .map(str::to_owned)
        .collect();
    if rels.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(repo::root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git cat-file: {e}"))?;
    {
        let stdin = child.stdin.as_mut().ok_or("git cat-file: no stdin")?;
        for rel in &rels {
            writeln!(stdin, "{tag}:{rel}").map_err(|e| format!("git cat-file stdin: {e}"))?;
        }
    }
    drop(child.stdin.take());

    let stdout = child.stdout.take().ok_or("git cat-file: no stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut out = BTreeMap::new();
    for rel in rels {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|e| format!("git cat-file {tag}:{rel}: {e}"))?;
        let size: usize = header
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("git cat-file {tag}:{rel}: unexpected header {header:?}"))?;
        let mut buf = vec![0_u8; size];
        reader
            .read_exact(&mut buf)
            .map_err(|e| format!("git cat-file {tag}:{rel}: {e}"))?;
        let mut nl = [0_u8; 1];
        reader
            .read_exact(&mut nl)
            .map_err(|e| format!("git cat-file {tag}:{rel}: {e}"))?;
        let val: Value = serde_json::from_slice(&buf)
            .map_err(|e| format!("{tag}:{rel}: invalid JSON: {e} (TV-021)"))?;
        out.insert(rel, val);
    }
    let status = child.wait().map_err(|e| format!("git cat-file: {e}"))?;
    if !status.success() {
        return Err(format!("git cat-file exited with {status}"));
    }
    Ok(out)
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
    let tags = release_tags()?;
    let mut problems = Vec::new();
    let mut ids: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut by_links: Vec<(String, String)> = Vec::new();
    let mut current_raw: BTreeMap<String, Value> = BTreeMap::new();
    let mut files = 0;
    let mut cases = 0;

    for path in repo::files("test-vectors", &["json"]) {
        let rel = repo::rel(&path);
        if rel.starts_with(&format!("{SCHEMA_DIR}/")) {
            continue;
        }
        files += 1;
        let text = repo::read(&path)?;
        if let Ok(raw) = serde_json::from_str::<Value>(&text) {
            current_raw.insert(rel.clone(), raw);
        }
        let mut unique = |id: &str, at: &str, is_superseded: bool, problems: &mut Vec<String>| {
            if let Some((prev, _)) = ids.insert(id.to_owned(), (at.to_owned(), is_superseded)) {
                problems.push(format!("{at}: duplicate id {id:?} (also {prev})"));
            }
        };
        match VectorFile::from_json(&text) {
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

    for tag in &tags {
        for (rel, released) in tag_blobs(tag)? {
            if let Some(current) = current_raw.get(&rel) {
                check_against_released(&rel, tag, &released, current, &mut problems);
            } else {
                problems.push(format!(
                    "{rel}: vector file released in {tag} was removed or renamed (TV-021)"
                ));
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
    use super::{check_against_released, check_by_links, check_superseded};
    use aldis_testkit::Superseded;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn superseded_links_and_released_immutability() {
        // req: TV-021
        let mut ids = BTreeMap::new();
        ids.insert("old-001".to_owned(), ("f.json#old-001".to_owned(), true));
        ids.insert("new-002".to_owned(), ("f.json#new-002".to_owned(), false));

        let mut by_links = Vec::new();
        let mut problems = Vec::new();
        check_superseded(
            Some(&Superseded {
                reason: "r".to_owned(),
                by: None,
            }),
            "f.json#old-001",
            &mut by_links,
            &mut problems,
        );
        assert_eq!(problems.len(), 1);

        problems.clear();
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

        let released = json!({
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
        });
        let mut superseded_ok = released.clone();
        superseded_ok["cases"][0]["superseded"] = json!({ "reason": "expanded", "by": "c-002" });
        let mut p_ok = Vec::new();
        check_against_released("f.json", "v0.1.0-m0", &released, &superseded_ok, &mut p_ok);
        assert!(p_ok.is_empty(), "{p_ok:?}");

        let mut edited = superseded_ok.clone();
        edited["cases"][0]["requirements"] = json!(["CMN-001", "CMN-002"]);
        let mut p_edit = Vec::new();
        check_against_released("f.json", "v0.1.0-m0", &released, &edited, &mut p_edit);
        assert_eq!(p_edit.len(), 1);

        let mut no_by = released.clone();
        no_by["cases"][0]["superseded"] = json!({ "reason": "retired" });
        let mut p_no_by = Vec::new();
        check_against_released("f.json", "v0.1.0-m0", &released, &no_by, &mut p_no_by);
        assert_eq!(p_no_by.len(), 1);

        let mut meta_edit = superseded_ok;
        meta_edit["standard"] = json!("bogus");
        let mut p_meta = Vec::new();
        check_against_released("f.json", "v0.1.0-m0", &released, &meta_edit, &mut p_meta);
        assert_eq!(p_meta.len(), 1);
    }
}
