//! `xtask changelog`: the unreleased section of `CHANGELOG.md`, generated from the titles of the
//! merged pull requests (GEN-080, OI-63).
//!
//! A pull request does not touch `CHANGELOG.md`: its title, a conventional commit
//! (`type(scope)!: description`), is the entry. The merge commits of `main` carry the titles
//! (`Merge pull request #N from ...`, the title as the body), so the generator reads `git log`
//! and needs no network. It lists the merges after a base: the commit named by the marker
//! `<!-- changelog-base: <sha> -->` in the `## [Unreleased]` section of `CHANGELOG.md`, else the
//! last `v*` tag. The text of the Unreleased section is kept in the release as it stands.

use std::fmt::Write as _;
use std::process::Command;

use crate::repo;

/// Keep a Changelog sections, in release order.
const SECTIONS: [&str; 6] = [
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
];

/// Index of a section in [`SECTIONS`].
const ADDED: usize = 0;
const CHANGED: usize = 1;
const FIXED: usize = 4;
const SECURITY: usize = 5;

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

/// The section a title belongs to, or `None` when it is not user-visible (`docs`, `chore`,
/// `test`, `ci`, `build`, `style`) and not breaking.
fn section(t: &Title<'_>) -> Option<usize> {
    match t.kind {
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

/// Reads the merges from `git log --first-parent --merges --format=%s%x1f%b%x1e` output, oldest
/// first. A merge whose subject is not `Merge pull request #N ...` is not a pull request.
fn parse_merges(log: &str) -> Vec<Merge> {
    let mut out: Vec<Merge> = log
        .split('\x1e')
        .filter_map(|record| {
            let (subject, body) = record.trim().split_once('\x1f')?;
            let number = subject
                .strip_prefix("Merge pull request #")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            let title = body.lines().map(str::trim).find(|l| !l.is_empty())?;
            Some(Merge {
                number,
                title: title.to_owned(),
            })
        })
        .collect();
    out.reverse();
    out
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
            let Some(i) = section(&t) else { continue };
            let mut line = String::from("- ");
            if t.breaking {
                line.push_str("**Breaking:** ");
            }
            if let Some(scope) = t.scope {
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
fn merges_since_base() -> Result<(Vec<Merge>, String, String), String> {
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
        "--format=%s%x1f%b%x1e",
        &format!("{base}..HEAD"),
    ])?;
    Ok((parse_merges(&log), base, head))
}

/// `xtask changelog`: prints the entries the next release will list, from the merges after the
/// base. Titles that are not conventional commits are reported after the entries.
pub fn show() -> Result<usize, String> {
    let (merges, base, _) = merges_since_base()?;
    let g = generate(&merges);
    print!("{}", render(&g.sections));
    println!("changelog: {} pull request(s) after {base}", merges.len());
    for m in &g.malformed {
        println!("warning: title is not `type(scope): description`: {m}");
    }
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
    let (merges, _, head) = merges_since_base()?;
    let g = generate(&merges);
    let path = repo::root().join("CHANGELOG.md");
    let updated = insert_release(
        &repo::read(&path)?,
        version,
        date,
        &render(&g.sections),
        &head,
    )?;
    std::fs::write(&path, updated).map_err(|e| format!("CHANGELOG.md: {e}"))?;
    for m in &g.malformed {
        println!("warning: title is not `type(scope): description`: {m}");
    }
    println!(
        "changelog: {} pull request(s) released as {version}; add a summary line under the new heading if wanted",
        merges.len()
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{Merge, generate, insert_release, marker, parse_merges, parse_title, render};

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
        ]);
        assert_eq!(
            render(&g.sections),
            "### Added\n- spp: add a codec (#1)\n\n\
             ### Changed\n- **Breaking:** ffi: rename a call (#5)\n- faster (#6)\n- no form (#8)\n\n\
             ### Fixed\n- wrap the count (#2)\n\n\
             ### Security\n- close a hole (#7)\n\n"
        );
        assert_eq!(g.malformed, ["#8: no form"]);
    }

    #[test]
    fn reads_merge_commits_oldest_first() {
        let log = "Merge pull request #3 from a/b\x1ffix: later\n\n\x1e\
                   Merge pull request #2 from a/c\x1f\nfeat: earlier\n\nMore.\x1e\
                   Merge origin/main into x\x1f\x1e\
                   Merge pull request #1 from a/d\x1f\x1e";
        assert_eq!(
            parse_merges(log),
            [merge(2, "feat: earlier"), merge(3, "fix: later")]
        );
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
}
