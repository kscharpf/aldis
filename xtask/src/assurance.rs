//! `xtask assurance`: the references and the formats of the hazard analysis and the assurance
//! case (VER-065, VER-066). The rules are in `docs/verification/assurance-evidence.md`; section
//! numbers in comments are that file's.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use aldis_testkit::VectorFile;
use serde_json::json;

use crate::md::{Table, scan_ids, tables};
use crate::records;
use crate::repo;
use crate::rs020::{Tok, group_end, tokens};
use crate::spec::Spec;

const HAZARDS: &str = "docs/verification/hazards.md";
const HAZARD_DIR: &str = "docs/verification/hazards";
const CASE: &str = "docs/verification/assurance-case.md";
const CODECS: [&str; 5] = ["X", "TM", "TC", "AOS", "USLP"];
const FILE_CODECS: [(&str, &str); 4] = [
    ("tm.md", "TM"),
    ("tc.md", "TC"),
    ("aos.md", "AOS"),
    ("uslp.md", "USLP"),
];
const SLUGS: [&str; 12] = [
    "corrupt-accepted",
    "valid-rejected",
    "misrouting",
    "truncation",
    "wrong-value",
    "nonconformant-output",
    "unbounded-time",
    "memory",
    "panic",
    "state-corruption",
    "abi-misuse",
    "hidden-deviation",
];
const ROWS: [&str; 12] = [
    "Category",
    "Status",
    "Severity",
    "Failure mode",
    "Cause",
    "Mission effect",
    "Requirements",
    "Evidence",
    "Residual risk",
    "Assumptions",
    "Proposals",
    "Claims",
];
const STATUSES: [&str; 4] = ["todo", "controlled", "residual", "gap"];
const KINDS: [&str; 19] = [
    "file",
    "test",
    "nightly-test",
    "proof",
    "vector",
    "superseded-vector",
    "record",
    "check",
    "nightly-check",
    "ci",
    "job",
    "fuzz",
    "oi",
    "gh",
    "hz",
    "ac",
    "asm",
    "df",
    "proposal",
];
const EVIDENCE_HEADER: [&str; 3] = ["Evidence", "Shows", "Requirements"];
const ASSUMPTION_HEADER: [&str; 4] = ["ID", "Assumption", "Basis", "Integrator obligation"];
const DEFEATER_HEADER: [&str; 5] = ["ID", "Defeater", "Weakens", "References", "Status"];
const COVERAGE_HEADER: [&str; 3] = ["Category", "Hazards", "State"];
const PROPOSAL_HEADER: [&str; 4] = ["ID", "Kind", "Proposal", "Status"];
const ASSUMPTIONS_HEADING: &str = "Assumptions and context";

// ---------------------------------------------------------------------------------------------
// References (§2)

/// Splits a code span into kind and target if it has the shape of a reference (§2.1).
fn parse_ref(span: &str) -> Option<(&str, &str)> {
    let (kind, target) = span.split_once(':')?;
    let kind_ok = !kind.is_empty()
        && kind
            .split('-')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase()));
    (kind_ok && !target.is_empty() && !target.starts_with(':')).then_some((kind, target))
}

/// The code spans of `text`: the odd-numbered pieces of a split at backticks. `None` when the
/// number of backticks is odd.
fn spans(text: &str) -> Option<Vec<&str>> {
    text.matches('`')
        .count()
        .is_multiple_of(2)
        .then(|| text.split('`').skip(1).step_by(2).collect())
}

/// The text between the code spans of `text`.
fn between_spans(text: &str) -> String {
    text.split('`').step_by(2).collect()
}

/// The content of `cell` when it is one code span and nothing else.
fn single_span(cell: &str) -> Option<&str> {
    let inner = cell.strip_prefix('`')?.strip_suffix('`')?;
    (!inner.contains('`')).then_some(inner)
}

fn is_ident(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// One function the scan of a Rust file found (§2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
struct FnDef {
    name: String,
    test: bool,
    proof: bool,
    ignored: bool,
}

/// Index of the bracket that opens the one closing at `close`, not below `floor`.
fn open_before(t: &[Tok], close: usize, floor: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut i = close + 1;
    while i > floor {
        i -= 1;
        match t[i].text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The outer attributes (tokens between `#[` and `]`) before the `fn` at `fn_at`, after the
/// modifiers `pub`, `const`, `async`, `unsafe`, `extern`, and a `pub(…)` group; not before `floor`.
fn attribute_run(t: &[Tok], fn_at: usize, floor: usize) -> Vec<&[Tok]> {
    let mut i = fn_at;
    while i > floor {
        match t[i - 1].text.as_str() {
            "pub" | "const" | "async" | "unsafe" | "extern" => i -= 1,
            ")" => match open_before(t, i - 1, floor) {
                Some(o) if o > floor && t[o - 1].text == "pub" => i = o - 1,
                _ => break,
            },
            _ => break,
        }
    }
    let mut run = Vec::new();
    while i > floor && t[i - 1].text == "]" {
        match open_before(t, i - 1, floor) {
            Some(open) if open > floor && t[open - 1].text == "#" => {
                run.push(&t[open + 1..i - 1]);
                i = open - 1;
            }
            _ => break,
        }
    }
    run
}

fn words(attr: &[Tok]) -> Vec<&str> {
    attr.iter().map(|t| t.text.as_str()).collect()
}

fn is_open(t: &[Tok], at: usize) -> bool {
    t.get(at)
        .is_some_and(|o| matches!(o.text.as_str(), "(" | "[" | "{"))
}

fn is_text(t: &[Tok], at: usize, text: &str) -> bool {
    t.get(at).is_some_and(|o| o.text == text)
}

/// The name after the `fn` at `at`, if it is one.
fn fn_name(t: &[Tok], at: usize) -> Option<&str> {
    let tok = t.get(at + 1)?;
    (t[at].text == "fn" && is_ident(&tok.text)).then_some(tok.text.as_str())
}

fn ignored(run: &[&[Tok]]) -> bool {
    run.iter().any(|a| words(a).first() == Some(&"ignore"))
}

/// Cases (a) and (b) of §2.4: attributes on a function, and functions inside `proptest!`.
fn attributed_fns(t: &[Tok]) -> BTreeMap<usize, FnDef> {
    let mut found = BTreeMap::new();
    for at in 0..t.len() {
        let Some(name) = fn_name(t, at) else { continue };
        let run = attribute_run(t, at, 0);
        let test = run.iter().any(|a| words(a) == ["test"]);
        let proof = run.iter().any(|a| words(a) == ["kani", "::", "proof"]);
        if test || proof {
            let def = FnDef {
                name: name.to_owned(),
                test,
                proof,
                ignored: ignored(&run),
            };
            found.insert(at, def);
        }
    }
    for i in 0..t.len() {
        if !(is_text(t, i, "proptest") && is_text(t, i + 1, "!") && is_open(t, i + 2)) {
            continue;
        }
        let Ok(end) = group_end(t, i + 2) else {
            continue;
        };
        for at in i + 3..end {
            let Some(name) = fn_name(t, at) else { continue };
            let run = attribute_run(t, at, i + 2);
            let def = FnDef {
                name: name.to_owned(),
                test: true,
                proof: found.get(&at).is_some_and(|d| d.proof),
                ignored: ignored(&run),
            };
            found.insert(at, def);
        }
    }
    found
}

/// Case (c) of §2.4: names passed to a local `macro_rules!` whose body has `#[test]`.
fn macro_tests(t: &[Tok]) -> Vec<FnDef> {
    // Macro name -> whether its body mentions `ignore`.
    let mut macros: BTreeMap<&str, bool> = BTreeMap::new();
    for i in 0..t.len() {
        if !(is_text(t, i, "macro_rules") && is_text(t, i + 1, "!") && is_open(t, i + 3)) {
            continue;
        }
        let Ok(end) = group_end(t, i + 3) else {
            continue;
        };
        let body = &t[i + 4..end];
        if body.windows(4).any(|w| words(w) == ["#", "[", "test", "]"]) {
            macros.insert(
                t[i + 2].text.as_str(),
                body.iter().any(|x| x.text == "ignore"),
            );
        }
    }
    let mut out = Vec::new();
    for (i, tok) in t.iter().enumerate() {
        let Some(&ignored) = macros.get(tok.text.as_str()) else {
            continue;
        };
        let first = t.get(i + 3).filter(|f| is_ident(&f.text));
        if let Some(first) = first.filter(|_| is_text(t, i + 1, "!") && is_open(t, i + 2)) {
            out.push(FnDef {
                name: first.text.clone(),
                test: true,
                proof: false,
                ignored,
            });
        }
    }
    out
}

/// Every test and proof function a Rust file defines (§2.4).
fn fn_defs(text: &str) -> Vec<FnDef> {
    let t = tokens(text);
    let mut out: Vec<FnDef> = attributed_fns(&t).into_values().collect();
    out.extend(macro_tests(&t));
    out
}

/// What the references to the analysed files resolve against.
#[derive(Default)]
struct Defs {
    hz: BTreeSet<String>,
    ac: BTreeSet<String>,
    asm: BTreeSet<String>,
    df: BTreeSet<String>,
    /// Proposal ID -> `proposed`, `accepted`, or `rejected`.
    proposals: BTreeMap<String, String>,
}

/// Reads a file of the repository by its path, if it exists.
type ReadFile = Box<dyn Fn(&str) -> Option<String>>;

/// The repository, as the references of §2.3 see it.
struct Env {
    spec: Spec,
    files: BTreeSet<String>,
    read: ReadFile,
    fn_cache: RefCell<BTreeMap<String, Vec<FnDef>>>,
    /// Vector case ID -> superseded.
    vectors: BTreeMap<String, bool>,
    /// Paths of the record files.
    records: BTreeSet<String>,
    /// Check name -> (gating, nightly).
    checks: BTreeMap<String, (bool, bool)>,
    ci: BTreeSet<String>,
    jobs: BTreeSet<String>,
    fuzz: BTreeSet<String>,
    /// OI ID -> status (the last cell).
    ois: BTreeMap<String, String>,
}

/// The jobs of a workflow: the lines of two spaces, a name, and `:` after `jobs:`.
fn workflow_jobs(workflow: &str) -> BTreeSet<String> {
    workflow
        .lines()
        .skip_while(|l| *l != "jobs:")
        .skip(1)
        .filter_map(|l| l.strip_prefix("  ")?.strip_suffix(':'))
        .filter(|n| !n.is_empty() && !n.contains(char::is_whitespace))
        .map(str::to_owned)
        .collect()
}

fn open_issues(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for t in tables(text) {
        for (_, row) in t.rows {
            if let (Some(id), Some(status)) = (row.first(), row.last())
                && id.starts_with("OI-")
            {
                out.insert(id.clone(), status.clone());
            }
        }
    }
    out
}

fn vector_ids() -> Result<BTreeMap<String, bool>, String> {
    let mut out: BTreeMap<String, bool> = BTreeMap::new();
    let mut add = |id: &str, superseded: bool| {
        let e = out.entry(id.to_owned()).or_insert(true);
        *e = *e && superseded;
    };
    for path in repo::files("test-vectors", &["json"]) {
        let rel = repo::rel(&path);
        if !crate::vectors::is_vector_file(&rel) {
            continue;
        }
        match VectorFile::from_json(&repo::read(&path)?) {
            Ok(VectorFile::Codec(f)) => {
                for c in &f.cases {
                    add(&c.id, c.superseded.is_some());
                }
            }
            Ok(VectorFile::EngineScenario(s)) => add(&s.id, s.superseded.is_some()),
            Err(e) => return Err(format!("{rel}: {e} (run `cargo xtask vectors validate`)")),
        }
    }
    Ok(out)
}

impl Env {
    fn load(spec: Spec) -> Result<Env, String> {
        let root = repo::root();
        let text = |rel: &str| repo::read(&root.join(rel));
        let files = repo::all_files(".").iter().map(|p| repo::rel(p)).collect();
        let records = records::load(&spec)?.into_iter().map(|r| r.path).collect();
        Ok(Env {
            files,
            read: Box::new(|p| std::fs::read_to_string(repo::root().join(p)).ok()),
            fn_cache: RefCell::new(BTreeMap::new()),
            vectors: vector_ids()?,
            records,
            checks: crate::verify::all_checks()
                .into_iter()
                .map(|c| (c.name, (c.gating, c.nightly)))
                .collect(),
            ci: crate::CI.iter().map(|(n, _)| (*n).to_owned()).collect(),
            jobs: workflow_jobs(&text(".github/workflows/nightly.yml")?),
            fuzz: crate::consistency::bin_targets(&text("fuzz/Cargo.toml")?),
            ois: open_issues(&text("docs/09-open-issues.md")?),
            spec,
        })
    }

    /// A withdrawn requirement (00-introduction.md §5.2).
    fn is_deleted(&self, id: &str) -> bool {
        self.spec
            .requirements
            .get(id)
            .is_some_and(|r| crate::spec::is_withdrawn(&r.text))
    }

    fn repo_file(&self, target: &str) -> Result<(), String> {
        let bad =
            target.starts_with('/') || target.contains('#') || target.split('/').any(|s| s == "..");
        if bad {
            return Err(
                "expected a path relative to the repository root, without `..` or `#`".to_owned(),
            );
        }
        if self.files.contains(target) {
            Ok(())
        } else {
            Err("no such file".to_owned())
        }
    }

    fn defs_of(&self, path: &str) -> Vec<FnDef> {
        if let Some(d) = self.fn_cache.borrow().get(path) {
            return d.clone();
        }
        let defs = (self.read)(path).map_or_else(Vec::new, |t| fn_defs(&t));
        self.fn_cache
            .borrow_mut()
            .insert(path.to_owned(), defs.clone());
        defs
    }

    /// The kinds `test`, `nightly-test`, and `proof` (§2.3, §2.4).
    fn rust_item(&self, kind: &str, target: &str) -> Result<(), String> {
        let (path, name) = target
            .rsplit_once("::")
            .ok_or("expected `<path>::<name>`")?;
        let rs = std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e == "rs");
        if !is_ident(name) || !rs {
            return Err("expected `<path>.rs::<name>`".to_owned());
        }
        self.repo_file(path)?;
        let proof = kind == "proof";
        let defs: Vec<FnDef> = self
            .defs_of(path)
            .into_iter()
            .filter(|d| d.name == name && if proof { d.proof } else { d.test })
            .collect();
        if defs.is_empty() {
            let what = if proof { "proof" } else { "test" };
            return Err(format!("`{path}` defines no {what} `{name}`"));
        }
        if proof {
            return Ok(());
        }
        let n_ignored = defs.iter().filter(|d| d.ignored).count();
        if n_ignored != 0 && n_ignored != defs.len() {
            return Err(format!(
                "`{name}` is both ignored and not ignored in `{path}`"
            ));
        }
        match (kind, n_ignored == 0) {
            ("test", false) => Err(format!("`{name}` is ignored: cite it as `nightly-test:`")),
            ("nightly-test", true) => Err(format!("`{name}` is not ignored: cite it as `test:`")),
            _ => Ok(()),
        }
    }

    fn check_kind(&self, kind: &str, target: &str) -> Result<(), String> {
        let (gating, nightly) = *self
            .checks
            .get(target)
            .ok_or("no such check in `cargo xtask verify list`")?;
        match (kind, gating && !nightly) {
            ("check", false) => {
                Err("not gating, or nightly only: cite it as `nightly-check:`".to_owned())
            }
            ("nightly-check", true) => {
                Err("gating and not nightly only: cite it as `check:`".to_owned())
            }
            _ => Ok(()),
        }
    }

    fn vector_kind(&self, kind: &str, target: &str) -> Result<(), String> {
        match self.vectors.get(target) {
            None => Err("no such vector case".to_owned()),
            Some(true) if kind == "vector" => {
                Err("superseded: cite it as `superseded-vector:`".to_owned())
            }
            Some(false) if kind == "superseded-vector" => {
                Err("not superseded: cite it as `vector:`".to_owned())
            }
            Some(_) => Ok(()),
        }
    }

    /// Whether the reference resolves (§2.3). `hz`, `ac`, `asm`, `df`, and `proposal` resolve
    /// against `defs`.
    fn resolve(&self, defs: &Defs, kind: &str, target: &str) -> Result<(), String> {
        let found =
            |ok: bool, what: &str| ok.then_some(()).ok_or_else(|| format!("no such {what}"));
        match kind {
            "file" => self.repo_file(target),
            "test" | "nightly-test" | "proof" => self.rust_item(kind, target),
            "vector" | "superseded-vector" => self.vector_kind(kind, target),
            "record" => found(
                self.records
                    .contains(&format!("{}/{target}.md", records::DIR)),
                "record",
            ),
            "check" | "nightly-check" => self.check_kind(kind, target),
            "ci" => found(self.ci.contains(target), "step of `cargo xtask ci`"),
            "job" => found(self.jobs.contains(target), "job of the nightly workflow"),
            "fuzz" => found(self.fuzz.contains(target), "fuzz target"),
            "oi" => found(self.ois.contains_key(target), "open issue"),
            "gh" => found(
                target
                    .strip_prefix('#')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
                "issue number (expected `#N`)",
            ),
            "hz" => found(defs.hz.contains(target), "hazard"),
            "ac" => found(defs.ac.contains(target), "claim"),
            "asm" => found(defs.asm.contains(target), "assumption or context row"),
            "df" => found(defs.df.contains(target), "defeater"),
            "proposal" => found(defs.proposals.contains_key(target), "proposal"),
            _ => Err("unknown kind".to_owned()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Documents

/// A heading of a Markdown file.
#[derive(Debug, Clone)]
struct Heading {
    /// 1-based line.
    line: usize,
    level: usize,
    text: String,
}

/// A Markdown file with its headings and fenced code blocks found.
struct Doc<'a> {
    path: &'a str,
    lines: Vec<&'a str>,
    in_fence: Vec<bool>,
    heads: Vec<Heading>,
}

impl<'a> Doc<'a> {
    fn new(path: &'a str, text: &'a str) -> Doc<'a> {
        let lines: Vec<&str> = text.lines().collect();
        let mut in_fence = Vec::new();
        let mut heads = Vec::new();
        let mut fenced = false;
        for (n, line) in lines.iter().enumerate() {
            let fence = line.trim_start().starts_with("```");
            in_fence.push(fenced || fence);
            if fence {
                fenced = !fenced;
                continue;
            }
            let level = line.bytes().take_while(|b| *b == b'#').count();
            if !fenced && (1..=6).contains(&level) && line.as_bytes().get(level) == Some(&b' ') {
                heads.push(Heading {
                    line: n + 1,
                    level,
                    text: line.get(level + 1..).unwrap_or_default().trim().to_owned(),
                });
            }
        }
        Doc {
            path,
            lines,
            in_fence,
            heads,
        }
    }

    /// The 0-based line range after heading `k`, up to the next heading, or, when `nested`, to the
    /// next heading of the same or a higher rank.
    fn section(&self, k: usize, nested: bool) -> (usize, usize) {
        let h = &self.heads[k];
        let end = self.heads[k + 1..]
            .iter()
            .find(|n| !nested || n.level <= h.level)
            .map_or(self.lines.len(), |n| n.line - 1);
        (h.line, end)
    }

    /// The tables of the lines `from..to` (0-based), with absolute line numbers.
    fn tables_in(&self, from: usize, to: usize) -> Vec<Table> {
        let body = self.lines.get(from..to).unwrap_or_default().join("\n");
        let mut found = tables(&body);
        for t in &mut found {
            for row in &mut t.rows {
                row.0 += from;
            }
        }
        found
    }

    fn heading(&self, level: usize, text: &str) -> Option<usize> {
        self.heads
            .iter()
            .position(|h| h.level == level && h.text == text)
    }

    const fn at(&self, line: usize) -> At<'a> {
        At {
            path: self.path,
            line,
        }
    }
}

/// A place in an analysed file.
#[derive(Clone, Copy)]
struct At<'a> {
    path: &'a str,
    line: usize,
}

/// A reference found in an analysed file.
#[derive(Debug, Clone)]
struct Reference {
    file: String,
    line: usize,
    kind: String,
    target: String,
}

#[derive(Default)]
struct Report {
    problems: Vec<String>,
}

impl Report {
    fn add(&mut self, at: At, msg: impl AsRef<str>) {
        let p = format!("{}:{}: {}", at.path, at.line, msg.as_ref());
        if !self.problems.contains(&p) {
            self.problems.push(p);
        }
    }
}

/// Every reference on the lines of `doc` outside fenced code (§2.1): a code span of the shape of
/// a reference with a kind of §2.3.
fn scan_references(doc: &Doc) -> Vec<Reference> {
    let mut out = Vec::new();
    for (n, line) in doc.lines.iter().enumerate() {
        if doc.in_fence[n] {
            continue;
        }
        for span in spans(line).unwrap_or_default() {
            if let Some((kind, target)) = parse_ref(span).filter(|(k, _)| KINDS.contains(k)) {
                out.push(Reference {
                    file: doc.path.to_owned(),
                    line: n + 1,
                    kind: kind.to_owned(),
                    target: target.to_owned(),
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Cells

/// An Evidence cell (§2.1): `TODO`, or code spans that are all references and at least one.
/// Returns whether the cell is `TODO`.
fn evidence_cell(rep: &mut Report, at: At, cell: &str, allow_todo: bool) -> bool {
    if cell == "TODO" {
        if !allow_todo {
            rep.add(
                at,
                "Evidence is `TODO`, which only a hazard with the status `todo` may have",
            );
        }
        return true;
    }
    let Some(found) = spans(cell) else {
        rep.add(at, "Evidence cell with an odd number of backticks");
        return false;
    };
    for span in &found {
        match parse_ref(span) {
            Some((kind, _)) if KINDS.contains(&kind) => {}
            Some((kind, _)) => rep.add(at, format!("`{span}`: unknown reference kind `{kind}`")),
            None => rep.add(at, format!("`{span}` is not a reference (`kind:target`)")),
        }
    }
    if found.is_empty() {
        rep.add(at, "Evidence cell without a reference");
    }
    false
}

/// A cell of references of one kind and separators (`Assumptions`, `Proposals`, `Claims`,
/// `Weakens`). Returns the targets.
fn ref_list(rep: &mut Report, at: At, cell: &str, kind: &str) -> Vec<String> {
    let Some(found) = spans(cell) else {
        rep.add(at, "cell with an odd number of backticks");
        return Vec::new();
    };
    let mut out = Vec::new();
    for span in found {
        match parse_ref(span) {
            Some((k, target)) if k == kind => out.push(target.to_owned()),
            _ => rep.add(at, format!("`{span}` is not a `{kind}:` reference")),
        }
    }
    let separators = |c: char| c.is_whitespace() || c == ';' || c == ',';
    if !between_spans(cell).chars().all(separators) {
        rep.add(
            at,
            format!("only `{kind}:` references and separators are allowed here"),
        );
    }
    out
}

/// A `Requirements` cell or column (§2.2). Returns whether it is `TODO`.
fn requirements_cell(rep: &mut Report, env: &Env, at: At, cell: &str, allow_todo: bool) -> bool {
    if cell == "TODO" {
        if !allow_todo {
            rep.add(
                at,
                "Requirements is `TODO`, which only a hazard with the status `todo` may have",
            );
        }
        return true;
    }
    let ids = scan_ids(cell, &env.spec.area_set());
    if ids.is_empty() {
        rep.add(at, "Requirements names no requirement ID");
    }
    for r in &ids {
        let mut ends = vec![r.number];
        ends.extend(r.range_end);
        for n in ends {
            let id = format!("{}-{n:03}", r.area);
            if !env.spec.requirements.contains_key(&id) {
                rep.add(at, format!("{id} is not a defined requirement"));
            } else if env.is_deleted(&id) {
                rep.add(at, format!("{id} is deleted"));
            }
        }
        if env.spec.expand(&r.area, r.number, r.range_end).is_empty() {
            rep.add(at, format!("{} expands to no requirement", r.id()));
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------
// Hazard files (§3)

/// A hazard, for the cross checks and the outputs.
#[derive(Debug, Clone)]
struct Hazard {
    id: String,
    file: String,
    line: usize,
    category: String,
    status: String,
    severity: String,
    claims: Vec<String>,
    proposals: Vec<String>,
}

/// `HZ-<codec>-NN <title>` -> (ID, codec, number).
fn parse_hazard_heading(text: &str) -> Option<(String, &str, u32)> {
    let (id, title) = text.split_once(' ')?;
    let (codec, nn) = id.strip_prefix("HZ-")?.split_once('-')?;
    let ok = CODECS.contains(&codec)
        && nn.len() == 2
        && nn.bytes().all(|b| b.is_ascii_digit())
        && !title.trim().is_empty();
    ok.then(|| (id.to_owned(), codec, nn.parse().unwrap_or(0)))
}

/// A hazard table: row name -> (line, value).
type Cells<'a> = BTreeMap<&'a str, (usize, &'a str)>;

/// The state shared by the passes over the analysed files.
struct Pass<'a> {
    env: &'a Env,
    strict: bool,
    rep: Report,
    defs: Defs,
    hazards: Vec<Hazard>,
    claims: Vec<Claim>,
    /// IDs of the proposals some hazard names.
    named_proposals: BTreeSet<String>,
    /// Rows of the assumptions table, with their lines.
    assumptions: Vec<(String, usize)>,
    /// Every proposal with the line and file that define it.
    proposal_places: Vec<(String, usize, String)>,
}

impl Pass<'_> {
    fn hazard_file(&mut self, doc: &Doc, codec: &str) {
        let mut last: Option<u32> = None;
        for (k, h) in doc.heads.iter().enumerate() {
            if !h.text.starts_with("HZ-") {
                continue;
            }
            let at = doc.at(h.line);
            let parsed = parse_hazard_heading(&h.text).filter(|_| h.level == 3);
            let Some((id, hz_codec, num)) = parsed else {
                self.rep.add(
                    at,
                    "malformed hazard heading, expected `### HZ-<codec>-NN <title>`",
                );
                continue;
            };
            if hz_codec != codec {
                self.rep
                    .add(at, format!("{id}: the codec of this file is `{codec}`"));
            }
            if !self.defs.hz.insert(id.clone()) {
                self.rep.add(at, format!("{id} is defined twice"));
            }
            if last.is_some_and(|l| num <= l) {
                self.rep.add(
                    at,
                    format!("{id}: hazard numbers must increase in file order"),
                );
            }
            last = Some(num);
            self.hazard_table(doc, k, &id);
        }
        if codec != "X" {
            self.coverage(doc);
        }
        self.proposals_table(doc, codec);
    }

    fn hazard_table(&mut self, doc: &Doc, k: usize, id: &str) {
        let (from, to) = doc.section(k, false);
        let head = doc.heads[k].line;
        let at = doc.at(head);
        let Some(table) = doc.tables_in(from, to).into_iter().next() else {
            self.rep.add(at, format!("{id}: no hazard table"));
            return;
        };
        if table.header != ["", ""] {
            self.rep
                .add(at, format!("{id}: the hazard table starts with `| | |`"));
        }
        let mut cells = Cells::new();
        let mut names = Vec::new();
        for (line, row) in &table.rows {
            let [name, value] = row.as_slice() else {
                self.rep.add(doc.at(*line), "a hazard row has two cells");
                continue;
            };
            names.push(name.as_str());
            if cells.insert(name, (*line, value)).is_some() {
                self.rep
                    .add(doc.at(*line), format!("duplicate row `{name}`"));
            }
        }
        for want in ROWS.iter().filter(|w| !names.contains(w)) {
            self.rep.add(at, format!("{id}: missing row `{want}`"));
        }
        for name in names.iter().filter(|n| !ROWS.contains(n)) {
            self.rep.add(at, format!("{id}: unexpected row `{name}`"));
        }
        let known: Vec<&str> = names.iter().copied().filter(|n| ROWS.contains(n)).collect();
        let misplaced = known.iter().zip(ROWS).find(|(got, want)| **got != *want);
        if let Some((got, want)) = misplaced.filter(|_| known.len() == ROWS.len()) {
            self.rep.add(
                at,
                format!("{id}: row `{got}` is misplaced, `{want}` comes here"),
            );
        }
        self.hazard_cells(doc, id, head, &cells);
    }

    fn hazard_cells(&mut self, doc: &Doc, id: &str, head: usize, cells: &Cells) {
        let line_of = |name: &str| doc.at(cells.get(name).map_or(head, |c| c.0));
        let value = |name: &str| cells.get(name).map_or("", |c| c.1);
        let category = single_span(value("Category")).unwrap_or_default();
        if !SLUGS.contains(&category) {
            self.rep.add(
                line_of("Category"),
                format!("{id}: `Category` is one code span holding a slug of hazards.md §1.2"),
            );
        }
        let status = single_span(value("Status")).unwrap_or_default();
        if !STATUSES.contains(&status) {
            self.rep.add(
                line_of("Status"),
                format!(
                    "{id}: `Status` is one code span: `todo`, `controlled`, `residual`, or `gap`"
                ),
            );
        }
        let severity = value("Severity");
        if !["S1", "S2", "S3", "S4"].contains(&severity) {
            self.rep.add(
                line_of("Severity"),
                format!("{id}: `Severity` is S1, S2, S3, or S4"),
            );
        }
        for name in ["Failure mode", "Cause", "Mission effect", "Residual risk"] {
            if cells.contains_key(name) && value(name).is_empty() {
                self.rep
                    .add(line_of(name), format!("{id}: `{name}` is empty"));
            }
        }
        if status == "residual" && value("Residual risk") == "—" {
            self.rep.add(
                line_of("Residual risk"),
                format!("{id}: a residual hazard states its residual risk"),
            );
        }
        let todo = status == "todo";
        if todo && self.strict {
            self.rep
                .add(doc.at(head), format!("{id}: status `todo` (--strict)"));
        }
        if cells.contains_key("Requirements") {
            let at = line_of("Requirements");
            requirements_cell(&mut self.rep, self.env, at, value("Requirements"), todo);
        }
        if cells.contains_key("Evidence") {
            evidence_cell(&mut self.rep, line_of("Evidence"), value("Evidence"), todo);
        }
        if cells.contains_key("Assumptions") && value("Assumptions") != "—" {
            let at = line_of("Assumptions");
            if ref_list(&mut self.rep, at, value("Assumptions"), "asm").is_empty() {
                self.rep.add(
                    at,
                    format!("{id}: `Assumptions` is `—` or names an `asm:` reference"),
                );
            }
        }
        let proposals = self.proposal_row(id, status, line_of("Proposals"), value("Proposals"));
        let claims = self.claims_row(id, todo, line_of("Claims"), value("Claims"));
        self.hazards.push(Hazard {
            id: id.to_owned(),
            file: doc.path.to_owned(),
            line: head,
            category: category.to_owned(),
            status: status.to_owned(),
            severity: severity.to_owned(),
            claims,
            proposals,
        });
    }

    fn claims_row(&mut self, id: &str, todo: bool, at: At, cell: &str) -> Vec<String> {
        if cell == "TODO" {
            if !todo {
                self.rep.add(
                    at,
                    format!("{id}: `Claims` is `TODO`, but the status is not `todo`"),
                );
            }
            return Vec::new();
        }
        let claims = ref_list(&mut self.rep, at, cell, "ac");
        if claims.is_empty() {
            self.rep
                .add(at, format!("{id}: a hazard names at least one claim"));
        }
        claims
    }

    fn proposal_row(&mut self, id: &str, status: &str, at: At, cell: &str) -> Vec<String> {
        let listed = if cell == "—" {
            Vec::new()
        } else {
            let l = ref_list(&mut self.rep, at, cell, "proposal");
            if l.is_empty() {
                self.rep.add(
                    at,
                    format!("{id}: `Proposals` is `—` or names a `proposal:` reference"),
                );
            }
            l
        };
        if status == "gap" && listed.is_empty() {
            self.rep.add(at, format!("{id}: a gap needs a proposal"));
        }
        if status == "controlled" && !listed.is_empty() {
            self.rep
                .add(at, format!("{id}: a controlled hazard has no proposal"));
        }
        self.named_proposals.extend(listed.iter().cloned());
        listed
    }

    fn coverage(&mut self, doc: &Doc) {
        let Some(k) = doc.heading(2, "Coverage") else {
            self.rep.add(doc.at(1), "no `## Coverage` heading");
            return;
        };
        let (from, to) = doc.section(k, false);
        let head = doc.at(doc.heads[k].line);
        let Some(table) = doc.tables_in(from, to).into_iter().next() else {
            self.rep.add(head, "no coverage table");
            return;
        };
        if table.header != COVERAGE_HEADER {
            self.rep.add(
                head,
                "the coverage table has the header `| Category | Hazards | State |`",
            );
        }
        if table.rows.len() != SLUGS.len() {
            let n = table.rows.len();
            self.rep.add(
                head,
                format!("the coverage table has {n} rows, expected 12"),
            );
        }
        let mut listed: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for (i, (line, row)) in table.rows.iter().enumerate() {
            let at = doc.at(*line);
            let slug = row.first().and_then(|c| single_span(c));
            let want = SLUGS.get(i).copied();
            let (Some(slug), true) = (slug, slug == want) else {
                let want = want.unwrap_or("no row");
                self.rep.add(at, format!("row {} is `{want}`", i + 1));
                continue;
            };
            let cell = |n: usize| row.get(n).map_or("", String::as_str);
            let ids = self.coverage_hazards(at, slug, cell(1));
            self.coverage_state(at, slug, cell(2), &ids);
            listed.insert(slug, ids);
        }
        for h in self.hazards.iter().filter(|h| h.file == doc.path) {
            if listed
                .get(h.category.as_str())
                .is_some_and(|ids| !ids.contains(&h.id))
            {
                let msg = format!("{} is missing from the coverage row `{}`", h.id, h.category);
                self.rep.add(doc.at(h.line), msg);
            }
        }
    }

    fn coverage_hazards(&mut self, at: At, slug: &str, cell: &str) -> Vec<String> {
        if cell == "—" {
            return Vec::new();
        }
        let ids: Vec<String> = cell.split(',').map(|s| s.trim().to_owned()).collect();
        for id in &ids {
            match self
                .hazards
                .iter()
                .find(|h| h.id == *id && h.file == at.path)
            {
                None => self.rep.add(
                    at,
                    format!("`{slug}` lists {id}, which this file does not define"),
                ),
                Some(h) if h.category != slug => self.rep.add(
                    at,
                    format!("`{slug}` lists {id}, whose category is `{}`", h.category),
                ),
                Some(_) => {}
            }
        }
        ids
    }

    fn coverage_state(&mut self, at: At, slug: &str, state: &str, ids: &[String]) {
        let todo_listed = ids.iter().any(|id| {
            self.hazards
                .iter()
                .any(|h| h.id == *id && h.status == "todo")
        });
        match state {
            "done" => {
                if ids.is_empty() {
                    self.rep
                        .add(at, format!("`{slug}` is `done` with no hazard"));
                }
                if todo_listed {
                    self.rep.add(
                        at,
                        format!("`{slug}` is `done` but lists a hazard with the status `todo`"),
                    );
                }
            }
            "TODO" => {
                if self.strict {
                    self.rep.add(at, format!("`{slug}` is `TODO` (--strict)"));
                }
            }
            _ => match state.strip_prefix("n/a:") {
                Some(reason) if !reason.trim().is_empty() => {
                    if !ids.is_empty() {
                        self.rep
                            .add(at, format!("`{slug}` is `n/a` but lists hazards"));
                    }
                }
                _ => self.rep.add(
                    at,
                    format!("`{slug}`: the state is `done`, `TODO`, or `n/a: <reason>`"),
                ),
            },
        }
    }

    fn proposals_table(&mut self, doc: &Doc, codec: &str) {
        let Some(k) = doc.heading(2, "Proposals") else {
            self.rep.add(doc.at(1), "no `## Proposals` heading");
            return;
        };
        let (from, to) = doc.section(k, false);
        let head = doc.at(doc.heads[k].line);
        let Some(table) = doc.tables_in(from, to).into_iter().next() else {
            self.rep.add(head, "no proposals table");
            return;
        };
        if table.header != PROPOSAL_HEADER {
            self.rep.add(
                head,
                "the proposals table has the header `| ID | Kind | Proposal | Status |`",
            );
        }
        for (line, row) in &table.rows {
            let at = doc.at(*line);
            let [id, kind, text, status] = row.as_slice() else {
                self.rep.add(at, "a proposal row has four cells");
                continue;
            };
            self.proposal_row_checks(at, codec, [id, kind, text, status]);
        }
    }

    fn proposal_row_checks(&mut self, at: At, codec: &str, [id, kind, text, status]: [&String; 4]) {
        let id_ok = id
            .strip_prefix("P-")
            .and_then(|r| r.split_once('-'))
            .is_some_and(|(c, nn)| {
                c == codec && nn.len() == 2 && nn.bytes().all(|b| b.is_ascii_digit())
            });
        if !id_ok {
            self.rep.add(
                at,
                format!("`{id}` is not `P-{codec}-NN` with this file's codec"),
            );
        }
        let kinds = ["requirement", "oi", "test", "doc", "tooling"];
        if !single_span(kind).is_some_and(|k| kinds.contains(&k)) {
            self.rep.add(
                at,
                format!("{id}: `Kind` is one code span: `requirement`, `oi`, `test`, `doc`, or `tooling`"),
            );
        }
        if text.is_empty() {
            self.rep.add(at, format!("{id}: `Proposal` is empty"));
        }
        let word = status
            .strip_prefix('`')
            .and_then(|s| s.split_once('`'))
            .filter(|(w, _)| ["proposed", "accepted", "rejected"].contains(w));
        let Some((word, rest)) = word else {
            self.rep.add(
                at,
                format!("{id}: `Status` begins with `proposed`, `accepted`, or `rejected` in a code span"),
            );
            return;
        };
        if word != "proposed" && rest.trim().is_empty() {
            self.rep.add(
                at,
                format!("{id}: say where it was written, or why it was rejected"),
            );
        }
        self.proposal_places
            .push((id.clone(), at.line, at.path.to_owned()));
        if self
            .defs
            .proposals
            .insert(id.clone(), word.to_owned())
            .is_some()
        {
            self.rep.add(at, format!("{id} is defined twice"));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The case (§4)

/// A claim of the case.
#[derive(Debug, Clone)]
struct Claim {
    id: String,
    title: String,
    parent: Option<String>,
    leaf: bool,
    todo_rows: usize,
    line: usize,
    evidence: Vec<String>,
    hazards: Vec<String>,
}

/// `AC-1.2 Title` -> (ID, numbers, title).
fn parse_claim_heading(text: &str) -> Option<(String, Vec<u32>, String)> {
    let (id, title) = text.split_once(' ')?;
    let nums: Option<Vec<u32>> = id
        .strip_prefix("AC-")?
        .split('.')
        .map(|n| {
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            n.parse().ok()
        })
        .collect();
    (!title.trim().is_empty()).then_some((id.to_owned(), nums?, title.trim().to_owned()))
}

fn parent_of(nums: &[u32]) -> Option<String> {
    match nums {
        [] | [0] => None,
        [_] => Some("AC-0".to_owned()),
        [init @ .., _] => {
            let parts: Vec<String> = init.iter().map(u32::to_string).collect();
            Some(format!("AC-{}", parts.join(".")))
        }
    }
}

impl Pass<'_> {
    fn case(&mut self, doc: &Doc) {
        let heads = self.claim_headings(doc);
        let claim_lines: Vec<usize> = heads.iter().map(|(k, _)| doc.heads[*k].line).collect();
        for (k, claim) in heads {
            let ends = |h: &&Heading| {
                claim_lines.contains(&h.line) || (h.level == 2 && h.text == ASSUMPTIONS_HEADING)
            };
            let from = doc.heads[k].line;
            let to = doc.heads[k + 1..]
                .iter()
                .find(ends)
                .map_or(doc.lines.len(), |h| h.line - 1);
            self.claim_section(doc, claim, from, to);
        }
        let parents: BTreeSet<Option<String>> =
            self.claims.iter().map(|c| c.parent.clone()).collect();
        for c in &mut self.claims {
            c.leaf = !parents.contains(&Some(c.id.clone()));
        }
        self.assumptions_table(doc);
        self.defeaters_table(doc);
    }

    /// The claim headings with their index in `doc.heads`, checked for form, level, and parent.
    fn claim_headings(&mut self, doc: &Doc) -> Vec<(usize, Claim)> {
        let mut out: Vec<(usize, Claim)> = Vec::new();
        for (k, h) in doc
            .heads
            .iter()
            .enumerate()
            .filter(|(_, h)| h.text.starts_with("AC-"))
        {
            let at = doc.at(h.line);
            let parsed = parse_claim_heading(&h.text).filter(|_| (2..=5).contains(&h.level));
            let Some((id, nums, title)) = parsed else {
                self.rep.add(
                    at,
                    "malformed claim heading, expected `## AC-n <title>`, one more `#` per further number",
                );
                continue;
            };
            let want = if nums.len() == 1 { 2 } else { nums.len() + 1 };
            if h.level != want {
                let level = h.level;
                self.rep.add(
                    at,
                    format!("{id}: the heading takes {want} `#`, found {level}"),
                );
            }
            if !self.defs.ac.insert(id.clone()) {
                self.rep.add(at, format!("{id} is defined twice"));
            }
            let parent = parent_of(&nums);
            if let Some(p) = &parent
                && !out.iter().any(|(_, c)| c.id == *p)
            {
                self.rep
                    .add(at, format!("{id}: the parent {p} is not defined above it"));
            }
            out.push((
                k,
                Claim {
                    id,
                    title,
                    parent,
                    leaf: true,
                    todo_rows: 0,
                    line: h.line,
                    evidence: Vec::new(),
                    hazards: Vec::new(),
                },
            ));
        }
        out
    }

    fn claim_section(&mut self, doc: &Doc, mut claim: Claim, from: usize, to: usize) {
        let at = doc.at(claim.line);
        let id = claim.id.clone();
        let line_with = |prefix: &str| {
            (from..to)
                .filter(|n| !doc.in_fence[*n])
                .filter_map(|n| doc.lines.get(n))
                .find_map(|l| l.trim_start().strip_prefix(prefix).map(str::to_owned))
        };
        if line_with("**Claim.**").is_none_or(|t| t.trim().is_empty()) {
            self.rep
                .add(at, format!("{id}: no `**Claim.**` line with text"));
        }
        let evidence: Vec<Table> = doc
            .tables_in(from, to)
            .into_iter()
            .filter(|t| t.header == EVIDENCE_HEADER)
            .collect();
        let leaf = !doc.heads.iter().any(|h| {
            parse_claim_heading(&h.text)
                .is_some_and(|(_, nums, _)| parent_of(&nums).as_deref() == Some(id.as_str()))
        });
        if leaf {
            match evidence.as_slice() {
                [t] if !t.rows.is_empty() => self.evidence_rows(doc, &mut claim, t),
                [_] => self
                    .rep
                    .add(at, format!("{id}: the evidence table has no row")),
                [] => self.rep.add(
                    at,
                    format!("{id}: a leaf claim has an evidence table `| Evidence | Shows | Requirements |`"),
                ),
                _ => self.rep.add(
                    at,
                    format!("{id}: a leaf claim has exactly one evidence table"),
                ),
            }
        } else {
            if line_with("**Strategy.**").is_none() {
                self.rep.add(at, format!("{id}: no `**Strategy.**` line"));
            }
            if !evidence.is_empty() {
                self.rep.add(
                    at,
                    format!("{id}: a claim with sub-claims has no evidence table"),
                );
            }
        }
        self.claims.push(claim);
    }

    fn evidence_rows(&mut self, doc: &Doc, claim: &mut Claim, table: &Table) {
        for (line, row) in &table.rows {
            let at = doc.at(*line);
            let [evidence, shows, reqs] = row.as_slice() else {
                self.rep.add(at, "an evidence row has three cells");
                continue;
            };
            if evidence_cell(&mut self.rep, at, evidence, true) {
                claim.todo_rows += 1;
                if self.strict {
                    self.rep
                        .add(at, format!("{}: evidence row `TODO` (--strict)", claim.id));
                }
            } else {
                let found = spans(evidence).unwrap_or_default();
                claim.evidence.extend(found.into_iter().map(str::to_owned));
            }
            if shows.is_empty() {
                self.rep.add(at, "`Shows` is empty");
            }
            requirements_cell(&mut self.rep, self.env, at, reqs, false);
        }
    }

    /// The first table after the level-2 heading `heading`, which must have `header`.
    fn table_after(&mut self, doc: &Doc, heading: &str, header: &[&str]) -> Option<Table> {
        let Some(k) = doc.heading(2, heading) else {
            self.rep
                .add(doc.at(1), format!("no `## {heading}` heading"));
            return None;
        };
        let (from, to) = doc.section(k, true);
        match doc.tables_in(from, to).into_iter().next() {
            Some(t) if t.header == header => Some(t),
            _ => {
                let cols = header.join(" | ");
                let msg = format!("`## {heading}` is followed by a table `| {cols} |`");
                self.rep.add(doc.at(doc.heads[k].line), msg);
                None
            }
        }
    }

    fn assumptions_table(&mut self, doc: &Doc) {
        let Some(table) = self.table_after(doc, ASSUMPTIONS_HEADING, &ASSUMPTION_HEADER) else {
            return;
        };
        for (line, row) in &table.rows {
            let at = doc.at(*line);
            let [id, assumption, basis, _] = row.as_slice() else {
                self.rep.add(at, "an assumption row has four cells");
                continue;
            };
            let ok = id.split_once('-').is_some_and(|(p, n)| {
                (p == "ASM" || p == "CTX") && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
            });
            if !ok {
                self.rep
                    .add(at, format!("`{id}` is not `ASM-n` or `CTX-n`"));
            }
            if !self.defs.asm.insert(id.clone()) {
                self.rep.add(at, format!("{id} is defined twice"));
            }
            if assumption.is_empty() || basis.is_empty() {
                self.rep
                    .add(at, format!("{id}: `Assumption` and `Basis` are not empty"));
            }
            self.assumptions.push((id.clone(), *line));
        }
    }

    fn defeaters_table(&mut self, doc: &Doc) {
        let Some(table) = self.table_after(doc, "Defeaters", &DEFEATER_HEADER) else {
            return;
        };
        for (line, row) in &table.rows {
            let at = doc.at(*line);
            let [id, _, weakens, refs, status] = row.as_slice() else {
                self.rep.add(at, "a defeater row has five cells");
                continue;
            };
            let ok = id
                .strip_prefix("DF-")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if !ok {
                self.rep.add(at, format!("`{id}` is not `DF-n`"));
            }
            if !self.defs.df.insert(id.clone()) {
                self.rep.add(at, format!("{id} is defined twice"));
            }
            if ref_list(&mut self.rep, at, weakens, "ac").is_empty() {
                self.rep.add(
                    at,
                    format!("{id}: `Weakens` names at least one `ac:` claim"),
                );
            }
            evidence_cell(&mut self.rep, at, refs, false);
            match single_span(status) {
                Some("open") => self.open_defeater(at, id, refs),
                Some("closed") => {}
                _ => self
                    .rep
                    .add(at, format!("{id}: `Status` is `open` or `closed`")),
            }
        }
    }

    fn open_defeater(&mut self, at: At, id: &str, refs: &str) {
        for span in spans(refs).unwrap_or_default() {
            if let Some(("oi", oi)) = parse_ref(span)
                && self
                    .env
                    .ois
                    .get(oi)
                    .is_some_and(|s| s.starts_with("Resolved"))
            {
                self.rep
                    .add(at, format!("{id} is open but {oi} is resolved"));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The whole check

/// An analysed file.
struct Source {
    path: String,
    /// The codec of a hazard file, `None` for the case.
    codec: Option<&'static str>,
    text: String,
}

/// What the check found.
struct Analysis {
    problems: Vec<String>,
    hazards: Vec<Hazard>,
    claims: Vec<Claim>,
    refs: Vec<Reference>,
}

fn analyse(env: &Env, sources: &[Source], strict: bool) -> Analysis {
    let mut pass = Pass {
        env,
        strict,
        rep: Report::default(),
        defs: Defs::default(),
        hazards: Vec::new(),
        claims: Vec::new(),
        named_proposals: BTreeSet::new(),
        assumptions: Vec::new(),
        proposal_places: Vec::new(),
    };
    let mut refs = Vec::new();
    for s in sources {
        let doc = Doc::new(&s.path, &s.text);
        match s.codec {
            Some(codec) => pass.hazard_file(&doc, codec),
            None => pass.case(&doc),
        }
        refs.extend(scan_references(&doc));
    }
    cross_checks(&mut pass, &refs);
    for h in &pass.hazards {
        for c in &h.claims {
            if let Some(claim) = pass.claims.iter_mut().find(|x| x.id == *c) {
                claim.hazards.push(h.id.clone());
            }
        }
    }
    Analysis {
        problems: pass.rep.problems,
        hazards: pass.hazards,
        claims: pass.claims,
        refs,
    }
}

/// What needs every file: references, gaps, proposals nobody names, assumptions nobody names.
fn cross_checks(pass: &mut Pass, refs: &[Reference]) {
    for r in refs {
        if let Err(e) = pass.env.resolve(&pass.defs, &r.kind, &r.target) {
            let at = At {
                path: &r.file,
                line: r.line,
            };
            pass.rep.add(at, format!("`{}:{}`: {e}", r.kind, r.target));
        }
    }
    for h in pass.hazards.iter().filter(|h| h.status == "gap") {
        let proposed = h
            .proposals
            .iter()
            .any(|p| pass.defs.proposals.get(p).is_some_and(|s| s == "proposed"));
        if !h.proposals.is_empty() && !proposed {
            let at = At {
                path: &h.file,
                line: h.line,
            };
            let msg = format!(
                "{}: every proposal of this gap is decided, so re-rate it",
                h.id
            );
            pass.rep.add(at, msg);
        }
    }
    for (id, line, file) in pass.proposal_places.clone() {
        if !pass.named_proposals.contains(&id) {
            let at = At { path: &file, line };
            pass.rep.add(at, format!("{id} is named by no hazard"));
        }
    }
    let named: BTreeSet<&str> = refs
        .iter()
        .filter(|r| r.kind == "asm")
        .map(|r| r.target.as_str())
        .collect();
    for (id, line) in &pass.assumptions {
        if !named.contains(id.as_str()) {
            let at = At {
                path: CASE,
                line: *line,
            };
            pass.rep
                .add(at, format!("{id} is named by no `asm:` reference"));
        }
    }
}

fn load_sources() -> Result<(Vec<Source>, Vec<String>), String> {
    let root = repo::root();
    let mut sources = vec![Source {
        path: HAZARDS.to_owned(),
        codec: Some("X"),
        text: repo::read(&root.join(HAZARDS))?,
    }];
    let mut problems = Vec::new();
    for path in repo::files(HAZARD_DIR, &["md"]) {
        let rel = repo::rel(&path);
        let name = rel.rsplit('/').next().unwrap_or_default();
        match FILE_CODECS.iter().find(|(f, _)| *f == name) {
            Some((_, codec)) => sources.push(Source {
                path: rel.clone(),
                codec: Some(codec),
                text: repo::read(&path)?,
            }),
            None => problems.push(format!("{rel}:1: not a codec file (tm, tc, aos, uslp)")),
        }
    }
    sources.push(Source {
        path: CASE.to_owned(),
        codec: None,
        text: repo::read(&root.join(CASE))?,
    });
    Ok((sources, problems))
}

fn outputs(a: &Analysis) -> Result<(), String> {
    let hazards: Vec<_> = a
        .hazards
        .iter()
        .map(|h| {
            json!({"id": h.id, "file": h.file, "line": h.line, "category": h.category,
                "status": h.status, "severity": h.severity, "claims": h.claims,
                "proposals": h.proposals})
        })
        .collect();
    let claims: Vec<_> = a
        .claims
        .iter()
        .map(|c| {
            json!({"id": c.id, "parent": c.parent, "leaf": c.leaf,
                "todo_rows": c.todo_rows, "hazards": c.hazards})
        })
        .collect();
    let gh: Vec<_> = a
        .refs
        .iter()
        .filter(|r| r.kind == "gh")
        .map(|r| json!({"target": r.target, "file": r.file, "line": r.line}))
        .collect();
    let body = json!({"hazards": hazards, "claims": claims, "gh": gh});
    let mut md = String::from(
        "# Assurance case: claims, hazards, and evidence\n\nGenerated by `cargo xtask assurance`.\n\n\
         | Claim | Title | Hazards | Evidence |\n|---|---|---|---|\n",
    );
    for c in &a.claims {
        let list = |v: &[String]| {
            if v.is_empty() {
                "—".to_owned()
            } else {
                v.join("; ")
            }
        };
        let evidence = list(&c.evidence).replace('|', "\\|");
        let _ = writeln!(
            md,
            "| {} | {} | {} | {evidence} |",
            c.id,
            c.title,
            list(&c.hazards)
        );
    }
    let dir = repo::root().join("target/spec");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let pretty = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("assurance.json"), pretty + "\n").map_err(|e| e.to_string())?;
    std::fs::write(dir.join("assurance.md"), md).map_err(|e| e.to_string())
}

/// `xtask assurance [--strict]`.
pub fn run(strict: bool) -> Result<usize, String> {
    let env = Env::load(Spec::load()?)?;
    let (sources, mut problems) = load_sources()?;
    let a = analyse(&env, &sources, strict);
    problems.extend(a.problems.iter().cloned());
    outputs(&a)?;
    for p in &problems {
        println!("error: {p}");
    }
    let todo = a.hazards.iter().filter(|h| h.status == "todo").count();
    let leaves = a.claims.iter().filter(|c| c.leaf).count();
    let rows: usize = a.claims.iter().map(|c| c.todo_rows).sum();
    let gh = a.refs.iter().filter(|r| r.kind == "gh").count();
    println!(
        "assurance: {} hazards ({todo} todo), {} claims ({leaves} leaves, {rows} TODO rows), \
         {} references ({gh} gh, unchecked), {} problem(s)",
        a.hazards.len(),
        a.claims.len(),
        a.refs.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Requirement;

    const RUST: &str = r#"
#[test]
fn plain() {}
fn not_a_test() {}
#[test]
#[ignore = "slow"]
fn slow() {}
#[test]
pub(crate) fn vis() {}
#[test]
pub const unsafe fn modifiers() {}
proptest! {
    #![proptest_config(ProptestConfig::default())]
    #[test]
    fn prop(x in 0..3) {}
    #[test]
    #[ignore]
    fn prop_slow(x in 0..3) {}
}
macro_rules! vtest {
    ($name:ident) => { #[test] fn $name() {} };
}
vtest!(by_macro);
macro_rules! itest {
    ($name:ident) => { #[test] #[ignore] fn $name() {} };
}
itest!(by_ignored_macro);
mod a { #[test] fn dup() {} }
mod b { #[test] #[ignore] fn dup() {} }
#[kani::proof]
fn proves() {}
"#;

    fn requirement(id: &str, text: &str) -> Requirement {
        let (area, n) = id.split_once('-').unwrap();
        Requirement {
            id: id.to_owned(),
            area: area.to_owned(),
            number: n.parse().unwrap(),
            doc: "x.md".to_owned(),
            line: 1,
            text: text.to_owned(),
            source: "s".to_owned(),
            verif: "T".to_owned(),
        }
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn env() -> Env {
        let spec = Spec {
            areas: BTreeMap::from([("TM".to_owned(), vec!["x.md".to_owned()])]),
            requirements: ["TM-001", "TM-002", "TM-003"]
                .iter()
                .map(|id| {
                    let text = if *id == "TM-003" {
                        "Withdrawn: superseded 2026"
                    } else {
                        "The frame"
                    };
                    ((*id).to_owned(), requirement(id, text))
                })
                .collect(),
            error_kinds: BTreeSet::new(),
            problems: Vec::new(),
        };
        let texts = BTreeMap::from([("p/b.rs".to_owned(), RUST.to_owned())]);
        Env {
            spec,
            files: set(&["a.rs", "docs/x.md", "p/b.rs", "p/c.rs"]),
            read: Box::new(move |p| texts.get(p).cloned()),
            fn_cache: RefCell::new(BTreeMap::new()),
            vectors: BTreeMap::from([("v-live".to_owned(), false), ("v-old".to_owned(), true)]),
            records: set(&["docs/verification/records/VER-011.md"]),
            checks: BTreeMap::from([
                ("stack".to_owned(), (true, false)),
                ("kani".to_owned(), (false, true)),
                ("rust-sparc-leon3".to_owned(), (false, false)),
            ]),
            ci: set(&["loops", "trace --scope m2 --strict"]),
            jobs: set(&["bit-flips"]),
            fuzz: set(&["tc_decode"]),
            ois: BTreeMap::from([
                ("OI-45".to_owned(), "Open".to_owned()),
                ("OI-07".to_owned(), "Resolved".to_owned()),
            ]),
        }
    }

    fn defs() -> Defs {
        Defs {
            hz: set(&["HZ-TC-01"]),
            ac: set(&["AC-1"]),
            asm: set(&["ASM-1", "CTX-1"]),
            df: set(&["DF-1"]),
            proposals: BTreeMap::from([("P-TC-01".to_owned(), "proposed".to_owned())]),
        }
    }

    fn ok(kind: &str, target: &str) -> bool {
        env().resolve(&defs(), kind, target).is_ok()
    }

    fn none() -> Vec<String> {
        Vec::new()
    }

    fn has(problems: &[String], part: &str) -> bool {
        problems.iter().any(|p| p.contains(part))
    }

    // req: VER-066
    #[test]
    fn every_kind_resolves_what_exists_and_rejects_what_does_not() {
        let cases = [
            ("file", "docs/x.md", "docs/missing.md"),
            ("test", "p/b.rs::plain", "p/b.rs::missing"),
            ("nightly-test", "p/b.rs::slow", "p/b.rs::plain"),
            ("proof", "p/b.rs::proves", "p/b.rs::plain"),
            ("vector", "v-live", "v-none"),
            ("superseded-vector", "v-old", "v-none"),
            ("record", "VER-011", "VER-012"),
            ("check", "stack", "nope"),
            ("nightly-check", "kani", "nope"),
            ("ci", "trace --scope m2 --strict", "trace --scope m9"),
            ("job", "bit-flips", "nope"),
            ("fuzz", "tc_decode", "nope"),
            ("oi", "OI-45", "OI-99"),
            ("gh", "#114", "114"),
            ("hz", "HZ-TC-01", "HZ-TC-02"),
            ("ac", "AC-1", "AC-9"),
            ("asm", "CTX-1", "ASM-9"),
            ("df", "DF-1", "DF-9"),
            ("proposal", "P-TC-01", "P-TC-09"),
        ];
        assert_eq!(cases.len(), KINDS.len());
        for (kind, good, bad) in cases {
            assert!(ok(kind, good), "{kind}:{good}");
            assert!(!ok(kind, bad), "{kind}:{bad}");
        }
        // A path is taken as written: no `..`, no leading `/`, no `#`, not a directory.
        for bad in ["../a.rs", "/a.rs", "a.rs#x", "docs", "docs/"] {
            assert!(!ok("file", bad), "{bad}");
        }
        assert!(!ok("test", "docs/x.md::plain"), "not a Rust path");
        assert!(!ok("test", "p/c.rs::plain"), "a file without the test");
        assert!(!ok("test", "p/b.rs"), "no name");
        assert!(!ok("nope", "x"));
    }

    // req: VER-066
    #[test]
    fn tests_are_found_by_attribute_property_test_and_macro() {
        for name in ["plain", "vis", "modifiers", "prop", "by_macro"] {
            assert!(ok("test", &format!("p/b.rs::{name}")), "{name}");
            assert!(!ok("nightly-test", &format!("p/b.rs::{name}")), "{name}");
        }
        for name in ["slow", "prop_slow", "by_ignored_macro"] {
            assert!(!ok("test", &format!("p/b.rs::{name}")), "{name}");
            assert!(ok("nightly-test", &format!("p/b.rs::{name}")), "{name}");
        }
        // A function without `#[test]`, a macro variable, and a macro definition are no tests.
        for name in ["not_a_test", "vtest", "proves"] {
            assert!(!ok("test", &format!("p/b.rs::{name}")), "{name}");
        }
        for kind in ["test", "nightly-test"] {
            let err = env().resolve(&defs(), kind, "p/b.rs::dup").unwrap_err();
            assert!(err.contains("both ignored and not ignored"), "{err}");
        }
        let err = env().resolve(&defs(), "test", "p/b.rs::slow").unwrap_err();
        assert!(err.contains("nightly-test"), "{err}");
    }

    // req: VER-066
    #[test]
    fn a_proof_is_a_kani_proof_and_not_a_test() {
        assert!(ok("proof", "p/b.rs::proves"));
        assert!(!ok("proof", "p/b.rs::plain"));
        assert!(!ok("test", "p/b.rs::proves"));
    }

    // req: VER-066
    #[test]
    fn a_superseded_vector_is_not_live() {
        let live = env().resolve(&defs(), "vector", "v-old").unwrap_err();
        assert!(live.contains("superseded"), "{live}");
        let old = env()
            .resolve(&defs(), "superseded-vector", "v-live")
            .unwrap_err();
        assert!(old.contains("not superseded"), "{old}");
    }

    // req: VER-066
    #[test]
    fn nightly_checks_and_gating_checks_are_not_mixed_up() {
        assert!(ok("check", "stack"));
        assert!(!ok("check", "kani"));
        assert!(!ok("check", "rust-sparc-leon3"));
        assert!(ok("nightly-check", "rust-sparc-leon3"));
        assert!(!ok("nightly-check", "stack"));
    }

    // req: VER-066
    #[test]
    fn what_is_a_reference_and_what_an_evidence_cell_may_hold() {
        assert_eq!(parse_ref("core::fmt"), None);
        assert_eq!(parse_ref("stack"), None);
        assert_eq!(parse_ref("record:VER-011"), Some(("record", "VER-011")));
        assert_eq!(
            parse_ref("ci:trace --scope m2"),
            Some(("ci", "trace --scope m2"))
        );
        assert_eq!(parse_ref("Record:VER-011"), None);
        assert_eq!(parse_ref("a_b:x"), None);
        assert_eq!(parse_ref("record:"), None);
        let at = At {
            path: "f.md",
            line: 3,
        };
        let cell = |text: &str, todo: bool| {
            let mut rep = Report::default();
            let is_todo = evidence_cell(&mut rep, at, text, todo);
            (is_todo, rep.problems)
        };
        assert_eq!(cell("`record:VER-011`; and `file:a.rs`", false).1, none());
        assert_eq!(cell("TODO", true).1, none());
        assert_ne!(cell("TODO", false).1, none());
        assert!(has(&cell("`bogus:x`", false).1, "unknown reference kind"));
        assert!(has(&cell("`stack`", false).1, "not a reference"));
        assert!(has(&cell("`core::fmt`", false).1, "not a reference"));
        assert!(has(
            &cell("`record:VER-011` and `", false).1,
            "odd number of backticks"
        ));
        assert!(has(&cell("none", false).1, "without a reference"));
        // Elsewhere an unknown kind is ignored: no reference is scanned from it.
        let text = "| Failure mode | `bogus:x` and `core::fmt` and `stack` and `file:a.rs` |";
        let refs = scan_references(&Doc::new("f.md", text));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, "file");
        // Fenced code is not scanned.
        assert!(scan_references(&Doc::new("f.md", "```\n`file:a.rs`\n```\n")).is_empty());
    }

    // req: VER-066
    #[test]
    fn requirement_ids_must_be_defined_and_not_deleted() {
        let env = env();
        let at = At {
            path: "f.md",
            line: 3,
        };
        let check = |cell: &str, todo: bool| {
            let mut rep = Report::default();
            requirements_cell(&mut rep, &env, at, cell, todo);
            rep.problems
        };
        let undefined = format!("TM-{}", 777);
        assert_eq!(check("TM-001, TM-002 and TM-001…002", false), none());
        assert!(has(&check(&undefined, false), "not a defined requirement"));
        let range = format!("TM-001…{}", &undefined[3..]);
        assert!(has(&check(&range, false), "not a defined requirement"));
        assert!(has(
            &check("TM-002…001", false),
            "expands to no requirement"
        ));
        assert!(has(&check("TM-003", false), "is deleted"));
        assert!(has(&check("none", false), "names no requirement ID"));
        assert_eq!(check("TODO", true), none());
        assert!(has(&check("TODO", false), "TODO"));
    }

    fn hz(id: &str) -> String {
        format!(
            "### {id} A title\n\n| | |\n|---|---|\n| Category | `corrupt-accepted` |\n\
             | Status | `controlled` |\n| Severity | S1 |\n| Failure mode | f |\n| Cause | c |\n\
             | Mission effect | m |\n| Requirements | TM-001 |\n| Evidence | `file:docs/x.md` |\n\
             | Residual risk | — |\n| Assumptions | `asm:ASM-1` |\n| Proposals | — |\n\
             | Claims | `ac:AC-1` |\n\n"
        )
    }

    fn coverage() -> String {
        let mut s = String::from("## Coverage\n\n| Category | Hazards | State |\n|---|---|---|\n");
        for (i, slug) in SLUGS.iter().enumerate() {
            let row = if i == 0 {
                "HZ-TC-01 | done"
            } else {
                "— | n/a: none"
            };
            let _ = writeln!(s, "| `{slug}` | {row} |");
        }
        s
    }

    fn tc() -> String {
        format!(
            "# TC\n\n{}{}\n## Proposals\n\n| ID | Kind | Proposal | Status |\n|---|---|---|---|\n",
            hz("HZ-TC-01"),
            coverage()
        )
    }

    fn case() -> String {
        "# Case\n\n## AC-0 Top\n\n**Claim.** top\n\n**Strategy.** s\n\n## AC-1 One\n\n\
         **Claim.** one\n\n| Evidence | Shows | Requirements |\n|---|---|---|\n\
         | `file:docs/x.md` | s | TM-001 |\n\n## Assumptions and context\n\n\
         | ID | Assumption | Basis | Integrator obligation |\n|---|---|---|---|\n\
         | ASM-1 | a | b | c |\n\n## Defeaters\n\n\
         | ID | Defeater | Weakens | References | Status |\n|---|---|---|---|---|\n\
         | DF-1 | d | `ac:AC-1` | `oi:OI-45` | `open` |\n"
            .to_owned()
    }

    fn analyse_texts(tc: &str, case: &str, strict: bool) -> Vec<String> {
        let sources = [
            Source {
                path: "hz/tc.md".to_owned(),
                codec: Some("TC"),
                text: tc.to_owned(),
            },
            Source {
                path: "case.md".to_owned(),
                codec: None,
                text: case.to_owned(),
            },
        ];
        analyse(&env(), &sources, strict).problems
    }

    fn tc_problems(text: &str) -> Vec<String> {
        analyse_texts(text, &case(), false)
    }

    fn case_problems(text: &str) -> Vec<String> {
        analyse_texts(&tc(), text, false)
    }

    fn changed(text: &str, from: &str, to: &str) -> String {
        assert!(text.contains(from), "{from}");
        text.replacen(from, to, 1)
    }

    const PROPOSAL_ROW: &str = "| P-TC-01 | `doc` | text | `proposed` |\n";

    // req: VER-066, VER-065
    #[test]
    fn the_fixture_files_are_valid() {
        assert_eq!(analyse_texts(&tc(), &case(), false), Vec::<String>::new());
    }

    // req: VER-066, VER-065
    #[test]
    fn hazard_tables_follow_the_format() {
        let t = tc();
        let p = tc_problems(&changed(&t, "| Cause | c |\n", ""));
        assert!(has(&p, "missing row `Cause`"), "{p:?}");
        let extra = changed(&t, "| Cause | c |\n", "| Cause | c |\n| Extra | e |\n");
        assert!(has(&tc_problems(&extra), "unexpected row `Extra`"));
        let swapped = changed(
            &t,
            "| Failure mode | f |\n| Cause | c |\n",
            "| Cause | c |\n| Failure mode | f |\n",
        );
        assert!(has(&tc_problems(&swapped), "misplaced"));
        let category = changed(&t, "`corrupt-accepted` |\n| Status", "`nope` |\n| Status");
        assert!(has(&tc_problems(&category), "Category"));
        let status = changed(&t, "| Status | `controlled` |", "| Status | `done` |");
        assert!(has(&tc_problems(&status), "Status"));
        let severity = changed(&t, "| Severity | S1 |", "| Severity | S5 |");
        assert!(has(&tc_problems(&severity), "Severity"));
        let empty = changed(&t, "| Cause | c |", "| Cause |  |");
        assert!(has(&tc_problems(&empty), "`Cause` is empty"));
        let heading = changed(&t, "### HZ-TC-01 A", "### HZ-TC-1 A");
        assert!(has(&tc_problems(&heading), "malformed hazard heading"));
    }

    // req: VER-066, VER-065
    #[test]
    fn the_status_decides_what_a_hazard_may_say() {
        let t = tc();
        let gap = changed(&t, "| Status | `controlled` |", "| Status | `gap` |");
        assert!(has(&tc_problems(&gap), "a gap needs a proposal"));
        // A gap names a proposal that is still `proposed`; one that is decided must be re-rated.
        let named = changed(
            &gap,
            "| Proposals | — |",
            "| Proposals | `proposal:P-TC-01` |",
        );
        let open = format!("{named}{PROPOSAL_ROW}");
        assert_eq!(tc_problems(&open), Vec::<String>::new());
        let decided = open.replace("`proposed` |", "`accepted` OI-99 |");
        assert!(has(
            &tc_problems(&decided),
            "every proposal of this gap is decided"
        ));
        let controlled = changed(
            &t,
            "| Proposals | — |",
            "| Proposals | `proposal:P-TC-01` |",
        );
        let p = tc_problems(&format!("{controlled}{PROPOSAL_ROW}"));
        assert!(has(&p, "a controlled hazard has no proposal"), "{p:?}");
        let residual = changed(&t, "| Status | `controlled` |", "| Status | `residual` |");
        assert!(has(&tc_problems(&residual), "states its residual risk"));
        let evidence = changed(&t, "| Evidence | `file:docs/x.md` |", "| Evidence | TODO |");
        assert!(has(&tc_problems(&evidence), "TODO"));
        let claims = changed(&t, "| Claims | `ac:AC-1` |", "| Claims | TODO |");
        assert!(has(&tc_problems(&claims), "`Claims` is `TODO`"));
        let none = changed(&t, "| Claims | `ac:AC-1` |", "| Claims | — |");
        assert!(has(&tc_problems(&none), "names at least one claim"));
        // The status `todo` allows `TODO` in the three rows.
        let todo = changed(&claims, "| Status | `controlled` |", "| Status | `todo` |");
        let todo = changed(
            &todo,
            "| Evidence | `file:docs/x.md` |",
            "| Evidence | TODO |",
        );
        let todo = changed(
            &todo,
            "| Requirements | TM-001 |",
            "| Requirements | TODO |",
        );
        let p = tc_problems(&todo);
        assert!(!has(&p, "TODO"), "{p:?}");
    }

    // req: VER-066, VER-065
    #[test]
    fn hazard_ids_belong_to_their_file_and_increase() {
        let t = tc();
        let p = tc_problems(&t.replace("HZ-TC-01 A", "HZ-TM-01 A"));
        assert!(has(&p, "the codec of this file is `TC`"), "{p:?}");
        let again = changed(&t, "## Coverage", &format!("{}## Coverage", hz("HZ-TC-01")));
        assert!(has(&tc_problems(&again), "HZ-TC-01 is defined twice"));
        let down = changed(&t, "## Coverage", &format!("{}## Coverage", hz("HZ-TC-00")));
        assert!(has(&tc_problems(&down), "must increase"));
        let up = changed(&t, "## Coverage", &format!("{}## Coverage", hz("HZ-TC-07")));
        assert!(!has(&tc_problems(&up), "must increase"), "a gap is allowed");
        // A duplicate across files.
        let sources = [
            Source {
                path: "hz/tc.md".to_owned(),
                codec: Some("TC"),
                text: t.clone(),
            },
            Source {
                path: "hz/other.md".to_owned(),
                codec: Some("TC"),
                text: t,
            },
            Source {
                path: "case.md".to_owned(),
                codec: None,
                text: case(),
            },
        ];
        let p = analyse(&env(), &sources, false).problems;
        assert!(has(&p, "hz/other.md:3: HZ-TC-01 is defined twice"), "{p:?}");
    }

    // req: VER-066, VER-065
    #[test]
    fn coverage_tables_follow_the_format() {
        let t = tc();
        let row = |slug: &str, cells: &str| format!("| `{slug}` | {cells} |");
        let rows = |slug: &str, from: &str, to: &str| changed(&t, &row(slug, from), &row(slug, to));
        let missing = changed(&t, &format!("{}\n", row("memory", "— | n/a: none")), "");
        assert!(has(&tc_problems(&missing), "has 11 rows"));
        assert!(has(&tc_problems(&missing), "row 8 is `memory`"));
        let wrong = changed(&t, "| `valid-rejected` |", "| `misrouting` |");
        assert!(has(&tc_problems(&wrong), "row 2 is `valid-rejected`"));
        let none = rows("valid-rejected", "— | n/a: none", "— | done");
        assert!(has(&tc_problems(&none), "`done` with no hazard"));
        let todo = changed(&t, "| Status | `controlled` |", "| Status | `todo` |");
        assert!(has(
            &tc_problems(&todo),
            "lists a hazard with the status `todo`"
        ));
        let unlisted = rows("corrupt-accepted", "HZ-TC-01 | done", "— | n/a: none");
        assert!(has(
            &tc_problems(&unlisted),
            "is missing from the coverage row `corrupt-accepted`"
        ));
        let elsewhere = rows("valid-rejected", "— | n/a: none", "HZ-TC-01 | done");
        assert!(has(
            &tc_problems(&elsewhere),
            "whose category is `corrupt-accepted`"
        ));
        let unknown = rows("valid-rejected", "— | n/a: none", "HZ-TC-09 | done");
        assert!(has(
            &tc_problems(&unknown),
            "which this file does not define"
        ));
        let no_reason = rows("valid-rejected", "— | n/a: none", "— | n/a:");
        assert!(has(&tc_problems(&no_reason), "the state is `done`"));
        let na_hazard = rows("corrupt-accepted", "HZ-TC-01 | done", "HZ-TC-01 | n/a: x");
        assert!(has(&tc_problems(&na_hazard), "is `n/a` but lists hazards"));
        let heading = changed(&t, "## Coverage", "## Coverag");
        assert!(has(&tc_problems(&heading), "no `## Coverage` heading"));
    }

    // req: VER-066, VER-065
    #[test]
    fn proposals_follow_the_format() {
        let t = tc();
        let unnamed = format!("{t}{PROPOSAL_ROW}");
        assert!(has(&tc_problems(&unnamed), "P-TC-01 is named by no hazard"));
        let codec = format!("{t}{}", PROPOSAL_ROW.replace("P-TC-01", "P-TM-01"));
        assert!(has(&tc_problems(&codec), "with this file's codec"));
        let accepted = format!("{t}{}", PROPOSAL_ROW.replace("`proposed`", "`accepted`"));
        assert!(has(&tc_problems(&accepted), "say where it was written"));
        let kind = format!("{t}{}", PROPOSAL_ROW.replace("`doc`", "`idea`"));
        assert!(has(&tc_problems(&kind), "`Kind`"));
        let status = format!("{t}{}", PROPOSAL_ROW.replace("`proposed`", "proposed"));
        assert!(has(&tc_problems(&status), "`Status` begins with"));
        let empty = format!("{t}{}", PROPOSAL_ROW.replace("| text |", "|  |"));
        assert!(has(&tc_problems(&empty), "`Proposal` is empty"));
        let rejected = format!(
            "{t}{}",
            PROPOSAL_ROW.replace("`proposed`", "`rejected` the OI says no")
        );
        assert!(!has(&tc_problems(&rejected), "say where"));
    }

    // req: VER-066
    #[test]
    fn claims_follow_the_format() {
        let c = case();
        let orphan = changed(
            &c,
            "## Assumptions",
            "#### AC-3.1.1 Orphan\n\n**Claim.** x\n\n## Assumptions",
        );
        let p = case_problems(&orphan);
        assert!(
            has(&p, "the parent AC-3.1 is not defined above it"),
            "{p:?}"
        );
        let level = changed(&c, "## AC-1 One", "### AC-1 One");
        assert!(has(
            &case_problems(&level),
            "the heading takes 2 `#`, found 3"
        ));
        let deep = changed(
            &c,
            "## Assumptions",
            "## AC-1.1 Sub\n\n**Claim.** s\n\n## Assumptions",
        );
        assert!(has(
            &case_problems(&deep),
            "the heading takes 3 `#`, found 2"
        ));
        let table = "| Evidence | Shows | Requirements |\n|---|---|---|\n| `file:docs/x.md` | s | TM-001 |\n";
        let no_table = changed(&c, table, "");
        assert!(has(
            &case_problems(&no_table),
            "AC-1: a leaf claim has an evidence table"
        ));
        let with_table = changed(
            &c,
            "**Strategy.** s\n",
            &format!("**Strategy.** s\n\n{table}"),
        );
        assert!(has(
            &case_problems(&with_table),
            "a claim with sub-claims has no evidence table"
        ));
        let no_claim = changed(&c, "**Claim.** one", "claim one");
        assert!(has(&case_problems(&no_claim), "no `**Claim.**` line"));
        let empty_claim = changed(&c, "**Claim.** top", "**Claim.**");
        assert!(has(&case_problems(&empty_claim), "no `**Claim.**` line"));
        let no_strategy = changed(&c, "**Strategy.** s", "strategy");
        assert!(has(&case_problems(&no_strategy), "no `**Strategy.**` line"));
        let malformed = changed(&c, "AC-1 One", "AC-1");
        assert!(has(&case_problems(&malformed), "malformed claim heading"));
        let dup = changed(
            &c,
            "## Assumptions",
            "## AC-1 Again\n\n**Claim.** x\n\n## Assumptions",
        );
        assert!(has(&case_problems(&dup), "AC-1 is defined twice"));
        let shows = changed(&c, "| s | TM-001 |", "|  | TM-001 |");
        assert!(has(&case_problems(&shows), "`Shows` is empty"));
        let reqs = changed(&c, "| s | TM-001 |", "| s | TODO |");
        assert!(has(&case_problems(&reqs), "Requirements is `TODO`"));
    }

    // req: VER-066
    #[test]
    fn a_todo_row_passes_without_strict_and_fails_with_it() {
        let c = changed(&case(), "| `file:docs/x.md` | s |", "| TODO | s |");
        assert_eq!(analyse_texts(&tc(), &c, false), Vec::<String>::new());
        let p = analyse_texts(&tc(), &c, true);
        assert!(has(&p, "evidence row `TODO` (--strict)"), "{p:?}");
    }

    // req: VER-066
    #[test]
    fn defeaters_and_assumptions_follow_the_format() {
        let c = case();
        let resolved = changed(&c, "`oi:OI-45`", "`oi:OI-07`");
        assert!(has(
            &case_problems(&resolved),
            "DF-1 is open but OI-07 is resolved"
        ));
        let closed = changed(&resolved, "`open`", "`closed`");
        assert_eq!(case_problems(&closed), Vec::<String>::new());
        let status = changed(&c, "`open`", "open");
        assert!(has(&case_problems(&status), "`open` or `closed`"));
        let claim = changed(&c, "`ac:AC-1`", "`ac:AC-9`");
        assert!(has(&case_problems(&claim), "no such claim"));
        let weakens = changed(&c, "`ac:AC-1` |", "text |");
        assert!(has(&case_problems(&weakens), "names at least one `ac:`"));
        let todo = changed(&c, "`oi:OI-45`", "TODO");
        assert!(has(&case_problems(&todo), "TODO"));
        let id = changed(&c, "| DF-1 |", "| D-1 |");
        assert!(has(&case_problems(&id), "is not `DF-n`"));
        let basis = changed(&c, "| ASM-1 | a | b |", "| ASM-1 | a |  |");
        assert!(has(&case_problems(&basis), "`Assumption` and `Basis`"));
        let asm_id = changed(&c, "| ASM-1 |", "| ASX-1 |");
        assert!(has(&case_problems(&asm_id), "is not `ASM-n` or `CTX-n`"));
        // An assumption that no hazard or claim names is a problem.
        let unnamed = changed(
            &tc(),
            "| Assumptions | `asm:ASM-1` |",
            "| Assumptions | — |",
        );
        let p = analyse_texts(&unnamed, &c, false);
        assert!(has(&p, "ASM-1 is named by no `asm:` reference"), "{p:?}");
        let junk = changed(
            &tc(),
            "| Assumptions | `asm:ASM-1` |",
            "| Assumptions | `asm:ASM-1` and also |",
        );
        assert!(has(
            &tc_problems(&junk),
            "only `asm:` references and separators"
        ));
    }

    // req: VER-066, VER-065
    #[test]
    fn strict_mode_counts_every_todo_and_normal_mode_none() {
        let t = tc();
        let t = changed(&t, "| Status | `controlled` |", "| Status | `todo` |");
        let t = changed(&t, "| Evidence | `file:docs/x.md` |", "| Evidence | TODO |");
        let t = changed(
            &t,
            "| `corrupt-accepted` | HZ-TC-01 | done |",
            "| `corrupt-accepted` | HZ-TC-01 | TODO |",
        );
        let c = changed(&case(), "| `file:docs/x.md` | s |", "| TODO | s |");
        assert_eq!(analyse_texts(&t, &c, false), Vec::<String>::new());
        let p = analyse_texts(&t, &c, true);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(has(&p, "status `todo` (--strict)"));
        assert!(has(&p, "`corrupt-accepted` is `TODO` (--strict)"));
        assert!(has(&p, "evidence row `TODO` (--strict)"));
    }

    // req: VER-066, VER-065
    #[test]
    fn the_hazards_and_the_case_of_the_repository_have_no_problem() {
        assert_eq!(run(true), Ok(0));
    }

    // req: VER-066
    #[test]
    fn the_check_runs_in_ci_and_in_the_verify_check_spec() {
        assert!(
            crate::CI
                .iter()
                .any(|(name, _)| *name == "assurance --strict")
        );
        let spec = crate::verify::all_checks()
            .into_iter()
            .find(|c| c.name == "spec")
            .unwrap();
        assert!(spec.script.contains("cargo xtask assurance --strict"));
        assert!(workflow_jobs("name: x\njobs:\n  a-b:\n    runs-on: x\n  c:\n").contains("a-b"));
        assert!(!workflow_jobs("  early:\njobs:\n  a:\n").contains("early"));
    }
}
