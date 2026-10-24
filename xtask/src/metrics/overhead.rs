//! The process overhead of the pull requests (VER-063): how much of the work, the commits, and
//! the verification minutes went to the process (tooling, workflows, plans, records, and the
//! log) rather than the product (the code, the vectors, and the specification), and how many
//! commits only kept the books.
//!
//! A changed path is **product** when it lies in a product directory ([`PRODUCT_DIRS`]) or is
//! part of the specification: `docs/` other than its process parts ([`PROCESS_DOCS`]), so the
//! requirement documents `docs/0*.md`, `docs/profiles/`, `docs/standards/`, and the rest of the
//! specification (`docs/README.md`, `docs/glossary.md`, `docs/reference-application.md`). Every
//! other path is **process**: `xtask/`, `.github/`, `docs/process/`, `docs/verification/`,
//! `docs/milestones/`, `changelog.d/`, `AGENTS.md`, and the other files and directories at the
//! root. A pull request or a commit is product-only, process-only, or mixed by its changed
//! paths. A commit is **bookkeeping-only** when every path it changes is the decision log, a
//! verification record (`docs/verification/records/`), or a milestone plan in which only rows of
//! the work-package status table changed.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::pr::Pr;
use super::time;
use super::waits::{Measure, n_of, short};

/// The top-level directories of the product.
pub const PRODUCT_DIRS: &[&str] = &[
    "crates",
    "cpp",
    "include",
    "test-vectors",
    "specs",
    "exerciser",
    "fuzz",
    "refapp",
    "crosscheck",
    "difftest",
];

/// The parts of `docs/` that are process, not specification.
pub const PROCESS_DOCS: &[&str] = &["docs/process/", "docs/verification/", "docs/milestones/"];

/// The decision log.
const DECISIONS: &str = "docs/process/decisions.md";
/// The verification records.
const RECORDS: &str = "docs/verification/records/";
/// The milestone plans.
const PLANS: &str = "docs/milestones/";
/// The header of a milestone plan's work-package status table.
const STATUS_HEADER: [&str; 4] = ["WP", "State", "PRs", "Notes"];

/// The process-only share of the period's verification minutes, in percent, from which the
/// "Recommendations input" lists the process-only pull requests: a third, when the process starts
/// to cost half as much verification as the product.
pub const PROCESS_SHARE_PCT: i64 = 33;
/// A pull request with at least this many bookkeeping-only commits: one is expected, the commit
/// of the decision-log lines just before the final verification run (decision of 2026-10-24).
pub const BOOKKEEPING_MIN: usize = 2;

/// Whether a changed path is product.
pub fn is_product(path: &str) -> bool {
    match path.split_once('/') {
        Some(("docs", _)) => !PROCESS_DOCS.iter().any(|p| path.starts_with(p)),
        Some((top, rest)) => !rest.is_empty() && PRODUCT_DIRS.contains(&top),
        None => false,
    }
}

/// A pull request's or a commit's share of the split.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Only product paths.
    Product,
    /// Only process paths.
    Process,
    /// Both.
    Mixed,
}

impl Class {
    /// Every class, in report order.
    pub const ALL: [Self; 3] = [Self::Process, Self::Product, Self::Mixed];

    /// Its name in the report.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Product => "product-only",
            Self::Process => "process-only",
            Self::Mixed => "mixed",
        }
    }

    /// The class of `paths`, or `None` if there are none.
    pub fn of<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let (mut product, mut process) = (false, false);
        for p in paths {
            if is_product(p) {
                product = true;
            } else {
                process = true;
            }
        }
        match (product, process) {
            (true, false) => Some(Self::Product),
            (false, true) => Some(Self::Process),
            (true, true) => Some(Self::Mixed),
            (false, false) => None,
        }
    }
}

/// A path a commit changes against its parent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    /// The path.
    pub path: String,
    /// For a milestone plan: whether only rows of its work-package status table changed.
    pub status_rows_only: bool,
}

/// Whether `old` and `new`, two versions of a milestone plan, differ only in rows of the
/// work-package status table: every line that `new` lacks is such a row of `old`, and every line
/// that `old` lacks is such a row of `new` (lines compared as a multiset, so a moved line is not
/// a change).
pub fn status_rows_only(old: &str, new: &str) -> bool {
    let rows = |text: &str| -> Vec<usize> {
        crate::md::tables(text)
            .into_iter()
            .filter(|t| t.header == STATUS_HEADER)
            .flat_map(|t| t.rows.into_iter().map(|(line, _)| line))
            .collect()
    };
    let only_rows = |from: &str, to: &str| {
        let mut left: BTreeMap<&str, usize> = BTreeMap::new();
        for line in to.lines() {
            *left.entry(line).or_default() += 1;
        }
        let status = rows(from);
        from.lines()
            .enumerate()
            .all(|(i, line)| match left.get_mut(line).filter(|n| **n > 0) {
                Some(n) => {
                    *n -= 1;
                    true
                }
                None => status.contains(&(i + 1)),
            })
    };
    only_rows(old, new) && only_rows(new, old)
}

/// Whether a commit that changes `changed` only keeps the books.
pub fn is_bookkeeping(changed: &[Changed]) -> bool {
    !changed.is_empty()
        && changed.iter().all(|c| {
            c.path == DECISIONS
                || c.path.starts_with(RECORDS)
                || (c.path.starts_with(PLANS) && c.status_rows_only)
        })
}

/// A commit of a pull request other than a merge, by its changed paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Its ID.
    pub sha: String,
    /// Its class.
    pub class: Class,
    /// Whether it is bookkeeping-only.
    pub bookkeeping: bool,
}

/// The overhead of one pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overhead {
    /// The class of its changed files, `None` if it changed none.
    pub class: Option<Class>,
    /// Its commits other than merges whose changed paths are known, oldest first.
    pub commits: Vec<Commit>,
    /// Its commits other than merges whose changed paths are not known.
    pub unknown: usize,
}

impl Overhead {
    /// The overhead of a pull request whose files are `files` and whose commits other than
    /// merges are `commits`, with the changed paths of those that `changed` has.
    pub fn compute<'a>(
        files: impl IntoIterator<Item = &'a str>,
        commits: &[&str],
        changed: &BTreeMap<String, Vec<Changed>>,
    ) -> Self {
        let mut out = Self {
            class: Class::of(files),
            ..Self::default()
        };
        for sha in commits {
            match changed
                .get(*sha)
                .and_then(|c| Class::of(c.iter().map(|c| c.path.as_str())).map(|class| (class, c)))
            {
                Some((class, c)) => out.commits.push(Commit {
                    sha: (*sha).to_owned(),
                    class,
                    bookkeeping: is_bookkeeping(c),
                }),
                None => out.unknown += 1,
            }
        }
        out
    }

    /// Its bookkeeping-only commits.
    pub fn bookkeeping(&self) -> usize {
        self.commits.iter().filter(|c| c.bookkeeping).count()
    }

    /// The JSON form.
    pub fn to_json(&self) -> Value {
        let mut commits: BTreeMap<&str, usize> = Class::ALL.iter().map(|c| (c.name(), 0)).collect();
        for c in &self.commits {
            *commits.entry(c.class.name()).or_default() += 1;
        }
        json!({
            "class": self.class.map(Class::name),
            "commits": commits,
            "unknown_commits": self.unknown,
            "bookkeeping_commits": self.commits.iter().filter(|c| c.bookkeeping)
                .map(|c| c.sha.as_str()).collect::<Vec<_>>(),
        })
    }
}

/// The overhead of a period's pull requests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Pull requests per class.
    pub prs: BTreeMap<Class, usize>,
    /// Commits other than merges per class.
    pub commits: BTreeMap<Class, usize>,
    /// Commits other than merges whose changed paths are not known.
    pub unknown: usize,
    /// Verification wall time per class of the pull request, in seconds.
    pub verify_s: BTreeMap<Class, i64>,
    /// The process-only pull requests and their verification seconds, most first.
    pub process_only: Vec<(u64, i64)>,
    /// The pull requests with bookkeeping-only commits, and how many.
    pub bookkeeping: Vec<(u64, usize)>,
}

/// The totals of `prs`.
pub fn totals(prs: &[Pr]) -> Totals {
    let mut t = Totals::default();
    for pr in prs {
        let o = &pr.overhead;
        if let Some(class) = o.class {
            *t.prs.entry(class).or_default() += 1;
            *t.verify_s.entry(class).or_default() += pr.verify_seconds();
            if class == Class::Process {
                t.process_only.push((pr.number, pr.verify_seconds()));
            }
        }
        for c in &o.commits {
            *t.commits.entry(c.class).or_default() += 1;
        }
        t.unknown += o.unknown;
        let k = o.bookkeeping();
        if k > 0 {
            t.bookkeeping.push((pr.number, k));
        }
    }
    t.process_only
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    t
}

/// `part` of the values of `of`, as a rounded percentage, or `–` when they sum to 0.
pub fn share(of: &BTreeMap<Class, i64>, part: Class) -> String {
    let total: i64 = of.values().sum();
    let a = of.get(&part).copied().unwrap_or(0);
    (a * 100 + total / 2)
        .checked_div(total)
        .map_or_else(|| "–".to_owned(), |p| format!("{p} %"))
}

/// Counts, for [`share`].
pub fn wide(m: &BTreeMap<Class, usize>) -> BTreeMap<Class, i64> {
    m.iter()
        .map(|(k, v)| (*k, i64::try_from(*v).unwrap_or(i64::MAX)))
        .collect()
}

impl Totals {
    /// Whether the process-only pull requests took at least [`PROCESS_SHARE_PCT`] of the
    /// verification minutes.
    pub fn process_heavy(&self) -> bool {
        let total: i64 = self.verify_s.values().sum();
        let process = self.verify_s.get(&Class::Process).copied().unwrap_or(0);
        total > 0 && process * 100 >= PROCESS_SHARE_PCT * total
    }

    /// The JSON form.
    pub fn to_json(&self) -> Value {
        let by = |m: &dyn Fn(Class) -> Value| -> serde_json::Map<String, Value> {
            Class::ALL
                .iter()
                .map(|c| (c.name().to_owned(), m(*c)))
                .collect()
        };
        let usize_map =
            |m: &BTreeMap<Class, usize>| by(&|c| json!(m.get(&c).copied().unwrap_or(0)));
        json!({
            "prs": usize_map(&self.prs),
            "commits": usize_map(&self.commits),
            "unknown_commits": self.unknown,
            "verify_s": by(&|c| json!(self.verify_s.get(&c).copied().unwrap_or(0))),
            "bookkeeping_commits": self.bookkeeping.iter().map(|(_, k)| k).sum::<usize>(),
            "bookkeeping_by_pr": self.bookkeeping.iter()
                .map(|(n, k)| json!({"pr": n, "commits": k})).collect::<Vec<_>>(),
        })
    }
}

/// The rows of the "Recommendations input" for the overhead: the verification minutes of the
/// process-only pull requests, and the bookkeeping-only commits.
pub fn measures(t: &Totals) -> Vec<Measure> {
    let d = time::duration;
    let total_s: i64 = t.verify_s.values().sum();
    let process_s = t.verify_s.get(&Class::Process).copied().unwrap_or(0);
    let bookkeeping: usize = t.bookkeeping.iter().map(|(_, k)| k).sum();
    vec![
        Measure {
            name: "Verification of process-only PRs",
            threshold: format!("≥ {PROCESS_SHARE_PCT} % of the period's verify minutes"),
            total: format!(
                "{} of {} ({}); {} of PRs, {} of commits process-only",
                d(process_s),
                d(total_s),
                share(&t.verify_s, Class::Process),
                share(&wide(&t.prs), Class::Process),
                share(&wide(&t.commits), Class::Process),
            ),
            crossed: if t.process_heavy() {
                t.process_only
                    .iter()
                    .map(|(n, s)| format!("#{n} ({})", d(*s)))
                    .collect()
            } else {
                Vec::new()
            },
        },
        Measure {
            name: "Bookkeeping-only commits",
            threshold: format!("≥ {BOOKKEEPING_MIN} in one PR"),
            total: format!(
                "{} in {}",
                n_of(bookkeeping, "commit"),
                n_of(t.bookkeeping.len(), "PR")
            ),
            crossed: t
                .bookkeeping
                .iter()
                .filter(|(_, k)| *k >= BOOKKEEPING_MIN)
                .map(|(n, k)| format!("#{n} ({})", n_of(*k, "commit")))
                .collect(),
        },
    ]
}

/// The bookkeeping-only commits of `pr`, as short IDs.
pub fn bookkeeping_shas(pr: &Pr) -> Vec<&str> {
    pr.overhead
        .commits
        .iter()
        .filter(|c| c.bookkeeping)
        .map(|c| short(&c.sha))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-063
    #[test]
    fn paths_are_product_or_process() {
        for path in [
            "crates/aldis-core/src/lib.rs",
            "cpp/include/aldis/core/error.hpp",
            "include/aldis/aldis.h",
            "test-vectors/core/fields.json",
            "specs/manifest.tsv",
            "exerciser/src/main.rs",
            "fuzz/Cargo.toml",
            "refapp/flight/src/main.rs",
            "crosscheck/Cargo.toml",
            "difftest/src/lib.rs",
            "docs/03-api-model.md",
            "docs/profiles/rust.md",
            "docs/standards/tm-frame.md",
            "docs/reference-application.md",
            "docs/glossary.md",
        ] {
            assert!(is_product(path), "{path}");
        }
        for path in [
            "xtask/src/main.rs",
            ".github/workflows/nightly.yml",
            "docs/process/decisions.md",
            "docs/verification/records/VER-063.md",
            "docs/milestones/m2-frame-codecs.md",
            "AGENTS.md",
            "changelog.d/chore-x.md",
            "Cargo.lock",
            ".gitignore",
            ".cargo/config.toml",
            "crates",
        ] {
            assert!(!is_product(path), "{path}");
        }
        assert_eq!(
            Class::of(["crates/a.rs", "docs/01-architecture.md"]),
            Some(Class::Product)
        );
        assert_eq!(Class::of(["xtask/a.rs", "AGENTS.md"]), Some(Class::Process));
        assert_eq!(Class::of(["xtask/a.rs", "crates/a.rs"]), Some(Class::Mixed));
        assert_eq!(Class::of([]), None);
    }

    const PLAN: &str = "# M2\n\n| WP | State | PRs | Notes |\n|---|---|---|---|\n\
                        | WP-1 | open | – | x |\n| WP-2 | open | – | y |\n\n## Decisions\n\nText.\n";

    // req: VER-063
    #[test]
    fn a_plan_change_is_bookkeeping_when_only_status_rows_change() {
        let rows = PLAN.replace("| WP-1 | open | – | x |", "| WP-1 | merged | #9 | x |");
        assert!(status_rows_only(PLAN, &rows));
        let added = PLAN.replace(
            "| WP-2 | open | – | y |",
            "| WP-2 | open | – | y |\n| WP-3 | open | – | z |",
        );
        assert!(status_rows_only(PLAN, &added));
        assert!(status_rows_only(&added, PLAN));
        assert!(status_rows_only(PLAN, PLAN));
        let text = PLAN.replace("Text.", "Other text.");
        assert!(!status_rows_only(PLAN, &text));
        let both = rows.replace("Text.", "Other text.");
        assert!(!status_rows_only(PLAN, &both));
        // A row of another table is not a status row.
        let other = format!("{PLAN}\n| A | B |\n|---|---|\n| 1 | 2 |\n");
        assert!(!status_rows_only(
            &other,
            &other.replace("| 1 | 2 |", "| 1 | 3 |")
        ));
        let changed = |paths: &[(&str, bool)]| -> Vec<Changed> {
            paths
                .iter()
                .map(|(p, s)| Changed {
                    path: (*p).to_owned(),
                    status_rows_only: *s,
                })
                .collect()
        };
        assert!(is_bookkeeping(&changed(&[
            ("docs/process/decisions.md", false),
            ("docs/verification/records/VER-063.md", false),
            ("docs/milestones/m2-frame-codecs.md", true),
        ])));
        assert!(!is_bookkeeping(&changed(&[(
            "docs/milestones/m2-frame-codecs.md",
            false
        )])));
        assert!(!is_bookkeeping(&changed(&[
            ("docs/process/decisions.md", false),
            ("changelog.d/x.md", false),
        ])));
        assert!(!is_bookkeeping(&changed(&[(
            "docs/verification/inspections/x.md",
            false
        )])));
        assert!(!is_bookkeeping(&[]));
    }

    /// The changed paths of a commit; a milestone plan changed only in its status rows.
    fn changed(paths: &[&str]) -> Vec<Changed> {
        paths
            .iter()
            .map(|p| Changed {
                path: (*p).to_owned(),
                status_rows_only: p.starts_with(PLANS),
            })
            .collect()
    }

    /// #70 of the fixture, mixed, with commits a (product) and c (mixed) known and e not; and
    /// #71, process-only, whose commits c and e are bookkeeping-only.
    fn two() -> (Pr, Pr) {
        let mut raw = super::super::pr::tests::fixture();
        raw.commit_files = BTreeMap::from([
            ("a".to_owned(), changed(&["crates/aldis-core/src/lib.rs"])),
            (
                "c".to_owned(),
                changed(&["xtask/src/main.rs", "docs/04-engines.md"]),
            ),
            // A merge is never classified, even with paths.
            ("b".to_owned(), changed(&["xtask/src/main.rs"])),
        ]);
        let mixed = super::super::pr::compute(&raw).unwrap();
        raw.pull["number"] = json!(71);
        raw.files = ["xtask/src/fields.rs", "docs/process/decisions.md"]
            .iter()
            .map(|f| json!({"filename": f}))
            .collect();
        raw.commit_files = BTreeMap::from([
            ("a".to_owned(), changed(&["xtask/src/fields.rs"])),
            ("c".to_owned(), changed(&["docs/process/decisions.md"])),
            (
                "e".to_owned(),
                changed(&[
                    "docs/milestones/m2-frame-codecs.md",
                    "docs/verification/records/VER-063.md",
                ]),
            ),
        ]);
        let process = super::super::pr::compute(&raw).unwrap();
        (mixed, process)
    }

    // req: VER-063
    #[test]
    fn classifies_pull_requests_and_their_commits() {
        let (mixed, process) = two();
        let o = &mixed.overhead;
        assert_eq!(o.class, Some(Class::Mixed));
        let classes: Vec<(&str, Class, bool)> = o
            .commits
            .iter()
            .map(|c| (c.sha.as_str(), c.class, c.bookkeeping))
            .collect();
        assert_eq!(
            classes,
            [("a", Class::Product, false), ("c", Class::Mixed, false)]
        );
        assert_eq!((o.unknown, o.bookkeeping()), (1, 0));
        let o = &process.overhead;
        assert_eq!(o.class, Some(Class::Process));
        assert_eq!((o.commits.len(), o.unknown, o.bookkeeping()), (3, 0, 2));
        let j = o.to_json();
        assert_eq!(j["class"], "process-only");
        assert_eq!(j["commits"]["process-only"], 3);
        assert_eq!(j["bookkeeping_commits"], json!(["c", "e"]));
    }

    // req: VER-063
    #[test]
    fn totals_shares_and_thresholds() {
        let (mixed, process) = two();
        let secs = mixed.verify_seconds();
        assert_eq!(secs, process.verify_seconds());
        let sum = totals(&[mixed.clone(), process.clone()]);
        assert_eq!(
            sum.prs,
            BTreeMap::from([(Class::Process, 1), (Class::Mixed, 1)])
        );
        assert_eq!(
            sum.commits,
            BTreeMap::from([(Class::Product, 1), (Class::Process, 3), (Class::Mixed, 1)])
        );
        assert_eq!(sum.unknown, 1);
        assert_eq!(sum.process_only, [(71, secs)]);
        assert_eq!(sum.bookkeeping, [(71, 2)]);
        assert_eq!(share(&sum.verify_s, Class::Process), "50 %");
        assert_eq!(share(&wide(&sum.commits), Class::Process), "60 %");
        assert!(sum.process_heavy());
        let rows = measures(&sum);
        let dur = time::duration;
        assert_eq!(rows[0].name, "Verification of process-only PRs");
        assert_eq!(rows[0].threshold, "≥ 33 % of the period's verify minutes");
        assert_eq!(
            rows[0].total,
            format!(
                "{} of {} (50 %); 50 % of PRs, 60 % of commits process-only",
                dur(secs),
                dur(2 * secs)
            )
        );
        assert_eq!(rows[0].crossed, [format!("#71 ({})", dur(secs))]);
        assert_eq!(rows[1].name, "Bookkeeping-only commits");
        assert_eq!(
            (rows[1].threshold.as_str(), rows[1].total.as_str()),
            ("≥ 2 in one PR", "2 commits in 1 PR")
        );
        assert_eq!(rows[1].crossed, ["#71 (2 commits)"]);
        let doc = sum.to_json();
        assert_eq!(doc["prs"]["process-only"], 1);
        assert_eq!(doc["verify_s"]["product-only"], 0);
        assert_eq!(doc["bookkeeping_commits"], 2);
        assert_eq!(doc["bookkeeping_by_pr"][0]["pr"], 71);
        // Below the threshold: the process-only pull request ran no verification.
        let mut light = process;
        light.runs.clear();
        light.overhead.commits.truncate(2);
        let sum = totals(&[mixed, light]);
        assert!(!sum.process_heavy());
        let rows = measures(&sum);
        assert!(rows[0].crossed.is_empty(), "{rows:?}");
        assert!(rows[0].total.starts_with("0 s of "), "{}", rows[0].total);
        // One bookkeeping-only commit is expected, not crossed.
        assert_eq!(sum.bookkeeping, [(71, 1)]);
        assert_eq!(rows[1].crossed, Vec::<String>::new());
        // Nothing verified, nothing changed: no share, nothing crossed.
        let none = totals(&[]);
        assert!(!none.process_heavy());
        assert_eq!(share(&none.verify_s, Class::Process), "–");
        assert!(measures(&none).iter().all(|rows| rows.crossed.is_empty()));
    }
}
