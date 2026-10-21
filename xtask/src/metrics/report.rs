//! The aggregate over a period's pull requests and the report (VER-063): Markdown for the
//! maintainer, JSON for trend analysis.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Value, json};

use super::decisions::{self, Log};
use super::pr::{AREAS, Pr};
use super::time;

/// What a report covers.
#[derive(Debug)]
pub struct Report {
    /// `owner/repo`.
    pub repo: String,
    /// When it was made.
    pub generated: i64,
    /// The period's start (midnight UTC), or `None` for one pull request (`--pr`).
    pub since: Option<i64>,
    /// How many rows the top lists show.
    pub top: usize,
    /// The merged pull requests, oldest merge first.
    pub prs: Vec<Pr>,
    /// The maintainer's decision log, if it exists.
    pub decisions: Option<Log>,
}

/// The aggregate of a set of pull requests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Pull requests.
    pub prs: usize,
    /// Median opened to merged, in seconds.
    pub open_to_merge: Option<i64>,
    /// Median first commit to merged, in seconds.
    pub commit_to_merge: Option<i64>,
    /// Files changed, summed over the pull requests.
    pub files: usize,
    /// Lines added.
    pub additions: u64,
    /// Lines removed.
    pub deletions: u64,
    /// Commits.
    pub commits: usize,
    /// Pull requests touching each area.
    pub areas: BTreeMap<String, usize>,
    /// Verification runs on a contributor's machine (published records).
    pub local_runs: usize,
    /// Runs of `verify.yml`.
    pub github_runs: usize,
    /// Wall time of every run, in seconds.
    pub verify_seconds: i64,
    /// Median wall time of a run, in seconds.
    pub run_median: Option<i64>,
    /// Median number of runs of a pull request that had any.
    pub runs_per_pr: Option<i64>,
    /// Pull requests whose head has a published record.
    pub final_records: usize,
    /// Of those, the ones whose record skipped or reused at least one check.
    pub carried: usize,
    /// Merges of `main` into a branch.
    pub merges: usize,
    /// Pull requests with at least one.
    pub prs_with_merges: usize,
    /// Merges that had conflicts.
    pub conflicted_merges: usize,
    /// Conflicted merges per path, most first.
    pub conflict_files: Vec<(String, usize)>,
    /// Per check group: minutes in milliseconds and runs, most first.
    pub check_groups: Vec<(String, u64, usize)>,
    /// Pull requests with at least one review.
    pub reviewed: usize,
    /// Reviews and review comments.
    pub review_items: usize,
    /// Median time from opening to the first review, in seconds.
    pub first_review: Option<i64>,
    /// Reviewed pull requests with commits after the first review.
    pub fixed: usize,
    /// Follow-up issues linked.
    pub follow_ups: usize,
}

/// Aggregates the pull requests.
pub fn summary(prs: &[Pr]) -> Summary {
    let mut s = Summary {
        prs: prs.len(),
        open_to_merge: time::median(&prs.iter().map(Pr::open_to_merge).collect::<Vec<_>>()),
        commit_to_merge: time::median(&prs.iter().map(Pr::commit_to_merge).collect::<Vec<_>>()),
        ..Summary::default()
    };
    let mut run_times = Vec::new();
    let mut runs_per_pr = Vec::new();
    let mut first_reviews = Vec::new();
    let mut conflicts: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    for pr in prs {
        s.files += pr.files;
        s.additions += pr.additions;
        s.deletions += pr.deletions;
        s.commits += pr.commits;
        for area in pr.areas.keys() {
            *s.areas.entry(area.clone()).or_default() += 1;
        }
        s.local_runs += pr.runs_of("local");
        s.github_runs += pr.runs_of("github");
        s.verify_seconds += pr.verify_seconds();
        run_times.extend(pr.runs.iter().filter_map(|r| r.seconds));
        if !pr.runs.is_empty() {
            runs_per_pr.push(i64::try_from(pr.runs.len()).unwrap_or(i64::MAX));
        }
        if let Some(carried) = pr.carried() {
            s.final_records += 1;
            s.carried += usize::from(carried);
        }
        s.merges += pr.merges.len();
        s.prs_with_merges += usize::from(!pr.merges.is_empty());
        for m in pr.merges.iter().filter(|m| !m.conflicts.is_empty()) {
            s.conflicted_merges += 1;
            for path in &m.conflicts {
                *conflicts.entry(path.clone()).or_default() += 1;
            }
        }
        for (g, (ms, n)) in &pr.check_ms {
            let e = groups.entry(g.clone()).or_default();
            e.0 += ms;
            e.1 += n;
        }
        if let Some(first) = pr.first_review {
            s.reviewed += 1;
            s.review_items += pr.reviews;
            s.fixed += usize::from(pr.fix_commits > 0);
            first_reviews.push(first - pr.opened);
        }
        s.follow_ups += pr.follow_ups.len();
    }
    s.run_median = time::median(&run_times);
    s.runs_per_pr = time::median(&runs_per_pr);
    s.first_review = time::median(&first_reviews);
    s.conflict_files = conflicts.into_iter().collect();
    s.conflict_files
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    s.check_groups = groups.into_iter().map(|(g, (ms, n))| (g, ms, n)).collect();
    s.check_groups
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    s
}

/// `a` of `b` as a rounded percentage.
fn pct(a: usize, b: usize) -> String {
    (a * 100 + b / 2)
        .checked_div(b)
        .map_or_else(|| "–".to_owned(), |p| format!("{p} %"))
}

fn opt_duration(secs: Option<i64>) -> String {
    secs.map_or_else(|| "–".to_owned(), time::duration)
}

/// Milliseconds as minutes with one decimal.
fn minutes(ms: u64) -> String {
    let tenths = (ms + 3000) / 6000;
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// The report's period, in words.
fn period(r: &Report) -> String {
    match (r.since, r.prs.as_slice()) {
        (Some(since), _) => format!(
            "pull requests merged from {} to {}",
            time::date(since),
            time::date(r.generated)
        ),
        (None, [pr]) => format!("pull request #{}", pr.number),
        (None, _) => "the selected pull requests".to_owned(),
    }
}

/// `n` and the noun, plural unless `n` is 1.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The plain-language summary.
fn prose(r: &Report, s: &Summary, out: &mut String) {
    let _ = writeln!(
        out,
        "- **{}** merged, {}, {} changed (+{} / −{} lines). Half were merged within **{}** of \
         being opened, and within **{}** of their first commit (medians).",
        count(s.prs, "pull request", "pull requests"),
        count(s.commits, "commit", "commits"),
        count(s.files, "file", "files"),
        s.additions,
        s.deletions,
        opt_duration(s.open_to_merge),
        opt_duration(s.commit_to_merge),
    );
    let _ = writeln!(
        out,
        "- **Verification:** {} ({} published from a contributor's machine, {} on GitHub), \
         **{}** of wall time in all; a run took {}, and a pull request that was verified needed \
         {} (medians). {} of {} final records skipped or reused at least one check ({}).",
        count(s.local_runs + s.github_runs, "run", "runs"),
        s.local_runs,
        s.github_runs,
        time::duration(s.verify_seconds),
        opt_duration(s.run_median),
        s.runs_per_pr.map_or_else(
            || "–".to_owned(),
            |n| count(usize::try_from(n).unwrap_or(0), "run", "runs")
        ),
        s.carried,
        s.final_records,
        pct(s.carried, s.final_records),
    );
    let top_conflict = s.conflict_files.first().map_or(String::new(), |(f, n)| {
        format!(", most often in `{f}` ({n})")
    });
    let _ = writeln!(
        out,
        "- **Merges from `main`:** {} in {}; **{}** had conflicts to resolve{}.",
        s.merges,
        count(s.prs_with_merges, "pull request", "pull requests"),
        s.conflicted_merges,
        top_conflict,
    );
    let _ = writeln!(
        out,
        "- **Review:** {} of {} reviewed ({}), the first review arriving {} after opening \
         (median). Commits followed the first review in **{} of {}** ({}), a sign that a finding \
         was addressed; {} opened for deferred findings.",
        s.reviewed,
        count(s.prs, "pull request", "pull requests"),
        count(s.review_items, "review or comment", "reviews and comments"),
        opt_duration(s.first_review),
        s.fixed,
        s.reviewed,
        pct(s.fixed, s.reviewed),
        count(s.follow_ups, "follow-up issue was", "follow-up issues were"),
    );
    match (&r.decisions, r.since) {
        (None, _) => {
            let _ = writeln!(
                out,
                "- **Maintainer decisions:** `{}` does not exist yet, so none are counted.",
                decisions::PATH
            );
        }
        (Some(log), Some(since)) => {
            let period = log.entries.iter().filter(|d| d.day >= since).count();
            let _ = writeln!(
                out,
                "- **Maintainer decisions:** {period} in the period, {} logged in all (`{}`).",
                log.entries.len(),
                decisions::PATH
            );
        }
        (Some(log), None) => {
            let _ = writeln!(
                out,
                "- **Maintainer decisions:** {} logged in all (`{}`).",
                log.entries.len(),
                decisions::PATH
            );
        }
    }
}

/// The top-N tables.
fn friction(r: &Report, s: &Summary, out: &mut String) {
    let _ = writeln!(out, "\n## Where the time goes\n");
    let _ = writeln!(out, "### Check groups by verification minutes\n");
    if s.check_groups.is_empty() {
        let _ = writeln!(out, "No published record ran a check.");
    } else {
        let total: u64 = s.check_groups.iter().map(|g| g.1).sum();
        let _ = writeln!(
            out,
            "| Check group | Minutes | Share | Runs |\n|---|---:|---:|---:|"
        );
        for (g, ms, n) in s.check_groups.iter().take(r.top) {
            let share = pct(
                usize::try_from(*ms).unwrap_or(usize::MAX),
                usize::try_from(total).unwrap_or(usize::MAX),
            );
            let _ = writeln!(out, "| `{g}` | {} | {share} | {n} |", minutes(*ms));
        }
    }
    let _ = writeln!(out, "\n### Files with merge conflicts\n");
    if s.conflict_files.is_empty() {
        let _ = writeln!(out, "No merge from `main` had conflicts.");
    } else {
        let _ = writeln!(out, "| File | Conflicted merges |\n|---|---:|");
        for (f, n) in s.conflict_files.iter().take(r.top) {
            let _ = writeln!(out, "| `{f}` | {n} |");
        }
    }
    let _ = writeln!(out, "\n### Areas changed\n");
    let _ = writeln!(out, "| Area | Pull requests |\n|---|---:|");
    for area in AREAS.iter().copied().chain(["other"]) {
        let n = s.areas.get(area).copied().unwrap_or(0);
        let _ = writeln!(out, "| `{area}` | {n} |");
    }
    if let Some(log) = &r.decisions {
        let _ = writeln!(out, "\n### Maintainer decisions by category\n");
        let counts = decisions::counts(log, r.since.unwrap_or(i64::MIN));
        if counts.is_empty() {
            let _ = writeln!(out, "No entries under `## Log` yet.");
        } else if r.since.is_some() {
            let _ = writeln!(
                out,
                "| Category | In the period | In all |\n|---|---:|---:|"
            );
            for (c, (period, all)) in counts {
                let _ = writeln!(out, "| {c} | {period} | {all} |");
            }
        } else {
            // One pull request has no period.
            let _ = writeln!(out, "| Category | In all |\n|---|---:|");
            for (c, (_, all)) in counts {
                let _ = writeln!(out, "| {c} | {all} |");
            }
        }
        if !log.malformed.is_empty() {
            let _ = writeln!(
                out,
                "\n{} line(s) under `## Log` are not in the form \
                 `- YYYY-MM-DD · <category> · <ref> · <line>` and are not counted.",
                log.malformed.len()
            );
        }
    }
}

/// The per-PR table.
fn table(r: &Report, out: &mut String) {
    let _ = writeln!(out, "\n## Pull requests\n");
    let _ = writeln!(
        out,
        "| PR | Open → merged | 1st commit → merged | Files | Lines | Verify runs (local + GitHub) | \
         Verify time | Final record | Merges from main (conflicted) | Reviews | Fixes after review | \
         Follow-ups |\n|---|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---|"
    );
    for pr in &r.prs {
        let record = pr.final_record.as_ref().map_or_else(
            || "none".to_owned(),
            |f| {
                format!(
                    "{} ran, {} skipped, {} reused",
                    f.ran,
                    f.skipped.len(),
                    f.reused.len()
                )
            },
        );
        let conflicted = pr.merges.iter().filter(|m| !m.conflicts.is_empty()).count();
        let follow_ups: Vec<String> = pr.follow_ups.iter().map(|n| format!("#{n}")).collect();
        let _ = writeln!(
            out,
            "| #{} `{}` | {} | {} | {} | +{} −{} | {} + {} | {} | {} | {} ({}) | {} | {} | {} |",
            pr.number,
            pr.branch,
            time::duration(pr.open_to_merge()),
            time::duration(pr.commit_to_merge()),
            pr.files,
            pr.additions,
            pr.deletions,
            pr.runs_of("local"),
            pr.runs_of("github"),
            time::duration(pr.verify_seconds()),
            record,
            pr.merges.len(),
            conflicted,
            pr.reviews,
            if pr.first_review.is_some() {
                pr.fix_commits.to_string()
            } else {
                "–".to_owned()
            },
            if follow_ups.is_empty() {
                "–".to_owned()
            } else {
                follow_ups.join(" ")
            },
        );
    }
}

/// How each number is made, and what is not measured.
const METHOD: &str = "\
## How these are measured

- **Cycle:** the pull request's `created_at` and `merged_at`; the first commit is the earliest \
author date of its commits.
- **Verify runs:** the `verify.yml` runs on the branch for one of its commits (wall time from \
start to last update), and the records in `refs/notes/verify` made locally (wall time from the \
record's `started` to `finished`, which leaves out building the image). A local run that failed or \
was never published leaves no trace and is not counted. Check minutes are summed over the checks \
that ran in each published record of the pull request's commits.
- **Final record:** the published record of the head commit; `skipped` and `reused` checks took \
their result from another record (VER-056).
- **Merges from `main`:** commits of the pull request with a parent outside it. Each is redone \
with `git merge-tree --write-tree` to find the paths that conflicted. A rebase onto `main` leaves \
no merge and is not counted.
- **Review:** reviews and review comments, and conversation comments whose first line is a \
heading naming a review; anything that contains `Generated with [Claude Code]` is a reply and \
not counted. A fix is a commit other than a merge authored after the first review.
- **Follow-ups:** issues (not pull requests) linked as `#N` from the description or the author's \
replies and opened between the first commit and a day after the merge.
- Generated by `cargo xtask metrics` (docs/06-verification.md §9, VER-063).
";

/// The Markdown report.
pub fn markdown(r: &Report) -> String {
    let s = summary(&r.prs);
    let mut out = String::new();
    let _ = writeln!(out, "# Process metrics\n");
    let _ = writeln!(
        out,
        "Covers {} in `{}`. Generated {}.\n",
        period(r),
        r.repo,
        crate::verify::utc(u64::try_from(r.generated).unwrap_or(0))
    );
    let _ = writeln!(out, "## Summary\n");
    prose(r, &s, &mut out);
    friction(r, &s, &mut out);
    table(r, &mut out);
    let _ = write!(out, "\n{METHOD}");
    out
}

/// The JSON report: the summary and the per-PR data, for trend analysis.
pub fn json(r: &Report) -> Value {
    let s = summary(&r.prs);
    let at = |t: i64| crate::verify::utc(u64::try_from(t).unwrap_or(0));
    json!({
        "format": "aldis-metrics/1",
        "repo": r.repo,
        "generated": at(r.generated),
        "since": r.since.map(at),
        "summary": {
            "prs": s.prs,
            "open_to_merge_median_s": s.open_to_merge,
            "first_commit_to_merge_median_s": s.commit_to_merge,
            "files": s.files,
            "additions": s.additions,
            "deletions": s.deletions,
            "commits": s.commits,
            "areas": s.areas,
            "local_runs": s.local_runs,
            "github_runs": s.github_runs,
            "verify_seconds": s.verify_seconds,
            "run_median_s": s.run_median,
            "runs_per_pr_median": s.runs_per_pr,
            "final_records": s.final_records,
            "final_records_carried": s.carried,
            "merges_from_main": s.merges,
            "prs_with_merges": s.prs_with_merges,
            "conflicted_merges": s.conflicted_merges,
            "conflict_files": s.conflict_files.iter()
                .map(|(f, n)| json!({"path": f, "merges": n})).collect::<Vec<_>>(),
            "check_groups": s.check_groups.iter()
                .map(|(g, ms, n)| json!({"group": g, "ms": ms, "runs": n})).collect::<Vec<_>>(),
            "reviewed": s.reviewed,
            "review_items": s.review_items,
            "first_review_median_s": s.first_review,
            "fixed_after_review": s.fixed,
            "follow_ups": s.follow_ups,
        },
        "decisions": decisions::to_json(r.decisions.as_ref(), r.since.unwrap_or(i64::MIN)),
        "prs": r.prs.iter().map(Pr::to_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::pr::{self, compute};

    fn two() -> Vec<Pr> {
        let first = compute(&pr::tests::fixture()).unwrap();
        // A second pull request: unreviewed, no merges, no record, one local run.
        let mut raw = pr::tests::fixture();
        raw.pull["number"] = json!(71);
        raw.pull["head"] = json!({"ref": "chore/x", "sha": "zz"});
        raw.pull["created_at"] = json!("2026-10-21T14:00:00Z");
        raw.pull["merged_at"] = json!("2026-10-21T14:30:00Z");
        raw.commits = vec![pr::tests::commit("q", &["base"], "2026-10-21T13:00:00Z")];
        raw.reviews.clear();
        raw.review_comments.clear();
        raw.issue_comments.clear();
        raw.records.retain(|k, _| k == "c");
        raw.pull["body"] = json!("");
        let second = compute(&raw).unwrap();
        vec![first, second]
    }

    // req: VER-063
    #[test]
    fn aggregates_medians_totals_and_top_lists() {
        let s = summary(&two());
        assert_eq!(s.prs, 2);
        // 11_775 s and 1_800 s: the lower middle one.
        assert_eq!(s.open_to_merge, Some(1_800));
        assert_eq!(s.commit_to_merge, Some(5_400));
        assert_eq!((s.files, s.additions, s.deletions), (6, 240, 40));
        assert_eq!(s.areas["crates"], 2);
        // Runs: 1 local + 2 GitHub for #70; 1 local for #71 (the record of c).
        assert_eq!((s.local_runs, s.github_runs), (2, 2));
        assert_eq!(s.verify_seconds, 1_757 + 600);
        assert_eq!(s.run_median, Some(600));
        assert_eq!(s.runs_per_pr, Some(1));
        assert_eq!((s.final_records, s.carried), (1, 1));
        assert_eq!(
            (s.merges, s.prs_with_merges, s.conflicted_merges),
            (2, 1, 1)
        );
        assert_eq!(s.conflict_files, [("docs/plan.md".to_owned(), 1)]);
        assert_eq!(s.check_groups[0], ("clippy".to_owned(), 60_000, 2));
        assert_eq!(s.check_groups[1], ("exerciser".to_owned(), 50_000, 5));
        assert_eq!((s.reviewed, s.review_items, s.fixed), (1, 2, 1));
        assert_eq!(s.first_review, Some(6_536));
        assert_eq!(s.follow_ups, 2);
        assert_eq!(summary(&[]), Summary::default());
    }

    // req: VER-063
    #[test]
    fn renders_markdown_and_json() {
        let report = Report {
            repo: "o/r".to_owned(),
            generated: time::parse("2026-10-21T16:00:00Z").unwrap(),
            since: time::parse("2026-10-16"),
            top: 1,
            prs: two(),
            decisions: None,
        };
        let md = markdown(&report);
        for want in [
            "# Process metrics\n",
            "Covers pull requests merged from 2026-10-16 to 2026-10-21 in `o/r`.",
            "- **2 pull requests** merged, 6 commits, 6 files changed (+240 / −40 lines).",
            "within **30 min** of being opened",
            "4 runs (2 published from a contributor's machine, 2 on GitHub)",
            "1 of 1 final records skipped or reused at least one check (100 %)",
            "- **Merges from `main`:** 2 in 1 pull request; **1** had conflicts to resolve, most \
             often in `docs/plan.md` (1).",
            "1 of 2 pull requests reviewed (2 reviews and comments)",
            "Commits followed the first review in **1 of 1** (100 %)",
            "2 follow-up issues were opened",
            "a pull request that was verified needed 1 run (medians)",
            "`docs/process/decisions.md` does not exist yet",
            "| `clippy` | 1.0 | 54 % | 2 |",
            "| `docs/plan.md` | 1 |",
            "| #70 `m2/wp0b` | 3 h 16 min | 3 h 21 min | 3 | +120 −20 | 1 + 2 | 29 min | \
             2 ran, 1 skipped, 1 reused | 2 (1) | 2 | 1 | #71 #72 |",
            "| #71 `chore/x` | 30 min | 1 h 30 min | 3 | +120 −20 | 1 + 0 | 10 min | none | \
             0 (0) | 0 | – | – |",
            "## How these are measured",
        ] {
            assert!(md.contains(want), "missing {want:?} in\n{md}");
        }
        // --top 1 shows one check group.
        assert!(!md.contains("| `exerciser` |"));

        let with_log = Report {
            decisions: Some(decisions::parse(
                "## Log\n\n- 2026-10-20 · scope · #56 · x\n- 2026-10-01 · scope · chat · y\n- bad\n",
            )),
            ..report
        };
        let md = markdown(&with_log);
        assert!(md.contains("**Maintainer decisions:** 1 in the period, 2 logged in all"));
        assert!(md.contains("| scope | 1 | 2 |"));
        assert!(md.contains("1 line(s) under `## Log` are not in the form"));
        // One pull request has no period.
        let one = Report {
            since: None,
            prs: two().into_iter().take(1).collect(),
            ..with_log
        };
        let md = markdown(&one);
        assert!(md.contains("Covers pull request #70 in `o/r`."), "{md}");
        assert!(md.contains("- **1 pull request** merged, 5 commits, 3 files changed"));
        assert!(md.contains("**Maintainer decisions:** 2 logged in all"));
        assert!(md.contains("| Category | In all |\n|---|---:|\n| scope | 2 |"));
        let with_log = Report {
            since: time::parse("2026-10-16"),
            prs: two(),
            ..one
        };
        let j = json(&with_log);
        assert_eq!(j["summary"]["prs"], 2);
        assert_eq!(j["summary"]["conflict_files"][0]["path"], "docs/plan.md");
        assert_eq!(j["decisions"]["categories"]["scope"]["period"], 1);
        assert_eq!(j["prs"][1]["number"], 71);
        assert_eq!(j["since"], "2026-10-16T00:00:00Z");
    }
}
