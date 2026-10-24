//! The field catalogue of `docs/03-api-model.md` §9.1 and the three files generated from it
//! (API-082; M2 plan D2): the Rust module `aldis_core::field`, `test-vectors/core/fields.json`
//! for the C++ and binding runners, and the `ALDIS_FIELD_*` constants of the C ABI
//! (`crates/aldis-ffi/src/field.rs`, CABI-021).
//!
//! `cargo xtask fields` checks the catalogue and that the files match it; `cargo xtask fields
//! --write` regenerates them. The catalogue's rows are in code order, each code and name used
//! once, every "Used by" entry a codec crate or the owner of a block, and every code in its
//! owner's block (M2 plan D2, amended 2026-10-24): codes 1 to [`LEGACY_LAST`] keep the numbers
//! they had before the blocks; a code above them used by one crate that has a block lies in that
//! block; any other code above them lies in the common block. Within a block the codes run from
//! its first code without a gap.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::num::NonZeroU16;

use aldis_testkit::{FieldCatalogue, FieldEntry, FieldKind};

use crate::{md, repo};

/// The document that holds the catalogue.
const DOC: &str = "docs/03-api-model.md";
/// The generated Rust module.
pub const MODULE: &str = "crates/aldis-core/src/field.rs";
/// The generated catalogue for the C++ and binding runners.
pub const JSON: &str = "test-vectors/core/fields.json";
/// The generated field-code constants of the C ABI (CABI-021).
pub const FFI: &str = "crates/aldis-ffi/src/field.rs";
/// The names the "Used by" column may hold: the codec crates, `aldis-<name>`.
const USED_BY: [&str; 8] = ["core", "time", "spp", "epp", "tm", "tc", "aos", "uslp"];

/// Header of the catalogue table.
const HEADER: [&str; 6] = ["Code", "Name", "Kind", "Meaning", "Used by", "Defined by"];
/// Header of the block table.
const BLOCK_HEADER: [&str; 3] = ["Codes", "Owner", "Reserved by"];
/// The last code assigned before the blocks: codes 1 to this keep their numbers and lie in no
/// crate's block (M2 plan D2, amended 2026-10-24).
const LEGACY_LAST: u16 = 106;
/// The last code of the common block, which follows the legacy range: the codes of fields that
/// several crates name, or that a crate without a block names.
const COMMON_LAST: u16 = 149;
/// The number of codes in a crate's block, reserved when its milestone plan is accepted.
const BLOCK_LEN: u16 = 50;
/// rustfmt's default line width, which the generated module keeps to.
const MAX_WIDTH: usize = 100;

/// One row of the catalogue table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 1-based line of the row in the document.
    line: usize,
    code: u16,
    name: String,
    kind: FieldKind,
    meaning: String,
    /// The "Used by" entries.
    used_by: Vec<String>,
    defined_by: String,
}

/// Who a block of codes belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    /// Codes 1 to [`LEGACY_LAST`], assigned before the blocks.
    Legacy,
    /// The common block.
    Common,
    /// One crate, by the part of its name after `aldis-`.
    Crate(String),
}

/// One row of the block table: a range of codes and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    first: u16,
    last: u16,
    owner: Owner,
}

impl Block {
    fn contains(&self, code: u16) -> bool {
        (self.first..=self.last).contains(&code)
    }

    /// The block, as problems name it.
    fn describe(&self) -> String {
        let (first, last) = (self.first, self.last);
        match &self.owner {
            Owner::Legacy => format!("the legacy range {first}–{last}"),
            Owner::Common => format!("the common block {first}–{last}"),
            Owner::Crate(name) => format!("the block of `{name}`, {first}–{last}"),
        }
    }
}

impl Entry {
    /// The name of the Rust constant: the display name in upper case.
    fn constant(&self) -> String {
        self.name.to_ascii_uppercase()
    }
}

/// Whether `name` is a display name: lower-case ASCII letters, digits, and `_`, starting with a
/// letter, not ending with `_`, and without `__`, so that its upper-case form is a constant name.
fn is_display_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('_')
        && !name.contains("__")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The codes of a block table cell, `first–last` (an en dash, or `-`).
fn code_range(cell: &str) -> Option<(u16, u16)> {
    let (first, last) = cell.split_once('–').or_else(|| cell.split_once('-'))?;
    let (first, last) = (first.trim().parse().ok()?, last.trim().parse().ok()?);
    (first <= last).then_some((first, last))
}

/// The owner a block table cell names, or `None` if it is neither `legacy`, `common`, nor a
/// crate name (lower-case letters and digits) in backticks.
fn owner(cell: &str) -> Option<Owner> {
    match cell {
        "legacy" => Some(Owner::Legacy),
        "common" => Some(Owner::Common),
        _ => {
            let name = cell.strip_prefix('`')?.strip_suffix('`')?;
            let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            valid.then(|| Owner::Crate(name.to_owned()))
        }
    }
}

/// Why `block`, row `index` of the block table, is not where the table's shape puts it: the
/// legacy range 1 to [`LEGACY_LAST`], the common block to [`COMMON_LAST`], then one block of
/// [`BLOCK_LEN`] codes per crate, each following the one before, each crate once, and none of
/// them a codec crate of [`USED_BY`], whose fields keep to the legacy range and the common block.
fn misshapen(block: &Block, earlier: &[Block]) -> Vec<String> {
    let fixed = |first, last, owner| Block { first, last, owner };
    let expected = match earlier.len() {
        0 => Some(fixed(1, LEGACY_LAST, Owner::Legacy)),
        1 => Some(fixed(LEGACY_LAST + 1, COMMON_LAST, Owner::Common)),
        _ => None,
    };
    if let Some(want) = expected {
        return if *block == want {
            Vec::new()
        } else {
            vec![format!("expected {}", want.describe())]
        };
    }
    let Owner::Crate(name) = &block.owner else {
        return vec![
            "only the first two rows are the legacy range and the common block".to_owned(),
        ];
    };
    let mut out = Vec::new();
    let start = earlier.last().map_or(1, |b| b.last.saturating_add(1));
    if block.first != start || block.last.checked_sub(block.first) != Some(BLOCK_LEN - 1) {
        out.push(format!(
            "the block of `{name}` is {}–{}; a crate's block is the next {BLOCK_LEN} codes, \
             {start}–{}",
            block.first,
            block.last,
            start.saturating_add(BLOCK_LEN - 1)
        ));
    }
    if USED_BY.contains(&name.as_str()) {
        out.push(format!(
            "`{name}` is a codec crate of M1 or M2, whose fields keep to the legacy range and \
             the common block"
        ));
    }
    if earlier.iter().any(|b| b.owner == block.owner) {
        out.push(format!("`{name}` has a block already"));
    }
    out
}

/// Parses the one block table of `text` and checks its shape ([`misshapen`]); `doc` names the
/// document in problems.
fn parse_blocks(text: &str, doc: &str, problems: &mut Vec<String>) -> Vec<Block> {
    let tables: Vec<md::Table> = md::tables(text)
        .into_iter()
        .filter(|t| t.header == BLOCK_HEADER)
        .collect();
    let [table] = tables.as_slice() else {
        problems.push(format!(
            "{doc}: expected one table of code blocks ({}), found {}",
            BLOCK_HEADER.join(" | "),
            tables.len()
        ));
        return Vec::new();
    };
    let mut blocks: Vec<Block> = Vec::new();
    for (line, cells) in &table.rows {
        let at = format!("{doc}:{line}");
        let [codes, owner_cell, reserved_by] = cells.as_slice() else {
            problems.push(format!(
                "{at}: expected {} cells, found {}",
                BLOCK_HEADER.len(),
                cells.len()
            ));
            continue;
        };
        let Some((first, last)) = code_range(codes) else {
            problems.push(format!(
                "{at}: codes {codes:?} are not a range `first–last` of 16-bit numbers"
            ));
            continue;
        };
        let Some(owner) = owner(owner_cell) else {
            problems.push(format!(
                "{at}: owner {owner_cell:?} is neither `legacy`, `common`, nor a crate name in \
                 backticks"
            ));
            continue;
        };
        if reserved_by.is_empty() {
            problems.push(format!("{at}: empty Reserved by"));
        }
        let block = Block { first, last, owner };
        for why in misshapen(&block, &blocks) {
            problems.push(format!("{at}: {why}"));
        }
        blocks.push(block);
    }
    if blocks.len() < 2 && problems.is_empty() {
        problems.push(format!(
            "{doc}: the block table needs the legacy range and the common block"
        ));
    }
    blocks
}

/// Parses the one catalogue table of `text`; `doc` names the document in problems, and `owners`
/// the crates with a block, which "Used by" may name besides the codec crates.
fn parse(text: &str, doc: &str, owners: &[&str], problems: &mut Vec<String>) -> Vec<Entry> {
    let tables: Vec<md::Table> = md::tables(text)
        .into_iter()
        .filter(|t| t.header == HEADER)
        .collect();
    let [table] = tables.as_slice() else {
        problems.push(format!(
            "{doc}: expected one field catalogue table ({}), found {}",
            HEADER.join(" | "),
            tables.len()
        ));
        return Vec::new();
    };
    let mut entries = Vec::new();
    for (line, cells) in &table.rows {
        let at = format!("{doc}:{line}");
        let [code, name, kind, meaning, used_by, defined_by] = cells.as_slice() else {
            problems.push(format!(
                "{at}: expected {} cells, found {}",
                HEADER.len(),
                cells.len()
            ));
            continue;
        };
        let Ok(code) = code.parse::<u16>() else {
            problems.push(format!("{at}: code {code:?} is not a 16-bit number"));
            continue;
        };
        let name = name.trim_matches('`').to_owned();
        if !is_display_name(&name) {
            problems.push(format!(
                "{at}: name {name:?} is not lower case with words joined by `_`"
            ));
        }
        if name == "all" {
            problems.push(format!(
                "{at}: name \"all\" is taken by `aldis_core::field::ALL`"
            ));
        }
        let kind = match kind.as_str() {
            "field" => FieldKind::Field,
            "parameter" => FieldKind::Parameter,
            other => {
                problems.push(format!(
                    "{at}: kind {other:?} is neither `field` nor `parameter`"
                ));
                continue;
            }
        };
        for (column, value) in [
            ("Meaning", meaning),
            ("Used by", used_by),
            ("Defined by", defined_by),
        ] {
            if value.is_empty() {
                problems.push(format!("{at}: empty {column}"));
            }
        }
        let mut users = Vec::new();
        if !used_by.is_empty() {
            for user in used_by.split(',').map(str::trim) {
                if USED_BY.contains(&user) || owners.contains(&user) {
                    users.push(user.to_owned());
                } else {
                    let known: Vec<&str> = USED_BY.iter().chain(owners).copied().collect();
                    problems.push(format!(
                        "{at}: \"Used by\" names {user:?}, not one of {}",
                        known.join(", ")
                    ));
                }
            }
        }
        entries.push(Entry {
            line: *line,
            code,
            name,
            kind,
            meaning: meaning.clone(),
            used_by: users,
            defined_by: defined_by.clone(),
        });
    }
    entries
}

/// Why a code above the legacy range does not belong in `block`, given the crates that use it:
/// a crate's block holds codes its crate uses, and the common block holds no code that one crate
/// with a block uses alone. A code never moves, so a code of a crate's block stays there when
/// other crates start to use its field (API-082).
fn misplaced(users: &[String], block: &Block, blocks: &[Block]) -> Option<String> {
    match &block.owner {
        Owner::Legacy => None,
        Owner::Common => {
            let [only] = users else { return None };
            let own = blocks
                .iter()
                .find(|b| b.owner == Owner::Crate(only.clone()))?;
            Some(format!(
                "is used by `{only}` alone, so it takes the next code of {}, not of the common \
                 block",
                own.describe()
            ))
        }
        Owner::Crate(name) => (!users.contains(name)).then(|| {
            format!(
                "lies in {}, but `{name}` does not use it: a field of one crate with a block \
                 takes a code of that block, and any other field one of the common block {}–{}",
                block.describe(),
                LEGACY_LAST + 1,
                COMMON_LAST
            )
        }),
    }
}

/// Checks that the rows are in code order, that no code or name is used twice, and that each
/// code is the next free code of its owner's block (API-082; M2 plan D2, amended 2026-10-24);
/// `doc` names the document in problems.
fn check(entries: &[Entry], blocks: &[Block], doc: &str, problems: &mut Vec<String>) {
    let mut codes: BTreeMap<u16, usize> = BTreeMap::new();
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    let mut next: Vec<u16> = blocks.iter().map(|b| b.first).collect();
    let mut highest: Option<u16> = None;
    for e in entries {
        let at = format!("{doc}:{}", e.line);
        if let Some(first) = names.insert(&e.name, e.line) {
            problems.push(format!(
                "{at}: name `{}` is used twice (also line {first})",
                e.name
            ));
        }
        if let Some(first) = codes.insert(e.code, e.line) {
            problems.push(format!(
                "{at}: code {} is used twice (also line {first})",
                e.code
            ));
            continue;
        }
        if let Some(h) = highest.filter(|h| e.code < *h) {
            problems.push(format!(
                "{at}: code {} out of order: the rows are in code order, and an earlier row has \
                 {h}",
                e.code
            ));
        }
        highest = highest.max(Some(e.code));
        if e.code == 0 {
            problems.push(format!(
                "{at}: code 0 is never assigned: it means no field (CABI-021)"
            ));
            continue;
        }
        let Some((i, block)) = blocks.iter().enumerate().find(|(_, b)| b.contains(e.code)) else {
            problems.push(format!(
                "{at}: code {} lies in no block; a new crate reserves a block of {BLOCK_LEN} codes \
                 when its milestone plan is accepted",
                e.code
            ));
            continue;
        };
        if let Some(n) = next.get_mut(i) {
            if e.code > *n {
                problems.push(format!(
                    "{at}: code {} leaves a gap: the next free code of {} is {n}",
                    e.code,
                    block.describe()
                ));
            }
            *n = (*n).max(e.code.saturating_add(1));
        }
        if let Some(why) = misplaced(&e.used_by, block, blocks) {
            problems.push(format!("{at}: code {} {why}", e.code));
        }
    }
}

/// Parses and checks the catalogue of `text`.
fn catalogue(text: &str, doc: &str, problems: &mut Vec<String>) -> Vec<Entry> {
    let blocks = parse_blocks(text, doc, problems);
    let owners: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match &b.owner {
            Owner::Crate(name) => Some(name.as_str()),
            Owner::Legacy | Owner::Common => None,
        })
        .collect();
    let entries = parse(text, doc, &owners, problems);
    check(&entries, &blocks, doc, problems);
    if entries.is_empty() && problems.is_empty() {
        problems.push(format!("{doc}: the field catalogue has no rows"));
    }
    entries
}

/// Loads the catalogue from the repository: the codes and names of every row, or the problems
/// found.
pub fn load() -> Result<BTreeMap<u16, String>, Vec<String>> {
    let text = repo::read(&repo::root().join(DOC)).map_err(|e| vec![e])?;
    let mut problems = Vec::new();
    let entries = catalogue(&text, DOC, &mut problems);
    if problems.is_empty() {
        Ok(entries.into_iter().map(|e| (e.code, e.name)).collect())
    } else {
        Err(problems)
    }
}

/// `text` as `///` lines of at most [`MAX_WIDTH`] characters, broken between words.
fn doc_comment(text: &str) -> String {
    let mut out = String::new();
    let mut line = String::from("///");
    for word in text.split_whitespace() {
        if line.len() > 3 && line.chars().count() + 1 + word.chars().count() > MAX_WIDTH {
            out.push_str(&line);
            out.push('\n');
            line = String::from("///");
        }
        line.push(' ');
        line.push_str(word);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

/// One `pub const` item, wrapped as rustfmt wraps it when it is too long for one line.
fn const_item(e: &Entry) -> String {
    let lhs = format!("pub const {}: FieldId =", e.constant());
    let rhs = format!("FieldId::catalogued::<{}>(\"{}\");", e.code, e.name);
    if lhs.len() + 1 + rhs.len() <= MAX_WIDTH {
        format!("{lhs} {rhs}\n")
    } else {
        format!("{lhs}\n    {rhs}\n")
    }
}

/// The generated module `aldis_core::field`.
fn module(entries: &[Entry]) -> String {
    let mut out = String::from(
        "// Generated by `cargo xtask fields --write` from the field catalogue in\n\
         // docs/03-api-model.md §9.1. Do not edit: change the table and regenerate.\n\
         \n\
         //! Field codes: one constant per row of the field catalogue (03-api-model.md §9.1;\n\
         //! API-082, GEN-041).\n\
         //!\n\
         //! An [`Error`](crate::Error) names the field it refers to with one of these constants,\n\
         //! and the C ABI carries its code (CABI-021). Codes are never renumbered or reused. The\n\
         //! name is for display only and is never compared. Every constant lives here, generated\n\
         //! from the catalogue; a codec crate re-exports the constants it uses, so a constant never\n\
         //! moves when a second crate starts to use its field (M2 plan D2).\n\
         //!\n\
         //! A `field` names a field of a data unit defined by a Blue Book; a `parameter` names a\n\
         //! managed parameter, a configuration value, an argument, or a length the caller passes.\n\
         \n\
         use crate::error::FieldId;\n",
    );
    for e in entries {
        let kind = match e.kind {
            FieldKind::Field => "field",
            FieldKind::Parameter => "parameter",
        };
        out.push('\n');
        out.push_str(&doc_comment(&format!("{}.", e.meaning)));
        out.push_str("///\n");
        out.push_str(&doc_comment(&format!(
            "Code {}, a {kind} ({}; API-082).",
            e.code, e.defined_by
        )));
        out.push_str(&const_item(e));
    }
    let _ = write!(
        out,
        "\n/// Every field code, in code order from 1 (API-082).\npub const ALL: [FieldId; {}] = [\n",
        entries.len()
    );
    for e in entries {
        let _ = writeln!(out, "    {},", e.constant());
    }
    out.push_str("];\n");
    out
}

/// The generated `ALDIS_FIELD_*` constants of `aldis-ffi` (CABI-021), with a compile-time check
/// that each equals the code of its `aldis_core::field` constant.
fn ffi(entries: &[Entry]) -> String {
    let mut out = String::from(
        "// Generated by `cargo xtask fields --write` from the field catalogue in\n\
         // docs/03-api-model.md §9.1. Do not edit: change the table and regenerate.\n\
         \n\
         //! Field codes of the C ABI: `aldis_error_detail_t::field` holds one of these, or 0 for no\n\
         //! field (03-api-model.md §9.1; API-082, CABI-021). Codes are never renumbered or reused.\n\
         \n\
         use aldis::core::field;\n",
    );
    for e in entries {
        let kind = match e.kind {
            FieldKind::Field => "field",
            FieldKind::Parameter => "parameter",
        };
        out.push('\n');
        out.push_str(&doc_comment(&format!("{}.", e.meaning)));
        out.push_str("///\n");
        out.push_str(&doc_comment(&format!(
            "Code {}, a {kind} ({}; API-082, CABI-021).",
            e.code, e.defined_by
        )));
        let _ = writeln!(
            out,
            "pub const ALDIS_FIELD_{}: u16 = {};",
            e.constant(),
            e.code
        );
    }
    out.push_str("\n// Each code is the code of its `aldis_core::field` constant (API-082).\nconst _: () = {\n");
    for e in entries {
        let c = e.constant();
        let line = format!("    assert!(ALDIS_FIELD_{c} == field::{c}.code().get());");
        let inner = format!("        ALDIS_FIELD_{c} == field::{c}.code().get()");
        if line.len() <= MAX_WIDTH {
            out.push_str(&line);
            out.push('\n');
        } else if inner.len() <= MAX_WIDTH {
            let _ = writeln!(out, "    assert!(\n{inner}\n    );");
        } else {
            // As rustfmt breaks a comparison that does not fit on one line.
            let _ = writeln!(
                out,
                "    assert!(\n        ALDIS_FIELD_{c}\n            == field::{c}.code().get()\n    );"
            );
        }
    }
    out.push_str("};\n");
    out
}

/// The generated `test-vectors/core/fields.json`.
fn json(entries: &[Entry]) -> Result<String, String> {
    let fields = entries
        .iter()
        .map(|e| {
            NonZeroU16::new(e.code)
                .map(|code| FieldEntry {
                    code,
                    name: e.name.clone(),
                    kind: e.kind,
                })
                .ok_or_else(|| format!("{DOC}:{}: code 0 is never assigned", e.line))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let file = FieldCatalogue {
        source: format!("{DOC} §9.1, generated by `cargo xtask fields --write`; do not edit"),
        fields,
    };
    serde_json::to_string_pretty(&file)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}

/// The generated files whose committed text differs from `generated`, as problems.
fn stale(generated: &[(&str, String)], read: impl Fn(&str) -> Option<String>) -> Vec<String> {
    generated
        .iter()
        .filter(|(path, body)| read(path).as_deref() != Some(body.as_str()))
        .map(|(path, _)| {
            format!(
                "{path} differs from the catalogue in {DOC} §9.1; run `cargo xtask fields --write`"
            )
        })
        .collect()
}

/// Checks the catalogue and the generated files, or with `write` regenerates the files; returns
/// the number of problems.
pub fn run(write: bool) -> Result<usize, String> {
    let root = repo::root();
    let text = repo::read(&root.join(DOC))?;
    let mut problems = Vec::new();
    let entries = catalogue(&text, DOC, &mut problems);
    if problems.is_empty() {
        let generated = [
            (MODULE, module(&entries)),
            (JSON, json(&entries)?),
            (FFI, ffi(&entries)),
        ];
        if write {
            for (path, body) in &generated {
                std::fs::write(root.join(path), body).map_err(|e| format!("{path}: {e}"))?;
                println!("wrote {path}");
            }
        } else {
            problems = stale(&generated, |path| {
                std::fs::read_to_string(root.join(path)).ok()
            });
        }
    }
    for p in &problems {
        println!("error: {p}");
    }
    let codes: BTreeSet<u16> = entries.iter().map(|e| e.code).collect();
    println!(
        "fields: {} code(s) in the catalogue, {} problem(s)",
        codes.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_HEADER, HEADER, catalogue, code_range, doc_comment, ffi, is_display_name, json,
        module, stale,
    };
    use aldis_testkit::{FieldCatalogue, FieldKind};

    /// The block table of 03 §9.1 as amended on 2026-10-24.
    const BLOCKS: [&str; 4] = [
        "| 1–106 | legacy | M1, M2 |",
        "| 107–149 | common | D2 amended |",
        "| 150–199 | `cop1` | M3 |",
        "| 200–249 | `link` | M4 |",
    ];

    /// A document with the catalogue `rows` (the first on line 7) and the block table `blocks`.
    fn doc_with(rows: &[&str], blocks: &[&str]) -> String {
        let mut text = format!(
            "# Doc\n\nText.\n\n| {} |\n|---|---|---|---|---|---|\n",
            HEADER.join(" | ")
        );
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        text.push_str("\n| ");
        text.push_str(&BLOCK_HEADER.join(" | "));
        text.push_str(" |\n|---|---|---|\n");
        for row in blocks {
            text.push_str(row);
            text.push('\n');
        }
        text
    }

    fn doc(rows: &[&str]) -> String {
        doc_with(rows, &BLOCKS)
    }

    const APID: &str =
        "| 1 | `apid` | field | Application Process Identifier | core | 133.0 §4.1.3.3.4 |";
    const WIDTH: &str = "| 2 | `width` | parameter | Width of a bit field | core | GEN-050 |";

    fn problems(rows: &[&str]) -> Vec<String> {
        let mut problems = Vec::new();
        catalogue(&doc(rows), "d.md", &mut problems);
        problems
    }

    #[test]
    fn parses_rows_in_code_order() {
        // req: API-082
        let mut p = Vec::new();
        let entries = catalogue(&doc(&[APID, WIDTH]), "d.md", &mut p);
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].code, entries[0].name.as_str()), (1, "apid"));
        assert_eq!(entries[0].kind, FieldKind::Field);
        assert_eq!(entries[0].line, 7);
        assert_eq!(entries[0].defined_by, "133.0 §4.1.3.3.4");
        assert_eq!(
            (entries[1].code, entries[1].kind),
            (2, FieldKind::Parameter)
        );
    }

    #[test]
    fn duplicate_codes_and_names_are_reported() {
        // req: API-082
        let dup_code = "| 1 | `width` | parameter | Width | core | GEN-050 |";
        let p = problems(&[APID, dup_code]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("code 1 is used twice (also line 7)"), "{p:?}");
        let dup_name = "| 2 | `apid` | field | Again | core | 133.0 §4.1.3.3.4 |";
        let p = problems(&[APID, dup_name]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("name `apid` is used twice"), "{p:?}");
    }

    #[test]
    fn codes_are_in_order_and_the_next_free_code_of_their_block() {
        // req: API-082
        let gap = "| 3 | `width` | parameter | Width | core | GEN-050 |";
        let p = problems(&[APID, gap]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].contains("code 3 leaves a gap: the next free code of the legacy range 1–106 is 2"),
            "{p:?}"
        );
        let zero = "| 0 | `apid` | field | APID | core | 133.0 §4.1.3.3.4 |";
        assert!(problems(&[zero])[0].contains("code 0 is never assigned"));
        let swapped = "| 2 | `apid` | field | APID | core | 133.0 §4.1.3.3.4 |";
        let first = "| 1 | `width` | parameter | Width | core | GEN-050 |";
        let p = problems(&[swapped, first]);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[1].contains("code 1 out of order"), "{p:?}");
        // Each block has its own next free code: 107 and 150 follow 1 without a gap.
        let common = "| 107 | `shared` | field | Shared | tc, cop1 | 232.0 |";
        let cop1 = "| 150 | `fop_state` | parameter | FOP-1 state | cop1 | 232.1 |";
        assert_eq!(problems(&[APID, common, cop1]), Vec::<String>::new());
        let p = problems(&[APID, "| 108 | `shared` | field | S | tc, cop1 | 232.0 |"]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("the common block 107–149 is 107"), "{p:?}");
        let p = problems(&[APID, "| 151 | `fop_state` | parameter | S | cop1 | 232.1 |"]);
        assert!(
            p[0].contains("the block of `cop1`, 150–199 is 150"),
            "{p:?}"
        );
        let p = problems(&[APID, "| 250 | `later` | field | L | core | X |"]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("code 250 lies in no block"), "{p:?}");
    }

    #[test]
    fn legacy_codes_are_grandfathered() {
        // req: API-082
        // A code of 1–106 may be used by any crate, a crate with a block too.
        let legacy = "| 1 | `apid` | field | APID | cop1 | 133.0 §4.1.3.3.4 |";
        assert_eq!(problems(&[legacy]), Vec::<String>::new());
        let shared = "| 1 | `apid` | field | APID | core, link | 133.0 §4.1.3.3.4 |";
        assert_eq!(problems(&[shared]), Vec::<String>::new());
    }

    #[test]
    fn a_code_of_one_crate_with_a_block_lies_in_that_block() {
        // req: API-082
        let own = "| 150 | `fop_state` | parameter | FOP-1 state | cop1 | 232.1 |";
        assert_eq!(problems(&[APID, own]), Vec::<String>::new());
        // In the common block, or in another crate's block: misplaced.
        let common = "| 107 | `fop_state` | parameter | FOP-1 state | cop1 | 232.1 |";
        let p = problems(&[APID, common]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].contains(
                "code 107 is used by `cop1` alone, so it takes the next code of the \
                           block of `cop1`, 150–199"
            ),
            "{p:?}"
        );
        let other = "| 200 | `fop_state` | parameter | FOP-1 state | cop1 | 232.1 |";
        let p = problems(&[APID, other]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].contains(
                "code 200 lies in the block of `link`, 200–249, but `link` does not \
                           use it"
            ),
            "{p:?}"
        );
    }

    #[test]
    fn a_shared_code_lies_in_the_common_block() {
        // req: API-082
        // Several crates, or one crate without a block (a codec crate of M1 or M2).
        for users in ["tc, cop1", "cop1, link", "tm", "core, tc"] {
            let row = format!("| 107 | `shared` | field | Shared | {users} | 232.0 |");
            assert_eq!(problems(&[APID, &row]), Vec::<String>::new(), "{users}");
        }
        // A code of a crate's block that a second crate starts to use stays there (API-082).
        let stays = "| 150 | `fop_state` | parameter | FOP-1 state | cop1, link | 232.1 |";
        assert_eq!(problems(&[APID, stays]), Vec::<String>::new());
        // A shared field that its owner does not use is not the owner's.
        let foreign = "| 150 | `shared` | field | Shared | tc, link | 232.0 |";
        assert!(problems(&[APID, foreign])[0].contains("but `cop1` does not use it"));
    }

    #[test]
    fn the_block_table_has_its_fixed_shape() {
        // req: API-082
        let blocks = |rows: &[&str]| {
            let mut p = Vec::new();
            catalogue(&doc_with(&[APID], rows), "d.md", &mut p);
            p
        };
        assert_eq!(blocks(&BLOCKS[..2]), Vec::<String>::new());
        let p = blocks(&[]);
        assert!(
            p[0].contains("needs the legacy range and the common block"),
            "{p:?}"
        );
        let p = blocks(&["| 1–100 | legacy | x |", BLOCKS[1]]);
        assert!(p[0].contains("expected the legacy range 1–106"), "{p:?}");
        let p = blocks(&[BLOCKS[0], "| 107–149 | `tc` | x |"]);
        assert!(p[0].contains("expected the common block 107–149"), "{p:?}");
        let p = blocks(&[BLOCKS[0], BLOCKS[1], "| 150–189 | `cop1` | M3 |"]);
        assert!(
            p[0].contains(
                "the block of `cop1` is 150–189; a crate's block is the next 50 codes, \
                           150–199"
            ),
            "{p:?}"
        );
        let p = blocks(&[BLOCKS[0], BLOCKS[1], "| 200–249 | `link` | M4 |"]);
        assert!(p[0].contains("next 50 codes, 150–199"), "{p:?}");
        let p = blocks(&[BLOCKS[0], BLOCKS[1], BLOCKS[2], "| 200–249 | `cop1` | x |"]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("`cop1` has a block already"), "{p:?}");
        let p = blocks(&[BLOCKS[0], BLOCKS[1], "| 150–199 | `tm` | x |"]);
        assert!(p[0].contains("`tm` is a codec crate of M1 or M2"), "{p:?}");
        let p = blocks(&[BLOCKS[0], BLOCKS[1], "| 150–199 | common | x |"]);
        assert!(p[0].contains("only the first two rows"), "{p:?}");
        for (row, want) in [
            ("| 1 | legacy | x |", "not a range"),
            ("| 9–1 | legacy | x |", "not a range"),
            ("| 1–106 | Legacy | x |", "neither `legacy`"),
            ("| 1–106 | `Cop1` | x |", "neither `legacy`"),
            ("| 1–106 | cop1 | x |", "neither `legacy`"),
            ("| 1–106 | legacy |  |", "empty Reserved by"),
            ("| 1–106 | legacy |", "expected 3 cells"),
        ] {
            let p = blocks(&[row, BLOCKS[1]]);
            assert!(p[0].contains(want), "{row}: {p:?}");
        }
        assert_eq!(code_range("150-199"), Some((150, 199)));
        assert_eq!(code_range(" 1 – 106 "), Some((1, 106)));
        let mut p = Vec::new();
        catalogue(&format!("{}\n{}", doc(&[APID]), doc(&[])), "d.md", &mut p);
        assert!(
            p.iter().any(|p| p.contains("one table of code blocks")),
            "{p:?}"
        );
    }

    #[test]
    fn malformed_rows_are_reported() {
        // req: API-082
        assert!(problems(&["| x | `apid` | field | A | core | B |"])[0].contains("not a 16-bit"));
        assert!(problems(&["| 1 | `Apid` | field | A | core | B |"])[0].contains("lower case"));
        assert!(problems(&["| 1 | `all` | field | A | core | B |"])[0].contains("ALL"));
        assert!(problems(&["| 1 | `apid` | wire | A | core | B |"])[0].contains("neither"));
        assert!(problems(&["| 1 | `apid` | field |  | core | B |"])[0].contains("empty Meaning"));
        assert!(problems(&["| 1 | `apid` | field | A | core |"])[0].contains("expected 6 cells"));
        let mut p = Vec::new();
        catalogue("# No table\n", "d.md", &mut p);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("expected one table of code blocks"), "{p:?}");
        assert!(p[1].contains("expected one field catalogue table"), "{p:?}");
        let mut p = Vec::new();
        catalogue(&doc(&[]), "d.md", &mut p);
        assert!(p[0].contains("no rows"), "{p:?}");
        assert!(is_display_name("pfield_tail") && is_display_name("t_len"));
        for bad in ["", "_a", "a_", "a__b", "1a", "a-b", "aB"] {
            assert!(!is_display_name(bad), "{bad}");
        }
    }

    #[test]
    fn generated_files_follow_the_catalogue() {
        // req: API-082
        let mut p = Vec::new();
        let entries = catalogue(&doc(&[APID, WIDTH]), "d.md", &mut p);
        let m = module(&entries);
        assert!(m.starts_with("// Generated by `cargo xtask fields --write`"));
        assert!(m.contains(
            "/// Application Process Identifier.\n///\n/// Code 1, a field (133.0 §4.1.3.3.4; \
             API-082).\npub const APID: FieldId = FieldId::catalogued::<1>(\"apid\");\n"
        ));
        assert!(m.contains("/// Code 2, a parameter (GEN-050; API-082).\n"));
        assert!(m.contains("pub const ALL: [FieldId; 2] = [\n    APID,\n    WIDTH,\n];\n"));
        let j: FieldCatalogue = serde_json::from_str(&json(&entries).unwrap()).unwrap();
        assert_eq!(j.fields.len(), 2);
        assert_eq!(
            (j.fields[1].code.get(), j.fields[1].name.as_str()),
            (2, "width")
        );
        assert_eq!(j.fields[1].kind, FieldKind::Parameter);
        let c = ffi(&entries);
        assert!(c.starts_with("// Generated by `cargo xtask fields --write`"));
        assert!(c.contains(
            "/// Application Process Identifier.\n///\n/// Code 1, a field (133.0 §4.1.3.3.4; \
             API-082, CABI-021).\npub const ALDIS_FIELD_APID: u16 = 1;\n"
        ));
        assert!(c.contains("    assert!(ALDIS_FIELD_WIDTH == field::WIDTH.code().get());\n"));
    }

    #[test]
    fn used_by_names_codec_crates() {
        // req: API-082
        let shared = "| 1 | `apid` | field | APID | core, spp, tm | 133.0 §4.1.3.3.4 |";
        assert_eq!(problems(&[shared]), Vec::<String>::new());
        let typo = "| 1 | `apid` | field | APID | core, sp | 133.0 §4.1.3.3.4 |";
        let p = problems(&[typo]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("names \"sp\""), "{p:?}");
        // The owner of a block is a valid entry.
        let owner = "| 1 | `apid` | field | APID | core, link | 133.0 §4.1.3.3.4 |";
        assert_eq!(problems(&[owner]), Vec::<String>::new());
        let crate_name = "| 1 | `apid` | field | APID | aldis-spp | 133.0 §4.1.3.3.4 |";
        assert_eq!(problems(&[crate_name]).len(), 1);
        let empty_entry = "| 1 | `apid` | field | APID | core, | 133.0 §4.1.3.3.4 |";
        assert_eq!(problems(&[empty_entry]).len(), 1);
    }

    #[test]
    fn long_items_wrap_as_rustfmt_does() {
        // req: API-082
        let long =
            "| 1 | `a_very_long_display_name_for_a_field_code_xx` | field | Long | core | X |";
        let mut p = Vec::new();
        let m = module(&catalogue(&doc(&[long]), "d.md", &mut p));
        assert!(m.contains(
            "pub const A_VERY_LONG_DISPLAY_NAME_FOR_A_FIELD_CODE_XX: FieldId =\n    \
             FieldId::catalogued::<1>(\"a_very_long_display_name_for_a_field_code_xx\");\n"
        ));
    }

    #[test]
    fn doc_comments_wrap_between_words() {
        // req: RS-015
        let text = "word ".repeat(30);
        let doc = doc_comment(text.trim());
        let lines: Vec<&str> = doc.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("/// ") && l.chars().count() <= 100)
        );
        assert_eq!(lines[0].chars().count(), 98);
        assert_eq!(doc_comment("Short."), "/// Short.\n");
    }

    #[test]
    fn stale_files_are_reported() {
        // req: API-082
        let generated = [
            ("a.rs", "new\n".to_owned()),
            ("b.json", "same\n".to_owned()),
        ];
        let p = stale(&generated, |path| match path {
            "a.rs" => Some("old\n".to_owned()),
            _ => Some("same\n".to_owned()),
        });
        assert_eq!(p.len(), 1);
        assert!(
            p[0].starts_with("a.rs differs") && p[0].contains("--write"),
            "{p:?}"
        );
        assert_eq!(stale(&generated, |_| None).len(), 2);
    }
}
