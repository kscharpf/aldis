//! One merged pull request: the data fetched for it (`Raw`) and the metrics computed from it
//! (`Pr`, VER-063). Everything here is a pure function of the fetched JSON, so that the tests
//! need no network.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::time;

/// The line the agent's replies end with; a comment that contains it is a reply, not a review.
pub const MARKER: &str = "Generated with [Claude Code]";

/// The commit status a published passing record sets (VER-058).
const STATUS: &str = "aldis/verify";

/// The top-level areas a change is reported by; every other path is `other`.
pub const AREAS: &[&str] = &["crates", "cpp", "xtask", "docs", "test-vectors", ".github"];

/// Check names whose suffix is a target or a compiler: their group is the prefix.
const GROUPED: &[&str] = &["no-std-", "exerciser-", "cpp-build-", "cpp-bare-metal-"];

/// Everything fetched for one pull request, as the GitHub REST API and git return it.
#[derive(Debug, Default)]
pub struct Raw {
    /// `pulls/{n}`.
    pub pull: Value,
    /// `pulls/{n}/files`.
    pub files: Vec<Value>,
    /// `pulls/{n}/commits`, oldest first.
    pub commits: Vec<Value>,
    /// `pulls/{n}/reviews`.
    pub reviews: Vec<Value>,
    /// `pulls/{n}/comments`: the inline review comments and their replies.
    pub review_comments: Vec<Value>,
    /// `issues/{n}/comments`: the conversation.
    pub issue_comments: Vec<Value>,
    /// Runs of the `verify.yml` workflow; those on other branches or commits are ignored.
    pub runs: Vec<Value>,
    /// The published evidence record of each commit that has one (`refs/notes/verify`).
    pub records: BTreeMap<String, Value>,
    /// The commit statuses of each commit that has a record.
    pub statuses: BTreeMap<String, Vec<Value>>,
    /// The issues and pull requests that `#N` in the description or a reply names (`issues/{n}`).
    pub issues: BTreeMap<u64, Value>,
    /// For each merge from `main`, the paths `git merge-tree` reports as conflicted.
    pub conflicts: BTreeMap<String, Vec<String>>,
}

/// One verification run of the pull request's commits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// `local` (a published record made on a contributor's machine) or `github` (a run of
    /// `verify.yml`, VER-060).
    pub kind: &'static str,
    /// The commit verified.
    pub commit: String,
    /// Wall time: the workflow run's, or the record's `started` to `finished`.
    pub seconds: Option<i64>,
    /// `success`, `failure`, `cancelled`, ... for a workflow run; `published` for a local record.
    pub result: String,
}

/// A merge of `main` into the branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge {
    /// The merge commit.
    pub commit: String,
    /// Its first parent, on the branch.
    pub ours: String,
    /// The parent that is not a commit of the pull request: from `main`.
    pub theirs: String,
    /// The paths that conflicted (`git merge-tree`), resolved in the merge commit.
    pub conflicts: Vec<String>,
}

/// What the published record of the pull request's head says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Final {
    /// `local` or `github`.
    pub source: String,
    /// Checks that ran (passed or failed).
    pub ran: usize,
    /// Checks skipped under the input rule (VER-056).
    pub skipped: Vec<String>,
    /// Checks reused from an earlier commit of the branch (VER-056).
    pub reused: Vec<String>,
    /// Checks not run (host-bound, or `--only`).
    pub not_run: Vec<String>,
}

/// The metrics of one merged pull request (VER-063).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pr {
    /// Its number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Its head branch.
    pub branch: String,
    /// When it was opened.
    pub opened: i64,
    /// When it was merged.
    pub merged: i64,
    /// The earliest author date of its commits.
    pub first_commit: i64,
    /// Files changed.
    pub files: usize,
    /// Lines added.
    pub additions: u64,
    /// Lines removed.
    pub deletions: u64,
    /// Files changed per area of [`AREAS`] (and `other`).
    pub areas: BTreeMap<String, usize>,
    /// Its commits, merges included.
    pub commits: usize,
    /// Its verification runs, oldest first.
    pub runs: Vec<Run>,
    /// `aldis/verify` statuses set on its commits.
    pub statuses: usize,
    /// Milliseconds and number of runs per check group, over every published record of its
    /// commits, counting the checks that ran.
    pub check_ms: BTreeMap<String, (u64, usize)>,
    /// The record of its head commit, if one was published.
    pub final_record: Option<Final>,
    /// The merges of `main` into its branch.
    pub merges: Vec<Merge>,
    /// Reviews and review comments, not counting the agent's replies.
    pub reviews: usize,
    /// When the first of them arrived.
    pub first_review: Option<i64>,
    /// Commits other than merges authored after the first review: a finding was addressed.
    pub fix_commits: usize,
    /// Issues opened while the pull request was worked on and linked from its description or a
    /// reply: deferred findings.
    pub follow_ups: Vec<u64>,
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn time_of(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_str).and_then(time::parse)
}

fn login(v: &Value) -> &str {
    v.get("user").map_or("", |u| str_of(u, "login"))
}

/// The group a check is reported in: its name, less a target or compiler suffix.
pub fn group(check: &str) -> &str {
    GROUPED
        .iter()
        .find(|p| check.starts_with(*p))
        .map_or(check, |p| p.trim_end_matches('-'))
}

/// The area of a changed path.
pub fn area(path: &str) -> &'static str {
    let top = path.split('/').next().unwrap_or("");
    AREAS
        .iter()
        .find(|a| **a == top && path.len() > top.len())
        .copied()
        .unwrap_or("other")
}

/// The numbers `#N` in `text`, in order, each once.
pub fn references(text: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    for (i, _) in text.match_indices('#') {
        // `#N` at a word start: not `abc#1` (an anchor) or `##` (a heading).
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'#') {
            continue;
        }
        let digits: String = text[i + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let next = text[i + 1 + digits.len()..].chars().next();
        if digits.is_empty() || next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if let Ok(n) = digits.parse::<u64>()
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// Whether a conversation comment is a review: not a reply of the agent, and opening with a
/// Markdown heading that says so (`## Review of …`, `## PR Review: …`). Reviews posted as a
/// review of the pull request need no such heading.
pub fn is_review_comment(comment: &Value) -> bool {
    let body = str_of(comment, "body");
    let first = body.lines().next().unwrap_or("").to_ascii_lowercase();
    !body.contains(MARKER)
        && first.starts_with('#')
        && first.contains("review")
        && !first.contains("response")
}

/// The texts in which the author links follow-ups: the description, and the author's comments
/// that are not reviews.
fn author_texts(raw: &Raw) -> Vec<&str> {
    let author = login(&raw.pull);
    let mut out = vec![str_of(&raw.pull, "body")];
    out.extend(
        raw.issue_comments
            .iter()
            .filter(|c| login(c) == author && !is_review_comment(c))
            .map(|c| str_of(c, "body")),
    );
    out.extend(
        raw.review_comments
            .iter()
            .filter(|c| login(c) == author && str_of(c, "body").contains(MARKER))
            .map(|c| str_of(c, "body")),
    );
    out
}

/// The `#N` that the description and the author's replies name, other than the pull request
/// itself: the issues to fetch to tell follow-ups from other links.
pub fn candidate_references(raw: &Raw) -> BTreeSet<u64> {
    let own = raw.pull.get("number").and_then(Value::as_u64);
    author_texts(raw)
        .into_iter()
        .flat_map(references)
        .filter(|n| Some(*n) != own)
        .collect()
}

/// The merges of `main` into the branch: commits with a parent that is not a commit of the pull
/// request. Conflicts are filled in from `raw.conflicts`.
pub fn merges(raw: &Raw) -> Vec<Merge> {
    let shas: BTreeSet<&str> = raw.commits.iter().map(|c| str_of(c, "sha")).collect();
    raw.commits
        .iter()
        .filter_map(|c| {
            let parents: Vec<&str> = c
                .get("parents")
                .and_then(Value::as_array)
                .map(|p| p.iter().map(|p| str_of(p, "sha")).collect())
                .unwrap_or_default();
            let (ours, rest) = parents.split_first()?;
            let theirs = rest.iter().find(|p| !shas.contains(*p))?;
            let commit = str_of(c, "sha").to_owned();
            Some(Merge {
                conflicts: raw.conflicts.get(&commit).cloned().unwrap_or_default(),
                commit,
                ours: (*ours).to_owned(),
                theirs: (*theirs).to_owned(),
            })
        })
        .collect()
}

/// The ID of the workflow run that made a record, from its source URL
/// (`…/actions/runs/<id>/attempts/<n>`).
fn record_run_id(record: &Value) -> Option<u64> {
    let source = record.get("source")?;
    let url = source.get("run")?.as_str()?;
    let rest = url.split("/actions/runs/").nth(1)?;
    rest.split('/').next()?.parse().ok()
}

fn record_source(record: &Value) -> &'static str {
    match record.get("source").map_or("", |s| str_of(s, "kind")) {
        "github-actions" => "github",
        _ => "local",
    }
}

/// The verification runs of the pull request's commits: the `verify.yml` runs on its branch and
/// commits, the records that a run not in that list published, and the records made locally.
fn runs(raw: &Raw, shas: &BTreeSet<&str>, branch: &str) -> Vec<Run> {
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    let mut ordered: Vec<(i64, Run)> = Vec::new();
    for r in &raw.runs {
        let sha = str_of(r, "head_sha");
        if str_of(r, "head_branch") != branch || !shas.contains(sha) {
            continue;
        }
        if let Some(id) = r.get("id").and_then(Value::as_u64) {
            ids.insert(id);
        }
        let started = time_of(r, "run_started_at");
        let seconds = match (str_of(r, "status"), started, time_of(r, "updated_at")) {
            ("completed", Some(s), Some(e)) => Some(e - s),
            _ => None,
        };
        let result = r
            .get("conclusion")
            .and_then(Value::as_str)
            .unwrap_or_else(|| str_of(r, "status"));
        ordered.push((
            started.unwrap_or(0),
            Run {
                kind: "github",
                commit: sha.to_owned(),
                seconds,
                result: result.to_owned(),
            },
        ));
    }
    for (sha, record) in &raw.records {
        let kind = record_source(record);
        if kind == "github" && record_run_id(record).is_some_and(|id| ids.contains(&id)) {
            continue;
        }
        let started = time_of(record, "started");
        let seconds = started.zip(time_of(record, "finished")).map(|(s, e)| e - s);
        ordered.push((
            started.unwrap_or(0),
            Run {
                kind,
                commit: sha.clone(),
                seconds,
                result: if kind == "github" {
                    "success".to_owned()
                } else {
                    "published".to_owned()
                },
            },
        ));
    }
    ordered.sort_by_key(|(t, _)| *t);
    out.extend(ordered.into_iter().map(|(_, r)| r));
    out
}

/// The checks of a record by result.
fn final_record(record: &Value) -> Final {
    let mut f = Final {
        source: record_source(record).to_owned(),
        ..Final::default()
    };
    for c in record
        .get("checks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = str_of(c, "name").to_owned();
        match str_of(c, "result") {
            "pass" | "fail" => f.ran += 1,
            "skipped" => f.skipped.push(name),
            "reused" => f.reused.push(name),
            _ => f.not_run.push(name),
        }
    }
    f
}

/// The time a review item arrived.
fn review_times(raw: &Raw) -> Vec<i64> {
    // A `COMMENTED` review without a body only holds inline comments, counted below.
    let reviews = raw
        .reviews
        .iter()
        .filter(|r| {
            let body = str_of(r, "body");
            let container = body.trim().is_empty() && str_of(r, "state") == "COMMENTED";
            !container && !body.contains(MARKER)
        })
        .filter_map(|r| time_of(r, "submitted_at"));
    let inline = raw
        .review_comments
        .iter()
        .filter(|c| !str_of(c, "body").contains(MARKER))
        .filter_map(|c| time_of(c, "created_at"));
    let conversation = raw
        .issue_comments
        .iter()
        .filter(|c| is_review_comment(c))
        .filter_map(|c| time_of(c, "created_at"));
    reviews.chain(inline).chain(conversation).collect()
}

/// Computes the metrics of one merged pull request.
pub fn compute(raw: &Raw) -> Result<Pr, String> {
    let pull = &raw.pull;
    let number = pull
        .get("number")
        .and_then(Value::as_u64)
        .ok_or("a pull request without a number")?;
    let opened = time_of(pull, "created_at").ok_or(format!("#{number}: no created_at"))?;
    let merged = time_of(pull, "merged_at").ok_or(format!("#{number}: not merged"))?;
    let branch = pull.get("head").map_or("", |h| str_of(h, "ref")).to_owned();
    let head = pull.get("head").map_or("", |h| str_of(h, "sha"));
    let shas: BTreeSet<&str> = raw.commits.iter().map(|c| str_of(c, "sha")).collect();
    let commit_time = |c: &Value, who: &str| {
        c.get("commit")
            .and_then(|c| c.get(who))
            .and_then(|a| time_of(a, "date"))
    };
    let first_commit = raw
        .commits
        .iter()
        .filter_map(|c| commit_time(c, "author"))
        .min()
        .unwrap_or(opened);
    let mut areas: BTreeMap<String, usize> = BTreeMap::new();
    for f in &raw.files {
        *areas
            .entry(area(str_of(f, "filename")).to_owned())
            .or_default() += 1;
    }
    let mut check_ms: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    for record in raw.records.values() {
        for c in record
            .get("checks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if matches!(str_of(c, "result"), "pass" | "fail") {
                let ms = c.get("duration_ms").and_then(Value::as_u64).unwrap_or(0);
                let e = check_ms
                    .entry(group(str_of(c, "name")).to_owned())
                    .or_default();
                e.0 += ms;
                e.1 += 1;
            }
        }
    }
    let statuses = raw
        .statuses
        .values()
        .flatten()
        .filter(|s| str_of(s, "context") == STATUS)
        .count();
    let first_review = review_times(raw).into_iter().min();
    let fix_commits = first_review.map_or(0, |first| {
        raw.commits
            .iter()
            .filter(|c| c.get("parents").and_then(Value::as_array).map(Vec::len) == Some(1))
            // The author date: a rebase keeps it, so rebased commits do not count as fixes.
            .filter(|c| commit_time(c, "author").is_some_and(|t| t > first))
            .count()
    });
    let follow_ups = candidate_references(raw)
        .into_iter()
        .filter(|n| {
            raw.issues.get(n).is_some_and(|issue| {
                issue.get("pull_request").is_none()
                    && time_of(issue, "created_at")
                        .is_some_and(|t| t >= first_commit && t <= merged + 86_400)
            })
        })
        .collect();
    Ok(Pr {
        number,
        title: str_of(pull, "title").to_owned(),
        runs: runs(raw, &shas, &branch),
        branch,
        opened,
        merged,
        first_commit,
        files: usize::try_from(
            pull.get("changed_files")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
        .unwrap_or(raw.files.len()),
        additions: pull.get("additions").and_then(Value::as_u64).unwrap_or(0),
        deletions: pull.get("deletions").and_then(Value::as_u64).unwrap_or(0),
        areas,
        commits: raw.commits.len(),
        statuses,
        check_ms,
        final_record: raw.records.get(head).map(final_record),
        merges: merges(raw),
        reviews: review_times(raw).len(),
        first_review,
        fix_commits,
        follow_ups,
    })
}

impl Pr {
    /// Opened to merged, in seconds.
    pub const fn open_to_merge(&self) -> i64 {
        self.merged - self.opened
    }

    /// First commit to merged, in seconds.
    pub const fn commit_to_merge(&self) -> i64 {
        self.merged - self.first_commit
    }

    /// Total wall time of its verification runs that have one, in seconds.
    pub fn verify_seconds(&self) -> i64 {
        self.runs.iter().filter_map(|r| r.seconds).sum()
    }

    /// Its runs of `kind`.
    pub fn runs_of(&self, kind: &str) -> usize {
        self.runs.iter().filter(|r| r.kind == kind).count()
    }

    /// Whether its final record took at least one check's result from another record.
    pub fn carried(&self) -> Option<bool> {
        self.final_record
            .as_ref()
            .map(|f| !f.skipped.is_empty() || !f.reused.is_empty())
    }

    /// The JSON form, for `--format json`.
    pub fn to_json(&self) -> Value {
        let at = |t: i64| crate::verify::utc(u64::try_from(t).unwrap_or(0));
        json!({
            "number": self.number,
            "title": self.title,
            "branch": self.branch,
            "opened": at(self.opened),
            "merged": at(self.merged),
            "first_commit": at(self.first_commit),
            "cycle": {
                "open_to_merge_s": self.open_to_merge(),
                "first_commit_to_merge_s": self.commit_to_merge(),
            },
            "size": {
                "files": self.files,
                "additions": self.additions,
                "deletions": self.deletions,
                "areas": self.areas,
                "commits": self.commits,
            },
            "verify": {
                "runs": self.runs.iter().map(|r| json!({
                    "kind": r.kind, "commit": r.commit, "seconds": r.seconds, "result": r.result,
                })).collect::<Vec<_>>(),
                "local_runs": self.runs_of("local"),
                "github_runs": self.runs_of("github"),
                "seconds": self.verify_seconds(),
                "statuses": self.statuses,
                "check_groups": self.check_ms.iter().map(|(k, (ms, n))| {
                    (k.clone(), json!({"ms": ms, "runs": n}))
                }).collect::<serde_json::Map<_, _>>(),
                "final_record": self.final_record.as_ref().map(|f| json!({
                    "source": f.source, "ran": f.ran, "skipped": f.skipped,
                    "reused": f.reused, "not_run": f.not_run,
                })),
            },
            "merges_from_main": self.merges.iter().map(|m| json!({
                "commit": m.commit, "ours": m.ours, "theirs": m.theirs, "conflicts": m.conflicts,
            })).collect::<Vec<_>>(),
            "review": {
                "items": self.reviews,
                "first_review": self.first_review.map(at),
                "first_review_after_s": self.first_review.map(|t| t - self.opened),
                "fix_commits_after_first_review": self.fix_commits,
            },
            "follow_ups": self.follow_ups,
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A commit as `pulls/{n}/commits` lists it.
    pub fn commit(sha: &str, parents: &[&str], at: &str) -> Value {
        json!({
            "sha": sha,
            "parents": parents.iter().map(|p| json!({"sha": p})).collect::<Vec<_>>(),
            "commit": {"author": {"date": at}, "committer": {"date": at}},
        })
    }

    /// The fixture: PR #70, opened 11:57, reviewed 13:46, merged 15:13, with two merges from
    /// main (one conflicted), a GitHub run, a local record, and a follow-up issue.
    pub fn fixture() -> Raw {
        let local = json!({
            "source": {"kind": "local"},
            "started": "2026-10-21T12:00:00Z", "finished": "2026-10-21T12:10:00Z",
            "checks": [
                {"name": "clippy", "result": "pass", "duration_ms": 30_000},
                {"name": "exerciser-thumbv7em-none-eabihf", "result": "pass", "duration_ms": 10_000},
                {"name": "exerciser-armv7r-none-eabihf", "result": "pass", "duration_ms": 9_000},
            ],
        });
        let github = json!({
            "source": {"kind": "github-actions",
                       "run": "https://github.com/o/r/actions/runs/900/attempts/1"},
            "started": "2026-10-21T15:00:00Z", "finished": "2026-10-21T15:09:00Z",
            "checks": [
                {"name": "clippy", "result": "reused", "duration_ms": 0},
                {"name": "cpp-tidy", "result": "skipped", "duration_ms": 0},
                {"name": "test-macos", "result": "not-run", "duration_ms": 0},
                {"name": "exerciser-thumbv7em-none-eabihf", "result": "pass", "duration_ms": 12_000},
                {"name": "cpp-leon3", "result": "fail", "duration_ms": 1_000},
            ],
        });
        Raw {
            pull: json!({
                "number": 70, "title": "WP-0b", "created_at": "2026-10-21T11:57:07Z",
                "merged_at": "2026-10-21T15:13:22Z", "user": {"login": "me"},
                "head": {"ref": "m2/wp0b", "sha": "e"},
                "changed_files": 3, "additions": 120, "deletions": 20,
                "body": "Follows #64; Miri becomes #71. Not abc#9, not ##.",
            }),
            files: ["crates/aldis-testkit/src/lib.rs", "docs/x.md", "Cargo.lock"]
                .iter()
                .map(|f| json!({"filename": f}))
                .collect(),
            commits: vec![
                commit("a", &["base"], "2026-10-21T11:51:49Z"),
                commit("b", &["a", "m1"], "2026-10-21T11:54:26Z"),
                // Rebased after the review: committed then, authored before.
                json!({"sha": "c", "parents": [{"sha": "b"}], "commit": {
                    "author": {"date": "2026-10-21T11:56:40Z"},
                    "committer": {"date": "2026-10-21T14:00:00Z"},
                }}),
                commit("d", &["c", "m2"], "2026-10-21T13:49:02Z"),
                commit("e", &["d"], "2026-10-21T13:54:12Z"),
            ],
            reviews: vec![
                json!({"user": {"login": "me"}, "state": "COMMENTED",
                       "submitted_at": "2026-10-21T13:46:03Z", "body": "# Review"}),
                // The container of an inline reply: not a review of its own.
                json!({"user": {"login": "me"}, "state": "COMMENTED",
                       "submitted_at": "2026-10-21T13:40:00Z", "body": ""}),
            ],
            review_comments: vec![
                json!({"user": {"login": "me"}, "created_at": "2026-10-21T13:47:00Z",
                       "body": "Inline finding"}),
                json!({"user": {"login": "me"}, "created_at": "2026-10-21T13:50:00Z",
                       "body": format!("Fixed; deferred to #72.\n\n{MARKER}")}),
            ],
            issue_comments: vec![
                json!({"user": {"login": "me"}, "created_at": "2026-10-21T13:48:00Z",
                       "body": format!("## Response to the review\n\nSee #68, #69.\n{MARKER}")}),
            ],
            runs: vec![
                json!({"id": 900, "head_branch": "m2/wp0b", "head_sha": "e", "status": "completed",
                       "conclusion": "success", "run_started_at": "2026-10-21T14:57:36Z",
                       "updated_at": "2026-10-21T15:12:53Z"}),
                json!({"id": 899, "head_branch": "m2/wp0b", "head_sha": "d", "status": "completed",
                       "conclusion": "cancelled", "run_started_at": "2026-10-21T13:50:00Z",
                       "updated_at": "2026-10-21T13:54:00Z"}),
                // Another branch, or a commit not in the PR: not counted.
                json!({"id": 1, "head_branch": "other", "head_sha": "e", "status": "completed"}),
                json!({"id": 2, "head_branch": "m2/wp0b", "head_sha": "zz", "status": "completed"}),
            ],
            records: BTreeMap::from([("c".to_owned(), local), ("e".to_owned(), github)]),
            statuses: BTreeMap::from([(
                "e".to_owned(),
                vec![
                    json!({"context": "aldis/verify", "state": "success"}),
                    json!({"context": "other", "state": "success"}),
                ],
            )]),
            issues: BTreeMap::from([
                (
                    64,
                    json!({"created_at": "2026-10-21T10:00:00Z", "pull_request": {}}),
                ),
                (
                    68,
                    json!({"created_at": "2026-10-21T12:00:00Z", "pull_request": {}}),
                ),
                (69, json!({"created_at": "2026-10-20T12:00:00Z"})),
                (71, json!({"created_at": "2026-10-21T13:16:32Z"})),
                (72, json!({"created_at": "2026-10-21T15:20:00Z"})),
            ]),
            conflicts: BTreeMap::from([("d".to_owned(), vec!["docs/plan.md".to_owned()])]),
        }
    }

    // req: VER-063
    #[test]
    fn references_are_word_start_numbers() {
        assert_eq!(
            references("#1, (#22) and #3. Not a#4, ##5, #6x, #, or #7_; #1 again; end #8"),
            [1, 22, 3, 8]
        );
        assert_eq!(references(""), Vec::<u64>::new());
    }

    // req: VER-063
    #[test]
    fn groups_and_areas() {
        assert_eq!(group("exerciser-thumbv7em-none-eabihf"), "exerciser");
        assert_eq!(group("cpp-bare-metal-rv32imac-gcc"), "cpp-bare-metal");
        assert_eq!(group("cpp-tidy"), "cpp-tidy");
        assert_eq!(group("no-std-armv7r-none-eabihf"), "no-std");
        assert_eq!(area("crates/aldis-core/src/lib.rs"), "crates");
        assert_eq!(area(".github/workflows/nightly.yml"), ".github");
        assert_eq!(area("docs"), "other");
        assert_eq!(area("README.md"), "other");
    }

    // req: VER-063
    #[test]
    fn review_comments_are_told_from_replies() {
        let c = |body: &str| json!({"body": body});
        assert!(is_review_comment(&c("## PR Review: C0 WP-3\n\nFindings")));
        assert!(is_review_comment(&c("# Comprehensive Review of PR #66")));
        assert!(!is_review_comment(&c("## Response to the review\n")));
        assert!(!is_review_comment(&c("Thanks for the review. I rebased.")));
        assert!(!is_review_comment(&c(&format!("## Code review\n{MARKER}"))));
    }

    // req: VER-063
    #[test]
    fn computes_cycle_size_verify_merges_review_and_follow_ups() {
        let pr = compute(&fixture()).unwrap();
        assert_eq!(pr.number, 70);
        // Cycle: 11:57:07 -> 15:13:22, and the first commit 11:51:49 -> 15:13:22.
        assert_eq!(pr.open_to_merge(), 3 * 3600 + 16 * 60 + 15);
        assert_eq!(pr.commit_to_merge(), 3 * 3600 + 21 * 60 + 33);
        // Size.
        assert_eq!(
            (pr.files, pr.additions, pr.deletions, pr.commits),
            (3, 120, 20, 5)
        );
        assert_eq!(
            pr.areas,
            BTreeMap::from([
                ("crates".to_owned(), 1),
                ("docs".to_owned(), 1),
                ("other".to_owned(), 1)
            ])
        );
        // Verify: two workflow runs (the record of run 900 is not counted again) and one local
        // record, oldest first.
        let kinds: Vec<(&str, &str, Option<i64>)> = pr
            .runs
            .iter()
            .map(|r| (r.kind, r.commit.as_str(), r.seconds))
            .collect();
        assert_eq!(
            kinds,
            [
                ("local", "c", Some(600)),
                ("github", "d", Some(240)),
                ("github", "e", Some(15 * 60 + 17))
            ]
        );
        assert_eq!((pr.runs_of("local"), pr.runs_of("github")), (1, 2));
        assert_eq!(pr.verify_seconds(), 600 + 240 + 917);
        assert_eq!(pr.statuses, 1);
        assert_eq!(pr.check_ms["exerciser"], (31_000, 3));
        assert_eq!(pr.check_ms["clippy"], (30_000, 1));
        assert_eq!(pr.check_ms["cpp-leon3"], (1_000, 1));
        let f = pr.final_record.clone().unwrap();
        assert_eq!(f.source, "github");
        assert_eq!(f.ran, 2);
        assert_eq!(f.skipped, ["cpp-tidy"]);
        assert_eq!(f.reused, ["clippy"]);
        assert_eq!(f.not_run, ["test-macos"]);
        assert_eq!(pr.carried(), Some(true));
        // Merges from main: b (clean) and d (conflicted in docs/plan.md).
        assert_eq!(pr.merges.len(), 2);
        assert_eq!(
            (pr.merges[0].ours.as_str(), pr.merges[0].theirs.as_str()),
            ("a", "m1")
        );
        assert_eq!(pr.merges[0].conflicts, Vec::<String>::new());
        assert_eq!(pr.merges[1].conflicts, ["docs/plan.md"]);
        // Review: the review and the inline finding, not the agent's replies; the first at
        // 13:46:03 (not the empty review container), followed by one commit that is not a merge
        // and was authored after it (e; c was only rebased after it).
        assert_eq!(pr.reviews, 2);
        assert_eq!(pr.first_review, time::parse("2026-10-21T13:46:03Z"));
        assert_eq!(pr.fix_commits, 1);
        // Follow-ups: issues opened from the first commit to a day after the merge (#71, #72),
        // not pull requests (#64, #68) or older issues (#69).
        assert_eq!(pr.follow_ups, [71, 72]);
        assert_eq!(
            candidate_references(&fixture()),
            BTreeSet::from([64, 68, 69, 71, 72])
        );
        let j = pr.to_json();
        assert_eq!(j["cycle"]["open_to_merge_s"], 11_775);
        assert_eq!(j["verify"]["github_runs"], 2);
        assert_eq!(j["merges_from_main"][1]["conflicts"][0], "docs/plan.md");
        assert_eq!(j["review"]["first_review_after_s"], 6_536);
    }

    // req: VER-063
    #[test]
    fn an_unmerged_or_unreviewed_pull_request() {
        let mut raw = fixture();
        raw.pull["merged_at"] = Value::Null;
        assert_eq!(compute(&raw).unwrap_err(), "#70: not merged");
        let mut raw = fixture();
        raw.reviews.clear();
        raw.review_comments.clear();
        raw.records.clear();
        let pr = compute(&raw).unwrap();
        assert_eq!((pr.reviews, pr.first_review, pr.fix_commits), (0, None, 0));
        assert_eq!(pr.final_record, None);
        assert_eq!(pr.carried(), None);
    }
}
