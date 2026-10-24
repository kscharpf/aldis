//! Publishing an evidence record and checking a published one (VER-057, VER-058, VER-064).
//!
//! A record is published as a git note on the verified commit under `refs/notes/verify`, pushed
//! to `origin`, with its SSH signature as a note on the same commit under
//! `refs/notes/verify-signature` (`sign.rs`); both refs are pushed together. A passing record of
//! a commit that contains the tip of `main` then sets the commit status `aldis/verify`, which
//! branch protection on `main` requires. A passing record of a commit that no longer contains it,
//! because `main` moved during the run, is published all the same, without the status: it can be
//! a source for a later commit (VER-056), and the head needs a merge from `main` before it can
//! merge (VER-058).
//!
//! A local record is signed when it is published, with the key git signs with. A record made by
//! the GitHub Actions run of VER-060 is published unsigned and sets no status: it counts only once
//! an allowed signer has countersigned it with `verify sign`, after checking that the run is this
//! repository's `verify.yml` on that commit and that its artefact holds the same record.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::sign::{self, RECORD_NAMESPACE, Scratch, Signer, Signers};
use super::{checks, git_in, record, scope};
use crate::repo;

/// The notes ref that holds the records.
const NOTES: &str = "refs/notes/verify";
/// Where `origin`'s notes are fetched to before a merge or a check.
const NOTES_ORIGIN: &str = "refs/notes/verify-origin";
/// The notes ref that holds the records' signatures (VER-064).
const SIGNATURES: &str = "refs/notes/verify-signature";
/// Where `origin`'s signatures are fetched to.
const SIGNATURES_ORIGIN: &str = "refs/notes/verify-signature-origin";
/// The commit status a passing record sets.
const CONTEXT: &str = "aldis/verify";
/// The workflow of the verification runs on GitHub-hosted runners (VER-060).
const WORKFLOW: &str = ".github/workflows/verify.yml";

/// A GitHub Actions run, as `verify sign` checks it before countersigning its record (VER-060,
/// VER-064).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    /// The run's page, `https://github.com/<owner>/<repo>/actions/runs/<id>`.
    pub html_url: String,
    /// The attempt.
    pub attempt: u64,
    /// The commit it ran on.
    pub head_sha: String,
    /// The workflow file it ran, `.github/workflows/verify.yml`.
    pub path: String,
    /// `completed` once it finished.
    pub status: String,
    /// The record in its artefact `verify-<commit>`, if it has one.
    pub record: Option<String>,
}

/// What `verify publish` and `verify sign` need from GitHub: whether it has a commit, setting a
/// commit status, and a GitHub Actions run of this repository. [`Gh`] does it with the `gh`
/// command; the tests record the calls instead.
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
    /// The run attempt of this repository that `url` names, with the record of `commit` in its
    /// artefact.
    fn run(&self, dir: &Path, url: &str, commit: &str) -> Result<Run, String>;
}

/// [`Forge`] through the `gh` command.
pub struct Gh;

/// Runs `gh` in `dir`; returns its standard output.
fn gh(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "gh {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The run ID and attempt that a run URL ends in, `.../actions/runs/<id>/attempts/<n>`.
fn run_ids(url: &str) -> Option<(&str, &str)> {
    let mut parts = url.rsplit('/');
    let attempt = parts.next()?;
    let (attempts, id, runs) = (parts.next()?, parts.next()?, parts.next()?);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (attempts == "attempts" && runs == "runs" && digits(id) && digits(attempt))
        .then_some((id, attempt))
}

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

    fn run(&self, dir: &Path, url: &str, commit: &str) -> Result<Run, String> {
        let (id, attempt) =
            run_ids(url).ok_or_else(|| format!("{url} does not name a run attempt"))?;
        let text = gh(
            dir,
            &[
                "api",
                &format!("repos/{{owner}}/{{repo}}/actions/runs/{id}/attempts/{attempt}"),
            ],
        )?;
        let run: Value = serde_json::from_str(&text).map_err(|e| format!("run {id}: {e}"))?;
        let field = |k: &str| run.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let scratch = Scratch::new("run-artefact")?;
        let out = scratch.path().to_string_lossy().into_owned();
        let artefact = format!("verify-{commit}");
        let record = gh(dir, &["run", "download", id, "-n", &artefact, "-D", &out])
            .ok()
            .and_then(|_| {
                [scratch.path().join(commit), scratch.path().to_path_buf()]
                    .iter()
                    .find_map(|d| std::fs::read_to_string(d.join("record.json")).ok())
            });
        Ok(Run {
            html_url: field("html_url"),
            attempt: run.get("run_attempt").and_then(Value::as_u64).unwrap_or(0),
            head_sha: field("head_sha"),
            path: field("path"),
            status: field("status"),
            record,
        })
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

/// Why the record `text` of `commit` made by the run `url` must not be countersigned: `run`,
/// as GitHub reports it, is not that attempt of this repository's [`WORKFLOW`] on `commit`, it
/// has not finished, or its artefact does not hold the same record (VER-060, VER-064).
fn run_problems(run: &Run, url: &str, commit: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if format!("{}/attempts/{}", run.html_url, run.attempt) != url {
        problems.push(format!(
            "the record names the run {url}, but GitHub reports {}/attempts/{}",
            run.html_url, run.attempt
        ));
    }
    if run.head_sha != commit {
        problems.push(format!("the run is of {}, not {commit}", run.head_sha));
    }
    if run.path != WORKFLOW {
        problems.push(format!("the run is of {}, not {WORKFLOW}", run.path));
    }
    if run.status != "completed" {
        problems.push(format!("the run is {}, not completed", run.status));
    }
    match &run.record {
        None => problems.push(format!("the run's artefact verify-{commit} has no record")),
        Some(r) if r.trim() != text.trim() => problems.push(format!(
            "the published record (sha256 {}) is not the one in the run's artefact (sha256 {})",
            record::sha256_hex(text.trim().as_bytes()),
            record::sha256_hex(r.trim().as_bytes())
        )),
        Some(_) => {}
    }
    problems
}

/// Whether `origin` has `refs` (`git ls-remote`), each in order.
fn on_origin(dir: &Path, refs: &[&str]) -> Result<Vec<bool>, String> {
    let mut args = vec!["ls-remote", "origin"];
    args.extend_from_slice(refs);
    let listed = git_in(dir, &args)?;
    Ok(refs
        .iter()
        .map(|r| {
            listed
                .lines()
                .any(|l| l.split_whitespace().nth(1) == Some(*r))
        })
        .collect())
}

/// Whether the ref `name` exists in the repository at `dir`.
fn exists(dir: &Path, name: &str) -> bool {
    git_in(dir, &["rev-parse", "--verify", "-q", name]).is_ok()
}

/// Fetches `origin`'s notes into [`NOTES_ORIGIN`], and their signatures into
/// [`SIGNATURES_ORIGIN`], in the repository at `dir`; false if `origin` has no records yet.
pub fn fetch_notes(dir: &Path) -> Result<bool, String> {
    let has = on_origin(dir, &[NOTES, SIGNATURES])?;
    let (Some(&records), Some(&signatures)) = (has.first(), has.get(1)) else {
        return Ok(false);
    };
    if !records {
        return Ok(false);
    }
    let notes = format!("+{NOTES}:{NOTES_ORIGIN}");
    let sigs = format!("+{SIGNATURES}:{SIGNATURES_ORIGIN}");
    if signatures {
        git_in(dir, &["fetch", "-q", "origin", &notes, &sigs])?;
    } else {
        git_in(dir, &["fetch", "-q", "origin", &notes])?;
        if exists(dir, SIGNATURES_ORIGIN) {
            git_in(dir, &["update-ref", "-d", SIGNATURES_ORIGIN])?;
        }
    }
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

/// The signature of the record of `commit` published on `origin`, as last fetched.
fn published_signature(dir: &Path, commit: &str) -> Option<String> {
    git_in(
        dir,
        &[
            "notes",
            &format!("--ref={SIGNATURES_ORIGIN}"),
            "show",
            commit,
        ],
    )
    .ok()
}

/// Who signed the published record `text` of `commit`, when its published signature verifies
/// against `signers` (VER-064); otherwise why not.
pub fn authenticate(
    dir: &Path,
    signers: &Signers,
    commit: &str,
    text: &str,
) -> Result<Signer, String> {
    let signature = published_signature(dir, commit)
        .ok_or_else(|| format!("its record is not signed (no note under {SIGNATURES})"))?;
    signers
        .verify(text.trim().as_bytes(), &signature, RECORD_NAMESPACE)
        .map_err(|why| format!("the signature of its record {why}"))
}

/// Adds the record `text` as a note on `commit`, with `signature` as its signature note or, with
/// none, no signature note, on top of `origin`'s notes, and pushes both refs together.
fn push_note(dir: &Path, commit: &str, text: &str, signature: Option<&str>) -> Result<(), String> {
    let scratch = Scratch::new("note")?;
    let record_file = scratch.file("record.json", text.as_bytes())?;
    let record_file = record_file.to_string_lossy();
    let signature_file = signature
        .map(|s| scratch.file("signature", s.as_bytes()))
        .transpose()?;
    let mut last = String::new();
    for _ in 0..3 {
        if fetch_notes(dir)? {
            // Start from origin's notes; a local note on the same commit is replaced below.
            git_in(dir, &["update-ref", NOTES, NOTES_ORIGIN])?;
        }
        if exists(dir, SIGNATURES_ORIGIN) {
            git_in(dir, &["update-ref", SIGNATURES, SIGNATURES_ORIGIN])?;
        } else if exists(dir, SIGNATURES) {
            git_in(dir, &["update-ref", "-d", SIGNATURES])?;
        }
        let notes = format!("--ref={NOTES}");
        git_in(
            dir,
            &["notes", &notes, "add", "-f", "-F", &record_file, commit],
        )?;
        let sigs = format!("--ref={SIGNATURES}");
        match &signature_file {
            Some(file) => {
                let file = file.to_string_lossy();
                git_in(dir, &["notes", &sigs, "add", "-f", "-F", &file, commit])?;
            }
            None if exists(dir, SIGNATURES) => {
                git_in(dir, &["notes", &sigs, "remove", "--ignore-missing", commit])?;
            }
            None => {}
        }
        let mut push = vec!["push", "-q", "--atomic", "origin", NOTES];
        if exists(dir, SIGNATURES) {
            push.push(SIGNATURES);
        }
        match git_in(dir, &push) {
            Ok(_) => return Ok(()),
            // Someone else pushed a note in between: fetch theirs and add ours again.
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Why a record of the commit it names does not pass on its own (VER-058): the record's own
/// rules, and its skips re-derived from git in the repository at `dir` rather than trusted.
/// Whether the commit contains the tip of `main` is not part of it: see [`contains_main`]; nor
/// is the record's own signature, which is checked where it is made or read.
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

/// The signature of the record `text` of `commit`, made with the key git signs with, and who
/// made it, checked against the allowed signers before anything is pushed (VER-064).
fn sign_record(dir: &Path, commit: &str, text: &str) -> Result<(String, Signer), String> {
    let signature = sign::sign(dir, text.as_bytes(), RECORD_NAMESPACE)?;
    let signers = Signers::of(dir, commit)?;
    let signer = signers
        .verify(text.as_bytes(), &signature, RECORD_NAMESPACE)
        .map_err(|why| {
            format!(
                "your signature {why}: an allowed signer adds your key to {} in a reviewed \
                 commit first (docs/06-verification.md §7, VER-064)",
                sign::ALLOWED_SIGNERS
            )
        })?;
    Ok((signature, signer))
}

/// After a signed record of `commit` with the SHA-256 `sha` is published: sets the commit status
/// when `commit` contains the tip of `main` (0), and says that it cannot merge yet otherwise (1).
fn set_status(
    dir: &Path,
    commit: &str,
    record: &Value,
    sha: &str,
    contains: bool,
    forge: &dyn Forge,
) -> Result<usize, String> {
    if !contains {
        println!(
            "verify: main moved during the run: the record is published, and a later commit can \
             take results from it (VER-056), but the commit status {CONTEXT} is not set"
        );
        println!("verify: gate: {}", behind_main(commit));
        return Ok(1);
    }
    let url = run_url(record);
    let description = if url.is_some() {
        format!("record sha256:{sha} (GitHub Actions)")
    } else {
        format!("record sha256:{sha}")
    };
    forge.set_status(dir, commit, &description, url)?;
    println!("verify: commit status {CONTEXT} = success");
    Ok(0)
}

/// Publishes the local record of `commit` from `target/verify/<commit>/record.json`.
pub fn publish(commit: &str) -> Result<usize, String> {
    publish_in(&repo::root(), commit, &Gh)
}

/// Publishes the local record of `commit` in the repository at `dir` (VER-057, VER-058,
/// VER-064): only a record that passes on its own, of a commit that is on GitHub, is published,
/// as a git note on `origin`. A local record is signed first, and nothing is pushed if it cannot
/// be signed with a key of the allowed signers; a record of a GitHub Actions run is published
/// unsigned, sets no status, and waits for `verify sign` (0, or 1 behind main). When the commit
/// contains the tip of `main`, the commit status is set and the result is 0. When it does not,
/// because `main` moved during the run, the note is published all the same, so that a later
/// commit can take results from it (VER-056), but no status is set, and the result is 1: the
/// head needs a merge from `main` before it can merge.
pub(super) fn publish_in(dir: &Path, commit: &str, forge: &dyn Forge) -> Result<usize, String> {
    let path = dir.join("target/verify").join(commit).join("record.json");
    let text = repo::read(&path)
        .map_err(|e| format!("{e}\nrun `cargo xtask verify` on {commit} first"))?;
    let text = text.trim();
    let record = load(dir, text, commit)?;
    let contains = contains_main(dir, commit)?;
    let problems = problems(dir, &record);
    if !problems.is_empty() {
        return Err(format!(
            "the record does not pass the merge gate, so it is not published:\n  {}",
            problems.join("\n  ")
        ));
    }
    if !forge.has_commit(dir, commit)? {
        return Err(format!(
            "{commit} is not on GitHub yet: push the branch first"
        ));
    }
    let sha = record::sha256_hex(text.as_bytes());
    if let Some(url) = run_url(&record) {
        push_note(dir, commit, text, None)?;
        println!("verify: published the record of {commit} ({NOTES}, sha256 {sha}), unsigned");
        println!(
            "verify: a record of the GitHub Actions run {url} counts once an allowed signer has \
             checked the run and countersigned it: `cargo xtask verify sign {commit}` (VER-064); \
             until then it sets no commit status and is no source"
        );
        if !contains {
            println!("verify: gate: {}", behind_main(commit));
        }
        return Ok(usize::from(!contains));
    }
    let (signature, signer) = sign_record(dir, commit, text)?;
    push_note(dir, commit, text, Some(&signature))?;
    println!(
        "verify: published the record of {commit} ({NOTES}, sha256 {sha}), signed by {signer}"
    );
    set_status(dir, commit, &record, &sha, contains, forge)
}

/// Countersigns the published record of `rev` (VER-064).
pub fn countersign(rev: &str) -> Result<usize, String> {
    let dir = repo::root();
    let commit = git_in(&dir, &["rev-parse", rev])?;
    sign_in(&dir, &commit, &Gh)
}

/// Signs the record of `commit` published on `origin` in the repository at `dir` with the key
/// git signs with, and pushes the signature (VER-064): only a record that passes on its own and,
/// when a GitHub Actions run made it, whose run is this repository's [`WORKFLOW`] on `commit`,
/// finished, with the same record in its artefact (VER-060). Then sets the commit status as
/// [`publish_in`] does. For a local record, it replaces the signature, as after a key rotation.
pub(super) fn sign_in(dir: &Path, commit: &str, forge: &dyn Forge) -> Result<usize, String> {
    if !fetch_notes(dir)? {
        return Err(format!("origin has no {NOTES} yet"));
    }
    let text = published(dir, commit)
        .ok_or_else(|| format!("origin has no published record for {commit}"))?;
    let text = text.trim();
    let record = load(dir, text, commit)?;
    let contains = contains_main(dir, commit)?;
    let mut problems = problems(dir, &record);
    if let Some(url) = run_url(&record) {
        problems.extend(run_problems(
            &forge.run(dir, url, commit)?,
            url,
            commit,
            text,
        ));
    }
    if !problems.is_empty() {
        return Err(format!(
            "the record is not signed:\n  {}",
            problems.join("\n  ")
        ));
    }
    let (signature, signer) = sign_record(dir, commit, text)?;
    push_note(dir, commit, text, Some(&signature))?;
    let sha = record::sha256_hex(text.as_bytes());
    println!("verify: signed the record of {commit} (sha256 {sha}) as {signer}");
    set_status(dir, commit, &record, &sha, contains, forge)
}

/// Checks the record of `rev` published on `origin` against the merge gate.
pub fn check(rev: &str) -> Result<usize, String> {
    check_in(&repo::root(), rev)
}

/// Checks the record of `rev` published on the `origin` of the repository at `dir` against the
/// merge gate (VER-058, VER-064): the record passes on its own, its signature verifies against
/// the allowed signers, and `rev` contains the tip of `main`.
pub(super) fn check_in(dir: &Path, rev: &str) -> Result<usize, String> {
    let commit = git_in(dir, &["rev-parse", rev])?;
    if !fetch_notes(dir)? {
        return Err(format!("origin has no {NOTES} yet"));
    }
    let text = published(dir, &commit)
        .ok_or_else(|| format!("origin has no published record for {commit}"))?;
    let record = load(dir, &text, &commit)?;
    let contains = contains_main(dir, &commit)?;
    let mut problems = problems(dir, &record);
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
    match Signers::of(dir, &commit)
        .and_then(|signers| Ok((authenticate(dir, &signers, &commit, &text)?, signers.from)))
    {
        Ok((signer, from)) => {
            println!("verify: signed by {signer}, verified against {from} (VER-064)");
        }
        Err(why) => problems.push(why),
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
             the source's published, signed record (VER-058)"
        );
    }
    if !contains {
        problems.push(behind_main(&commit));
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
    use super::super::sign::testing::TestKey;
    use super::super::testing::{Forge, Repo};
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
        assert_eq!(run_ids(url), Some(("7", "1")));
        assert_eq!(run_ids("https://github.com/o/r/actions/runs/7"), None);
        assert_eq!(
            run_ids("https://github.com/o/r/actions/runs/x/attempts/1"),
            None
        );
        assert_eq!(run_ids("https://github.com/o/r/pulls/7/attempts/1"), None);
    }

    /// A passing record of `commit` in `repo`, every check run, made where `source` says.
    fn passing(repo: &Repo, commit: &str, source: &Value) -> Value {
        let entries: Vec<Value> = checks::all()
            .iter()
            .map(|c| {
                json!({"name": c.name, "host": c.host.name(), "gating": c.gating,
                "result": "pass"})
            })
            .collect();
        let mut rec = repo.record_for(record::FORMAT, commit, &json!(entries));
        rec["source"] = source.clone();
        rec
    }

    /// `origin` with a commit on `main`, and a clone on a branch with one commit of its own that
    /// is pushed, whose local record passes; the clone signs with origin's key.
    fn branch(name: &str, source: &Value) -> (Repo, Repo, String) {
        let origin = Repo::new(&format!("{name}-origin"));
        origin.write("docs/a.md", "a\n");
        origin.commit("start");
        let clone = Repo::clone_of(&origin, &format!("{name}-clone"));
        clone.sign_with(clone.key());
        clone.write(".git/info/exclude", "target/\n");
        clone.git(&["checkout", "-q", "-b", "topic"]);
        clone.write("docs/a.md", "a, on the topic\n");
        let head = clone.commit("topic");
        clone.git(&["push", "-q", "origin", "topic"]);
        let rec = passing(&clone, &head, source);
        clone.write(
            &format!("target/verify/{head}/record.json"),
            &record::canonical(&rec),
        );
        (origin, clone, head)
    }

    // req: VER-057, VER-058, VER-064
    #[test]
    fn a_local_record_is_signed_when_published_and_its_signature_is_checked() {
        let (origin, clone, head) = branch("signed", &json!({"kind": "local"}));
        let forge = Forge::default();
        assert_eq!(publish_in(&clone.0, &head, &forge), Ok(0));
        assert_eq!(forge.commits(), std::slice::from_ref(&head));
        let text = origin.note(NOTES, &head).unwrap();
        let signature = origin.note(SIGNATURES, &head).unwrap();
        let signers = Signers::of(&clone.0, &head).unwrap();
        assert!(
            signers
                .verify(text.as_bytes(), &signature, RECORD_NAMESPACE)
                .is_ok()
        );
        assert_eq!(check_in(&clone.0, &head), Ok(0));

        // Why the published record of `commit` does not authenticate, as `check` reports it.
        let refusal = |commit: &str| {
            assert_eq!(check_in(&clone.0, commit), Ok(1));
            let text = published(&clone.0, commit).unwrap();
            let signers = Signers::of(&clone.0, commit).unwrap();
            authenticate(&clone.0, &signers, commit, &text).unwrap_err()
        };
        let does_not_verify = "the signature of its record does not verify against";
        // The record altered on origin, with the signature left: refused.
        let mut altered: Value = serde_json::from_str(&text).unwrap();
        altered["finished"] = json!("2026-10-24T00:00:00Z");
        origin.publish_unsigned(&altered);
        assert!(refusal(&head).starts_with(does_not_verify));
        // Signed by a key that is not allowed: refused.
        let stranger = TestKey::new("stranger@example.org");
        origin.add_signature(
            &head,
            &stranger.sign(record::canonical(&altered).as_bytes()),
        );
        assert_eq!(
            refusal(&head),
            format!(
                "the signature of its record is by a key that is not in origin/main:{}",
                sign::ALLOWED_SIGNERS
            )
        );
        // No signature at all: refused.
        origin.git(&["notes", &format!("--ref={SIGNATURES}"), "remove", &head]);
        assert_eq!(
            refusal(&head),
            format!("its record is not signed (no note under {SIGNATURES})")
        );
        // The allowed key's signature of another commit's record, or of this record as a tag:
        // refused.
        origin.publish_unsigned(&serde_json::from_str(&text).unwrap());
        let key = origin.key();
        let main = clone.git(&["rev-parse", "origin/main"]);
        let other = record::canonical(&passing(&clone, &main, &json!({"kind": "local"})));
        origin.add_signature(&head, &key.sign(other.as_bytes()));
        assert!(refusal(&head).starts_with(does_not_verify));
        origin.add_signature(&head, &key.sign_in(text.as_bytes(), sign::TAG_NAMESPACE));
        assert!(refusal(&head).starts_with(does_not_verify));
        origin.add_signature(&head, &key.sign(text.as_bytes()));
        assert_eq!(check_in(&clone.0, &head), Ok(0));

        // Revoked: main drops the key; a record it signed no longer passes.
        origin.git(&["checkout", "-q", "main"]);
        origin.write(sign::ALLOWED_SIGNERS, "# no signers\n");
        origin.commit("revoke");
        clone.git(&["fetch", "-q", "origin"]);
        clone.git(&["merge", "-q", "--no-edit", "origin/main"]);
        clone.git(&["push", "-q", "origin", "topic"]);
        let merged = clone.git(&["rev-parse", "HEAD"]);
        origin.publish_record(&passing(&clone, &merged, &json!({"kind": "local"})));
        assert!(
            refusal(&merged).starts_with("the signature of its record is by a key that is not in")
        );
    }

    // req: VER-064
    #[test]
    fn nothing_is_published_without_a_key_of_the_allowed_signers() {
        let (origin, clone, head) = branch("unsigned", &json!({"kind": "local"}));
        let forge = Forge::default();
        let stranger = TestKey::new("stranger@example.org");
        clone.sign_with(&stranger);
        let refused = publish_in(&clone.0, &head, &forge).unwrap_err();
        assert!(refused.contains("is by a key that is not in"), "{refused}");
        clone.git(&["config", "--unset", "user.signingKey"]);
        let refused = publish_in(&clone.0, &head, &forge).unwrap_err();
        assert!(refused.contains("user.signingKey"), "{refused}");
        assert_eq!(origin.note(NOTES, &head), None);
        assert_eq!(forge.commits().len(), 0);
    }

    // req: VER-060, VER-064
    #[test]
    fn a_record_of_a_github_actions_run_counts_once_countersigned() {
        let url = "https://github.com/o/r/actions/runs/7/attempts/1";
        let source = json!({"kind": "github-actions", "run": url, "workflow": "verify"});
        let (origin, clone, head) = branch("ci", &source);
        let forge = Forge::default();
        // The run publishes its record unsigned, and sets no status.
        assert_eq!(publish_in(&clone.0, &head, &forge), Ok(0));
        assert!(origin.note(NOTES, &head).is_some());
        assert_eq!(origin.note(SIGNATURES, &head), None);
        assert_eq!(forge.commits().len(), 0);
        assert_eq!(check_in(&clone.0, &head), Ok(1));
        let text = origin.note(NOTES, &head).unwrap();
        let run = Run {
            html_url: "https://github.com/o/r/actions/runs/7".to_owned(),
            attempt: 1,
            head_sha: head.clone(),
            path: WORKFLOW.to_owned(),
            status: "completed".to_owned(),
            record: Some(text.clone()),
        };
        // A run that is not the one the record names, or whose artefact differs: not signed.
        let bad = [
            Run {
                head_sha: "f".repeat(40),
                ..run.clone()
            },
            Run {
                path: ".github/workflows/other.yml".to_owned(),
                ..run.clone()
            },
            Run {
                status: "in_progress".to_owned(),
                ..run.clone()
            },
            Run {
                attempt: 2,
                ..run.clone()
            },
            Run {
                record: Some(format!("{text} ")),
                ..run.clone()
            },
            Run {
                record: Some(text.replace("\"pass\"", "\"fail\"")),
                ..run.clone()
            },
            Run {
                record: None,
                ..run.clone()
            },
        ];
        for (i, r) in bad.into_iter().enumerate() {
            let problems = run_problems(&r, url, &head, &text);
            assert_eq!(problems.len(), usize::from(i != 4), "{i}: {problems:?}");
        }
        forge.runs.borrow_mut().push((
            url.to_owned(),
            Run {
                record: Some("{}".to_owned()),
                ..run.clone()
            },
        ));
        let refused = sign_in(&clone.0, &head, &forge).unwrap_err();
        assert!(
            refused.contains("is not the one in the run's artefact"),
            "{refused}"
        );
        assert_eq!(origin.note(SIGNATURES, &head), None);
        // The run checks out: countersigned, and the status links to the run.
        forge.runs.borrow_mut().clear();
        forge.runs.borrow_mut().push((url.to_owned(), run));
        assert_eq!(sign_in(&clone.0, &head, &forge), Ok(0));
        assert_eq!(
            *forge.statuses.borrow(),
            [(head.clone(), Some(url.to_owned()))]
        );
        assert_eq!(check_in(&clone.0, &head), Ok(0));
        // Publishing it again unsigned drops the countersignature.
        assert_eq!(publish_in(&clone.0, &head, &forge), Ok(0));
        assert_eq!(origin.note(SIGNATURES, &head), None);
        assert_eq!(check_in(&clone.0, &head), Ok(1));
    }
}
