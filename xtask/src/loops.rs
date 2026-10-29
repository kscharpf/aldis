//! `xtask loops`: mechanical check of GEN-022 (every loop bounded) over the library code of the
//! crates.
//!
//! **The rule.** A loop is bounded *by construction*, and needs nothing, when it runs over a
//! slice, an array, a fixed container, or a range with an end: a `for` loop or an iterator
//! consumer whose source the scanner does not find unbounded, and the slice methods of
//! [`SLICE_LOOPS`]. Their bound is the length of what they iterate, which the type gives. A loop
//! needs an *annotation* when its bound is an argument: every `while`, `while let`, and `loop`,
//! and every `for` or consumer whose source holds an open range (`n..`) or one of
//! [`UNBOUNDED_SOURCES`]. The annotation is a comment block directly above the line of the loop:
//!
//! ```text
//! // bound: capacity N, each pass removes one item
//! while let Some(x) = self.pop() {
//! ```
//!
//! The block starts with `// bound:` and a non-empty reason (the bound and why the loop reaches
//! it) and may continue on `//` lines. `xtask loops` fails on a loop that needs an annotation
//! and has none, and on an annotation that is not directly above a loop. Recursion is not
//! checked here: the verify check `stack` fails on any call cycle (VER-011).
//!
//! **What counts as a loop.** In the library code of each crate of [`CRATES`] (the modules
//! reachable from `src/lib.rs`, without those and the items gated on `cfg(test)` or `cfg(kani)`):
//! - the loop expressions `for … in`, `while` (also `while let`), and `loop`;
//! - a call of an iterator method that consumes the iterator, and so runs a loop: [`CONSUMERS`];
//! - a call of a slice method that runs over the slice: [`SLICE_LOOPS`].
//!
//! Not found: iterator adaptors (`zip`, `map`, `rev`, …), which only run inside a consumer or a
//! `for`; `min` and `max`, which on integers are not loops; comparisons of slices and arrays (`==`);
//! array repeat expressions (`[x; N]`); derived implementations; and loops inside the functions of
//! other crates. The scanner is textual (the tokens of `rs020`), so a method of the crate's own
//! named like one of the lists counts too.
//!
//! **Identifiers.** A loop is named by its file (relative to the crate's `src/`), its item, and
//! its index, as `` `config.rs VcTable::new #2` ``. The item is the innermost function, with the
//! type of the `impl` or `trait` block it is in (`VcTable::new`, never the trait), a `const` or
//! `static` item for a loop in its initialiser, or `name!` for a loop in the body of a
//! `macro_rules!` definition. The index counts the loops of the item in source order from 1; a
//! labelled loop may be named by its label instead (`#'outer`). One span may name several loops
//! of an item (`` `writer.rs TcFrameWriter::start #1 #2` ``).

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

/// Sources of an iterator that have no end, or whose end the scanner cannot see.
pub const UNBOUNDED_SOURCES: &[&str] = &["repeat", "repeat_with", "cycle", "from_fn", "successors"];

/// The marker of a bound annotation.
pub const MARKER: &str = "// bound:";

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
    /// Why it needs a bound annotation, or `None` when it is bounded by construction.
    pub needs_bound: Option<&'static str>,
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
pub fn after_group(t: &[Tok], i: usize) -> usize {
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
pub fn after_item(t: &[Tok], mut i: usize) -> usize {
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
pub fn test_only(attr: &[Tok]) -> bool {
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
pub fn impl_type(t: &[Tok], mut i: usize) -> String {
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
pub fn header_end(t: &[Tok], mut i: usize) -> usize {
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

/// The tokens between `in` and the `{` of the `for` loop at `i`.
fn for_source(t: &[Tok], i: usize) -> &[Tok] {
    let mut j = i + 1;
    while j < t.len() && t[j].text != "in" {
        j = if matches!(t[j].text.as_str(), "(" | "[") {
            after_group(t, j)
        } else {
            j + 1
        };
    }
    let start = (j + 1).min(t.len());
    let mut end = start;
    while end < t.len() && t[end].text != "{" {
        end = if matches!(t[end].text.as_str(), "(" | "[") {
            after_group(t, end)
        } else {
            end + 1
        };
    }
    t.get(start..end).unwrap_or(&[])
}

/// The receiver expression of the method call whose `.` is at `dot`: the tokens back to the
/// start of the expression (an unmatched opening bracket, or a `;`, `=`, `,`, `=>`, or keyword
/// that ends one).
fn receiver(t: &[Tok], dot: usize) -> &[Tok] {
    let mut depth = 0_usize;
    let mut start = dot;
    while start > 0 {
        match t[start - 1].text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth == 0 => break,
            "(" | "[" | "{" => depth -= 1,
            ";" | "=" | "," | "=>" | "return" | "let" | "if" | "else" | "match" | "in"
                if depth == 0 =>
            {
                break;
            }
            _ => {}
        }
        start -= 1;
    }
    t.get(start..dot).unwrap_or(&[])
}

/// Why the source of an iteration is not bounded by construction: it holds an open range or one
/// of [`UNBOUNDED_SOURCES`].
fn unbounded_source(src: &[Tok]) -> Option<&'static str> {
    if src
        .iter()
        .any(|t| UNBOUNDED_SOURCES.contains(&t.text.as_str()))
    {
        return Some("unbounded iterator source");
    }
    // `..` is two `.` tokens; `..=` has an end, and so has `..` followed by an expression.
    let open_range = src.windows(2).enumerate().any(|(k, w)| {
        w[0].text == "."
            && w[1].text == "."
            && src
                .get(k + 2)
                .is_none_or(|n| matches!(n.text.as_str(), ")" | "]" | ","))
    });
    open_range.then_some("open range")
}

/// Why the loop of `kind` at token `i` needs a bound annotation, or `None` when its type bounds it.
fn needs_bound(t: &[Tok], i: usize, kind: &str) -> Option<&'static str> {
    match kind {
        "while" | "loop" => Some("not a for loop"),
        "for" => unbounded_source(for_source(t, i)),
        k if CONSUMERS.contains(&k) => unbounded_source(receiver(t, i)),
        _ => None,
    }
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
                needs_bound: needs_bound(&t, i, &kind),
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

/// The bound annotations of a file: `(line of the marker, line directly below the block,
/// reason)`. The loop must be on the second line.
fn annotations(src: &str) -> Vec<(usize, usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut n = 0;
    while n < lines.len() {
        let line = lines.get(n).map_or("", |l| l.trim_start());
        if let Some(reason) = line.strip_prefix(MARKER) {
            let mut text = reason.trim().to_owned();
            let mut end = n + 1;
            while let Some(next) = lines.get(end).map(|l| l.trim_start()) {
                let Some(more) = next.strip_prefix("//").filter(|m| {
                    !m.starts_with(['/', '!']) && !m.trim_start().starts_with("bound:")
                }) else {
                    break;
                };
                text.push(' ');
                text.push_str(more.trim());
                end += 1;
            }
            out.push((n + 1, end + 1, text.trim().to_owned()));
            n = end;
        } else {
            n += 1;
        }
    }
    out
}

/// The problems of one file: loops that need a bound annotation and have none, annotations
/// without a reason, and annotations not directly above a loop.
pub fn problems(prefix: &str, file: &str, src: &str) -> Vec<String> {
    let loops = scan(file, src);
    let file = format!("{prefix}{file}");
    let notes = annotations(src);
    let mut out = Vec::new();
    for l in &loops {
        if let Some(why) = l.needs_bound
            && !notes.iter().any(|(_, below, _)| *below == l.line)
        {
            out.push(format!(
                "{file}:{}: `{}` ({}, {why}) has no `{MARKER} <reason>` comment directly above it",
                l.line,
                l.id(),
                l.kind,
            ));
        }
    }
    for (at, below, text) in &notes {
        if text.is_empty() {
            out.push(format!("{file}:{at}: `{MARKER}` gives no reason"));
        }
        if !loops.iter().any(|l| l.line == *below) {
            out.push(format!(
                "{file}:{at}: `{MARKER}` is not directly above a loop"
            ));
        }
    }
    out
}

/// The problems of a crate's library code.
fn crate_problems(krate: &str) -> Result<Vec<String>, String> {
    let src = repo::root().join("crates").join(krate).join("src");
    Ok(modules(&src)?
        .iter()
        .flat_map(|(rel, text)| problems(&format!("crates/{krate}/src/"), rel, text))
        .collect())
}

/// `xtask loops [list [<crate>...]]`: GEN-022 over the library code. It fails on a loop that
/// needs a bound annotation and has none, and on an annotation not directly above a loop;
/// `list` also prints every loop found.
pub fn run(args: &[&str]) -> Result<usize, String> {
    let (list, names) = match args {
        ["list", rest @ ..] => (true, rest),
        rest => (false, rest),
    };
    let crates: Vec<&str> = if names.is_empty() {
        CRATES.to_vec()
    } else {
        names.to_vec()
    };
    let mut total = 0;
    let mut annotated = 0;
    let mut out = String::new();
    let mut failures = Vec::new();
    for krate in &crates {
        let loops = crate_loops(krate)?;
        total += loops.len();
        annotated += loops.iter().filter(|l| l.needs_bound.is_some()).count();
        failures.extend(crate_problems(krate)?);
        for l in &loops {
            let _ = writeln!(
                out,
                "crates/{krate}/src/{}:{}\t`{}`\t{}{}\t{}",
                l.file,
                l.line,
                l.id(),
                l.kind,
                l.label
                    .as_ref()
                    .map_or_else(String::new, |s| format!(" '{s}")),
                l.needs_bound.map_or("by construction", |_| "annotated")
            );
        }
    }
    if list {
        print!("{out}");
    }
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("loops: {f}");
        }
        return Err(format!(
            "{} loop-bound problem(s) (GEN-022)",
            failures.len()
        ));
    }
    println!(
        "loops: {} crate(s), {total} loop(s) in library code: {annotated} with a bound annotation, the rest bounded by construction",
        crates.len()
    );
    Ok(0)
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

    fn problems_of(src: &str) -> Vec<String> {
        problems("", "x.rs", src)
    }

    // req: GEN-022
    #[test]
    fn a_while_or_loop_needs_a_bound_annotation() {
        let bare = "fn f() { while go() {} }\nfn g() { loop { break; } }";
        assert_eq!(problems_of(bare).len(), 2, "{:?}", problems_of(bare));
        let ok = "fn f() {\n    // bound: 8 passes;\n    // each pass halves n.\n    while n > 0 {}\n    // bound: one pass\n    let x = loop { break 1; };\n}";
        assert_eq!(problems_of(ok), Vec::<String>::new());
    }

    // req: GEN-022
    #[test]
    fn an_open_or_endless_source_needs_a_bound_annotation() {
        for src in [
            "fn f() {\n    for i in 0.. { }\n}",
            "fn f() {\n    for i in (0..).step_by(2) { }\n}",
            "fn f() {\n    let x = (3..).take(2).count();\n}",
            "fn f() {\n    let x = core::iter::repeat(1).take(2).sum::<u8>();\n}",
            "fn f() {\n    let x = it.cycle().take(5).all(|v| v);\n}",
            "fn f() {\n    let x = core::iter::from_fn(g).last();\n}",
        ] {
            assert_eq!(problems_of(src).len(), 1, "{src}");
        }
        let ok = "fn f() {\n    // bound: take(2) ends it\n    let x = (3..).take(2).count();\n}";
        assert_eq!(problems_of(ok), Vec::<String>::new());
    }

    // req: GEN-022
    #[test]
    fn a_loop_over_a_slice_or_a_closed_range_needs_nothing() {
        let src = "fn f(a: &[u8], b: &mut [u8]) {\n    for x in a.iter().rev() {}\n    for i in 0..a.len() {}\n    for i in 0..=7 {}\n    let n = a.iter().filter(|x| **x > 0).count();\n    let m = (0..4).map(|x| x).sum::<u8>();\n    b.copy_from_slice(a);\n    b.fill(0);\n}";
        assert_eq!(problems_of(src), Vec::<String>::new());
    }

    // req: GEN-022
    #[test]
    fn an_annotation_must_be_directly_above_a_loop_and_give_a_reason() {
        let dangling =
            "fn f() {\n    // bound: nothing here\n    let x = 1;\n    for i in 0..2 {}\n}";
        assert_eq!(problems_of(dangling).len(), 1);
        let gap = "fn f() {\n    // bound: 4 passes\n\n    while go() {}\n}";
        assert_eq!(problems_of(gap).len(), 2);
        let empty = "fn f() {\n    // bound:\n    while go() {}\n}";
        assert_eq!(problems_of(empty).len(), 1);
        // On a loop that is bounded by construction, an annotation is allowed.
        let extra = "fn f() {\n    // bound: 2 passes\n    for i in 0..2 {}\n}";
        assert_eq!(problems_of(extra), Vec::<String>::new());
    }

    // req: GEN-022
    #[test]
    fn the_library_has_no_unannotated_loop() {
        for krate in CRATES {
            assert_eq!(crate_problems(krate).unwrap(), Vec::<String>::new());
        }
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
