//! `xtask stack [<target>...]`: the worst-case stack use of every entry point of the library, per
//! bare-metal target, against a budget per target (VER-011, extended by M2 plan WP-7; GEN-022).
//!
//! **Build.** The static library of the C ABI is built as it ships (`cargo build -p aldis-ffi
//! --no-default-features --features panic-handler --release --target <t>`), on the tooling
//! nightly (RS-002) with `-Z build-std=core`, so that `core` and `compiler_builtins` are compiled
//! with the same flags as the library, and with `-Z emit-stack-sizes --emit=llvm-ir,link`. The
//! archive holds one object per crate (the release profile's LTO does not apply to a crate that
//! is also an `rlib`), with a `.stack_sizes` section that gives each function's frame, and one
//! LLVM IR file per crate, written by the same compilation.
//!
//! **Graph.** The nodes are the functions of the archive's objects; a function's callees are the
//! functions its relocations name: calls, tail calls, and addresses taken, which can only
//! over-approximate. The IR of each function says whether it makes an indirect call (a `dyn` call
//! or a call through a function pointer), runs inline assembly, or allocates a dynamic amount of
//! stack. GEN-022 forbids recursion, so the graph is a DAG, and a function's worst case is its
//! frame plus the largest worst case of its callees.
//!
//! **Entry points.** Every `aldis_*` function of the C ABI, and every global function that the
//! object of a library crate defines (each non-generic public function, and the functions that
//! public generic or inline code calls): a superset of the public non-generic API. A generic
//! function has no code until it is instantiated; it is measured where the C ABI instantiates it,
//! with the capacities of `crates/aldis-ffi/capacities.toml`.
//!
//! **Completeness.** A reachable function without a frame (hand-written assembly), without IR,
//! with an indirect call, inline assembly, or a dynamic allocation, a call to a symbol that no
//! object defines, and a cycle are each a failure, unless [`BUDGETS`] lists an exemption with a
//! stated bound: `frame` for a function in assembly (its own frame, read from the disassembly;
//! its direct callees stay in the graph), or `indirect` for a function from which indirect calls
//! are reached (the bytes they may take, added to its worst case). An exemption that matches
//! nothing is a failure too, so the list stays exact.
//!
//! **Gate.** Each entry point's worst case must not exceed its target's budget for its kind
//! (`rust` or `c-abi`). The report `target/spec/stack-<target>.md` lists every entry point with
//! its worst case and deepest path, and the frame of every reachable function; it is the table
//! that VER-011 publishes per release (the nightly job `stack-sizes`).
//!
//! The bound does not include interrupt handlers or the caller's own frames, and holds for this
//! build: another compiler version, profile, or set of features can change it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::{binaries, repo};

/// The bare-metal targets measured (docs/06-verification.md §4).
pub const TARGETS: &[&str] = &[
    "thumbv7em-none-eabihf",
    "armv7r-none-eabihf",
    "riscv32imac-unknown-none-elf",
];

/// The budgets and exemptions, relative to the repository root.
pub const BUDGETS: &str = "xtask/stack-budgets.toml";

/// The build directory of the measurement, apart from the ordinary builds (other flags, and
/// `core` built from source).
const TARGET_DIR: &str = "target/stack";

/// The two kinds of entry point, each with its own budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// A function of the C ABI (`aldis_*` in `aldis-ffi`).
    CAbi,
    /// A global function of a Rust library crate.
    Rust,
}

impl Kind {
    const fn key(self) -> &'static str {
        match self {
            Self::CAbi => "c-abi",
            Self::Rust => "rust",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// LLVM IR

/// What the IR of one function shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct IrFacts {
    /// Calls whose callee is a value, not a function: `dyn` calls and function pointers.
    indirect: usize,
    /// Inline assembly.
    asm: usize,
    /// `alloca` with a run-time count.
    dynamic_alloca: usize,
}

impl IrFacts {
    const fn merge(self, other: Self) -> Self {
        Self {
            indirect: self.indirect + other.indirect,
            asm: self.asm + other.asm,
            dynamic_alloca: self.dynamic_alloca + other.dynamic_alloca,
        }
    }
}

/// The functions an IR module defines, and its aliases.
#[derive(Debug, Default)]
struct Ir {
    defined: HashMap<String, IrFacts>,
    aliases: HashMap<String, String>,
}

/// The global name (`@name` or `@"name"`) that starts `s`, without the `@`.
fn global_name(s: &str) -> Option<&str> {
    let s = s.strip_prefix('@')?;
    if let Some(q) = s.strip_prefix('"') {
        return q.split('"').next();
    }
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '-')))
        .unwrap_or(s.len());
    (end > 0).then(|| &s[..end])
}

/// The callee of a `call` or `invoke` instruction (the text after the keyword): `Some(true)`
/// when it is a function (`@f(`), `Some(false)` when it is a value (`%5(`) or unreadable,
/// `None` for inline assembly. The callee is the first name directly followed by the argument
/// list; a type such as `%"core::fmt::Arguments"` or `i32 (ptr, ...)`, and an attribute such as
/// `range(i32 1, 0)`, is never a name directly followed by `(`.
fn callee_is_function(rest: &str) -> Option<bool> {
    if rest.split_whitespace().any(|w| w == "asm") {
        return None;
    }
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'@' | b'%' => {
                let sigil = b[i];
                let mut j = i + 1;
                if b.get(j) == Some(&b'"') {
                    j += 1;
                    while j < b.len() && b[j] != b'"' {
                        j += 1;
                    }
                    j += 1;
                } else {
                    while j < b.len()
                        && (b[j].is_ascii_alphanumeric()
                            || matches!(b[j], b'_' | b'.' | b'$' | b'-'))
                    {
                        j += 1;
                    }
                }
                if b.get(j) == Some(&b'(') {
                    return Some(sigil == b'@');
                }
                i = j.max(i + 1);
            }
            _ => i += 1,
        }
    }
    // No name in front of an argument list: a callee this reader does not know, counted as
    // indirect.
    Some(false)
}

/// Reads one IR module into `ir`.
fn parse_ir(text: &str, ir: &mut Ir) {
    let mut current: Option<(String, IrFacts)> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("define ") {
            if let Some(name) = rest.find('@').and_then(|at| global_name(&rest[at..])) {
                current = Some((name.to_owned(), IrFacts::default()));
            }
            continue;
        }
        if line.starts_with('}') {
            if let Some((name, facts)) = current.take() {
                let merged = ir.defined.get(&name).map_or(facts, |f| f.merge(facts));
                ir.defined.insert(name, merged);
            }
            continue;
        }
        if current.is_none() {
            // `@a = ... alias <type>, ptr @b`: `a` is `b`.
            if line.starts_with('@')
                && line.split_whitespace().any(|w| w == "alias")
                && let (Some(name), Some(target)) = (
                    global_name(line),
                    line.rfind('@').and_then(|at| global_name(&line[at..])),
                )
                && name != target
            {
                ir.aliases.insert(name.to_owned(), target.to_owned());
            }
            continue;
        }
        let Some((_, facts)) = current.as_mut() else {
            continue;
        };
        // Comments, such as rustc's `; call <path>` annotations.
        if line.trim_start().starts_with(';') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if let Some(k) = words.iter().position(|w| *w == "call" || *w == "invoke") {
            let rest = words.get(k + 1..).map_or_else(String::new, |w| w.join(" "));
            match callee_is_function(&rest) {
                Some(true) => {}
                Some(false) => facts.indirect += 1,
                None => facts.asm += 1,
            }
        } else if let Some(k) = words.iter().position(|w| *w == "alloca") {
            // `alloca <ty>, <ty> %n, align a`: the count is a value.
            let rest = words.get(k + 1..).map_or_else(String::new, |w| w.join(" "));
            if rest.split(',').nth(1).is_some_and(|count| {
                count
                    .split_whitespace()
                    .nth(1)
                    .is_some_and(|v| v.starts_with('%'))
            }) {
                facts.dynamic_alloca += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Objects

/// One function of the archive.
#[derive(Debug, Clone, Default)]
struct Node {
    /// Its symbol names (several when symbols alias one address).
    names: Vec<String>,
    /// The demangled name of its first symbol.
    display: String,
    /// The crate whose object defines it.
    krate: String,
    /// A global (not weak) definition: a candidate entry point.
    global: bool,
    /// Its frame, from `.stack_sizes`.
    frame: Option<u64>,
    /// The functions its relocations name.
    edges: BTreeSet<usize>,
    /// Symbols it refers to that no object of the archive defines.
    undefined: BTreeSet<String>,
    /// Its IR, if a module defines one of its names.
    ir: Option<IrFacts>,
}

fn field<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(v, |v, k| v.get(k).unwrap_or(&Value::Null))
}

fn num(v: &Value, path: &[&str]) -> u64 {
    field(v, path).as_u64().unwrap_or(0)
}

fn text<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    field(v, path).as_str().unwrap_or("")
}

/// The crate of an archive member: `aldis_core-<hash>.aldis_core.<hash>-cgu.0.rcgu.o`, or
/// `aldis_ffi.aldis_ffi.<hash>-cgu.0.rcgu.o` for the crate of the archive itself.
fn member_crate(file: &str) -> String {
    let member = file
        .rsplit_once('(')
        .map_or(file, |(_, m)| m.trim_end_matches(')'));
    member.split(['-', '.']).next().unwrap_or(member).to_owned()
}

/// The JSON array at `path` of `v`, or an empty slice.
fn list<'a>(v: &'a Value, path: &[&str]) -> &'a [Value] {
    field(v, path).as_array().map_or(&[], Vec::as_slice)
}

/// The archive's functions while its objects are read.
#[derive(Default)]
struct Archive {
    nodes: Vec<Node>,
    /// Global and weak definitions by name; a global one wins over a weak one.
    globals: HashMap<String, (bool, usize)>,
    /// References to undefined symbols, resolved once every object is read.
    pending: Vec<(usize, String)>,
}

/// The functions of one object, by address.
#[derive(Default)]
struct Functions<'a> {
    /// By (section, address); the Thumb bit is not part of the address.
    at: BTreeMap<(u64, u64), usize>,
    by_name: HashMap<&'a str, usize>,
    /// Per node: its start and size within its section.
    extent: HashMap<usize, (u64, u64)>,
}

impl Functions<'_> {
    fn in_section(&self, section: u64) -> Vec<usize> {
        self.at
            .range((section, 0)..=(section, u64::MAX))
            .map(|(_, &id)| id)
            .collect()
    }
}

impl Archive {
    /// Adds the function symbols of one object, and their frames.
    fn functions<'a>(
        &mut self,
        object: &Value,
        symbols: &[&'a Value],
        names: &[&str],
    ) -> Functions<'a> {
        let krate = member_crate(text(object, &["FileSummary", "File"]));
        let mut f = Functions::default();
        for (si, s) in symbols.iter().enumerate() {
            let section = num(s, &["Section", "Value"]);
            if text(s, &["Type", "Name"]) != "Function" || section == 0 || section >= 0xff00 {
                continue;
            }
            let name = text(s, &["Name", "Name"]);
            let value = num(s, &["Value"]) & !1;
            let binding = text(s, &["Binding", "Name"]);
            let id = *f.at.entry((section, value)).or_insert_with(|| {
                self.nodes.push(Node {
                    display: names.get(si).copied().unwrap_or(name).to_owned(),
                    krate: krate.clone(),
                    ..Node::default()
                });
                self.nodes.len() - 1
            });
            let node = &mut self.nodes[id];
            node.names.push(name.to_owned());
            node.global |= binding == "Global";
            let size = num(s, &["Size"]);
            let e = f.extent.entry(id).or_insert((value, size));
            e.1 = e.1.max(size);
            f.by_name.insert(name, id);
            let strong = binding == "Global";
            let replace = match self.globals.get(name) {
                _ if !matches!(binding, "Global" | "Weak") => false,
                None => true,
                Some(&(was_strong, _)) => strong && !was_strong,
            };
            if replace {
                self.globals.insert(name.to_owned(), (strong, id));
            }
        }
        for entry in list(object, &["StackSizes"]) {
            let size = num(entry, &["Entry", "Size"]);
            for name in list(entry, &["Entry", "Functions"]) {
                if let Some(&id) = name.as_str().and_then(|n| f.by_name.get(n)) {
                    let frame = self.nodes[id].frame.get_or_insert(0);
                    *frame = (*frame).max(size);
                }
            }
        }
        f
    }

    /// Adds the edges of one object's relocations in executable sections.
    fn relocations(&mut self, object: &Value, symbols: &[&Value], f: &Functions) {
        let sections: HashMap<u64, &Value> = list(object, &["Sections"])
            .iter()
            .map(|s| (num(s, &["Section", "Index"]), field(s, &["Section"])))
            .collect();
        let executable = |index: u64| {
            sections.get(&index).is_some_and(|s| {
                list(s, &["Flags", "Flags"])
                    .iter()
                    .any(|f| text(f, &["Name"]) == "SHF_EXECINSTR")
            })
        };
        for block in list(object, &["Relocations"]) {
            let rel = num(block, &["SectionIndex"]);
            let target = sections.get(&rel).map_or(0, |s| num(s, &["Info"]));
            if !executable(target) {
                continue;
            }
            let sources = f.in_section(target);
            for r in list(block, &["Relocs"]) {
                let r = field(r, &["Relocation"]);
                let offset = num(r, &["Offset"]);
                let index = num(r, &["Symbol", "Value"]);
                let Some(sym) = usize::try_from(index).ok().and_then(|i| symbols.get(i)) else {
                    continue;
                };
                let name = text(sym, &["Name", "Name"]);
                if index == 0 || name.is_empty() {
                    continue;
                }
                let from: Vec<usize> = sources
                    .iter()
                    .copied()
                    .filter(|id| {
                        f.extent.get(id).is_some_and(|&(start, size)| {
                            offset >= start && offset < start + size.max(1)
                        })
                    })
                    .collect();
                let section = num(sym, &["Section", "Value"]);
                let to: Vec<usize> = match text(sym, &["Type", "Name"]) {
                    _ if section == 0 => {
                        self.pending
                            .extend(from.iter().map(|&id| (id, name.to_owned())));
                        continue;
                    }
                    "Function" => {
                        f.at.get(&(section, num(sym, &["Value"]) & !1))
                            .copied()
                            .into_iter()
                            .collect()
                    }
                    // A section symbol stands for the functions of that section.
                    "Section" if section != target => f.in_section(section),
                    // Labels inside the function itself, and data.
                    _ => Vec::new(),
                };
                for id in from {
                    self.nodes[id].edges.extend(to.iter().copied());
                }
            }
        }
    }
}

/// The functions of `objects` (the JSON of `llvm-readobj --section-headers --symbols -r
/// --stack-sizes` on the archive), with the demangled names of `demangled` (the same symbols,
/// `--demangle`), their frames, and their edges.
fn graph(objects: &Value, demangled: &Value) -> Result<Vec<Node>, String> {
    let objects = objects
        .as_array()
        .ok_or("llvm-readobj: expected a JSON array")?;
    let demangled = demangled
        .as_array()
        .ok_or("llvm-readobj: expected a JSON array")?;
    let mut archive = Archive::default();
    for (oi, object) in objects.iter().enumerate() {
        let symbols: Vec<&Value> = list(object, &["Symbols"])
            .iter()
            .map(|s| field(s, &["Symbol"]))
            .collect();
        let names: Vec<&str> = list(demangled.get(oi).unwrap_or(&Value::Null), &["Symbols"])
            .iter()
            .map(|s| text(s, &["Symbol", "Name", "Name"]))
            .collect();
        let functions = archive.functions(object, &symbols, &names);
        archive.relocations(object, &symbols, &functions);
    }
    let Archive {
        mut nodes,
        globals,
        pending,
    } = archive;
    for (from, name) in pending {
        match globals.get(&name) {
            Some(&(_, id)) => {
                nodes[from].edges.insert(id);
            }
            None => {
                nodes[from].undefined.insert(name);
            }
        }
    }
    Ok(nodes)
}

/// Attaches the IR facts of each node, following aliases.
fn attach_ir(nodes: &mut [Node], ir: &Ir) {
    for node in nodes {
        for name in &node.names {
            let mut name = name.as_str();
            for _ in 0..8 {
                match ir.aliases.get(name) {
                    Some(target) => name = target,
                    None => break,
                }
            }
            if let Some(&facts) = ir.defined.get(name) {
                node.ir = Some(node.ir.map_or(facts, |f| f.merge(facts)));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Budgets and exemptions

/// What an exemption states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    /// The function's own frame: assembly, which has no `.stack_sizes` entry and no IR.
    Frame(u64),
    /// The bytes that the indirect calls reached from the function may take.
    Indirect(u64),
}

#[derive(Debug, Clone)]
struct Exemption {
    /// The demangled name.
    function: String,
    /// The targets it applies to; empty for all.
    targets: Vec<String>,
    bound: Bound,
    line: usize,
}

#[derive(Debug, Default)]
struct Config {
    /// Per target: per kind, the budget in bytes.
    budgets: BTreeMap<String, BTreeMap<String, u64>>,
    exemptions: Vec<Exemption>,
}

/// A value of the TOML subset: a string, an integer, or an array of strings, on one line.
#[derive(Debug, PartialEq, Eq)]
enum Val {
    Str(String),
    Int(u64),
    List(Vec<String>),
}

fn value(raw: &str) -> Option<Val> {
    let raw = raw.trim();
    if let Some(s) = raw.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return (!s.contains('"')).then(|| Val::Str(s.to_owned()));
    }
    if let Some(items) = raw.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return items
            .split(',')
            .map(str::trim)
            .filter(|i| !i.is_empty())
            .map(|i| match value(i) {
                Some(Val::Str(s)) => Some(s),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(Val::List);
    }
    raw.replace('_', "").parse().ok().map(Val::Int)
}

/// One `[[budget]]` or `[[exemption]]` table: its kind, its line, and its keys.
struct Table {
    kind: String,
    line: usize,
    entries: Vec<(String, Val)>,
}

impl Table {
    fn get(&self, key: &str) -> Option<&Val> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// The tables of [`BUDGETS`], and the problems of its lines.
fn tables(text: &str) -> (Vec<Table>, Vec<String>) {
    let mut tables: Vec<Table> = Vec::new();
    let mut problems = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let at = format!("{BUDGETS}:{}", n + 1);
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(kind) = line.strip_prefix("[[").and_then(|l| l.strip_suffix("]]")) {
            if matches!(kind, "budget" | "exemption") {
                tables.push(Table {
                    kind: kind.to_owned(),
                    line: n + 1,
                    entries: Vec::new(),
                });
            } else {
                problems.push(format!("{at}: unknown table `[[{kind}]]`"));
            }
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            problems.push(format!("{at}: expected `key = value` or a table header"));
            continue;
        };
        let key = key.trim();
        let Some(v) = value(raw_value) else {
            problems.push(format!(
                "{at}: `{key}` must be a string, an integer, or a list of strings on one line"
            ));
            continue;
        };
        let Some(table) = tables.last_mut() else {
            problems.push(format!("{at}: `{key}` outside a table"));
            continue;
        };
        if table.get(key).is_some() {
            problems.push(format!("{at}: `{key}` given twice"));
            continue;
        }
        table.entries.push((key.to_owned(), v));
    }
    (tables, problems)
}

/// Adds one `[[budget]]` table to `out`.
fn budget(t: &Table, out: &mut Config, problems: &mut Vec<String>) {
    let at = format!("{BUDGETS}:{}", t.line);
    let Some(Val::Str(target)) = t.get("target") else {
        problems.push(format!("{at}: a budget without a `target`"));
        return;
    };
    if out.budgets.contains_key(target) {
        problems.push(format!("{at}: {target} has two budgets"));
        return;
    }
    let mut per_kind = BTreeMap::new();
    for k in [Kind::Rust, Kind::CAbi] {
        match t.get(k.key()) {
            Some(Val::Int(b)) if *b > 0 => {
                per_kind.insert(k.key().to_owned(), *b);
            }
            _ => problems.push(format!("{at}: {target} needs `{} = <bytes>`", k.key())),
        }
    }
    out.budgets.insert(target.clone(), per_kind);
}

/// Reads one `[[exemption]]` table.
fn exemption(t: &Table, problems: &mut Vec<String>) -> Option<Exemption> {
    let at = format!("{BUDGETS}:{}", t.line);
    let Some(Val::Str(function)) = t
        .get("function")
        .filter(|f| !matches!(f, Val::Str(s) if s.is_empty()))
    else {
        problems.push(format!("{at}: an exemption without a `function`"));
        return None;
    };
    if !matches!(t.get("reason"), Some(Val::Str(r)) if !r.trim().is_empty()) {
        problems.push(format!("{at}: the exemption of {function} gives no reason"));
    }
    let targets = match t.get("targets") {
        None => Vec::new(),
        Some(Val::List(list)) => list.clone(),
        Some(_) => {
            problems.push(format!("{at}: `targets` must be a list of strings"));
            Vec::new()
        }
    };
    let bound = match (t.get("frame"), t.get("indirect")) {
        (Some(Val::Int(b)), None) => Bound::Frame(*b),
        (None, Some(Val::Int(b))) => Bound::Indirect(*b),
        _ => {
            problems.push(format!(
                "{at}: the exemption of {function} states one bound in bytes: `frame` or \
                 `indirect`"
            ));
            return None;
        }
    };
    Some(Exemption {
        function: function.clone(),
        targets,
        bound,
        line: t.line,
    })
}

/// Parses [`BUDGETS`]: `[[budget]]` tables with `target`, `rust`, and `c-abi`, and
/// `[[exemption]]` tables with `function`, `targets` (optional), `frame` or `indirect`, and
/// `reason` (a line-based subset of TOML, as `xtask/rs020-exemptions.toml` is read). Returns the
/// configuration and the problems of the file itself.
fn config(text: &str) -> (Config, Vec<String>) {
    let (tables, mut problems) = tables(text);
    let mut out = Config::default();
    for t in &tables {
        let allowed: &[&str] = if t.kind == "budget" {
            &["target", "rust", "c-abi"]
        } else {
            &["function", "targets", "frame", "indirect", "reason"]
        };
        for (k, _) in &t.entries {
            if !allowed.contains(&k.as_str()) {
                problems.push(format!(
                    "{BUDGETS}:{}: unknown key `{k}` in [[{}]] (use {})",
                    t.line,
                    t.kind,
                    allowed.join(", ")
                ));
            }
        }
        if t.kind == "budget" {
            budget(t, &mut out, &mut problems);
        } else if let Some(e) = exemption(t, &mut problems) {
            out.exemptions.push(e);
        }
    }
    (out, problems)
}

// ---------------------------------------------------------------------------------------------
// Analysis

/// A reason why the graph is incomplete at a function.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Gap {
    NoFrame,
    NoIr,
    Indirect(usize),
    Asm(usize),
    DynamicAlloca(usize),
    Undefined(String),
}

impl Gap {
    fn describe(&self) -> String {
        match self {
            Self::NoFrame => "has no `.stack_sizes` entry (assembly?)".to_owned(),
            Self::NoIr => "has no LLVM IR, so its indirect calls are unknown".to_owned(),
            Self::Indirect(n) => {
                format!("makes {n} indirect call(s) (`dyn` or a function pointer)")
            }
            Self::Asm(n) => format!("runs inline assembly ({n})"),
            Self::DynamicAlloca(n) => format!("allocates a dynamic amount of stack ({n})"),
            Self::Undefined(s) => format!("calls `{s}`, which no object defines"),
        }
    }
}

/// One entry point's result.
#[derive(Debug, Clone)]
struct Entry {
    kind: Kind,
    node: usize,
    worst: u64,
    /// The deepest path, from the entry point.
    path: Vec<usize>,
}

/// The result of one target.
#[derive(Debug, Default)]
struct Analysis {
    entries: Vec<Entry>,
    /// The gaps at each reachable function, with an entry point that reaches it.
    gaps: BTreeMap<usize, (BTreeSet<Gap>, usize)>,
    /// The cycles found, each from the function where it closes.
    cycles: BTreeSet<Vec<usize>>,
    /// The functions reached from any entry point.
    reached: BTreeSet<usize>,
    /// The exemptions (by index) that applied.
    used: BTreeSet<usize>,
}

/// The depth-first walk of [`analyse`].
struct Walk<'a> {
    nodes: &'a [Node],
    /// Per node: the frame that an exemption states, with the exemption's index.
    frame_of: &'a HashMap<usize, (u64, usize)>,
    /// Per node: the bytes that an exemption grants its indirect calls, with its index.
    indirect_of: &'a HashMap<usize, (u64, usize)>,
    /// Per (node, covered): its worst case and the callee on its deepest path.
    memo: HashMap<(usize, bool), (u64, Option<usize>)>,
    on_path: Vec<usize>,
    out: Analysis,
    /// The entry point being walked, named with the gaps it reaches.
    entry: usize,
}

impl Walk<'_> {
    fn gap(&mut self, node: usize, gap: Gap) {
        let entry = self.entry;
        self.out
            .gaps
            .entry(node)
            .or_insert_with(|| (BTreeSet::new(), entry))
            .0
            .insert(gap);
    }

    /// The gaps of `node` itself, and its own frame.
    fn own(&mut self, node: usize, covered: bool) -> u64 {
        let n = &self.nodes[node];
        let stated = self.frame_of.get(&node).copied();
        let frame = match (n.frame, stated) {
            (Some(f), _) => f,
            (None, Some((bytes, ex))) => {
                self.out.used.insert(ex);
                bytes
            }
            (None, None) => {
                self.gap(node, Gap::NoFrame);
                0
            }
        };
        // A frame stated for assembly also stands for its missing IR (the reason says what the
        // assembly calls; its direct calls are relocations, so they stay in the graph).
        let assembly = n.frame.is_none() && stated.is_some();
        let undefined: Vec<String> = n.undefined.iter().cloned().collect();
        match n.ir {
            None if !assembly => self.gap(node, Gap::NoIr),
            None => {}
            Some(f) => {
                if f.dynamic_alloca > 0 {
                    self.gap(node, Gap::DynamicAlloca(f.dynamic_alloca));
                }
                if !covered && f.indirect > 0 {
                    self.gap(node, Gap::Indirect(f.indirect));
                }
                if !covered && f.asm > 0 {
                    self.gap(node, Gap::Asm(f.asm));
                }
            }
        }
        for name in undefined {
            self.gap(node, Gap::Undefined(name));
        }
        frame
    }

    /// The worst case of `node`; `covered` when an `indirect` exemption of a caller on the path
    /// accepts the indirect calls below it.
    fn worst(&mut self, node: usize, covered: bool) -> u64 {
        if let Some(pos) = self.on_path.iter().position(|&n| n == node) {
            self.out.cycles.insert(self.on_path[pos..].to_vec());
            return 0;
        }
        if let Some(&(w, _)) = self.memo.get(&(node, covered)) {
            return w;
        }
        self.out.reached.insert(node);
        let granted = self.indirect_of.get(&node).map(|&(bytes, _)| bytes);
        let below = covered || granted.is_some();
        let own = self.own(node, below);
        self.on_path.push(node);
        let mut best: (u64, Option<usize>) = (0, None);
        let edges: Vec<usize> = self.nodes[node].edges.iter().copied().collect();
        for callee in edges {
            let w = self.worst(callee, below);
            if best.1.is_none() || w > best.0 {
                best = (w, Some(callee));
            }
        }
        self.on_path.pop();
        let worst = own + granted.unwrap_or(0) + best.0;
        self.memo.insert((node, covered), (worst, best.1));
        worst
    }

    /// The deepest path from `node`, from the memo.
    fn path(&self, node: usize) -> Vec<usize> {
        let mut out = vec![node];
        let (mut at, mut covered) = (node, false);
        // A path visits each function once (the graph is checked for cycles separately).
        while let Some(&(_, Some(next))) = self.memo.get(&(at, covered)) {
            if out.contains(&next) {
                break;
            }
            covered = covered || self.indirect_of.contains_key(&at);
            out.push(next);
            at = next;
        }
        out
    }
}

/// The entry points among `nodes`: the `aldis_*` functions of the C ABI, and the global
/// functions of the Rust library crates.
fn entry_points(nodes: &[Node]) -> Vec<(Kind, usize)> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.global)
        .filter_map(|(i, n)| {
            if n.krate == "aldis_ffi" {
                n.names
                    .iter()
                    .any(|s| s.starts_with("aldis_"))
                    .then_some((Kind::CAbi, i))
            } else if n.krate == "aldis" || n.krate.starts_with("aldis_") {
                Some((Kind::Rust, i))
            } else {
                None
            }
        })
        .collect()
}

fn applies(e: &Exemption, target: &str) -> bool {
    e.targets.is_empty() || e.targets.iter().any(|t| t == target)
}

/// Walks every entry point of `nodes` on `target`; returns the analysis and its problems: the
/// gaps, the cycles, and the exemptions that apply to nothing.
fn analyse(nodes: &[Node], target: &str, exemptions: &[Exemption]) -> (Analysis, Vec<String>) {
    let mut frame_of = HashMap::new();
    let mut indirect_of = HashMap::new();
    let mut problems = Vec::new();
    for (ex, e) in exemptions.iter().enumerate() {
        if !applies(e, target) {
            continue;
        }
        for (i, _) in nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.display == e.function)
        {
            match e.bound {
                Bound::Frame(b) => frame_of.insert(i, (b, ex)),
                Bound::Indirect(b) => indirect_of.insert(i, (b, ex)),
            };
        }
    }
    let mut walk = Walk {
        nodes,
        frame_of: &frame_of,
        indirect_of: &indirect_of,
        memo: HashMap::new(),
        on_path: Vec::new(),
        out: Analysis::default(),
        entry: 0,
    };
    let mut entries = Vec::new();
    for (kind, node) in entry_points(nodes) {
        walk.entry = node;
        let worst = walk.worst(node, false);
        entries.push(Entry {
            kind,
            node,
            worst,
            path: walk.path(node),
        });
    }
    let mut out = std::mem::take(&mut walk.out);
    out.entries = entries;
    // An `indirect` exemption applies when an indirect call or inline assembly is reachable
    // from its function.
    for (&node, &(_, ex)) in &indirect_of {
        if !out.reached.contains(&node) {
            continue;
        }
        let mut seen = BTreeSet::from([node]);
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if nodes[n].ir.is_some_and(|f| f.indirect > 0 || f.asm > 0) {
                out.used.insert(ex);
                break;
            }
            for &c in &nodes[n].edges {
                if seen.insert(c) {
                    stack.push(c);
                }
            }
        }
    }
    let name = |i: usize| nodes.get(i).map_or("?", |n| n.display.as_str());
    for (node, (gaps, entry)) in &out.gaps {
        for gap in gaps {
            problems.push(format!(
                "`{}` {} (reached from `{}`): the graph is incomplete; remove the cause, or \
                 exempt it with a stated bound in {BUDGETS}",
                name(*node),
                gap.describe(),
                name(*entry)
            ));
        }
    }
    for cycle in &out.cycles {
        let mut names: Vec<&str> = cycle.iter().map(|&i| name(i)).collect();
        names.push(name(cycle[0]));
        problems.push(format!("recursion (GEN-022): {}", names.join(" → ")));
    }
    for (ex, e) in exemptions.iter().enumerate() {
        if applies(e, target) && !out.used.contains(&ex) {
            problems.push(format!(
                "{BUDGETS}:{}: the exemption of `{}` applies to nothing reachable on {target}; \
                 remove it, or name the targets it applies to",
                e.line, e.function
            ));
        }
    }
    (out, problems)
}

/// The entry points over their budget, and a missing budget.
fn over_budget(
    nodes: &[Node],
    target: &str,
    entries: &[Entry],
    budgets: &BTreeMap<String, BTreeMap<String, u64>>,
) -> Vec<String> {
    let Some(per_kind) = budgets.get(target) else {
        return vec![format!("{BUDGETS}: no [[budget]] for {target}")];
    };
    let mut out = Vec::new();
    for e in entries {
        let Some(&budget) = per_kind.get(e.kind.key()) else {
            continue;
        };
        if e.worst > budget {
            out.push(format!(
                "`{}` needs {} bytes of stack, over the {} budget of {budget} bytes on {target} \
                 (deepest path: {})",
                nodes[e.node].display,
                e.worst,
                e.kind.key(),
                path_text(nodes, &e.path)
            ));
        }
    }
    out
}

fn path_text(nodes: &[Node], path: &[usize]) -> String {
    path.iter()
        .map(|&i| {
            let n = &nodes[i];
            n.frame.map_or_else(
                || format!("{} (?)", n.display),
                |f| format!("{} ({f})", n.display),
            )
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

/// Escapes a name for a Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// The VER-011 table of one target.
fn report(
    target: &str,
    nightly: &str,
    nodes: &[Node],
    analysis: &Analysis,
    config: &Config,
    problems: &[String],
) -> String {
    let mut md = format!(
        "# Worst-case stack per entry point — `{target}` (VER-011)\n\n\
         Generated by `cargo xtask stack {target}` on `{nightly}` (`-Z build-std=core`, \
         `-Z emit-stack-sizes`) from the release build of the `aldis-ffi` static library \
         (`--no-default-features --features panic-handler`). The worst case of an entry point \
         is its frame plus the deepest chain of callee frames; the call graph comes from the \
         objects' relocations and is checked complete against their LLVM IR: no indirect call, \
         no recursion (`xtask/src/stack.rs`). Interrupts and the caller's frames are not \
         included.\n\n"
    );
    let budgets = config.budgets.get(target);
    let budget = |k: Kind| budgets.and_then(|b| b.get(k.key())).copied();
    for kind in [Kind::Rust, Kind::CAbi] {
        let of: Vec<&Entry> = analysis.entries.iter().filter(|e| e.kind == kind).collect();
        let max = of.iter().map(|e| e.worst).max().unwrap_or(0);
        let _ = writeln!(
            md,
            "- {} entry points (`{}`): {}, the largest {max} bytes; budget {}",
            match kind {
                Kind::Rust => "Rust",
                Kind::CAbi => "C ABI",
            },
            kind.key(),
            of.len(),
            budget(kind).map_or_else(|| "none".to_owned(), |b| format!("{b} bytes"))
        );
    }
    let _ = writeln!(
        md,
        "- functions reached: {}; problems: {}\n",
        analysis.reached.len(),
        problems.len()
    );
    for p in problems {
        let _ = writeln!(md, "- **Problem:** {p}");
    }
    let applied: Vec<&Exemption> = config
        .exemptions
        .iter()
        .filter(|e| applies(e, target))
        .collect();
    if !applied.is_empty() {
        md.push_str("\n## Exemptions\n\n| Function | Bound |\n|---|---|\n");
        for e in applied {
            let bound = match e.bound {
                Bound::Frame(b) => format!("own frame {b} bytes"),
                Bound::Indirect(b) => format!("indirect calls {b} bytes"),
            };
            let _ = writeln!(md, "| `{}` | {bound} |", cell(&e.function));
        }
    }
    md.push_str(
        "\n## Entry points\n\n| Bytes | Kind | Entry point | Deepest path below it (frame bytes) \
         |\n|---:|---|---|---|\n",
    );
    let mut entries: Vec<&Entry> = analysis.entries.iter().collect();
    entries.sort_by(|a, b| {
        b.worst
            .cmp(&a.worst)
            .then_with(|| nodes[a.node].display.cmp(&nodes[b.node].display))
    });
    for e in entries {
        let _ = writeln!(
            md,
            "| {} | {} | `{}` ({}) | {} |",
            e.worst,
            e.kind.key(),
            cell(&nodes[e.node].display),
            nodes[e.node]
                .frame
                .map_or_else(|| "?".to_owned(), |f| f.to_string()),
            cell(&path_text(nodes, e.path.get(1..).unwrap_or(&[])))
        );
    }
    md.push_str("\n## Frames of the functions reached\n\n| Bytes | Function |\n|---:|---|\n");
    let mut frames: Vec<(u64, &str)> = analysis
        .reached
        .iter()
        .map(|&i| (nodes[i].frame.unwrap_or(0), nodes[i].display.as_str()))
        .collect();
    frames.sort_by(|a, b| b.cmp(a));
    for (f, name) in frames {
        let _ = writeln!(md, "| {f} | `{}` |", cell(name));
    }
    md
}

// ---------------------------------------------------------------------------------------------
// Build

fn run_tool(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{what}: {e}"))
}

/// Builds the measured static library for `target`; returns the archive and the IR files of
/// the crates in it, found through Cargo's artefact messages so that no stale file is read.
fn build(nightly: &str, target: &str) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
    let stream = run_tool(
        Command::new("rustup")
            .args(["run", nightly, "cargo", "build", "-Z", "build-std=core"])
            .args(["--locked", "-p", "aldis-ffi", "--no-default-features"])
            .args([
                "--features",
                "panic-handler",
                "--release",
                "--target",
                target,
            ])
            .arg("--message-format=json-render-diagnostics")
            .env("CARGO_TARGET_DIR", repo::root().join(TARGET_DIR))
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env(
                "RUSTFLAGS",
                format!("{flags} -Z emit-stack-sizes --emit=llvm-ir,link").trim(),
            ),
        &format!("the aldis-ffi build for {target}"),
    )?;
    let mut archive = None;
    let mut dirs = BTreeSet::new();
    for line in stream.lines() {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if text(&msg, &["reason"]) != "compiler-artifact" {
            continue;
        }
        for f in field(&msg, &["filenames"])
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let path = PathBuf::from(f);
            if path.file_name().and_then(|n| n.to_str()) == Some("libaldis_ffi.a") {
                archive = Some(path.clone());
            }
            if let Some(dir) = path.parent() {
                dirs.insert(dir.to_path_buf());
            }
        }
    }
    let archive = archive.ok_or("the build reported no libaldis_ffi.a")?;
    let mut ir = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("ll") {
                ir.push(e.path());
            }
        }
    }
    ir.sort();
    if ir.is_empty() {
        return Err("the build wrote no LLVM IR (--emit=llvm-ir)".to_owned());
    }
    Ok((archive, ir))
}

fn readobj_json(tool: &Path, archive: &Path, args: &[&str]) -> Result<Value, String> {
    let out = run_tool(
        Command::new(tool)
            .arg("--elf-output-style=JSON")
            .args(args)
            .arg(archive),
        "llvm-readobj",
    )?;
    serde_json::from_str(&out).map_err(|e| format!("llvm-readobj JSON: {e}"))
}

/// Measures `target`: builds, reads, analyses, checks the budgets, and writes the report.
/// Returns the number of problems.
fn measure(nightly: &str, target: &str, config: &Config) -> Result<usize, String> {
    let start = std::time::Instant::now();
    let (archive, ir_files) = build(nightly, target)?;
    let readobj = binaries::llvm_tool("llvm-readobj")?;
    let objects = readobj_json(
        &readobj,
        &archive,
        &["--section-headers", "--symbols", "-r", "--stack-sizes"],
    )?;
    let demangled = readobj_json(&readobj, &archive, &["--symbols", "--demangle"])?;
    let mut nodes = graph(&objects, &demangled)?;
    let mut ir = Ir::default();
    for f in &ir_files {
        parse_ir(&repo::read(f)?, &mut ir);
    }
    attach_ir(&mut nodes, &ir);
    let (analysis, mut problems) = analyse(&nodes, target, &config.exemptions);
    problems.extend(over_budget(
        &nodes,
        target,
        &analysis.entries,
        &config.budgets,
    ));
    let md = report(target, nightly, &nodes, &analysis, config, &problems);
    let out = repo::root().join("target/spec");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let file = out.join(format!("stack-{target}.md"));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    for p in &problems {
        println!("error: stack {target}: {p}");
    }
    let largest = |kind: Kind| {
        analysis
            .entries
            .iter()
            .filter(|e| e.kind == kind)
            .max_by_key(|e| e.worst)
            .map_or_else(
                || "none".to_owned(),
                |e| format!("{} bytes (`{}`)", e.worst, nodes[e.node].display),
            )
    };
    println!(
        "stack {target}: {} entry point(s), {} function(s) reached; largest rust {}, c-abi {}; \
         {} problem(s); {:.0} s; report: {}",
        analysis.entries.len(),
        analysis.reached.len(),
        largest(Kind::Rust),
        largest(Kind::CAbi),
        problems.len(),
        start.elapsed().as_secs_f64(),
        repo::rel(&file)
    );
    Ok(problems.len())
}

/// `xtask stack [<target>...]`: every target of [`TARGETS`], or the ones named.
pub fn run(args: &[&str]) -> Result<usize, String> {
    let targets: Vec<&str> = if args.is_empty() {
        TARGETS.to_vec()
    } else {
        args.to_vec()
    };
    if let Some(t) = targets.iter().find(|t| !TARGETS.contains(t)) {
        return Err(format!(
            "stack: unknown target {t} (one of {})",
            TARGETS.join(", ")
        ));
    }
    let nightly = repo::tooling_nightly()?;
    let (config, problems) = config(&repo::read(&repo::root().join(BUDGETS))?);
    for p in &problems {
        println!("error: {p}");
    }
    let mut count = problems.len();
    for t in targets {
        count += measure(&nightly, t, &config)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // req: VER-011, GEN-022
    #[test]
    fn reads_direct_and_indirect_calls_from_the_ir() {
        let ir_text = r#"
@alias_of_f = hidden unnamed_addr alias void (ptr), ptr @f

define internal void @f(ptr %x) unnamed_addr {
start:
  ; call core::fmt::write
  call void @llvm.memcpy.p0.p0.i32(ptr %x, ptr %x, i32 4, i1 false)
  %_0.i = call noundef range(i32 1, 0) i32 @llvm.uadd.sat.i32(i32 1, i32 2)
  %r = tail call noundef zeroext i1 @"quoted name"(ptr %x)
  %v = call i32 (ptr, ...) @printf(ptr %x)
  ret void
}

define void @g(ptr %vtable) {
start:
  %m = load ptr, ptr %vtable, align 4
  %r = tail call noundef zeroext i1 %m(ptr %vtable)
  %t = call %"core::fmt::Arguments" %m(ptr %vtable)
  call void asm sideeffect "bkpt 0xab", ""()
  %buf = alloca [16 x i8], align 4
  %dyn = alloca i8, i32 %n, align 1
  ret void
}
"#;
        let mut ir = Ir::default();
        parse_ir(ir_text, &mut ir);
        assert_eq!(ir.defined["f"], IrFacts::default());
        assert_eq!(
            ir.defined["g"],
            IrFacts {
                indirect: 2,
                asm: 1,
                dynamic_alloca: 1
            }
        );
        assert_eq!(ir.aliases["alias_of_f"], "f");
        assert_eq!(global_name("@\"a b\"(x"), Some("a b"));
        assert_eq!(global_name("@_RNv.x$y-1(z"), Some("_RNv.x$y-1"));
        assert_eq!(global_name("@("), None);
    }

    // req: VER-011
    #[test]
    fn names_the_crate_of_an_archive_member() {
        assert_eq!(
            member_crate("libaldis_ffi.a(aldis_core-f9bd.aldis_core.d358-cgu.0.rcgu.o)"),
            "aldis_core"
        );
        assert_eq!(
            member_crate("libaldis_ffi.a(aldis_ffi.aldis_ffi.f4f0-cgu.0.rcgu.o)"),
            "aldis_ffi"
        );
        assert_eq!(member_crate("core-1.core.2-cgu.0.rcgu.o"), "core");
    }

    fn symbol(name: &str, section: u64, value: u64, size: u64, kind: &str, binding: &str) -> Value {
        json!({"Symbol": {
            "Name": {"Name": name}, "Value": value, "Size": size,
            "Binding": {"Name": binding}, "Type": {"Name": kind},
            "Section": {"Name": "", "Value": section}
        }})
    }

    fn section(index: u64, info: u64, exec: bool) -> Value {
        let flags = if exec {
            json!([{"Name": "SHF_ALLOC"}, {"Name": "SHF_EXECINSTR"}])
        } else {
            json!([])
        };
        json!({"Section": {"Index": index, "Info": info, "Flags": {"Flags": flags}}})
    }

    fn reloc(offset: u64, symbol: u64) -> Value {
        json!({"Relocation": {"Offset": offset, "Type": {"Name": "R_ARM_THM_CALL"},
            "Symbol": {"Name": "", "Value": symbol}}})
    }

    /// Two objects: `aldis_core` with `entry` (global, section 2) calling `helper` (local,
    /// section 3) and the undefined `memcpy`, and taking a label in its own section; and
    /// `compiler_builtins` defining `memcpy` (weak) and an alias of it.
    fn archive() -> (Value, Value) {
        let core_syms = vec![
            symbol("", 0, 0, 0, "None", "Local"),
            symbol("entry", 2, 1, 40, "Function", "Global"),
            symbol("helper", 3, 0, 20, "Function", "Local"),
            symbol("memcpy", 0, 0, 0, "None", "Global"),
            symbol(".Ltmp0", 2, 8, 0, "None", "Local"),
            symbol(".text.helper", 3, 0, 0, "Section", "Local"),
        ];
        let objects = json!([
            {
                "FileSummary": {"File": "lib.a(aldis_core-1.aldis_core.2-cgu.0.rcgu.o)"},
                "Sections": [section(2, 0, true), section(3, 0, true), section(4, 2, false),
                             section(5, 3, false), section(6, 0, false)],
                "Symbols": core_syms,
                "Relocations": [
                    {"SectionIndex": 4, "Relocs": [reloc(4, 2), reloc(8, 3), reloc(12, 4)]},
                    {"SectionIndex": 5, "Relocs": [reloc(2, 0)]},
                ],
                "StackSizes": [
                    {"Entry": {"Functions": ["entry"], "Size": 32}},
                    {"Entry": {"Functions": ["helper"], "Size": 16}},
                ]
            },
            {
                "FileSummary": {"File": "lib.a(compiler_builtins-3.compiler_builtins.4-cgu.0.rcgu.o)"},
                "Sections": [section(2, 0, true)],
                "Symbols": [symbol("", 0, 0, 0, "None", "Local"),
                            symbol("memcpy", 2, 0, 8, "Function", "Weak"),
                            symbol("memcpy_alias", 2, 0, 8, "Function", "Global")],
                "StackSizes": [{"Entry": {"Functions": ["memcpy", "memcpy_alias"], "Size": 8}}]
            }
        ]);
        let demangled = json!([
            {"Symbols": [{"Symbol": {"Name": {"Name": ""}}},
                         {"Symbol": {"Name": {"Name": "aldis_core::entry"}}},
                         {"Symbol": {"Name": {"Name": "aldis_core::helper"}}}]},
            {"Symbols": []}
        ]);
        (objects, demangled)
    }

    // req: VER-011, GEN-022
    #[test]
    fn builds_the_call_graph_from_relocations() {
        let (objects, demangled) = archive();
        let nodes = graph(&objects, &demangled).unwrap();
        assert_eq!(nodes.len(), 3, "{nodes:#?}");
        let entry = &nodes[0];
        assert_eq!(entry.display, "aldis_core::entry");
        assert_eq!(entry.krate, "aldis_core");
        assert!(entry.global);
        assert_eq!(entry.frame, Some(32));
        // helper, and memcpy resolved in the other object; not the label in its own section.
        assert_eq!(entry.edges, BTreeSet::from([1, 2]));
        assert!(entry.undefined.is_empty());
        let memcpy = &nodes[2];
        assert_eq!(memcpy.names, ["memcpy", "memcpy_alias"]);
        assert_eq!(memcpy.frame, Some(8));
        assert_eq!(memcpy.krate, "compiler_builtins");
        // The relocation without a symbol in helper's section adds nothing.
        assert!(nodes[1].edges.is_empty());
        assert_eq!(entry_points(&nodes), [(Kind::Rust, 0)]);
    }

    fn node(name: &str, krate: &str, frame: Option<u64>, edges: &[usize]) -> Node {
        Node {
            names: vec![name.to_owned()],
            display: name.to_owned(),
            krate: krate.to_owned(),
            global: name.starts_with("aldis_") || krate != "aldis_ffi",
            frame,
            edges: edges.iter().copied().collect(),
            undefined: BTreeSet::new(),
            ir: Some(IrFacts::default()),
        }
    }

    // req: VER-011
    #[test]
    fn the_worst_case_is_the_deepest_chain_of_frames() {
        let mut nodes = vec![
            node("aldis_x_call", "aldis_ffi", Some(100), &[2, 3]),
            node("aldis_core::f", "aldis_core", Some(40), &[3]),
            node("helper", "aldis_ffi", Some(10), &[3]),
            node("leaf", "core", Some(8), &[]),
        ];
        nodes[2].global = false;
        nodes[3].global = false;
        let (a, problems) = analyse(&nodes, "t", &[]);
        assert!(problems.is_empty(), "{problems:?}");
        let worst: Vec<(Kind, u64, Vec<usize>)> = a
            .entries
            .iter()
            .map(|e| (e.kind, e.worst, e.path.clone()))
            .collect();
        assert_eq!(
            worst,
            [
                (Kind::CAbi, 118, vec![0, 2, 3]),
                (Kind::Rust, 48, vec![1, 3])
            ]
        );
        let budgets = BTreeMap::from([(
            "t".to_owned(),
            BTreeMap::from([("rust".to_owned(), 48), ("c-abi".to_owned(), 117)]),
        )]);
        let over = over_budget(&nodes, "t", &a.entries, &budgets);
        assert_eq!(over.len(), 1, "{over:?}");
        assert!(
            over[0].contains("`aldis_x_call` needs 118 bytes"),
            "{over:?}"
        );
        assert!(
            over[0].contains("c-abi budget of 117 bytes on t"),
            "{over:?}"
        );
        assert_eq!(
            over_budget(&nodes, "u", &a.entries, &budgets),
            ["xtask/stack-budgets.toml: no [[budget]] for u"]
        );
    }

    // req: GEN-022, VER-011
    #[test]
    fn recursion_and_gaps_fail_unless_exempted() {
        let mut nodes = vec![
            node("aldis_core::a", "aldis_core", Some(16), &[1, 2, 3]),
            node("b", "core", Some(8), &[0]),
            node("asm_fn", "compiler_builtins", None, &[]),
            node("dyn_caller", "core", Some(4), &[]),
        ];
        nodes[1].global = false;
        nodes[2].ir = None;
        nodes[3].ir = Some(IrFacts {
            indirect: 1,
            ..IrFacts::default()
        });
        nodes[3].undefined.insert("missing".to_owned());
        let (_, problems) = analyse(&nodes, "t", &[]);
        let text = problems.join("\n");
        assert!(
            text.contains("recursion (GEN-022): aldis_core::a → b → aldis_core::a"),
            "{text}"
        );
        assert!(
            text.contains("`asm_fn` has no `.stack_sizes` entry"),
            "{text}"
        );
        assert!(text.contains("`asm_fn` has no LLVM IR"), "{text}");
        assert!(
            text.contains("`dyn_caller` makes 1 indirect call(s)"),
            "{text}"
        );
        assert!(text.contains("`dyn_caller` calls `missing`"), "{text}");
        assert_eq!(problems.len(), 5, "{text}");

        // Without the cycle, the exemptions close the gaps and add their bounds.
        nodes[1].edges.clear();
        nodes[3].undefined.clear();
        let exemptions = [
            Exemption {
                function: "asm_fn".to_owned(),
                targets: vec!["t".to_owned()],
                bound: Bound::Frame(24),
                line: 1,
            },
            Exemption {
                function: "aldis_core::a".to_owned(),
                targets: Vec::new(),
                bound: Bound::Indirect(100),
                line: 2,
            },
            Exemption {
                function: "nowhere".to_owned(),
                targets: Vec::new(),
                bound: Bound::Frame(1),
                line: 3,
            },
            Exemption {
                function: "asm_fn".to_owned(),
                targets: vec!["other".to_owned()],
                bound: Bound::Frame(1),
                line: 4,
            },
        ];
        let (a, problems) = analyse(&nodes, "t", &exemptions);
        assert_eq!(
            problems,
            [
                "xtask/stack-budgets.toml:3: the exemption of `nowhere` applies to nothing \
              reachable on t; remove it, or name the targets it applies to"
            ]
        );
        // 16 (a) + 100 (its indirect calls) + 24 (asm_fn, the deepest callee).
        assert_eq!(a.entries[0].worst, 140);
        assert_eq!(a.entries[0].path, [0, 2]);

        // An `indirect` exemption with no indirect call below it is stale.
        nodes[3].ir = Some(IrFacts::default());
        let (_, problems) = analyse(&nodes, "t", &exemptions[..2]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains(":2: the exemption of `aldis_core::a`"),
            "{problems:?}"
        );
    }

    // req: VER-011
    #[test]
    fn reads_the_budgets_and_exemptions() {
        let (c, problems) = config(
            "# comment\n\
             [[budget]]\ntarget = \"t\"\nrust = 1_024\nc-abi = 10240\n\
             [[exemption]]\nfunction = \"f\"\ntargets = [\"t\", \"u\"]\nframe = 24\nreason = \"asm\"\n\
             [[exemption]]\nfunction = \"g\"\nindirect = 0\nreason = \"caller's writer\"\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(c.budgets["t"]["rust"], 1024);
        assert_eq!(c.budgets["t"]["c-abi"], 10240);
        assert_eq!(c.exemptions.len(), 2);
        assert_eq!(c.exemptions[0].targets, ["t", "u"]);
        assert_eq!(c.exemptions[0].bound, Bound::Frame(24));
        assert_eq!(c.exemptions[1].bound, Bound::Indirect(0));
        assert_eq!(c.exemptions[1].line, 11);

        let (_, problems) = config(
            "rust = 1\n[[budget]]\ntarget = \"t\"\nrust = 1\n[[budget]]\ntarget = \"t\"\n\
             [[exemption]]\nfunction = \"f\"\nframe = 1\nindirect = 2\nreason = \"x\"\n\
             [[exemption]]\nfunction = \"g\"\nframe = 1\ncolour = \"red\"\n[[other]]\nnot a pair\n",
        );
        let text = problems.join("\n");
        for expected in [
            ":1: `rust` outside a table",
            ":2: t needs `c-abi = <bytes>`",
            ":5: t has two budgets",
            ":7: the exemption of f states one bound in bytes",
            ":12: unknown key `colour`",
            ":12: the exemption of g gives no reason",
            ":16: unknown table `[[other]]`",
            ":17: expected `key = value`",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
    }

    // req: VER-011
    #[test]
    fn the_committed_budgets_cover_every_target() {
        let text = std::fs::read_to_string(repo::root().join(BUDGETS)).unwrap();
        let (c, problems) = config(&text);
        assert!(problems.is_empty(), "{problems:?}");
        for t in TARGETS {
            assert!(c.budgets.contains_key(*t), "{t}");
        }
        for e in &c.exemptions {
            for t in &e.targets {
                assert!(TARGETS.contains(&t.as_str()), "{t}");
            }
        }
    }
}
