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

/// Extensions of C++ sources and headers (CPP-057).
pub const CPP_EXTENSIONS: &[&str] = &["hpp", "cpp", "h"];

/// Text files scanned for references to requirement IDs.
pub fn referencing_files() -> Vec<std::path::PathBuf> {
    let mut files = repo::files(
        ".",
        &["md", "rs", "json", "jsonl", "yml", "yaml", "toml", "py"],
    );
    // C++ sources and headers, so a C++ test cannot cite an undefined ID (CPP-057).
    files.extend(repo::files(".", CPP_EXTENSIONS));
    files.retain(|p| !repo::rel(p).starts_with("test-vectors/schema/"));
    files
}

/// The one form of a withdrawn requirement (00-introduction.md §5.2): the text starts
/// `Withdrawn: ` and a reason follows.
pub fn is_withdrawn(text: &str) -> bool {
    text.starts_with("Withdrawn:")
}

/// What is wrong with how `text` marks a requirement withdrawn, if it marks it so at all:
/// any marker other than `Withdrawn: <reason>` (`Deleted`, `**Withdrawn**`, `~~…~~`), or an
/// empty reason.
fn withdrawal_problem(text: &str) -> Option<&'static str> {
    if let Some(reason) = text.strip_prefix("Withdrawn:") {
        return reason
            .trim()
            .is_empty()
            .then_some("`Withdrawn:` needs a reason");
    }
    let lower = text
        .trim_start_matches(['*', '_', '~', ' '])
        .to_ascii_lowercase();
    (text.starts_with("~~") || lower.starts_with("withdrawn") || lower.starts_with("deleted"))
        .then_some("a withdrawn requirement starts `Withdrawn: <reason>`, in no other form")
}

fn check_verif(cell: &str) -> bool {
    cell == "—"
        || cell
            .split('/')
            .all(|t| matches!(t, "T" | "V" | "F" | "A" | "I"))
}

/// Prefix of the placeholder that a pull request writes for a new open issue until
/// `scripts/reverify` numbers it (docs/09-open-issues.md). The placeholder is the prefix and a slug
/// of lowercase letters, digits, and inner hyphens.
const OI_PLACEHOLDER: &str = "OI-NEW-";

/// The placeholders of one line of text, in order.
fn placeholders(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (at, _) in line.match_indices(OI_PLACEHOLDER) {
        let start = at + OI_PLACEHOLDER.len();
        let rest = line.get(start..).unwrap_or_default();
        let len = rest
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
            .count();
        let slug = rest.get(..len).unwrap_or_default().trim_end_matches('-');
        if slug.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && let Some(token) = line.get(at..start + slug.len())
        {
            out.push(token);
        }
    }
    out
}

/// Problems with the open-issue IDs: an ID used by two rows of the table of `oi_doc`, a row whose
/// ID is neither `OI-<digits>` nor a placeholder, a placeholder with no row, and, with `numbered`
/// (the verification run), any placeholder at all. `files` are the (path, text) pairs to scan for
/// placeholders.
fn oi_problems(oi_doc: &str, files: &[(String, String)], numbered: bool) -> Vec<String> {
    let at = "docs/09-open-issues.md";
    let mut problems = Vec::new();
    let mut rows: BTreeMap<String, usize> = BTreeMap::new();
    for table in tables(oi_doc) {
        for (line, row) in table.rows {
            let Some(id) = row.first().filter(|c| c.starts_with("OI-")) else {
                continue;
            };
            let digits = id
                .strip_prefix("OI-")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
            if !digits && placeholders(id) != [id.as_str()] {
                problems.push(format!(
                    "{at}:{line}: {id:?} is neither OI-<number> nor an {OI_PLACEHOLDER}<slug> placeholder"
                ));
            }
            if let Some(first) = rows.insert(id.clone(), line) {
                problems.push(format!(
                    "{at}:{line}: duplicate {id} (first at line {first}); if two pull requests took \
                     the same number, run scripts/reverify, which renumbers the open issues the \
                     branch added; if a merge kept two versions of one row, keep one"
                ));
            }
        }
    }
    for (path, text) in files {
        for (n, line) in text.lines().enumerate() {
            for token in placeholders(line) {
                let here = format!("{path}:{}", n + 1);
                if numbered {
                    problems.push(format!(
                        "{here}: {token} is not numbered yet: scripts/reverify assigns the next \
                         free OI number when it brings the branch up to date with main"
                    ));
                } else if !rows.contains_key(token) {
                    problems.push(format!(
                        "{here}: {token} has no row in {at}: add the row, or fix the slug"
                    ));
                }
            }
        }
    }
    problems
}

/// `xtask spec-lint`: structure of requirement tables and validity of every ID reference. A
/// placeholder for a new open issue is allowed, so that a branch can pass its own checks.
pub fn lint() -> Result<usize, String> {
    run_lint(false)
}

/// `xtask spec-lint --numbered`, the check of the verification run: also refuses a placeholder
/// of an open issue, so that a pull request cannot merge before `scripts/reverify` numbers it.
pub fn lint_numbered() -> Result<usize, String> {
    run_lint(true)
}

fn run_lint(numbered: bool) -> Result<usize, String> {
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
        if let Some(why) = withdrawal_problem(&r.text) {
            problems.push(format!("{at}: {}: {why}", r.id));
        }
        if !check_verif(&r.verif) {
            problems.push(format!(
                "{at}: {} has invalid Verif. {:?} (use T/V/F/A/I joined by '/', or —)",
                r.id, r.verif
            ));
        }
    }

    let mut texts = Vec::new();
    for path in referencing_files() {
        texts.push((repo::rel(&path), repo::read(&path)?));
    }
    let oi_doc = repo::read(&repo::root().join("docs/09-open-issues.md"))?;
    problems.extend(oi_problems(&oi_doc, &texts, numbered));

    for (path, text) in &texts {
        for id in scan_ids(text, &areas) {
            if !spec.requirements.contains_key(&id.id()) {
                problems.push(format!(
                    "{path}:{}: reference to undefined requirement {}",
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

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{is_withdrawn, oi_problems, placeholders, withdrawal_problem};

    /// A placeholder, built here so that spec-lint does not see one in this file.
    fn ph(slug: &str) -> String {
        format!("OI-{}-{slug}", "NEW")
    }

    fn table(rows: &[String]) -> String {
        let mut t =
            "| ID | Issue | Affects | Proposed resolution | Status |\n|---|---|---|---|---|\n"
                .to_owned();
        for r in rows {
            let _ = writeln!(t, "| {r} | x | y | z | Open |");
        }
        t
    }

    fn file(text: &str) -> Vec<(String, String)> {
        vec![("docs/x.md".to_owned(), text.to_owned())]
    }

    #[test]
    fn placeholder_syntax_is_exact() {
        let a = ph("cpp-std");
        assert_eq!(
            placeholders(&format!("see {a}, and {a}.")),
            [a.as_str(), a.as_str()]
        );
        assert_eq!(placeholders(&format!("({a}-)")), [a.as_str()]);
        assert_eq!(
            placeholders(&format!("OI-{}-<slug>", "NEW")),
            Vec::<&str>::new()
        );
        assert_eq!(
            placeholders(&format!("OI-{}-Upper", "NEW")),
            Vec::<&str>::new()
        );
        assert_eq!(placeholders("OI-67 and OI-01"), Vec::<&str>::new());
    }

    #[test]
    fn the_gate_refuses_a_placeholder_and_a_branch_may_carry_one() {
        let a = ph("a");
        let doc = table(&["OI-01".to_owned(), a.clone()]);
        let refs = file(&format!("see {a}"));
        assert_eq!(oi_problems(&doc, &refs, false), Vec::<String>::new());
        let numbered = oi_problems(&doc, &refs, true);
        assert_eq!(numbered.len(), 1, "{numbered:?}");
        assert!(numbered[0].starts_with("docs/x.md:1:"), "{numbered:?}");
        assert!(numbered[0].contains("scripts/reverify"), "{numbered:?}");
        assert_eq!(
            oi_problems(&table(&["OI-01".to_owned()]), &[], true),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_placeholder_needs_a_row_and_an_id_is_used_once() {
        let a = ph("a");
        let no_row = oi_problems(&table(&["OI-01".to_owned()]), &file(&a), false);
        assert_eq!(no_row.len(), 1, "{no_row:?}");
        assert!(no_row[0].contains("has no row"), "{no_row:?}");
        let dup = oi_problems(
            &table(&["OI-67".to_owned(), "OI-67".to_owned()]),
            &[],
            false,
        );
        assert_eq!(dup.len(), 1, "{dup:?}");
        assert!(dup[0].contains("duplicate OI-67"), "{dup:?}");
        let dup_ph = oi_problems(&table(&[a.clone(), a]), &[], false);
        assert_eq!(dup_ph.len(), 1, "{dup_ph:?}");
        let bad = oi_problems(&table(&["OI-6x".to_owned()]), &[], false);
        assert_eq!(bad.len(), 1, "{bad:?}");
    }

    #[test]
    fn one_withdrawn_form() {
        assert!(is_withdrawn("Withdrawn: replaced by X-001."));
        assert!(!is_withdrawn("Deleted"));
        assert!(!is_withdrawn("withdrawn: x"));
        assert!(!is_withdrawn("The frame SHALL be withdrawn: never."));
        assert_eq!(withdrawal_problem("Withdrawn: replaced by X-001."), None);
        assert_eq!(
            withdrawal_problem("The frame SHALL carry a Deleted flag."),
            None
        );
        for bad in [
            "**Deleted**",
            "Deleted",
            "withdrawn",
            "Withdrawn",
            "**Withdrawn** 2026",
            "~~The frame SHALL.~~",
            "Withdrawn:",
            "Withdrawn:   ",
        ] {
            assert!(withdrawal_problem(bad).is_some(), "{bad}");
        }
    }
}
