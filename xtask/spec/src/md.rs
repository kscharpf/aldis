//! Minimal Markdown helpers: pipe tables and requirement-ID tokens.

use std::collections::BTreeSet;

/// Splits a Markdown table row into trimmed cells (`\|` is an escaped pipe).
pub fn split_row(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let body = t.strip_prefix('|')?;
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(cur.trim().to_owned());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        cells.push(cur.trim().to_owned());
    }
    Some(cells)
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim_matches(':');
            !c.is_empty() && c.chars().all(|ch| ch == '-')
        })
}

/// A pipe table: header cells and body rows with their 1-based line numbers.
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<(usize, Vec<String>)>,
}

/// All pipe tables in a Markdown document.
pub fn tables(text: &str) -> Vec<Table> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        let (Some(header), Some(sep)) = (split_row(lines[i]), split_row(lines[i + 1])) else {
            i += 1;
            continue;
        };
        if !is_separator(&sep) {
            i += 1;
            continue;
        }
        let mut rows = Vec::new();
        let mut j = i + 2;
        while j < lines.len() {
            match split_row(lines[j]) {
                Some(cells) => rows.push((j + 1, cells)),
                None => break,
            }
            j += 1;
        }
        out.push(Table { header, rows });
        i = j;
    }
    out
}

/// A requirement-ID token found in text, possibly the start of a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdRef {
    pub area: String,
    pub number: u16,
    /// 1-based line number.
    pub line: usize,
    /// End number if the token starts a range such as `GEN-010…012` or `VER-001..VER-004`.
    pub range_end: Option<u16>,
}

impl IdRef {
    pub fn id(&self) -> String {
        format!("{}-{:03}", self.area, self.number)
    }
}

fn three_digits(b: &[u8], at: usize) -> Option<u16> {
    let d = b.get(at..at + 3)?;
    if !d.iter().all(u8::is_ascii_digit) || b.get(at + 3).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some(d.iter().fold(0, |n, &c| n * 10 + u16::from(c - b'0')))
}

/// Finds `AREA-NNN` tokens for the given areas (ASCII scan; safe on UTF-8 text).
pub fn scan_ids(text: &str, areas: &BTreeSet<String>) -> Vec<IdRef> {
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let boundary = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
            if !boundary || !b[i].is_ascii_uppercase() {
                i += 1;
                continue;
            }
            let start = i;
            let mut end = i;
            while end < b.len() && b[end].is_ascii_uppercase() {
                end += 1;
            }
            let area = &line[start..end];
            if b.get(end) == Some(&b'-')
                && areas.contains(area)
                && let Some(number) = three_digits(b, end + 1)
            {
                let after = end + 4;
                let rest = &line[after..];
                // A range continues with a separator, an optional repeated `AREA-`, and 3 digits.
                let range = ["…", "..", "–"].iter().find_map(|sep| {
                    let r = rest.strip_prefix(sep)?;
                    let r = r
                        .strip_prefix(area)
                        .and_then(|x| x.strip_prefix('-'))
                        .unwrap_or(r);
                    let n = three_digits(r.as_bytes(), 0)?;
                    Some((n, rest.len() - r.len() + 3))
                });
                out.push(IdRef {
                    area: area.to_owned(),
                    number,
                    line: line_no + 1,
                    range_end: range.map(|(n, _)| n),
                });
                i = after + range.map_or(0, |(_, consumed)| consumed);
            } else {
                i = end.max(i + 1);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{scan_ids, split_row, tables};
    use std::collections::BTreeSet;

    fn areas() -> BTreeSet<String> {
        ["GEN", "VER", "SPP"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    #[test]
    fn rows_and_tables() {
        assert_eq!(
            split_row("| a | b \\| c | d |").unwrap(),
            ["a", "b | c", "d"]
        );
        let t = tables("text\n| ID | X |\n|---|:-:|\n| GEN-001 | y |\n\nafter");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].header, ["ID", "X"]);
        assert_eq!(t[0].rows, [(4, vec!["GEN-001".to_owned(), "y".to_owned()])]);
    }

    #[test]
    fn ids_ranges_and_non_ids() {
        let ids = scan_ids(
            "GEN-010…012, VER-001..VER-004, SPP-002; SHA-256 XGEN-001 GEN-0011 GEN-01",
            &areas(),
        );
        let got: Vec<(String, Option<u16>)> = ids.iter().map(|r| (r.id(), r.range_end)).collect();
        assert_eq!(
            got,
            [
                ("GEN-010".to_owned(), Some(12)),
                ("VER-001".to_owned(), Some(4)),
                ("SPP-002".to_owned(), None)
            ]
        );
    }
}
