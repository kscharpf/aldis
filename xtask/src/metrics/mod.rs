//! `cargo xtask metrics`: how the project's pull requests went (docs/06-verification.md §9,
//! VER-063). Cycle time, size, verification runs and their cost, merges from `main` and their
//! conflicts, review outcomes, follow-ups, and the maintainer's decision log.
//!
//! It reads GitHub through `gh api` (a tooling dependency, RS-017, as `verify` uses it) and the
//! local repository: the published records under `refs/notes/verify`, fetched as `verify check`
//! fetches them, and the merge commits, redone with `git merge-tree`. It needs the network, so it
//! is never part of `cargo xtask ci`. The computation (`pr.rs`, `report.rs`, `decisions.rs`) is
//! pure and tested on fixtures.

mod conflicts;
mod decisions;
mod pr;
mod report;
mod stages;
pub(crate) mod time;
mod waits;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::{repo, verify};

pub const HELP: &str = "\
Process metrics (docs/06-verification.md §9, VER-063; needs `gh` and the network, never in ci):
  metrics [--since <YYYY-MM-DD> | --pr <n>] [--format md|json] [--top <n>] [--json <file>]
          [--no-previous]      cycle time, size, verification runs and minutes, merges from main
                               and their conflicts, reviews and fixes, and follow-ups of the pull
                               requests merged since the date (default: the last 30 days), or of
                               one; maintainer decisions from docs/process/decisions.md; the
                               stages each waited in, raw and in the maintainer's active time;
                               and the process-created waits (order inversions, verification
                               after approval, failed and cancelled runs, rework after a
                               decision, repeated conflicts) against thresholds, beside the
                               previous period of the same length (--no-previous: not fetched).
                               Prints a Markdown report (default) or the per-PR data as JSON;
                               --json also writes the JSON to <file>; --top sets the length of
                               the top lists (default 10)
";

/// What to report on and how.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    since: Option<i64>,
    pr: Option<u64>,
    json: bool,
    top: usize,
    json_out: Option<String>,
    previous: bool,
}

fn parse_options(args: &[&str], now: i64) -> Result<Options, String> {
    let mut opts = Options {
        since: None,
        pr: None,
        json: false,
        top: 10,
        json_out: None,
        previous: true,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .copied()
                .ok_or_else(|| format!("{arg} needs a value\n\n{HELP}"))
        };
        match *arg {
            "--since" => {
                let v = value()?;
                opts.since = Some(
                    time::parse(v)
                        .filter(|_| v.len() == 10)
                        .ok_or_else(|| format!("--since: expected YYYY-MM-DD, found `{v}`"))?,
                );
            }
            "--pr" => {
                let v = value()?;
                opts.pr = Some(
                    v.trim_start_matches('#')
                        .parse()
                        .map_err(|_| format!("--pr: expected a number, found `{v}`"))?,
                );
            }
            "--format" => {
                opts.json = match value()? {
                    "md" => false,
                    "json" => true,
                    other => return Err(format!("--format: expected md or json, found `{other}`")),
                };
            }
            "--top" => {
                let v = value()?;
                opts.top = v
                    .parse()
                    .map_err(|_| format!("--top: expected a number, found `{v}`"))?;
            }
            "--json" => opts.json_out = Some(value()?.to_owned()),
            "--no-previous" => opts.previous = false,
            other => return Err(format!("unknown argument `{other}`\n\n{HELP}")),
        }
    }
    if opts.since.is_some() && opts.pr.is_some() {
        return Err("--since and --pr exclude each other".to_owned());
    }
    if opts.pr.is_none() && opts.since.is_none() {
        // Midnight UTC, 30 days ago.
        opts.since = Some((now - 30 * 86_400).div_euclid(86_400) * 86_400);
    }
    Ok(opts)
}

/// Explains what `metrics` needs when `gh` cannot be run or fails.
const NEEDS: &str = "`cargo xtask metrics` reads GitHub through the GitHub CLI: it needs `gh` \
     (https://cli.github.com) on the PATH, authenticated with `gh auth login` or GH_TOKEN, and \
     network access. It is not part of `cargo xtask ci`, which stays offline.";

/// Runs `gh api --paginate` on `path` with `fields` (`-f k=v`, sent as a GET query); returns
/// every value it printed, one per page.
fn gh_pages(path: &str, fields: &[(&str, &str)]) -> Result<Vec<Value>, String> {
    let mut cmd = Command::new("gh");
    cmd.args(["api", "--paginate", "-X", "GET", path]);
    for (k, v) in fields {
        cmd.args(["-f", &format!("{k}={v}")]);
    }
    let out = cmd
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("gh: {e}\n{NEEDS}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh api {path}: {}\n{NEEDS}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::Deserializer::from_str(&text)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("gh api {path}: {e}"))
}

/// The items of a paginated list: arrays, and the `items` or `workflow_runs` of search and
/// workflow pages, concatenated.
fn gh_list(path: &str, fields: &[(&str, &str)]) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for page in gh_pages(path, fields)? {
        match page {
            Value::Array(items) => out.extend(items),
            Value::Object(mut obj) => {
                for key in ["items", "workflow_runs"] {
                    if let Some(Value::Array(items)) = obj.remove(key) {
                        out.extend(items);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// One object.
fn gh_one(path: &str) -> Result<Value, String> {
    gh_pages(path, &[])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("gh api {path}: no output"))
}

/// Runs git in `dir`; true if it succeeded.
fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The GitHub data of one pull request.
fn fetch_pr(repo: &str, number: u64) -> Result<pr::Raw, String> {
    let base = format!("repos/{repo}");
    let list = |what: &str| gh_list(&format!("{base}/{what}?per_page=100"), &[]);
    Ok(pr::Raw {
        pull: gh_one(&format!("{base}/pulls/{number}"))?,
        files: list(&format!("pulls/{number}/files"))?,
        commits: list(&format!("pulls/{number}/commits"))?,
        reviews: list(&format!("pulls/{number}/reviews"))?,
        review_comments: list(&format!("pulls/{number}/comments"))?,
        issue_comments: list(&format!("issues/{number}/comments"))?,
        ..pr::Raw::default()
    })
}

/// Fetches the pull requests in parallel, a few at a time, keeping their order.
fn fetch_prs(repo: &str, numbers: &[u64]) -> Result<Vec<pr::Raw>, String> {
    const WORKERS: usize = 4;
    let chunks: Vec<Vec<(usize, u64)>> = (0..WORKERS)
        .map(|w| {
            numbers
                .iter()
                .copied()
                .enumerate()
                .filter(|(i, _)| i % WORKERS == w)
                .collect()
        })
        .collect();
    let results: Vec<Result<Vec<(usize, pr::Raw)>, String>> = std::thread::scope(|s| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|(i, n)| {
                            eprintln!("metrics: fetching #{n}");
                            fetch_pr(repo, *n).map(|raw| (*i, raw))
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("a fetch thread panicked".to_owned()))
            })
            .collect()
    });
    let mut all = Vec::new();
    for r in results {
        all.extend(r?);
    }
    all.sort_by_key(|(i, _)| *i);
    Ok(all.into_iter().map(|(_, raw)| raw).collect())
}

/// Adds the local data: published records, their commit statuses, the issues the replies link
/// to, and the conflicts of each merge from `main`.
fn complete(
    repo: &str,
    maintainer: &str,
    raw: &mut pr::Raw,
    runs: &[Value],
    notes: bool,
    issues: &mut BTreeMap<u64, Value>,
) -> Result<(), String> {
    let root = repo::root();
    maintainer.clone_into(&mut raw.maintainer);
    let number = raw.pull.get("number").and_then(Value::as_u64).unwrap_or(0);
    let branch = raw
        .pull
        .get("head")
        .and_then(|h| h.get("ref"))
        .and_then(Value::as_str)
        .unwrap_or("");
    raw.runs = runs
        .iter()
        .filter(|r| r.get("head_branch").and_then(Value::as_str) == Some(branch))
        .cloned()
        .collect();
    let shas: Vec<String> = raw
        .commits
        .iter()
        .filter_map(|c| c.get("sha").and_then(Value::as_str).map(str::to_owned))
        .collect();
    if notes {
        for sha in &shas {
            let Some(text) = verify::published(&root, sha) else {
                continue;
            };
            let Ok(record) = serde_json::from_str::<Value>(text.trim()) else {
                eprintln!("metrics: #{number}: the record of {sha} is not JSON; ignored");
                continue;
            };
            raw.statuses.insert(
                sha.clone(),
                gh_list(
                    &format!("repos/{repo}/commits/{sha}/statuses?per_page=100"),
                    &[],
                )?,
            );
            raw.records.insert(sha.clone(), record);
        }
    }
    for n in pr::candidate_references(raw) {
        if let std::collections::btree_map::Entry::Vacant(e) = issues.entry(n) {
            // A number that is not an issue or pull request of the repository is skipped.
            if let Ok(issue) = gh_one(&format!("repos/{repo}/issues/{n}")) {
                e.insert(issue);
            }
        }
        if let Some(issue) = issues.get(&n) {
            raw.issues.insert(n, issue.clone());
        }
    }
    let merges = pr::merges(raw);
    let present = |sha: &str| git_ok(&root, &["cat-file", "-e", &format!("{sha}^{{commit}}")]);
    let head = raw
        .pull
        .get("head")
        .and_then(|h| h.get("sha"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if !present(&head)
        || merges
            .iter()
            .any(|m| !present(&m.ours) || !present(&m.theirs))
    {
        // Commits that main no longer reaches (a squash, or a force-push): the pull request's
        // head still has them.
        let _ = git_ok(
            &root,
            &["fetch", "-q", "origin", &format!("pull/{number}/head")],
        );
    }
    for m in merges {
        match conflicts::paths(&root, &m.ours, &m.theirs) {
            Ok(paths) => {
                raw.conflicts.insert(m.commit, paths);
            }
            Err(e) => eprintln!("metrics: #{number}: merge {}: {e}", m.commit),
        }
    }
    raw.decision_commits = decision_commits(&root, number, &head)
        .into_iter()
        .filter(|c| shas.contains(c))
        .collect();
    for r in &raw.runs {
        let (Some(id), Some(sha)) = (
            r.get("id").and_then(Value::as_u64),
            r.get("head_sha").and_then(Value::as_str),
        ) else {
            continue;
        };
        let failed = r
            .get("conclusion")
            .and_then(Value::as_str)
            .is_some_and(|c| pr::FAILED.contains(&c));
        if failed
            && shas.iter().any(|s| s.as_str() == sha)
            && let Some(record) = run_record(repo, id, sha)
        {
            raw.run_records.insert(id, record);
        }
    }
    Ok(())
}

/// The commits reachable from `head` that added or removed a decision-log line naming `#number`
/// (`git log -S`, which follows no merge).
fn decision_commits(root: &Path, number: u64, head: &str) -> Vec<String> {
    let out = Command::new("git")
        .args([
            "log",
            "--format=%H",
            &format!("-S · #{number} · "),
            head,
            "--",
            decisions::PATH,
        ])
        .current_dir(root)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// The record that the failed `verify.yml` run `id` of `sha` left in its artefact
/// `verify-<sha>` (VER-060), downloaded once into `target/metrics-runs/<id>/` (outside
/// `target/metrics/`, which the nightly job keeps as its artefact) with `gh run
/// download`; `None` if the artefact has expired or holds no record.
fn run_record(repo: &str, id: u64, sha: &str) -> Option<Value> {
    let dir = repo::root()
        .join("target/metrics-runs")
        .join(id.to_string());
    let path = dir.join(sha).join("record.json");
    if !path.is_file() {
        let ok = Command::new("gh")
            .args(["run", "download", &id.to_string(), "-R", repo])
            .args(["-n", &format!("verify-{sha}"), "-D"])
            .arg(&dir)
            .current_dir(repo::root())
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            eprintln!("metrics: run {id}: no artefact verify-{sha}; its checks are not known");
            return None;
        }
    }
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The pull requests merged in `range` (a search qualifier: `>=2026-10-01`,
/// `2026-09-01..2026-09-30`).
fn merged_in(repo: &str, range: &str) -> Result<Vec<u64>, String> {
    let found = gh_list(
        "search/issues",
        &[
            ("q", &format!("repo:{repo} is:pr is:merged merged:{range}")),
            ("per_page", "100"),
        ],
    )?;
    Ok(found
        .iter()
        .filter_map(|i| i.get("number").and_then(Value::as_u64))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

/// The start of the period before the one from `since` to `now`: as many whole days, ending the
/// day before `since`.
const fn previous_start(since: i64, now: i64) -> i64 {
    let days = now.div_euclid(86_400) * 86_400 - since;
    since - if days < 86_400 { 86_400 } else { days }
}

/// The times of the maintainer's comments on every issue and pull request since `since`, for
/// their active time.
fn maintainer_comments(repo: &str, maintainer: &str, since: i64) -> Result<Vec<i64>, String> {
    let comments = gh_list(
        &format!("repos/{repo}/issues/comments"),
        &[
            ("per_page", "100"),
            (
                "since",
                &crate::verify::utc(u64::try_from(since).unwrap_or(0)),
            ),
        ],
    )?;
    Ok(comments
        .iter()
        .filter(|c| stages::is_maintainer_comment(c, maintainer))
        .filter_map(|c| c.get("created_at")?.as_str().and_then(time::parse))
        .collect())
}

/// Dispatches `cargo xtask metrics ...`.
pub fn run(args: &[&str]) -> Result<usize, String> {
    let now = time::now();
    let opts = parse_options(args, now)?;
    let root = repo::root();
    let info = gh_one("repos/{owner}/{repo}")?;
    let field = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_owned();
    let repo = field(info.get("full_name"));
    if repo.is_empty() {
        return Err("gh api repos/{owner}/{repo}: no full_name".to_owned());
    }
    let maintainer = field(info.get("owner").and_then(|o| o.get("login")));
    let numbers: Vec<u64> = if let Some(n) = opts.pr {
        vec![n]
    } else {
        merged_in(&repo, &format!(">={}", time::date(opts.since.unwrap_or(0))))?
    };
    let previous_since = opts
        .since
        .filter(|_| opts.previous)
        .map(|since| previous_start(since, now));
    let previous_numbers: Vec<u64> = match (previous_since, opts.since) {
        (Some(from), Some(since)) => merged_in(
            &repo,
            &format!("{}..{}", time::date(from), time::date(since - 86_400)),
        )?
        .into_iter()
        .filter(|n| !numbers.contains(n))
        .collect(),
        _ => Vec::new(),
    };
    let mut raws = fetch_prs(&repo, &[numbers.clone(), previous_numbers.clone()].concat())?;
    if raws
        .iter()
        .any(|r| r.pull.get("merged_at").is_none_or(Value::is_null))
    {
        return Err(format!(
            "#{}: not merged; metrics covers merged pull requests",
            numbers.first().copied().unwrap_or(0)
        ));
    }
    // The verify.yml runs from the earliest first commit on.
    let earliest = raws
        .iter()
        .flat_map(|r| &r.commits)
        .filter_map(|c| {
            c.get("commit")?
                .get("author")?
                .get("date")?
                .as_str()
                .and_then(time::parse)
        })
        .min()
        .unwrap_or(now);
    let runs = gh_list(
        &format!("repos/{repo}/actions/workflows/verify.yml/runs"),
        &[
            ("per_page", "100"),
            ("created", &format!(">={}", time::date(earliest - 86_400))),
        ],
    )?;
    let notes = verify::fetch_notes(&root)?;
    if !notes {
        eprintln!("metrics: origin has no refs/notes/verify; no records counted");
    }
    let _ = git_ok(&root, &["fetch", "-q", "origin", "main"]);
    let mut issues = BTreeMap::new();
    let (mut prs, mut previous) = (Vec::new(), Vec::new());
    for raw in &mut raws {
        complete(&repo, &maintainer, raw, &runs, notes, &mut issues)?;
        let pr = pr::compute(raw)?;
        if previous_numbers.contains(&pr.number) {
            previous.push(pr);
        } else {
            prs.push(pr);
        }
    }
    prs.sort_by_key(|p| (p.merged, p.number));
    previous.sort_by_key(|p| (p.merged, p.number));
    let events = maintainer_comments(&repo, &maintainer, earliest - 86_400)?;
    let log = repo::read(&root.join(decisions::PATH))
        .ok()
        .map(|t| decisions::parse(&t));
    let report = report::Report {
        maintainer,
        events,
        previous: previous_since.map(|since| report::Previous {
            since,
            prs: previous,
        }),
        repo,
        generated: now,
        since: opts.since,
        top: opts.top,
        prs,
        decisions: log,
    };
    output(&opts, &report)?;
    Ok(0)
}

/// Prints the report, and writes its JSON where `--json` says.
fn output(opts: &Options, report: &report::Report) -> Result<(), String> {
    let json = report::json(report);
    if let Some(path) = &opts.json_out {
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())? + "\n";
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", report::markdown(report));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-063
    #[test]
    fn options() {
        let now = time::parse("2026-10-21T15:30:00Z").unwrap();
        let o = parse_options(&[], now).unwrap();
        assert_eq!(o.since, time::parse("2026-09-21"));
        assert_eq!((o.pr, o.json, o.top), (None, false, 10));
        let o = parse_options(
            &[
                "--since",
                "2026-10-16",
                "--format",
                "json",
                "--top",
                "3",
                "--json",
                "x",
            ],
            now,
        )
        .unwrap();
        assert_eq!(o.since, time::parse("2026-10-16"));
        assert_eq!((o.json, o.top, o.json_out.as_deref()), (true, 3, Some("x")));
        let o = parse_options(&["--pr", "#70"], now).unwrap();
        assert_eq!((o.pr, o.since, o.previous), (Some(70), None, true));
        assert!(!parse_options(&["--no-previous"], now).unwrap().previous);
        // 30 whole days from 2026-09-21 to today: the previous period starts 2026-08-22.
        let since = time::parse("2026-09-21").unwrap();
        assert_eq!(Some(previous_start(since, now)), time::parse("2026-08-22"));
        // A period that starts today: the day before.
        let today = time::parse("2026-10-21").unwrap();
        assert_eq!(Some(previous_start(today, now)), time::parse("2026-10-20"));
        for bad in [
            &["--since", "2026-10-16T00:00:00Z"][..],
            &["--since"],
            &["--format", "html"],
            &["--pr", "x"],
            &["--since", "2026-10-16", "--pr", "1"],
            &["--bogus"],
        ] {
            assert!(parse_options(bad, now).is_err(), "{bad:?}");
        }
    }
}
