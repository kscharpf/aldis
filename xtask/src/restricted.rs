//! `xtask restricted`: every restricted type of a codec crate has a property test (RS-020,
//! VER-023; M2 plan WP-7, "A test for every restricted type"; part of `cargo xtask ci`).
//!
//! A *restricted type* is a public struct or enum of a crate of [`rs020::CRATES`] with a
//! *checked constructor*: an inherent `pub fn` that takes its arguments by value (no `self`
//! receiver and no reference) and returns `Option<Self>` or `Result<Self…>` (the type may be
//! named instead of `Self`), or an `impl TryFrom<…>` for it. A function that takes a reference
//! reads a buffer or another value: it parses, writes, or converts (a frame writer's `begin`, a
//! table built from a slice of channels, `Agency::new` over the T-field octets), and the codec's
//! round-trip and decoder property tests cover it; so do the decoders, functions named
//! `decode…` or `parse…`. A type that a `macro_rules!` body defines with a checked constructor
//! counts once for each invocation of the macro, named by its first identifier.
//!
//! Each restricted type `T` of crate `aldis-x` must be named by a marker comment
//! `// restricted: T` (several names may share one marker, separated by commas) in a file of
//! `crates/aldis-x/tests/`, placed in the property test that shows the type's accepted range,
//! its rejected values, and its wire round trip. A marker that names no restricted type of its
//! crate is a problem too, so the markers stay exact. The check is textual: it shows that a test
//! claims the type; the test itself shows the property.
//!
//! The scan reuses the lexer of [`rs020`]: comments and literals are removed, and items gated on
//! `cfg(test)` or `cfg(kani)` are skipped. Public reachability is not resolved: a `pub` type of
//! a private module counts, which can only ask for a test too many.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::repo;
use crate::rs020::{self, Tok, group_end, test_only, tokens};

/// The marker that names the restricted types a test covers.
const MARKER: &str = "// restricted:";

/// Whether token `i` of `t` is `text`.
fn is(t: &[Tok], i: usize, text: &str) -> bool {
    t.get(i).is_some_and(|tok| tok.text == text)
}

/// Whether `t[i]` is a bare `pub` (not `pub(crate)` or another restricted visibility).
fn bare_pub(t: &[Tok], i: usize) -> bool {
    is(t, i, "pub") && !is(t, i + 1, "(")
}

/// Index just past the angle-bracketed group that opens at `i` (`<…>`), or `i` if none opens.
fn skip_angles(t: &[Tok], i: usize) -> usize {
    if !is(t, i, "<") {
        return i;
    }
    let mut depth = 0_usize;
    let mut k = i;
    while k < t.len() {
        match t[k].text.as_str() {
            "<" => depth += 1,
            ">" => {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            // `->` in a bound such as `Fn() -> u8` is one token and does not close.
            _ => {}
        }
        k += 1;
    }
    k
}

/// The token ranges that `#[cfg(test)]` or `#[cfg(kani)]` items cover, to be skipped.
fn gated_ranges(t: &[Tok]) -> Result<Vec<(usize, usize)>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if is(t, i, "#") && is(t, i + 1, "[") {
            let end = group_end(t, i + 1)?;
            if test_only(&t[i + 2..end]) {
                // The item ends at its first `;` or at the end of its first brace group.
                let mut k = end + 1;
                while k < t.len() && !is(t, k, ";") && !is(t, k, "{") {
                    k += 1;
                }
                let stop = if is(t, k, "{") { group_end(t, k)? } else { k };
                out.push((i, stop));
                i = stop + 1;
                continue;
            }
            i = end + 1;
            continue;
        }
        i += 1;
    }
    Ok(out)
}

/// What one source file of a crate contributes.
#[derive(Debug, Default)]
struct FileScan {
    /// Public structs and enums defined in the file (`$name` inside a macro body).
    public_types: BTreeSet<String>,
    /// Checked constructors: the type (or `$name`) and the function (`TryFrom` for the trait),
    /// with the enclosing `macro_rules!` for a type named by a macro variable.
    checked: Vec<(String, String, Option<String>)>,
    /// Macro invocations: the macro and the first identifier of its input.
    invocations: Vec<(String, String)>,
}

/// The `macro_rules!` definitions of `t`: name and body range.
fn macro_ranges(t: &[Tok]) -> Result<Vec<(String, usize, usize)>, String> {
    let mut out = Vec::new();
    for i in 0..t.len() {
        if is(t, i, "macro_rules") && is(t, i + 1, "!") {
            let name = t.get(i + 2).map(|t| t.text.clone()).unwrap_or_default();
            let end = group_end(t, i + 3)?;
            out.push((name, i + 3, end));
        }
    }
    Ok(out)
}

/// Whether the return type `ret` (the tokens after `->`) is `Option<Self>` or `Result<Self…>`,
/// with `ty` standing for `Self`.
fn returns_self(ret: &[Tok], ty: &str) -> bool {
    let words: Vec<&str> = ret.iter().take(3).map(|t| t.text.as_str()).collect();
    matches!(words.as_slice(), ["Option" | "Result", "<", s] if *s == "Self" || *s == ty)
}

/// The checked constructors of the inherent impl of `ty` whose body is `t[open..=close]`.
fn inherent_constructors(t: &[Tok], open: usize, close: usize, ty: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = open + 1;
    while i < close {
        if !bare_pub(t, i) {
            i += 1;
            continue;
        }
        let mut k = i + 1;
        while matches!(
            t.get(k).map(|t| t.text.as_str()),
            Some("const" | "unsafe" | "async")
        ) {
            k += 1;
        }
        if !is(t, k, "fn") {
            i += 1;
            continue;
        }
        let name = t.get(k + 1).map(|t| t.text.clone()).unwrap_or_default();
        let args = skip_angles(t, k + 2);
        let Ok(args_end) = group_end(t, args) else {
            i += 1;
            continue;
        };
        // A receiver, or a reference: the function reads a buffer or another value.
        let by_value = !t[args..args_end]
            .iter()
            .any(|tok| tok.text == "self" || tok.text == "&");
        let mut ret_end = args_end + 1;
        while ret_end < close && !is(t, ret_end, "{") && !is(t, ret_end, "where") {
            ret_end += 1;
        }
        let decoder = name.starts_with("decode") || name.starts_with("parse");
        if by_value
            && !decoder
            && is(t, args_end + 1, "->")
            && returns_self(&t[args_end + 2..ret_end], ty)
        {
            out.push(name);
        }
        i = ret_end;
    }
    out
}

/// Scans one source file.
fn scan_file(text: &str) -> Result<FileScan, String> {
    let all = tokens(text);
    let gated = gated_ranges(&all)?;
    let t: Vec<Tok> = all
        .iter()
        .enumerate()
        .filter(|(i, _)| !gated.iter().any(|(a, b)| a <= i && i <= b))
        .map(|(_, tok)| tok.clone())
        .collect();
    let macros = macro_ranges(&t)?;
    let enclosing = |i: usize| {
        macros
            .iter()
            .find(|(_, a, b)| *a <= i && i <= *b)
            .map(|(n, _, _)| n.clone())
    };
    let mut scan = FileScan::default();
    for i in 0..t.len() {
        match t[i].text.as_str() {
            "struct" | "enum" if i > 0 && bare_pub(&t, i - 1) => {
                if let Some(name) = t.get(i + 1) {
                    scan.public_types.insert(name.text.clone());
                }
            }
            "impl" => {
                let mut open = skip_angles(&t, i + 1);
                let head = open;
                while open < t.len() && !is(&t, open, "{") && !is(&t, open, ";") {
                    open += 1;
                }
                if !is(&t, open, "{") {
                    continue;
                }
                let close = group_end(&t, open)?;
                let header = &t[head..open];
                let at_for = header.iter().position(|tok| tok.text == "for");
                let (trait_toks, self_ty) = match at_for {
                    Some(f) => (&header[..f], header.get(f + 1)),
                    None => (&header[..0], header.first()),
                };
                let Some(self_ty) = self_ty.map(|tok| tok.text.clone()) else {
                    continue;
                };
                let in_macro = if self_ty.starts_with('$') {
                    enclosing(i)
                } else {
                    None
                };
                let try_from = trait_toks
                    .iter()
                    .position(|tok| tok.text == "<")
                    .and_then(|p| p.checked_sub(1))
                    .is_some_and(|p| trait_toks[p].text == "TryFrom");
                if try_from {
                    scan.checked
                        .push((self_ty.clone(), "TryFrom".to_owned(), in_macro.clone()));
                } else if at_for.is_none() {
                    for f in inherent_constructors(&t, open, close, &self_ty) {
                        scan.checked.push((self_ty.clone(), f, in_macro.clone()));
                    }
                }
            }
            "!" if i > 0 && matches!(t.get(i + 1).map(|t| t.text.as_str()), Some("(" | "{")) => {
                let mac = t[i - 1].text.clone();
                if i > 1 && is(&t, i - 2, "macro_rules") || mac == "macro_rules" {
                    continue;
                }
                let mut k = i + 2;
                while is(&t, k, "#") && is(&t, k + 1, "[") {
                    k = group_end(&t, k + 1)? + 1;
                }
                if let Some(first) = t.get(k) {
                    scan.invocations.push((mac, first.text.clone()));
                }
            }
            _ => {}
        }
    }
    Ok(scan)
}

/// The restricted types of crate `krate`, each with its checked constructors; `read` lists the
/// crate's source files as (path, text).
fn restricted_types(files: &[(String, String)]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut public = BTreeSet::new();
    let mut checked: Vec<(String, String, Option<String>)> = Vec::new();
    let mut invocations = Vec::new();
    for (path, text) in files {
        let scan = scan_file(text).map_err(|e| format!("{path}: {e}"))?;
        public.extend(scan.public_types);
        checked.extend(scan.checked);
        invocations.extend(scan.invocations);
    }
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (ty, func, mac) in checked {
        match mac {
            Some(mac) if public.contains(&ty) => {
                for (_, name) in invocations.iter().filter(|(m, _)| *m == mac) {
                    out.entry(name.clone()).or_default().push(func.clone());
                }
            }
            None if public.contains(&ty) => out.entry(ty).or_default().push(func),
            _ => {}
        }
    }
    for funcs in out.values_mut() {
        funcs.sort();
        funcs.dedup();
    }
    Ok(out)
}

/// The type names of the markers in `text`, with their 1-based line numbers.
fn markers(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if let Some(rest) = line.trim_start().strip_prefix(MARKER) {
            for name in rest.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                out.push((name.to_owned(), n + 1));
            }
        }
    }
    out
}

/// The problems of one crate: restricted types without a marker, and markers that name none.
fn problems(
    krate: &str,
    types: &BTreeMap<String, Vec<String>>,
    tests: &[(String, String)],
) -> Vec<String> {
    let mut named = BTreeSet::new();
    let mut out = Vec::new();
    for (path, text) in tests {
        for (name, line) in markers(text) {
            if types.contains_key(&name) {
                named.insert(name);
            } else {
                out.push(format!(
                    "{path}:{line}: `{MARKER} {name}` names no restricted type of {krate}"
                ));
            }
        }
    }
    for (ty, funcs) in types {
        if !named.contains(ty) {
            out.push(format!(
                "{krate}: restricted type `{ty}` (checked by {}) has no property test: add one \
                 under crates/{krate}/tests/ with `{MARKER} {ty}`",
                funcs.join(", ")
            ));
        }
    }
    out
}

/// The `.rs` files under `dir`, as (relative path, text).
fn read_dir(dir: &str) -> Result<Vec<(String, String)>, String> {
    repo::files(dir, &["rs"])
        .iter()
        .map(|p| Ok((repo::rel(p), repo::read(p)?)))
        .collect()
}

/// `xtask restricted [list]`: checks the markers, or lists the restricted types.
pub fn run(list: bool) -> Result<usize, String> {
    let mut all = Vec::new();
    let mut total = 0;
    let mut report = String::new();
    for krate in rs020::CRATES {
        let types = restricted_types(&read_dir(&format!("crates/{krate}/src"))?)?;
        total += types.len();
        if list {
            for (ty, funcs) in &types {
                let _ = writeln!(report, "{krate}\t{ty}\t{}", funcs.join(", "));
            }
        } else {
            all.extend(problems(
                krate,
                &types,
                &read_dir(&format!("crates/{krate}/tests"))?,
            ));
        }
    }
    for p in &all {
        let _ = writeln!(report, "error: {p}");
    }
    print!("{report}");
    println!(
        "restricted: {} crate(s), {total} restricted type(s), {} problem(s)",
        rs020::CRATES.len(),
        all.len()
    );
    Ok(all.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(src: &str) -> Vec<String> {
        restricted_types(&[("lib.rs".to_owned(), src.to_owned())])
            .unwrap()
            .into_iter()
            .map(|(t, f)| format!("{t}:{}", f.join("+")))
            .collect()
    }

    #[test]
    fn finds_checked_constructors() {
        // req: RS-020, VER-023
        let src = r"
            /// A doc naming pub struct Fake;
            pub struct Offset(u16);
            impl Offset {
                pub const fn new(v: u16) -> Option<Offset> { None }
                pub const fn get(self) -> u16 { self.0 }
                pub fn same(&self) -> Option<Self> { None }
            }
            pub struct Len(usize);
            impl Len { pub fn new(v: usize) -> Result<Self, Error> { Err(e) } }
            pub enum Kind { A, B }
            impl Kind { pub fn from_code(c: u8) -> Option<Kind> { None } }
            impl TryFrom<u8> for Kind { type Error = Error; fn try_from(v: u8) -> Result<Self> { x } }
            pub struct Frame<'a>(&'a [u8]);
            impl<'a> Frame<'a> { pub fn decode(b: &'a [u8]) -> Result<Frame<'a>> { x } }
            pub struct Hidden(u8);
            impl Hidden { pub(crate) fn new(v: u8) -> Option<Self> { None } }
            struct Private(u8);
            impl Private { pub fn new(v: u8) -> Option<Self> { None } }
            pub struct Writer<'b>(&'b mut [u8]);
            impl<'b> Writer<'b> { pub fn begin(out: &'b mut [u8]) -> Result<Writer<'b>> { x } }
            pub struct Plain(u8);
            impl Plain { pub const fn new(v: u8) -> Plain { Plain(v) } }
            #[cfg(test)]
            mod tests { pub struct T(u8); impl T { pub fn new() -> Option<T> { None } } }
        ";
        assert_eq!(
            types(src),
            ["Kind:TryFrom+from_code", "Len:new", "Offset:new"]
        );
    }

    #[test]
    fn expands_macro_types_by_invocation() {
        // req: RS-020, VER-023
        let src = r"
            macro_rules! small {
                ($(#[$doc:meta])* $name:ident, $max:literal) => {
                    pub struct $name(u8);
                    impl $name { pub const fn new(v: u8) -> Option<$name> { None } }
                    impl TryFrom<u8> for $name { type Error = E; fn try_from(v: u8) -> Result<Self, E> { x } }
                };
            }
            small!(
                /// The doc.
                Status, 7
            );
            small! { #[doc = 1] Cop, 3 }
            other!(NotAType);
        ";
        assert_eq!(types(src), ["Cop:TryFrom+new", "Status:TryFrom+new"]);
    }

    #[test]
    fn markers_must_match_exactly() {
        // req: RS-020, VER-023
        let types: BTreeMap<String, Vec<String>> = [("A", "new"), ("B", "TryFrom")]
            .iter()
            .map(|(t, f)| ((*t).to_owned(), vec![(*f).to_owned()]))
            .collect();
        let tests = vec![(
            "crates/aldis-x/tests/properties.rs".to_owned(),
            "fn t() {\n    // restricted: A, Gone\n}\n".to_owned(),
        )];
        let p = problems("aldis-x", &types, &tests);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains(":2: `// restricted: Gone`"), "{p:?}");
        assert!(p[1].contains("`B`"), "{p:?}");
        let tests = vec![(
            "t.rs".to_owned(),
            "// restricted: A\n// restricted: B\n".to_owned(),
        )];
        assert_eq!(problems("aldis-x", &types, &tests), Vec::<String>::new());
    }
}
