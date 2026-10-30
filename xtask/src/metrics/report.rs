//! The aggregate over a period's pull requests and the report (VER-063): Markdown for the
//! maintainer, JSON for trend analysis.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Value, json};

use super::overhead::{self, Class};
use super::pr::{AREAS, Pr};
use super::stages::{self, ACTIVE_WINDOW, Activity, ApprovalKind};
use super::time;
use super::waits::{self, Totals, short};

/// The period before the report's, of the same length, for comparison.
#[derive(Debug)]
pub struct Previous {
    /// Its start (midnight UTC); it ends where the report's period starts.
    pub since: i64,
    /// The pull requests merged in it.
    pub prs: Vec<Pr>,
}

/// What a report covers.
#[derive(Debug)]
pub struct Report {
    /// The maintainer's login.
    pub maintainer: String,
    /// The maintainer's events outside the pull requests' own: their comments on any issue or pull
    /// request of the repository.
    pub events: Vec<i64>,
    /// The previous period, if one was fetched.
    pub previous: Option<Previous>,
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
fn prose(s: &Summary, out: &mut String) {
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
}

/// The split between product and process of the pull requests, their commits, and their
/// verification minutes, and the bookkeeping-only commits.
fn overhead_section(r: &Report, t: &Totals, out: &mut String) {
    let o = &t.overhead;
    let _ = writeln!(out, "\n## Process overhead\n");
    let _ = writeln!(
        out,
        "Pull requests, their commits other than merges, and their verification minutes by \
         what they changed: product, process, or both (see \"How these are measured\"). A \
         pull request's verification minutes count under its class.\n"
    );
    let _ = writeln!(
        out,
        "| | Process-only | Product-only | Mixed |\n|---|---:|---:|---:|"
    );
    let counts = |m: &std::collections::BTreeMap<Class, usize>| {
        let wide = overhead::wide(m);
        Class::ALL
            .iter()
            .map(|c| {
                format!(
                    "{} ({})",
                    m.get(c).copied().unwrap_or(0),
                    overhead::share(&wide, *c)
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let minutes = Class::ALL
        .iter()
        .map(|c| {
            format!(
                "{} ({})",
                time::duration(o.verify_s.get(c).copied().unwrap_or(0)),
                overhead::share(&o.verify_s, *c)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let _ = writeln!(out, "| Pull requests | {} |", counts(&o.prs));
    let _ = writeln!(out, "| Commits | {} |", counts(&o.commits));
    let _ = writeln!(out, "| Verification minutes | {minutes} |");
    if o.unknown > 0 {
        let _ = writeln!(
            out,
            "\nNot counted: {} whose changed paths git could not read.",
            count(o.unknown, "commit", "commits")
        );
    }
    let _ = writeln!(out, "\n### Bookkeeping-only commits\n");
    if o.bookkeeping.is_empty() {
        let _ = writeln!(out, "No commit changed only bookkeeping.");
        return;
    }
    let _ = writeln!(
        out,
        "Commits that change only the work-package status rows of a milestone plan, or \
         `docs/verification/records/`: {} in all.\n",
        o.bookkeeping.iter().map(|(_, k)| k).sum::<usize>()
    );
    let _ = writeln!(out, "| PR | Bookkeeping-only | Commits |\n|---|---:|---|");
    for pr in r.prs.iter().filter(|p| p.overhead.bookkeeping() > 0) {
        let shas: Vec<String> = overhead::bookkeeping_shas(pr)
            .into_iter()
            .map(|s| format!("`{s}`"))
            .collect();
        let _ = writeln!(
            out,
            "| #{} | {} | {} |",
            pr.number,
            pr.overhead.bookkeeping(),
            shas.join(" ")
        );
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

impl Report {
    /// Every pull request fetched: the period's and the previous period's.
    fn all_prs(&self) -> impl Iterator<Item = &Pr> {
        self.prs
            .iter()
            .chain(self.previous.iter().flat_map(|p| &p.prs))
    }

    /// The maintainer's active time, from their events on every pull request fetched and their
    /// comments on the repository.
    pub fn activity(&self) -> Activity {
        let mut events = self.events.clone();
        for pr in self.all_prs() {
            events.extend(&pr.events);
        }
        Activity::new(&events, ACTIVE_WINDOW)
    }

    /// The process-created waits of the period, and of the previous one if it was fetched. Order
    /// inversions are found among every pull request fetched, so that one approved in the period
    /// can wait on one merged before it.
    pub fn totals(&self) -> (Totals, Option<Totals>) {
        let all: Vec<&Pr> = self.all_prs().collect();
        let found = waits::inversions(&all);
        let of = |prs: &[Pr]| waits::totals(prs, &found, &summary(prs).conflict_files);
        (of(&self.prs), self.previous.as_ref().map(|p| of(&p.prs)))
    }
}

/// Per named interval of [`stages::INTERVALS`]: its label, the pull requests that passed both of
/// its stages in that order, and its medians raw and in the maintainer's active time.
pub fn stage_medians(
    prs: &[Pr],
    activity: &Activity,
) -> Vec<(&'static str, usize, Option<i64>, Option<i64>)> {
    stages::INTERVALS
        .iter()
        .map(|(from, to, label)| {
            let spans: Vec<(i64, i64)> = prs
                .iter()
                .filter_map(|p| p.stages.between(from, to))
                .collect();
            let raw: Vec<i64> = spans.iter().map(|(a, b)| b - a).collect();
            let active: Vec<i64> = spans.iter().map(|(a, b)| activity.active(*a, *b)).collect();
            (
                *label,
                spans.len(),
                time::median(&raw),
                time::median(&active),
            )
        })
        .collect()
}

/// The "Recommendations input": each process-created wait against its threshold, beside the
/// previous period.
fn recommendations(r: &Report, now: &Totals, before: Option<&Totals>, out: &mut String) {
    let _ = writeln!(out, "\n## Recommendations input\n");
    let previous = r.previous.as_ref().map_or_else(
        || "no previous period was fetched".to_owned(),
        |p| {
            format!(
                "the previous period is {} to {} ({})",
                time::date(p.since),
                time::date(r.since.unwrap_or(p.since) - 86_400),
                count(p.prs.len(), "pull request", "pull requests")
            )
        },
    );
    let _ = writeln!(
        out,
        "The waits the process created, each against a fixed threshold (see \"How these are \
         measured\"); {previous}. This is data for the coordinator's recommendations, which the \
         maintainer decides on. Review latency is not in it: its raw figure is the maintainer's \
         availability.\n"
    );
    let _ = writeln!(
        out,
        "| Measure | Threshold | In the period | Crossed | Previous period | Crossed by |\n\
         |---|---|---|---:|---:|---|"
    );
    let earlier = before.map(waits::measures);
    for (i, m) in waits::measures(now).iter().enumerate() {
        let prev = earlier
            .as_ref()
            .and_then(|e| e.get(i))
            .map_or_else(|| "–".to_owned(), |e| e.crossed.len().to_string());
        let shown: Vec<&str> = m.crossed.iter().take(r.top).map(String::as_str).collect();
        let more = m.crossed.len().saturating_sub(r.top);
        let by = match (shown.is_empty(), more) {
            (true, _) => "–".to_owned(),
            (false, 0) => shown.join(", "),
            (false, k) => format!("{}, and {k} more", shown.join(", ")),
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | **{}** | {prev} | {by} |",
            m.name,
            m.threshold,
            m.total,
            m.crossed.len()
        );
    }
}

/// The medians of the stage intervals, raw and in active time.
fn stage_summary(r: &Report, activity: &Activity, out: &mut String) {
    let _ = writeln!(out, "\n## Stages\n");
    let _ = writeln!(
        out,
        "Medians over the pull requests that passed both stages, in that order. *Active* counts \
         only the time within {} of a maintainer event (a comment, a review, or a merge) on the \
         repository.\n",
        time::duration(ACTIVE_WINDOW)
    );
    let _ = writeln!(
        out,
        "| Interval | PRs | Median | Median in active time |\n|---|---:|---:|---:|"
    );
    for (label, n, raw, active) in stage_medians(&r.prs, activity) {
        let note = if label == stages::REVIEW_WAIT {
            " (raw: not actionable)"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "| {label}{note} | {n} | {} | {} |",
            opt_duration(raw),
            opt_duration(active)
        );
    }
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for pr in &r.prs {
        *kinds.entry(pr.stages.approval.kind.name()).or_default() += 1;
    }
    let kinds: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} by {k}")).collect();
    let _ = writeln!(out, "\nApprovals: {}.", kinds.join(", "));
}

/// The details behind the "Recommendations input": order inversions and verification after
/// approval.
fn wait_details(r: &Report, t: &Totals, activity: &Activity, out: &mut String) {
    let at = |s: i64| crate::verify::utc(u64::try_from(s).unwrap_or(0));
    let _ = writeln!(out, "\n## Process-created waits\n");
    let _ = writeln!(out, "### Order inversions\n");
    let all: Vec<&Pr> = r.all_prs().collect();
    let pairs: Vec<String> = waits::dependencies(&all)
        .iter()
        .filter(|(b, _)| r.prs.iter().any(|p| p.number == b.number))
        .map(|(b, a)| format!("#{} on #{}", b.number, a.number))
        .collect();
    let _ = writeln!(
        out,
        "Dependencies found (branch ancestry): {}.\n",
        if pairs.is_empty() {
            "none".to_owned()
        } else {
            pairs.join(", ")
        }
    );
    if t.inversions.is_empty() {
        let _ = writeln!(
            out,
            "No pull request was approved while one it depends on was unreviewed."
        );
    } else {
        let _ = writeln!(
            out,
            "| Approved | At | Depends on | First reviewed or approved | Inverted for |\n\
             |---|---|---|---|---:|"
        );
        for i in &t.inversions {
            let _ = writeln!(
                out,
                "| #{} | {} | #{} | {} | {} |",
                i.pr,
                at(i.approved),
                i.waiting_on,
                at(i.reviewed),
                time::duration(i.seconds())
            );
        }
    }
    let _ = writeln!(out, "\n### Verification after approval\n");
    if t.after_approval.is_empty() {
        let _ = writeln!(out, "Nothing ran after an approval that was not the merge.");
    } else {
        let _ = writeln!(
            out,
            "| PR | Approval | Merges | Runs | Verify time | Approval → merged | In active time |\n\
             |---|---|---:|---:|---:|---:|---:|"
        );
        for (n, w) in t.after_approval.iter().take(r.top) {
            let Some(pr) = r.prs.iter().find(|p| p.number == *n) else {
                continue;
            };
            let a = pr.stages.approval;
            let _ = writeln!(
                out,
                "| #{n} | {} ({}) | {} | {} | {} | {} | {} |",
                at(a.at),
                a.kind.name(),
                w.merges,
                w.runs,
                time::duration(w.verify_s),
                time::duration(pr.merged - a.at),
                time::duration(activity.active(a.at, pr.merged))
            );
        }
    }
}

/// The failed and cancelled runs, and the rework after a maintainer decision.
fn run_details(r: &Report, t: &Totals, out: &mut String) {
    let at = |s: i64| crate::verify::utc(u64::try_from(s).unwrap_or(0));
    let _ = writeln!(out, "\n### Failed and cancelled runs\n");
    let mut wasted: Vec<(u64, &super::pr::Run)> =
        t.failed.iter().map(|(n, run)| (*n, run)).collect();
    for pr in &r.prs {
        wasted.extend(pr.waits.cancelled.iter().map(|run| (pr.number, run)));
    }
    if wasted.is_empty() {
        let _ = writeln!(out, "No run failed or was cancelled.");
    } else {
        wasted.sort_by_key(|(_, run)| std::cmp::Reverse(run.seconds.unwrap_or(0)));
        let _ = writeln!(
            out,
            "| PR | Commit | Result | Wall time | First gating failure | Ran after it |\n\
             |---|---|---|---:|---|---:|"
        );
        for (n, run) in wasted.iter().take(r.top) {
            let _ = writeln!(
                out,
                "| #{n} | `{}` | {} | {} | {} | {} |",
                short(&run.commit),
                run.result,
                opt_duration(run.seconds),
                run.first_failure
                    .as_ref()
                    .map_or_else(|| "–".to_owned(), |f| format!("`{f}`")),
                opt_duration(run.after_failure_s)
            );
        }
    }
    let _ = writeln!(out, "\n### Rework after a maintainer decision\n");
    if t.rework.is_empty() {
        let _ = writeln!(out, "No work followed a maintainer decision.");
    } else {
        let _ = writeln!(
            out,
            "| PR | First decision | Commits | Merges | Runs | Verify time |\n\
             |---|---|---:|---:|---:|---:|"
        );
        for (n, w) in t.rework.iter().take(r.top) {
            let first = r
                .prs
                .iter()
                .find(|p| p.number == *n)
                .and_then(|p| p.stages.first_decision)
                .map_or_else(|| "–".to_owned(), at);
            let _ = writeln!(
                out,
                "| #{n} | {first} | {} | {} | {} | {} |",
                w.commits,
                w.merges,
                w.runs,
                time::duration(w.verify_s)
            );
        }
    }
}

/// The per-PR stage timeline: each stage's distance from the stage before it that happened.
fn timeline(r: &Report, activity: &Activity, out: &mut String) {
    let _ = writeln!(out, "\n## Stage timeline\n");
    let _ = writeln!(
        out,
        "Each cell is the time from the stage before it that happened; a negative one came before \
         it (a review before the first passing record). The review wait runs from opened to the \
         first review.\n"
    );
    let heads: Vec<&str> = stages::NAMES.iter().skip(1).copied().collect();
    let _ = writeln!(
        out,
        "| PR | {} | Review wait | In active time |\n|---|{}---:|---:|",
        heads
            .iter()
            .map(|h| format!("→ {h}"))
            .collect::<Vec<_>>()
            .join(" | "),
        "---:|".repeat(heads.len())
    );
    for pr in &r.prs {
        let gaps = pr.stages.gaps();
        let cells: Vec<String> = heads
            .iter()
            .map(|h| {
                let gap = gaps.iter().find(|(n, _)| n == h).map(|(_, g)| *g);
                match (gap, *h) {
                    (Some(g), "approval") => {
                        format!("{} ({})", time::duration(g), pr.stages.approval.kind.name())
                    }
                    (None, "approval") if pr.stages.approval.kind == ApprovalKind::Merge => {
                        "by merge".to_owned()
                    }
                    (Some(g), _) => time::duration(g),
                    (None, _) => "–".to_owned(),
                }
            })
            .collect();
        let wait = pr.stages.between("opened", "first review");
        let _ = writeln!(
            out,
            "| #{} | {} | {} | {} |",
            pr.number,
            cells.join(" | "),
            opt_duration(wait.map(|(a, b)| b - a)),
            opt_duration(wait.map(|(a, b)| activity.active(a, b)))
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
";

/// How the stages and the process-created waits are made, with the constants they use.
fn method_stages() -> String {
    let d = time::duration;
    format!(
        "\
- **Maintainer:** the repository owner's account, which the agents post under too. A review is \
the maintainer's (as above). A conversation comment is the maintainer's own if it does not \
contain the agent's marker, is no review, has at most {max} characters, and has no bold text; the \
agent's replies that lack the marker are longer or open in bold.
- **Stages:** the first commit; opened; the first passing record (`finished` of the earliest \
published record of any of its commits; only passing records are published); the first review; \
the last maintainer decision (their last comment in the conversation); the approval; the passing record of the merged head; merged.
- **Approval:** the first `APPROVED` review; otherwise the maintainer's last word (the last of \
their reviews and comments up to the merge) if no commit other than a merge \
was authored after it; otherwise the merge itself, as the maintainer approves by merging.
- **Active time:** the union of the windows of {window} before and after each maintainer event: a \
review or review comment, a maintainer comment on any issue or pull request, and a merge. Pushes \
and workflow dispatches are left out, because agents make them while the maintainer is away. \
Each interval is reported raw and in active time; the raw wait for the first review is the \
maintainer's availability and not actionable.
- **Dependency:** pull request B depends on A if A's head is one of B's commits, or a parent of \
one of B's commits from outside B is a commit of A (B branched from A's branch or merged it). A \
merge from `main` brings `main`'s merge commit, which is no commit of A. Only the pull requests \
fetched (this period and the previous one) are compared.
- **Order inversion:** B was approved while A, which it depends on, was unreviewed: A's first \
review (or its approval, if that came first) is later than B's approval; it lasts from B's \
approval to that review.
- **Verification after approval:** from an approval that was not the merge to the merge: the \
merges, and the verification runs started then, with their wall time.
- **Wasted runs:** failed and cancelled `verify.yml` runs. For a failed run whose artefact holds \
its record, the first gating check that failed and the minutes of the checks that ran after it \
(the record's order, the macOS checks last). A local run that fails leaves no trace.
- **Rework after a maintainer decision:** from the first decision to the approval: the commits \
other than merges authored then, and the verification runs started then. A pull request counts \
only if at least one commit was authored then; a run without one re-ran a head.
- **Process overhead:** a changed path is *product* in `crates/`, `cpp/`, `include/`, \
`test-vectors/`, `specs/`, `exerciser/`, `fuzz/`, `refapp/`, `crosscheck/`, and `difftest/`, and \
in the specification: `docs/` except `docs/process/`, `docs/verification/`, and \
`docs/milestones/` (so `docs/0*.md`, `docs/profiles/`, `docs/standards/`, and the other \
documents of `docs/`). Every other path is *process*: `xtask/`, `.github/`, those three parts of \
`docs/`, `AGENTS.md`, and the other root files. A pull request is product-only, \
process-only, or mixed by the files it changed (`pulls/{{n}}/files`), a commit other than a merge \
by the paths it changes against its parent (`git diff-tree`); merges are not classified. A \
*bookkeeping-only* commit changes only files under \
`docs/verification/records/`, or the rows of a milestone plan's work-package status table (the \
plan before and after, `git show`).
- **Thresholds:** every order inversion; verification after approval of {update} or more in one \
pull request; a failed run that went on {fail} or more after its first gating failure; cancelled \
runs of {cancel} or more in one pull request; at least {rework} verification run after a decision \
in one pull request; a file in {conflicts} or more conflicted merges in the period; process-only \
pull requests that took {share} % or more of the period's verification minutes (each of them is \
then listed); {bookkeeping} or more bookkeeping-only commits in one pull request (one is expected: \
the commit of its records and status rows). The previous period has the same length and ends where \
this one starts.
- Generated by `cargo xtask metrics` (docs/06-verification.md §9, VER-063).
",
        max = stages::HUMAN_MAX,
        window = d(ACTIVE_WINDOW),
        update = d(waits::UPDATE_MIN_S),
        fail = d(waits::AFTER_FAILURE_MIN_S),
        cancel = d(waits::CANCELLED_MIN_S),
        rework = waits::REWORK_MIN_RUNS,
        conflicts = waits::CONFLICT_MIN_MERGES,
        share = overhead::PROCESS_SHARE_PCT,
        bookkeeping = overhead::BOOKKEEPING_MIN,
    )
}

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
    prose(&s, &mut out);
    let activity = r.activity();
    let (now, before) = r.totals();
    recommendations(r, &now, before.as_ref(), &mut out);
    stage_summary(r, &activity, &mut out);
    wait_details(r, &now, &activity, &mut out);
    run_details(r, &now, &mut out);
    overhead_section(r, &now, &mut out);
    friction(r, &s, &mut out);
    table(r, &mut out);
    timeline(r, &activity, &mut out);
    let _ = write!(out, "\n{METHOD}{}", method_stages());
    out
}

/// The JSON report: the summary and the per-PR data, for trend analysis.
pub fn json(r: &Report) -> Value {
    let s = summary(&r.prs);
    let at = |t: i64| crate::verify::utc(u64::try_from(t).unwrap_or(0));
    let activity = r.activity();
    let (now, before) = r.totals();
    let medians = |prs: &[Pr]| {
        stage_medians(prs, &activity)
            .into_iter()
            .map(|(label, n, raw, active)| {
                json!({"interval": label, "prs": n, "raw_median_s": raw,
                       "active_median_s": active})
            })
            .collect::<Vec<_>>()
    };
    json!({
        "format": "aldis-metrics/1",
        "repo": r.repo,
        "generated": at(r.generated),
        "since": r.since.map(at),
        "maintainer": r.maintainer,
        "active_window_s": ACTIVE_WINDOW,
        "stages": medians(&r.prs),
        "waits": waits::to_json(&now),
        "previous": r.previous.as_ref().map(|p| json!({
            "since": at(p.since),
            "prs": p.prs.len(),
            "stages": medians(&p.prs),
            "waits": before.as_ref().map(waits::to_json),
        })),
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
        "prs": r.prs.iter().map(|p| p.to_json(&activity)).collect::<Vec<_>>(),
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

    /// #70 with a decision (approved by its last word at 13:54:12), stacked on #69, whose head m2
    /// it merged; #69 was first reviewed at 14:30, after #70's approval.
    fn stacked() -> Vec<Pr> {
        let top = compute(&pr::tests::decided()).unwrap();
        let mut raw = pr::tests::fixture();
        raw.pull["number"] = json!(69);
        raw.pull["head"] = json!({"ref": "m2/base", "sha": "m2"});
        raw.pull["base"] = json!({"ref": "main"});
        raw.pull["created_at"] = json!("2026-10-21T11:00:00Z");
        raw.pull["merged_at"] = json!("2026-10-21T15:30:00Z");
        raw.commits = vec![pr::tests::commit("m2", &["base"], "2026-10-21T11:00:00Z")];
        raw.reviews = vec![json!({"user": {"login": "me"}, "state": "COMMENTED",
            "submitted_at": "2026-10-21T14:30:00Z", "body": "# Review"})];
        raw.review_comments.clear();
        raw.issue_comments.clear();
        raw.records.clear();
        raw.runs.clear();
        let base = compute(&raw).unwrap();
        let mut top = top;
        top.base = "main".to_owned();
        vec![top, base]
    }

    // req: VER-063
    #[test]
    fn renders_stages_waits_and_recommendations_input() {
        let report = Report {
            maintainer: "me".to_owned(),
            events: Vec::new(),
            previous: Some(Previous {
                since: time::parse("2026-09-16").unwrap(),
                prs: Vec::new(),
            }),
            repo: "o/r".to_owned(),
            generated: time::parse("2026-10-21T16:00:00Z").unwrap(),
            since: time::parse("2026-10-16"),
            top: 10,
            prs: stacked(),
        };
        let md = markdown(&report);
        for want in [
            "## Recommendations input\n",
            "the previous period is 2026-09-16 to 2026-10-15 (0 pull requests).",
            "| Order inversions | every one | 1, 35 min in all | **1** | 0 | #70 before #69 \
             (35 min) |",
            "| Verification after approval | ≥ 10 min of runs in one PR | 1 PR, 15 min of runs | \
             **1** | 0 | #70 (15 min, 1 run) |",
            "| Failed runs that went on after the first gating failure | ≥ 5 min after it, per \
             run | 1 failed run, 20 min; 3 min after a first gating failure | **0** | 0 | – |",
            "| Cancelled runs | ≥ 10 min of cancelled runs in one PR | 1 run, 4 min | **0** | 0 | – |",
            "| Rework after a maintainer decision | ≥ 1 verification run after it, in one PR | 1 PR, \
             1 commit, 0 s of runs | **0** | 0 | – |",
            "| Repeated conflicts | a file in ≥ 3 conflicted merges | 1 file conflicted | **0** | 0 \
             | – |",
            "## Stages\n",
            "| waiting for the first review (raw: not actionable) | 2 |",
            "Approvals: 2 by last word.",
            "## Process-created waits\n",
            "Dependencies found (branch ancestry): #70 on #69.\n",
            "| #70 | 2026-10-21T13:54:12Z | #69 | 2026-10-21T14:30:00Z | 35 min |",
            // Events from 13:46 to 15:30, windows of 30 min: active from 13:16 to 16:00.
            "| #70 | 2026-10-21T13:54:12Z (last word) | 0 | 1 | 15 min | 1 h 19 min | 1 h 19 min |",
            "| #70 | `c` | failure | 20 min | `header` | 3 min |",
            "| #70 | `d` | cancelled | 4 min | – | – |",
            "| #70 | 2026-10-21T13:52:00Z | 1 | 0 | 0 | 0 s |",
            "## Stage timeline\n",
            "| #70 | 5 min | 12 min | 1 h 36 min | 8 min | 0 s (last word) | 1 h 14 min | 4 min | \
             1 h 48 min | 30 min |",
            // Opened 11:00, reviewed 14:30: active from 13:16:03.
            "| #69 | 0 s | – | 3 h 30 min | – | 0 s (last word) | – | 1 h 00 min | 3 h 30 min | \
             1 h 13 min |",
            "- **Approval:** the first `APPROVED` review;",
            "the union of the windows of 30 min before and after each maintainer event",
        ] {
            assert!(md.contains(want), "missing {want:?} in\n{md}");
        }
        let j = json(&report);
        assert_eq!(j["maintainer"], "me");
        assert_eq!(j["active_window_s"], 1_800);
        assert_eq!(j["waits"]["inversions"][0]["pr"], 70);
        assert_eq!(j["waits"]["measures"][0]["crossed"], 1);
        assert_eq!(j["stages"][2]["interval"], "waiting for the first review");
        assert_eq!(j["stages"][2]["prs"], 2);
        assert_eq!(j["previous"]["prs"], 0);
        assert_eq!(j["previous"]["waits"]["measures"][0]["crossed"], 0);
        assert_eq!(j["prs"][0]["stages"]["approval"]["kind"], "last word");
        // Without a previous period, its column is empty.
        let none = Report {
            previous: None,
            ..report
        };
        let md = markdown(&none);
        assert!(md.contains("no previous period was fetched"));
        assert!(
            md.contains("| **1** | – | #70 before #69 (35 min) |"),
            "{md}"
        );
    }

    // req: VER-063
    #[test]
    fn renders_the_process_overhead() {
        use crate::metrics::overhead::{Commit, Overhead};
        let mut prs = two();
        prs[1].overhead = Overhead {
            class: Some(Class::Process),
            commits: vec![
                Commit {
                    sha: "0123456789ab".to_owned(),
                    class: Class::Process,
                    bookkeeping: true,
                },
                Commit {
                    sha: "fedcba987654".to_owned(),
                    class: Class::Process,
                    bookkeeping: true,
                },
            ],
            unknown: 1,
        };
        prs[0].overhead.class = Some(Class::Product);
        let report = Report {
            maintainer: "me".to_owned(),
            events: Vec::new(),
            previous: None,
            repo: "o/r".to_owned(),
            generated: time::parse("2026-10-21T16:00:00Z").unwrap(),
            since: time::parse("2026-10-16"),
            top: 10,
            prs,
        };
        let md = markdown(&report);
        for want in [
            // #70: 29 min of runs; #71: 10 min.
            "| Verification of process-only PRs | ≥ 33 % of the period's verify minutes | 10 min \
             of 39 min (25 %); 50 % of PRs, 100 % of commits process-only | **0** | – | – |",
            "| Bookkeeping-only commits | ≥ 2 in one PR | 2 commits in 1 PR | **1** | – | #71 (2 \
             commits) |",
            "## Process overhead\n",
            "| Pull requests | 1 (50 %) | 1 (50 %) | 0 (0 %) |",
            "| Commits | 2 (100 %) | 0 (0 %) | 0 (0 %) |",
            "| Verification minutes | 10 min (25 %) | 29 min (75 %) | 0 s (0 %) |",
            "Not counted: 4 commits whose changed paths git could not read.",
            "### Bookkeeping-only commits\n",
            "| #71 | 2 | `01234567` `fedcba98` |",
            "- **Process overhead:** a changed path is *product* in `crates/`",
            "process-only pull requests that took 33 % or more of the period's verification minutes",
        ] {
            assert!(md.contains(want), "missing {want:?} in\n{md}");
        }
        let j = json(&report);
        assert_eq!(j["waits"]["overhead"]["bookkeeping_commits"], 2);
        assert_eq!(j["prs"][1]["overhead"]["class"], "process-only");
        assert_eq!(j["waits"]["measures"][7]["crossed"], 1);
        // Without bookkeeping-only commits, the section says so.
        let mut none = report;
        none.prs[1].overhead.commits.clear();
        assert!(markdown(&none).contains("No commit changed only bookkeeping."));
    }

    // req: VER-063
    #[test]
    fn renders_markdown_and_json() {
        let report = Report {
            maintainer: "me".to_owned(),
            events: Vec::new(),
            previous: None,
            repo: "o/r".to_owned(),
            generated: time::parse("2026-10-21T16:00:00Z").unwrap(),
            since: time::parse("2026-10-16"),
            top: 1,
            prs: two(),
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

        // One pull request has no period.
        let one = Report {
            since: None,
            prs: two().into_iter().take(1).collect(),
            ..report
        };
        let md = markdown(&one);
        assert!(md.contains("Covers pull request #70 in `o/r`."), "{md}");
        assert!(md.contains("- **1 pull request** merged, 5 commits, 3 files changed"));
        let both = Report {
            since: time::parse("2026-10-16"),
            prs: two(),
            ..one
        };
        let j = json(&both);
        assert_eq!(j["summary"]["prs"], 2);
        assert_eq!(j["summary"]["conflict_files"][0]["path"], "docs/plan.md");
        assert_eq!(j["prs"][1]["number"], 71);
        assert_eq!(j["since"], "2026-10-16T00:00:00Z");
    }
}
