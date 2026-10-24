//! Publishing an evidence record and checking a published one (VER-057, VER-058).
//!
//! A record is published as a git note on the verified commit under `refs/notes/verify`, pushed
//! to `origin`. A passing record of a commit that contains the tip of `main` then sets the commit
//! status `aldis/verify`, which branch protection on `main` requires. A passing record of a
//! commit that no longer contains it, because `main` moved during the run, is published all the
//! same, without the status: it can be a source for a later commit (VER-056), and the head needs
//! a merge from `main` before it can merge (VER-058).

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::{checks, git_in, record, scope};
use crate::repo;

/// The notes ref that holds the records.
const NOTES: &str = "refs/notes/verify";
/// Where `origin`'s notes are fetched to before a merge or a check.
const NOTES_ORIGIN: &str = "refs/notes/verify-origin";
/// The commit status a passing record sets.
const CONTEXT: &str = "aldis/verify";

/// What `verify publish` needs from GitHub: whether it has a commit, and setting a commit status.
/// [`Gh`] does it with the `gh` command; the tests record the calls instead.
pub trait Forge {
    /// Whether the repository on GitHub has `commit`.
    fn has_commit(&self, dir: &Path, commit: &str) -> Result<bool, String>;
    /// Sets the status [`CONTEXT`] of `commit` to success with `description`, linked to `url`.
    fn set_status(
        &self,
        dir: &Path,
        commit: &str,
        description: &str,
        url: Option<&str>,
    ) -> Result<(), String>;
}

/// [`Forge`] through the `gh` command.
pub struct Gh;

impl Forge for Gh {
    fn has_commit(&self, dir: &Path, commit: &str) -> Result<bool, String> {
        let status = Command::new("gh")
            .args([
                "api",
                "--silent",
                &format!("repos/{{owner}}/{{repo}}/commits/{commit}"),
            ])
            .current_dir(dir)
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("gh: {e}"))?;
        Ok(status.success())
    }

    fn set_status(
        &self,
        dir: &Path,
        commit: &str,
        description: &str,
        url: Option<&str>,
    ) -> Result<(), String> {
        let mut gh = Command::new("gh");
        gh.args(["api", "--silent", "-X", "POST"])
            .arg(format!("repos/{{owner}}/{{repo}}/statuses/{commit}"))
            .args(["-f", "state=success", "-f", &format!("context={CONTEXT}")])
            .args(["-f", &format!("description={description}")]);
        if let Some(url) = url {
            gh.args(["-f", &format!("target_url={url}")]);
        }
        let status = gh
            .current_dir(dir)
            .status()
            .map_err(|e| format!("gh: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "setting the commit status {CONTEXT} on {commit} failed"
            ))
        }
    }
}

/// Reads and parses a record; checks that it is for `commit` and its tree.
fn load(dir: &Path, text: &str, commit: &str) -> Result<Value, String> {
    let record: Value = serde_json::from_str(text.trim()).map_err(|e| format!("record: {e}"))?;
    let tree = git_in(dir, &["rev-parse", &format!("{commit}^{{tree}}")])?;
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
fn push_note(dir: &Path, commit: &str, path: &str) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..3 {
        if fetch_notes(dir)? {
            // Start from origin's notes; a local note on the same commit is replaced below.
            git_in(dir, &["update-ref", NOTES, NOTES_ORIGIN])?;
        }
        git_in(
            dir,
            &[
                "notes",
                &format!("--ref={NOTES}"),
                "add",
                "-f",
                "-F",
                path,
                commit,
            ],
        )?;
        match git_in(dir, &["push", "-q", "origin", NOTES]) {
            Ok(_) => return Ok(()),
            // Someone else pushed a note in between: fetch theirs and add ours again.
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Why a record of the commit it names does not pass on its own (VER-058): the record's own
/// rules, and its skips re-derived from git in the repository at `dir` rather than trusted.
/// Whether the commit contains the tip of `main` is not part of it: see [`contains_main`].
fn problems(dir: &Path, record: &Value) -> Vec<String> {
    let mut problems = record::gate_problems(record);
    problems.extend(scope::recheck(dir, record, &checks::all()));
    problems
}

/// Whether `commit` contains the tip of `origin/main`, fetched first (VER-058).
fn contains_main(dir: &Path, commit: &str) -> Result<bool, String> {
    git_in(dir, &["fetch", "-q", "origin", "main"])?;
    Ok(git_in(dir, &["merge-base", "--is-ancestor", "origin/main", commit]).is_ok())
}

/// The problem of a head that does not contain the tip of `main` (VER-058).
fn behind_main(commit: &str) -> String {
    format!(
        "{commit} does not contain the tip of origin/main: merge main into the branch (or rebase \
         onto it) and verify again before it can merge (VER-058)"
    )
}

/// Publishes the local record of `commit` from `target/verify/<commit>/record.json`.
pub fn publish(commit: &str) -> Result<usize, String> {
    publish_in(&repo::root(), commit, &Gh)
}

/// Publishes the local record of `commit` in the repository at `dir` (VER-057, VER-058): only a
/// record that passes on its own, of a commit that is on GitHub, is published, as a git note on
/// `origin`. When the commit contains the tip of `main`, the commit status is set and the
/// result is 0. When it does not, because `main` moved during the run, the note is published
/// all the same, so that a later commit can take results from it (VER-056), but no status is
/// set, and the result is 1: the head needs a merge from `main` before it can merge.
pub(super) fn publish_in(dir: &Path, commit: &str, forge: &dyn Forge) -> Result<usize, String> {
    let path = dir.join("target/verify").join(commit).join("record.json");
    let text = repo::read(&path)
        .map_err(|e| format!("{e}\nrun `cargo xtask verify` on {commit} first"))?;
    let record = load(dir, &text, commit)?;
    let problems = problems(dir, &record);
    if !problems.is_empty() {
        return Err(format!(
            "the record does not pass the merge gate, so it is not published:\n  {}",
            problems.join("\n  ")
        ));
    }
    let contains = contains_main(dir, commit)?;
    if !forge.has_commit(dir, commit)? {
        return Err(format!(
            "{commit} is not on GitHub yet: push the branch first"
        ));
    }
    push_note(dir, commit, &path.to_string_lossy())?;
    let sha = record::sha256_hex(text.trim().as_bytes());
    println!("verify: published the record of {commit} ({NOTES}, sha256 {sha})");
    if !contains {
        println!(
            "verify: main moved during the run: the record is published, and a later commit can \
             take results from it (VER-056), but the commit status {CONTEXT} is not set"
        );
        println!("verify: gate: {}", behind_main(commit));
        return Ok(1);
    }
    let url = run_url(&record);
    let description = if url.is_some() {
        format!("record sha256:{sha} (GitHub Actions)")
    } else {
        format!("record sha256:{sha}")
    };
    forge.set_status(dir, commit, &description, url)?;
    println!("verify: commit status {CONTEXT} = success");
    Ok(0)
}

/// Checks the record of `rev` published on `origin` against the merge gate.
pub fn check(rev: &str) -> Result<usize, String> {
    check_in(&repo::root(), rev)
}

/// Checks the record of `rev` published on the `origin` of the repository at `dir` against the
/// merge gate (VER-058): the record passes on its own, and `rev` contains the tip of `main`.
pub(super) fn check_in(dir: &Path, rev: &str) -> Result<usize, String> {
    let commit = git_in(dir, &["rev-parse", rev])?;
    if !fetch_notes(dir)? {
        return Err(format!("origin has no {NOTES} yet"));
    }
    let text = published(dir, &commit)
        .ok_or_else(|| format!("origin has no published record for {commit}"))?;
    let record = load(dir, &text, &commit)?;
    let mut problems = problems(dir, &record);
    if !contains_main(dir, &commit)? {
        problems.push(behind_main(&commit));
    }
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
    let count = |result: &str| {
        record
            .get("checks")
            .and_then(Value::as_array)
            .map_or(0, |c| c.iter().filter(|e| e["result"] == result).count())
    };
    let (skipped, reused) = (count("skipped"), count("reused"));
    if skipped + reused > 0 {
        println!(
            "verify: {skipped} check(s) skipped, {reused} reused; each re-derived from git and \
             the source's published record (VER-058)"
        );
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
