//! Which checks a change needs (VER-056): the paths a commit changes against the tip of `main`,
//! matched against the inputs of each check (`checks.rs`); the published records whose results a
//! check that does not run takes; and the re-derivation of a record's skips and reuses that
//! `verify check` and `verify publish` make before they trust it (VER-058).
//!
//! A check does not run when an earlier commit's published passing record shows it passed, and
//! none of its inputs differs between that commit and the commit under test: it is `skipped` when
//! its inputs did not change against the base either (the record is then the one covering the
//! base, or the one that record took the result from), and `reused` otherwise (the record is that
//! of an earlier commit of the branch). The records searched are the one covering the base, then
//! those of the earlier commits of the branch, nearest first; each is followed to the record in
//! which the check actually ran, and that record is the one named.
//!
//! The rule is fail-safe: every check runs when the changed paths are not known (`origin/main`
//! cannot be fetched, there is no merge base, the commit is on `main`, nothing differs), or when
//! no published passing record covers the base or an earlier commit of the branch. A record is
//! not used for any check when a path that differs from its commit is an input of no check and
//! in none of the lists that `spec` alone reads (`SPEC_ONLY`: documentation, the GitHub
//! configuration, and the root configuration that no other check reads).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::rc::Rc;

use serde_json::{Map, Value, json};

use super::checks::{Check, DOCS, GITHUB, Inputs, NIGHTLY_ONLY, ROOT_CONFIG, SPEC_ONLY};
use super::record::Outcome;
use super::{git_bytes, git_in, publish, record};

/// At most this many changed paths are listed in a record.
const LISTED: usize = 100;
/// At most this many earlier commits of the branch are searched for records (VER-056).
const BRANCH_DEPTH: usize = 20;
/// At most this many records are followed from a record to the one in which a check ran.
const ORIGIN_DEPTH: usize = 20;

/// Whether `s` is a full object name, never an option or a revision expression.
pub(super) fn is_hash(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `path` matches `pattern`: `*` matches any characters within one path segment, `**`
/// any number of whole segments.
pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    segments(&pattern, &path)
}

fn segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            (0..=path.len()).any(|i| path.get(i..).is_some_and(|p| segments(rest, p)))
        }
        Some((first, rest)) => path.split_first().is_some_and(|(seg, tail)| {
            wildcard(first.as_bytes(), seg.as_bytes()) && segments(rest, tail)
        }),
    }
}

fn wildcard(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            (0..=text.len()).any(|i| text.get(i..).is_some_and(|t| wildcard(rest, t)))
        }
        Some((c, rest)) => text
            .split_first()
            .is_some_and(|(d, tail)| c == d && wildcard(rest, tail)),
    }
}

/// The tip of `main` that a change is measured against.
#[derive(Clone, Copy, Debug)]
pub enum Main<'a> {
    /// `origin/main`, fetched first.
    Origin,
    /// A given revision, for `verify scope --main` on past commits; nothing is fetched.
    Rev(&'a str),
}

/// The paths a commit changes against the tip of `main` (VER-056), or why they are not known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The merge base with the tip of `main`, when there is one.
    pub base: Option<String>,
    /// The changed paths, both sides of a rename and deleted paths included; or why they are
    /// not known, in which case every check runs.
    pub paths: Result<Vec<String>, String>,
}

/// The change of `commit` against the merge base with the tip of `main`.
pub fn change(dir: &Path, commit: &str, main: Main) -> Change {
    let unknown = |base: Option<&str>, why: &str| Change {
        base: base.map(str::to_owned),
        paths: Err(why.to_owned()),
    };
    let tip = match main {
        Main::Origin => {
            if git_in(dir, &["fetch", "-q", "origin", "main"]).is_err() {
                return unknown(None, "origin/main could not be fetched");
            }
            "origin/main"
        }
        Main::Rev(rev) => rev,
    };
    let Ok(base) = git_in(dir, &["merge-base", commit, tip]) else {
        return unknown(None, &format!("no merge base with {tip}"));
    };
    let Ok(commit) = git_in(
        dir,
        &["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
    ) else {
        return unknown(Some(&base), &format!("{commit} is not a commit"));
    };
    if base == commit {
        return unknown(Some(&base), "the commit is on main");
    }
    Change {
        paths: changed_paths(dir, &base, &commit),
        base: Some(base),
    }
}

/// The change of `commit` against `base`, a base read from a record: checked to be on
/// `origin/main` (fetched first) and an ancestor of `commit`.
pub fn change_from(dir: &Path, commit: &str, base: &str) -> Change {
    let unknown = |why: String| Change {
        base: Some(base.to_owned()),
        paths: Err(why),
    };
    if !is_hash(base) || !is_hash(commit) {
        return unknown(format!("the base {base} is not a full commit hash"));
    }
    if git_in(dir, &["fetch", "-q", "origin", "main"]).is_err() {
        return unknown("origin/main could not be fetched".to_owned());
    }
    if git_in(
        dir,
        &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")],
    )
    .is_err()
    {
        return unknown(format!("the base {base} is not a commit"));
    }
    if git_in(dir, &["merge-base", "--is-ancestor", base, "origin/main"]).is_err() {
        return unknown(format!("the base {base} is not on origin/main"));
    }
    if git_in(dir, &["merge-base", "--is-ancestor", base, commit]).is_err() {
        return unknown(format!("the base {base} is not an ancestor of {commit}"));
    }
    if git_in(dir, &["rev-parse", base]).ok().as_deref() == Some(commit) {
        return unknown("the commit is on main".to_owned());
    }
    Change {
        base: Some(base.to_owned()),
        paths: changed_paths(dir, base, commit),
    }
}

/// The changed paths from `base` to `commit` ([`diff_paths`]). An empty list is treated as
/// unknown: a commit that changes nothing against main has nothing to gain from the rule.
fn changed_paths(dir: &Path, base: &str, commit: &str) -> Result<Vec<String>, String> {
    let paths = diff_paths(dir, base, commit)?;
    if paths.is_empty() {
        return Err(format!("no path differs from main at {base}"));
    }
    Ok(paths)
}

/// `git diff --name-only` from `from` to `to`. Without renames, so that both sides of a rename
/// are listed, and with NUL separators, so that no path is quoted. `diff.relative` is switched
/// off by configuration rather than `--no-relative`, which needs git 2.28; the nightly
/// big-endian job runs the tests in a `cross` image with an older git.
fn diff_paths(dir: &Path, from: &str, to: &str) -> Result<Vec<String>, String> {
    let out = git_bytes(
        dir,
        &[
            "-c",
            "diff.relative=false",
            "diff",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            "-z",
            from,
            to,
        ],
    )
    .map_err(|e| format!("the changed paths are not known: {e}"))?;
    out.split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            String::from_utf8(p.to_vec()).map_err(|_| "a changed path is not UTF-8".to_owned())
        })
        .collect()
}

/// What `--all` and `--all-cpp` force to run.
#[derive(Clone, Copy, Debug, Default)]
pub struct Force {
    /// Every check (`--all`).
    pub all: bool,
    /// Every C++ check (`--all-cpp`).
    pub cpp: bool,
}

/// The record a check that does not run takes its result from (VER-056, VER-057): the published
/// record of an earlier commit, in which the check ran and passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// The commit of that record.
    pub commit: String,
    /// The SHA-256 of the record, as published.
    pub record: String,
}

impl Source {
    /// The `source` object of a result entry.
    pub fn to_json(&self) -> Value {
        json!({"commit": self.commit, "record": self.record})
    }
}

/// A record a check's result may be taken from, with the paths that differ between its commit
/// and the commit under test, or why they are not known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    /// The record.
    pub source: Source,
    /// The paths that differ from its commit to the commit under test.
    pub paths: Result<Vec<String>, String>,
}

/// The published records a commit may take check results from (VER-056).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    /// The commit whose published passing record covers the base, if one does.
    pub base_record: Option<String>,
    /// The earlier commits of the branch that have a published passing record, nearest first.
    pub branch: Vec<String>,
    /// For each check, the records in which it ran and passed that these records lead to: the
    /// base's first, then the branch's, nearest first.
    pub origins: BTreeMap<String, Vec<Origin>>,
    /// Why there are no records at all, when there are none; every check then runs.
    pub none: Option<String>,
}

/// What the rule decides for one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// The check.
    pub name: String,
    /// Whether it runs.
    pub run: bool,
    /// Why.
    pub why: String,
    /// When it does not run: the record its result is taken from.
    pub source: Option<Source>,
    /// When it does not run: whether its inputs changed against the base, so that its result is
    /// reused from an earlier commit of the branch rather than skipped.
    pub reused: bool,
}

impl Decision {
    /// The result recorded for a check that does not run (VER-057); `None` when it runs. A check
    /// that does not run and takes no result from a source is a nightly-only check outside the
    /// nightly run: not run.
    pub const fn outcome(&self) -> Option<Outcome> {
        match (self.run, self.source.is_some(), self.reused) {
            (true, _, _) => None,
            (false, false, _) => Some(Outcome::NotRun),
            (false, true, false) => Some(Outcome::Skipped),
            (false, true, true) => Some(Outcome::Reused),
        }
    }

    /// The decision for the nightly-only check `name` in a run without `--all` (VER-059).
    fn nightly(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            run: false,
            why: NIGHTLY_ONLY.to_owned(),
            source: None,
            reused: false,
        }
    }
}

/// What the rule decides for a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The merge base with the tip of `main`.
    pub base: Option<String>,
    /// The commit whose published passing record covers the base, if one does.
    pub base_record: Option<String>,
    /// The earlier commits of the branch with a published passing record, nearest first.
    pub branch: Vec<String>,
    /// The changed paths, when they are known.
    pub changed: Vec<String>,
    /// Why every check runs, when the rule is not applied.
    pub every: Option<String>,
    /// One decision per check, in table order.
    pub checks: Vec<Decision>,
}

/// The reason recorded for a check the rule skips.
pub fn skip_reason(base: &str) -> String {
    format!("none of its inputs changed against main at {base} (VER-056)")
}

/// The reason recorded for a check whose result the rule reuses.
pub fn reuse_reason(source: &Source) -> String {
    format!(
        "its inputs are those of {}, in whose record it passed (VER-056)",
        source.commit
    )
}

/// Whether some check reads `path`, or it is in a list that `spec` alone reads
/// ([`SPEC_ONLY`]).
fn listed(checks: &[Check], path: &str) -> bool {
    SPEC_ONLY.iter().any(|l| l.contains(path))
        || checks
            .iter()
            .any(|c| c.inputs != Inputs::Everything && c.inputs.contains(path))
}

/// Why the inputs of `check` may differ between two commits whose differing paths are `paths`;
/// `None` when they are identical. A path that no check reads and that is not documentation may
/// be read by any check, so it makes every check's inputs differ.
fn differs(checks: &[Check], check: &Check, paths: &[String]) -> Option<String> {
    if let Some(p) = paths.iter().find(|p| !listed(checks, p)) {
        return Some(format!(
            "{p} is an input of no check, and neither documentation nor configuration that \
             `spec` alone reads"
        ));
    }
    paths
        .iter()
        .find(|p| check.inputs.contains(p))
        .map(|p| format!("changes {p}"))
}

/// Applies the rule to `change`. `sources` finds the published records the commit may take
/// results from, given the base; it is asked only when the rule applies.
pub fn plan(
    checks: &[Check],
    change: &Change,
    force: Force,
    sources: impl FnOnce(&str) -> Sources,
) -> Plan {
    let every = |why: String, changed: Vec<String>| Plan {
        base: change.base.clone(),
        base_record: None,
        branch: Vec::new(),
        changed,
        checks: checks
            .iter()
            .map(|c| {
                if c.nightly && !force.all {
                    return Decision::nightly(&c.name);
                }
                Decision {
                    name: c.name.clone(),
                    run: true,
                    why: format!("every check runs: {why}"),
                    source: None,
                    reused: false,
                }
            })
            .collect(),
        every: Some(why),
    };
    let (paths, base) = match (&change.paths, &change.base) {
        (Err(why), _) => return every(why.clone(), Vec::new()),
        (Ok(_), None) => return every("no merge base".to_owned(), Vec::new()),
        (Ok(paths), Some(base)) => (paths.clone(), base.clone()),
    };
    if force.all {
        return every("--all".to_owned(), paths);
    }
    let found = sources(&base);
    if let Some(why) = found.none {
        return every(why, paths);
    }
    let decisions = checks
        .iter()
        .map(|c| {
            // A nightly-only check is not run, whatever its inputs; with --all, `every` ran it.
            if c.nightly {
                return Decision::nightly(&c.name);
            }
            let against_base = differs(checks, c, &paths);
            let origin = found.origins.get(&c.name).and_then(|origins| {
                origins.iter().find(|o| {
                    o.paths
                        .as_ref()
                        .is_ok_and(|p| differs(checks, c, p).is_none())
                })
            });
            let forced = c.cpp && force.cpp;
            match origin {
                Some(o) if !forced => Decision {
                    name: c.name.clone(),
                    run: false,
                    why: if against_base.is_some() {
                        reuse_reason(&o.source)
                    } else {
                        skip_reason(&base)
                    },
                    source: Some(o.source.clone()),
                    reused: against_base.is_some(),
                },
                _ => Decision {
                    name: c.name.clone(),
                    run: true,
                    why: if forced {
                        "--all-cpp".to_owned()
                    } else {
                        against_base.unwrap_or_else(|| {
                            "its inputs did not change against main, but no published record \
                             in which it ran and passed has the same inputs"
                                .to_owned()
                        })
                    },
                    source: None,
                    reused: false,
                },
            }
        })
        .collect();
    Plan {
        base: Some(base),
        base_record: found.base_record,
        branch: found.branch,
        changed: paths,
        every: None,
        checks: decisions,
    }
}

/// The tree of `commit`.
fn tree_of(dir: &Path, commit: &str) -> Option<String> {
    git_in(
        dir,
        &["rev-parse", "--verify", "-q", &format!("{commit}^{{tree}}")],
    )
    .ok()
}

/// A record read from `origin`'s notes, with the SHA-256 of its published text.
type Read = Option<(Rc<Value>, String)>;

/// `origin`'s published records, fetched once and each read and checked once.
struct Notes<'a> {
    dir: &'a Path,
    seen: HashMap<String, Read>,
}

impl<'a> Notes<'a> {
    /// Fetches `origin`'s notes; `None` when it has none or they cannot be fetched.
    fn fetch(dir: &'a Path) -> Option<Self> {
        publish::fetch_notes(dir)
            .ok()
            .filter(|fetched| *fetched)
            .map(|_| Self {
                dir,
                seen: HashMap::new(),
            })
    }

    /// The published record of `commit` and its SHA-256, when it names that commit and its tree
    /// and passes the merge gate on its own (`record::gate_problems`).
    fn passing(&mut self, commit: &str) -> Read {
        if let Some(read) = self.seen.get(commit) {
            return read.clone();
        }
        let read = self.read(commit);
        self.seen.insert(commit.to_owned(), read.clone());
        read
    }

    fn read(&self, commit: &str) -> Read {
        if !is_hash(commit) {
            return None;
        }
        let tree = tree_of(self.dir, commit)?;
        let text = publish::published(self.dir, commit)?;
        let rec: Value = serde_json::from_str(text.trim()).ok()?;
        let field = |k: &str| rec.get(k).and_then(Value::as_str);
        let ok = field("commit") == Some(commit)
            && field("tree") == Some(tree.as_str())
            && record::gate_problems(&rec).is_empty();
        ok.then(|| (Rc::new(rec), record::sha256_hex(text.trim().as_bytes())))
    }

    /// The commit whose published passing record covers `base`: `base` itself, or a parent of
    /// it with the same tree, as the head of the pull request that a merge commit on `main`
    /// merged when the branch was up to date (VER-058).
    fn cover(&mut self, base: &str) -> Option<String> {
        if !is_hash(base) {
            return None;
        }
        let tree = tree_of(self.dir, base)?;
        let line = git_in(self.dir, &["rev-list", "--parents", "-n", "1", base]).ok()?;
        line.split_whitespace()
            .find(|c| {
                tree_of(self.dir, c).as_deref() == Some(tree.as_str()) && self.passing(c).is_some()
            })
            .map(str::to_owned)
    }

    /// The record in which the check `name` ran and passed, reached from the record of `commit`:
    /// that record when it passed there; else, when it was skipped or reused there, the record
    /// that record took it from (format 3: the source it names; format 2: the record it names as
    /// covering its base; format 1: the record covering its base now), followed at most `depth`
    /// more times.
    fn origin(&mut self, commit: &str, name: &str, depth: usize) -> Option<Source> {
        let (rec, sha) = self.passing(commit)?;
        let entry = rec
            .get("checks")?
            .as_array()?
            .iter()
            .find(|e| e.get("name").and_then(Value::as_str) == Some(name))?;
        let result = entry.get("result").and_then(Value::as_str)?;
        if result == "pass" {
            return Some(Source {
                commit: commit.to_owned(),
                record: sha,
            });
        }
        let next = match (rec.get("format").and_then(Value::as_str)?, result) {
            (record::FORMAT, "skipped" | "reused") => {
                entry.get("source")?.get("commit")?.as_str()?.to_owned()
            }
            (record::FORMAT_2, "skipped") => {
                rec.get("scope")?.get("base_record")?.as_str()?.to_owned()
            }
            (record::FORMAT_1, "skipped") => {
                let base = rec.get("cpp")?.get("base")?.as_str()?.to_owned();
                self.cover(&base)?
            }
            _ => return None,
        };
        self.origin(&next, name, depth.checked_sub(1)?)
    }
}

/// The commit whose published passing record (on `origin`) covers `base` (VER-058).
pub fn covering_record(dir: &Path, base: &str) -> Result<String, String> {
    Notes::fetch(dir)
        .and_then(|mut notes| notes.cover(base))
        .ok_or_else(|| format!("no published passing record covers main at {base}"))
}

/// The paths that differ from `from` to `to`, when `from` is an earlier commit than `to`, that
/// is an ancestor other than itself.
fn earlier_diff(dir: &Path, from: &str, to: &str) -> Result<Vec<String>, String> {
    if from == to || git_in(dir, &["merge-base", "--is-ancestor", from, to]).is_err() {
        return Err(format!("{from} is not an ancestor of {to}"));
    }
    diff_paths(dir, from, to)
}

/// The published records that `commit` may take check results from (VER-056): the record that
/// covers `base`, then those of the earlier commits of the branch, the first-parent ancestors of
/// `commit` that are not on `tip` (nearest first, at most [`BRANCH_DEPTH`]); and, for each check,
/// the records in which it ran and passed that these lead to, with the paths that differ from
/// each to `commit`. `commit` is a full commit hash.
pub fn sources(dir: &Path, commit: &str, base: &str, tip: &str, checks: &[Check]) -> Sources {
    let none = |why: String| Sources {
        none: Some(why),
        ..Sources::default()
    };
    let Some(mut notes) = Notes::fetch(dir) else {
        return none("origin has no published records".to_owned());
    };
    let base_record = notes.cover(base);
    let limit = (BRANCH_DEPTH + 1).to_string();
    let not_on_main = format!("^{tip}");
    let branch: Vec<String> = git_in(
        dir,
        &[
            "rev-list",
            "--first-parent",
            "--max-count",
            &limit,
            commit,
            &not_on_main,
            "--",
        ],
    )
    .map(|out| {
        out.lines()
            .filter(|c| *c != commit)
            .take(BRANCH_DEPTH)
            .filter(|c| notes.passing(c).is_some())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default();
    if base_record.is_none() && branch.is_empty() {
        return none(format!(
            "no published passing record covers main at {base} or an earlier commit of the branch"
        ));
    }
    let mut diffs: HashMap<String, Result<Vec<String>, String>> = HashMap::new();
    let mut origins = BTreeMap::new();
    for check in checks {
        let mut found: Vec<Origin> = Vec::new();
        for candidate in base_record.iter().chain(&branch) {
            let Some(source) = notes.origin(candidate, &check.name, ORIGIN_DEPTH) else {
                continue;
            };
            if found.iter().any(|o| o.source == source) {
                continue;
            }
            let paths = diffs
                .entry(source.commit.clone())
                .or_insert_with(|| earlier_diff(dir, &source.commit, commit))
                .clone();
            found.push(Origin { source, paths });
        }
        origins.insert(check.name.clone(), found);
    }
    Sources {
        base_record,
        branch,
        origins,
        none: None,
    }
}

/// The record's scope object (VER-057).
pub fn to_json(plan: &Plan, checks: &[Check]) -> Value {
    let decisions: Map<String, Value> = plan
        .checks
        .iter()
        .map(|d| (d.name.clone(), json!({"run": d.run, "why": d.why})))
        .collect();
    let inputs: Map<String, Value> = checks
        .iter()
        .map(|c| (c.name.clone(), json!(c.inputs.patterns())))
        .collect();
    json!({
        "base": plan.base,
        "base_record": plan.base_record,
        "branch_records": plan.branch,
        "changed": plan.changed.iter().take(LISTED).collect::<Vec<_>>(),
        "changed_count": plan.changed.len(),
        "checks": decisions,
        "documentation": DOCS.patterns,
        "github": GITHUB.patterns,
        "root_config": ROOT_CONFIG.patterns,
        "every": plan.every,
        "inputs": inputs,
    })
}

/// The first 12 characters of a commit hash, for display.
fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// Lines describing `plan`, for the run and `verify list`.
pub fn describe(plan: &Plan, checks: &[Check]) -> Vec<String> {
    const SHOWN: usize = 10;
    let mut out = Vec::new();
    let base = plan.base.as_deref().unwrap_or("none");
    if let Some(why) = &plan.every {
        out.push(format!("every check runs: {why}"));
    } else {
        let count = |o: Option<Outcome>| plan.checks.iter().filter(|d| d.outcome() == o).count();
        out.push(format!(
            "{} changed path(s) against main at {base}: {} of {} checks run, {} skipped, {} reused, \
             {} nightly only",
            plan.changed.len(),
            count(None),
            plan.checks.len(),
            count(Some(Outcome::Skipped)),
            count(Some(Outcome::Reused)),
            count(Some(Outcome::NotRun)),
        ));
        match &plan.base_record {
            Some(rec) => out.push(format!("the base passed in the published record of {rec}")),
            None => out.push("no published passing record covers the base".to_owned()),
        }
        if !plan.branch.is_empty() {
            let commits: Vec<&str> = plan.branch.iter().map(|c| short(c)).collect();
            out.push(format!(
                "earlier commits of the branch with a published passing record: {}",
                commits.join(", ")
            ));
        }
    }
    for path in plan.changed.iter().take(SHOWN) {
        out.push(format!("  changed: {path}"));
    }
    if plan.changed.len() > SHOWN {
        out.push(format!("  and {} more", plan.changed.len() - SHOWN));
    }
    for (d, c) in plan.checks.iter().zip(checks) {
        let (what, why) = match (d.outcome(), &d.source) {
            (Some(o), Some(s)) => (o.name(), format!("from the record of {}", short(&s.commit))),
            (Some(_), None) => ("nightly", d.why.clone()),
            (None, _) => ("runs", d.why.clone()),
        };
        out.push(format!(
            "  {:40} {:16} {:7} [{}] {why}",
            d.name,
            c.host.name(),
            what,
            c.inputs.label(),
        ));
    }
    out
}

/// Why the skips of `names`, recorded against `base`, do not hold for `commit`: a check's inputs
/// changed against the base, or the change is not known; with `cover`, also when no published
/// passing record covers the base (formats 1 and 2, whose skips take the base's result).
fn recheck_skips(
    dir: &Path,
    commit: &str,
    base: &str,
    names: &[&Check],
    checks: &[Check],
    cover: bool,
) -> Vec<String> {
    let change = change_from(dir, commit, base);
    let paths = match &change.paths {
        Ok(paths) => paths,
        Err(why) => {
            return names
                .iter()
                .map(|c| format!("{}: skipped, but every check runs: {why}", c.name))
                .collect();
        }
    };
    let uncovered = if cover {
        covering_record(dir, base).err()
    } else {
        None
    };
    names
        .iter()
        .filter_map(|c| {
            if let Some(why) = differs(checks, c, paths) {
                return Some(format!("{}: skipped, but {why}", c.name));
            }
            uncovered
                .as_ref()
                .map(|why| format!("{}: skipped, but every check runs: {why}", c.name))
        })
        .collect()
}

/// Why the result that `entry` of a format 3 record of `commit` takes from its source does not
/// hold, re-derived from git and `origin`'s notes (VER-058): the source must be an ancestor of
/// `commit` whose published record has the named SHA-256, passes the gate, and shows that the
/// check ran and passed, and the check's inputs must be identical between the two commits.
fn recheck_source(
    notes: Option<&mut Notes>,
    dir: &Path,
    commit: &str,
    check: &Check,
    entry: &Value,
    checks: &[Check],
) -> Result<(), String> {
    let field = |k: &str| {
        entry
            .get("source")
            .and_then(|s| s.get(k))
            .and_then(Value::as_str)
    };
    let (Some(from), Some(sha)) = (field("commit"), field("record")) else {
        return Err("it names no source".to_owned());
    };
    if !is_hash(from) {
        return Err(format!("its source {from} is not a full commit hash"));
    }
    let notes = notes.ok_or("origin's published records could not be fetched")?;
    let (rec, published) = notes
        .passing(from)
        .ok_or_else(|| format!("no published record of {from} passes the merge gate"))?;
    if published != sha {
        return Err(format!(
            "the published record of {from} has sha256 {published}, not {sha}"
        ));
    }
    let passed = rec
        .get("checks")
        .and_then(Value::as_array)
        .and_then(|c| c.iter().find(|e| e["name"] == check.name.as_str()))
        .is_some_and(|e| e["result"] == "pass");
    if !passed {
        return Err(format!("it did not run and pass in the record of {from}"));
    }
    let paths = earlier_diff(dir, from, commit)?;
    match differs(checks, check, &paths) {
        Some(why) => Err(format!("{why} against {from}")),
        None => Ok(()),
    }
}

/// Why the skips and reuses of `record` do not hold for its commit, re-derived from git and
/// `origin`'s notes with the current inputs (VER-058); empty when they hold. A record whose
/// checks all ran needs nothing.
pub fn recheck(dir: &Path, rec: &Value, checks: &[Check]) -> Vec<String> {
    let entries = rec
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let format = rec.get("format").and_then(Value::as_str);
    let mut problems = Vec::new();
    if matches!(format, Some(record::FORMAT | record::FORMAT_2)) {
        for c in checks {
            if !entries.iter().any(|e| e["name"] == c.name.as_str()) {
                problems.push(format!("{}: not in the record", c.name));
            }
        }
    }
    let mut skipped = Vec::new();
    let mut carried = Vec::new();
    for e in &entries {
        let result = e.get("result").and_then(Value::as_str).unwrap_or_default();
        if !matches!(result, "skipped" | "reused") {
            continue;
        }
        let name = e.get("name").and_then(Value::as_str).unwrap_or("?");
        let Some(check) = checks.iter().find(|c| c.name == name) else {
            problems.push(format!("{name}: {result}, but there is no such check"));
            continue;
        };
        if result == "skipped" {
            skipped.push(check);
        }
        carried.push((check, result, e));
    }
    if carried.is_empty() {
        return problems;
    }
    let commit = rec
        .get("commit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !skipped.is_empty() {
        let scope = if format == Some(record::FORMAT_1) {
            rec.get("cpp")
        } else {
            rec.get("scope")
        };
        match scope.and_then(|s| s.get("base")).and_then(Value::as_str) {
            Some(base) => problems.extend(recheck_skips(
                dir,
                commit,
                base,
                &skipped,
                checks,
                format != Some(record::FORMAT),
            )),
            None => problems.push("checks are skipped, but the record names no base".to_owned()),
        }
    }
    if format == Some(record::FORMAT) {
        let mut notes = Notes::fetch(dir);
        for (check, result, e) in carried {
            if let Err(why) = recheck_source(notes.as_mut(), dir, commit, check, e, checks) {
                problems.push(format!("{}: {result}, but {why}", check.name));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::super::checks::all;
    use super::*;

    // req: VER-056
    #[test]
    fn patterns() {
        assert!(matches("docs/**", "docs/a.md"));
        assert!(matches("docs/**", "docs/milestones/m0.md"));
        assert!(!matches("docs/**", "docsx/a.md"));
        assert!(!matches("docs/**", "crates/docs/a.md"));
        assert!(matches("*.md", "README.md"));
        assert!(!matches("*.md", "crates/aldis/README.md"));
        assert!(!matches("*.md", "README.mdx"));
        assert!(matches(
            "crates/*/Cargo.toml",
            "crates/aldis-spp/Cargo.toml"
        ));
        assert!(!matches(
            "crates/*/Cargo.toml",
            "crates/aldis-spp/src/Cargo.toml"
        ));
        assert!(matches("a/**/*.md", "a/b.md"));
        assert!(matches("a/**/*.md", "a/b/c/d.md"));
        assert!(!matches("a/**/*.md", "a/b/c/d.json"));
        assert!(matches("tooling-nightly", "tooling-nightly"));
        assert!(!matches("tooling-nightly", "tooling-nightly2"));
    }

    fn changed(paths: &[&str]) -> Change {
        Change {
            base: Some("b".repeat(40)),
            paths: Ok(paths.iter().map(|p| (*p).to_owned()).collect()),
        }
    }

    /// The record of `cccc…` covers the base and passed every check; the paths that differ from
    /// it are those of the change.
    fn covered(change: &Change) -> impl FnOnce(&str) -> Sources + '_ {
        move |_| {
            let source = Source {
                commit: "c".repeat(40),
                record: "d".repeat(64),
            };
            let origin = Origin {
                source: source.clone(),
                paths: change.paths.clone(),
            };
            Sources {
                base_record: Some(source.commit),
                branch: Vec::new(),
                origins: all()
                    .into_iter()
                    .map(|c| (c.name, vec![origin.clone()]))
                    .collect(),
                none: None,
            }
        }
    }

    /// The names of the checks that run.
    fn running(change: &Change, force: Force) -> Vec<String> {
        plan(&all(), change, force, covered(change))
            .checks
            .into_iter()
            .filter(|d| d.run)
            .map(|d| d.name)
            .collect()
    }

    fn names(filter: impl Fn(&Check) -> bool) -> Vec<String> {
        all().into_iter().filter(filter).map(|c| c.name).collect()
    }

    // req: VER-056
    #[test]
    fn github_configuration_runs_spec_only() {
        // #95: GitHub Actions version bumps.
        let dependabot = changed(&[
            ".github/actions/setup-rust/action.yml",
            ".github/workflows/ci.yml",
            ".github/workflows/nightly.yml",
        ]);
        assert_eq!(running(&dependabot, Force::default()), ["spec"]);
        for path in [
            ".github/pull_request_template.md",
            ".github/CODEOWNERS",
            ".github/dependabot.yml",
            ".github/workflows/cpp.yml",
            ".github/workflows/verify.yml",
            ".github/actions/setup-cpp/action.yml",
        ] {
            assert_eq!(
                running(&changed(&[path]), Force::default()),
                ["spec"],
                "{path}"
            );
        }
        // The setup script that the image runs is read by every check; all but the
        // nightly-only kani run.
        assert_eq!(
            running(
                &changed(&[".github/actions/setup-cpp/setup_cpp.py"]),
                Force::default()
            ),
            names(|c| !c.nightly)
        );
    }

    // req: VER-056
    #[test]
    fn root_configuration_runs_spec_only() {
        assert_eq!(
            running(&changed(&[".gitignore"]), Force::default()),
            ["spec"]
        );
        assert_eq!(
            running(&changed(&[".gitignore", "docs/a.md"]), Force::default()),
            ["spec"]
        );
        // .gitattributes can change what a checkout writes: in no list, so every check runs.
        assert_eq!(
            running(&changed(&[".gitattributes"]), Force::default()),
            names(|c| !c.nightly)
        );
    }

    // req: VER-056
    #[test]
    fn documentation_runs_spec_only() {
        let docs = changed(&[
            "docs/06-verification.md",
            "changelog.d/chore-x.md",
            "README.md",
            "AGENTS.md",
            "specs/manifest.tsv",
        ]);
        let p = plan(&all(), &docs, Force::default(), covered(&docs));
        assert_eq!(p.every, None);
        assert_eq!(p.base_record.as_deref(), Some("c".repeat(40).as_str()));
        assert_eq!(running(&docs, Force::default()), ["spec"]);
        let skipped = p.checks.iter().find(|d| d.name == "clippy");
        assert_eq!(
            skipped.map(|d| d.why.clone()),
            Some(skip_reason(&"b".repeat(40)))
        );
        assert_eq!(skipped.and_then(Decision::outcome), Some(Outcome::Skipped));
        assert_eq!(
            skipped.and_then(|d| d.source.as_ref().map(|s| s.commit.clone())),
            Some("c".repeat(40))
        );
        // A deleted document is a changed path like any other.
        assert_eq!(
            running(&changed(&["docs/old.md"]), Force::default()),
            ["spec"]
        );
    }

    // req: VER-056
    #[test]
    fn code_runs_its_checks() {
        // kani is nightly-only: no change runs it without --all.
        let rust = names(|c| !c.cpp && !c.nightly);
        let cpp = names(|c| c.cpp || c.name == "spec");
        let everything = names(|c| !c.nightly);
        // A crate README is a doctest.
        assert_eq!(
            running(&changed(&["crates/aldis-spp/README.md"]), Force::default()),
            rust
        );
        assert_eq!(
            running(
                &changed(&["crates/aldis-spp/src/lib.rs", "README.md"]),
                Force::default()
            ),
            rust
        );
        // A rename out of docs/ lists both sides.
        assert_eq!(
            running(
                &changed(&["docs/a.md", "crates/aldis/a.md"]),
                Force::default()
            ),
            rust
        );
        // A crate with Kani harnesses runs the Rust checks, not kani, which is nightly-only
        // (OI-42, VER-059).
        assert_eq!(
            running(
                &changed(&["crates/aldis-core/src/proofs.rs"]),
                Force::default()
            ),
            rust
        );
        // The C++ tree: spec, the C++ checks, and the checks running the xtask tests.
        let mut cpp_and_tests = cpp;
        cpp_and_tests.extend(["test", "test-macos", "big-endian"].map(str::to_owned));
        let mut want = names(|c| cpp_and_tests.contains(&c.name));
        assert_eq!(
            running(&changed(&["cpp/src/core/crc.cpp"]), Force::default()),
            want
        );
        // The vectors are read by both implementations, not by the header check.
        assert_eq!(
            running(&changed(&["test-vectors/README.md"]), Force::default()),
            names(|c| c.name != "header" && !c.nightly)
        );
        // The committed header is read by the header check (and spec) alone.
        assert_eq!(
            running(&changed(&["include/aldis/aldis.h"]), Force::default()),
            names(|c| c.name == "header" || c.name == "spec")
        );
        // xtask and the image run everything.
        for path in [
            "xtask/src/main.rs",
            "xtask/verify/Containerfile",
            "cpp/toolchains.lock",
            "tooling-nightly",
            "Cargo.lock",
            "crates/aldis-testkit/src/lib.rs",
        ] {
            assert_eq!(
                running(&changed(&[path]), Force::default()),
                everything,
                "{path}"
            );
        }
        // --all-cpp adds the C++ checks.
        want = rust;
        want.extend(names(|c| c.cpp));
        want = names(|c| want.contains(&c.name));
        let all_cpp = Force {
            all: false,
            cpp: true,
        };
        assert_eq!(
            running(&changed(&["crates/aldis/src/lib.rs"]), all_cpp),
            want
        );
    }

    // req: VER-056
    #[test]
    fn fail_safe() {
        let everything = names(|_| true);
        let but_nightly = names(|c| !c.nightly);
        let nightly = |d: &Decision| d.name == "kani" && d.outcome() == Some(Outcome::NotRun);
        let default = Force::default();
        let all = Force {
            all: true,
            cpp: false,
        };
        // An unlisted path, alone or with documentation: no record that differs in it is used.
        for paths in [
            &[".gitattributes"][..],
            &["docs/a.md", "specs/fetch.sh"],
            &[".github/ISSUE_TEMPLATE/bug.md"],
        ] {
            let change = changed(paths);
            let p = plan(&all_checks(), &change, default, covered(&change));
            assert!(
                p.checks
                    .iter()
                    .all(|d| nightly(d) || (d.run && d.why.contains("an input of no check"))),
                "{paths:?}"
            );
        }
        // --all runs every check, the nightly-only ones too (VER-059).
        assert_eq!(running(&changed(&["docs/a.md"]), all), everything);
        let unknown = Change {
            base: None,
            paths: Err("origin/main could not be fetched".to_owned()),
        };
        assert_eq!(running(&unknown, default), but_nightly);
        assert_eq!(running(&unknown, all), everything);
        // No record covers the base, and the branch has none.
        let p = plan(&all_checks(), &changed(&["docs/a.md"]), default, |b| {
            Sources {
                none: Some(format!("nothing covers {b}")),
                ..Sources::default()
            }
        });
        assert_eq!(p.every, Some(format!("nothing covers {}", "b".repeat(40))));
        assert!(p.checks.iter().all(|d| d.run || nightly(d)));
        // A record whose diff is not known is not used.
        let mut change = changed(&["docs/a.md"]);
        let p = plan(&all_checks(), &change, default, covered(&change));
        assert!(p.checks.iter().any(|d| !d.run));
        let unknown_diff = Change {
            paths: Err("not known".to_owned()),
            ..change.clone()
        };
        let p = plan(&all_checks(), &change, default, covered(&unknown_diff));
        assert!(p.checks.iter().all(|d| d.run || nightly(d)));
        // Records are not looked for when every check runs anyway.
        change.paths = Err("the commit is on main".to_owned());
        let asked = std::cell::Cell::new(false);
        plan(&all_checks(), &change, default, |_| {
            asked.set(true);
            Sources::default()
        });
        assert!(!asked.get());
    }

    fn all_checks() -> Vec<Check> {
        super::super::checks::all()
    }

    // req: VER-056
    #[test]
    fn reuse_from_an_earlier_commit_of_the_branch() {
        // The branch changed crates/; main then changed cpp/, and the branch merged main.
        let change = changed(&["crates/x/src/lib.rs"]);
        let source = |c: char| Source {
            commit: c.to_string().repeat(40),
            record: "d".repeat(64),
        };
        let origins = |paths: &[&str], c: char| Origin {
            source: source(c),
            paths: Ok(paths.iter().map(|p| (*p).to_owned()).collect()),
        };
        let sources = |_: &str| Sources {
            base_record: Some("c".repeat(40)),
            branch: vec!["e".repeat(40)],
            origins: all_checks()
                .into_iter()
                .map(|c| {
                    let base = origins(&["crates/x/src/lib.rs"], 'c');
                    (c.name, vec![base, origins(&["cpp/src/a.cpp"], 'e')])
                })
                .collect(),
            none: None,
        };
        let p = plan(&all_checks(), &change, Force::default(), sources);
        let of = |o: Option<Outcome>| -> Vec<String> {
            p.checks
                .iter()
                .filter(|d| d.outcome() == o)
                .map(|d| d.name.clone())
                .collect()
        };
        let xtask_tests = ["test", "test-macos", "big-endian"];
        assert_eq!(
            of(Some(Outcome::Reused)),
            names(|c| {
                !c.cpp
                    && !matches!(c.name.as_str(), "spec" | "kani")
                    && !xtask_tests.contains(&c.name.as_str())
            })
        );
        assert_eq!(of(Some(Outcome::Skipped)), names(|c| c.cpp));
        // kani is nightly-only: neither skipped nor reused, whatever the records say.
        assert_eq!(of(Some(Outcome::NotRun)), ["kani"]);
        let kani = p.checks.iter().find(|d| d.name == "kani").unwrap();
        assert_eq!((kani.why.as_str(), &kani.source), (NIGHTLY_ONLY, &None));
        let mut run = vec!["spec".to_owned()];
        run.extend(xtask_tests.map(str::to_owned));
        assert_eq!(of(None), names(|c| run.contains(&c.name)));
        let clippy = p.checks.iter().find(|d| d.name == "clippy");
        assert_eq!(clippy.and_then(|d| d.source.clone()), Some(source('e')));
        assert_eq!(
            clippy.map(|d| d.why.clone()),
            Some(reuse_reason(&source('e')))
        );
        // --all-cpp runs the C++ checks whatever the records say.
        let forced = Force {
            all: false,
            cpp: true,
        };
        let p = plan(&all_checks(), &change, forced, sources);
        assert!(
            p.checks
                .iter()
                .filter(|d| d.name.starts_with("cpp-"))
                .all(|d| d.run && d.why == "--all-cpp")
        );
    }

    /// A throwaway git repository under the temporary directory.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("aldis-verify-scope-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Self(dir);
            // `init -b` needs git 2.28; the nightly big-endian job runs the tests in a `cross`
            // image with an older git.
            repo.git(&["init", "-q"]);
            repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
            repo
        }

        fn clone_of(origin: &Self, name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("aldis-verify-scope-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let repo = Self(dir.clone());
            origin.git(&[
                "clone",
                "-q",
                origin.0.to_str().unwrap(),
                dir.to_str().unwrap(),
            ]);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "init.defaultBranch=main",
                ])
                .args(args)
                .current_dir(&self.0)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        }

        fn write(&self, path: &str, text: &str) {
            let file = self.0.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }

        /// Commits everything; returns the commit.
        fn commit(&self, message: &str) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--allow-empty", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }

        /// A record of `commit` in `format` with the result entries `checks`, and a scope that
        /// decides as they say.
        fn record_for(&self, format: &str, commit: &str, checks: &Value) -> Value {
            let tree = self.git(&["rev-parse", &format!("{commit}^{{tree}}")]);
            let decisions: Map<String, Value> = checks
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    let run = !matches!(e["result"].as_str(), Some("skipped" | "reused"));
                    (e["name"].as_str().unwrap().to_owned(), json!({"run": run}))
                })
                .collect();
            let base = "0".repeat(40);
            json!({"format": format, "commit": commit, "tree": tree, "checks": checks,
                "scope": {"base": base, "base_record": base, "checks": decisions}})
        }

        /// Publishes `rec` as a note on this repository's notes ref; returns its SHA-256.
        fn publish_record(&self, rec: &Value) -> String {
            let text = record::canonical(rec);
            let commit = rec["commit"].as_str().unwrap();
            self.git(&[
                "notes",
                "--ref=refs/notes/verify",
                "add",
                "-f",
                "-m",
                &text,
                commit,
            ]);
            record::sha256_hex(text.as_bytes())
        }

        /// Publishes a format 3 record of `commit` with the result entries `checks`; returns its
        /// SHA-256.
        fn publish(&self, commit: &str, checks: &Value) -> String {
            self.publish_record(&self.record_for(record::FORMAT, commit, checks))
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pass(name: &str) -> Value {
        json!({"name": name, "host": "container", "gating": true, "result": "pass"})
    }

    /// `origin` with a merged pull request on `main`, and a clone of it.
    fn history(name: &str) -> (Repo, Repo, String) {
        let origin = Repo::new(&format!("{name}-origin"));
        origin.write("docs/a.md", "a\n");
        origin.write("crates/x/src/lib.rs", "//! x\n");
        origin.commit("start");
        origin.git(&["checkout", "-q", "-b", "pr"]);
        origin.write("crates/x/src/lib.rs", "//! x, changed\n");
        let head = origin.commit("pr");
        origin.git(&["checkout", "-q", "main"]);
        origin.git(&["merge", "-q", "--no-ff", "-m", "merge pr", "pr"]);
        let merge = origin.git(&["rev-parse", "HEAD"]);
        origin.publish(&head, &json!([pass("spec")]));
        let clone = Repo::clone_of(&origin, &format!("{name}-clone"));
        clone.git(&["checkout", "-q", "-b", "topic"]);
        (origin, clone, merge)
    }

    // req: VER-056
    #[test]
    fn changed_paths_from_git() {
        let (_origin, clone, merge) = history("paths");
        // The commit is on main.
        let on_main = change(&clone.0, "HEAD", Main::Origin);
        assert_eq!(on_main.paths, Err("the commit is on main".to_owned()));
        // An empty commit changes nothing.
        clone.commit("empty");
        let empty = change(&clone.0, "HEAD", Main::Origin);
        assert_eq!(empty.base.as_deref(), Some(merge.as_str()));
        assert!(empty.paths.is_err());
        // A rename lists both sides, a deletion the deleted path.
        clone.git(&["mv", "docs/a.md", "crates/x/a.md"]);
        clone.write("docs/b c.md", "b\n");
        clone.commit("rename");
        let renamed = change(&clone.0, "HEAD", Main::Origin);
        assert_eq!(
            renamed.paths,
            Ok(vec![
                "crates/x/a.md".to_owned(),
                "docs/a.md".to_owned(),
                "docs/b c.md".to_owned()
            ])
        );
        // The base is covered by the published record of the merged head.
        let covered = covering_record(&clone.0, &merge);
        assert_eq!(
            covered,
            Ok(clone.git(&["rev-parse", &format!("{merge}^2")]))
        );
        // No origin: unknown.
        clone.git(&["remote", "remove", "origin"]);
        let no_origin = change(&clone.0, "HEAD", Main::Origin);
        assert_eq!(
            no_origin.paths,
            Err("origin/main could not be fetched".to_owned())
        );
        assert!(covering_record(&clone.0, &merge).is_err());
    }

    // req: VER-056
    #[test]
    fn no_merge_base() {
        let (_origin, clone, _) = history("orphan");
        clone.git(&["checkout", "-q", "--orphan", "lonely"]);
        clone.write("docs/c.md", "c\n");
        clone.commit("orphan");
        let c = change(&clone.0, "HEAD", Main::Origin);
        assert_eq!(c.paths, Err("no merge base with origin/main".to_owned()));
    }

    // req: VER-056, VER-058
    #[test]
    fn uncovered_base_runs_everything() {
        let (origin, clone, _) = history("uncovered");
        // A commit pushed to main directly: no record covers it.
        origin.write("crates/x/src/lib.rs", "//! x, pushed\n");
        let pushed = origin.commit("direct push");
        clone.git(&["fetch", "-q", "origin"]);
        clone.git(&["reset", "-q", "--hard", "origin/main"]);
        clone.write("docs/a.md", "a, edited\n");
        clone.commit("docs");
        let c = change(&clone.0, "HEAD", Main::Origin);
        assert_eq!(c.base.as_deref(), Some(pushed.as_str()));
        let head = clone.git(&["rev-parse", "HEAD"]);
        let p = plan(&all(), &c, Force::default(), |b| {
            sources(&clone.0, &head, b, "origin/main", &all())
        });
        assert!(
            p.every
                .is_some_and(|w| w.contains("no published passing record"))
        );
    }

    fn record_of(clone: &Repo, base: &str, checks: &[Check], skip: &[&str]) -> Value {
        let commit = clone.git(&["rev-parse", "HEAD"]);
        let entries: Vec<Value> = checks
            .iter()
            .map(|c| {
                let result = if skip.contains(&c.name.as_str()) {
                    "skipped"
                } else {
                    "pass"
                };
                json!({"name": c.name, "host": c.host.name(), "gating": c.gating, "result": result})
            })
            .collect();
        json!({"format": record::FORMAT_2, "commit": commit, "scope": {"base": base}, "checks": entries})
    }

    // req: VER-057, VER-058
    #[test]
    fn recheck_rederives_the_skips_of_format_2() {
        let (_origin, clone, merge) = history("recheck");
        let checks = all();
        let rust: Vec<String> = names(|c| c.name != "spec");
        let rust: Vec<&str> = rust.iter().map(String::as_str).collect();
        clone.write("docs/a.md", "a, edited\n");
        clone.commit("docs");
        // A documentation change may skip everything but spec.
        let docs = record_of(&clone, &merge, &checks, &rust);
        assert_eq!(recheck(&clone.0, &docs, &checks), Vec::<String>::new());
        // A record that leaves out a check.
        let mut short = docs;
        short["checks"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["name"] != "deny");
        assert_eq!(
            recheck(&clone.0, &short, &checks),
            ["deny: not in the record"]
        );
        // A code change may not, whatever the record says.
        clone.write("crates/x/src/lib.rs", "//! x, again\n");
        clone.commit("code");
        let forged = record_of(&clone, &merge, &checks, &["clippy"]);
        assert_eq!(
            recheck(&clone.0, &forged, &checks),
            ["clippy: skipped, but changes crates/x/src/lib.rs"]
        );
        // A full run needs nothing.
        let full = record_of(&clone, &merge, &checks, &[]);
        assert_eq!(recheck(&clone.0, &full, &checks), Vec::<String>::new());
        // A base that is not on main.
        let topic = clone.git(&["rev-parse", "HEAD~1"]);
        let off_main = record_of(&clone, &topic, &checks, &["clippy"]);
        assert!(
            recheck(&clone.0, &off_main, &checks)
                .iter()
                .any(|p| p.contains("is not on origin/main"))
        );
        // A base that is not a commit hash is never passed to git.
        let option = record_of(&clone, "--output=x", &checks, &["clippy"]);
        assert!(
            recheck(&clone.0, &option, &checks)
                .iter()
                .any(|p| p.contains("not a full commit hash"))
        );
        // A skip without a base.
        let mut no_base = forged;
        no_base["scope"] = json!({});
        assert_eq!(
            recheck(&clone.0, &no_base, &checks),
            ["checks are skipped, but the record names no base"]
        );
    }

    // req: VER-058
    #[test]
    fn recheck_reads_format_1_records() {
        let (_origin, clone, merge) = history("format1");
        let checks = all();
        clone.write("crates/x/src/lib.rs", "//! x, again\n");
        let commit = clone.commit("code");
        let cpp: Vec<Value> = checks
            .iter()
            .map(|c| {
                let result = if c.cpp { "skipped" } else { "pass" };
                json!({"name": c.name, "host": c.host.name(), "gating": c.gating, "result": result})
            })
            .collect();
        let old = json!({
            "format": record::FORMAT_1,
            "commit": commit,
            "cpp": {"affected": false, "base": merge},
            "checks": cpp,
        });
        assert_eq!(recheck(&clone.0, &old, &checks), Vec::<String>::new());
    }

    /// Result entries of every check: the result `of` gives each, with the source it names.
    fn entries(checks: &[Check], of: impl Fn(&Check) -> (&'static str, Option<Value>)) -> Value {
        checks
            .iter()
            .map(|c| {
                let (result, source) = of(c);
                let mut e = json!({"name": c.name, "host": c.host.name(), "gating": c.gating,
                    "result": result});
                if let Some(s) = source {
                    e["source"] = s;
                }
                e
            })
            .collect()
    }

    /// The record a run of `commit` would write for `p`, every check that runs passing.
    fn record_from(repo: &Repo, commit: &str, p: &Plan, checks: &[Check]) -> Value {
        let tree = repo.git(&["rev-parse", &format!("{commit}^{{tree}}")]);
        let results: Vec<Value> = p
            .checks
            .iter()
            .zip(checks)
            .map(|(d, c)| {
                let mut e = json!({"name": c.name, "host": c.host.name(), "gating": c.gating,
                    "result": d.outcome().map_or("pass", Outcome::name)});
                if let Some(s) = &d.source {
                    e["source"] = s.to_json();
                }
                e
            })
            .collect();
        json!({"format": record::FORMAT, "commit": commit, "tree": tree,
            "scope": to_json(p, checks), "checks": results})
    }

    /// The history of a re-verification after `main` moved.
    struct Reverify {
        origin: Repo,
        clone: Repo,
        /// The head of the pull request merged first; every check passed there.
        head0: String,
        /// The SHA-256 of its record.
        sha0: String,
        /// The branch's verified head before `main` moved.
        p1: String,
        /// The head of the pull request that moved `main`; every check passed there.
        head1: String,
        /// The branch's head after it merged `main`.
        h: String,
    }

    /// PR 0 merged into `main` with every check passed; a branch from it changes
    /// `crates/x/src/lib.rs`, and its record passes the Rust checks and skips the C++ checks
    /// with PR 0's record as their source; PR 1 changes `path` and is merged with every check
    /// passed; then the branch merges `main`.
    fn reverify(name: &str, path: &str) -> Reverify {
        let checks = all();
        let origin = Repo::new(&format!("{name}-origin"));
        origin.write("docs/a.md", "a\n");
        origin.write("crates/x/src/lib.rs", "//! x\n");
        origin.write("crates/y/src/lib.rs", "//! y\n");
        origin.write("cpp/src/a.cpp", "// a\n");
        origin.commit("start");
        origin.git(&["checkout", "-q", "-b", "pr0"]);
        origin.write("docs/a.md", "a, PR 0\n");
        let head0 = origin.commit("pr0");
        origin.git(&["checkout", "-q", "main"]);
        origin.git(&["merge", "-q", "--no-ff", "-m", "merge pr0", "pr0"]);
        let sha0 = origin.publish(&head0, &entries(&checks, |_| ("pass", None)));
        let clone = Repo::clone_of(&origin, &format!("{name}-clone"));
        clone.git(&["checkout", "-q", "-b", "topic"]);
        clone.write("crates/x/src/lib.rs", "//! x, on the branch\n");
        let p1 = clone.commit("topic");
        clone.git(&["push", "-q", "origin", "topic"]);
        let src0 = json!({"commit": head0, "record": sha0});
        origin.publish(
            &p1,
            &entries(&checks, |c| {
                if c.cpp {
                    ("skipped", Some(src0.clone()))
                } else {
                    ("pass", None)
                }
            }),
        );
        origin.git(&["checkout", "-q", "-b", "pr1", "main"]);
        origin.write(path, "changed on main\n");
        let head1 = origin.commit("pr1");
        origin.git(&["checkout", "-q", "main"]);
        origin.git(&["merge", "-q", "--no-ff", "-m", "merge pr1", "pr1"]);
        origin.publish(&head1, &entries(&checks, |_| ("pass", None)));
        clone.git(&["fetch", "-q", "origin"]);
        clone.git(&["merge", "-q", "--no-edit", "origin/main"]);
        let h = clone.git(&["rev-parse", "HEAD"]);
        Reverify {
            origin,
            clone,
            head0,
            sha0,
            p1,
            head1,
            h,
        }
    }

    fn plan_of(r: &Reverify) -> Plan {
        let checks = all();
        let c = change(&r.clone.0, &r.h, Main::Origin);
        plan(&checks, &c, Force::default(), |b| {
            sources(&r.clone.0, &r.h, b, "origin/main", &checks)
        })
    }

    /// The names of the checks with result `o` in `p`, and the commits of their sources.
    fn decided(p: &Plan, o: Option<Outcome>) -> (Vec<String>, Vec<String>) {
        let chosen: Vec<&Decision> = p.checks.iter().filter(|d| d.outcome() == o).collect();
        let mut commits: Vec<String> = chosen
            .iter()
            .filter_map(|d| d.source.as_ref().map(|s| s.commit.clone()))
            .collect();
        commits.dedup();
        (chosen.iter().map(|d| d.name.clone()).collect(), commits)
    }

    // req: VER-056, VER-057, VER-058
    #[test]
    fn a_main_merge_that_changes_only_cpp_reuses_the_rust_checks() {
        let r = reverify("reuse", "cpp/src/a.cpp");
        let checks = all();
        let p = plan_of(&r);
        assert_eq!(p.base_record.as_deref(), Some(r.head1.as_str()));
        assert_eq!(p.branch, std::slice::from_ref(&r.p1));
        let xtask_tests = ["test", "test-macos", "big-endian"];
        // The Rust checks: their inputs are those of the branch's verified head.
        let rust = names(|c| {
            !c.cpp
                && !matches!(c.name.as_str(), "spec" | "kani")
                && !xtask_tests.contains(&c.name.as_str())
        });
        assert_eq!(
            decided(&p, Some(Outcome::Reused)),
            (rust.clone(), vec![r.p1.clone()])
        );
        // The C++ checks: unchanged against the base, which PR 1's record covers.
        assert_eq!(
            decided(&p, Some(Outcome::Skipped)),
            (names(|c| c.cpp), vec![r.head1.clone()])
        );
        assert_eq!(decided(&p, Some(Outcome::NotRun)).0, ["kani"]);
        // spec reads every path; the xtask tests read cpp/.
        let mut run = xtask_tests.map(str::to_owned).to_vec();
        run.push("spec".to_owned());
        assert_eq!(decided(&p, None).0, names(|c| run.contains(&c.name)));
        let lines = describe(&p, &checks);
        assert!(
            lines[0].ends_with(&format!(
                "4 of {} checks run, 15 skipped, {} reused, 1 nightly only",
                checks.len(),
                rust.len()
            )),
            "{lines:?}"
        );

        // The record of that run passes the gate, its reuses re-derived from git.
        let rec = record_from(&r.clone, &r.h, &p, &checks);
        assert_eq!(record::gate_problems(&rec), Vec::<String>::new());
        assert_eq!(recheck(&r.clone.0, &rec, &checks), Vec::<String>::new());

        // A forged reuse: PR 0's record passed clippy, but the branch changed its inputs since.
        let at = |name: &str| checks.iter().position(|c| c.name == name).unwrap();
        let mut forged = rec.clone();
        forged["checks"][at("clippy")]["source"] = json!({"commit": r.head0, "record": r.sha0});
        assert_eq!(
            recheck(&r.clone.0, &forged, &checks),
            [format!(
                "clippy: reused, but changes crates/x/src/lib.rs against {}",
                r.head0
            )]
        );
        // A source whose record is not the one named.
        let mut other = rec.clone();
        other["checks"][at("fmt")]["source"]["record"] = json!("0".repeat(64));
        let problems = recheck(&r.clone.0, &other, &checks);
        assert!(
            problems.len() == 1 && problems[0].contains(&format!("not {}", "0".repeat(64))),
            "{problems:?}"
        );
        // A skip whose inputs changed against the base, whatever its source.
        let mut skip = rec.clone();
        skip["checks"][at("clippy")]["result"] = json!("skipped");
        assert_eq!(
            recheck(&r.clone.0, &skip, &checks),
            ["clippy: skipped, but changes crates/x/src/lib.rs"]
        );
        // A source that did not run the check: the branch head skipped cpp-tidy.
        let mut skipped_there = rec;
        skipped_there["checks"][at("cpp-tidy")]["source"] = json!({"commit": r.p1,
            "record": record::sha256_hex(publish::published(&r.clone.0, &r.p1).unwrap().as_bytes())});
        let problems = recheck(&r.clone.0, &skipped_there, &checks);
        assert_eq!(
            problems,
            [format!(
                "cpp-tidy: skipped, but it did not run and pass in the record of {}",
                r.p1
            )]
        );
    }

    // req: VER-056
    #[test]
    fn a_main_merge_that_changes_a_rust_input_runs_the_rust_checks() {
        let r = reverify("rust-input", "crates/y/src/lib.rs");
        let p = plan_of(&r);
        assert_eq!(decided(&p, None).0, names(|c| !c.cpp && !c.nightly));
        assert_eq!(
            decided(&p, Some(Outcome::Skipped)),
            (names(|c| c.cpp), vec![r.head1])
        );
        assert_eq!(decided(&p, Some(Outcome::Reused)).0, Vec::<String>::new());
        let clippy = p.checks.iter().find(|d| d.name == "clippy").unwrap();
        assert_eq!(clippy.why, "changes crates/x/src/lib.rs");
    }

    // req: VER-056, VER-058
    #[test]
    fn a_source_whose_record_fails_the_gate_is_not_used() {
        let r = reverify("failing", "cpp/src/a.cpp");
        let checks = all();
        let rec = record_from(&r.clone, &r.h, &plan_of(&r), &checks);
        // The branch head is verified again, and fmt fails.
        r.origin.publish(
            &r.p1,
            &entries(&checks, |c| {
                if c.name == "fmt" {
                    ("fail", None)
                } else {
                    ("pass", None)
                }
            }),
        );
        let p = plan_of(&r);
        assert_eq!(p.branch, Vec::<String>::new());
        assert_eq!(decided(&p, Some(Outcome::Reused)).0, Vec::<String>::new());
        // The record that reused it no longer passes.
        let problems = recheck(&r.clone.0, &rec, &checks);
        // One problem per reused check: the Rust checks outside spec and the xtask tests.
        let xtask_tests = ["test", "test-macos", "big-endian"];
        let reused = names(|c| {
            !c.cpp
                && !matches!(c.name.as_str(), "spec" | "kani")
                && !xtask_tests.contains(&c.name.as_str())
        });
        assert_eq!(problems.len(), reused.len(), "{problems:?}");
        assert!(
            problems[0].ends_with(&format!(
                "no published record of {} passes the merge gate",
                r.p1
            )),
            "{problems:?}"
        );
    }

    // req: VER-056, VER-058
    #[test]
    fn a_source_that_is_not_an_ancestor_is_not_used() {
        let r = reverify("side", "cpp/src/a.cpp");
        let checks = all();
        // A side branch with the same crates as the merged head, verified.
        r.clone
            .git(&["checkout", "-q", "-b", "side", &format!("{}^1", r.h)]);
        r.clone
            .write("crates/x/src/lib.rs", "//! x, on the branch\n");
        let z = r.clone.commit("side");
        r.clone.git(&["push", "-q", "origin", "side"]);
        let sha_z = r.origin.publish(&z, &entries(&checks, |_| ("pass", None)));
        r.clone.git(&["checkout", "-q", "topic"]);
        // The branch head's record says clippy came from the side branch.
        let src_z = json!({"commit": z, "record": sha_z});
        let mut rec = r.origin.record_for(
            record::FORMAT,
            &r.p1,
            &entries(&checks, |c| {
                if c.name == "clippy" {
                    ("reused", Some(src_z.clone()))
                } else {
                    ("pass", None)
                }
            }),
        );
        rec["scope"]["base"] = json!(r.head0);
        r.origin.publish_record(&rec);
        let p = plan_of(&r);
        let clippy = p.checks.iter().find(|d| d.name == "clippy").unwrap();
        assert!(clippy.run, "{clippy:?}");
        // A record of the merged head that names it is refused.
        let mut forged = record_from(&r.clone, &r.h, &p, &checks);
        let at = checks.iter().position(|c| c.name == "clippy").unwrap();
        forged["checks"][at]["result"] = json!("reused");
        forged["checks"][at]["source"] = src_z;
        forged["scope"]["checks"]["clippy"]["run"] = json!(false);
        assert_eq!(record::gate_problems(&forged), Vec::<String>::new());
        assert_eq!(
            recheck(&r.clone.0, &forged, &checks),
            [format!(
                "clippy: reused, but {z} is not an ancestor of {}",
                r.h
            )]
        );
    }

    // req: VER-056, VER-057
    #[test]
    fn origins_are_followed_through_format_2_and_3_records() {
        let r = reverify("origins", "cpp/src/a.cpp");
        let checks = all();
        let mut notes = Notes::fetch(&r.clone.0).unwrap();
        let from0 = Source {
            commit: r.head0.clone(),
            record: r.sha0.clone(),
        };
        // Format 3: the source the entry names.
        assert_eq!(
            notes.origin(&r.p1, "cpp-tidy", ORIGIN_DEPTH),
            Some(from0.clone())
        );
        // Format 2: the record covering the base.
        let mut two = r.origin.record_for(
            record::FORMAT_2,
            &r.p1,
            &entries(&checks, |c| {
                if c.cpp {
                    ("skipped", None)
                } else {
                    ("pass", None)
                }
            }),
        );
        two["scope"]["base_record"] = json!(r.head0);
        r.origin.publish_record(&two);
        let mut notes = Notes::fetch(&r.clone.0).unwrap();
        assert_eq!(
            notes.origin(&r.p1, "cpp-tidy", ORIGIN_DEPTH),
            Some(from0.clone())
        );
        assert_eq!(notes.origin(&r.p1, "cpp-tidy", 0), None);
        // Format 1: the record that covers its base, the merge of PR 0.
        let merge0 = r.clone.git(&["rev-parse", &format!("{}^", r.p1)]);
        let mut one = two;
        one["format"] = json!(record::FORMAT_1);
        one["cpp"] = json!({"affected": false, "base": merge0});
        r.origin.publish_record(&one);
        let mut notes = Notes::fetch(&r.clone.0).unwrap();
        assert_eq!(notes.origin(&r.p1, "cpp-tidy", ORIGIN_DEPTH), Some(from0));
        // A record that names itself leads nowhere.
        let mut own = r.origin.record_for(
            record::FORMAT,
            &r.p1,
            &entries(&checks, |c| {
                if c.cpp {
                    ("reused", Some(json!({"commit": r.p1, "record": "x"})))
                } else {
                    ("pass", None)
                }
            }),
        );
        own["scope"]["base"] = json!(r.head0);
        r.origin.publish_record(&own);
        let mut notes = Notes::fetch(&r.clone.0).unwrap();
        assert_eq!(notes.origin(&r.p1, "cpp-tidy", ORIGIN_DEPTH), None);
        assert!(notes.origin(&r.p1, "clippy", ORIGIN_DEPTH).is_some());
    }

    // req: VER-056, VER-058
    #[test]
    fn a_check_that_did_not_run_is_never_a_source() {
        let r = reverify("stopped", "cpp/src/a.cpp");
        let checks = all();
        let stopped = "the run stopped after the gating check header failed (--fail-fast)";
        // A record that passes the gate, in which kani, nightly-only, did not run: kani takes no
        // result from it, and every check that passed does.
        let mut kani = r.origin.record_for(
            record::FORMAT,
            &r.p1,
            &entries(&checks, |c| {
                if c.name == "kani" {
                    ("not-run", None)
                } else {
                    ("pass", None)
                }
            }),
        );
        let at = checks.iter().position(|c| c.name == "kani").unwrap();
        kani["checks"][at]["reason"] = json!(stopped);
        assert_eq!(record::gate_problems(&kani), Vec::<String>::new());
        r.origin.publish_record(&kani);
        let mut notes = Notes::fetch(&r.clone.0).unwrap();
        assert_eq!(notes.origin(&r.p1, "kani", ORIGIN_DEPTH), None);
        assert_eq!(
            notes
                .origin(&r.p1, "clippy", ORIGIN_DEPTH)
                .map(|s| s.commit),
            Some(r.p1.clone())
        );
        // The record of a run that stopped at header, had it been published, is no source at
        // all: not even for the checks that passed before the stop.
        let order: Vec<String> = super::super::checks::run_order()
            .into_iter()
            .map(|c| c.name)
            .collect();
        let header = order.iter().position(|n| n == "header").unwrap();
        r.origin.publish(
            &r.p1,
            &entries(&checks, |c| {
                let at = order.iter().position(|n| *n == c.name).unwrap();
                match at.cmp(&header) {
                    std::cmp::Ordering::Less => ("pass", None),
                    std::cmp::Ordering::Equal => ("fail", None),
                    std::cmp::Ordering::Greater => ("not-run", None),
                }
            }),
        );
        let mut notes = Notes::fetch(&r.clone.0).unwrap();
        assert_eq!(notes.passing(&r.p1), None);
        assert_eq!(notes.origin(&r.p1, "fmt", ORIGIN_DEPTH), None);
        let p = plan_of(&r);
        assert_eq!(p.branch, Vec::<String>::new());
        assert!(
            p.checks
                .iter()
                .filter_map(|d| d.source.as_ref())
                .all(|s| s.commit != r.p1),
            "{p:?}"
        );
    }

    /// A [`publish::Forge`] that has every commit and records the statuses set.
    #[derive(Default)]
    struct Forge(std::cell::RefCell<Vec<String>>);

    impl publish::Forge for Forge {
        fn has_commit(&self, _: &Path, _: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn set_status(
            &self,
            _: &Path,
            commit: &str,
            _: &str,
            _: Option<&str>,
        ) -> Result<(), String> {
            self.0.borrow_mut().push(commit.to_owned());
            Ok(())
        }
    }

    /// Writes `rec` as the local record of its commit in `repo`, as a run does.
    fn write_record(repo: &Repo, rec: &Value) {
        let commit = rec["commit"].as_str().unwrap();
        repo.write(
            &format!("target/verify/{commit}/record.json"),
            &record::canonical(rec),
        );
    }

    /// Whether `origin` has a published record of `commit`.
    fn has_note(origin: &Repo, commit: &str) -> bool {
        Command::new("git")
            .args(["notes", "--ref=refs/notes/verify", "show", commit])
            .current_dir(&origin.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    // req: VER-056, VER-057, VER-058
    #[test]
    fn a_passing_record_of_a_commit_behind_main_is_published_without_a_status() {
        let (origin, clone, _) = history("behind");
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@t"),
            ("commit.gpgsign", "false"),
        ] {
            clone.git(&["config", k, v]);
        }
        // Local records are not committed.
        clone.write(".git/info/exclude", "target/\n");
        let checks = all();
        clone.write("crates/x/src/lib.rs", "//! x, on the topic\n");
        let p = clone.commit("topic");
        clone.git(&["push", "-q", "origin", "topic"]);
        // main moves while p is verified.
        origin.write("cpp/src/a.cpp", "// main moved\n");
        origin.commit("main moved");
        let passing = clone.record_for(record::FORMAT, &p, &entries(&checks, |_| ("pass", None)));
        write_record(&clone, &passing);
        let forge = Forge::default();
        // The note is published; no status is set, and the result says the head is behind.
        assert_eq!(publish::publish_in(&clone.0, &p, &forge), Ok(1));
        assert!(has_note(&origin, &p));
        assert_eq!(forge.0.borrow().len(), 0);
        // The merge gate still refuses the head.
        assert_eq!(publish::check_in(&clone.0, &p), Ok(1));

        // The branch merges main: the Rust checks take their results from p's record.
        clone.git(&["fetch", "-q", "origin"]);
        clone.git(&["merge", "-q", "--no-edit", "origin/main"]);
        let h = clone.git(&["rev-parse", "HEAD"]);
        let c = change(&clone.0, &h, Main::Origin);
        let plan_h = plan(&checks, &c, Force::default(), |b| {
            sources(&clone.0, &h, b, "origin/main", &checks)
        });
        assert_eq!(plan_h.branch, std::slice::from_ref(&p));
        let clippy = plan_h.checks.iter().find(|d| d.name == "clippy").unwrap();
        assert_eq!(clippy.outcome(), Some(Outcome::Reused));
        assert_eq!(
            clippy.source.as_ref().map(|s| s.commit.as_str()),
            Some(p.as_str())
        );
        let cpp = plan_h.checks.iter().find(|d| d.name == "cpp-tidy").unwrap();
        assert!(cpp.run, "{cpp:?}");
        // Its record passes, re-derived from git and p's record, and sets the status.
        let rec_h = record_from(&clone, &h, &plan_h, &checks);
        assert_eq!(recheck(&clone.0, &rec_h, &checks), Vec::<String>::new());
        write_record(&clone, &rec_h);
        assert_eq!(publish::publish_in(&clone.0, &h, &forge), Ok(0));
        assert_eq!(*forge.0.borrow(), std::slice::from_ref(&h));
        assert_eq!(publish::check_in(&clone.0, &h), Ok(0));

        // A failed run publishes nothing, behind main or not.
        clone.write("crates/x/src/lib.rs", "//! x, broken\n");
        let q = clone.commit("broken");
        let failed = clone.record_for(
            record::FORMAT,
            &q,
            &entries(&checks, |c| {
                if c.name == "fmt" {
                    ("fail", None)
                } else {
                    ("pass", None)
                }
            }),
        );
        write_record(&clone, &failed);
        let refused = publish::publish_in(&clone.0, &q, &forge).unwrap_err();
        assert!(refused.contains("fmt: fail"), "{refused}");
        assert!(!has_note(&origin, &q));
        assert_eq!(forge.0.borrow().len(), 1);
    }
}
