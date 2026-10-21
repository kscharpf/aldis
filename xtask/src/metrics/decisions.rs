//! The maintainer's decision log, `docs/process/decisions.md` (VER-063): one entry per decision
//! under the heading `## Log`, in the form
//!
//! ```markdown
//! - 2026-10-21 · <category> · <#PR, #issue, or "chat"> · <one line>
//! ```
//!
//! with the category one of the decision-rights categories (`docs/process/decision-rights.md`).
//! The metrics count the entries per category, in the report's period and in all.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::time;

/// Where the log is, relative to the repository root.
pub const PATH: &str = "docs/process/decisions.md";

/// The separator between the fields of an entry.
const SEP: &str = " · ";

/// One entry of the log.
#[derive(Debug, PartialEq, Eq)]
pub struct Decision {
    /// Midnight UTC of its date.
    pub day: i64,
    /// Its decision-rights category.
    pub category: String,
    /// The pull request or issue (`#N`), or `chat`.
    pub reference: String,
}

/// The entries of the log, and the lines under `## Log` that are not in the entry format.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Log {
    /// The well-formed entries, in file order.
    pub entries: Vec<Decision>,
    /// The other list items under `## Log`, quoted.
    pub malformed: Vec<String>,
}

/// Parses the log's entries: the `- ` items under `## Log`, up to the next `## ` heading.
pub fn parse(text: &str) -> Log {
    let mut log = Log::default();
    let mut in_log = false;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_log = heading.trim() == "Log";
            continue;
        }
        let Some(item) = line.strip_prefix("- ").filter(|_| in_log) else {
            continue;
        };
        let fields: Vec<&str> = item.splitn(4, SEP).map(str::trim).collect();
        match fields.as_slice() {
            [date, category, reference, summary]
                if date.len() == 10 && !category.is_empty() && !summary.is_empty() =>
            {
                if let Some(day) = time::parse(date) {
                    log.entries.push(Decision {
                        day,
                        category: (*category).to_owned(),
                        reference: (*reference).to_owned(),
                    });
                    continue;
                }
                log.malformed.push(line.to_owned());
            }
            _ => log.malformed.push(line.to_owned()),
        }
    }
    log
}

/// Per category: the number of entries on or after `since`, and in all.
pub fn counts(log: &Log, since: i64) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for d in &log.entries {
        let c = out.entry(d.category.clone()).or_default();
        c.1 += 1;
        if d.day >= since {
            c.0 += 1;
        }
    }
    out
}

/// The JSON form of the counts, or of a missing log.
pub fn to_json(log: Option<&Log>, since: i64) -> Value {
    let Some(log) = log else {
        return json!({"path": PATH, "exists": false});
    };
    let categories: serde_json::Map<String, Value> = counts(log, since)
        .into_iter()
        .map(|(k, (period, all))| (k, json!({"period": period, "all": all})))
        .collect();
    json!({
        "path": PATH,
        "exists": true,
        "period": log.entries.iter().filter(|d| d.day >= since).count(),
        "all": log.entries.len(),
        "chat": log.entries.iter().filter(|d| d.reference == "chat").count(),
        "categories": categories,
        "malformed": log.malformed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
# Decisions

Some prose with a list:

- not an entry, not under the log

## Log

- 2026-10-20 · scope · #56 · M2 takes the frame codecs
- 2026-10-21 · process · chat · inspections approved once per milestone
- 2026-10-21 · scope · #61 · TM before AOS
- 2026-10-21 · scope · #61
- 21 Oct 2026 · scope · #61 · a date in the wrong form

## Notes

- 2026-10-21 · scope · #1 · after the log: not counted
";

    // req: VER-063
    #[test]
    fn parses_the_log_and_counts_per_category() {
        let log = parse(LOG);
        assert_eq!(log.entries.len(), 3);
        assert_eq!(
            log.entries[1],
            Decision {
                day: time::parse("2026-10-21").unwrap(),
                category: "process".to_owned(),
                reference: "chat".to_owned(),
            }
        );
        assert_eq!(
            log.malformed,
            [
                "- 2026-10-21 · scope · #61",
                "- 21 Oct 2026 · scope · #61 · a date in the wrong form"
            ]
        );
        let since = time::parse("2026-10-21").unwrap();
        let counts = counts(&log, since);
        assert_eq!(counts["scope"], (1, 2));
        assert_eq!(counts["process"], (1, 1));
        let j = to_json(Some(&log), since);
        assert_eq!(j["period"], 2);
        assert_eq!(j["all"], 3);
        assert_eq!(j["chat"], 1);
        assert_eq!(j["categories"]["scope"]["all"], 2);
        assert_eq!(to_json(None, since)["exists"], false);
        assert_eq!(parse("no log here\n"), Log::default());
    }
}
