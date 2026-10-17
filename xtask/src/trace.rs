//! `xtask trace`: requirement → verification artefact matrix (VER-020).
//!
//! Artefacts are collected from `// req: <IDs>` annotations in Rust sources, the
//! `requirements` fields of test vectors, and `docs/verification/analysis-records.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use aldis_testkit::VectorFile;
use serde_json::json;

use crate::md::{scan_ids, tables};
use crate::repo;
use crate::spec::Spec;

fn add(
    map: &mut BTreeMap<String, BTreeSet<String>>,
    spec: &Spec,
    ids: &[crate::md::IdRef],
    artefact: &str,
) {
    for r in ids {
        for id in spec.expand(&r.area, r.number, r.range_end) {
            map.entry(id).or_default().insert(artefact.to_owned());
        }
    }
}

/// Collects artefacts for every requirement.
fn artefacts(spec: &Spec) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let areas = spec.area_set();
    let mut map = BTreeMap::new();

    for dir in ["crates", "xtask", "fuzz", "exerciser"] {
        for path in repo::files(dir, &["rs"]) {
            let text = repo::read(&path)?;
            for (n, line) in text.lines().enumerate() {
                if let Some((_, ids)) = line.split_once("// req:") {
                    let refs = scan_ids(ids, &areas);
                    add(
                        &mut map,
                        spec,
                        &refs,
                        &format!("test {}:{}", repo::rel(&path), n + 1),
                    );
                }
            }
        }
    }

    for path in repo::files("test-vectors", &["json"]) {
        if repo::rel(&path).starts_with("test-vectors/schema/") {
            continue;
        }
        let rel = repo::rel(&path);
        match VectorFile::from_json(&repo::read(&path)?) {
            Ok(VectorFile::Codec(f)) => {
                for c in f.cases.iter().filter(|c| c.superseded.is_none()) {
                    let refs = scan_ids(&c.requirements.join(" "), &areas);
                    add(&mut map, spec, &refs, &format!("vector {rel}#{}", c.id));
                }
            }
            Ok(VectorFile::EngineScenario(s)) => {
                let refs = scan_ids(&s.requirements.join(" "), &areas);
                add(&mut map, spec, &refs, &format!("vector {rel}#{}", s.id));
            }
            Err(e) => return Err(format!("{rel}: {e} (run `cargo xtask vectors validate`)")),
        }
    }

    let records = repo::root().join("docs/verification/analysis-records.md");
    if records.exists() {
        for t in tables(&repo::read(&records)?) {
            if t.header.first().map(String::as_str) == Some("ID") && t.header.len() == 3 {
                for (_, row) in t.rows {
                    if let [ids, method, artefact] = row.as_slice() {
                        let refs = scan_ids(ids, &areas);
                        add(&mut map, spec, &refs, &format!("{method}: {artefact}"));
                    }
                }
            }
        }
    }
    Ok(map)
}

/// Requirement IDs in scope of a milestone: the exit-criteria table of `docs/milestones/<scope>-*.md`.
fn scope_ids(spec: &Spec, scope: &str) -> Result<BTreeSet<String>, String> {
    let file = repo::files("docs/milestones", &["md"])
        .into_iter()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{scope}-")))
        })
        .ok_or_else(|| format!("no docs/milestones/{scope}-*.md"))?;
    let areas = spec.area_set();
    let mut ids = BTreeSet::new();
    for t in tables(&repo::read(&file)?) {
        if t.header == ["Area", "Criteria"] {
            for (_, row) in t.rows {
                for r in scan_ids(row.get(1).map_or("", String::as_str), &areas) {
                    ids.extend(spec.expand(&r.area, r.number, r.range_end));
                }
            }
        }
    }
    if ids.is_empty() {
        return Err(format!("{}: no Area/Criteria exit table", repo::rel(&file)));
    }
    Ok(ids)
}

/// Runs the trace. Returns the number of in-scope requirements without artefacts when
/// `strict`, otherwise 0.
pub fn run(scope: Option<&str>, strict: bool) -> Result<usize, String> {
    let spec = Spec::load()?;
    let arts = artefacts(&spec)?;
    let in_scope: BTreeSet<String> = match scope {
        Some(s) => scope_ids(&spec, s)?,
        None => spec.requirements.keys().cloned().collect(),
    };

    let mut md = format!(
        "# Trace matrix{}\n\n| ID | Verif. | Status | Artefacts |\n|---|---|---|---|\n",
        scope.map(|s| format!(" — scope {s}")).unwrap_or_default()
    );
    let mut rows = Vec::new();
    let mut untraced = Vec::new();
    for id in &in_scope {
        let r = &spec.requirements[id];
        let a = arts.get(id).cloned().unwrap_or_default();
        let status = if a.is_empty() { "untraced" } else { "traced" };
        if a.is_empty() {
            untraced.push(id.clone());
        }
        let list = a
            .iter()
            .map(|x| format!("`{x}`"))
            .collect::<Vec<_>>()
            .join("<br>");
        let _ = writeln!(md, "| {id} | {} | {status} | {list} |", r.verif);
        rows.push(json!({ "id": id, "verif": r.verif, "status": status, "artefacts": a }));
    }

    let out = repo::root().join("target/spec");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let name = scope.map_or_else(|| "trace".to_owned(), |s| format!("trace-{s}"));
    std::fs::write(out.join(format!("{name}.md")), md).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{name}.json")), body + "\n").map_err(|e| e.to_string())?;

    let traced = in_scope.len() - untraced.len();
    println!(
        "trace{}: {traced}/{} requirements traced; report: target/spec/{name}.md",
        scope.map(|s| format!(" --scope {s}")).unwrap_or_default(),
        in_scope.len()
    );
    if !untraced.is_empty() {
        println!("untraced: {}", untraced.join(", "));
    }
    Ok(if strict { untraced.len() } else { 0 })
}
