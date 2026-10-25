//! The public functions of the library crates, by definition path, read from their source (for
//! `xtask stack`, VER-011): which functions of a build are public entry points.
//!
//! A function is **public** when a caller outside its crate can name it:
//! - a free `pub fn` of a module reachable through `pub mod`s from `src/lib.rs`;
//! - a `pub fn` of an inherent `impl` of a type, in such a module, unless the crate defines the
//!   type in it only as private;
//! - a method of a trait `impl` (it has no visibility of its own) for such a type;
//! - a provided (default) method of a `pub trait`.
//!
//! `pub(crate)`, `pub(super)`, and `pub(in …)` items are not public, nor is anything in a
//! private module, in a function body, or gated on `cfg(test)` or `cfg(kani)`. Items that a
//! `macro_rules!` body defines are not read (their names are macro variables): `xtask stack`
//! treats a function it cannot find here by the old rule (a global symbol of its own crate's
//! object) and reports how many it treated so. Modules are reached with `mod name;` and inline
//! `mod name { … }`; re-exports (`pub use`) are not needed, because a symbol names the item's
//! definition path, and every module of these crates is `pub mod` or private.
//!
//! The scanner is textual, with the tokens of `rs020` and the item helpers of `loops`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::loops::{after_group, after_item, header_end, impl_type, test_only};
use crate::repo;
use crate::rs020::{Tok, tokens};

/// What names a function, as both a symbol's demangled path and the source give it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// A free function: its crate and its path in the crate (`writer::encode`).
    Free { krate: String, path: String },
    /// A method: its crate (that of the type), the type's name (`VcTable`, or `[T]` for a
    /// slice), and the method.
    Method {
        krate: String,
        ty: String,
        name: String,
    },
    /// A provided method of a trait: its crate, the trait's name, and the method.
    Provided {
        krate: String,
        tr: String,
        name: String,
    },
}

/// The functions found, each with whether it is public.
#[derive(Debug, Default)]
pub struct Index {
    pub items: BTreeMap<Key, bool>,
}

impl Index {
    /// Whether the index knows a key.
    pub fn knows(&self, key: &Key) -> bool {
        self.items.contains_key(key)
    }

    /// Whether a key is a public function.
    pub fn public(&self, key: &Key) -> bool {
        self.items.get(key).copied().unwrap_or(false)
    }
}

/// The visibility written before an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vis {
    Pub,
    Restricted,
    Private,
}

/// The visibility of the item whose keyword (`fn`, `struct`, `mod`, …) is at `i`: the tokens
/// before it, past `const`, `unsafe`, `async`, and `extern` (whose ABI string the tokenizer
/// removed).
fn vis_before(t: &[Tok], i: usize) -> Vis {
    let mut j = i;
    while j > 0
        && matches!(
            t[j - 1].text.as_str(),
            "const" | "unsafe" | "async" | "extern"
        )
    {
        j -= 1;
    }
    match j.checked_sub(1).map(|k| t[k].text.as_str()) {
        Some("pub") => Vis::Pub,
        Some(")") => {
            // `pub(crate)`, `pub(super)`, `pub(in path)`.
            let mut depth = 0_usize;
            let mut k = j - 1;
            loop {
                match t[k].text.as_str() {
                    ")" => depth += 1,
                    "(" => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                if k == 0 {
                    return Vis::Private;
                }
                k -= 1;
            }
            if k > 0 && t[k - 1].text == "pub" {
                Vis::Restricted
            } else {
                Vis::Private
            }
        }
        _ => Vis::Private,
    }
}

/// A block of the scan.
#[derive(Debug, Clone)]
enum Scope {
    /// An `impl` block: its type, and whether it implements a trait.
    Impl { ty: String, of_trait: bool },
    /// A `trait` definition: its name, and whether it is `pub`.
    Trait { name: String, public: bool },
    /// An inline module: whether it is public (with its parents).
    Mod { path: String, public: bool },
    /// A function body, a macro body, or any other block: nothing inside is read.
    Opaque,
    /// A block that is none of these, inside which items are still read (`const _: () = {}`
    /// is not one: it is `Opaque`).
    Other,
}

/// A function as the source declares it.
#[derive(Debug)]
struct FnItem {
    key: Key,
    /// Its written visibility.
    vis: Vis,
    /// Whether its module is public.
    mod_public: bool,
    /// For a method: its type, and whether it is a trait method.
    method: Option<(String, bool)>,
}

/// What one file contributes before the types are known.
#[derive(Debug, Default)]
struct Raw {
    fns: Vec<FnItem>,
    /// The types (`struct`, `enum`, `union`, `type`, `trait`) and whether each is public.
    types: BTreeMap<String, bool>,
    /// The file modules declared: name, written visibility.
    mods: Vec<(String, Vis)>,
}

/// Reads one file of module `path` (`""` for the crate root), public or not.
#[expect(
    clippy::too_many_lines,
    reason = "one pass over the tokens, one arm per item keyword"
)]
fn scan_file(krate: &str, path: &str, public: bool, src: &str, raw: &mut Raw) {
    let t = tokens(src);
    let mut open: Vec<(Scope, usize)> = Vec::new();
    let join = |p: &str, n: &str| {
        if p.is_empty() {
            n.to_owned()
        } else {
            format!("{p}::{n}")
        }
    };
    let mut i = 0;
    while i < t.len() {
        while open.last().is_some_and(|(_, end)| *end <= i) {
            open.pop();
        }
        // The innermost scope that items are read in.
        let (module, mod_public) = open
            .iter()
            .rev()
            .find_map(|(s, _)| match s {
                Scope::Mod { path, public } => Some((path.clone(), *public)),
                _ => None,
            })
            .unwrap_or_else(|| (path.to_owned(), public));
        let opaque = open.iter().any(|(s, _)| matches!(s, Scope::Opaque));
        let text = t[i].text.as_str();
        let next = t.get(i + 1).map(|t| t.text.as_str());
        match text {
            "#" if next == Some("[") => {
                let end = after_group(&t, i + 1);
                if test_only(t.get(i + 2..end.saturating_sub(1)).unwrap_or(&[])) {
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
            "macro_rules" if next == Some("!") => {
                let j = header_end(&t, i + 3);
                if t.get(j).is_some_and(|t| t.text == "{") {
                    open.push((Scope::Opaque, after_group(&t, j)));
                }
                i = j + 1;
                continue;
            }
            "impl" if !opaque => {
                let ty = impl_type(&t, i + 1);
                let j = header_end(&t, i + 1);
                let of_trait = t
                    .get(i + 1..j)
                    .unwrap_or(&[])
                    .windows(2)
                    .any(|w| w[0].text == "for" && w[1].text != "<");
                if t.get(j).is_some_and(|t| t.text == "{") {
                    open.push((Scope::Impl { ty, of_trait }, after_group(&t, j)));
                }
                i = j + 1;
                continue;
            }
            "trait" if !opaque => {
                let name = next.unwrap_or_default().to_owned();
                let vis = vis_before(&t, i);
                raw.types
                    .insert(name.clone(), vis == Vis::Pub && mod_public);
                let j = header_end(&t, i + 2);
                if t.get(j).is_some_and(|t| t.text == "{") {
                    open.push((
                        Scope::Trait {
                            name,
                            public: vis == Vis::Pub && mod_public,
                        },
                        after_group(&t, j),
                    ));
                }
                i = j + 1;
                continue;
            }
            "struct" | "enum" | "union" | "type" if !opaque => {
                if let Some(n) = next.filter(|n| n.starts_with(|c: char| c.is_alphabetic())) {
                    let p = vis_before(&t, i) == Vis::Pub && mod_public;
                    let e = raw.types.entry(n.to_owned()).or_insert(false);
                    *e |= p;
                }
            }
            "mod" if !opaque && next.is_some() => {
                let name = next.unwrap_or_default().to_owned();
                let vis = vis_before(&t, i);
                match t.get(i + 2).map(|t| t.text.as_str()) {
                    Some(";") if open.is_empty() => raw.mods.push((name, vis)),
                    Some("{") => {
                        open.push((
                            Scope::Mod {
                                path: join(&module, &name),
                                public: mod_public && vis == Vis::Pub,
                            },
                            after_group(&t, i + 2),
                        ));
                        i += 3;
                        continue;
                    }
                    _ => {}
                }
            }
            "fn" if next
                .is_some_and(|n| n.starts_with(|c: char| c.is_alphabetic() || c == '_')) =>
            {
                let name = next.unwrap_or_default().to_owned();
                let j = header_end(&t, i + 2);
                if !opaque {
                    let vis = vis_before(&t, i);
                    // The scope the function is an item of.
                    match open.iter().rev().find(|(s, _)| !matches!(s, Scope::Other)) {
                        None | Some((Scope::Mod { .. }, _)) => raw.fns.push(FnItem {
                            key: Key::Free {
                                krate: krate.to_owned(),
                                path: join(&module, &name),
                            },
                            vis,
                            mod_public,
                            method: None,
                        }),
                        Some((Scope::Impl { ty, of_trait }, _)) => raw.fns.push(FnItem {
                            key: Key::Method {
                                krate: krate.to_owned(),
                                ty: ty.clone(),
                                name,
                            },
                            vis,
                            mod_public,
                            method: Some((ty.clone(), *of_trait)),
                        }),
                        Some((Scope::Trait { name: tr, public }, _)) => {
                            if t.get(j).is_some_and(|t| t.text == "{") {
                                raw.fns.push(FnItem {
                                    key: Key::Provided {
                                        krate: krate.to_owned(),
                                        tr: tr.clone(),
                                        name,
                                    },
                                    vis: if *public { Vis::Pub } else { Vis::Private },
                                    mod_public: true,
                                    method: None,
                                });
                            }
                        }
                        Some(_) => {}
                    }
                }
                if t.get(j).is_some_and(|t| t.text == "{") {
                    open.push((Scope::Opaque, after_group(&t, j)));
                }
                i = j + 1;
                continue;
            }
            "{" => {
                let scope = if opaque { Scope::Opaque } else { Scope::Other };
                open.push((scope, after_group(&t, i)));
            }
            _ => {}
        }
        i += 1;
    }
}

/// Reads the modules of one crate from `src_dir`.
fn scan_crate(krate: &str, src_dir: &Path) -> Result<Raw, String> {
    let mut raw = Raw::default();
    // (file relative to src/, module path, public)
    let mut queue = vec![("lib.rs".to_owned(), String::new(), true)];
    while let Some((rel, path, public)) = queue.pop() {
        let text = repo::read(&src_dir.join(&rel))?;
        let before = raw.mods.len();
        scan_file(krate, &path, public, &text, &mut raw);
        let dir = match rel.as_str() {
            "lib.rs" => String::new(),
            r if r.ends_with("/mod.rs") => r.trim_end_matches("mod.rs").to_owned(),
            r => format!("{}/", r.trim_end_matches(".rs")),
        };
        for (name, vis) in raw.mods.split_off(before) {
            let flat = format!("{dir}{name}.rs");
            let nested = format!("{dir}{name}/mod.rs");
            let file = if src_dir.join(&flat).is_file() {
                flat
            } else if src_dir.join(&nested).is_file() {
                nested
            } else {
                return Err(format!(
                    "{}: `mod {name};` names no file",
                    repo::rel(&src_dir.join(&rel))
                ));
            };
            let child = if path.is_empty() {
                name
            } else {
                format!("{path}::{name}")
            };
            queue.push((file, child, public && vis == Vis::Pub));
        }
    }
    Ok(raw)
}

/// Decides which functions of `raw` are public.
fn decide(raw: Raw, index: &mut Index) {
    let types = raw.types;
    for FnItem {
        key,
        vis,
        mod_public,
        method,
    } in raw.fns
    {
        // A type the crate defines only privately makes its methods private; a type defined
        // elsewhere (another crate, a macro) does not decide.
        let ty_ok = |ty: &str| {
            let base = ty.trim_start_matches('[').trim_end_matches(']');
            types.get(base).copied().unwrap_or(true)
        };
        let public = match &method {
            None => vis == Vis::Pub && mod_public,
            Some((ty, false)) => vis == Vis::Pub && mod_public && ty_ok(ty),
            Some((ty, true)) => mod_public && ty_ok(ty),
        };
        let e = index.items.entry(key).or_insert(false);
        *e |= public;
    }
}

/// The index of the library crates named (`aldis-core`, …), from `crates/<name>/src`.
pub fn index(crates: &[&str]) -> Result<Index, String> {
    let mut index = Index::default();
    for krate in crates {
        let src = repo::root().join("crates").join(krate).join("src");
        let raw = scan_crate(&krate.replace('-', "_"), &src)?;
        decide(raw, &mut index);
    }
    Ok(index)
}

/// Removes the generic arguments (`<…>`) of a demangled path, leaving `>` of `->` alone.
fn strip_generics(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0_usize;
    let mut prev = ' ';
    for ch in s.chars() {
        match ch {
            '<' => depth += 1,
            '>' if prev != '-' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
        prev = ch;
    }
    out.replace("::::", "::").trim_end_matches("::").to_owned()
}

/// The segments of a path at angle and bracket depth 0.
fn segments(s: &str) -> Vec<String> {
    s.split("::").map(str::to_owned).collect()
}

/// The keys a demangled symbol name may have: none for a closure, a shim, or a function inside
/// another function.
pub fn keys_of(demangled: &str) -> Vec<Key> {
    let Some(rest) = demangled.strip_prefix('<') else {
        // `crate::module::name`, with generic arguments.
        let segs = segments(&strip_generics(demangled));
        if segs.len() < 2 || segs.iter().any(|s| s.starts_with('{') || s.is_empty()) {
            return Vec::new();
        }
        return vec![Key::Free {
            krate: segs[0].clone(),
            path: segs[1..].join("::"),
        }];
    };
    // `<Type as Trait>::method` or `<Type>::method`: the group up to its matching `>`.
    let mut depth = 1_usize;
    let mut end = None;
    let mut prev = '<';
    for (k, ch) in rest.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if prev != '-' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(k);
                    break;
                }
            }
            _ => {}
        }
        prev = ch;
    }
    let Some(end) = end else {
        return Vec::new();
    };
    let inner = &rest[..end];
    let Some(tail) = rest[end + 1..].strip_prefix("::") else {
        return Vec::new();
    };
    let tail = segments(&strip_generics(tail));
    if tail.len() != 1 || tail[0].starts_with('{') || tail[0].is_empty() {
        return Vec::new();
    }
    let name = tail[0].clone();
    let stripped = strip_generics(inner);
    let (ty, tr) = match stripped.split_once(" as ") {
        Some((ty, tr)) => (ty.to_owned(), Some(tr.to_owned())),
        None => (stripped, None),
    };
    let mut out = Vec::new();
    let ty = ty
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .to_owned();
    let (slice, path) = match ty.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        Some(t) => (true, t.to_owned()),
        None => (false, ty),
    };
    let segs = segments(&path);
    if segs.len() >= 2 {
        let last = segs[segs.len() - 1].clone();
        out.push(Key::Method {
            krate: segs[0].clone(),
            ty: if slice { format!("[{last}]") } else { last },
            name: name.clone(),
        });
    }
    if let Some(tr) = tr {
        let segs = segments(&tr);
        if segs.len() >= 2 {
            out.push(Key::Provided {
                krate: segs[0].clone(),
                tr: segs[segs.len() - 1].clone(),
                name,
            });
        }
    }
    out
}

/// The public keys of `index` that are not among `seen`.
pub fn unseen<'a>(index: &'a Index, seen: &BTreeSet<Key>) -> Vec<&'a Key> {
    index
        .items
        .iter()
        .filter(|(k, public)| **public && !seen.contains(k))
        .map(|(k, _)| k)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(krate: &str, ty: &str, name: &str) -> Key {
        Key::Method {
            krate: krate.to_owned(),
            ty: ty.to_owned(),
            name: name.to_owned(),
        }
    }

    // req: VER-011
    #[test]
    fn reads_the_keys_of_demangled_names() {
        assert_eq!(
            keys_of("<aldis_aos::config::VcTable<64>>::new"),
            [method("aldis_aos", "VcTable", "new")]
        );
        assert_eq!(
            keys_of("aldis_aos::writer::encode::<aldis_aos::config::VcTable<64>>"),
            [Key::Free {
                krate: "aldis_aos".to_owned(),
                path: "writer::encode".to_owned()
            }]
        );
        assert_eq!(
            keys_of("<aldis_core::error::Error as core::fmt::Display>::fmt"),
            [
                method("aldis_core", "Error", "fmt"),
                Key::Provided {
                    krate: "core".to_owned(),
                    tr: "Display".to_owned(),
                    name: "fmt".to_owned()
                }
            ]
        );
        assert_eq!(
            keys_of(
                "<[aldis_tm::config::TmMasterChannelConfig] as aldis_tm::config::VcLookup>::security_layout"
            )[0],
            method("aldis_tm", "[TmMasterChannelConfig]", "security_layout")
        );
        assert_eq!(
            keys_of("<aldis_x::T<fn(u8) -> u8>>::begin::<aldis_x::Q<1>>"),
            [method("aldis_x", "T", "begin")]
        );
        assert_eq!(
            keys_of("aldis_ffi::tc::aldis_tc_validate::{closure#0}"),
            Vec::<Key>::new()
        );
        assert_eq!(keys_of("<aldis_x::T>::f::{closure#0}"), Vec::<Key>::new());
        assert_eq!(keys_of("memcpy"), Vec::<Key>::new());
    }

    fn scan(krate: &str, src: &str) -> Index {
        let mut raw = Raw::default();
        scan_file(krate, "m", true, src, &mut raw);
        let mut index = Index::default();
        decide(raw, &mut index);
        index
    }

    // req: VER-011
    #[test]
    fn tells_public_functions_from_others() {
        let i = scan(
            "x",
            r"
            pub struct Pub;
            struct Private;
            pub fn free() { fn inner() {} }
            pub(crate) fn helper() {}
            fn private() {}
            pub const unsafe fn qualified() {}
            impl<const N: usize> Pub {
                pub fn new() -> Self { Pub }
                pub(crate) fn check(&self) {}
                fn own(&self) {}
            }
            impl Private { pub fn hidden() {} }
            impl core::fmt::Display for Pub { fn fmt(&self) {} }
            impl Clone for Private { fn clone(&self) -> Self { Private } }
            pub trait Lookup { fn get(&self); fn provided(&self) -> u8 { 0 } }
            pub mod inner { pub fn deep() {} }
            mod hidden { pub fn not_reachable() {} }
            #[cfg(test)]
            mod tests { pub fn t() {} }
            macro_rules! gen { () => { pub fn generated() {} }; }
            ",
        );
        let free = |p: &str| Key::Free {
            krate: "x".to_owned(),
            path: p.to_owned(),
        };
        for (key, public) in [
            (free("m::free"), true),
            (free("m::helper"), false),
            (free("m::private"), false),
            (free("m::qualified"), true),
            (method("x", "Pub", "new"), true),
            (method("x", "Pub", "check"), false),
            (method("x", "Pub", "own"), false),
            (method("x", "Private", "hidden"), false),
            (method("x", "Pub", "fmt"), true),
            (method("x", "Private", "clone"), false),
            (
                Key::Provided {
                    krate: "x".to_owned(),
                    tr: "Lookup".to_owned(),
                    name: "provided".to_owned(),
                },
                true,
            ),
            (free("m::inner::deep"), true),
            (free("m::hidden::not_reachable"), false),
        ] {
            assert!(i.knows(&key), "{key:?}");
            assert_eq!(i.public(&key), public, "{key:?}");
        }
        for key in [free("m::inner"), free("m::tests::t"), free("m::generated")] {
            assert!(!i.knows(&key), "{key:?}");
        }
    }

    // req: VER-011
    #[test]
    fn indexes_the_library_crates() {
        let i = index(&["aldis-aos", "aldis-tc"]).unwrap();
        assert!(i.public(&method("aldis_aos", "VcTable", "new")));
        assert!(i.public(&Key::Free {
            krate: "aldis_aos".to_owned(),
            path: "writer::encode".to_owned()
        }));
        for (krate, path) in [
            ("aldis_tc", "config::check_room"),
            ("aldis_aos", "config::fill"),
        ] {
            let key = Key::Free {
                krate: krate.to_owned(),
                path: path.to_owned(),
            };
            assert!(i.knows(&key) && !i.public(&key), "{key:?}");
        }
    }
}
