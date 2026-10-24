//! SANA registry snapshots and the tables generated from them (M2 plan D13; OI-06, USLP-007).
//!
//! A protocol value that a SANA registry assigns is never written from memory (AGENTS.md). Each
//! registry this module knows is kept as a committed snapshot under `specs/sana/`: the rows of
//! the registry's table, with the URL they were read from, the retrieval date, and the SHA-256 of
//! the rows. A Rust table is generated from the snapshot.
//!
//! - `cargo xtask sana import <registry>` downloads the registry's table (with `curl`, as
//!   `specs/fetch.sh` does), writes the snapshot, and regenerates the table.
//! - `cargo xtask sana` checks, offline, that each snapshot's rows match its SHA-256 and that
//!   each generated table matches its snapshot. It is part of `cargo xtask ci`.
//! - `cargo xtask sana check-live` downloads each registry and fails when its rows differ from
//!   the snapshot; the nightly job `sana-registries` runs it and opens an issue on a change.
//!
//! SANA publishes no machine-readable export of its registries without an account (checked
//! 2026-10-23: `/api/` asks for a login, and no CSV, JSON, or XML link exists on the registry
//! page), so the import reads the HTML table of the public registry page, one page of up to 100
//! rows, and checks the row count against the count the page states.

use std::fmt::Write as _;
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::repo;

/// One registry: where it is read from and where its snapshot and table go.
struct Registry {
    /// The registry's identifier in its URL.
    id: &'static str,
    /// The page whose table is the snapshot: all rows on one page, sorted by value.
    url: &'static str,
    /// The snapshot.
    snapshot: &'static str,
    /// The generated table.
    table: &'static str,
    /// The table generator.
    generate: fn(&Snapshot) -> Result<String, String>,
}

/// The registries this module knows.
const REGISTRIES: [Registry; 1] = [Registry {
    id: "uslp_protocol_id",
    url: "https://sanaregistry.org/r/uslp_protocol_id/?per_page=100&sort=upid",
    snapshot: "specs/sana/uslp_protocol_id.tsv",
    table: "crates/aldis-uslp/src/upid/table.rs",
    generate: upid_table,
}];

/// The columns of the registry table, in the order of the snapshot (the classes of the page's
/// `<td>` cells are `sana-<column>`).
const COLUMNS: [&str; 5] = ["status", "upid", "description", "references", "oid"];

/// rustfmt's default line width, which the generated table keeps to.
const MAX_WIDTH: usize = 100;

/// The Rust name of each assigned UPID, keyed by the registry's object identifier, which stays
/// with a record when its value or description changes. The values come only from the snapshot;
/// a record assigned later has no name here, and the generator fails until one is added.
const UPID_NAMES: [(&str, &str); 9] = [
    ("1.3.112.4.58.1", "Packets"),
    ("1.3.112.4.58.2", "Cop1ControlCommands"),
    ("1.3.112.4.58.3", "CopPControlCommands"),
    ("1.3.112.4.58.5", "OctetStream"),
    ("1.3.112.4.58.6", "MissionSpecificInformation1"),
    ("1.3.112.4.58.7", "Proximity1Spdus"),
    ("1.3.112.4.58.8", "OnlyIdleData"),
    ("1.3.112.4.58.9", "Proximity1PseudoPacketId1"),
    ("1.3.112.4.58.10", "Proximity1PseudoPacketId2"),
];

/// A snapshot: its metadata and its rows, each with one cell per column of [`COLUMNS`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    url: String,
    retrieved: String,
    sha256: String,
    rows: Vec<Vec<String>>,
}

/// The rows as the snapshot holds them: one line per row, cells separated by tabs.
fn row_lines(rows: &[Vec<String>]) -> String {
    rows.iter().fold(String::new(), |mut s, r| {
        let _ = writeln!(s, "{}", r.join("\t"));
        s
    })
}

/// The lower-case hex SHA-256 of the row lines.
fn digest(rows: &[Vec<String>]) -> String {
    let hash = Sha256::digest(row_lines(rows).as_bytes());
    hash.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

impl Snapshot {
    /// The snapshot file.
    fn render(&self, id: &str) -> String {
        format!(
            "# SANA registry {id}, a snapshot of its table (M2 plan D13; OI-06).\n\
             # Written by `cargo xtask sana import {id}`; never edit by hand. `cargo xtask sana`\n\
             # checks the SHA-256 of the rows below and the table generated from them.\n\
             # url\t{}\n# retrieved\t{}\n# sha256\t{}\n{}\n{}",
            self.url,
            self.retrieved,
            self.sha256,
            COLUMNS.join("\t"),
            row_lines(&self.rows)
        )
    }

    /// Parses a snapshot file.
    fn parse(text: &str) -> Result<Snapshot, String> {
        let meta = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("# {key}\t")))
                .map(str::to_owned)
                .ok_or(format!("no `# {key}` line"))
        };
        let (url, retrieved, sha256) = (meta("url")?, meta("retrieved")?, meta("sha256")?);
        let mut lines = text.lines().filter(|l| !l.starts_with('#'));
        if lines.next() != Some(COLUMNS.join("\t").as_str()) {
            return Err(format!("the column line is not `{}`", COLUMNS.join(" ")));
        }
        let rows: Vec<Vec<String>> = lines
            .map(|l| l.split('\t').map(str::to_owned).collect())
            .collect();
        if let Some(r) = rows.iter().find(|r| r.len() != COLUMNS.len()) {
            return Err(format!(
                "a row has {} cells, not {}",
                r.len(),
                COLUMNS.len()
            ));
        }
        Ok(Snapshot {
            url,
            retrieved,
            sha256,
            rows,
        })
    }
}

/// Replaces the HTML entities the registry pages use.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// The text of an HTML fragment: tags removed, entities replaced, white space collapsed, and
/// the dash the registry shows for an empty cell removed.
fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let text = unescape(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text == "\u{2014}" {
        String::new()
    } else {
        text
    }
}

/// The rows of the registry table in a registry page.
fn parse_page(html: &str) -> Result<Vec<Vec<String>>, String> {
    let body = html
        .split_once("<tbody>")
        .and_then(|(_, rest)| rest.split_once("</tbody>"))
        .map(|(body, _)| body)
        .ok_or("no table body in the page")?;
    let mut rows = Vec::new();
    for tr in body.split("<tr").skip(1) {
        let mut row = Vec::new();
        for col in COLUMNS {
            let marker = format!("class=\"sana-{col}\">");
            let cell = tr
                .split_once(&marker)
                .and_then(|(_, rest)| rest.split_once("</td>"))
                .map(|(cell, _)| cell)
                .ok_or(format!("a row has no `{col}` cell"))?;
            row.push(text_of(cell));
        }
        rows.push(row);
    }
    let stated = text_of(html)
        .split_once(" records in registry")
        .and_then(|(before, _)| before.rsplit(' ').next())
        .and_then(|n| n.parse::<usize>().ok())
        .ok_or("the page does not state its record count")?;
    if stated != rows.len() {
        return Err(format!(
            "the page states {stated} records but its table has {} rows (more than one page?)",
            rows.len()
        ));
    }
    Ok(rows)
}

/// Downloads `url` with `curl`.
fn download(url: &str) -> Result<String, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--max-time", "120", url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{url}: {e}"))
}

/// A UPID cell, `0b` and five binary digits, as its value.
fn upid_value(cell: &str) -> Result<u8, String> {
    cell.strip_prefix("0b")
        .filter(|d| d.len() == 5)
        .and_then(|d| u8::from_str_radix(d, 2).ok())
        .ok_or(format!("`{cell}` is not a 5-bit UPID"))
}

/// An assigned UPID: its value, Rust name, description, references, and OID.
type Assigned<'a> = (u8, &'static str, String, String, &'a String);

/// The assigned UPIDs of the snapshot, after checking that it lists each value once.
fn assigned_upids(snap: &Snapshot) -> Result<Vec<Assigned<'_>>, String> {
    let mut assigned = Vec::new();
    let mut seen = [false; 32];
    for row in &snap.rows {
        let [status, upid, description, references, oid] = row.as_slice() else {
            return Err("a row without five cells".to_owned());
        };
        let value = upid_value(upid)?;
        let slot = seen.get_mut(usize::from(value)).ok_or("UPID above 31")?;
        if *slot {
            return Err(format!("UPID {upid} is listed twice"));
        }
        *slot = true;
        match status.as_str() {
            "Assigned" => {
                let name = UPID_NAMES
                    .iter()
                    .find(|(o, _)| o == oid)
                    .map(|(_, n)| *n)
                    .ok_or(format!(
                        "UPID {upid} (OID {oid}) is assigned but has no name in UPID_NAMES of \
                         xtask/src/sana.rs: add one"
                    ))?;
                assigned.push((value, name, description.clone(), references.clone(), oid));
            }
            "Unassigned" => {}
            other => {
                return Err(format!(
                    "UPID {upid} has status `{other}`, which the generator does not know: decide \
                     whether it is a typed value (USLP-007) and extend xtask/src/sana.rs"
                ));
            }
        }
    }
    if seen.contains(&false) {
        return Err("the registry does not list all 32 UPIDs".to_owned());
    }
    Ok(assigned)
}

/// The generated UPID table of `aldis-uslp` (USLP-007).
fn upid_table(snap: &Snapshot) -> Result<String, String> {
    let assigned = assigned_upids(snap)?;
    let mut s = String::new();
    let intro = format!(
        "The USLP Protocol Identifiers that the SANA registry `uslp_protocol_id` assigns (732.1 \
         §4.1.4.2.3.3, reference \\[14\\]; USLP-007, M2 plan D13, OI-06).\n\n\
         Generated by `cargo xtask sana import uslp_protocol_id` from the snapshot \
         `specs/sana/uslp_protocol_id.tsv` (<{}>, retrieved {}, rows SHA-256 `{}`). Do not edit: \
         `cargo xtask sana` fails when this file differs from the snapshot.",
        snap.url, snap.retrieved, snap.sha256
    );
    for para in intro.split("\n\n") {
        if !s.is_empty() {
            s.push_str("//!\n");
        }
        for line in wrap(para, MAX_WIDTH - "//! ".len()) {
            let _ = writeln!(s, "//! {line}");
        }
    }
    s.push('\n');
    s.push_str("use super::UnassignedUpid;\n\n");
    s.push_str(
        "/// A USLP Protocol Identifier, typed by the SANA registry (732.1 §4.1.4.2.3; USLP-007).\n\
         ///\n\
         /// Each named variant is a value the registry assigns, with the registry's description;\n\
         /// every other 5-bit value is [`Upid::Other`] and decodes like any other (USLP-007).\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]\n\
         pub enum Upid {\n",
    );
    for (value, name, description, references, oid) in &assigned {
        let refs = references.replace('[', "\\[").replace(']', "\\]");
        // Words with `_` (such as MAPA_SDU) are identifiers to rustdoc and Clippy.
        let description: Vec<String> = description
            .split(' ')
            .map(|w| {
                if w.contains('_') {
                    format!("`{w}`")
                } else {
                    w.to_owned()
                }
            })
            .collect();
        let description = description.join(" ");
        let text = format!("`{value:05b}`: {description} (SANA {refs}, OID {oid})");
        for line in wrap(&text, MAX_WIDTH - "    /// ".len()) {
            let _ = writeln!(s, "    /// {line}");
        }
        let _ = writeln!(s, "    {name},");
    }
    s.push_str(
        "    /// A value the registry does not assign (USLP-007).\n    Other(UnassignedUpid),\n}\n\n",
    );
    s.push_str(
        "impl Upid {\n\
         \x20   /// The UPID of a 5-bit field value, or `None` above 31 (732.1 §4.1.4.2.3.1; USLP-007).\n\
         \x20   #[must_use]\n\
         \x20   pub const fn from_bits(bits: u8) -> Option<Upid> {\n\
         \x20       if bits > 0x1F {\n\
         \x20           return None;\n\
         \x20       }\n\
         \x20       Some(Upid::from_field(bits))\n\
         \x20   }\n\n\
         \x20   /// The UPID of the low five bits of `bits`, the field of a TFDF Header (732.1\n\
         \x20   /// §4.1.4.2.3.1; USLP-007).\n\
         \x20   pub(crate) const fn from_field(bits: u8) -> Upid {\n\
         \x20       match bits & 0x1F {\n",
    );
    for (value, name, ..) in &assigned {
        let _ = writeln!(s, "            0b{value:05b} => Upid::{name},");
    }
    s.push_str(
        "            other => Upid::Other(UnassignedUpid(other)),\n\
         \x20       }\n\
         \x20   }\n\n\
         \x20   /// The 5-bit field value (732.1 §4.1.4.2.3.1; USLP-007).\n\
         \x20   #[must_use]\n\
         \x20   pub const fn bits(self) -> u8 {\n\
         \x20       match self {\n",
    );
    for (value, name, ..) in &assigned {
        let _ = writeln!(s, "            Upid::{name} => 0b{value:05b},");
    }
    s.push_str(
        "            Upid::Other(u) => u.get(),\n        }\n    }\n}\n\n\
         /// Whether the registry assigns `bits` (USLP-007).\n\
         pub(crate) const fn is_assigned(bits: u8) -> bool {\n    matches!(\n        bits,\n",
    );
    let values: Vec<String> = assigned
        .iter()
        .map(|(v, ..)| format!("0b{v:05b}"))
        .collect();
    // As rustfmt lays it out: one line when it fits, else one value per line.
    let one_line = values.join(" | ");
    let body = if "        ".len() + one_line.len() <= MAX_WIDTH {
        one_line
    } else {
        values.join("\n            | ")
    };
    let _ = writeln!(s, "        {body}\n    )\n}}");
    Ok(s)
}

/// `text` in lines of at most `width` characters, broken at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
}

/// Writes or checks every registry's generated table; returns the problems.
fn check_tables(problems: &mut Vec<String>, write: bool) -> Result<(), String> {
    let root = repo::root();
    for reg in &REGISTRIES {
        let snap = Snapshot::parse(&repo::read(&root.join(reg.snapshot))?)
            .map_err(|e| format!("{}: {e}", reg.snapshot))?;
        let actual = digest(&snap.rows);
        if snap.sha256 != actual {
            problems.push(format!(
                "{}: the rows' SHA-256 is {actual}, the snapshot records {}; re-import it \
                 (`cargo xtask sana import {}`), never edit it by hand",
                reg.snapshot, snap.sha256, reg.id
            ));
            continue;
        }
        let table = (reg.generate)(&snap).map_err(|e| format!("{}: {e}", reg.snapshot))?;
        if write {
            std::fs::write(root.join(reg.table), &table)
                .map_err(|e| format!("{}: {e}", reg.table))?;
            println!("wrote {}", reg.table);
        } else if std::fs::read_to_string(root.join(reg.table))
            .ok()
            .as_deref()
            != Some(&table)
        {
            problems.push(format!(
                "{} differs from {} (`cargo xtask sana import {}`, or `cargo xtask sana \
                 --write` after a generator change)",
                reg.table, reg.snapshot, reg.id
            ));
        }
    }
    Ok(())
}

/// The registry named `id`.
fn registry(id: &str) -> Result<&'static Registry, String> {
    REGISTRIES
        .iter()
        .find(|r| r.id == id)
        .ok_or(format!("unknown registry `{id}`"))
}

/// Downloads registry `id`, writes its snapshot, and regenerates its table.
fn import(id: &str) -> Result<usize, String> {
    let reg = registry(id)?;
    let rows = parse_page(&download(reg.url)?)?;
    let snap = Snapshot {
        url: reg.url.to_owned(),
        retrieved: crate::metrics::time::date(crate::metrics::time::now()),
        sha256: digest(&rows),
        rows,
    };
    (reg.generate)(&snap)?;
    let path = repo::root().join(reg.snapshot);
    std::fs::write(&path, snap.render(reg.id)).map_err(|e| format!("{}: {e}", reg.snapshot))?;
    println!("wrote {} ({} rows)", reg.snapshot, snap.rows.len());
    let mut problems = Vec::new();
    check_tables(&mut problems, true)?;
    Ok(problems.len())
}

/// Compares every registry with its snapshot; returns the number that differ.
fn check_live() -> Result<usize, String> {
    let mut changed = 0;
    for reg in &REGISTRIES {
        let snap = Snapshot::parse(&repo::read(&repo::root().join(reg.snapshot))?)
            .map_err(|e| format!("{}: {e}", reg.snapshot))?;
        let live = parse_page(&download(reg.url)?)?;
        if live == snap.rows {
            println!("{}: unchanged since {}", reg.id, snap.retrieved);
            continue;
        }
        changed += 1;
        println!(
            "{}: the registry differs from {} (retrieved {}):",
            reg.id, reg.snapshot, snap.retrieved
        );
        for r in snap.rows.iter().filter(|r| !live.contains(r)) {
            println!("  - {}", r.join(" | "));
        }
        for r in live.iter().filter(|r| !snap.rows.contains(r)) {
            println!("  + {}", r.join(" | "));
        }
    }
    Ok(changed)
}

/// `cargo xtask sana …`.
pub fn run(args: &[&str]) -> Result<usize, String> {
    match args {
        [] | ["--write"] => {
            let mut problems = Vec::new();
            check_tables(&mut problems, !args.is_empty())?;
            for p in &problems {
                println!("error: {p}");
            }
            println!(
                "sana: {} registr{}, {} problem(s)",
                REGISTRIES.len(),
                if REGISTRIES.len() == 1 { "y" } else { "ies" },
                problems.len()
            );
            Ok(problems.len())
        }
        ["import", id] => import(id),
        ["check-live"] => check_live(),
        _ => Err(format!("unknown sana command: {}", args.join(" "))),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{COLUMNS, Snapshot, digest, parse_page, text_of, upid_table, upid_value};

    fn row(status: &str, upid: &str, oid: &str) -> Vec<String> {
        [status, upid, "Some data.", "[ccsds-732.1-B-1]", oid]
            .map(str::to_owned)
            .to_vec()
    }

    fn page(rows: &[(&str, &str, &str)], stated: usize) -> String {
        let mut s = format!("<p>{stated} records in registry</p><table><tbody>");
        for (status, upid, desc) in rows {
            s.push_str("<tr class=\"x\">");
            for (col, v) in COLUMNS.iter().zip([*status, *upid, *desc, "", "1.2"]) {
                let _ = write!(s, "<td class=\"sana-{col}\"><p>{v}</p></td>");
            }
            s.push_str("</tr>");
        }
        s.push_str("</tbody></table>");
        s
    }

    #[test]
    fn page_rows_and_count() {
        let html = page(
            &[
                ("Assigned", "0b00000", "A &amp; B"),
                ("Unassigned", "0b00001", "\u{2014}"),
            ],
            2,
        );
        let rows = parse_page(&html).unwrap();
        assert_eq!(rows[0], ["Assigned", "0b00000", "A & B", "", "1.2"]);
        assert_eq!(rows[1][2], "");
        assert!(parse_page(&page(&[("Assigned", "0b00000", "x")], 3)).is_err());
        assert!(parse_page("<p>no table</p>").is_err());
        assert_eq!(text_of("<a>x</a>\n  <b>y</b>"), "x y");
    }

    #[test]
    fn snapshot_round_trip() {
        let rows = vec![row("Assigned", "0b00000", "1.3.112.4.58.1")];
        let snap = Snapshot {
            url: "https://example".to_owned(),
            retrieved: "2026-10-23".to_owned(),
            sha256: digest(&rows),
            rows,
        };
        assert_eq!(Snapshot::parse(&snap.render("r")), Ok(snap));
        assert!(Snapshot::parse("# url\tx\n").is_err());
    }

    #[test]
    fn upid_generator() {
        assert_eq!(upid_value("0b11111"), Ok(31));
        assert!(upid_value("0b1111").is_err() && upid_value("31").is_err());
        let mut rows: Vec<Vec<String>> = (0..32)
            .map(|v| row("Unassigned", &format!("0b{v:05b}"), "9.9"))
            .collect();
        rows[31] = row("Assigned", "0b11111", "1.3.112.4.58.8");
        let snap = |rows: Vec<Vec<String>>| Snapshot {
            url: String::new(),
            retrieved: String::new(),
            sha256: String::new(),
            rows,
        };
        let table = upid_table(&snap(rows.clone())).unwrap();
        assert!(table.contains("    OnlyIdleData,\n"));
        assert!(table.contains("0b11111 => Upid::OnlyIdleData,"));
        let mut unnamed = rows.clone();
        unnamed[0] = row("Assigned", "0b00000", "1.3.112.4.58.99");
        assert!(upid_table(&snap(unnamed)).is_err());
        let mut odd = rows.clone();
        odd[0] = row("Reserved", "0b00000", "9.9");
        assert!(upid_table(&snap(odd)).is_err());
        assert!(upid_table(&snap(rows[1..].to_vec())).is_err());
        let mut twice = rows;
        twice[0] = row("Unassigned", "0b00001", "9.9");
        assert!(upid_table(&snap(twice)).is_err());
    }
}
