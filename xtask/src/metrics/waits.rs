//! The waits the process creates, as against those that are only the maintainer's availability
//! (VER-063): order inversions, verification after the approval, wasted runs, rework after a
//! maintainer decision, and repeated conflicts; and the thresholds that put them in the
//! "Recommendations input" of the report.

use serde_json::{Value, json};

use super::pr::{Pr, Run};
use super::time;

/// A pull request whose verification runs after its approval took at least this long, in all.
pub const UPDATE_MIN_S: i64 = 10 * 60;
/// A failed run that went on at least this long after its first gating failure.
pub const AFTER_FAILURE_MIN_S: i64 = 5 * 60;
/// A pull request whose cancelled runs took at least this long, in all.
pub const CANCELLED_MIN_S: i64 = 10 * 60;
/// A pull request with at least this many verification runs after a maintainer decision.
pub const REWORK_MIN_RUNS: usize = 1;
/// A file that conflicted in at least this many merges from `main`.
pub const CONFLICT_MIN_MERGES: usize = 3;

/// A commit of a pull request, as far as the waits need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Its author date: a rebase keeps it.
    pub authored: i64,
    /// Its committer date.
    pub committed: i64,
    /// Whether it merges a commit from outside the pull request (`main`, or a branch it is
    /// stacked on).
    pub merge: bool,
}

/// The work in an interval: commits, merges, verification runs, and their wall time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Commits other than merges, by author date.
    pub commits: usize,
    /// Merges, by committer date.
    pub merges: usize,
    /// Verification runs, by start.
    pub runs: usize,
    /// Their wall time, in seconds.
    pub verify_s: i64,
}

impl Work {
    /// Whether anything happened.
    pub const fn is_empty(&self) -> bool {
        self.commits == 0 && self.merges == 0 && self.runs == 0
    }

    fn to_json(self) -> Value {
        json!({
            "commits": self.commits, "merges": self.merges, "runs": self.runs,
            "verify_s": self.verify_s,
        })
    }
}

/// The work of `commits` and `runs` in `(from, to]`.
pub fn work_in(commits: &[Commit], runs: &[Run], from: i64, to: i64) -> Work {
    let inside = |t: i64| t > from && t <= to;
    let in_runs: Vec<&Run> = runs
        .iter()
        .filter(|r| r.started.is_some_and(inside))
        .collect();
    Work {
        commits: commits
            .iter()
            .filter(|c| !c.merge && inside(c.authored))
            .count(),
        merges: commits
            .iter()
            .filter(|c| c.merge && inside(c.committed))
            .count(),
        runs: in_runs.len(),
        verify_s: in_runs.iter().filter_map(|r| r.seconds).sum(),
    }
}

/// The process-created waits of one pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Waits {
    /// From an approval that was not the merge to the merge: what getting a mergeable head took.
    pub after_approval: Work,
    /// From the first maintainer decision to the approval: the work the decision caused.
    pub rework: Work,
    /// Its failed runs.
    pub failed: Vec<Run>,
    /// Its cancelled runs.
    pub cancelled: Vec<Run>,
}

impl Waits {
    /// Seconds of its cancelled runs.
    pub fn cancelled_s(&self) -> i64 {
        self.cancelled.iter().filter_map(|r| r.seconds).sum()
    }

    /// The JSON form.
    pub fn to_json(&self) -> Value {
        let run = |r: &Run| {
            json!({
                "commit": r.commit, "result": r.result, "seconds": r.seconds,
                "first_failure": r.first_failure, "after_failure_s": r.after_failure_s,
            })
        };
        json!({
            "after_approval": self.after_approval.to_json(),
            "rework_after_decision": self.rework.to_json(),
            "failed_runs": self.failed.iter().map(run).collect::<Vec<_>>(),
            "cancelled_runs": self.cancelled.iter().map(run).collect::<Vec<_>>(),
        })
    }
}

/// Whether `b` depends on `a`: `b`'s history took commits from `a`'s branch before `b` merged.
/// Either `a`'s head is one of `b`'s commits (`b` carried `a`'s work), or a parent of one of
/// `b`'s commits from outside `b` is a commit of `a` (`b` was branched from `a`, or merged it).
/// A merge from `main` takes `main`'s merge commit, which is no commit of `a`, so a pull request
/// merged into `main` before `b` merged `main` is no dependency. A pull request from the base
/// branch itself (`main` into another branch) is never one.
pub fn depends_on(b: &Pr, a: &Pr) -> bool {
    a.number != b.number
        && a.branch != a.base
        && b.branch != b.base
        && (b.shas.contains(&a.head) || b.outside_parents.iter().any(|p| a.shas.contains(p)))
}

/// An order inversion: `pr` was approved while `waiting_on`, a pull request it depends on, was
/// still unreviewed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inversion {
    /// The approved pull request.
    pub pr: u64,
    /// The pull request it depends on.
    pub waiting_on: u64,
    /// When `pr` was approved.
    pub approved: i64,
    /// When `waiting_on` was first reviewed, or approved if that came first.
    pub reviewed: i64,
}

impl Inversion {
    /// How long the dependency stayed unreviewed after the approval, in seconds.
    pub const fn seconds(&self) -> i64 {
        self.reviewed - self.approved
    }

    fn to_json(&self) -> Value {
        let at = |t: i64| crate::verify::utc(u64::try_from(t).unwrap_or(0));
        json!({
            "pr": self.pr, "waiting_on": self.waiting_on, "approved": at(self.approved),
            "reviewed": at(self.reviewed), "seconds": self.seconds(),
        })
    }
}

/// When a pull request was first looked at by the maintainer: its first review, or its approval
/// if that came first (an approval by merge, for one never reviewed).
fn reviewed(pr: &Pr) -> i64 {
    let approved = pr.stages.approval.at;
    pr.stages.first_review.map_or(approved, |r| r.min(approved))
}

/// The order inversions among `prs`, each under the approved pull request, in its order.
pub fn inversions(prs: &[&Pr]) -> Vec<Inversion> {
    dependencies(prs)
        .into_iter()
        .filter(|(b, a)| reviewed(a) > b.stages.approval.at)
        .map(|(b, a)| Inversion {
            pr: b.number,
            waiting_on: a.number,
            approved: b.stages.approval.at,
            reviewed: reviewed(a),
        })
        .collect()
}

/// The pairs `(b, a)` among `prs` where `b` depends on `a` ([`depends_on`]), in `b`'s order.
pub fn dependencies<'a>(prs: &[&'a Pr]) -> Vec<(&'a Pr, &'a Pr)> {
    let mut out = Vec::new();
    for b in prs {
        for a in prs {
            if depends_on(b, a) {
                out.push((*b, *a));
            }
        }
    }
    out
}

/// The process-created waits of a period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Order inversions whose approved pull request is in the period.
    pub inversions: Vec<Inversion>,
    /// Pull requests with work after their approval.
    pub after_approval: Vec<(u64, Work)>,
    /// Pull requests with work after a maintainer decision.
    pub rework: Vec<(u64, Work)>,
    /// Failed runs, by pull request.
    pub failed: Vec<(u64, Run)>,
    /// Pull requests with cancelled runs: their number and seconds.
    pub cancelled: Vec<(u64, usize, i64)>,
    /// Files and the merges from `main` they conflicted in, most first.
    pub conflicts: Vec<(String, usize)>,
}

/// The totals of `prs`, with `inversions` over a wider set and the period's `conflicts`.
pub fn totals(prs: &[Pr], inversions: &[Inversion], conflicts: &[(String, usize)]) -> Totals {
    let mut t = Totals {
        inversions: inversions
            .iter()
            .filter(|i| prs.iter().any(|p| p.number == i.pr))
            .cloned()
            .collect(),
        conflicts: conflicts.to_vec(),
        ..Totals::default()
    };
    for pr in prs {
        let w = &pr.waits;
        if !w.after_approval.is_empty() {
            t.after_approval.push((pr.number, w.after_approval));
        }
        // A run after a decision with no new commit re-ran a head: no rework.
        if w.rework.commits > 0 {
            t.rework.push((pr.number, w.rework));
        }
        t.failed
            .extend(w.failed.iter().map(|r| (pr.number, r.clone())));
        if !w.cancelled.is_empty() {
            t.cancelled
                .push((pr.number, w.cancelled.len(), w.cancelled_s()));
        }
    }
    t
}

/// One row of the "Recommendations input": a measure, its threshold, and what crossed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measure {
    /// What is measured.
    pub name: &'static str,
    /// What crosses the threshold.
    pub threshold: String,
    /// The period's total of the measure, in words.
    pub total: String,
    /// The pull requests, runs, or files that crossed it.
    pub crossed: Vec<String>,
}

/// The measures of [`Totals`], each with what crossed its threshold: order inversions,
/// verification after approval, failed runs, cancelled runs, rework, and repeated conflicts.
pub fn measures(t: &Totals) -> Vec<Measure> {
    [approval_measures(t), run_measures(t), work_measures(t)].concat()
}

/// Order inversions and verification after approval.
fn approval_measures(t: &Totals) -> Vec<Measure> {
    let d = time::duration;
    let inversion_s: i64 = t.inversions.iter().map(Inversion::seconds).sum();
    let update_s: i64 = t.after_approval.iter().map(|(_, w)| w.verify_s).sum();
    vec![
        Measure {
            name: "Order inversions",
            threshold: "every one".to_owned(),
            total: format!("{}, {} in all", t.inversions.len(), d(inversion_s)),
            crossed: t
                .inversions
                .iter()
                .map(|i| format!("#{} before #{} ({})", i.pr, i.waiting_on, d(i.seconds())))
                .collect(),
        },
        Measure {
            name: "Verification after approval",
            threshold: format!("≥ {} of runs in one PR", d(UPDATE_MIN_S)),
            total: format!(
                "{}, {} of runs",
                n_of(
                    t.after_approval.iter().filter(|(_, w)| w.runs > 0).count(),
                    "PR"
                ),
                d(update_s)
            ),
            crossed: t
                .after_approval
                .iter()
                .filter(|(_, w)| w.verify_s >= UPDATE_MIN_S)
                .map(|(n, w)| format!("#{n} ({}, {})", d(w.verify_s), n_of(w.runs, "run")))
                .collect(),
        },
    ]
}

/// Failed and cancelled runs.
fn run_measures(t: &Totals) -> Vec<Measure> {
    let d = time::duration;
    let after_failure: Vec<&(u64, Run)> = t
        .failed
        .iter()
        .filter(|(_, r)| r.after_failure_s.is_some_and(|s| s >= AFTER_FAILURE_MIN_S))
        .collect();
    let after_failure_s: i64 = t.failed.iter().filter_map(|(_, r)| r.after_failure_s).sum();
    let failed_s: i64 = t.failed.iter().filter_map(|(_, r)| r.seconds).sum();
    let cancelled_s: i64 = t.cancelled.iter().map(|c| c.2).sum();
    vec![
        Measure {
            name: "Failed runs that went on after the first gating failure",
            threshold: format!("≥ {} after it, per run", d(AFTER_FAILURE_MIN_S)),
            total: format!(
                "{}, {}; {} after a first gating failure",
                n_of(t.failed.len(), "failed run"),
                d(failed_s),
                d(after_failure_s)
            ),
            crossed: after_failure
                .iter()
                .map(|(n, r)| {
                    format!(
                        "#{n} `{}` ({} after `{}`)",
                        short(&r.commit),
                        d(r.after_failure_s.unwrap_or(0)),
                        r.first_failure.as_deref().unwrap_or("?")
                    )
                })
                .collect(),
        },
        Measure {
            name: "Cancelled runs",
            threshold: format!("≥ {} of cancelled runs in one PR", d(CANCELLED_MIN_S)),
            total: format!(
                "{}, {}",
                n_of(t.cancelled.iter().map(|c| c.1).sum::<usize>(), "run"),
                d(cancelled_s)
            ),
            crossed: t
                .cancelled
                .iter()
                .filter(|c| c.2 >= CANCELLED_MIN_S)
                .map(|(n, k, s)| format!("#{n} ({}, {})", n_of(*k, "run"), d(*s)))
                .collect(),
        },
    ]
}

/// Rework after a maintainer decision, and repeated conflicts.
fn work_measures(t: &Totals) -> Vec<Measure> {
    let d = time::duration;
    let rework_s: i64 = t.rework.iter().map(|(_, w)| w.verify_s).sum();
    let rework_commits: usize = t.rework.iter().map(|(_, w)| w.commits).sum();
    vec![
        Measure {
            name: "Rework after a maintainer decision",
            threshold: format!("≥ {REWORK_MIN_RUNS} verification run after it, in one PR"),
            total: format!(
                "{}, {}, {} of runs",
                n_of(t.rework.len(), "PR"),
                n_of(rework_commits, "commit"),
                d(rework_s)
            ),
            crossed: t
                .rework
                .iter()
                .filter(|(_, w)| w.runs >= REWORK_MIN_RUNS)
                .map(|(n, w)| {
                    format!(
                        "#{n} ({}, {}, {})",
                        n_of(w.commits, "commit"),
                        n_of(w.runs, "run"),
                        d(w.verify_s)
                    )
                })
                .collect(),
        },
        Measure {
            name: "Repeated conflicts",
            threshold: format!("a file in ≥ {CONFLICT_MIN_MERGES} conflicted merges"),
            total: format!("{} conflicted", n_of(t.conflicts.len(), "file")),
            crossed: t
                .conflicts
                .iter()
                .filter(|(_, n)| *n >= CONFLICT_MIN_MERGES)
                .map(|(f, n)| format!("`{f}` ({n})"))
                .collect(),
        },
    ]
}

/// `k` and the noun, with an `s` unless `k` is 1.
pub fn n_of(k: usize, noun: &str) -> String {
    format!("{k} {noun}{}", if k == 1 { "" } else { "s" })
}

/// The first eight characters of a commit ID.
pub fn short(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}

/// The JSON form of the totals and their measures.
pub fn to_json(t: &Totals) -> Value {
    json!({
        "inversions": t.inversions.iter().map(Inversion::to_json).collect::<Vec<_>>(),
        "after_approval": t.after_approval.iter()
            .map(|(n, w)| json!({"pr": n, "work": w.to_json()})).collect::<Vec<_>>(),
        "rework_after_decision": t.rework.iter()
            .map(|(n, w)| json!({"pr": n, "work": w.to_json()})).collect::<Vec<_>>(),
        "failed_runs": t.failed.len(),
        "failed_s": t.failed.iter().filter_map(|(_, r)| r.seconds).sum::<i64>(),
        "after_first_gating_failure_s": t.failed.iter()
            .filter_map(|(_, r)| r.after_failure_s).sum::<i64>(),
        "cancelled_runs": t.cancelled.iter().map(|c| c.1).sum::<usize>(),
        "cancelled_s": t.cancelled.iter().map(|c| c.2).sum::<i64>(),
        "measures": measures(t).iter().map(|m| json!({
            "name": m.name, "threshold": m.threshold, "total": m.total,
            "crossed": m.crossed.len(), "items": m.crossed,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::stages::{Approval, ApprovalKind, Stages};
    use std::collections::BTreeSet;

    fn run(started: i64, seconds: i64, result: &str) -> Run {
        Run {
            kind: "github",
            commit: format!("c{started}"),
            seconds: Some(seconds),
            result: result.to_owned(),
            started: Some(started),
            ..Run::default()
        }
    }

    // req: VER-063
    #[test]
    fn work_in_an_interval() {
        let commits = [
            Commit {
                authored: 10,
                committed: 10,
                merge: false,
            },
            // Rebased after 50, authored before: not counted after 50.
            Commit {
                authored: 20,
                committed: 60,
                merge: false,
            },
            Commit {
                authored: 55,
                committed: 70,
                merge: true,
            },
            Commit {
                authored: 80,
                committed: 80,
                merge: false,
            },
        ];
        let runs = [
            run(40, 5, "success"),
            run(75, 7, "success"),
            run(100, 9, "x"),
        ];
        let w = work_in(&commits, &runs, 50, 100);
        assert_eq!(
            w,
            Work {
                commits: 1,
                merges: 1,
                runs: 2,
                verify_s: 16
            }
        );
        assert!(work_in(&commits, &runs, 100, 50).is_empty());
        assert!(!w.is_empty());
    }

    /// A merged pull request with the given commits, head, and stages.
    fn pr(number: u64, shas: &[&str], outside: &[&str], review: Option<i64>, approved: i64) -> Pr {
        Pr {
            number,
            branch: format!("b{number}"),
            base: "main".to_owned(),
            head: (*shas.last().unwrap()).to_owned(),
            shas: shas.iter().map(|s| (*s).to_owned()).collect(),
            outside_parents: outside
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<BTreeSet<_>>(),
            stages: Stages {
                first_review: review,
                approval: Approval {
                    at: approved,
                    kind: ApprovalKind::Merge,
                },
                merged: approved,
                ..Stages::default()
            },
            ..Pr::default()
        }
    }

    // req: VER-063
    #[test]
    fn detects_dependencies_by_branch_ancestry() {
        // #1 on main; #2 branched from #1's head a2; #3 merged #2's branch (b1) into its own;
        // #4 merged main's merge commit m1 of #1: no dependency.
        let one = pr(1, &["a1", "a2"], &["base"], None, 100);
        let two = pr(2, &["b1", "b2"], &["a2"], None, 100);
        let three = pr(3, &["c1", "c2"], &["base", "b1"], None, 100);
        let four = pr(4, &["d1", "d2"], &["m1"], None, 100);
        // #5 carried #1's commits to main (they are among its own).
        let five = pr(5, &["a1", "a2", "e1"], &["base"], None, 100);
        assert!(depends_on(&two, &one));
        assert!(!depends_on(&one, &two));
        assert!(depends_on(&three, &two));
        assert!(!depends_on(&three, &one));
        assert!(!depends_on(&four, &one));
        assert!(depends_on(&five, &one));
        assert!(!depends_on(&one, &one));
        // A pull request from the base branch is never a dependency.
        let mut main = pr(6, &["a2"], &[], None, 1);
        main.branch = "main".to_owned();
        assert!(!depends_on(&two, &main));
    }

    // req: VER-063
    #[test]
    fn an_inversion_is_an_approval_before_the_dependency_was_reviewed() {
        // #2 depends on #1. #2 approved at 100; #1 first reviewed at 400: inverted for 300 s.
        let one = pr(1, &["a1"], &["base"], Some(400), 500);
        let two = pr(2, &["b1"], &["a1"], Some(50), 100);
        let found = inversions(&[&one, &two]);
        assert_eq!(
            found,
            [Inversion {
                pr: 2,
                waiting_on: 1,
                approved: 100,
                reviewed: 400
            }]
        );
        assert_eq!(found[0].seconds(), 300);
        // #1 reviewed before #2's approval: in order.
        let one = pr(1, &["a1"], &["base"], Some(90), 500);
        assert_eq!(inversions(&[&one, &two]), Vec::<Inversion>::new());
        // Never reviewed, but merged (approved by merging) before #2's approval: in order.
        let one = pr(1, &["a1"], &["base"], None, 80);
        assert_eq!(inversions(&[&one, &two]), Vec::<Inversion>::new());
        // Never reviewed and merged after #2: inverted until its merge.
        let one = pr(1, &["a1"], &["base"], None, 160);
        assert_eq!(inversions(&[&one, &two])[0].seconds(), 60);
    }

    // req: VER-063
    #[test]
    fn totals_and_measures_with_thresholds() {
        let mut first = pr(1, &["a1"], &["base"], None, 100);
        first.waits.after_approval = Work {
            merges: 1,
            runs: 2,
            verify_s: 11 * 60,
            ..Work::default()
        };
        first.waits.failed = vec![Run {
            after_failure_s: Some(6 * 60),
            first_failure: Some("header".to_owned()),
            ..run(10, 30 * 60, "failure")
        }];
        first.waits.cancelled = vec![run(20, 4 * 60, "cancelled"), run(30, 7 * 60, "cancelled")];
        // A run after a decision without a new commit re-ran a head: not rework.
        first.waits.rework = Work {
            runs: 1,
            verify_s: 60,
            ..Work::default()
        };
        let mut second = pr(2, &["b1"], &["a1"], None, 50);
        second.waits.after_approval = Work {
            merges: 1,
            runs: 1,
            verify_s: 60,
            ..Work::default()
        };
        second.waits.rework = Work {
            commits: 2,
            runs: 1,
            verify_s: 600,
            ..Work::default()
        };
        second.waits.failed = vec![Run {
            after_failure_s: Some(60),
            ..run(40, 120, "failure")
        }];
        let prs = vec![first, second];
        let inv = inversions(&prs.iter().collect::<Vec<_>>());
        let conflicts = vec![("docs/a.md".to_owned(), 3), ("docs/b.md".to_owned(), 2)];
        // An inversion of a pull request outside the period is left out.
        let outside = Inversion {
            pr: 9,
            waiting_on: 1,
            approved: 0,
            reviewed: 1,
        };
        let all = totals(&prs, &[inv.clone(), vec![outside]].concat(), &conflicts);
        assert_eq!(all.inversions, inv);
        assert_eq!(all.after_approval.len(), 2);
        assert_eq!(all.rework.len(), 1);
        assert_eq!(all.failed.len(), 2);
        assert_eq!(all.cancelled, [(1, 2, 11 * 60)]);
        let found = measures(&all);
        let crossed: Vec<(&str, Vec<String>)> =
            found.iter().map(|m| (m.name, m.crossed.clone())).collect();
        assert_eq!(
            crossed,
            [
                ("Order inversions", vec!["#2 before #1 (50 s)".to_owned()]),
                (
                    "Verification after approval",
                    vec!["#1 (11 min, 2 runs)".to_owned()]
                ),
                (
                    "Failed runs that went on after the first gating failure",
                    vec!["#1 `c10` (6 min after `header`)".to_owned()]
                ),
                ("Cancelled runs", vec!["#1 (2 runs, 11 min)".to_owned()]),
                (
                    "Rework after a maintainer decision",
                    vec!["#2 (2 commits, 1 run, 10 min)".to_owned()]
                ),
                ("Repeated conflicts", vec!["`docs/a.md` (3)".to_owned()]),
            ]
        );
        assert_eq!(
            found[2].total,
            "2 failed runs, 32 min; 7 min after a first gating failure"
        );
        let j = to_json(&all);
        assert_eq!(j["failed_runs"], 2);
        assert_eq!(j["cancelled_s"], 660);
        assert_eq!(j["measures"][0]["crossed"], 1);
        assert_eq!(j["inversions"][0]["seconds"], 50);
        assert_eq!(short("0123456789abcdef"), "01234567");
        assert_eq!(short("abc"), "abc");
        assert_eq!(n_of(1, "run"), "1 run");
        assert_eq!(n_of(0, "run"), "0 runs");
        let one = Waits {
            after_approval: Work {
                verify_s: 660,
                ..Work::default()
            },
            ..Waits::default()
        };
        assert_eq!(one.to_json()["after_approval"]["verify_s"], 660);
    }
}
