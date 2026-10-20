//! Publishing an evidence record and checking a published one (VER-057, VER-058).
//!
//! A record is published as a git note on the verified commit under `refs/notes/verify`, pushed
//! to `origin`. A passing record then sets the commit status `aldis/verify`, which branch
//! protection on `main` requires.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::{checks, git, git_in, record, scope};
use crate::repo;

/// The notes ref that holds the records.
const NOTES: &str = "refs/notes/verify";
/// Where `origin`'s notes are fetched to before a merge or a check.
const NOTES_ORIGIN: &str = "refs/notes/verify-origin";
/// The commit status a passing record sets.
const CONTEXT: &str = "aldis/verify";

/// Reads and parses a record; checks that it is for `commit` and its tree.
fn load(text: &str, commit: &str) -> Result<Value, String> {
    let record: Value = serde_json::from_str(text.trim()).map_err(|e| format!("record: {e}"))?;
    let tree = git(&["rev-parse", &format!("{commit}^{{tree}}")])?;
    let field = |k: &str| record.get(k).and_then(Value::as_str).unwrap_or("");
    if field("commit") != commit {
        return Err(format!(
            "the record is for commit {}, not {commit}",
            field("commit")
        ));
    }
    if field("tree") != tree {
        return Err(format!(
            "the record is for tree {}, not {tree}",
            field("tree")
        ));
    }
    Ok(record)
}

/// The GitHub Actions run that made the record (VER-060), if it was not made locally.
fn run_url(record: &Value) -> Option<&str> {
    let source = record.get("source")?;
    if source.get("kind")?.as_str()? == "github-actions" {
        source.get("run")?.as_str()
    } else {
        None
    }
}

/// Fetches `origin`'s notes into [`NOTES_ORIGIN`] in the repository at `dir`; false if
/// `origin` has none yet.
pub fn fetch_notes(dir: &Path) -> Result<bool, String> {
    let exists = Command::new("git")
        .args(["ls-remote", "--exit-code", "origin", NOTES])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("git: {e}"))?;
    if !exists.success() {
        return Ok(false);
    }
    git_in(
        dir,
        &["fetch", "-q", "origin", &format!("+{NOTES}:{NOTES_ORIGIN}")],
    )?;
    Ok(true)
}

/// The record of `commit` published on `origin`, as last fetched by [`fetch_notes`].
pub fn published(dir: &Path, commit: &str) -> Option<String> {
    git_in(
        dir,
        &["notes", &format!("--ref={NOTES_ORIGIN}"), "show", commit],
    )
    .ok()
}

/// Adds the record as a note on `commit` on top of `origin`'s notes and pushes them.
fn push_note(commit: &str, path: &str) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..3 {
        if fetch_notes(&repo::root())? {
            // Start from origin's notes; a local note on the same commit is replaced below.
            git(&["update-ref", NOTES, NOTES_ORIGIN])?;
        }
        git(&[
            "notes",
            &format!("--ref={NOTES}"),
            "add",
            "-f",
            "-F",
            path,
            commit,
        ])?;
        match git(&["push", "-q", "origin", NOTES]) {
            Ok(_) => return Ok(()),
            // Someone else pushed a note in between: fetch theirs and add ours again.
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Why a record of the commit it names does not meet the merge gate (VER-058): the record's own
/// rules, and its skips re-derived from git rather than trusted.
fn problems(record: &Value) -> Vec<String> {
    let mut problems = record::gate_problems(record);
    problems.extend(scope::recheck(&repo::root(), record, &checks::all()));
    problems
}

/// Publishes the local record of `commit` from `target/verify/<commit>/record.json`.
pub fn publish(commit: &str) -> Result<usize, String> {
    let path = repo::root()
        .join("target/verify")
        .join(commit)
        .join("record.json");
    let text = repo::read(&path)
        .map_err(|e| format!("{e}\nrun `cargo xtask verify` on {commit} first"))?;
    let record = load(&text, commit)?;
    let problems = problems(&record);
    if !problems.is_empty() {
        return Err(format!(
            "the record does not pass the merge gate, so it is not published:\n  {}",
            problems.join("\n  ")
        ));
    }
    git(&["fetch", "-q", "origin", "main"])?;
    if git(&["merge-base", "--is-ancestor", "origin/main", commit]).is_err() {
        return Err(format!(
            "{commit} does not contain the tip of origin/main: merge or rebase onto main, then \
             verify again (VER-058)"
        ));
    }
    let on_github = Command::new("gh")
        .args([
            "api",
            "--silent",
            &format!("repos/{{owner}}/{{repo}}/commits/{commit}"),
        ])
        .current_dir(repo::root())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("gh: {e}"))?;
    if !on_github.success() {
        return Err(format!(
            "{commit} is not on GitHub yet: push the branch first"
        ));
    }
    push_note(commit, &path.to_string_lossy())?;
    let sha = record::sha256_hex(text.trim().as_bytes());
    let mut gh = Command::new("gh");
    gh.args(["api", "--silent", "-X", "POST"])
        .arg(format!("repos/{{owner}}/{{repo}}/statuses/{commit}"))
        .args(["-f", "state=success", "-f", &format!("context={CONTEXT}")]);
    if let Some(url) = run_url(&record) {
        gh.args([
            "-f",
            &format!("description=record sha256:{sha} (GitHub Actions)"),
        ])
        .args(["-f", &format!("target_url={url}")]);
    } else {
        gh.args(["-f", &format!("description=record sha256:{sha}")]);
    }
    let status = gh
        .current_dir(repo::root())
        .status()
        .map_err(|e| format!("gh: {e}"))?;
    if !status.success() {
        return Err(format!(
            "setting the commit status {CONTEXT} on {commit} failed"
        ));
    }
    println!("verify: published the record of {commit} ({NOTES}, sha256 {sha})");
    println!("verify: commit status {CONTEXT} = success");
    Ok(0)
}

/// Checks the record of `rev` published on `origin` against the merge gate.
pub fn check(rev: &str) -> Result<usize, String> {
    let commit = git(&["rev-parse", rev])?;
    if !fetch_notes(&repo::root())? {
        return Err(format!("origin has no {NOTES} yet"));
    }
    let text = published(&repo::root(), &commit)
        .ok_or_else(|| format!("origin has no published record for {commit}"))?;
    let record = load(&text, &commit)?;
    let problems = problems(&record);
    println!(
        "verify: record of {commit}, sha256 {}, finished {}",
        record::sha256_hex(text.trim().as_bytes()),
        record
            .get("finished")
            .and_then(Value::as_str)
            .unwrap_or("?")
    );
    match run_url(&record) {
        Some(url) => println!("verify: made by GitHub Actions run {url}"),
        None => println!("verify: made locally"),
    }
    let skipped = record
        .get("checks")
        .and_then(Value::as_array)
        .map_or(0, |c| c.iter().filter(|e| e["result"] == "skipped").count());
    if skipped > 0 {
        println!("verify: {skipped} check(s) skipped; their skips re-derived from git (VER-058)");
    }
    for p in &problems {
        println!("verify: gate: {p}");
    }
    if problems.is_empty() {
        println!("verify: the record passes the merge gate (VER-058)");
    }
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // req: VER-057, VER-060
    #[test]
    fn run_url_only_for_a_github_actions_record() {
        let url = "https://github.com/o/r/actions/runs/7/attempts/1";
        let ci = json!({"source": {"kind": "github-actions", "run": url}});
        assert_eq!(run_url(&ci), Some(url));
        assert_eq!(run_url(&json!({"source": {"kind": "local"}})), None);
        assert_eq!(run_url(&json!({})), None);
    }
}
