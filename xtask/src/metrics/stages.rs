//! Where a pull request's time goes (VER-063): the stages it passed, who it waited on, and the
//! maintainer's active time on the repository.
//!
//! The maintainer and the agents post under the same GitHub account, so whose an event is has to
//! be read from the event: the agent's replies carry [`MARKER`], and the maintainer's own comments
//! are short and plain ([`is_maintainer_comment`]). The maintainer reviews by `COMMENTED` reviews
//! and approves by merging; [`approval`] defines what counts as the approval.

use serde_json::Value;

use super::pr::{MARKER, is_review_comment};

/// The longest comment that counts as the maintainer's own, in characters. The agent's replies
/// that lack the marker are longer (a reply to a review runs to a thousand characters or more).
pub const HUMAN_MAX: usize = 280;

/// The maintainer counts as active on the repository within this many seconds of one of their
/// events (a comment, a review, or a merge), before or after it.
pub const ACTIVE_WINDOW: i64 = 30 * 60;

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Whether a conversation or inline comment is the maintainer's own: posted by the maintainer's
/// account, not a reply of the agent (no [`MARKER`]), not a review ([`is_review_comment`]), at
/// most [`HUMAN_MAX`] characters, and without bold text, with which the agent's unmarked replies
/// open (`**Stacked on #98** …`).
pub fn is_maintainer_comment(comment: &Value, maintainer: &str) -> bool {
    let body = str_of(comment, "body").trim();
    let login = comment.get("user").map_or("", |u| str_of(u, "login"));
    login == maintainer
        && !body.is_empty()
        && !body.contains(MARKER)
        && !body.contains("**")
        && body.chars().count() <= HUMAN_MAX
        && !is_review_comment(comment)
}

/// How a pull request's approval was read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApprovalKind {
    /// A review in the state `APPROVED`.
    Review,
    /// The maintainer's last word: their last review, comment, or recorded decision, after which
    /// no commit but merges was authored.
    LastWord,
    /// The merge itself: the maintainer's last word was followed by more work, or there was none.
    #[default]
    Merge,
}

impl ApprovalKind {
    /// The name in the report.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::LastWord => "last word",
            Self::Merge => "merge",
        }
    }
}

/// When a pull request was approved, and how that was read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Approval {
    /// The time.
    pub at: i64,
    /// The rule that gave it.
    pub kind: ApprovalKind,
}

/// The approval of a pull request merged at `merged`: the first `APPROVED` review if there is one;
/// otherwise the maintainer's last word (the latest of `words`, their reviews, comments, and
/// recorded decisions, up to the merge) if no commit other than a merge was authored after it
/// (`work` holds the author dates of those commits); otherwise the merge.
pub fn approval(approved: Option<i64>, words: &[i64], work: &[i64], merged: i64) -> Approval {
    if let Some(at) = approved.filter(|t| *t <= merged) {
        return Approval {
            at,
            kind: ApprovalKind::Review,
        };
    }
    match words.iter().copied().filter(|t| *t <= merged).max() {
        Some(last) if work.iter().all(|t| *t <= last) => Approval {
            at: last,
            kind: ApprovalKind::LastWord,
        },
        _ => Approval {
            at: merged,
            kind: ApprovalKind::Merge,
        },
    }
}

/// The maintainer's active time: the union of the windows of [`ACTIVE_WINDOW`] around each of
/// their events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// Disjoint spans `[from, to)`, in order.
    spans: Vec<(i64, i64)>,
}

impl Activity {
    /// The active time around `events`, each widened by `window` on both sides.
    pub fn new(events: &[i64], window: i64) -> Self {
        let mut sorted = events.to_vec();
        sorted.sort_unstable();
        let mut spans: Vec<(i64, i64)> = Vec::new();
        for e in sorted {
            let (from, to) = (e - window, e + window);
            match spans.last_mut() {
                Some(last) if from <= last.1 => last.1 = last.1.max(to),
                _ => spans.push((from, to)),
            }
        }
        Self { spans }
    }

    /// The seconds of `[from, to)` in which the maintainer was active; 0 for an empty interval.
    pub fn active(&self, from: i64, to: i64) -> i64 {
        self.spans
            .iter()
            .map(|(a, b)| (to.min(*b) - from.max(*a)).max(0))
            .sum()
    }
}

/// The stages a merged pull request passed, as Unix times.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stages {
    /// The earliest author date of its commits.
    pub first_commit: i64,
    /// Opened.
    pub opened: i64,
    /// The first passing record of any of its commits (`finished`).
    pub first_pass: Option<i64>,
    /// The first review.
    pub first_review: Option<i64>,
    /// The last review up to the merge.
    pub last_review: Option<i64>,
    /// The first maintainer decision on it: a comment of theirs in its conversation, or a commit of
    /// it that recorded a decision-log line naming it.
    pub first_decision: Option<i64>,
    /// The last one, up to the merge.
    pub last_decision: Option<i64>,
    /// The approval.
    pub approval: Approval,
    /// The passing record of the merged head (`finished`).
    pub final_pass: Option<i64>,
    /// Merged.
    pub merged: i64,
}

/// The stages of the timeline, in order.
pub const NAMES: [&str; 8] = [
    "first commit",
    "opened",
    "first pass",
    "first review",
    "last decision",
    "approval",
    "final pass",
    "merged",
];

/// The named intervals the report gives medians of: their labels and the stages they span.
pub const INTERVALS: [(&str, &str, &str); 8] = [
    ("first commit", "opened", "work before opening"),
    ("opened", "first pass", "to the first passing record"),
    ("opened", "first review", "waiting for the first review"),
    (
        "first review",
        "last decision",
        "first review to the last decision",
    ),
    ("last decision", "approval", "last decision to the approval"),
    (
        "approval",
        "final pass",
        "approval to the final passing record",
    ),
    ("final pass", "merged", "final passing record to the merge"),
    ("opened", "merged", "opened to the merge"),
];

/// The interval whose raw figure is the maintainer's availability, not a process delay.
pub const REVIEW_WAIT: &str = "waiting for the first review";

impl Stages {
    /// Each stage of [`NAMES`] and its time, if it happened. An approval by merge is the merge and
    /// not a stage of its own.
    pub fn points(&self) -> [(&'static str, Option<i64>); 8] {
        let approval = (self.approval.kind != ApprovalKind::Merge).then_some(self.approval.at);
        let times = [
            Some(self.first_commit),
            Some(self.opened),
            self.first_pass,
            self.first_review,
            self.last_decision,
            approval,
            self.final_pass,
            Some(self.merged),
        ];
        let mut out = [("", None); 8];
        for ((slot, name), t) in out.iter_mut().zip(NAMES).zip(times) {
            *slot = (name, t);
        }
        out
    }

    /// The time of a stage of [`NAMES`].
    pub fn at(&self, name: &str) -> Option<i64> {
        self.points()
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, t)| *t)
    }

    /// The interval between two stages, if both happened and in that order.
    pub fn between(&self, from: &str, to: &str) -> Option<(i64, i64)> {
        let (a, b) = (self.at(from)?, self.at(to)?);
        (b >= a).then_some((a, b))
    }

    /// For each stage that happened after the first, the seconds since the stage before it that
    /// happened; negative when the two came in the other order (a review before the first passing
    /// record).
    pub fn gaps(&self) -> Vec<(&'static str, i64)> {
        let points = self.points();
        let mut out = Vec::new();
        let mut last: Option<i64> = None;
        for (name, t) in points {
            let Some(t) = t else { continue };
            if let Some(prev) = last {
                out.push((name, t - prev));
            }
            last = Some(t);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // req: VER-063
    #[test]
    fn tells_the_maintainers_comments_from_the_agents() {
        let c = |login: &str, body: &str| json!({"user": {"login": login}, "body": body});
        assert!(is_maintainer_comment(
            &c("me", "agree with recommendations on Q1-Q4"),
            "me"
        ));
        assert!(is_maintainer_comment(
            &c("me", "Q1: agree\nQ2: approve as built"),
            "me"
        ));
        // Another account, the agent's marker, bold text, a review, too long, or empty: not.
        assert!(!is_maintainer_comment(&c("you", "agree"), "me"));
        assert!(!is_maintainer_comment(
            &c("me", &format!("Done.\n{MARKER}")),
            "me"
        ));
        assert!(!is_maintainer_comment(
            &c("me", "**Stacked on #98**: wait"),
            "me"
        ));
        assert!(!is_maintainer_comment(&c("me", "## Review\nok"), "me"));
        assert!(!is_maintainer_comment(&c("me", &"x".repeat(281)), "me"));
        assert!(is_maintainer_comment(&c("me", &"é".repeat(280)), "me"));
        assert!(!is_maintainer_comment(&c("me", "  "), "me"));
    }

    // req: VER-063
    #[test]
    fn approval_is_a_review_the_last_word_or_the_merge() {
        // An APPROVED review wins, even with work after it.
        let a = approval(Some(50), &[40, 60], &[70], 100);
        assert_eq!((a.at, a.kind), (50, ApprovalKind::Review));
        // An approval after the merge does not count.
        let a = approval(Some(150), &[], &[], 100);
        assert_eq!((a.at, a.kind), (100, ApprovalKind::Merge));
        // The last word, when no work followed it (work at the same second does not count).
        let a = approval(None, &[40, 60, 120], &[30, 60], 100);
        assert_eq!((a.at, a.kind), (60, ApprovalKind::LastWord));
        // Work after the last word: approved by merging.
        let a = approval(None, &[40, 60], &[61], 100);
        assert_eq!((a.at, a.kind), (100, ApprovalKind::Merge));
        // No word at all.
        assert_eq!(approval(None, &[], &[10], 100).kind, ApprovalKind::Merge);
        assert_eq!(ApprovalKind::LastWord.name(), "last word");
        assert_eq!(ApprovalKind::Review.name(), "review");
        assert_eq!(ApprovalKind::Merge.name(), "merge");
    }

    // req: VER-063
    #[test]
    fn active_time_is_the_union_of_windows_around_events() {
        // Events at 1000, 1500 (overlapping windows of 300) and 5000.
        let act = Activity::new(&[5000, 1000, 1500], 300);
        assert_eq!(act.spans, [(700, 1800), (4700, 5300)]);
        // An overnight wait from 1600 to 5100: active only 1600..1800 and 4700..5100.
        assert_eq!(act.active(1600, 5100), 200 + 400);
        assert_eq!(act.active(0, 10_000), 1100 + 600);
        assert_eq!(act.active(2000, 4000), 0);
        assert_eq!(act.active(5100, 1600), 0);
        assert_eq!(Activity::new(&[], 300).active(0, 100), 0);
        // A window that ends inside a later one extends it.
        assert_eq!(Activity::new(&[0, 100, 50], 100).spans, [(-100, 200)]);
    }

    // req: VER-063
    #[test]
    fn stages_points_gaps_and_intervals() {
        let mut s = Stages {
            first_commit: 0,
            opened: 10,
            first_pass: Some(40),
            first_review: Some(30),
            last_review: Some(30),
            first_decision: Some(50),
            last_decision: Some(60),
            approval: Approval {
                at: 60,
                kind: ApprovalKind::LastWord,
            },
            final_pass: Some(90),
            merged: 100,
        };
        assert_eq!(
            s.gaps(),
            [
                ("opened", 10),
                ("first pass", 30),
                ("first review", -10),
                ("last decision", 30),
                ("approval", 0),
                ("final pass", 30),
                ("merged", 10)
            ]
        );
        assert_eq!(s.between("opened", "first review"), Some((10, 30)));
        assert_eq!(s.between("first pass", "first review"), None);
        assert_eq!(s.between("approval", "final pass"), Some((60, 90)));
        // An approval by merge is no stage of its own.
        s.approval.kind = ApprovalKind::Merge;
        s.first_pass = None;
        assert_eq!(s.at("approval"), None);
        assert_eq!(s.at("first pass"), None);
        assert_eq!(s.at("nonsense"), None);
        assert_eq!(s.gaps().len(), 5);
    }
}
