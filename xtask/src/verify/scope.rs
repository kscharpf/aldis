//! Which checks a change needs (VER-056): the paths a commit changes against the tip of `main`,
//! matched against the inputs of each check (`checks.rs`), and the re-derivation of a record's
//! skips that `verify check` and `verify publish` make before they trust it (VER-058).
//!
//! The rule is fail-safe: every check runs when the changed paths are not known (`origin/main`
//! cannot be fetched, there is no merge base, the commit is on `main`, nothing differs), when a
//! changed path is an input of no check and not documentation, or when no published passing
//! record covers the base. A check is skipped only because the base passed it and none of its
//! inputs differ from the base.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::checks::{Check, DOCS, Inputs};
use super::{git_bytes, git_in, publish, record};

/// At most this many changed paths are listed in a record.
const LISTED: usize = 100;

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
    // A full object name, never an option or a revision expression.
    let hex = |s: &str| matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit());
    if !hex(base) || !hex(commit) {
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

/// `git diff --name-only` from `base` to `commit`. Without renames, so that both sides of a
/// rename are listed, and with NUL separators, so that no path is quoted. An empty list is
/// treated as unknown: it cannot be told apart from a diff that went wrong, and a commit that
/// changes nothing has nothing to gain from the rule.
fn changed_paths(dir: &Path, base: &str, commit: &str) -> Result<Vec<String>, String> {
    let out = git_bytes(
        dir,
        &[
            "diff",
            "--no-relative",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            "-z",
            base,
            commit,
        ],
    )
    .map_err(|e| format!("the changed paths are not known: {e}"))?;
    let paths = out
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            String::from_utf8(p.to_vec()).map_err(|_| "a changed path is not UTF-8".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if paths.is_empty() {
        return Err(format!("no path differs from main at {base}"));
    }
    Ok(paths)
}

/// What `--all` and `--all-cpp` force to run.
#[derive(Clone, Copy, Debug, Default)]
pub struct Force {
    /// Every check (`--all`).
    pub all: bool,
    /// Every C++ check (`--all-cpp`).
    pub cpp: bool,
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
}

/// What the rule decides for a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The merge base with the tip of `main`.
    pub base: Option<String>,
    /// The commit whose published passing record covers the base, when a check is skipped.
    pub base_record: Option<String>,
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

/// Whether some check reads `path`, or it is documentation (read by `spec` alone).
fn listed(checks: &[Check], path: &str) -> bool {
    DOCS.contains(path)
        || checks
            .iter()
            .any(|c| c.inputs != Inputs::Everything && c.inputs.contains(path))
}

/// Applies the rule to `change`. `cover` names the commit whose published passing record covers
/// a base, or says why there is none; it is asked only when a check would be skipped.
pub fn plan(
    checks: &[Check],
    change: &Change,
    force: Force,
    cover: impl FnOnce(&str) -> Result<String, String>,
) -> Plan {
    let every = |why: String, changed: Vec<String>| Plan {
        base: change.base.clone(),
        base_record: None,
        changed,
        checks: checks
            .iter()
            .map(|c| Decision {
                name: c.name.clone(),
                run: true,
                why: format!("every check runs: {why}"),
            })
            .collect(),
        every: Some(why),
    };
    let paths = match (&change.paths, &change.base) {
        (Err(why), _) => return every(why.clone(), Vec::new()),
        (Ok(_), None) => return every("no merge base".to_owned(), Vec::new()),
        (Ok(paths), Some(_)) => paths.clone(),
    };
    if force.all {
        return every("--all".to_owned(), paths);
    }
    if let Some(p) = paths.iter().find(|p| !listed(checks, p)) {
        let why = format!("{p} is an input of no check and not documentation");
        return every(why, paths);
    }
    let decisions: Vec<Decision> = checks
        .iter()
        .map(|c| {
            let (run, why) = if c.inputs == Inputs::Everything {
                (true, "reads every path".to_owned())
            } else if let Some(p) = paths.iter().find(|p| c.inputs.contains(p)) {
                (true, format!("changes {p}"))
            } else if c.cpp && force.cpp {
                (true, "--all-cpp".to_owned())
            } else {
                (false, String::new())
            };
            Decision {
                name: c.name.clone(),
                run,
                why,
            }
        })
        .collect();
    let mut base_record = None;
    if let (true, Some(base)) = (decisions.iter().any(|d| !d.run), &change.base) {
        match cover(base) {
            Ok(commit) => base_record = Some(commit),
            Err(why) => return every(why, paths),
        }
    }
    let base = change.base.clone().unwrap_or_default();
    Plan {
        checks: decisions
            .into_iter()
            .map(|d| {
                if d.run {
                    d
                } else {
                    Decision {
                        why: skip_reason(&base),
                        ..d
                    }
                }
            })
            .collect(),
        base: change.base.clone(),
        base_record,
        changed: paths,
        every: None,
    }
}

/// The commit whose published passing record (on `origin`) covers `base`: `base` itself, or a
/// parent of it with the same tree, as the head of the pull request that a merge commit on
/// `main` merged when the branch was up to date (VER-058).
pub fn covering_record(dir: &Path, base: &str) -> Result<String, String> {
    let none = || format!("no published passing record covers main at {base}");
    if !publish::fetch_notes(dir).unwrap_or(false) {
        return Err(none());
    }
    let tree_of = |c: &str| git_in(dir, &["rev-parse", &format!("{c}^{{tree}}")]);
    let tree = tree_of(base).map_err(|_| none())?;
    let line = git_in(dir, &["rev-list", "--parents", "-n", "1", base]).map_err(|_| none())?;
    for commit in line.split_whitespace() {
        if tree_of(commit).ok().as_deref() != Some(tree.as_str()) {
            continue;
        }
        let Some(text) = publish::published(dir, commit) else {
            continue;
        };
        let Ok(rec) = serde_json::from_str::<Value>(text.trim()) else {
            continue;
        };
        let field = |k: &str| rec.get(k).and_then(Value::as_str);
        if field("commit") == Some(commit)
            && field("tree") == Some(tree.as_str())
            && record::gate_problems(&rec).is_empty()
        {
            return Ok(commit.to_owned());
        }
    }
    Err(none())
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
        "changed": plan.changed.iter().take(LISTED).collect::<Vec<_>>(),
        "changed_count": plan.changed.len(),
        "checks": decisions,
        "documentation": DOCS.patterns,
        "every": plan.every,
        "inputs": inputs,
    })
}

/// Lines describing `plan`, for the run and `verify list`.
pub fn describe(plan: &Plan, checks: &[Check]) -> Vec<String> {
    const SHOWN: usize = 10;
    let mut out = Vec::new();
    let base = plan.base.as_deref().unwrap_or("none");
    if let Some(why) = &plan.every {
        out.push(format!("every check runs: {why}"));
    } else {
        let ran = plan.checks.iter().filter(|d| d.run).count();
        out.push(format!(
            "{} changed path(s) against main at {base}: {ran} of {} checks run",
            plan.changed.len(),
            plan.checks.len()
        ));
        if let Some(rec) = &plan.base_record {
            out.push(format!("the base passed in the published record of {rec}"));
        }
    }
    for path in plan.changed.iter().take(SHOWN) {
        out.push(format!("  changed: {path}"));
    }
    if plan.changed.len() > SHOWN {
        out.push(format!("  and {} more", plan.changed.len() - SHOWN));
    }
    for (d, c) in plan.checks.iter().zip(checks) {
        let what = if d.run { "runs" } else { "skipped" };
        out.push(format!(
            "  {:40} {:16} {:7} [{}] {}",
            d.name,
            c.host.name(),
            what,
            c.inputs.label(),
            if d.run { d.why.as_str() } else { "" }
        ));
    }
    out
}

/// Why the skips of `record` do not hold for `commit`, re-derived from git with the current
/// inputs (VER-058); empty when they hold. A record whose checks all ran needs nothing.
pub fn recheck(dir: &Path, rec: &Value, checks: &[Check]) -> Vec<String> {
    let entries = rec
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut problems = Vec::new();
    if rec.get("format").and_then(Value::as_str) == Some(record::FORMAT) {
        for c in checks {
            if !entries
                .iter()
                .any(|e| e.get("name").and_then(Value::as_str) == Some(c.name.as_str()))
            {
                problems.push(format!("{}: not in the record", c.name));
            }
        }
    }
    let skipped: Vec<&str> = entries
        .iter()
        .filter(|e| e.get("result").and_then(Value::as_str) == Some("skipped"))
        .filter_map(|e| e.get("name").and_then(Value::as_str))
        .collect();
    if skipped.is_empty() {
        return problems;
    }
    let scope = if rec.get("format").and_then(Value::as_str) == Some(record::FORMAT) {
        rec.get("scope")
    } else {
        rec.get("cpp")
    };
    let Some(base) = scope.and_then(|s| s.get("base")).and_then(Value::as_str) else {
        problems.push("checks are skipped, but the record names no base".to_owned());
        return problems;
    };
    let commit = rec
        .get("commit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let change = change_from(dir, commit, base);
    let plan = plan(checks, &change, Force::default(), |b| {
        covering_record(dir, b)
    });
    for name in skipped {
        match plan.checks.iter().find(|d| d.name == name) {
            Some(d) if d.run => problems.push(format!("{name}: skipped, but {}", d.why)),
            Some(_) => {}
            None => problems.push(format!("{name}: skipped, but there is no such check")),
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

    /// Every base but the empty one is covered by the record of `cccc…`.
    fn covered(base: &str) -> Result<String, String> {
        if base.is_empty() {
            Err("no base".to_owned())
        } else {
            Ok("c".repeat(40))
        }
    }

    /// The names of the checks that run.
    fn running(change: &Change, force: Force) -> Vec<String> {
        plan(&all(), change, force, covered)
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
    fn documentation_runs_spec_only() {
        let docs = changed(&[
            "docs/06-verification.md",
            "changelog.d/chore-x.md",
            "README.md",
            "AGENTS.md",
            "specs/manifest.tsv",
        ]);
        let p = plan(&all(), &docs, Force::default(), covered);
        assert_eq!(p.every, None);
        assert_eq!(p.base_record.as_deref(), Some("c".repeat(40).as_str()));
        assert_eq!(running(&docs, Force::default()), ["spec"]);
        let skipped = p.checks.iter().find(|d| d.name == "clippy");
        assert_eq!(
            skipped.map(|d| d.why.clone()),
            Some(skip_reason(&"b".repeat(40)))
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
        let rust = names(|c| !c.cpp);
        let cpp = names(|c| c.cpp || c.name == "spec");
        let everything = names(|_| true);
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
            names(|c| c.name != "header")
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
        let default = Force::default();
        let all = Force {
            all: true,
            cpp: false,
        };
        // An unlisted path, alone or with documentation.
        for paths in [
            &[".gitignore"][..],
            &["docs/a.md", "specs/fetch.sh"],
            &[".github/CODEOWNERS"],
        ] {
            let p = plan(
                &super::super::checks::all(),
                &changed(paths),
                default,
                covered,
            );
            assert!(
                p.every.is_some_and(|w| w.contains("an input of no check")),
                "{paths:?}"
            );
        }
        assert_eq!(running(&changed(&["docs/a.md"]), all), everything);
        let unknown = Change {
            base: None,
            paths: Err("origin/main could not be fetched".to_owned()),
        };
        assert_eq!(running(&unknown, default), everything);
        // No record covers the base.
        let p = plan(
            &super::super::checks::all(),
            &changed(&["docs/a.md"]),
            default,
            |b| Err(format!("nothing covers {b}")),
        );
        assert_eq!(p.every, Some(format!("nothing covers {}", "b".repeat(40))));
        assert!(p.checks.iter().all(|d| d.run));
        // The base is asked about only when a check would be skipped.
        let p = plan(
            &super::super::checks::all(),
            &changed(&["xtask/src/main.rs"]),
            default,
            |_| Err("asked".to_owned()),
        );
        assert_eq!(p.every, None);
        assert_eq!(p.base_record, None);
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
            repo.git(&["init", "-q", "-b", "main"]);
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

        /// Publishes a passing record of `commit` as a note on this repository's notes ref.
        fn publish(&self, commit: &str, checks: &Value) {
            let tree = self.git(&["rev-parse", &format!("{commit}^{{tree}}")]);
            let rec =
                json!({"format": record::FORMAT, "commit": commit, "tree": tree, "checks": checks});
            self.git(&[
                "notes",
                "--ref=refs/notes/verify",
                "add",
                "-f",
                "-m",
                &record::canonical(&rec),
                commit,
            ]);
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
        let p = plan(&all(), &c, Force::default(), |b| {
            covering_record(&clone.0, b)
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
        json!({"format": record::FORMAT, "commit": commit, "scope": {"base": base}, "checks": entries})
    }

    // req: VER-058
    #[test]
    fn recheck_rederives_the_skips() {
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
}
