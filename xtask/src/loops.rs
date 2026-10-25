//! `xtask loops`: every loop in the library code of a crate is listed, with its bound, in the
//! crate's GEN-022/023 inspection record, and every loop a record lists exists (GEN-022, M2 plan
//! WP-7; part of `cargo xtask ci`).
//!
//! **What counts as a loop.** In the library code of each crate of [`CRATES`] (the modules
//! reachable from `src/lib.rs`, without those and the items gated on `cfg(test)` or `cfg(kani)`):
//! - the loop expressions `for … in`, `while` (also `while let`), and `loop`;
//! - a call of an iterator method that consumes the iterator, and so runs a loop: [`CONSUMERS`];
//! - a call of a slice method that runs over the slice: [`SLICE_LOOPS`].
//!
//! Not found, and listed in a record by hand where they matter: iterator adaptors (`zip`, `map`,
//! `rev`, …), which only run inside a consumer or a `for`; `min` and `max`, which on integers are
//! not loops; comparisons of slices and arrays (`==`); array repeat expressions (`[x; N]`);
//! derived implementations; and loops inside the functions of other crates, which are in those
//! crates' records. The scanner is textual (the tokens of `rs020`), so a method of the crate's own
//! named like one of the lists counts too.
//!
//! **Identifiers.** A loop is named by its file (relative to the crate's `src/`), its item, and
//! its index, as `` `config.rs VcTable::new #2` ``. The item is the innermost function, with the
//! type of the `impl` or `trait` block it is in (`VcTable::new`, never the trait), a `const` or
//! `static` item for a loop in its initialiser, or `name!` for a loop in the body of a
//! `macro_rules!` definition. The index counts the loops of the item in source order from 1; a
//! labelled loop may be named by its label instead (`#'outer`). One span may name several loops
//! of an item (`` `writer.rs TcFrameWriter::start #1 #2` ``).
//!
//! **Records.** The `## Loops` section of `docs/verification/inspections/gen-022-023-<crate>.md`
//! holds a table whose first column, `ID`, names the loops of each row, or holds `—` for a row
//! that names no loop this check finds (a call into another crate, an array initialiser). Every
//! loop found must be named by exactly one row, whose last column (the bound) is not empty, and
//! every name must be a loop found. A crate with a loop and no record fails.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::repo;
use crate::rs020::{Tok, tokens};

/// The library crates (Layers 0–2, the facade, and the C ABI).
pub const CRATES: &[&str] = &[
    "aldis-core",
    "aldis-time",
    "aldis-spp",
    "aldis-epp",
    "aldis-tm",
    "aldis-tc",
    "aldis-aos",
    "aldis-uslp",
    "aldis-link",
    "aldis-cop1",
    "aldis-framework",
    "aldis",
    "aldis-ffi",
];

/// Iterator methods that consume the iterator.
pub const CONSUMERS: &[&str] = &[
    "all",
    "any",
    "collect",
    "count",
    "find",
    "find_map",
    "fold",
    "for_each",
    "last",
    "nth",
    "position",
    "product",
    "rposition",
    "sum",
    "try_fold",
    "try_for_each",
];

/// Slice methods that run over the slice.
pub const SLICE_LOOPS: &[&str] = &["clone_from_slice", "copy_from_slice", "copy_within", "fill"];

/// The record of a crate.
fn record(krate: &str) -> String {
    format!("docs/verification/inspections/gen-022-023-{krate}.md")
}

/// One loop found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    /// The file, relative to the crate's `src/`.
    pub file: String,
    /// The item.
    pub item: String,
    /// The index among the item's loops, from 1.
    pub index: usize,
    /// Its label, without the `'`.
    pub label: Option<String>,
    /// What it is: `for`, `while`, `loop`, or the method.
    pub kind: String,
    pub line: usize,
}

impl Loop {
    /// Its identifier by index.
    fn id(&self) -> String {
        format!("{} {} #{}", self.file, self.item, self.index)
    }
}

/// A block that a loop can be in.
#[derive(Debug, Clone)]
enum Scope {
    /// `impl … Type` or `trait Name`: the type that qualifies its functions.
    Type(String),
    /// A function, `const`, or `static`.
    Item(String),
    /// A `macro_rules!` body.
    Macro(String),
    /// Any other block.
    Block,
}

/// Index of the token after the group that opens at `i` (`(`, `[`, `{`, or `<` counted with
/// `>`), or the end.
fn after_group(t: &[Tok], i: usize) -> usize {
    let (open, close) = match t.get(i).map(|t| t.text.as_str()) {
        Some("(") => ("(", ")"),
        Some("[") => ("[", "]"),
        Some("{") => ("{", "}"),
        Some("<") => ("<", ">"),
        _ => return i + 1,
    };
    let mut depth = 0_usize;
    for (j, tok) in t.iter().enumerate().skip(i) {
        if tok.text == open {
            depth += 1;
        } else if tok.text == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return j + 1;
            }
        }
    }
    t.len()
}

/// Index of the token after the item that starts at `i`: up to its `;` or through its first
/// brace group, at bracket depth 0.
fn after_item(t: &[Tok], mut i: usize) -> usize {
    while i < t.len() {
        match t[i].text.as_str() {
            ";" => return i + 1,
            "{" => return after_group(t, i),
            "(" | "[" => i = after_group(t, i),
            _ => i += 1,
        }
    }
    t.len()
}

/// Whether the attribute tokens (between `#[` and `]`) gate the item on tests or Kani.
fn test_only(attr: &[Tok]) -> bool {
    attr.first().is_some_and(|t| t.text == "cfg")
        && attr.get(1).is_some_and(|t| t.text == "(")
        && cfg_test_only(attr.get(2..attr.len().saturating_sub(1)).unwrap_or(&[]))
}

/// Whether a `cfg` predicate holds only in a test or Kani build: `test`, `kani`, `all(…)` with
/// such a member, or `any(…)` whose members all are. `not(…)` and anything else may hold in a
/// library build.
fn cfg_test_only(pred: &[Tok]) -> bool {
    let Some(head) = pred.first() else {
        return false;
    };
    match head.text.as_str() {
        "test" | "kani" => pred.len() == 1,
        op @ ("all" | "any") if pred.get(1).is_some_and(|t| t.text == "(") => {
            let inner = pred.get(2..pred.len().saturating_sub(1)).unwrap_or(&[]);
            let mut members = Vec::new();
            let (mut start, mut j) = (0, 0);
            while j < inner.len() {
                match inner[j].text.as_str() {
                    "(" => j = after_group(inner, j),
                    "," => {
                        members.push(&inner[start..j]);
                        j += 1;
                        start = j;
                    }
                    _ => j += 1,
                }
            }
            if start < inner.len() {
                members.push(&inner[start..]);
            }
            if op == "all" {
                members.iter().any(|m| cfg_test_only(m))
            } else {
                !members.is_empty() && members.iter().all(|m| cfg_test_only(m))
            }
        }
        _ => false,
    }
}

/// The type an `impl` header names: the last path segment before its generic arguments, of the
/// type after `for` if there is one. `i` is the token after `impl`.
fn impl_type(t: &[Tok], mut i: usize) -> String {
    if t.get(i).is_some_and(|t| t.text == "<") {
        i = after_group(t, i);
    }
    let mut header: Vec<&Tok> = Vec::new();
    while i < t.len() && !matches!(t[i].text.as_str(), "{" | "where" | ";") {
        if t[i].text == "<" {
            i = after_group(t, i);
            header.push(&t[i - 1]);
            continue;
        }
        header.push(&t[i]);
        i += 1;
    }
    let self_ty = header
        .iter()
        .rposition(|t| t.text == "for")
        .map_or(&header[..], |p| &header[p + 1..]);
    let name = self_ty
        .iter()
        .rev()
        .find(|t| {
            t.text
                .starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
        })
        .map_or_else(String::new, |t| t.text.clone());
    // A slice, `[T]`, keeps its brackets, so that its methods differ from those of `T`.
    if self_ty.first().is_some_and(|t| t.text == "[") {
        format!("[{name}]")
    } else {
        name
    }
}

/// Index of the `{` that opens the body of the item whose header starts at `i`, or of the `;`
/// that ends a header without a body, skipping bracket and angle groups.
fn header_end(t: &[Tok], mut i: usize) -> usize {
    while i < t.len() {
        match t[i].text.as_str() {
            "{" | ";" => return i,
            "(" | "[" | "<" => i = after_group(t, i),
            _ => i += 1,
        }
    }
    t.len()
}

/// Whether the `for` at `i` is a loop (`for <pattern> in`), not `impl … for` or `for<'a>`.
fn for_loop(t: &[Tok], i: usize) -> bool {
    if t.get(i + 1).is_some_and(|t| t.text == "<") {
        return false;
    }
    let mut j = i + 1;
    while j < t.len() {
        match t[j].text.as_str() {
            "in" => return true,
            "{" | ";" => return false,
            "(" | "[" => j = after_group(t, j),
            _ => j += 1,
        }
    }
    false
}

/// The loops of one file's source.
pub fn scan(file: &str, src: &str) -> Vec<Loop> {
    let t = tokens(src);
    let mut out: Vec<Loop> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    // Open blocks: the scope and the index of the token after the block.
    let mut open: Vec<(Scope, usize)> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        while open.last().is_some_and(|(_, end)| *end <= i) {
            open.pop();
        }
        let text = t[i].text.as_str();
        let next = t.get(i + 1).map(|t| t.text.as_str());
        match text {
            "#" if next == Some("[")
                || (next == Some("!") && t.get(i + 2).is_some_and(|t| t.text == "[")) =>
            {
                let start = if next == Some("[") { i + 1 } else { i + 2 };
                let end = after_group(&t, start);
                let inner = t.get(start + 1..end.saturating_sub(1)).unwrap_or(&[]);
                if next == Some("[") && test_only(inner) {
                    // Skip the attributes that follow and the item itself.
                    let mut j = end;
                    while t.get(j).is_some_and(|t| t.text == "#") {
                        j = after_group(&t, j + 1);
                    }
                    i = after_item(&t, j);
                } else {
                    i = end;
                }
                continue;
            }
            "impl" | "trait" | "fn" | "macro_rules" => {
                let (scope, from) = match text {
                    "impl" => (Some(Scope::Type(impl_type(&t, i + 1))), i + 1),
                    "trait" => (next.map(|n| Scope::Type(n.to_owned())), i + 2),
                    // Not a function pointer type (`fn(u8) -> u8`).
                    "fn" if next.is_some_and(|n| {
                        n.starts_with(|c: char| c.is_alphabetic() || c == '_')
                    }) =>
                    {
                        (next.map(|n| Scope::Item(n.to_owned())), i + 2)
                    }
                    "macro_rules" if next == Some("!") => (
                        t.get(i + 2).map(|n| Scope::Macro(format!("{}!", n.text))),
                        i + 3,
                    ),
                    _ => (None, i + 1),
                };
                if let Some(scope) = scope {
                    // The header (generics, parameters, return type, `where`) holds no loop and
                    // may hold `impl`, `for`, or `;` (`[u8; 4]`): skip to its body.
                    let j = header_end(&t, from);
                    if t.get(j).is_some_and(|t| t.text == "{") {
                        open.push((scope, after_group(&t, j)));
                    }
                    i = j + 1;
                    continue;
                }
            }
            "const" | "static"
                if t.get(i + 2).is_some_and(|t| t.text == ":")
                    && next.is_some_and(|n| n != "fn" && n != "mut")
                    // Not a const generic parameter (`<const N: usize>`).
                    && !(i > 0 && matches!(t[i - 1].text.as_str(), "<" | ","))
                    && !open.iter().any(|(s, _)| matches!(s, Scope::Item(_))) =>
            {
                // A `const` or `static` item: its initialiser ends at its `;`.
                let name = next.unwrap_or_default().to_owned();
                let end = after_item(&t, i);
                open.push((Scope::Item(name), end));
            }
            "{" => {
                let end = after_group(&t, i);
                open.push((Scope::Block, end));
            }
            _ => {}
        }
        let kind = match text {
            "while" | "loop" => Some(text.to_owned()),
            "for" if for_loop(&t, i) => Some("for".to_owned()),
            "." if next.is_some_and(|n| CONSUMERS.contains(&n) || SLICE_LOOPS.contains(&n))
                && t.get(i + 2)
                    .is_some_and(|t| t.text == "(" || t.text == "::") =>
            {
                next.map(str::to_owned)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            let label = (i >= 2 && t[i - 1].text == ":" && t[i - 2].text.starts_with('\''))
                .then(|| t[i - 2].text.trim_start_matches('\'').to_owned());
            let item = item_name(&open);
            let n = counts.entry(item.clone()).or_insert(0);
            *n += 1;
            out.push(Loop {
                file: file.to_owned(),
                item,
                index: *n,
                label,
                kind,
                line: t[i].line,
            });
        }
        i += 1;
    }
    out
}

/// The item that the innermost scopes name.
fn item_name(open: &[(Scope, usize)]) -> String {
    if let Some((Scope::Macro(m), _)) = open.iter().find(|(s, _)| matches!(s, Scope::Macro(_))) {
        return m.clone();
    }
    let Some(pos) = open.iter().rposition(|(s, _)| matches!(s, Scope::Item(_))) else {
        return "<file>".to_owned();
    };
    let Scope::Item(name) = &open[pos].0 else {
        return "<file>".to_owned();
    };
    // The type of an `impl` or `trait` block directly around the function.
    match open[..pos]
        .iter()
        .rev()
        .find(|(s, _)| !matches!(s, Scope::Block))
    {
        Some((Scope::Type(ty), _)) if !ty.is_empty() => format!("{ty}::{name}"),
        _ => name.clone(),
    }
}

/// The library modules of a crate: `(path relative to src/, source)`, following `mod x;` from
/// `src/lib.rs` and leaving out the modules gated on `cfg(test)` or `cfg(kani)`.
fn modules(src_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut queue = vec!["lib.rs".to_owned()];
    while let Some(rel) = queue.pop() {
        let text = repo::read(&src_dir.join(&rel))?;
        let t = tokens(&text);
        // The directory of this module's children.
        let dir = match rel.as_str() {
            "lib.rs" => String::new(),
            r if r.ends_with("/mod.rs") => r.trim_end_matches("mod.rs").to_owned(),
            r => format!("{}/", r.trim_end_matches(".rs")),
        };
        let mut gated = false;
        let mut i = 0;
        let mut depth = 0_usize;
        while i < t.len() {
            match t[i].text.as_str() {
                "#" if t.get(i + 1).is_some_and(|t| t.text == "[") => {
                    let end = after_group(&t, i + 1);
                    gated |= test_only(t.get(i + 2..end.saturating_sub(1)).unwrap_or(&[]));
                    i = end;
                    continue;
                }
                "{" => depth += 1,
                "}" => depth = depth.saturating_sub(1),
                "mod" if depth == 0 && t.get(i + 2).is_some_and(|t| t.text == ";") => {
                    if !gated && let Some(name) = t.get(i + 1) {
                        let flat = format!("{dir}{}.rs", name.text);
                        let nested = format!("{dir}{}/mod.rs", name.text);
                        if src_dir.join(&flat).is_file() {
                            queue.push(flat);
                        } else if src_dir.join(&nested).is_file() {
                            queue.push(nested);
                        } else {
                            return Err(format!(
                                "{}: `mod {};` names no file",
                                repo::rel(&src_dir.join(&rel)),
                                name.text
                            ));
                        }
                    }
                }
                _ => {}
            }
            if !matches!(
                t[i].text.as_str(),
                "pub" | "(" | ")" | "crate" | "super" | "in"
            ) {
                gated &= t[i].text == "mod" || t[i].text == "#";
            }
            i += 1;
        }
        out.push((rel, text));
    }
    out.sort();
    Ok(out)
}

/// The loops of a crate's library code.
pub fn crate_loops(krate: &str) -> Result<Vec<Loop>, String> {
    let src = repo::root().join("crates").join(krate).join("src");
    Ok(modules(&src)?
        .iter()
        .flat_map(|(rel, text)| scan(rel, text))
        .collect())
}

/// One row of a record's loop table: its line and the loops it names (file, item, index or
/// label), and whether its bound is stated.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    line: usize,
    names: Vec<(String, String, String)>,
    bound: bool,
}

/// Splits a Markdown table row into its cells.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut code = false;
    let mut prev = ' ';
    for ch in inner.chars() {
        match ch {
            '`' => {
                code = !code;
                cur.push(ch);
            }
            '|' if !code && prev != '\\' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
        prev = ch;
    }
    out.push(cur);
    out.into_iter().map(|c| c.trim().to_owned()).collect()
}

/// The loop rows of a record, and its problems.
fn rows(path: &str, text: &str) -> (Vec<Row>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut in_section = false;
    // 0: before the table, 1: in it, after its header, 2: after it.
    let mut state = 0;
    for (n, line) in text.lines().enumerate() {
        let at = format!("{path}:{}", n + 1);
        if let Some(h) = line.strip_prefix("## ") {
            in_section = h.trim() == "Loops";
            continue;
        }
        if !in_section || state == 2 {
            continue;
        }
        if !line.trim_start().starts_with('|') {
            if state == 1 {
                state = 2;
            }
            continue;
        }
        let c = cells(line);
        if state == 0 {
            if c.first().map(String::as_str) != Some("ID") {
                problems.push(format!(
                    "{at}: the Loops table needs `ID` as its first column (GEN-022)"
                ));
                return (out, problems);
            }
            state = 1;
            continue;
        }
        if c.iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':')))
        {
            continue;
        }
        let id = c.first().cloned().unwrap_or_default();
        let mut names = Vec::new();
        if id != "—" {
            for span in id.split('`').skip(1).step_by(2) {
                let words: Vec<&str> = span.split_whitespace().collect();
                let refs = words.get(2..).unwrap_or(&[]);
                if words.len() < 3 || !refs.iter().all(|r| r.starts_with('#') && r.len() > 1) {
                    problems.push(format!("{at}: `{span}` is not `<file> <item> #<n>`"));
                    continue;
                }
                for r in refs {
                    names.push((
                        words[0].to_owned(),
                        words[1].to_owned(),
                        r.trim_start_matches('#').to_owned(),
                    ));
                }
            }
            if names.is_empty() {
                problems.push(format!(
                    "{at}: the ID cell names no loop; write `—` for a row without one"
                ));
            }
        }
        let bound = c.len() > 1 && c.last().is_some_and(|b| !b.is_empty() && b != "—");
        out.push(Row {
            line: n + 1,
            names,
            bound,
        });
    }
    if state == 0 {
        problems.push(format!(
            "{path}: no `## Loops` table with an `ID` column (GEN-022)"
        ));
    }
    (out, problems)
}

/// The problems of one crate: its loops against its record.
fn compare(krate: &str, loops: &[Loop], record_text: Option<&str>) -> Vec<String> {
    let path = record(krate);
    let Some(text) = record_text else {
        return if loops.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "{path}: missing, and {krate} has {} loop(s) (GEN-022)",
                loops.len()
            )]
        };
    };
    let (rows, mut problems) = rows(&path, text);
    let mut named: BTreeMap<usize, usize> = BTreeMap::new();
    for row in &rows {
        for (file, item, r) in &row.names {
            let found = loops.iter().position(|l| {
                &l.file == file
                    && &l.item == item
                    && (r.parse::<usize>().ok() == Some(l.index)
                        || r.strip_prefix('\'')
                            .is_some_and(|r| l.label.as_deref() == Some(r)))
            });
            match found {
                None => problems.push(format!(
                    "{path}:{}: `{file} {item} #{r}` is not a loop of crates/{krate}/src (GEN-022): \
                     remove it, or correct its file, item, or index",
                    row.line
                )),
                Some(k) => {
                    if let Some(first) = named.insert(k, row.line) {
                        problems.push(format!(
                            "{path}:{}: `{}` is also named at line {first}",
                            row.line,
                            loops[k].id()
                        ));
                    }
                    if !row.bound {
                        problems.push(format!(
                            "{path}:{}: the row of `{}` states no bound",
                            row.line,
                            loops[k].id()
                        ));
                    }
                }
            }
        }
    }
    for (k, l) in loops.iter().enumerate() {
        if !named.contains_key(&k) {
            problems.push(format!(
                "crates/{krate}/src/{}:{}: loop `{}` (`{}`) is not in {path}: add it to the Loops \
                 table with its bound (GEN-022)",
                l.file,
                l.line,
                l.id(),
                l.kind
            ));
        }
    }
    problems
}

/// `xtask loops`: the check; `xtask loops list [<crate>...]`: the loops found.
pub fn run(args: &[&str]) -> Result<usize, String> {
    let (list, names) = match args {
        ["list", rest @ ..] => (true, rest),
        rest => (false, rest),
    };
    if !list && !names.is_empty() {
        return Err("usage: cargo xtask loops [list [<crate>...]]".to_owned());
    }
    let crates: Vec<&str> = if names.is_empty() {
        CRATES.to_vec()
    } else {
        names.to_vec()
    };
    let mut problems = Vec::new();
    let mut total = 0;
    let mut out = String::new();
    for krate in &crates {
        let loops = crate_loops(krate)?;
        total += loops.len();
        if list {
            for l in &loops {
                let _ = writeln!(
                    out,
                    "crates/{krate}/src/{}:{}\t`{}`\t{}{}",
                    l.file,
                    l.line,
                    l.id(),
                    l.kind,
                    l.label
                        .as_ref()
                        .map_or_else(String::new, |s| format!(" '{s}"))
                );
            }
            continue;
        }
        let path = repo::root().join(record(krate));
        let text = path.is_file().then(|| repo::read(&path)).transpose()?;
        problems.extend(compare(krate, &loops, text.as_deref()));
    }
    print!("{out}");
    if list {
        return Ok(0);
    }
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "loops: {} crate(s), {total} loop(s) in library code, {} problem(s)",
        crates.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(src: &str) -> Vec<String> {
        scan("x.rs", src)
            .iter()
            .map(|l| format!("{} {}", l.id(), l.kind))
            .collect()
    }

    // req: GEN-022
    #[test]
    fn finds_loops_and_names_their_items() {
        let src = r#"
            //! for x in docs: not code; "while" in a string either.
            const TABLE: [u16; 4] = {
                let mut i = 0;
                while i < 4 { i += 1; }
                [0; 4]
            };
            pub fn digits(n: u64) -> [u8; 12] {
                let mut out = [0; 12];
                for slot in out.iter_mut().rev() { *slot = 1; }
                'outer: loop { break 'outer; }
                out
            }
            impl<const N: usize> Table<N> {
                pub fn find(&self, f: fn(u8) -> bool, g: impl Fn(&u8) -> Option<u8>) -> bool {
                    self.items.iter().any(|x| f(*x)) && self.items.iter().copied().collect::<Vec<_>>().len() > 0
                }
            }
            impl<T> Lookup for [Config<T>] where for<'a> &'a T: Copy {
                fn get(&self) -> usize { self.iter().position(|c| c.ok()).unwrap_or(0) }
            }
            trait Fill { fn zero(buf: &mut [u8]) { buf.fill(0); } }
            macro_rules! octets {
                ($name:ident) => { fn $name(b: &mut [u8]) { b.copy_from_slice(&[0; 2]); } };
            }
            fn outer() { fn inner() { for _ in 0..2 {} } inner(); }
            fn not_loops(a: u8, b: u8) -> u8 { a.max(b).min(3) }
        "#;
        assert_eq!(
            ids(src),
            [
                "x.rs TABLE #1 while",
                "x.rs digits #1 for",
                "x.rs digits #2 loop",
                "x.rs Table::find #1 any",
                "x.rs Table::find #2 collect",
                "x.rs [Config]::get #1 position",
                "x.rs Fill::zero #1 fill",
                "x.rs octets! #1 copy_from_slice",
                "x.rs inner #1 for",
            ]
        );
        let labelled = scan("x.rs", src);
        assert_eq!(labelled[2].label.as_deref(), Some("outer"));
        assert_eq!(labelled[0].line, 5);
    }

    // req: GEN-022
    #[test]
    fn leaves_out_test_and_kani_code() {
        let src = r"
            #[cfg(test)]
            mod tests { fn t() { for _ in 0..3 {} } }
            #[cfg(kani)]
            #[kani::proof]
            fn proof() { loop {} }
            #[cfg(any(test, feature = 'hooks'))]
            fn hooks() { while true {} }
            #[cfg(all(test, feature = 'x'))]
            fn only_tests() { loop {} }
            #[cfg(not(test))]
            fn library() { loop {} }
        ";
        assert_eq!(ids(src), ["x.rs hooks #1 while", "x.rs library #1 loop"]);
    }

    fn found(item: &str, index: usize, label: Option<&str>) -> Loop {
        Loop {
            file: "a.rs".to_owned(),
            item: item.to_owned(),
            index,
            label: label.map(str::to_owned),
            kind: "for".to_owned(),
            line: 7,
        }
    }

    // req: GEN-022
    #[test]
    fn compares_the_loops_with_the_record() {
        let loops = [
            found("f", 1, None),
            found("f", 2, Some("outer")),
            found("g", 1, None),
            found("h", 1, None),
        ];
        let record = "\
# Inspection

## Loops

| ID | Location | Loop | Bound |
|---|---|---|---|
| `a.rs f #1 #'outer` | `a.rs` `f` | over the input | input length |
| `a.rs g #1`, `a.rs g #2` | `a.rs` `g` | twice | constant: 2 |
| `a.rs f #1` | again | | constant |
| — | through another crate | | — |
| `a.rs h #1` | `a.rs` `h` | no bound | — |
| `a.rs` | bad | | x |

## Recursion

| ID | not a loop table |
";
        let problems = compare("aldis-x", &loops, Some(record));
        let text = problems.join("\n");
        let path = "docs/verification/inspections/gen-022-023-aldis-x.md";
        for expected in [
            format!("{path}:8: `a.rs g #2` is not a loop of crates/aldis-x/src"),
            format!("{path}:9: `a.rs f #1` is also named at line 7"),
            format!("{path}:11: the row of `a.rs h #1` states no bound"),
            format!("{path}:12: `a.rs` is not `<file> <item> #<n>`"),
        ] {
            assert!(text.contains(&expected), "{expected}\n{text}");
        }
        assert_eq!(problems.len(), 5, "{text}");

        // A loop the record lacks, and a record without the ID column or without a record.
        let missing = compare("aldis-x", &[found("k", 1, None)], Some(record));
        assert!(
            missing
                .iter()
                .any(|p| p
                    .starts_with("crates/aldis-x/src/a.rs:7: loop `a.rs k #1` (`for`) is not in")),
            "{missing:?}"
        );
        let old =
            "## Loops\n\n| Location | Loop | Bound |\n|---|---|---|\n| `a.rs` `k` | x | y |\n";
        assert_eq!(
            compare("aldis-x", &[], Some(old)),
            [format!(
                "{path}:3: the Loops table needs `ID` as its first column (GEN-022)"
            )]
        );
        assert_eq!(
            compare("aldis-x", &loops, None),
            [format!(
                "{path}: missing, and aldis-x has 4 loop(s) (GEN-022)"
            )]
        );
        assert_eq!(compare("aldis-x", &[], None), Vec::<String>::new());
    }

    // req: GEN-022
    #[test]
    fn reads_every_library_crate() {
        let mut total = 0;
        for krate in CRATES {
            let loops = crate_loops(krate).unwrap();
            // The Kani proofs and the test modules are not library code.
            assert!(loops.iter().all(|l| l.file != "proofs.rs"), "{krate}");
            assert!(
                loops.iter().all(|l| l.item != "<file>"),
                "{krate}: {loops:?}"
            );
            total += loops.len();
        }
        assert!(total > 100, "{total}");
    }
}
