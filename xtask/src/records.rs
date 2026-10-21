//! Analysis and inspection records (method A and I, VER-020): one Markdown file per record in
//! [`DIR`], named after the first requirement ID it covers, so that changing one requirement's
//! record touches one small file and parallel pull requests do not conflict. There is no
//! index: `cargo xtask trace` reads every file in the directory.
//!
//! Each file starts with a title and a key-value table of exactly three rows, in this order:
//!
//! ```markdown
//! # Record: VER-024
//!
//! | | |
//! |---|---|
//! | Requirements | VER-024 |
//! | Method | A |
//! | Check | Verify check `coverage`: `cargo xtask coverage` |
//!
//! Free text: what the check or review establishes, and when it was last done.
//! ```
//!
//! `Method` is `A`, `I`, or `A, I`. The format is described for contributors in
//! `docs/verification/analysis-records.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::md::{scan_ids, tables};
use crate::repo;
use crate::spec::Spec;

/// The directory of the record files, relative to the repository root.
pub const DIR: &str = "docs/verification/records";

/// The keys of a record's header table, in order.
const KEYS: [&str; 3] = ["Requirements", "Method", "Check"];

/// One record file.
#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    /// Path relative to the repository root.
    pub path: String,
    /// The requirement IDs it covers, ranges expanded to the defined requirements.
    pub ids: Vec<String>,
    /// `A`, `I`, or `A, I`.
    pub method: String,
    /// The check or review, in a few words.
    pub check: String,
}

impl Record {
    /// The artefact label the trace matrix shows for this record.
    pub fn label(&self) -> String {
        format!("{}: {} ({})", self.method, self.check, self.path)
    }
}

/// Parses one record. `stem` is the file name without `.md`; it must be the first ID.
fn parse(path: &str, stem: &str, text: &str, spec: &Spec) -> Result<Record, String> {
    let table = tables(text)
        .into_iter()
        .next()
        .ok_or_else(|| format!("{path}: no header table (see {DIR} in docs)"))?;
    let rows: Vec<&Vec<String>> = table.rows.iter().map(|(_, r)| r).collect();
    let keys: Vec<&str> = rows
        .iter()
        .map(|r| r.first().map_or("", String::as_str))
        .collect();
    if keys != KEYS || rows.iter().any(|r| r.len() != 2) {
        return Err(format!(
            "{path}: the first table must have the rows {} with one value each, found {keys:?}",
            KEYS.join(", ")
        ));
    }
    let value = |i: usize| {
        rows.get(i)
            .and_then(|r| r.get(1))
            .map_or("", String::as_str)
    };
    let (ids_cell, method, check) = (value(0), value(1), value(2));
    if check.is_empty() {
        return Err(format!("{path}: `Check` is empty"));
    }
    if !matches!(method, "A" | "I" | "A, I") {
        return Err(format!(
            "{path}: `Method` must be `A`, `I`, or `A, I`, found `{method}`"
        ));
    }
    let refs = scan_ids(ids_cell, &spec.area_set());
    let first = refs
        .first()
        .ok_or_else(|| format!("{path}: `Requirements` names no requirement ID"))?;
    if first.id() != stem {
        return Err(format!(
            "{path}: the file must be named after its first requirement, {}.md",
            first.id()
        ));
    }
    let mut ids = Vec::new();
    for r in &refs {
        let expanded = spec.expand(&r.area, r.number, r.range_end);
        if expanded.is_empty() {
            return Err(format!("{path}: {} is not a defined requirement", r.id()));
        }
        ids.extend(expanded);
    }
    Ok(Record {
        path: path.to_owned(),
        ids,
        method: method.to_owned(),
        check: check.to_owned(),
    })
}

/// Fails when a requirement is covered by more than one record: its record is one file.
fn check_unique(records: &[Record]) -> Result<(), String> {
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    let mut problems = BTreeSet::new();
    for r in records {
        for id in &r.ids {
            if let Some(prev) = owner.insert(id, &r.path) {
                problems.insert(format!(
                    "{id} has two records, {prev} and {}: merge them",
                    r.path
                ));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.into_iter().collect::<Vec<_>>().join("\n"))
    }
}

/// Reads every record in [`DIR`].
pub fn load(spec: &Spec) -> Result<Vec<Record>, String> {
    let mut out = Vec::new();
    for path in repo::files(DIR, &["md"]) {
        let rel = repo::rel(&path);
        let stem = Path::new(&rel)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        out.push(parse(&rel, &stem, &repo::read(&path)?, spec)?);
    }
    if out.is_empty() {
        return Err(format!("{DIR}: no records"));
    }
    check_unique(&out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Record, check_unique, parse};
    use crate::spec::Spec;

    fn record(ids: &str, method: &str, check: &str) -> String {
        format!(
            "# Record: {ids}\n\n| | |\n|---|---|\n| Requirements | {ids} |\n| Method | {method} |\n\
             | Check | {check} |\n\nFree text, with a table below.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n"
        )
    }

    #[test]
    fn records_parse_and_expand() {
        // req: VER-020
        let spec = Spec::load().unwrap();
        let r = parse(
            "x/GEN-001.md",
            "GEN-001",
            &record("GEN-001…GEN-004", "A", "Verify check `layers`"),
            &spec,
        )
        .unwrap();
        assert_eq!(r.ids, ["GEN-001", "GEN-002", "GEN-003", "GEN-004"]);
        assert_eq!(r.label(), "A: Verify check `layers` (x/GEN-001.md)");
        let r = parse(
            "x/VER-030.md",
            "VER-030",
            &record("VER-030, GEN-007", "A, I", "c"),
            &spec,
        )
        .unwrap();
        assert_eq!(r.ids, ["VER-030", "GEN-007"]);
    }

    #[test]
    fn malformed_records_fail() {
        // req: VER-020
        let spec = Spec::load().unwrap();
        // Built at run time, so that `cargo xtask spec-lint` does not see an undefined ID here.
        let undefined = format!("VER-{}", 999);
        let bad = [
            ("VER-024", record("VER-024", "T", "c")),
            ("VER-024", record("VER-024", "A", "")),
            ("VER-025", record("VER-024", "A", "c")),
            ("VER-024", record("no id", "A", "c")),
            (undefined.as_str(), record(&undefined, "A", "c")),
            ("VER-024", "# Record\n\nno table\n".to_owned()),
            (
                "VER-024",
                "| | |\n|---|---|\n| Method | A |\n| Requirements | VER-024 |\n| Check | c |\n"
                    .to_owned(),
            ),
            (
                "VER-024",
                record("VER-024", "A", "c").replace("| Check | c |", "| Check | c | d |"),
            ),
        ];
        for (stem, text) in bad {
            assert!(
                parse(&format!("x/{stem}.md"), stem, &text, &spec).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn one_record_per_requirement() {
        // req: VER-020
        let r = |path: &str, ids: &[&str]| Record {
            path: path.to_owned(),
            ids: ids.iter().map(|s| (*s).to_owned()).collect(),
            method: "A".to_owned(),
            check: "c".to_owned(),
        };
        assert!(check_unique(&[r("a", &["VER-001"]), r("b", &["VER-002"])]).is_ok());
        let err =
            check_unique(&[r("a", &["VER-001", "VER-002"]), r("b", &["VER-002"])]).unwrap_err();
        assert!(err.contains("VER-002 has two records, a and b"), "{err}");
    }
}
