//! `xtask rs020`: no bare integer in the public API of the codec crates, unless exempted with a
//! reason (RS-020, "parse, don't validate"; M2 plan WP-7; part of `cargo xtask ci`).
//!
//! RS-020 requires a field with a restricted range to be a type that enforces it. A bare
//! integer in a public field or enum payload is where such a range most easily goes unenforced:
//! the caller can build any value, and the code that receives it must check it again or trust
//! it. This check finds, in the public API of each crate of [`CRATES`]:
//! - an enum variant whose payload includes a bare integer;
//! - a `pub` field (named, or of a tuple struct) of a public struct or union that is a bare
//!   integer.
//!
//! A *bare integer* is a primitive integer type (`u8` … `u128`, `usize`, `i8` … `i128`,
//! `isize`), also behind a reference, inside `Option<…>`, or as an element of a tuple. Arrays
//! and slices of integers are octets, not values, and do not count. In the body of a
//! `macro_rules!` definition, a field whose type is a macro variable (`$t`) counts too, since an
//! invocation can make it an integer.
//!
//! **Public API.** An item counts when it is `pub` and reachable from the crate root: it lies in
//! a chain of `pub mod`s from `src/lib.rs`, or a `pub use` in such a module re-exports it (a
//! name, a glob, or a module). `pub(crate)` and other restricted items, items of private modules
//! that are not re-exported, and modules gated on `cfg(test)` or `cfg(kani)` do not count.
//! Types that a `macro_rules!` body defines count wherever the macro is defined, since the
//! invocations are not expanded. Re-exports from other crates are checked in their own crate.
//!
//! **Exemptions.** [`EXEMPTIONS`] lists each finding that is not a gap, as an `[[exemption]]`
//! table with the item's path and a reason: a length or offset whose only bound is the type, a
//! raw wire value of a lenient view (API-032), a full-width counter. The path is that of the
//! definition (`aldis_core::layout::FrameLayout.frame_len`,
//! `aldis_tm::header::PacketPointer::StartsAt.0`), or of the re-export for an item of a private
//! module. An exemption without a reason, with an unknown key, listed twice, or that no longer
//! matches a finding is a problem, so the list stays exact.
//!
//! **Parser.** The source is read with a textual scanner, not a Rust parser: `xtask` has no
//! parser dependency (adding one is the maintainer's decision). The scanner removes comments and
//! literals, splits the rest into tokens, and reads only item structure (attributes, visibility,
//! `struct`, `enum`, `union`, `mod`, `use`, `macro_rules!`), skipping every other item as a
//! balanced token group. A construct it cannot read, such as an unbalanced bracket or a module
//! file that does not exist, is a problem rather than a silent pass.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::repo;

/// The codec crates whose public API is checked (Layers 0 and 1).
pub const CRATES: &[&str] = &[
    "aldis-core",
    "aldis-time",
    "aldis-spp",
    "aldis-epp",
    "aldis-tm",
    "aldis-tc",
    "aldis-aos",
    "aldis-uslp",
];

/// The exemption list, relative to the repository root.
pub const EXEMPTIONS: &str = "xtask/rs020-exemptions.toml";

/// The primitive integer types.
const INTS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
];

// ---------------------------------------------------------------------------------------------
// Lexing

/// Whether `ch` can be part of an identifier.
fn ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// End (exclusive) of the block comment that starts at `i`; block comments nest.
fn block_comment_end(c: &[char], mut i: usize) -> usize {
    let mut depth = 0_usize;
    while i < c.len() {
        if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            depth += 1;
            i += 2;
        } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    i
}

/// End (exclusive) of the raw string (`r"…"`, `r#"…"#`) whose `r` is at `i`, if one starts
/// there.
fn raw_string_end(c: &[char], i: usize) -> Option<usize> {
    let prefix_ok =
        i == 0 || !ident_char(c[i - 1]) || (c[i - 1] == 'b' && (i == 1 || !ident_char(c[i - 2])));
    let hashes = c[i + 1..].iter().take_while(|&&h| h == '#').count();
    if !prefix_ok || c.get(i + 1 + hashes) != Some(&'"') {
        return None;
    }
    let mut j = i + hashes + 2;
    while j < c.len() {
        if c[j] == '"' && j + hashes < c.len() && c[j + 1..=j + hashes].iter().all(|&h| h == '#') {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(j)
}

/// End (exclusive) of the string that starts at `i`, with its escapes.
const fn string_end(c: &[char], mut i: usize) -> usize {
    i += 1;
    while i < c.len() {
        match c[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// End (exclusive) of the character literal at `i` (`'x'`, `'\n'`, `'\u{…}'`), or `None` for
/// a lifetime.
fn char_end(c: &[char], i: usize) -> Option<usize> {
    let mut j = match (c.get(i + 1), c.get(i + 2)) {
        (Some('\\'), _) => i + 3,
        (_, Some('\'')) => i + 2,
        _ => return None,
    };
    while j < c.len() && c[j] != '\'' {
        j += 1;
    }
    Some(j + 1)
}

/// Replaces comments and string and character literals by spaces, keeping line breaks, so that
/// the tokens left are code.
fn strip(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        let byte_prefix = ch == 'b'
            && (i == 0 || !ident_char(c[i - 1]))
            && (matches!(next, Some('"' | '\''))
                || (next == Some('r') && raw_string_end(&c, i + 1).is_some()));
        let end = match ch {
            '/' if next == Some('/') => c[i..]
                .iter()
                .position(|&x| x == '\n')
                .map_or(c.len(), |n| i + n),
            '/' if next == Some('*') => block_comment_end(&c, i),
            'r' => raw_string_end(&c, i).unwrap_or(i),
            '"' => string_end(&c, i),
            '\'' => char_end(&c, i).unwrap_or(i),
            // The prefix of a byte string or byte literal, removed with it.
            _ if byte_prefix => i + 1,
            _ => i,
        };
        if end == i {
            out.push(ch);
            i += 1;
        } else {
            for &x in &c[i..end.min(c.len())] {
                out.push(if x == '\n' { '\n' } else { ' ' });
            }
            i = end;
        }
    }
    out
}

/// One token: an identifier, keyword, number, lifetime, or punctuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tok {
    pub(crate) text: String,
    pub(crate) line: usize,
}

/// The tokens of `text`, with 1-based line numbers. `::`, `->`, and `=>` are single tokens; a
/// macro variable (`$name`) is one identifier token; other punctuation is one character each.
pub(crate) fn tokens(text: &str) -> Vec<Tok> {
    let stripped = strip(text);
    let c: Vec<char> = stripped.chars().collect();
    let ident = ident_char;
    let mut out = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ident(ch)
            || ((ch == '$' || ch == '\'') && c.get(i + 1).is_some_and(|&n| ident(n)))
        {
            let start = i;
            i += 1;
            while i < c.len() && ident(c[i]) {
                i += 1;
            }
            out.push(Tok {
                text: c[start..i].iter().collect(),
                line,
            });
        } else {
            let pair: String = c[i..(i + 2).min(c.len())].iter().collect();
            let len = if matches!(pair.as_str(), "::" | "->" | "=>") {
                2
            } else {
                1
            };
            out.push(Tok {
                text: c[i..i + len].iter().collect(),
                line,
            });
            i += len;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Item structure

/// The visibility of an item or field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vis {
    /// `pub`, or a macro's `$vis`.
    Pub,
    /// `pub(crate)`, `pub(super)`, `pub(in …)`.
    Restricted,
    /// No visibility.
    Private,
}

/// A field of a struct, union, or enum variant.
#[derive(Debug, Clone)]
struct Field {
    /// The field's name, or its index in a tuple.
    name: String,
    vis: Vis,
    ty: Vec<String>,
    line: usize,
}

/// A struct, union, or enum.
#[derive(Debug, Clone)]
struct TypeDef {
    name: String,
    vis: Vis,
    /// Struct and union fields.
    fields: Vec<Field>,
    /// Enum variants, each with its payload fields (public with the enum).
    variants: Vec<(String, Vec<Field>)>,
}

/// A `mod` declaration.
#[derive(Debug, Clone)]
struct ModDecl {
    name: String,
    vis: Vis,
    line: usize,
    /// The items of an inline module (`mod x { … }`); `None` for a file module (`mod x;`).
    inline: Option<Items>,
}

/// The items of a module (or of a `macro_rules!` body) that the check reads.
#[derive(Debug, Clone, Default)]
struct Items {
    types: Vec<TypeDef>,
    /// `use` declarations: visibility and the tokens between `use` and `;`.
    uses: Vec<(Vis, Vec<String>)>,
    mods: Vec<ModDecl>,
    /// `macro_rules!` definitions: name and the items of each arm's body.
    macros: Vec<(String, Items)>,
}

/// Index of the bracket that closes the one at `open`.
pub(crate) fn group_end(t: &[Tok], open: usize) -> Result<usize, String> {
    if !t
        .get(open)
        .is_some_and(|tok| matches!(tok.text.as_str(), "(" | "[" | "{"))
    {
        let line = t.get(open).map_or(0, |t| t.line);
        return Err(format!("line {line}: expected a bracket"));
    }
    let mut stack = Vec::new();
    for (i, tok) in t.iter().enumerate().skip(open) {
        match tok.text.as_str() {
            "(" | "[" | "{" => stack.push(tok.text.clone()),
            ")" | "]" | "}" => {
                let want = match tok.text.as_str() {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                if stack.pop().as_deref() != Some(want) {
                    return Err(format!("line {}: unbalanced `{}`", tok.line, tok.text));
                }
                if stack.is_empty() {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    let line = t.get(open).map_or(0, |t| t.line);
    Err(format!("line {line}: unclosed bracket"))
}

fn is(t: &[Tok], i: usize, text: &str) -> bool {
    t.get(i).is_some_and(|tok| tok.text == text)
}

/// Splits `t` at the commas outside brackets, including angle brackets.
fn split_commas(t: &[Tok]) -> Result<Vec<&[Tok]>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut angle = 0_i32;
    let mut i = 0;
    while i < t.len() {
        match t[i].text.as_str() {
            "(" | "[" | "{" => i = group_end(t, i)?,
            "<" => angle += 1,
            ">" => angle -= 1,
            "," if angle == 0 => {
                parts.push(&t[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&t[start..]);
    Ok(parts.into_iter().filter(|p| !p.is_empty()).collect())
}

/// Whether attribute tokens (between `#[` and `]`) gate the item on `test` or `kani`.
pub(crate) fn test_only(attr: &[Tok]) -> bool {
    let words: Vec<&str> = attr.iter().map(|t| t.text.as_str()).collect();
    match words.as_slice() {
        ["cfg", "(", "test" | "kani", ")"] => true,
        ["cfg", "(", "all", "(", rest @ ..] => rest.iter().any(|w| matches!(*w, "test" | "kani")),
        _ => false,
    }
}

/// Skips the attributes at `*i`; returns whether one gates the item on `test` or `kani`.
fn attributes(t: &[Tok], i: &mut usize) -> Result<bool, String> {
    let mut gated = false;
    while is(t, *i, "#") {
        let open = if is(t, *i + 1, "!") { *i + 2 } else { *i + 1 };
        if !is(t, open, "[") {
            break;
        }
        let end = group_end(t, open)?;
        gated |= test_only(&t[open + 1..end]);
        *i = end + 1;
    }
    Ok(gated)
}

/// Reads the visibility at `*i`.
fn visibility(t: &[Tok], i: &mut usize) -> Result<Vis, String> {
    if is(t, *i, "pub") {
        if is(t, *i + 1, "(") {
            *i = group_end(t, *i + 1)? + 1;
            return Ok(Vis::Restricted);
        }
        *i += 1;
        return Ok(Vis::Pub);
    }
    let item_keyword = t.get(*i + 1).is_some_and(|n| {
        matches!(
            n.text.as_str(),
            "struct" | "enum" | "union" | "mod" | "use" | "fn" | "const" | "static" | "type"
        )
    });
    if t.get(*i).is_some_and(|tok| tok.text.starts_with('$')) && item_keyword {
        *i += 1;
        return Ok(Vis::Pub);
    }
    Ok(Vis::Private)
}

/// Skips generic parameters at `*i` (`<…>`), if any.
fn generics(t: &[Tok], i: &mut usize) -> Result<(), String> {
    if !is(t, *i, "<") {
        return Ok(());
    }
    let mut depth = 0_i32;
    while *i < t.len() {
        match t[*i].text.as_str() {
            "(" | "[" | "{" => *i = group_end(t, *i)?,
            "<" => depth += 1,
            ">" => {
                depth -= 1;
                if depth == 0 {
                    *i += 1;
                    return Ok(());
                }
            }
            _ => {}
        }
        *i += 1;
    }
    Err("unclosed generic parameters".to_owned())
}

/// Skips a `where` clause at `*i` up to the `{`, `(`, or `;` that follows it.
fn where_clause(t: &[Tok], i: &mut usize) -> Result<(), String> {
    if !is(t, *i, "where") {
        return Ok(());
    }
    while *i < t.len() && !matches!(t[*i].text.as_str(), "{" | ";") {
        if matches!(t[*i].text.as_str(), "(" | "[") {
            *i = group_end(t, *i)?;
        }
        *i += 1;
    }
    Ok(())
}

/// The fields between a pair of brackets: named (`{ a: T }`) or tuple (`(T, U)`).
fn fields(t: &[Tok], named: bool, enum_payload: bool) -> Result<Vec<Field>, String> {
    let mut out = Vec::new();
    for (index, part) in split_commas(t)?.into_iter().enumerate() {
        let mut k = 0;
        attributes(part, &mut k)?;
        let vis = if enum_payload {
            visibility(part, &mut k)?;
            Vis::Pub
        } else {
            visibility(part, &mut k)?
        };
        let name = if named {
            let name = part.get(k).map(|t| t.text.clone()).unwrap_or_default();
            if !is(part, k + 1, ":") {
                let line = part.get(k).map_or(0, |t| t.line);
                return Err(format!(
                    "line {line}: expected `name: Type` in a field list"
                ));
            }
            k += 2;
            name
        } else {
            index.to_string()
        };
        let Some(first) = part.get(k) else {
            return Err("a field without a type".to_owned());
        };
        out.push(Field {
            name,
            vis,
            ty: part[k..].iter().map(|t| t.text.clone()).collect(),
            line: first.line,
        });
    }
    Ok(out)
}

/// Parses a struct or union whose keyword is at `*i`; leaves `*i` after it.
fn struct_def(t: &[Tok], i: &mut usize, vis: Vis) -> Result<TypeDef, String> {
    let name = t.get(*i + 1).map(|t| t.text.clone()).unwrap_or_default();
    *i += 2;
    generics(t, i)?;
    where_clause(t, i)?;
    let mut def = TypeDef {
        name,
        vis,
        fields: Vec::new(),
        variants: Vec::new(),
    };
    match t.get(*i).map(|t| t.text.as_str()) {
        Some(";") => *i += 1,
        Some(open @ ("(" | "{")) => {
            let named = open == "{";
            let end = group_end(t, *i)?;
            def.fields = fields(&t[*i + 1..end], named, false)?;
            *i = end + 1;
            if !named {
                where_clause(t, i)?;
                if is(t, *i, ";") {
                    *i += 1;
                }
            }
        }
        _ => {
            let line = t.get(*i).map_or(0, |t| t.line);
            return Err(format!("line {line}: cannot read struct `{}`", def.name));
        }
    }
    Ok(def)
}

/// Parses an enum whose keyword is at `*i`; leaves `*i` after it.
fn enum_def(t: &[Tok], i: &mut usize, vis: Vis) -> Result<TypeDef, String> {
    let name = t.get(*i + 1).map(|t| t.text.clone()).unwrap_or_default();
    *i += 2;
    generics(t, i)?;
    where_clause(t, i)?;
    if !is(t, *i, "{") {
        let line = t.get(*i).map_or(0, |t| t.line);
        return Err(format!("line {line}: cannot read enum `{name}`"));
    }
    let end = group_end(t, *i)?;
    let mut variants = Vec::new();
    for part in split_commas(&t[*i + 1..end])? {
        let mut k = 0;
        attributes(part, &mut k)?;
        let Some(vname) = part.get(k) else {
            continue;
        };
        let payload = match part.get(k + 1).map(|t| t.text.as_str()) {
            Some(open @ ("(" | "{")) => {
                let close = group_end(part, k + 1)?;
                fields(&part[k + 2..close], open == "{", true)?
            }
            _ => Vec::new(),
        };
        variants.push((vname.text.clone(), payload));
    }
    *i = end + 1;
    Ok(TypeDef {
        name,
        vis,
        fields: Vec::new(),
        variants,
    })
}

/// Removes the repetition wrappers of a macro body (`$( … ) sep? op`), keeping their contents,
/// so that the body reads as items.
fn unwrap_repetitions(t: &[Tok]) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i].text == "$" && is(t, i + 1, "(") {
            let end = group_end(t, i + 1)?;
            out.extend(unwrap_repetitions(&t[i + 2..end])?);
            i = end + 1;
            if matches!(t.get(i).map(|t| t.text.as_str()), Some("*" | "+" | "?")) {
                i += 1;
            } else if matches!(t.get(i + 1).map(|t| t.text.as_str()), Some("*" | "+" | "?")) {
                i += 2;
            }
        } else {
            out.push(t[i].clone());
            i += 1;
        }
    }
    Ok(out)
}

/// Parses the arms of a `macro_rules!` body (between its brackets).
fn macro_arms(t: &[Tok]) -> Result<Items, String> {
    let mut all = Items::default();
    let mut i = 0;
    while i < t.len() {
        if is(t, i, ";") {
            i += 1;
            continue;
        }
        let matcher_end = group_end(t, i)?;
        if !is(t, matcher_end + 1, "=>") {
            return Err(format!("line {}: cannot read a macro arm", t[i].line));
        }
        let body = matcher_end + 2;
        let body_end = group_end(t, body)?;
        let items = parse_items(&unwrap_repetitions(&t[body + 1..body_end])?)?;
        all.types.extend(items.types);
        all.macros.extend(items.macros);
        i = body_end + 1;
    }
    Ok(all)
}

/// Skips an item that the check does not read (functions, impls, traits, constants, macro
/// invocations, …) at `*i`.
fn skip_item(t: &[Tok], i: &mut usize) -> Result<(), String> {
    let mut k = *i;
    while matches!(
        t.get(k).map(|t| t.text.as_str()),
        Some("unsafe" | "async" | "extern" | "default")
    ) {
        k += 1;
    }
    let ends_at_semicolon = match t.get(k).map(|t| t.text.as_str()) {
        Some("const") => !matches!(
            t.get(k + 1).map(|t| t.text.as_str()),
            Some("fn" | "unsafe" | "async" | "extern")
        ),
        Some("static" | "type" | "crate") => true,
        _ => false,
    };
    while *i < t.len() {
        match t[*i].text.as_str() {
            ";" => {
                *i += 1;
                return Ok(());
            }
            "(" | "[" | "{" => {
                let brace = t[*i].text == "{";
                *i = group_end(t, *i)? + 1;
                if brace && !ends_at_semicolon {
                    return Ok(());
                }
            }
            _ => *i += 1,
        }
    }
    Ok(())
}

/// Parses the items of a module body (or of a macro body).
fn parse_items(t: &[Tok]) -> Result<Items, String> {
    let mut items = Items::default();
    let mut i = 0;
    while i < t.len() {
        let gated = attributes(t, &mut i)?;
        if i >= t.len() {
            break;
        }
        let vis = visibility(t, &mut i)?;
        let start = i;
        match t.get(i).map(|t| t.text.as_str()) {
            Some("struct" | "union") => {
                let def = struct_def(t, &mut i, vis)?;
                if !gated {
                    items.types.push(def);
                }
            }
            Some("enum") => {
                let def = enum_def(t, &mut i, vis)?;
                if !gated {
                    items.types.push(def);
                }
            }
            Some("mod") => {
                let name = t.get(i + 1).map(|t| t.text.clone()).unwrap_or_default();
                let line = t[i].line;
                let inline = if is(t, i + 2, "{") {
                    let end = group_end(t, i + 2)?;
                    let body = parse_items(&t[i + 3..end])?;
                    i = end + 1;
                    Some(body)
                } else {
                    i += 3;
                    None
                };
                if !gated {
                    items.mods.push(ModDecl {
                        name,
                        vis,
                        line,
                        inline,
                    });
                }
            }
            Some("use") => {
                let mut end = i;
                while end < t.len() && t[end].text != ";" {
                    end += 1;
                }
                if !gated {
                    items
                        .uses
                        .push((vis, t[i + 1..end].iter().map(|t| t.text.clone()).collect()));
                }
                i = end + 1;
            }
            Some("macro_rules") if is(t, i + 1, "!") => {
                let name = t.get(i + 2).map(|t| t.text.clone()).unwrap_or_default();
                let end = group_end(t, i + 3)?;
                let body = macro_arms(&t[i + 4..end])?;
                if !gated {
                    items.macros.push((name, body));
                }
                i = end + 1;
                if is(t, i, ";") {
                    i += 1;
                }
            }
            _ => skip_item(t, &mut i)?,
        }
        if i == start {
            // Every branch consumes at least one token; this guards the loop all the same.
            i += 1;
        }
    }
    Ok(items)
}

// ---------------------------------------------------------------------------------------------
// Modules, reachability, and findings

/// A module of a crate.
#[derive(Debug, Clone)]
struct Module {
    /// Path segments below the crate root.
    path: Vec<String>,
    /// Whether every module from the root to this one is `pub`.
    public: bool,
    /// The file that holds the module's items, relative to the repository root.
    file: String,
    items: Items,
}

/// Reads every module of a crate from `src` (`crates/<name>/src`), following `mod` declarations
/// from `lib.rs`; `read` returns a file's text, or `None` if it does not exist.
fn modules(src: &str, read: &dyn Fn(&str) -> Option<String>) -> Result<Vec<Module>, String> {
    let root = format!("{src}/lib.rs");
    let text = read(&root).ok_or_else(|| format!("{root}: not found"))?;
    let items = parse_items(&tokens(&text)).map_err(|e| format!("{root}: {e}"))?;
    let mut out = Vec::new();
    let mut stack = vec![(Vec::new(), true, root, src.to_owned(), items)];
    while let Some((path, public, file, dir, items)) = stack.pop() {
        for m in &items.mods {
            let mut child = path.clone();
            child.push(m.name.clone());
            let child_public = public && m.vis == Vis::Pub;
            let child_dir = format!("{dir}/{}", m.name);
            if let Some(inline) = &m.inline {
                stack.push((child, child_public, file.clone(), child_dir, inline.clone()));
                continue;
            }
            let flat = format!("{dir}/{}.rs", m.name);
            let nested = format!("{child_dir}/mod.rs");
            let (child_file, text) = match read(&flat) {
                Some(text) => (flat, text),
                None => match read(&nested) {
                    Some(text) => (nested, text),
                    None => {
                        return Err(format!(
                            "{file}:{}: module `{}` has no file {flat} or {nested}",
                            m.line, m.name
                        ));
                    }
                },
            };
            let child_items =
                parse_items(&tokens(&text)).map_err(|e| format!("{child_file}: {e}"))?;
            stack.push((child, child_public, child_file, child_dir, child_items));
        }
        out.push(Module {
            path,
            public,
            file,
            items,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The paths a `use` declaration names, each with the name it binds (`*` for a glob).
fn use_paths(toks: &[String]) -> Vec<(Vec<String>, String)> {
    fn tree(
        t: &[String],
        i: &mut usize,
        prefix: &mut Vec<String>,
        out: &mut Vec<(Vec<String>, String)>,
    ) {
        let depth = prefix.len();
        while let Some(tok) = t.get(*i) {
            match tok.as_str() {
                "{" => {
                    *i += 1;
                    while t.get(*i).is_some_and(|x| x != "}") {
                        tree(t, i, prefix, out);
                        if t.get(*i).is_some_and(|x| x == ",") {
                            *i += 1;
                        }
                    }
                    *i += 1;
                    break;
                }
                "*" => {
                    *i += 1;
                    let mut p = prefix.clone();
                    p.push("*".to_owned());
                    out.push((p, "*".to_owned()));
                    break;
                }
                "::" => *i += 1,
                seg => {
                    prefix.push(seg.to_owned());
                    *i += 1;
                    if t.get(*i).is_some_and(|x| x == "::") {
                        *i += 1;
                        continue;
                    }
                    let mut p = prefix.clone();
                    if p.last().is_some_and(|s| s == "self") {
                        p.pop();
                    }
                    let alias = if t.get(*i).is_some_and(|x| x == "as") {
                        *i += 2;
                        t.get(*i - 1).cloned().unwrap_or_default()
                    } else {
                        p.last().cloned().unwrap_or_default()
                    };
                    out.push((p, alias));
                    break;
                }
            }
        }
        prefix.truncate(depth);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        tree(toks, &mut i, &mut Vec::new(), &mut out);
        if toks.get(i).is_some_and(|x| x == ",") {
            i += 1;
        }
    }
    out
}

/// A bare integer in the public API.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Finding {
    /// `crate::module::Type.field` or `crate::module::Type::Variant.field`.
    item: String,
    file: String,
    line: usize,
    ty: String,
}

/// Whether a type, as tokens, is a bare integer: a primitive integer, a reference to one,
/// `Option` of one, or a tuple holding one. A macro variable counts (`allow_var`).
fn bare_integer(ty: &[String], allow_var: bool) -> bool {
    let words: Vec<&str> = ty.iter().map(String::as_str).collect();
    match words.as_slice() {
        [t] => INTS.contains(t) || (allow_var && t.starts_with('$') && *t != "$crate"),
        ["&", rest @ ..] => {
            let rest = match rest {
                [l, rest @ ..] if l.starts_with('\'') => rest,
                _ => rest,
            };
            let rest = rest.strip_prefix(&["mut"]).unwrap_or(rest);
            bare_integer(&ty[ty.len() - rest.len()..], allow_var)
        }
        ["Option", "<", .., ">"] => bare_integer(&ty[2..ty.len() - 1], allow_var),
        ["core" | "std", "::", "option", "::", "Option", "<", .., ">"] => {
            bare_integer(&ty[6..ty.len() - 1], allow_var)
        }
        ["(", .., ")"] => {
            let inner: Vec<Tok> = ty[1..ty.len() - 1]
                .iter()
                .map(|text| Tok {
                    text: text.clone(),
                    line: 0,
                })
                .collect();
            split_commas(&inner).is_ok_and(|parts| {
                parts.iter().any(|p| {
                    let p: Vec<String> = p.iter().map(|t| t.text.clone()).collect();
                    bare_integer(&p, allow_var)
                })
            })
        }
        _ => false,
    }
}

/// The findings of one type, at path `path`.
fn type_findings(def: &TypeDef, path: &str, file: &str, in_macro: bool, out: &mut Vec<Finding>) {
    let mut push = |item: String, f: &Field| {
        out.push(Finding {
            item,
            file: file.to_owned(),
            line: f.line,
            ty: f.ty.join(" "),
        });
    };
    for f in &def.fields {
        if f.vis == Vis::Pub && bare_integer(&f.ty, in_macro) {
            push(format!("{path}.{}", f.name), f);
        }
    }
    for (variant, payload) in &def.variants {
        for f in payload {
            if bare_integer(&f.ty, in_macro) {
                push(format!("{path}::{variant}.{}", f.name), f);
            }
        }
    }
}

/// The findings of the types defined by `macro_rules!` bodies, which count wherever they are.
fn macro_findings(macros: &[(String, Items)], prefix: &str, file: &str, out: &mut Vec<Finding>) {
    for (name, body) in macros {
        let at = format!("{prefix}::{name}!");
        for def in body.types.iter().filter(|d| d.vis == Vis::Pub) {
            type_findings(def, &format!("{at}::{}", def.name), file, true, out);
        }
        macro_findings(&body.macros, &at, file, out);
    }
}

/// What a path of a `pub use` in module `m` names in this crate.
enum Target {
    /// A type, by its name in the module at this path.
    Type(Vec<String>, String),
    /// Every type of the module at the first path, re-exported under the second (a glob, or a
    /// module re-exported by name).
    Module(Vec<String>, Vec<String>),
}

/// Resolves `target`, bound to `alias`, against the module paths of the crate; `None` for a
/// path into another crate.
fn resolve_use(
    m: &Module,
    target: &[String],
    alias: &str,
    modules: &BTreeSet<&[String]>,
) -> Option<Target> {
    let (last, dirs) = target.split_last()?;
    let child = |name: &str| {
        let mut p = m.path.clone();
        p.push(name.to_owned());
        p
    };
    let mut at = m.path.clone();
    let mut segs = dirs.iter();
    match dirs.first().map(String::as_str) {
        Some("crate") => {
            at.clear();
            segs.next();
        }
        Some("self") => {
            segs.next();
        }
        // Another crate, or (for a single segment) an item that is no local module or type.
        Some(first) if first != "super" && !modules.contains(child(first).as_slice()) => {
            return None;
        }
        None if !modules.contains(child(last).as_slice())
            && !m.items.types.iter().any(|d| &d.name == last) =>
        {
            return None;
        }
        _ => {}
    }
    for s in segs {
        if s == "super" {
            at.pop();
        } else {
            at.push(s.clone());
        }
    }
    if last == "*" {
        return Some(Target::Module(at, m.path.clone()));
    }
    let mut sub = at.clone();
    sub.push(last.clone());
    if modules.contains(sub.as_slice()) {
        return Some(Target::Module(sub, child(alias)));
    }
    Some(Target::Type(at, last.clone()))
}

/// Every bare integer in the public API of crate `krate` (`aldis-core`), sorted by item.
fn crate_findings(
    krate: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Finding>, String> {
    let lib = krate.replace('-', "_");
    let mods = modules(&format!("crates/{krate}/src"), read)?;
    let path_of = |prefix: &[String], name: &str| {
        let mut p = lib.clone();
        for s in prefix {
            p.push_str("::");
            p.push_str(s);
        }
        if !name.is_empty() {
            p.push_str("::");
            p.push_str(name);
        }
        p
    };
    let paths: BTreeSet<&[String]> = mods.iter().map(|m| m.path.as_slice()).collect();
    let position = |path: &[String]| mods.iter().position(|m| m.path == path);

    // Reachable types, by index of the module and of the type, with their public path: the
    // definition's path if every module to it is public, else that of a re-export.
    let mut reachable: BTreeMap<(usize, usize), String> = BTreeMap::new();
    for (mi, m) in mods.iter().enumerate().filter(|(_, m)| m.public) {
        for (ti, def) in m.items.types.iter().enumerate() {
            if def.vis == Vis::Pub {
                reachable.insert((mi, ti), path_of(&m.path, &def.name));
            }
        }
    }
    for m in mods.iter().filter(|m| m.public) {
        for (_, toks) in m.items.uses.iter().filter(|(v, _)| *v == Vis::Pub) {
            for (target, alias) in use_paths(toks) {
                let (module, name, prefix) = match resolve_use(m, &target, &alias, &paths) {
                    Some(Target::Type(module, name)) => (module, Some(name), m.path.clone()),
                    Some(Target::Module(module, prefix)) => (module, None, prefix),
                    None => continue,
                };
                let Some(mi) = position(&module) else {
                    continue;
                };
                for (ti, def) in mods[mi].items.types.iter().enumerate() {
                    if def.vis != Vis::Pub {
                        continue;
                    }
                    let path = match &name {
                        None => path_of(&prefix, &def.name),
                        Some(n) if *n == def.name => path_of(&prefix, &alias),
                        Some(_) => continue,
                    };
                    reachable.entry((mi, ti)).or_insert(path);
                }
            }
        }
    }

    let mut out = Vec::new();
    for ((mi, ti), path) in &reachable {
        let m = &mods[*mi];
        type_findings(&m.items.types[*ti], path, &m.file, false, &mut out);
    }
    // Macros count wherever they are defined, inline modules included.
    for m in &mods {
        macro_findings(&m.items.macros, &path_of(&m.path, ""), &m.file, &mut out);
    }
    out.sort_by(|a, b| a.item.cmp(&b.item));
    out.dedup_by(|a, b| a.item == b.item);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Exemptions

/// One entry of the exemption list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Exemption {
    item: String,
    reason: String,
    line: usize,
}

/// Parses the exemption list: `[[exemption]]` tables with `item` and `reason` (a line-based
/// subset of TOML, as `coverage-waivers.toml` is read, to avoid a TOML dependency). Returns the
/// entries and the problems found in the file itself.
fn exemptions(text: &str) -> (Vec<Exemption>, Vec<String>) {
    let mut entries: Vec<(usize, Vec<(String, String)>)> = Vec::new();
    let mut problems = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let at = format!("{EXEMPTIONS}:{}", n + 1);
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[exemption]]" {
            entries.push((n + 1, Vec::new()));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            problems.push(format!(
                "{at}: expected `key = \"value\"` or `[[exemption]]`"
            ));
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let Some(value) = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .filter(|v| !v.contains('"'))
        else {
            problems.push(format!("{at}: `{key}` must be one string on one line"));
            continue;
        };
        if !matches!(key, "item" | "reason") {
            problems.push(format!(
                "{at}: unknown key `{key}` (use `item` and `reason`)"
            ));
            continue;
        }
        let Some((_, entry)) = entries.last_mut() else {
            problems.push(format!("{at}: `{key}` outside an [[exemption]] table"));
            continue;
        };
        if entry.iter().any(|(k, _)| k == key) {
            problems.push(format!("{at}: `{key}` given twice in one exemption"));
            continue;
        }
        entry.push((key.to_owned(), value.to_owned()));
    }
    let mut out: Vec<Exemption> = Vec::new();
    for (line, entry) in entries {
        let at = format!("{EXEMPTIONS}:{line}");
        let get = |k: &str| {
            entry
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.trim().to_owned())
        };
        let Some(item) = get("item").filter(|i| !i.is_empty()) else {
            problems.push(format!("{at}: an exemption without an `item`"));
            continue;
        };
        let reason = get("reason").unwrap_or_default();
        if reason.is_empty() {
            problems.push(format!("{at}: the exemption of {item} gives no reason"));
        }
        if out.iter().any(|e| e.item == item) {
            problems.push(format!("{at}: {item} is exempted twice"));
            continue;
        }
        out.push(Exemption { item, reason, line });
    }
    (out, problems)
}

/// The problems: every finding without an exemption, and every exemption that is malformed or
/// matches no finding. Returns the problems and the number of exempt findings.
fn problems(findings: &[Finding], list: &str) -> (Vec<String>, usize) {
    let (exempt, mut problems) = exemptions(list);
    let items: BTreeSet<&str> = exempt.iter().map(|e| e.item.as_str()).collect();
    let mut exempted = 0;
    for f in findings {
        if items.contains(f.item.as_str()) {
            exempted += 1;
        } else {
            problems.push(format!(
                "{}:{}: RS-020 {} is a bare integer (`{}`): give it a type with a checked \
                 constructor, or exempt it with a reason in {EXEMPTIONS}",
                f.file, f.line, f.item, f.ty
            ));
        }
    }
    let found: BTreeSet<&str> = findings.iter().map(|f| f.item.as_str()).collect();
    for e in &exempt {
        if !found.contains(e.item.as_str()) {
            problems.push(format!(
                "{EXEMPTIONS}:{}: {} matches no bare integer in the public API; remove the \
                 exemption",
                e.line, e.item
            ));
        }
    }
    (problems, exempted)
}

/// `xtask rs020`.
pub fn check() -> Result<usize, String> {
    let root = repo::root();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let mut findings = Vec::new();
    for krate in CRATES {
        findings.extend(crate_findings(krate, &read)?);
    }
    let list = repo::read(&root.join(EXEMPTIONS))?;
    let (problems, exempted) = problems(&findings, &list);
    let mut report = String::new();
    for p in &problems {
        let _ = writeln!(report, "error: {p}");
    }
    print!("{report}");
    println!(
        "rs020: {} crate(s), {} bare integer(s) in the public API, {exempted} exempt, {} \
         problem(s)",
        CRATES.len(),
        findings.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The findings of a crate `aldis-x` made of `files` (paths under `crates/aldis-x/src/`).
    fn scan(files: &[(&str, &str)]) -> Vec<String> {
        let files: BTreeMap<String, String> = files
            .iter()
            .map(|(p, t)| (format!("crates/aldis-x/src/{p}"), (*t).to_owned()))
            .collect();
        crate_findings("aldis-x", &|rel| files.get(rel).cloned())
            .unwrap()
            .into_iter()
            .map(|f| f.item)
            .collect()
    }

    #[test]
    fn finds_bare_integers_in_enum_payloads() {
        // req: RS-020
        let lib = "
            pub mod bpdu;
            /// Doc with a struct Fake(pub u8) in it.
            pub enum Pointer { ValidBits(u16), AllValid, Idle = 3 }
            pub enum Pair<'a> { Both(Kind, Option<u8>), Data(&'a [u8]), Arr([u8; 4]) }
            pub enum Named { At { offset: usize, kind: Kind }, Ref(&'static u32), Tup((Kind, i64)) }
            pub(crate) enum Hidden { V(u8) }
            enum Private { V(u8) }
        ";
        let bpdu = "#[derive(Debug)]\npub enum Bitstream<T: Into<u8>> where T: Copy {\n    #[doc = \"x\"]\n    Valid(T, u8),\n}\n";
        assert_eq!(
            scan(&[("lib.rs", lib), ("bpdu.rs", bpdu)]),
            [
                "aldis_x::Named::At.offset",
                "aldis_x::Named::Ref.0",
                "aldis_x::Named::Tup.0",
                "aldis_x::Pair::Both.1",
                "aldis_x::Pointer::ValidBits.0",
                "aldis_x::bpdu::Bitstream::Valid.1",
            ]
        );
    }

    #[test]
    fn finds_bare_integer_fields_of_public_types() {
        // req: RS-020
        let lib = r#"
            pub mod layout;
            pub struct Count(pub u16);
            pub struct Private(u16);
            pub struct Mixed(pub(crate) u8, pub Kind, pub usize);
            pub struct Layout<const N: usize> { pub frame_len: usize, len: u16, pub(crate) x: u8, pub kind: Kind }
            pub struct Unit;
            struct Hidden { pub x: u8 }
            pub(crate) struct Crate(pub u8);
            const S: &str = "pub struct InString(pub u8);";
            /* pub struct InComment(pub u8); */
            pub fn f() -> [u8; 4] { let s = Inner { x: 1 }; [0; 4] }
            impl Count { pub const fn get(self) -> u16 { self.0 } }
            pub union Raw { pub word: u32, pub bytes: [u8; 4] }
        "#;
        let layout =
            "pub struct Frame<'a> {\n    pub data: &'a [u8],\n    pub count: Option<u32>,\n}\n";
        assert_eq!(
            scan(&[("lib.rs", lib), ("layout.rs", layout)]),
            [
                "aldis_x::Count.0",
                "aldis_x::Layout.frame_len",
                "aldis_x::Mixed.2",
                "aldis_x::Raw.word",
                "aldis_x::layout::Frame.count",
            ]
        );
    }

    #[test]
    fn counts_only_items_reachable_from_the_crate_root() {
        // req: RS-020
        let lib = "
            pub mod upid;
            mod util;
            pub(crate) mod internal;
            pub use util::Exported;
            pub use self::util::{Renamed as Public, inner::*};
            #[cfg(test)]
            pub mod tests { pub struct T(pub u8); }
            #[cfg(kani)]
            mod proofs;
            pub mod inline { pub struct InlinePub(pub u8); mod deep { pub struct D(pub u8); } }
        ";
        let upid = "mod table;\npub use table::Upid;\n";
        let table = "pub enum Upid { Assigned(u8) }\npub enum NotExported { V(u8) }\n";
        let util = "pub mod inner;\npub struct Exported(pub u8);\npub struct Renamed(pub u8);\npub struct Unused(pub u8);\n";
        let inner = "pub struct Globbed(pub u8);\n";
        let internal = "pub struct Internal(pub u8);\n";
        assert_eq!(
            scan(&[
                ("lib.rs", lib),
                ("upid.rs", upid),
                ("upid/table.rs", table),
                ("util.rs", util),
                ("util/inner.rs", inner),
                ("internal.rs", internal),
            ]),
            [
                "aldis_x::Exported.0",
                "aldis_x::Globbed.0",
                "aldis_x::Public.0",
                "aldis_x::inline::InlinePub.0",
                "aldis_x::upid::Upid::Assigned.0",
            ]
        );
    }

    #[test]
    fn reads_the_types_that_macros_define() {
        // req: RS-020
        let lib = "
            macro_rules! bounded {
                ($(#[$doc:meta])* $name:ident, $repr:ty) => {
                    $(#[$doc])*
                    pub struct $name($repr);
                    pub struct Open(pub $repr, pub u8);
                    impl $name { pub const fn get(self) -> $repr { self.0 } }
                };
            }
            macro_rules! flags {
                ($vis:vis enum $name:ident { $( $flag:ident = $bit:literal ),+ }) => {
                    $vis enum $name { $( $flag, )+ }
                    pub enum Wide { $( $flag(u16), )* }
                }
            }
            bounded!(Hour, u8);
        ";
        assert_eq!(
            scan(&[("lib.rs", lib)]),
            [
                "aldis_x::bounded!::Open.0",
                "aldis_x::bounded!::Open.1",
                "aldis_x::flags!::Wide::$flag.0",
            ]
        );
    }

    #[test]
    fn unreadable_source_is_an_error() {
        // req: RS-020
        let missing = BTreeMap::from([(
            "crates/aldis-x/src/lib.rs".to_owned(),
            "pub mod gone;".to_owned(),
        )]);
        let e = crate_findings("aldis-x", &|rel| missing.get(rel).cloned()).unwrap_err();
        assert!(e.contains("module `gone` has no file"), "{e}");
        let open = BTreeMap::from([(
            "crates/aldis-x/src/lib.rs".to_owned(),
            "pub struct S { pub x: u8 ".to_owned(),
        )]);
        let e = crate_findings("aldis-x", &|rel| open.get(rel).cloned()).unwrap_err();
        assert!(e.contains("unclosed"), "{e}");
    }

    #[test]
    fn strips_comments_and_literals_but_keeps_lifetimes() {
        // req: RS-020
        let t: Vec<String> = tokens(
            "a /* x /* nested */ y */ b // c\n'x' '\\'' '\\u{1F600}' 'a r#\"q\"# br\"z\" b\"s\\\"\" e",
        )
        .into_iter()
        .map(|t| t.text)
        .collect();
        assert_eq!(t, ["a", "b", "'a", "e"]);
        let lines: Vec<usize> = tokens("x\n/* \n */ y\n\"\n\" z")
            .iter()
            .map(|t| t.line)
            .collect();
        assert_eq!(lines, [1, 3, 5]);
    }

    fn finding(item: &str) -> Finding {
        Finding {
            item: item.to_owned(),
            file: "crates/aldis-x/src/lib.rs".to_owned(),
            line: 7,
            ty: "u16".to_owned(),
        }
    }

    #[test]
    fn exemptions_silence_findings_with_a_reason() {
        // req: RS-020
        let findings = [
            finding("aldis_x::Layout.frame_len"),
            finding("aldis_x::Gap.0"),
        ];
        let list = "# comment\n\n[[exemption]]\nitem = \"aldis_x::Layout.frame_len\"\nreason = \"a length in octets with no range beyond usize\"\n";
        let (p, exempted) = problems(&findings, list);
        assert_eq!(exempted, 1);
        assert_eq!(
            p,
            [
                "crates/aldis-x/src/lib.rs:7: RS-020 aldis_x::Gap.0 is a bare integer (`u16`): give it a type with a checked constructor, or exempt it with a reason in xtask/rs020-exemptions.toml"
            ]
        );
    }

    #[test]
    fn exemptions_are_validated() {
        // req: RS-020
        let findings = [finding("aldis_x::A.0"), finding("aldis_x::B.0")];
        let list = "\
item = \"aldis_x::Outside.0\"
[[exemption]]
item = \"aldis_x::A.0\"
[[exemption]]
item = \"aldis_x::B.0\"
reason = \"  \"
[[exemption]]
item = \"aldis_x::Stale.0\"
reason = \"an offset\"
[[exemption]]
item = \"aldis_x::A.0\"
reason = \"twice\"
[[exemption]]
reason = \"no item\"
[[exemption]]
item = \"aldis_x::C.0\"
note = \"x\"
reason = 'single'
reason = \"x\"
reason = \"y\"
garbage
";
        let (p, _) = problems(&findings, list);
        let expected = [
            ":1: `item` outside an [[exemption]] table",
            ":17: unknown key `note`",
            ":18: `reason` must be one string on one line",
            ":20: `reason` given twice in one exemption",
            ":21: expected `key = \"value\"` or `[[exemption]]`",
            ":2: the exemption of aldis_x::A.0 gives no reason",
            ":4: the exemption of aldis_x::B.0 gives no reason",
            ":10: aldis_x::A.0 is exempted twice",
            ":13: an exemption without an `item`",
            ":7: aldis_x::Stale.0 matches no bare integer in the public API",
            ":15: aldis_x::C.0 matches no bare integer in the public API",
        ];
        assert_eq!(p.len(), expected.len(), "{p:#?}");
        for (got, want) in p.iter().zip(expected) {
            assert!(got.contains(want), "{got} does not contain {want}");
        }
    }

    #[test]
    fn the_codec_crates_read_cleanly() {
        // req: RS-020
        let root = repo::root();
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
        for krate in CRATES {
            let findings = crate_findings(krate, &read).unwrap();
            assert!(
                findings
                    .iter()
                    .all(|f| f.item.starts_with(&krate.replace('-', "_"))),
                "{krate}"
            );
        }
        let (_, problems) = exemptions(&repo::read(&root.join(EXEMPTIONS)).unwrap());
        assert_eq!(problems, Vec::<String>::new());
    }
}
