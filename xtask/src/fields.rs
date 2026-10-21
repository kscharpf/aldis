//! The field catalogue of `docs/03-api-model.md` §9.1 and the three files generated from it
//! (API-082; M2 plan D2): the Rust module `aldis_core::field`, `test-vectors/core/fields.json`
//! for the C++ and binding runners, and the `ALDIS_FIELD_*` constants of the C ABI
//! (`crates/aldis-ffi/src/field.rs`, CABI-021).
//!
//! `cargo xtask fields` checks the catalogue (codes 1 to n in table order, each code and name
//! used once, every "Used by" entry a codec crate) and that the files match it;
//! `cargo xtask fields --write` regenerates them.

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
    defined_by: String,
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

/// Parses the one catalogue table of `text`; `doc` names the document in problems.
fn parse(text: &str, doc: &str, problems: &mut Vec<String>) -> Vec<Entry> {
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
        if !used_by.is_empty() {
            for user in used_by.split(',').map(str::trim) {
                if !USED_BY.contains(&user) {
                    problems.push(format!(
                        "{at}: \"Used by\" names {user:?}, not one of {}",
                        USED_BY.join(", ")
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
            defined_by: defined_by.clone(),
        });
    }
    entries
}

/// Checks that the codes run from 1 in table order with no gap, and that no code or name is
/// used twice (API-082); `doc` names the document in problems.
fn check(entries: &[Entry], doc: &str, problems: &mut Vec<String>) {
    let mut codes: BTreeMap<u16, usize> = BTreeMap::new();
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for (expected, e) in (1_usize..).zip(entries) {
        let at = format!("{doc}:{}", e.line);
        if let Some(first) = codes.insert(e.code, e.line) {
            problems.push(format!(
                "{at}: code {} is used twice (also line {first})",
                e.code
            ));
        } else if usize::from(e.code) != expected {
            problems.push(format!(
                "{at}: code {} out of order: codes run from 1 in table order, so this row is {expected}",
                e.code
            ));
        }
        if let Some(first) = names.insert(&e.name, e.line) {
            problems.push(format!(
                "{at}: name `{}` is used twice (also line {first})",
                e.name
            ));
        }
    }
}

/// Parses and checks the catalogue of `text`.
fn catalogue(text: &str, doc: &str, problems: &mut Vec<String>) -> Vec<Entry> {
    let entries = parse(text, doc, problems);
    check(&entries, doc, problems);
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
        if line.len() <= MAX_WIDTH {
            out.push_str(&line);
            out.push('\n');
        } else {
            let _ = writeln!(
                out,
                "    assert!(\n        ALDIS_FIELD_{c} == field::{c}.code().get()\n    );"
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
    use super::{HEADER, catalogue, doc_comment, ffi, is_display_name, json, module, stale};
    use aldis_testkit::{FieldCatalogue, FieldKind};

    fn doc(rows: &[&str]) -> String {
        let mut text = format!(
            "# Doc\n\nText.\n\n| {} |\n|---|---|---|---|---|---|\n",
            HEADER.join(" | ")
        );
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        text
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
    fn codes_run_from_one_without_gap() {
        // req: API-082
        let gap = "| 3 | `width` | parameter | Width | core | GEN-050 |";
        let p = problems(&[APID, gap]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].contains("code 3 out of order") && p[0].contains("row is 2"),
            "{p:?}"
        );
        let zero = "| 0 | `apid` | field | APID | core | 133.0 §4.1.3.3.4 |";
        assert!(problems(&[zero])[0].contains("code 0 out of order"));
        let swapped = "| 2 | `apid` | field | APID | core | 133.0 §4.1.3.3.4 |";
        let first = "| 1 | `width` | parameter | Width | core | GEN-050 |";
        assert_eq!(problems(&[swapped, first]).len(), 2);
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
        assert!(p[0].contains("expected one field catalogue table"), "{p:?}");
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
