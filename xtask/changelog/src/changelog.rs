//! `xtask changelog`: per-PR changelog fragments in `changelog.d/` (GEN-080).
//!
//! Each pull request adds one fragment instead of editing `CHANGELOG.md`, so parallel PRs do not
//! conflict. A release collects the fragments into a new section of `CHANGELOG.md`.

use std::fmt::Write as _;
use std::path::PathBuf;

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

/// Fragment directory, relative to the repository root.
const DIR: &str = "changelog.d";

/// Entries of one fragment, indexed like `SECTIONS`.
type Entries = [Vec<String>; SECTIONS.len()];

/// Parsed fragments with their paths, and the problems found in the others.
type Loaded = (Vec<(PathBuf, Entries)>, Vec<String>);

/// Parses a fragment: `### <Section>` headings, each followed by `- ` entries. An entry may
/// continue on lines indented by two spaces.
fn parse(text: &str) -> Result<Entries, String> {
    let mut entries = Entries::default();
    let mut seen = [false; SECTIONS.len()];
    let mut current = None;
    for (n, line) in text.lines().enumerate() {
        let at = n + 1;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("### ") {
            let i = SECTIONS
                .iter()
                .position(|s| *s == name.trim())
                .ok_or_else(|| {
                    format!(
                        "line {at}: unknown section `{}` (use {})",
                        name.trim(),
                        SECTIONS.join(", ")
                    )
                })?;
            if seen[i] {
                return Err(format!(
                    "line {at}: section `{}` appears twice",
                    SECTIONS[i]
                ));
            }
            seen[i] = true;
            current = Some(i);
        } else if line.starts_with("- ") {
            let i =
                current.ok_or_else(|| format!("line {at}: entry before any `### <Section>`"))?;
            entries[i].push(line.to_owned());
        } else if line.starts_with("  ") {
            let last = current
                .and_then(|i| entries[i].last_mut())
                .ok_or_else(|| format!("line {at}: continuation line without an entry"))?;
            last.push('\n');
            last.push_str(line);
        } else {
            return Err(format!(
                "line {at}: expected `### <Section>`, a `- ` entry, or a continuation indented by two spaces"
            ));
        }
    }
    if let Some(i) = (0..SECTIONS.len()).find(|&i| seen[i] && entries[i].is_empty()) {
        return Err(format!("section `{}` has no entries", SECTIONS[i]));
    }
    if entries.iter().all(Vec::is_empty) {
        return Err("no entries".to_owned());
    }
    Ok(entries)
}

/// Renders the entries of all fragments as release sections, in `SECTIONS` order.
fn render(fragments: &[Entries]) -> String {
    let mut out = String::new();
    for (i, name) in SECTIONS.iter().enumerate() {
        let lines: Vec<&str> = fragments
            .iter()
            .flat_map(|f| f[i].iter().map(String::as_str))
            .collect();
        if !lines.is_empty() {
            let _ = write!(out, "### {name}\n{}\n\n", lines.join("\n"));
        }
    }
    out
}

/// Inserts a `## [<version>] — <date>` section after the `## [Unreleased]` section.
fn insert_release(
    changelog: &str,
    version: &str,
    date: &str,
    body: &str,
) -> Result<String, String> {
    if changelog.contains(&format!("## [{version}]")) {
        return Err(format!("CHANGELOG.md already has a section for {version}"));
    }
    let start = changelog
        .find("## [Unreleased]\n")
        .ok_or("CHANGELOG.md: no `## [Unreleased]` section")?;
    let after = start + "## [Unreleased]\n".len();
    let end = changelog[after..]
        .find("\n## [")
        .map_or(changelog.len(), |i| after + i + 1);
    Ok(format!(
        "{}## [{version}] — {date}\n\n{body}{}",
        &changelog[..end],
        &changelog[end..]
    ))
}

/// Fragment files, sorted by name. `README.md` describes the format and is not a fragment.
fn fragment_paths() -> Result<Vec<PathBuf>, String> {
    let dir = repo::root().join(DIR);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{DIR}: {e}"))?;
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n != "README.md"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Parses every fragment; returns the parsed ones and the problems found.
fn load() -> Result<Loaded, String> {
    let mut parsed = Vec::new();
    let mut problems = Vec::new();
    for path in fragment_paths()? {
        let rel = repo::rel(&path);
        let name_ok = path.extension().is_some_and(|e| e == "md")
            && path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c))
            });
        if !name_ok {
            problems.push(format!("{rel}: name must be `[a-z0-9._-]+.md`"));
            continue;
        }
        match parse(&repo::read(&path)?) {
            Ok(entries) => parsed.push((path, entries)),
            Err(e) => problems.push(format!("{rel}: {e}")),
        }
    }
    Ok((parsed, problems))
}

/// `xtask changelog check`: every fragment is well formed.
pub fn check() -> Result<usize, String> {
    let (parsed, problems) = load()?;
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "changelog: {} fragment(s), {} problem(s)",
        parsed.len() + problems.len(),
        problems.len()
    );
    Ok(problems.len())
}

/// `xtask changelog show`: prints the unreleased changes as the next release would list them.
pub fn show() -> Result<usize, String> {
    let (parsed, problems) = load()?;
    let entries: Vec<Entries> = parsed.into_iter().map(|(_, e)| e).collect();
    print!("{}", render(&entries));
    for p in &problems {
        println!("error: {p}");
    }
    Ok(problems.len())
}

/// `xtask changelog release <version> <date>`: moves every fragment into a new section of
/// `CHANGELOG.md` and deletes the fragments.
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
    let (parsed, problems) = load()?;
    if !problems.is_empty() {
        return Err(format!(
            "fix the fragments first (`cargo xtask changelog check`):\n{}",
            problems.join("\n")
        ));
    }
    if parsed.is_empty() {
        return Err(format!("no fragments in {DIR}/"));
    }
    let path = repo::root().join("CHANGELOG.md");
    let entries: Vec<Entries> = parsed.iter().map(|(_, e)| e.clone()).collect();
    let updated = insert_release(&repo::read(&path)?, version, date, &render(&entries))?;
    std::fs::write(&path, updated).map_err(|e| format!("CHANGELOG.md: {e}"))?;
    for (fragment, _) in &parsed {
        std::fs::remove_file(fragment).map_err(|e| format!("{}: {e}", repo::rel(fragment)))?;
    }
    println!(
        "changelog: {} fragment(s) released as {version}; add a summary line under the new heading if wanted",
        parsed.len()
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{insert_release, parse, render};

    #[test]
    fn parses_sections_entries_and_continuations() {
        let e = parse("### Fixed\n- a\n  more\n\n### Added\n- b\n- c\n").unwrap();
        assert_eq!(e[0], ["- b", "- c"]);
        assert_eq!(e[4], ["- a\n  more"]);
        assert_eq!(e[1], [] as [&str; 0]);
    }

    #[test]
    fn rejects_malformed_fragments() {
        for (text, msg) in [
            ("- a\n", "entry before"),
            ("### Misc\n- a\n", "unknown section"),
            ("### Added\n- a\n### Added\n- b\n", "appears twice"),
            ("### Added\n### Fixed\n- a\n", "`Added` has no entries"),
            ("### Added\ntext\n", "expected"),
            ("  orphan\n", "continuation"),
            ("\n", "no entries"),
        ] {
            let err = parse(text).unwrap_err();
            assert!(err.contains(msg), "{text:?}: {err}");
        }
    }

    #[test]
    fn renders_in_section_order_then_fragment_order() {
        let a = parse("### Fixed\n- a1\n### Added\n- a2\n").unwrap();
        let b = parse("### Added\n- b1\n").unwrap();
        assert_eq!(
            render(&[a, b]),
            "### Added\n- a2\n- b1\n\n### Fixed\n- a1\n\n"
        );
    }

    #[test]
    fn inserts_release_after_unreleased() {
        let log = "# Changelog\n\n## [Unreleased]\n\nSee changelog.d/.\n\n## [0.1.0] — 2026-01-01\n- old\n";
        let out = insert_release(log, "0.2.0", "2026-02-01", "### Added\n- new\n\n").unwrap();
        assert_eq!(
            out,
            "# Changelog\n\n## [Unreleased]\n\nSee changelog.d/.\n\n## [0.2.0] — 2026-02-01\n\n### Added\n- new\n\n## [0.1.0] — 2026-01-01\n- old\n"
        );
        assert!(insert_release(&out, "0.2.0", "2026-02-01", "").is_err());
        assert!(insert_release("# Changelog\n", "0.2.0", "2026-02-01", "").is_err());
    }
}
