//! `xtask decisions`: the format of the decision log, `docs/process/decisions.md` (VER-062).
//!
//! The log's `## Categories` section names the category slugs in backticks, each of which must
//! appear as `` (`slug`) `` in a heading of `docs/process/decision-rights.md`, so that the two
//! files list the same categories. Every non-blank line of its `## Log` section must read
//! `- YYYY-MM-DD · <category> · <#N or chat> · <decision>`, with a known category, a real date,
//! and dates in order. `cargo xtask metrics` parses the same lines.

use std::collections::{BTreeMap, BTreeSet};

use crate::repo;

/// The decision log, relative to the repository root.
const LOG: &str = "docs/process/decisions.md";

/// The decision rights, which define the categories.
const RIGHTS: &str = "docs/process/decision-rights.md";

/// The separator between the fields of a log line.
const SEP: &str = " · ";

/// One parsed line of the log.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Entry<'a> {
    /// `YYYY-MM-DD`, which sorts as the date does.
    date: &'a str,
    category: &'a str,
    /// `#<number>` or `chat`.
    source: &'a str,
    text: &'a str,
}

/// The lines of the `## <name>` section of `text`, with their 1-based line numbers.
fn section<'a>(text: &'a str, name: &str) -> Option<Vec<(usize, &'a str)>> {
    let heading = format!("## {name}");
    let mut lines = text.lines().enumerate();
    lines.find(|(_, l)| l.trim_end() == heading)?;
    Some(
        lines
            .take_while(|(_, l)| !l.starts_with("## "))
            .map(|(n, l)| (n + 1, l))
            .collect(),
    )
}

/// The words in backticks on `lines`.
fn backticked<'a>(lines: &[(usize, &'a str)]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for (_, line) in lines {
        let mut parts = line.split('`');
        parts.next();
        while let (Some(word), Some(_)) = (parts.next(), parts.next()) {
            out.push(word);
        }
    }
    out
}

/// The category slugs a heading of the rights defines, written `` (`slug`) ``.
fn heading_slugs(heading: &str) -> Vec<&str> {
    heading
        .split("(`")
        .skip(1)
        .filter_map(|rest| rest.split_once("`)").map(|(slug, _)| slug))
        .collect()
}

/// Whether `date` is `YYYY-MM-DD` with a month of 1–12 and a day the month can have.
fn is_date(date: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<&str> = date.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return false;
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 || !(digits(y) && digits(m) && digits(d)) {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>())
    else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// Whether `source` is `chat` or `#` followed by a number without a leading zero.
fn is_source(source: &str) -> bool {
    source == "chat"
        || source.strip_prefix('#').is_some_and(|n| {
            !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
        })
}

/// Parses one log line, or says why it is not one.
fn parse<'a>(line: &'a str, categories: &BTreeSet<&str>) -> Result<Entry<'a>, String> {
    let body = line
        .strip_prefix("- ")
        .ok_or("expected `- YYYY-MM-DD · <category> · <#N or chat> · <decision>`")?;
    let fields: Vec<&str> = body.splitn(4, SEP).collect();
    let [date, category, source, text] = fields.as_slice() else {
        return Err(format!(
            "expected four fields separated by `{}`",
            SEP.trim()
        ));
    };
    if !is_date(date) {
        return Err(format!("`{date}` is not a date YYYY-MM-DD"));
    }
    if !categories.contains(category) {
        return Err(format!(
            "unknown category `{category}` (the slugs of `## Categories`: {})",
            categories.iter().copied().collect::<Vec<_>>().join(", ")
        ));
    }
    if !is_source(source) {
        return Err(format!(
            "`{source}` is neither a pull request or issue number (`#74`) nor `chat`"
        ));
    }
    if text.trim().is_empty() {
        return Err("the decision is empty".to_owned());
    }
    Ok(Entry {
        date,
        category,
        source,
        text,
    })
}

/// Checks the log against the rights; returns the problems, each with its line number.
fn problems(log: &str, rights: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(listed) = section(log, "Categories") else {
        return vec![format!("{LOG}: no `## Categories` section")];
    };
    let categories: BTreeSet<&str> = backticked(&listed).into_iter().collect();
    if categories.is_empty() {
        out.push(format!("{LOG}: `## Categories` names no slug in backticks"));
    }
    for slug in &categories {
        let defined = rights
            .lines()
            .any(|l| l.starts_with('#') && l.contains(&format!("(`{slug}`)")));
        if !defined {
            out.push(format!(
                "{LOG}: category `{slug}` has no heading \"… (`{slug}`)\" in {RIGHTS}"
            ));
        }
    }
    // The other direction: a category the rights define is listed in the log, so the two files
    // cannot drift apart when the rights gain a category.
    for slug in rights
        .lines()
        .filter(|l| l.starts_with('#'))
        .flat_map(heading_slugs)
    {
        if !categories.contains(slug) {
            out.push(format!(
                "{LOG}: {RIGHTS} defines category `{slug}`, which `## Categories` does not list"
            ));
        }
    }
    let Some(lines) = section(log, "Log") else {
        out.push(format!("{LOG}: no `## Log` section"));
        return out;
    };
    let mut last: Option<&str> = None;
    let mut seen: BTreeMap<Entry<'_>, usize> = BTreeMap::new();
    for (n, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        match parse(line, &categories) {
            Ok(entry) => {
                if let Some(prev) = last
                    && entry.date < prev
                {
                    out.push(format!(
                        "{LOG}:{n}: {} comes after {prev}; keep the log in date order",
                        entry.date
                    ));
                }
                last = Some(entry.date);
                if let Some(first) = seen.get(&entry) {
                    out.push(format!(
                        "{LOG}:{n}: repeats the entry of line {first}; record a decision once"
                    ));
                } else {
                    seen.insert(entry, n);
                }
            }
            Err(e) => out.push(format!("{LOG}:{n}: {e}")),
        }
    }
    out
}

/// Checks `docs/process/decisions.md`; returns the number of problems.
pub fn check() -> Result<usize, String> {
    let root = repo::root();
    let log = repo::read(&root.join(LOG))?;
    let rights = repo::read(&root.join(RIGHTS))?;
    let found = problems(&log, &rights);
    for p in &found {
        eprintln!("decisions: {p}");
    }
    let entries = section(&log, "Log").map_or(0, |lines| {
        lines.iter().filter(|(_, l)| !l.trim().is_empty()).count()
    });
    println!(
        "decisions: {entries} log entries, {} problem(s)",
        found.len()
    );
    Ok(found.len())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Entry, is_date, parse, problems};

    const RIGHTS: &str = "# Decision rights\n\n### 2.1 Spec interpretation (`spec`)\n\n\
                          ### 2.2 API and ABI shape (`api`)\n";

    fn log(lines: &str) -> String {
        format!("# Decision log\n\n## Categories\n\n`spec` · `api`\n\n## Log\n\n{lines}\n")
    }

    #[test]
    fn a_well_formed_log_passes() {
        // req: VER-062
        let text = log(
            "- 2026-10-20 · spec · #61 · OI-35 item 1: expose the rule and the UPID\n\
                        - 2026-10-21 · api · chat · status codes stay negative · by CABI-020",
        );
        assert_eq!(problems(&text, RIGHTS), Vec::<String>::new());
    }

    #[test]
    fn a_line_parses_into_its_four_fields() {
        // req: VER-062
        let categories = BTreeSet::from(["api"]);
        assert_eq!(
            parse("- 2026-10-21 · api · #72 · a · b", &categories),
            Ok(Entry {
                date: "2026-10-21",
                category: "api",
                source: "#72",
                text: "a · b",
            })
        );
    }

    #[test]
    fn malformed_lines_are_reported() {
        // req: VER-062
        let text = log("- 2026-10-21 · api · #72\n\
                        * 2026-10-21 · api · #72 · bullet\n\
                        - 2026-13-01 · api · #72 · month\n\
                        - 2026-10-21 · ui · #72 · category\n\
                        - 2026-10-21 · api · PR 72 · source\n\
                        - 2026-10-21 · api · #072 · leading zero\n\
                        - 2026-10-21 · api · #72 ·  ");
        let found = problems(&text, RIGHTS);
        assert_eq!(found.len(), 7, "{found:#?}");
        assert!(
            found[0].contains(":9: expected four fields"),
            "{}",
            found[0]
        );
        assert!(found[3].contains("unknown category `ui`"), "{}", found[3]);
    }

    #[test]
    fn dates_must_not_go_back() {
        // req: VER-062
        let text = log("- 2026-10-21 · api · #72 · later\n- 2026-10-20 · spec · #61 · earlier");
        let found = problems(&text, RIGHTS);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("2026-10-20 comes after 2026-10-21"));
    }

    #[test]
    fn every_category_needs_a_heading_in_the_rights() {
        // req: VER-062
        let text = "## Categories\n\n`spec` · `merge`\n\n## Log\n";
        let found = problems(text, RIGHTS);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].contains("category `merge`"), "{}", found[0]);
        assert!(found[1].contains("defines category `api`"), "{}", found[1]);
        assert_eq!(problems("## Log\n", RIGHTS).len(), 1);
        // No log section, and `api` is not listed.
        assert_eq!(problems("## Categories\n\n`spec`\n", RIGHTS).len(), 2);
    }

    #[test]
    fn every_category_of_the_rights_is_listed() {
        // req: VER-062
        let text = "## Categories\n\n`spec`\n\n## Log\n";
        let found = problems(text, RIGHTS);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("defines category `api`"), "{}", found[0]);
    }

    #[test]
    fn a_repeated_entry_is_reported() {
        // req: VER-062
        let line = "- 2026-10-21 · api · chat · status codes stay negative";
        let text = log(&format!(
            "{line}\n- 2026-10-21 · spec · #61 · other\n{line}"
        ));
        let found = problems(&text, RIGHTS);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found[0].contains("repeats the entry of line"),
            "{}",
            found[0]
        );
    }

    #[test]
    fn dates_are_checked_for_their_month() {
        // req: VER-062
        assert!(is_date("2024-02-29"));
        assert!(!is_date("2026-02-29"));
        assert!(!is_date("2026-04-31"));
        assert!(!is_date("2026-10-00"));
        assert!(!is_date("26-10-21"));
        assert!(!is_date("2026-1-21"));
        assert!(!is_date("2026-10-+1"));
    }
}
