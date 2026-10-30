//! `xtask status-table`: the State and PRs columns of the work-package status tables of
//! `docs/milestones/*.md`, derived from the pull requests of GitHub (OI-63).
//!
//! A pull request belongs to a work package when its title scope names it, `feat(m3/wp-3b): ...`
//! (milestone `m3`, work package `WP-3b`); only a title without such a scope falls back to its
//! head branch (`m3/wp-3b-farm1`), as older pull requests have. The plan is the file
//! `docs/milestones/<id>-*.md`. A row `WP-3b ...` collects the pull requests naming `WP-3b`; a
//! row `WP-3` also collects those naming a sub-part, `WP-3a`, when the plan has no row for it.
//! The pull requests already listed in a row stay in it (older pull requests carry no token).
//! State: an open pull request is `in progress`, else `merged`. A row with no pull request, a row
//! whose id another row shares, and a row whose first cell is not `WP-<n>` (the `Plan` row) are
//! kept as written. The Notes column stays hand-written.
//!
//! Reads GitHub through `gh`, so it is a coordinator tool and no check of the gate.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

use crate::repo;

/// Header of the status table.
const HEADER: &str = "| WP | State | PRs | Notes |";

/// A pull request as `gh pr list --json number,title,headRefName,state` prints it.
#[derive(Debug, PartialEq, Eq)]
struct Pr {
    number: u64,
    title: String,
    branch: String,
    open: bool,
    merged: bool,
}

/// Parses the JSON array of `gh pr list`.
fn parse_prs(json: &str) -> Result<Vec<Pr>, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("gh pr list: {e}"))?;
    let items = value.as_array().ok_or("gh pr list: not an array")?;
    items
        .iter()
        .map(|v| {
            let text = |k: &str| {
                v.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let state = text("state");
            Ok(Pr {
                number: v
                    .get("number")
                    .and_then(Value::as_u64)
                    .ok_or("gh pr list: no number")?,
                title: text("title"),
                branch: text("headRefName"),
                open: state == "OPEN",
                merged: state == "MERGED",
            })
        })
        .collect()
}

/// The plan and work package a title scope names: `m<N>/wp-<n>[letter]` in `type(scope)!: ...`.
fn title_location(title: &str) -> Option<(String, String)> {
    let scope = title.split_once('(')?.1.split_once(')')?.0;
    let (plan, wp) = scope.split_once('/')?;
    let plan = plan.trim().to_lowercase();
    let digits_ok = plan
        .strip_prefix('m')
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
    if !digits_ok || !wp.trim().to_lowercase().starts_with("wp") {
        return None;
    }
    Some((plan, wp_token(wp)?))
}

/// The plan and work package of a pull request: its title scope is authoritative; only a title
/// without such a scope falls back to the branch name (`m3/wp-3b-farm1`), as older PRs have.
fn locate(pr: &Pr) -> Option<(String, String)> {
    if let Some(found) = title_location(&pr.title) {
        return Some(found);
    }
    let (prefix, rest) = pr.branch.split_once('/')?;
    Some((prefix.to_lowercase(), wp_token(rest)?))
}

/// The first work-package token in `text`, normalised (`WP-3b`): `wp`, an optional `-` or space,
/// digits, and at most one lower-case letter that ends the token.
fn wp_token(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("wp") {
        let start = from + at;
        let before_ok = lower[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let rest = lower[start + 2..].trim_start_matches(['-', ' ']);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if before_ok && !digits.is_empty() {
            let tail = &rest[digits.len()..];
            let mut it = tail.chars();
            let letter = it
                .next()
                .filter(char::is_ascii_lowercase)
                .filter(|_| it.next().is_none_or(|c| !c.is_ascii_alphanumeric()));
            return Some(format!(
                "WP-{digits}{}",
                letter.map(String::from).unwrap_or_default()
            ));
        }
        from = start + 2;
    }
    None
}

/// The work-package id at the start of a row's first cell (`WP-3b`), if it has one.
fn row_id(cell: &str) -> Option<String> {
    let rest = cell.trim().strip_prefix("WP-")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let tail = &rest[digits.len()..];
    let mut it = tail.chars();
    let letter = it
        .next()
        .filter(char::is_ascii_lowercase)
        .filter(|_| it.next().is_none_or(|c| !c.is_ascii_alphanumeric()));
    Some(format!(
        "WP-{digits}{}",
        letter.map(String::from).unwrap_or_default()
    ))
}

/// The PR numbers written in a cell (`#12, #15`).
fn cell_numbers(cell: &str) -> BTreeSet<u64> {
    cell.split('#')
        .skip(1)
        .filter_map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect()
}

/// The PRs column text for the numbers, or `—`.
fn render_numbers(numbers: &BTreeSet<u64>) -> String {
    if numbers.is_empty() {
        "—".to_owned()
    } else {
        numbers
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Rewrites the State and PRs cells of the status table in `plan`, from the pull requests of
/// milestone `id`. Returns the new text and the tokens no row takes.
fn derive(plan: &str, id: &str, prs: &[Pr]) -> (String, Vec<String>) {
    let lines: Vec<&str> = plan.lines().collect();
    let Some(head) = lines.iter().position(|l| l.trim() == HEADER) else {
        return (plan.to_owned(), Vec::new());
    };
    let end = lines[head..]
        .iter()
        .position(|l| !l.starts_with('|'))
        .map_or(lines.len(), |n| head + n);
    let rows = &lines[head + 2..end];
    let row_ids: Vec<String> = rows
        .iter()
        .filter_map(|l| {
            row_id(
                l.trim_start_matches('|')
                    .split('|')
                    .next()
                    .unwrap_or_default(),
            )
        })
        .collect();
    // Two rows with one id (a work package split into parts) are left as written.
    let ids: BTreeSet<String> = row_ids
        .iter()
        .filter(|id| row_ids.iter().filter(|o| o == id).count() == 1)
        .cloned()
        .collect();
    let mine: Vec<(&Pr, String)> = prs
        .iter()
        .filter(|p| p.open || p.merged)
        .filter_map(|p| locate(p).filter(|(m, _)| m == id).map(|(_, t)| (p, t)))
        .collect();
    // The row that takes a token: the row of the token, else the row of its work package.
    let target = |token: &str| -> Option<String> {
        if ids.contains(token) {
            return Some(token.to_owned());
        }
        let base: String = token
            .chars()
            .take_while(|c| !c.is_ascii_lowercase())
            .collect();
        ids.contains(&base).then_some(base)
    };
    let orphans: Vec<String> = mine
        .iter()
        .filter(|(_, t)| target(t).is_none())
        .map(|(p, t)| format!("#{} ({t})", p.number))
        .collect();
    let mut out: Vec<String> = lines[..head + 2].iter().map(|l| (*l).to_owned()).collect();
    for row in rows {
        let cells: Vec<&str> = row.trim_start_matches('|').split('|').collect();
        let Some(rid) = cells
            .first()
            .and_then(|c| row_id(c))
            .filter(|rid| ids.contains(rid))
        else {
            out.push((*row).to_owned());
            continue;
        };
        let mut numbers = cells.get(2).map(|c| cell_numbers(c)).unwrap_or_default();
        numbers.extend(
            mine.iter()
                .filter(|(_, t)| target(t).as_deref() == Some(rid.as_str()))
                .map(|(p, _)| p.number),
        );
        // No pull request known: the hand-written state and PRs stay (`planned`, `in review`).
        if numbers.is_empty() {
            out.push((*row).to_owned());
            continue;
        }
        let known: Vec<&Pr> = prs.iter().filter(|p| numbers.contains(&p.number)).collect();
        let state = if known.iter().any(|p| p.open) || known.iter().all(|p| !p.merged) {
            "in progress"
        } else {
            "merged"
        };
        let notes = cells.get(3..).map(|c| c.join("|")).unwrap_or_default();
        out.push(format!(
            "|{}| {state} | {} |{}",
            cells.first().copied().unwrap_or_default(),
            render_numbers(&numbers),
            notes
        ));
    }
    out.extend(lines[end..].iter().map(|l| (*l).to_owned()));
    let mut text = out.join("\n");
    if plan.ends_with('\n') {
        text.push('\n');
    }
    (text, orphans)
}

/// The pull requests of the repository, from `gh`.
fn fetch() -> Result<Vec<Pr>, String> {
    let out = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            "1000",
            "--json",
            "number,title,headRefName,state",
        ])
        .current_dir(repo::root())
        .output()
        .map_err(|e| {
            format!("gh: {e}\n`cargo xtask status-table` needs `gh` on the PATH and network access")
        })?;
    if !out.status.success() {
        return Err(format!(
            "gh pr list: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    parse_prs(&String::from_utf8_lossy(&out.stdout))
}

/// `xtask status-table [--write]`: prints the derived status tables; with `--write`, updates the
/// plan files. Returns 1 when a plan would change and `--write` was not given.
pub fn run(write: bool) -> Result<usize, String> {
    let prs = fetch()?;
    let mut stale = 0;
    for path in repo::files("docs/milestones", &["md"]) {
        let name = repo::rel(&path);
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = stem.split('-').next().unwrap_or_default().to_lowercase();
        let text = repo::read(&path)?;
        let (new, orphans) = derive(&text, &id, &prs);
        if new == text && orphans.is_empty() {
            continue;
        }
        for o in &orphans {
            println!("{name}: pull request {o} names a work package without a unique row");
        }
        if new != text {
            if write {
                std::fs::write(&path, &new).map_err(|e| format!("{name}: {e}"))?;
                println!("{name}: status table updated");
            } else {
                println!("{name}: status table differs (run with --write)");
                stale += 1;
            }
        }
    }
    Ok(if write { 0 } else { stale })
}

#[cfg(test)]
mod tests {
    use super::{Pr, cell_numbers, derive, locate, parse_prs, wp_token};

    fn pr(number: u64, title: &str, branch: &str, state: &str) -> Pr {
        Pr {
            number,
            title: title.to_owned(),
            branch: branch.to_owned(),
            open: state == "OPEN",
            merged: state == "MERGED",
        }
    }

    #[test]
    fn finds_tokens() {
        assert_eq!(wp_token("m3/wp-3b-farm1").as_deref(), Some("WP-3b"));
        assert_eq!(wp_token("feat(m3/wp-3): x").as_deref(), Some("WP-3"));
        assert_eq!(wp_token("WP 12 things").as_deref(), Some("WP-12"));
        assert_eq!(wp_token("wp1-status").as_deref(), Some("WP-1"));
        assert_eq!(wp_token("wp-4x9").as_deref(), Some("WP-4"));
        assert_eq!(wp_token("swp-3 nothing"), None);
        assert_eq!(wp_token("wpa"), None);
    }

    #[test]
    fn title_scope_is_authoritative_and_the_branch_a_fallback() {
        let loc = |t: &str, b: &str| locate(&pr(1, t, b, "OPEN"));
        let some = |m: &str, w: &str| Some((m.to_owned(), w.to_owned()));
        assert_eq!(
            loc("feat(m3/wp-2): x", "m3/wp-2-vectors"),
            some("m3", "WP-2")
        );
        // The title wins over a disagreeing branch.
        assert_eq!(
            loc("feat(m3/wp-4b): x", "m3/wp-2-vectors"),
            some("m3", "WP-4b")
        );
        assert_eq!(loc("fix(m2/wp-1)!: x", "fix-it"), some("m2", "WP-1"));
        // No `m<N>/wp-<n>` scope: the branch.
        assert_eq!(loc("feat: x", "m3/wp-3a-engine"), some("m3", "WP-3a"));
        assert_eq!(loc("feat(spp): x", "m3/wp-3a-engine"), some("m3", "WP-3a"));
        assert_eq!(loc("chore: x", "chore"), None);
        assert_eq!(loc("feat(m3/other): x", "feature"), None);
    }

    #[test]
    fn parses_numbers() {
        assert_eq!(
            cell_numbers("#135, #140,#141")
                .into_iter()
                .collect::<Vec<_>>(),
            [135, 140, 141]
        );
        assert!(cell_numbers("—").is_empty());
    }

    #[test]
    fn parses_gh_output() {
        let prs =
            parse_prs(r#"[{"number":7,"title":"t","headRefName":"m3/wp-1","state":"MERGED"}]"#)
                .unwrap();
        assert_eq!(prs, [pr(7, "t", "m3/wp-1", "MERGED")]);
        assert!(parse_prs("{}").is_err());
    }

    const PLAN: &str = "# Plan\n\n| WP | State | PRs | Notes |\n|---|---|---|---|\n\
| Plan | merged | #1 | The plan |\n\
| WP-0 Infra | complete | #2, #3 | Old, no tokens |\n\
| WP-1a Readings | planned | — | Notes a |\n\
| WP-2 Vectors | planned | — | Notes | with bar |\n\
| WP-3 Engine | planned | — | n |\n\
| WP-4 Later | planned | — | n |\n\
| WP-5 Split | merged | #20 | a |\n| WP-5 Split | merged | #21 | b |\n\nAfter.\n";

    #[test]
    fn derives_state_and_prs() {
        let prs = [
            pr(2, "old", "chore/x", "MERGED"),
            pr(3, "old", "chore/y", "OPEN"),
            pr(10, "feat(m3/wp-1a): r", "m3/wp-1a-readings", "MERGED"),
            pr(11, "feat: v", "m3/wp-2-vectors", "OPEN"),
            pr(12, "feat: e", "m3/wp-3a-engine", "MERGED"),
            pr(13, "feat: e", "m3/wp-3b-engine", "MERGED"),
            pr(14, "feat: other milestone", "m2/wp-4-x", "OPEN"),
            pr(15, "feat: lost", "m3/wp-9-x", "OPEN"),
            pr(16, "feat: closed", "m3/wp-4-x", "CLOSED"),
            pr(17, "feat: split", "m3/wp-5-x", "OPEN"),
        ];
        let (text, orphans) = derive(PLAN, "m3", &prs);
        // WP-5 has two rows, so a pull request cannot be placed in either.
        assert_eq!(orphans, ["#15 (WP-9)", "#17 (WP-5)"]);
        assert_eq!(
            text,
            "# Plan\n\n| WP | State | PRs | Notes |\n|---|---|---|---|\n\
| Plan | merged | #1 | The plan |\n\
| WP-0 Infra | in progress | #2, #3 | Old, no tokens |\n\
| WP-1a Readings | merged | #10 | Notes a |\n\
| WP-2 Vectors | in progress | #11 | Notes | with bar |\n\
| WP-3 Engine | merged | #12, #13 | n |\n\
| WP-4 Later | planned | — | n |\n\
| WP-5 Split | merged | #20 | a |\n| WP-5 Split | merged | #21 | b |\n\nAfter.\n"
        );
        // A derived table derives to itself.
        assert_eq!(derive(&text, "m3", &prs).0, text);
    }

    #[test]
    fn leaves_a_plan_without_a_table_alone() {
        assert_eq!(derive("# x\n", "m3", &[]).0, "# x\n");
    }
}
