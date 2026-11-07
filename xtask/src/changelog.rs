//! `xtask changelog`: the unreleased section of `CHANGELOG.md`, generated from the titles of the
//! merged pull requests (GEN-080, OI-63).
//!
//! A pull request does not touch `CHANGELOG.md`: its title, a conventional commit
//! (`type(scope)!: description`), is the entry. The merge commits of `main` carry the titles
//! (`Merge pull request #N from ...`, the title as the body), so the generator reads `git log`
//! and needs no network, except for a hand-built merge train (below). It lists the merges after
//! a base: the commit named by the marker `<!-- changelog-base: <sha> -->` in the
//! `## [Unreleased]` section of `CHANGELOG.md`, else the last `v*` tag. A merge train (a pull
//! request from a `train/` branch) gets no entry of its own: the pull requests it merged do, read
//! from the merges between the first and the second parent of its merge commit (`Merge pull
//! request #N from ...`, as `scripts/train` writes them). For a hand-built train, `Merge branch
//! '<b>' into train/...` is resolved to its pull request by the second parent of the merge, the
//! head of the pull request: `gh pr list --state merged --search <sha>` keeps the result whose
//! head is that commit and not a `train/*` branch; a failure is a warning naming the merge. A
//! merge of `main` into the train is ignored. The text of the Unreleased section is kept in the
//! release as it stands.

use std::fmt::Write as _;
use std::process::Command;

use crate::repo;

/// Keep a Changelog sections, in release order.
const SECTIONS: [&str; 7] = [
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Deviations fixed",
    "Security",
];

/// Index of a section in [`SECTIONS`].
const ADDED: usize = 0;
const CHANGED: usize = 1;
const FIXED: usize = 4;
const DEVIATIONS: usize = 5;
const SECURITY: usize = 6;

/// The scope reserved for fixes of deviations from a CCSDS Blue Book (GEN-080).
const BLUE_BOOK: &str = "blue-book";

/// Types that are not user-visible and so are not listed (unless breaking).
const UNLISTED: [&str; 6] = ["docs", "chore", "test", "ci", "build", "style"];

/// The marker that names the commit after which merges are listed.
const MARKER_OPEN: &str = "<!-- changelog-base: ";

/// A pull request title in conventional-commit form.
#[derive(Debug, PartialEq, Eq)]
struct Title<'a> {
    kind: &'a str,
    scope: Option<&'a str>,
    breaking: bool,
    description: &'a str,
}

/// Parses `type(scope)!: description`; `None` when the title does not follow the form.
fn parse_title(title: &str) -> Option<Title<'_>> {
    let (head, description) = title.split_once(": ")?;
    let description = description.trim();
    let (head, breaking) = head.strip_suffix('!').map_or((head, false), |h| (h, true));
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => (kind, Some(rest.strip_suffix(')')?)),
        None => (head, None),
    };
    let kind_ok = !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase());
    let scope_ok = scope.is_none_or(|s| !s.is_empty() && !s.contains(['(', ')']));
    (kind_ok && scope_ok && !description.is_empty()).then_some(Title {
        kind,
        scope,
        breaking,
        description,
    })
}

/// The section a title belongs to; `None` for a type that is neither listed nor in [`UNLISTED`].
/// A fix with the scope [`BLUE_BOOK`] is a fix of a deviation (GEN-080).
fn section(t: &Title<'_>) -> Option<usize> {
    match t.kind {
        "fix" if t.scope == Some(BLUE_BOOK) => Some(DEVIATIONS),
        _ if t.breaking => Some(CHANGED),
        "feat" => Some(ADDED),
        "fix" => Some(FIXED),
        "security" => Some(SECURITY),
        "perf" | "refactor" | "revert" => Some(CHANGED),
        _ => None,
    }
}

/// A merged pull request: its number and title.
#[derive(Debug, PartialEq, Eq)]
struct Merge {
    number: u64,
    title: String,
}

/// The merge commits of a log: the pull requests, and one description per merge commit that is
/// not usable (no `Merge pull request #N` subject, or an empty body).
struct Merges {
    prs: Vec<Merge>,
    skipped: Vec<String>,
    /// Merge commits in the log, skipped or not.
    total: usize,
}

/// Reads the merges from `git log --first-parent --merges --format=%s%x1f%b%x1e` output, oldest
/// first.
fn parse_merges(log: &str) -> Merges {
    let mut out = Merges {
        prs: Vec::new(),
        skipped: Vec::new(),
        total: 0,
    };
    for record in log.split('\x1e').map(str::trim).filter(|r| !r.is_empty()) {
        out.total += 1;
        let (subject, body) = record.split_once('\x1f').unwrap_or((record, ""));
        let number = subject
            .strip_prefix("Merge pull request #")
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok());
        let title = body.lines().map(str::trim).find(|l| !l.is_empty());
        match (number, title) {
            (Some(number), Some(title)) => out.prs.push(Merge {
                number,
                title: title.to_owned(),
            }),
            (Some(number), None) => out
                .skipped
                .push(format!("#{number} has no title in its merge commit")),
            (None, _) => out.skipped.push(format!("not a pull request: {subject}")),
        }
    }
    out.prs.reverse();
    out
}

/// Whether the subject is that of a train's merge commit: `Merge pull request #N from
/// <owner>/train/<name>` (or `from train/<name>`), not any branch with `train` inside its name.
fn is_train_merge(subject: &str) -> bool {
    let Some((_, from)) = subject
        .strip_prefix("Merge pull request #")
        .and_then(|r| r.split_once(" from "))
    else {
        return false;
    };
    let from = from.trim();
    from.starts_with("train/")
        || from
            .split_once('/')
            .is_some_and(|(_, branch)| branch.starts_with("train/"))
}

/// What a merge into a train branch is, when it is no `Merge pull request` merge.
enum TrainMerge<'a> {
    /// `Merge branch '<b>' into train/...`: a pull request's branch merged by hand.
    Branch(&'a str),
    /// A merge of `main` into the train: a refresh, ignored.
    Refresh,
}

/// Reads `Merge branch '<b>' into train/...` (also `Merge remote-tracking branch`).
fn train_branch_merge(subject: &str) -> Option<TrainMerge<'_>> {
    let rest = subject
        .strip_prefix("Merge branch '")
        .or_else(|| subject.strip_prefix("Merge remote-tracking branch '"))?;
    let (branch, _) = rest.split_once("' into train/")?;
    Some(if branch == "main" || branch == "origin/main" {
        TrainMerge::Refresh
    } else {
        TrainMerge::Branch(branch)
    })
}

/// Turns the `%H%x1f%s%x1f%b%x1e` log of `main` into the `%s%x1f%b%x1e` log that `parse_merges`
/// reads. A merge train (`docs/process/coordinator.md`) is a pull request from a `train/`
/// branch: its own merge commit is dropped and the merge commits of its constituents, which `sub`
/// reads (as `%P%x1f%s%x1f%b%x1e`) for the hash of the train's merge commit, take its place. A
/// constituent merged by hand (`Merge branch '<b>' into train/...`) is resolved by `lookup`, given
/// the merge's second parent (the head of the pull request) and the branch, to the number and
/// title of the merged pull request; one that `lookup` cannot resolve stays and is reported as
/// skipped, naming the train; a merge of `main` into the train is dropped.
fn expand_trains(
    log: &str,
    mut sub: impl FnMut(&str) -> Result<String, String>,
    mut lookup: impl FnMut(&str, &str) -> Option<(u64, String)>,
) -> Result<String, String> {
    let mut out = String::new();
    for record in log.split('\x1e').map(str::trim).filter(|r| !r.is_empty()) {
        let (hash, rest) = record.split_once('\x1f').unwrap_or(("", record));
        let subject = rest.split('\x1f').next().unwrap_or_default();
        if !is_train_merge(subject) {
            out.push_str(rest);
            out.push('\x1e');
            continue;
        }
        let members = sub(hash)?;
        for member in members
            .split('\x1e')
            .map(str::trim)
            .filter(|r| !r.is_empty())
        {
            let (parents, member) = member.split_once('\x1f').unwrap_or(("", member));
            let member_subject = member.split('\x1f').next().unwrap_or_default();
            let resolved = match train_branch_merge(member_subject) {
                Some(TrainMerge::Refresh) => continue,
                Some(TrainMerge::Branch(branch)) => parents
                    .split_whitespace()
                    .nth(1)
                    .and_then(|sha| lookup(sha, branch))
                    .map(|r| (branch, r)),
                None => None,
            };
            if let Some((branch, (number, title))) = resolved {
                let _ = write!(
                    out,
                    "Merge pull request #{number} from gh/{branch}\x1f{title}\x1e"
                );
            } else {
                out.push_str(member);
                out.push('\x1e');
            }
        }
    }
    Ok(out)
}

/// The constituent among `gh pr list --state merged --search <sha>` results, one pull request per
/// line as `number<TAB>title<TAB>headRefOid<TAB>headRefName`: the one whose head is exactly `sha`
/// and whose head branch is no `train/*` (the search also finds the train pull request that
/// merged the commit, and a commit already on `main` belongs to the train in
/// `commits/<sha>/pulls`).
fn constituent_of(sha: &str, results: &str) -> Option<(u64, String)> {
    results.lines().find_map(|line| {
        let mut f = line.split('\t');
        let (number, title, head, branch) = (f.next()?, f.next()?, f.next()?, f.next()?);
        (head == sha && !branch.starts_with("train/"))
            .then(|| Some((number.parse().ok()?, title.to_owned())))
            .flatten()
    })
}

/// The number and title of the merged pull request whose head commit is `sha` (the second parent
/// of the merge of `branch` into a train), from `gh pr list --state merged --search <sha>`. A
/// `gh` failure, or no match, prints a warning naming the merge and gives `None`.
fn merged_pull_request(sha: &str, branch: &str) -> Option<(u64, String)> {
    let out = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "merged",
            "--search",
            sha,
            "--json",
            "number,title,headRefOid,headRefName",
            "--jq",
            r#".[] | [.number, .title, .headRefOid, .headRefName] | @tsv"#,
        ])
        .output();
    let found = match out {
        Ok(o) if o.status.success() => constituent_of(sha, &String::from_utf8_lossy(&o.stdout)),
        _ => {
            println!("warning: gh could not look up the merge of '{branch}' ({sha}) into a train");
            return None;
        }
    };
    if found.is_none() {
        println!(
            "warning: no merged pull request has the head {sha} (merge of '{branch}' into a train)"
        );
    }
    found
}

/// The generated entries, by section, and the titles that do not follow the form.
struct Generated {
    sections: [Vec<String>; SECTIONS.len()],
    malformed: Vec<String>,
}

/// Maps the merges to changelog entries. A breaking title starts its entry with **Breaking:**.
/// A title that is not a conventional commit is listed under Changed and reported.
fn generate(merges: &[Merge]) -> Generated {
    let mut g = Generated {
        sections: Default::default(),
        malformed: Vec::new(),
    };
    for m in merges {
        let (i, line) = if let Some(t) = parse_title(&m.title) {
            let Some(i) = section(&t) else {
                if !UNLISTED.contains(&t.kind) {
                    g.malformed.push(format!(
                        "#{}: unknown type `{}`: {}",
                        m.number, t.kind, m.title
                    ));
                    g.sections[CHANGED].push(format!("- {} (#{})", m.title, m.number));
                }
                continue;
            };
            let mut line = String::from("- ");
            if t.breaking {
                line.push_str("**Breaking:** ");
            }
            // A milestone scope (`m3/wp-4`) names the plan, and `blue-book` the section: neither
            // is printed.
            if let Some(scope) = t
                .scope
                .filter(|s| !s.contains('/') && (*s != BLUE_BOOK || t.kind != "fix"))
            {
                let _ = write!(line, "{scope}: ");
            }
            let _ = write!(line, "{} (#{})", t.description, m.number);
            (i, line)
        } else {
            g.malformed.push(format!("#{}: {}", m.number, m.title));
            (CHANGED, format!("- {} (#{})", m.title, m.number))
        };
        g.sections[i].push(line);
    }
    g
}

/// Renders the sections in `SECTIONS` order, each followed by a blank line.
fn render(sections: &[Vec<String>; SECTIONS.len()]) -> String {
    let mut out = String::new();
    for (name, lines) in SECTIONS.iter().zip(sections) {
        if !lines.is_empty() {
            let _ = write!(out, "### {name}\n{}\n\n", lines.join("\n"));
        }
    }
    out
}

/// The span of the `## [Unreleased]` section: from the end of its heading line to the start of
/// the next `## [` heading (or the end).
fn unreleased_span(changelog: &str) -> Result<(usize, usize), String> {
    let start = changelog
        .find("## [Unreleased]\n")
        .ok_or("CHANGELOG.md: no `## [Unreleased]` section")?;
    let after = start + "## [Unreleased]\n".len();
    let end = changelog[after..]
        .find("\n## [")
        .map_or(changelog.len(), |i| after + i + 1);
    Ok((after, end))
}

/// The commit named by the marker in the Unreleased text, if any.
fn marker(unreleased: &str) -> Option<&str> {
    let rest = &unreleased[unreleased.find(MARKER_OPEN)? + MARKER_OPEN.len()..];
    Some(rest[..rest.find(" -->")?].trim())
}

/// The Unreleased text without the marker line.
fn without_marker(unreleased: &str) -> String {
    unreleased
        .lines()
        .filter(|l| !l.contains(MARKER_OPEN))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds the release section: the heading, the hand-kept text of the Unreleased section (if
/// any), and the generated entries. The Unreleased section becomes empty again, with the marker
/// set to `head`.
fn insert_release(
    changelog: &str,
    version: &str,
    date: &str,
    generated: &str,
    head: &str,
) -> Result<String, String> {
    if changelog.contains(&format!("## [{version}]")) {
        return Err(format!("CHANGELOG.md already has a section for {version}"));
    }
    let (after, end) = unreleased_span(changelog)?;
    let kept = without_marker(&changelog[after..end]);
    let kept = kept.trim();
    let mut body = String::new();
    if !kept.is_empty() {
        let _ = write!(body, "{kept}\n\n");
    }
    body.push_str(generated);
    Ok(format!(
        "{}\n{MARKER_OPEN}{head} -->\n\n## [{version}] — {date}\n\n{body}{}",
        &changelog[..after],
        &changelog[end..]
    ))
}

/// Released versions that predate the rule that a release has a tag, each with the reason.
const UNTAGGED_EXEMPT: &[(&str, &str)] = &[(
    "0.1.0-spec",
    "a spec-only release of 2026-10-16, from before tags were made",
)];

/// The versions of the `## [x.y.z]` headings of `changelog`, other than `[Unreleased]`.
fn released_versions(changelog: &str) -> Vec<&str> {
    changelog
        .lines()
        .filter_map(|l| l.strip_prefix("## [")?.split_once(']'))
        .map(|(version, _)| version)
        .filter(|v| *v != "Unreleased")
        .collect()
}

/// The tags `v<version>` that a released version lacks in `tags`, apart from the versions of
/// [`UNTAGGED_EXEMPT`].
fn untagged_releases(changelog: &str, tags: &[&str]) -> Vec<String> {
    released_versions(changelog)
        .into_iter()
        .filter(|v| !UNTAGGED_EXEMPT.iter().any(|(e, _)| e == v))
        .map(|v| format!("v{v}"))
        .filter(|t| !tags.contains(&t.as_str()))
        .collect()
}

/// Runs git in the repository; returns its standard output.
fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The merged pull requests after the base, and the head commit.
fn merges_since_base() -> Result<(Merges, String, String), String> {
    let changelog = repo::read(&repo::root().join("CHANGELOG.md"))?;
    let (after, end) = unreleased_span(&changelog)?;
    let base = match marker(&changelog[after..end]) {
        Some(sha) => sha.to_owned(),
        None => git(&["describe", "--tags", "--abbrev=0", "--match", "v*"])
            .map_err(|e| format!("no marker in CHANGELOG.md and no release tag: {e}"))?
            .trim()
            .to_owned(),
    };
    let head = git(&["rev-parse", "HEAD"])?.trim().to_owned();
    let log = git(&[
        "log",
        "--first-parent",
        "--merges",
        "--format=%H%x1f%s%x1f%b%x1e",
        &format!("{base}..HEAD"),
    ])?;
    let log = expand_trains(
        &log,
        |hash| {
            git(&[
                "log",
                "--first-parent",
                "--merges",
                "--format=%P%x1f%s%x1f%b%x1e",
                &format!("{hash}^1..{hash}^2"),
            ])
        },
        merged_pull_request,
    )?;
    Ok((parse_merges(&log), base, head))
}

/// Prints the count of merge commits before anything is skipped, then a warning for each merge
/// commit that could not be used and for each title that is not a known conventional commit.
fn report(merges: &Merges, g: &Generated, base: &str) {
    println!(
        "changelog: {} merge commit(s) after {base}: {} pull request(s), {} skipped",
        merges.total,
        merges.prs.len(),
        merges.skipped.len()
    );
    for m in &merges.skipped {
        println!("warning: merge commit skipped, {m}");
    }
    for m in &g.malformed {
        println!(
            "warning: listed under Changed, not `type(scope): description` with a known type: {m}"
        );
    }
}

/// `xtask changelog`: prints the entries the next release will list, from the merges after the
/// base. Titles that are not conventional commits are reported after the entries.
pub fn show() -> Result<usize, String> {
    let (merges, base, _) = merges_since_base()?;
    let g = generate(&merges.prs);
    print!("{}", render(&g.sections));
    report(&merges, &g, &base);
    Ok(0)
}

/// `xtask changelog release <version> <date>`: writes the release section of `CHANGELOG.md`.
pub fn release(version: &str, date: &str) -> Result<usize, String> {
    let date_ok = date.len() == 10
        && date.char_indices().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
    if version.is_empty() || version.contains(char::is_whitespace) || !date_ok {
        return Err("usage: cargo xtask changelog release <version> <YYYY-MM-DD>".to_owned());
    }
    let path = repo::root().join("CHANGELOG.md");
    // Before a new section: every version already released has its tag. The tag of the section
    // written here is made after the release pull request merges, so it is not asked for.
    let tags = git(&["tag", "--list", "v*"])?;
    let missing = untagged_releases(&repo::read(&path)?, &tags.lines().collect::<Vec<_>>());
    if !missing.is_empty() {
        return Err(format!(
            "CHANGELOG.md has released versions without a git tag in this clone: {}; run \
             `git fetch --tags origin` first, and if they are still missing ask the maintainer \
             to tag them (tags are the maintainer's) before the next release",
            missing.join(", ")
        ));
    }
    let (merges, base, head) = merges_since_base()?;
    let g = generate(&merges.prs);
    let updated = insert_release(
        &repo::read(&path)?,
        version,
        date,
        &render(&g.sections),
        &head,
    )?;
    std::fs::write(&path, updated).map_err(|e| format!("CHANGELOG.md: {e}"))?;
    report(&merges, &g, &base);
    println!(
        "changelog: released as {version}; add a summary line under the new heading if wanted"
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{
        Merge, constituent_of, expand_trains, generate, insert_release, is_train_merge, marker,
        parse_merges, parse_title, render, untagged_releases,
    };

    fn merge(number: u64, title: &str) -> Merge {
        Merge {
            number,
            title: title.to_owned(),
        }
    }

    #[test]
    fn parses_conventional_titles() {
        let t = parse_title("feat(tm)!: new codec").unwrap();
        assert_eq!(
            (t.kind, t.scope, t.breaking, t.description),
            ("feat", Some("tm"), true, "new codec")
        );
        let t = parse_title("fix: wrap the count").unwrap();
        assert_eq!((t.kind, t.scope, t.breaking), ("fix", None, false));
        for bad in [
            "Update stuff",
            "feat: ",
            "Feat: x",
            "feat(: x",
            "feat(): x",
            ": x",
        ] {
            assert!(parse_title(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn maps_types_to_sections() {
        let g = generate(&[
            merge(1, "feat(spp): add a codec"),
            merge(2, "fix: wrap the count"),
            merge(3, "chore: tidy"),
            merge(4, "docs: words"),
            merge(5, "refactor(ffi)!: rename a call"),
            merge(6, "perf: faster"),
            merge(7, "security: close a hole"),
            merge(8, "no form"),
            merge(9, "feat(m3/wp-4): implement FOP-1"),
            merge(10, "feature(tm): add X"),
            merge(11, "fix(blue-book): wrap the count"),
            merge(12, "fix(blue-book)!: change the field"),
            merge(13, "style: fmt"),
        ]);
        assert_eq!(
            render(&g.sections),
            "### Added\n- spp: add a codec (#1)\n- implement FOP-1 (#9)\n\n\
             ### Changed\n- **Breaking:** ffi: rename a call (#5)\n- faster (#6)\n- no form (#8)\n- feature(tm): add X (#10)\n\n\
             ### Fixed\n- wrap the count (#2)\n\n\
             ### Deviations fixed\n- wrap the count (#11)\n- **Breaking:** change the field (#12)\n\n\
             ### Security\n- close a hole (#7)\n\n"
        );
        assert_eq!(
            g.malformed,
            [
                "#8: no form",
                "#10: unknown type `feature`: feature(tm): add X"
            ]
        );
    }

    #[test]
    fn reads_merge_commits_oldest_first_and_reports_what_it_skips() {
        let log = "Merge pull request #3 from a/b\x1ffix: later\n\n\x1e\
                   Merge pull request #2 from a/c\x1f\nfeat: earlier\n\nMore.\x1e\
                   Merge origin/main into x\x1f\x1e\
                   Merge pull request #1 from a/d\x1f\x1e";
        let m = parse_merges(log);
        assert_eq!(m.prs, [merge(2, "feat: earlier"), merge(3, "fix: later")]);
        assert_eq!(m.total, 4);
        assert_eq!(
            m.skipped,
            [
                "not a pull request: Merge origin/main into x",
                "#1 has no title in its merge commit"
            ]
        );
    }

    #[test]
    fn a_train_is_replaced_by_its_pull_requests() {
        let log = "h3\x1fMerge pull request #9 from a/train/2026-11-07-1\x1fchore(train): x\x1e\
                   h2\x1fMerge pull request #2 from a/c\x1ffeat: solo\x1e";
        let out = expand_trains(
            log,
            |h| {
                assert_eq!(h, "h3");
                Ok("p1 p2\x1fMerge pull request #5 from a/e\x1ffix: b\x1e\
                    p3 p4\x1fMerge pull request #4 from a/f\x1ffeat: a\x1e"
                    .to_owned())
            },
            |_, _| None,
        )
        .unwrap();
        let m = parse_merges(&out);
        assert_eq!(
            m.prs,
            [
                merge(2, "feat: solo"),
                merge(4, "feat: a"),
                merge(5, "fix: b")
            ]
        );
        assert_eq!(m.total, 3);
        assert!(expand_trains(log, |_| Err("no".to_owned()), |_, _| None).is_err());
    }

    #[test]
    fn a_hand_built_train_resolves_its_branch_merges() {
        // The shape of #329: `Merge branch '<b>' into train/...`, no body, and a refresh of a
        // constituent from main inside the train, which is ignored without a warning.
        let log = "h9\x1fMerge pull request #329 from kscharpf/train/2026-11-07-1\x1fchore: t\x1e";
        let out = expand_trains(
            log,
            |_| {
                Ok("a1 b1\x1fMerge branch 'm5/time' into train/2026-11-07-1\x1f\x1e\
                    a2 b2\x1fMerge branch 'main' into train/2026-11-07-1\x1f\x1e\
                    a3 b3\x1fMerge remote-tracking branch 'origin/main' into train/2026-11-07-1\x1f\x1e\
                    a4 b4\x1fMerge branch 'a1/lost' into train/2026-11-07-1\x1f\x1e\
                    a5 b5\x1fMerge branch 'a1/wp-5' into train/2026-11-07-1\x1f\x1e"
                    .to_owned())
            },
            |sha, _| match sha {
                "b1" => Some((322, "refactor(m5): time".to_owned())),
                "b5" => Some((323, "feat(a1/wp-5): golden".to_owned())),
                _ => None,
            },
        )
        .unwrap();
        let m = parse_merges(&out);
        assert_eq!(
            m.prs,
            [
                merge(323, "feat(a1/wp-5): golden"),
                merge(322, "refactor(m5): time")
            ]
        );
        assert_eq!(
            m.skipped,
            ["not a pull request: Merge branch 'a1/lost' into train/2026-11-07-1"]
        );
    }

    #[test]
    fn the_constituent_is_chosen_and_not_the_train() {
        // The real shape of `gh pr list --state merged --search <sha>`: the constituent and the
        // train that merged it, the train first.
        let sha = "8e9f3414";
        let results = "329\tchore(train): 2026-11-07-1\t44ab7a1b\ttrain/2026-11-07-1\n\
                       322\trefactor(m5): time\t8e9f3414\tm5/time-cuc-instant\n";
        assert_eq!(
            constituent_of(sha, results),
            Some((322, "refactor(m5): time".to_owned()))
        );
        // A train whose head happens to be the commit, or no result with that head: none.
        assert_eq!(constituent_of("44ab7a1b", results), None);
        assert_eq!(constituent_of("deadbeef", results), None);
        assert_eq!(constituent_of(sha, ""), None);
    }

    #[test]
    fn only_a_train_branch_is_a_train() {
        assert!(is_train_merge(
            "Merge pull request #9 from kscharpf/train/2026-11-07-1"
        ));
        assert!(is_train_merge("Merge pull request #9 from train/x"));
        assert!(!is_train_merge(
            "Merge pull request #9 from kscharpf/a1/train/x"
        ));
        assert!(!is_train_merge(
            "Merge pull request #9 from kscharpf/train-a"
        ));
        assert!(!is_train_merge("Merge branch 'x' into train/y"));
    }

    #[test]
    fn release_keeps_hand_text_and_resets_the_marker() {
        let log = "# Changelog\n\n## [Unreleased]\n\n<!-- changelog-base: aaa -->\n\n### Added\n- hand\n\n## [0.1.0] — 2026-01-01\n- old\n";
        assert_eq!(marker(&log[log.find("## [U").unwrap()..]), Some("aaa"));
        let out = insert_release(
            log,
            "0.2.0",
            "2026-02-01",
            "### Fixed\n- gen (#9)\n\n",
            "bbb",
        )
        .unwrap();
        assert_eq!(
            out,
            "# Changelog\n\n## [Unreleased]\n\n<!-- changelog-base: bbb -->\n\n\
             ## [0.2.0] — 2026-02-01\n\n### Added\n- hand\n\n### Fixed\n- gen (#9)\n\n\
             ## [0.1.0] — 2026-01-01\n- old\n"
        );
        assert!(insert_release(&out, "0.2.0", "2026-02-01", "", "c").is_err());
        assert!(insert_release("# Changelog\n", "0.2.0", "2026-02-01", "", "c").is_err());
    }

    #[test]
    fn every_released_version_but_the_exempt_one_needs_a_tag() {
        let log = "# Changelog\n\n## [Unreleased]\n\n## [0.3.0-m2] — 2026-10-27\n\n\
                   ## [0.2.0-m1] — 2026-10-20\n\n## [0.1.0-spec] — 2026-10-16\n";
        assert_eq!(untagged_releases(log, &["v0.2.0-m1"]), ["v0.3.0-m2"]);
        assert_eq!(
            untagged_releases(log, &["v0.2.0-m1", "v0.3.0-m2"]),
            <[String; 0]>::default()
        );
        // [Unreleased] never needs a tag; neither does the spec-only release.
        assert_eq!(
            untagged_releases("## [Unreleased]\n## [0.1.0-spec] — x\n", &[]),
            <[String; 0]>::default()
        );
    }
}
