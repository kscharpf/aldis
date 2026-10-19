//! Requirement tables in `docs/` (00-introduction.md §5) and `xtask spec-lint`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::md::{scan_ids, tables};
use crate::repo;

/// One requirement row.
#[derive(Debug, Clone)]
pub struct Requirement {
    pub id: String,
    pub area: String,
    pub number: u16,
    pub doc: String,
    pub line: usize,
    pub text: String,
    pub source: String,
    pub verif: String,
}

/// The parsed specification.
pub struct Spec {
    /// Area code → document paths relative to `docs/` (from 00-introduction.md §5.2).
    pub areas: BTreeMap<String, Vec<String>>,
    pub requirements: BTreeMap<String, Requirement>,
    /// Error kind names from the catalogue (03-api-model.md §9).
    pub error_kinds: BTreeSet<String>,
    /// Problems found while loading (duplicates, malformed rows, …).
    pub problems: Vec<String>,
}

const REQ_HEADER: [&str; 4] = ["ID", "Requirement", "Source", "Verif."];

fn strip_code(s: &str) -> String {
    s.trim().trim_matches('`').to_owned()
}

/// All Markdown link targets in a cell, without anchors.
fn link_targets(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else { break };
        out.push(
            after[..end]
                .split('#')
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
        rest = &after[end..];
    }
    out
}

impl Spec {
    /// Loads areas, requirements, and error kinds.
    pub fn load() -> Result<Spec, String> {
        let docs = repo::root().join("docs");
        let mut problems = Vec::new();

        let intro = repo::read(&docs.join("00-introduction.md"))?;
        let mut areas = BTreeMap::new();
        for t in tables(&intro) {
            if t.header == ["Area", "Document"] {
                for (_, row) in t.rows {
                    if let (Some(area), Some(cell)) = (row.first(), row.get(1)) {
                        areas.insert(area.clone(), link_targets(cell));
                    }
                }
            }
        }
        if areas.is_empty() {
            return Err("docs/00-introduction.md: no Area/Document table found".to_owned());
        }
        let area_set: BTreeSet<String> = areas.keys().cloned().collect();

        let mut requirements: BTreeMap<String, Requirement> = BTreeMap::new();
        for path in repo::files("docs", &["md"]) {
            let text = repo::read(&path)?;
            let doc = repo::rel(&path).trim_start_matches("docs/").to_owned();
            for t in tables(&text) {
                if t.header != REQ_HEADER {
                    continue;
                }
                for (line, row) in t.rows {
                    let at = format!("docs/{doc}:{line}");
                    let [id, req, source, verif] = row.as_slice() else {
                        problems.push(format!(
                            "{at}: requirement row has {} cells, expected 4",
                            row.len()
                        ));
                        continue;
                    };
                    let refs = scan_ids(id, &area_set);
                    let Some(r) = refs.first().filter(|r| r.id() == *id && refs.len() == 1) else {
                        problems.push(format!(
                            "{at}: {id:?} is not a valid requirement ID of a known area"
                        ));
                        continue;
                    };
                    if let Some(prev) = requirements.get(id) {
                        problems.push(format!(
                            "{at}: duplicate {id} (first at docs/{}:{})",
                            prev.doc, prev.line
                        ));
                        continue;
                    }
                    requirements.insert(
                        id.clone(),
                        Requirement {
                            id: id.clone(),
                            area: r.area.clone(),
                            number: r.number,
                            doc: doc.clone(),
                            line,
                            text: req.clone(),
                            source: source.clone(),
                            verif: verif.clone(),
                        },
                    );
                }
            }
        }

        let api = repo::read(&docs.join("03-api-model.md"))?;
        let mut error_kinds = BTreeSet::new();
        for t in tables(&api) {
            if t.header == ["Code", "Kind", "Meaning"] {
                for (_, row) in t.rows {
                    if let Some(kind) = row.get(1) {
                        error_kinds.insert(strip_code(kind));
                    }
                }
            }
        }
        if error_kinds.is_empty() {
            problems.push(
                "docs/03-api-model.md: error catalogue (Code/Kind/Meaning) not found".to_owned(),
            );
        }

        Ok(Spec {
            areas,
            requirements,
            error_kinds,
            problems,
        })
    }

    /// Known area codes.
    pub fn area_set(&self) -> BTreeSet<String> {
        self.areas.keys().cloned().collect()
    }

    /// Expands an ID or range to the defined requirement IDs it covers.
    pub fn expand(&self, area: &str, from: u16, to: Option<u16>) -> Vec<String> {
        let to = to.unwrap_or(from);
        self.requirements
            .values()
            .filter(|r| r.area == area && (from..=to).contains(&r.number))
            .map(|r| r.id.clone())
            .collect()
    }
}

/// Text files scanned for references to requirement IDs.
pub fn referencing_files() -> Vec<std::path::PathBuf> {
    let mut files = repo::files(
        ".",
        &["md", "rs", "json", "jsonl", "yml", "yaml", "toml", "py"],
    );
    files.retain(|p| !repo::rel(p).starts_with("test-vectors/schema/"));
    files
}

fn check_verif(cell: &str) -> bool {
    cell == "—"
        || cell
            .split('/')
            .all(|t| matches!(t, "T" | "V" | "F" | "A" | "I"))
}

/// `xtask spec-lint`: structure of requirement tables and validity of every ID reference.
pub fn lint() -> Result<usize, String> {
    let spec = Spec::load()?;
    let mut problems = spec.problems.clone();
    let areas = spec.area_set();

    for r in spec.requirements.values() {
        let at = format!("docs/{}:{}", r.doc, r.line);
        let docs = spec.areas.get(&r.area).cloned().unwrap_or_default();
        if !docs.contains(&r.doc) {
            problems.push(format!(
                "{at}: {} is defined outside its area document(s) ({})",
                r.id,
                docs.join(", ")
            ));
        }
        if r.text.is_empty() || r.source.is_empty() {
            problems.push(format!(
                "{at}: {} has an empty Requirement or Source cell",
                r.id
            ));
        }
        if !check_verif(&r.verif) {
            problems.push(format!(
                "{at}: {} has invalid Verif. {:?} (use T/V/F/A/I joined by '/', or —)",
                r.id, r.verif
            ));
        }
    }

    for path in referencing_files() {
        let text = repo::read(&path)?;
        for id in scan_ids(&text, &areas) {
            if !spec.requirements.contains_key(&id.id()) {
                problems.push(format!(
                    "{}:{}: reference to undefined requirement {}",
                    repo::rel(&path),
                    id.line,
                    id.id()
                ));
            }
        }
    }

    let out_dir = repo::root().join("target/spec");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let json: Vec<_> = spec
        .requirements
        .values()
        .map(|r| {
            json!({ "id": r.id, "doc": format!("docs/{}", r.doc), "line": r.line,
                         "requirement": r.text, "source": r.source, "verif": r.verif })
        })
        .collect();
    let body = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(out_dir.join("requirements.json"), body + "\n").map_err(|e| e.to_string())?;

    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "spec-lint: {} requirements in {} areas, {} problem(s)",
        spec.requirements.len(),
        areas.len(),
        problems.len()
    );
    Ok(problems.len())
}
