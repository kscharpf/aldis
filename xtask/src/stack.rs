//! `xtask stack [<target>...]`: the worst-case stack use of every entry point of the library, per
//! bare-metal target, against a budget per target (VER-011, extended by M2 plan WP-7; GEN-022).
//!
//! **Builds.** Two builds per target, both on the tooling nightly (RS-002) with
//! `-Z build-std=core`, so that `core` and `compiler_builtins` are compiled with the same flags as
//! the library, and with `-Z emit-stack-sizes --emit=llvm-ir,link`:
//! - `ffi`: the static library of the C ABI as it ships (`cargo build -p aldis-ffi
//!   --no-default-features --features panic-handler --release`). The archive holds one object per
//!   crate (the release profile's LTO does not apply to a crate that is also an `rlib`).
//! - `exerciser`: the exerciser (`exerciser/`), which calls every public function with real
//!   capacities, without LTO (`CARGO_PROFILE_RELEASE_LTO=off`), its own crate in 256 codegen units
//!   without MIR inlining (`-C codegen-units=256 -Z inline-mir=no`), so that the generic public
//!   functions it instantiates stay functions of their own, and with `-C save-temps`, which keeps
//!   their objects. The library crates' objects are those of their `rlib`s.
//!
//! Each object has a `.stack_sizes` section, which gives each function's frame, and each
//! compilation writes its LLVM IR; only the files that the build's own compilations wrote are read.
//!
//! **Graph.** The nodes are the functions of a build's objects; a function's callees are the
//! functions its relocations name: calls, tail calls, and addresses taken, which can only
//! over-approximate. The IR of each function says whether it makes an indirect call (a `dyn` call
//! or a call through a function pointer), runs inline assembly, allocates a dynamic amount of
//! stack, or never returns. GEN-022 forbids recursion, so the graph is a DAG, and a function's
//! worst case is its frame plus the largest worst case of its callees. The walk stops at a
//! diverging function of `core` (the panic machinery), counting its frame: GEN-020 forbids panics
//! and VER-010 proves that no panic path survives the release link, so its callees never run.
//!
//! **Entry points.** Every `aldis_*` function of the C ABI (in `ffi`), and every function whose
//! demangled path names a public function of a library crate ([`api`]: by the item's Rust
//! visibility, not by the symbol's binding), wherever it is emitted: a monomorphisation of a
//! generic function lies, with local binding, in the object of the crate that instantiates it.
//! `pub(crate)` helpers are not entry points. A function that the source index cannot see (one
//! that a `macro_rules!` body defines) counts when it is a global symbol of its own crate's
//! object. An entry point measured in both builds takes the larger worst case. The report lists
//! the public functions that neither build has as a function of its own: they are inlined into
//! every caller (their stack is in the callers' frames), or nothing instantiates them.
//!
//! **Completeness.** A reachable function without a frame (hand-written assembly), without IR,
//! with an indirect call, inline assembly, or a dynamic allocation, a call to a symbol that no
//! object defines, and a cycle are each a failure, unless [`BUDGETS`] lists an exemption with a
//! stated bound: `frame` for a function in assembly (its own frame, read from the disassembly;
//! its direct callees stay in the graph), or `indirect` for a function that makes indirect calls
//! itself (the bytes they may take, added to its worst case). An `indirect` exemption accepts only
//! the named function's own indirect calls: not inline assembly, and nothing in its callees. An
//! exemption that applies to nothing in either build is a failure too, so the list stays exact.
//!
//! **Gate.** Each entry point's worst case must not exceed its target's budget for its kind
//! (`rust` or `c-abi`). The report `target/spec/stack-<target>.md` lists every entry point with
//! its worst case, build, and deepest path, and the frame of every reachable function; it is the
//! table that VER-011 publishes per release (the nightly job `stack-sizes`).
//!
//! The bound does not include interrupt handlers or the caller's own frames, and holds for these
//! builds: another compiler version, profile, or set of features can change it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::{api, binaries, repo};

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
    /// The function never returns (`noreturn`).
    noreturn: bool,
}

impl IrFacts {
    const fn merge(self, other: Self) -> Self {
        Self {
            indirect: self.indirect + other.indirect,
            asm: self.asm + other.asm,
            dynamic_alloca: self.dynamic_alloca + other.dynamic_alloca,
            noreturn: self.noreturn || other.noreturn,
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

/// The comma-separated operands of an instruction, split only outside `{}`, `[]`, `<>`, and
/// `()`, so that an aggregate type such as `{ i32, i32 }` stays one operand.
fn operands(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0_usize, 0);
    for (i, ch) in s.char_indices() {
        match ch {
            '{' | '[' | '<' | '(' => depth += 1,
            '}' | ']' | '>' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out
}

/// Whether the operands of an `alloca` (the text after the keyword) give a run-time element
/// count: `alloca <ty>, <ty> %n`, and not `alloca <ty>, align 4` or a constant count.
fn dynamic_count(rest: &str) -> bool {
    operands(rest).get(1).is_some_and(|count| {
        let mut words = count.split_whitespace();
        words
            .next()
            .is_some_and(|w| w != "align" && !w.starts_with("addrspace"))
            && words.next().is_some_and(|v| v.starts_with('%'))
    })
}

/// Reads one IR module into `ir`.
fn parse_ir(text: &str, ir: &mut Ir) {
    let mut current: Option<(String, IrFacts)> = None;
    // LLVM writes `; Function Attrs: …` before each definition.
    let mut attrs = String::new();
    for line in text.lines() {
        if let Some(a) = line.strip_prefix("; Function Attrs:") {
            a.clone_into(&mut attrs);
            continue;
        }
        if let Some(rest) = line.strip_prefix("define ") {
            if let Some(name) = rest.find('@').and_then(|at| global_name(&rest[at..])) {
                let noreturn = attrs.split_whitespace().any(|w| w == "noreturn")
                    || rest.split_whitespace().any(|w| w == "noreturn");
                current = Some((
                    name.to_owned(),
                    IrFacts {
                        noreturn,
                        ..IrFacts::default()
                    },
                ));
            }
            attrs.clear();
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
            if dynamic_count(&rest) {
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

/// One entry point's result in one build.
#[derive(Debug, Clone)]
struct Entry {
    kind: Kind,
    node: usize,
    worst: u64,
    /// The deepest path, from the entry point.
    path: Vec<usize>,
}

/// The result of one build.
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
    /// The functions of the panic machinery reached, where the walk stopped.
    panicking: BTreeSet<usize>,
}

/// Whether a function is an entry of the panic machinery: a function of `core` that never
/// returns (`core::panicking::panic_fmt`, `panic_bounds_check`, the `slice_*_fail`
/// functions, `unwrap_failed`, …; with `panic = "abort"` nothing else in `core` diverges).
/// GEN-020 forbids panics and VER-010 proves that no panic path survives the release link, but a
/// build without LTO keeps some of them; the walk counts the frame of that function and stops
/// there, since its callees (the message formatting and the panic handler) never run.
fn panic_entry(node: &Node) -> bool {
    node.krate == "core" && node.ir.is_some_and(|f| f.noreturn)
}

/// The depth-first walk of [`analyse`].
struct Walk<'a> {
    nodes: &'a [Node],
    /// Per node: the frame that an exemption states, with the exemption's index.
    frame_of: &'a HashMap<usize, (u64, usize)>,
    /// Per node: the bytes that an exemption grants the node's own indirect calls, with its
    /// index.
    indirect_of: &'a HashMap<usize, (u64, usize)>,
    /// Per node: its worst case and the callee on its deepest path.
    memo: HashMap<usize, (u64, Option<usize>)>,
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

    /// The gaps of `node` itself, and its own frame. An `indirect` exemption accepts only the
    /// node's own indirect calls: never inline assembly, and nothing in its callees.
    fn own(&mut self, node: usize) -> u64 {
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
        let granted = self.indirect_of.get(&node).copied();
        match n.ir {
            None if !assembly => self.gap(node, Gap::NoIr),
            None => {}
            Some(f) => {
                if f.dynamic_alloca > 0 {
                    self.gap(node, Gap::DynamicAlloca(f.dynamic_alloca));
                }
                if f.indirect > 0 {
                    match granted {
                        Some((_, ex)) => {
                            self.out.used.insert(ex);
                        }
                        None => self.gap(node, Gap::Indirect(f.indirect)),
                    }
                }
                if f.asm > 0 {
                    self.gap(node, Gap::Asm(f.asm));
                }
            }
        }
        for name in undefined {
            self.gap(node, Gap::Undefined(name));
        }
        frame
    }

    /// The worst case of `node`: its frame, the bytes an exemption grants its indirect calls,
    /// and the largest worst case of its callees.
    fn worst(&mut self, node: usize) -> u64 {
        if let Some(pos) = self.on_path.iter().position(|&n| n == node) {
            self.out.cycles.insert(self.on_path[pos..].to_vec());
            return 0;
        }
        if let Some(&(w, _)) = self.memo.get(&node) {
            return w;
        }
        self.out.reached.insert(node);
        if panic_entry(&self.nodes[node]) {
            // The walk stops at the panic machinery: its own frame counts, its callees (the
            // message formatting and the panic handler) never run (GEN-020, VER-010).
            self.out.panicking.insert(node);
            let frame = self.nodes[node].frame.unwrap_or(0);
            self.memo.insert(node, (frame, None));
            return frame;
        }
        let own = self.own(node);
        let granted = self.indirect_of.get(&node).map_or(0, |&(bytes, _)| bytes);
        self.on_path.push(node);
        let mut best: (u64, Option<usize>) = (0, None);
        let edges: Vec<usize> = self.nodes[node].edges.iter().copied().collect();
        for callee in edges {
            let w = self.worst(callee);
            if best.1.is_none() || w > best.0 {
                best = (w, Some(callee));
            }
        }
        self.on_path.pop();
        let worst = own + granted + best.0;
        self.memo.insert(node, (worst, best.1));
        worst
    }

    /// The deepest path from `node`, from the memo.
    fn path(&self, node: usize) -> Vec<usize> {
        let mut out = vec![node];
        let mut at = node;
        // A path visits each function once (the graph is checked for cycles separately).
        while let Some(&(_, Some(next))) = self.memo.get(&at) {
            if out.contains(&next) {
                break;
            }
            out.push(next);
            at = next;
        }
        out
    }
}

/// Whether a crate is a Rust library crate whose public functions are entry points (not the C
/// ABI crate, whose Rust items are not its API).
fn library_crate(krate: &str) -> bool {
    (krate == "aldis" || krate.starts_with("aldis_")) && krate != "aldis_ffi"
}

/// The crate a key names.
fn key_crate(key: &api::Key) -> &str {
    match key {
        api::Key::Free { krate, .. }
        | api::Key::Method { krate, .. }
        | api::Key::Provided { krate, .. } => krate,
    }
}

/// The entry points among `nodes`, and how many Rust entry points the source index did not
/// know (taken by the fallback rule).
///
/// - C ABI (when `c_abi`): the global `aldis_*` functions of the `aldis_ffi` object.
/// - Rust: every function whose demangled path names a public function of a library crate in
///   `index`, wherever it is emitted: a monomorphisation of a generic function lies in the
///   object of the crate that instantiates it, with local binding. A function the index does
///   not know (one that a `macro_rules!` body defines) counts when it is a global symbol of its
///   own crate's object, the rule before the index.
fn entry_points(nodes: &[Node], index: &api::Index, c_abi: bool) -> (Vec<(Kind, usize)>, usize) {
    let mut out = Vec::new();
    let mut unindexed = 0;
    for (i, n) in nodes.iter().enumerate() {
        if n.krate == "aldis_ffi" && n.global && n.names.iter().any(|s| s.starts_with("aldis_")) {
            if c_abi {
                out.push((Kind::CAbi, i));
            }
            continue;
        }
        let keys = api::keys_of(&n.display);
        let Some(first) = keys.first() else {
            continue;
        };
        if keys.iter().any(|k| index.knows(k)) {
            if keys.iter().any(|k| index.public(k)) && library_crate(key_crate(first)) {
                out.push((Kind::Rust, i));
            }
        } else if library_crate(key_crate(first)) && n.global && n.krate == key_crate(first) {
            out.push((Kind::Rust, i));
            unindexed += 1;
        }
    }
    (out, unindexed)
}

fn applies(e: &Exemption, target: &str) -> bool {
    e.targets.is_empty() || e.targets.iter().any(|t| t == target)
}

/// Walks the entry points of `nodes` on `target`; returns the analysis and its problems: the
/// gaps and the cycles.
fn analyse(
    nodes: &[Node],
    entry_points: &[(Kind, usize)],
    target: &str,
    exemptions: &[Exemption],
) -> (Analysis, Vec<String>) {
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
    for &(kind, node) in entry_points {
        walk.entry = node;
        let worst = walk.worst(node);
        entries.push(Entry {
            kind,
            node,
            worst,
            path: walk.path(node),
        });
    }
    let mut out = std::mem::take(&mut walk.out);
    out.entries = entries;
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
    (out, problems)
}

/// The exemptions for `target` that no build used.
fn stale(exemptions: &[Exemption], target: &str, used: &BTreeSet<usize>) -> Vec<String> {
    exemptions
        .iter()
        .enumerate()
        .filter(|(ex, e)| applies(e, target) && !used.contains(ex))
        .map(|(_, e)| {
            format!(
                "{BUDGETS}:{}: the exemption of `{}` applies to nothing reachable on {target}; \
                 remove it, or name the targets it applies to",
                e.line, e.function
            )
        })
        .collect()
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

/// One entry point across the builds: its largest worst case.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Measured {
    kind: Kind,
    display: String,
    worst: u64,
    frame: Option<u64>,
    /// The deepest path below the entry point, with frames.
    below: String,
    /// The build it was measured in.
    unit: &'static str,
}

/// The entry points of one build, as [`Measured`].
fn measured(unit: &'static str, nodes: &[Node], analysis: &Analysis) -> Vec<Measured> {
    analysis
        .entries
        .iter()
        .map(|e| Measured {
            kind: e.kind,
            display: nodes[e.node].display.clone(),
            worst: e.worst,
            frame: nodes[e.node].frame,
            below: path_text(nodes, e.path.get(1..).unwrap_or(&[])),
            unit,
        })
        .collect()
}

/// Keeps, per entry point and kind, the build with the largest worst case.
fn merge(all: Vec<Measured>) -> Vec<Measured> {
    let mut by: BTreeMap<(Kind, String), Measured> = BTreeMap::new();
    for m in all {
        let key = (m.kind, m.display.clone());
        match by.get(&key) {
            Some(old) if old.worst >= m.worst => {}
            _ => {
                by.insert(key, m);
            }
        }
    }
    let mut out: Vec<Measured> = by.into_values().collect();
    out.sort_by(|a, b| {
        b.worst
            .cmp(&a.worst)
            .then_with(|| a.display.cmp(&b.display))
    });
    out
}

/// The entry points over their budget, and a missing budget.
fn over_budget(
    target: &str,
    entries: &[Measured],
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
                "`{}` needs {} bytes of stack, {} over the {} budget of {budget} bytes on \
                 {target} (build {}; its frame {}, then: {})",
                e.display,
                e.worst,
                e.worst - budget,
                e.kind.key(),
                e.unit,
                e.frame.map_or_else(|| "?".to_owned(), |f| f.to_string()),
                e.below
            ));
        }
    }
    out
}

/// Escapes a name for a Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// What one target's report shows.
struct Summary<'a> {
    target: &'a str,
    nightly: &'a str,
    entries: &'a [Measured],
    /// Per function reached, in any build: its largest frame.
    frames: &'a BTreeMap<String, u64>,
    /// The public functions measured in no build.
    unseen: &'a [String],
    unindexed: usize,
    /// Per build, the functions of the panic machinery reached (where the walk stopped).
    panics: &'a [(&'static str, Vec<String>)],
    problems: &'a [String],
}

/// The summary lines of a report: the entry points per kind, the limits of the measurement, the
/// panic paths, and the problems.
fn summary(md: &mut String, s: &Summary<'_>, config: &Config) {
    let budgets = config.budgets.get(s.target);
    let budget = |k: Kind| budgets.and_then(|b| b.get(k.key())).copied();
    for kind in [Kind::Rust, Kind::CAbi] {
        let of: Vec<&Measured> = s.entries.iter().filter(|e| e.kind == kind).collect();
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
        "- Rust entry points taken without the source index (macro-generated): {}\n\
         - public functions measured in neither build (inlined at every call, or never \
         instantiated): {}\n\
         - functions reached: {}; problems: {}",
        s.unindexed,
        s.unseen.len(),
        s.frames.len(),
        s.problems.len()
    );
    for (unit, names) in s.panics {
        let list = if names.is_empty() {
            "none".to_owned()
        } else {
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            md,
            "- panic paths in build `{unit}`, where the walk stops (GEN-020, VER-010): {list}"
        );
    }
    md.push('\n');
    for p in s.problems {
        let _ = writeln!(md, "- **Problem:** {p}");
    }
}

/// The VER-011 table of one target.
fn report(s: &Summary<'_>, config: &Config) -> String {
    let (target, nightly) = (s.target, s.nightly);
    let mut md = format!(
        "# Worst-case stack per entry point — `{target}` (VER-011)\n\n\
         Generated by `cargo xtask stack {target}` on `{nightly}` (`-Z build-std=core`, \
         `-Z emit-stack-sizes`) from two builds: `ffi`, the release build of the `aldis-ffi` \
         static library (`--no-default-features --features panic-handler`), and `exerciser`, \
         the exerciser (`exerciser/`, which calls every public function with real capacities) \
         without LTO, its own crate in 256 codegen units and without MIR inlining, so that the \
         public functions it instantiates are functions of their own. The worst case of an \
         entry point is its frame plus the deepest chain of callee frames, the largest of the \
         two builds; the call graph comes from the objects' relocations and is checked complete \
         against their LLVM IR: no indirect call, no recursion (`xtask/src/stack.rs`). \
         Interrupts and the caller's frames are not included.\n\n"
    );
    summary(&mut md, s, config);
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
                Bound::Indirect(b) => format!("its own indirect calls {b} bytes"),
            };
            let _ = writeln!(md, "| `{}` | {bound} |", cell(&e.function));
        }
    }
    md.push_str(
        "\n## Entry points\n\n| Bytes | Kind | Build | Entry point (frame) | Deepest path below it \
         (frame bytes) |\n|---:|---|---|---|---|\n",
    );
    for e in s.entries {
        let _ = writeln!(
            md,
            "| {} | {} | {} | `{}` ({}) | {} |",
            e.worst,
            e.kind.key(),
            e.unit,
            cell(&e.display),
            e.frame.map_or_else(|| "?".to_owned(), |f| f.to_string()),
            cell(&e.below)
        );
    }
    if !s.unseen.is_empty() {
        md.push_str(
            "\n## Public functions measured in neither build\n\nEach is inlined into its \
             callers at every call in both builds (its stack is in their frames), or no build \
             instantiates it.\n\n",
        );
        for u in s.unseen {
            let _ = writeln!(md, "- `{}`", cell(u));
        }
    }
    md.push_str("\n## Frames of the functions reached\n\n| Bytes | Function |\n|---:|---|\n");
    let mut frames: Vec<(u64, &str)> = s.frames.iter().map(|(n, &f)| (f, n.as_str())).collect();
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

/// The files of the `compiler-artifact` messages of a Cargo JSON stream.
fn artefacts(stream: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in stream.lines() {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if text(&msg, &["reason"]) != "compiler-artifact" {
            continue;
        }
        out.extend(
            list(&msg, &["filenames"])
                .iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from),
        );
    }
    out
}

/// The IR files that the compilations of `artefacts` wrote: in each artefact's directory, the
/// files named after its stem (`aldis_tc-<hash>.ll`, `aldis_tc-<hash>.<cgu>.rcgu.ll`, or
/// `aldis_ffi.ll` for the crate of the build), so that an older compilation's file in the same
/// directory, or another unit's, is not read.
fn ir_files(artefacts: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = BTreeSet::new();
    for a in artefacts {
        let (Some(dir), Some(name)) = (a.parent(), a.file_name().and_then(|n| n.to_str())) else {
            continue;
        };
        let stem = name
            .strip_prefix("lib")
            .unwrap_or(name)
            .rsplit_once('.')
            .map_or(name, |(s, _)| s);
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let file = e.file_name();
            let Some(f) = file.to_str() else {
                continue;
            };
            let Some(rest) = f.strip_prefix(stem) else {
                continue;
            };
            let ll = Path::new(f).extension().is_some_and(|x| x == "ll");
            if ll && rest.starts_with('.') {
                out.insert(e.path());
            }
        }
    }
    out.into_iter().collect()
}

/// One build to measure: its objects and archives, and its IR files.
struct Built {
    name: &'static str,
    objects: Vec<PathBuf>,
    ir: Vec<PathBuf>,
}

/// `rustup run <nightly> cargo <args>` with the measurement's flags and target directory.
fn cargo(nightly: &str, args: &[&str], env: &[(&str, &str)], what: &str) -> Result<String, String> {
    let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
    let mut cmd = Command::new("rustup");
    cmd.args(["run", nightly, "cargo"])
        .args(args)
        .env("CARGO_TARGET_DIR", repo::root().join(TARGET_DIR))
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env(
            "RUSTFLAGS",
            format!("{flags} -Z emit-stack-sizes --emit=llvm-ir,link").trim(),
        );
    for (k, v) in env {
        cmd.env(k, v);
    }
    run_tool(&mut cmd, what)
}

/// Builds the static library of the C ABI for `target`, as it ships.
fn build_ffi(nightly: &str, target: &str) -> Result<Built, String> {
    let stream = cargo(
        nightly,
        &[
            "build",
            "-Z",
            "build-std=core",
            "--locked",
            "-p",
            "aldis-ffi",
            "--no-default-features",
            "--features",
            "panic-handler",
            "--release",
            "--target",
            target,
            "--message-format=json-render-diagnostics",
        ],
        &[],
        &format!("the aldis-ffi build for {target}"),
    )?;
    let files = artefacts(&stream);
    let archive = files
        .iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("libaldis_ffi.a"))
        .ok_or("the build reported no libaldis_ffi.a")?
        .clone();
    let ir = ir_files(&files);
    if ir.is_empty() {
        return Err("the aldis-ffi build wrote no LLVM IR (--emit=llvm-ir)".to_owned());
    }
    Ok(Built {
        name: "ffi",
        objects: vec![archive],
        ir,
    })
}

/// Builds the exerciser for `target` without LTO; its own crate in 256 codegen units without
/// MIR inlining (`-C codegen-units=256 -Z inline-mir=no`), so that the generic public
/// functions it instantiates stay functions of their own, and with `-C save-temps`, which keeps
/// the objects of those codegen units. The link ignores undefined symbols: without LTO a panic
/// path can stay referenced, and the panic handler of the link check calls a symbol that
/// nothing defines (VER-010); the check reports it when an entry point reaches it. The
/// exerciser's own outputs are removed first, so that only this compilation's are read.
fn build_exerciser(nightly: &str, target: &str) -> Result<Built, String> {
    let manifest = "exerciser/Cargo.toml";
    cargo(
        nightly,
        &[
            "clean",
            "-p",
            "aldis-exerciser",
            "--release",
            "--target",
            target,
            "--manifest-path",
            manifest,
        ],
        &[],
        &format!("cleaning the exerciser for {target}"),
    )?;
    let env = [("CARGO_PROFILE_RELEASE_LTO", "off")];
    let stream = cargo(
        nightly,
        &[
            "rustc",
            "-Z",
            "build-std=core",
            "--locked",
            "--release",
            "--target",
            target,
            "--manifest-path",
            manifest,
            "--bin",
            "exerciser",
            "--message-format=json-render-diagnostics",
            "--",
            "-C",
            "codegen-units=256",
            "-Z",
            "inline-mir=no",
            "-C",
            "save-temps",
            "-C",
            "link-arg=--unresolved-symbols=ignore-all",
        ],
        &env,
        &format!("the exerciser build for {target}"),
    )?;
    let files = artefacts(&stream);
    // The target's libraries, not the host crates of the build script.
    let target_dir = repo::root().join(TARGET_DIR).join(target);
    let mut objects: Vec<PathBuf> = files
        .iter()
        .filter(|p| p.starts_with(&target_dir))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rlib"))
        .cloned()
        .collect();
    let mut ir = ir_files(&files);
    let units = target_dir.join("release/build/aldis-exerciser");
    for dir in std::fs::read_dir(&units)
        .map_err(|e| format!("{}: {e}", repo::rel(&units)))?
        .flatten()
    {
        let Ok(entries) = std::fs::read_dir(dir.path().join("out")) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with("exerciser.") && name.ends_with(".rcgu.o") {
                objects.push(path);
            } else if name.starts_with("exerciser.") && name.ends_with(".rcgu.ll") {
                ir.push(path);
            }
        }
    }
    objects.sort();
    ir.sort();
    if !objects
        .iter()
        .any(|p| p.extension().and_then(|e| e.to_str()) == Some("o"))
    {
        return Err("the exerciser build left no objects (-C save-temps)".to_owned());
    }
    Ok(Built {
        name: "exerciser",
        objects,
        ir,
    })
}

fn readobj_json(tool: &Path, files: &[PathBuf], args: &[&str]) -> Result<Value, String> {
    let out = run_tool(
        Command::new(tool)
            .arg("--elf-output-style=JSON")
            .args(args)
            .args(files),
        "llvm-readobj",
    )?;
    serde_json::from_str(&out).map_err(|e| format!("llvm-readobj JSON: {e}"))
}

/// The call graph of one build, with its IR.
fn nodes_of(built: &Built) -> Result<Vec<Node>, String> {
    let readobj = binaries::llvm_tool("llvm-readobj")?;
    let objects = readobj_json(
        &readobj,
        &built.objects,
        &["--section-headers", "--symbols", "-r", "--stack-sizes"],
    )?;
    let demangled = readobj_json(&readobj, &built.objects, &["--symbols", "--demangle"])?;
    let mut nodes = graph(&objects, &demangled)?;
    let mut ir = Ir::default();
    for f in &built.ir {
        parse_ir(&repo::read(f)?, &mut ir);
    }
    attach_ir(&mut nodes, &ir);
    Ok(nodes)
}

/// Measures `target`: builds, reads, analyses, checks the budgets, and writes the report.
/// Returns the number of problems.
fn measure(
    nightly: &str,
    target: &str,
    config: &Config,
    index: &api::Index,
) -> Result<usize, String> {
    let start = std::time::Instant::now();
    let mut entries = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut used = BTreeSet::new();
    let mut frames: BTreeMap<String, u64> = BTreeMap::new();
    let mut seen: BTreeSet<api::Key> = BTreeSet::new();
    let mut unindexed = 0;
    let mut panics = Vec::new();
    for built in [
        build_ffi(nightly, target)?,
        build_exerciser(nightly, target)?,
    ] {
        let nodes = nodes_of(&built)?;
        let (points, fallback) = entry_points(&nodes, index, built.name == "ffi");
        unindexed = unindexed.max(fallback);
        let (analysis, found) = analyse(&nodes, &points, target, &config.exemptions);
        for p in found {
            let p = format!("{p} [build {}]", built.name);
            if !problems.contains(&p) {
                problems.push(p);
            }
        }
        used.extend(analysis.used.iter().copied());
        panics.push((
            built.name,
            analysis
                .panicking
                .iter()
                .map(|&i| nodes[i].display.clone())
                .collect(),
        ));
        for &i in &analysis.reached {
            let f = frames.entry(nodes[i].display.clone()).or_insert(0);
            *f = (*f).max(nodes[i].frame.unwrap_or(0));
        }
        for n in &nodes {
            seen.extend(api::keys_of(&n.display));
        }
        entries.extend(measured(built.name, &nodes, &analysis));
    }
    problems.extend(stale(&config.exemptions, target, &used));
    let entries = merge(entries);
    problems.extend(over_budget(target, &entries, &config.budgets));
    let unseen: Vec<String> = api::unseen(index, &seen)
        .into_iter()
        .map(|k| match k {
            api::Key::Free { krate, path } => format!("{krate}::{path}"),
            api::Key::Method { krate, ty, name } => format!("{krate}: <{ty}>::{name}"),
            api::Key::Provided { krate, tr, name } => format!("{krate}: <_ as {tr}>::{name}"),
        })
        .collect();
    let md = report(
        &Summary {
            target,
            nightly,
            entries: &entries,
            frames: &frames,
            unseen: &unseen,
            unindexed,
            panics: &panics,
            problems: &problems,
        },
        config,
    );
    let out = repo::root().join("target/spec");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let file = out.join(format!("stack-{target}.md"));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    for p in &problems {
        println!("error: stack {target}: {p}");
    }
    let largest = |kind: Kind| {
        entries
            .iter()
            .filter(|e| e.kind == kind)
            .max_by_key(|e| e.worst)
            .map_or_else(
                || "none".to_owned(),
                |e| format!("{} bytes (`{}`, {})", e.worst, e.display, e.unit),
            )
    };
    println!(
        "stack {target}: {} entry point(s), {} function(s) reached, {} public function(s) \
         measured in neither build; largest rust {}, c-abi {}; {} problem(s); {:.0} s; report: {}",
        entries.len(),
        frames.len(),
        unseen.len(),
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
    let index = api::index(LIBRARY_CRATES)?;
    let mut count = problems.len();
    for t in targets {
        count += measure(&nightly, t, &config, &index)?;
    }
    Ok(count)
}

/// The library crates whose public functions are entry points.
const LIBRARY_CRATES: &[&str] = &[
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
];

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
  %agg = alloca { i32, i32 }, i32 %n, align 4
  %fixed = alloca { i32, i32 }, i32 3, align 4
  %arr = alloca [4 x { i8, i8 }], align 2
  %space = alloca i8, addrspace(5)
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
                dynamic_alloca: 2,
                noreturn: false
            }
        );
        // LLVM's attribute comment before a definition marks a diverging function.
        let mut diverging = Ir::default();
        parse_ir(
            "; Function Attrs: cold noreturn nounwind\ndefine void @fail() {\n  unreachable\n}\n\
             define void @ok() {\n  ret void\n}\n",
            &mut diverging,
        );
        assert!(diverging.defined["fail"].noreturn);
        assert!(!diverging.defined["ok"].noreturn);
        assert!(dynamic_count("{ i32, [2 x i8] }, i64 %len, align 8"));
        assert!(!dynamic_count("{ i32, i32 }, align 4"));
        assert_eq!(
            operands("{ a, b }, <2 x i8> (c, d), x"),
            ["{ a, b }", "<2 x i8> (c, d)", "x"]
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
        // Not in the source index: the global function of its own crate's object counts.
        let (points, unindexed) = entry_points(&nodes, &api::Index::default(), true);
        assert_eq!(points, [(Kind::Rust, 0)]);
        assert_eq!(unindexed, 1);
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

    fn free(krate: &str, path: &str) -> api::Key {
        api::Key::Free {
            krate: krate.to_owned(),
            path: path.to_owned(),
        }
    }

    // req: VER-011
    #[test]
    fn rust_entry_points_follow_the_source_visibility() {
        let mut nodes = vec![
            // A monomorphisation emitted into the C ABI's object, local.
            node(
                "<aldis_aos::config::VcTable<64>>::new",
                "aldis_ffi",
                Some(1),
                &[],
            ),
            // A `pub(crate)` helper, global because generic code calls it.
            node("aldis_tc::config::check_room", "aldis_tc", Some(1), &[]),
            // A public free function, and its closure.
            node(
                "aldis_aos::writer::encode::<aldis_aos::config::VcTable<64>>",
                "aldis_ffi",
                Some(1),
                &[],
            ),
            node(
                "aldis_aos::writer::encode::{closure#0}",
                "aldis_ffi",
                Some(1),
                &[],
            ),
            // The C ABI.
            node("aldis_tc_decode", "aldis_ffi", Some(1), &[]),
            // The C ABI crate's own Rust items are not entry points.
            node(
                "aldis_ffi::tc::master_channel_at",
                "aldis_ffi",
                Some(1),
                &[],
            ),
            // A trait method of a public type.
            node(
                "<aldis_core::error::Error as core::fmt::Display>::fmt",
                "aldis_core",
                Some(1),
                &[],
            ),
        ];
        // Rust symbols are mangled; only the C ABI's are their own names.
        for (i, n) in nodes.iter_mut().enumerate() {
            if i != 4 {
                n.names = vec![format!("_RN{i}")];
            }
        }
        nodes[0].global = false;
        nodes[2].global = false;
        nodes[3].global = false;
        nodes[5].global = false;
        let mut index = api::Index::default();
        for (key, public) in [
            (
                api::Key::Method {
                    krate: "aldis_aos".to_owned(),
                    ty: "VcTable".to_owned(),
                    name: "new".to_owned(),
                },
                true,
            ),
            (free("aldis_tc", "config::check_room"), false),
            (free("aldis_aos", "writer::encode"), true),
            (
                api::Key::Method {
                    krate: "aldis_core".to_owned(),
                    ty: "Error".to_owned(),
                    name: "fmt".to_owned(),
                },
                true,
            ),
        ] {
            index.items.insert(key, public);
        }
        let (points, unindexed) = entry_points(&nodes, &index, true);
        assert_eq!(
            points,
            [
                (Kind::Rust, 0),
                (Kind::Rust, 2),
                (Kind::CAbi, 4),
                (Kind::Rust, 6)
            ]
        );
        assert_eq!(unindexed, 0);
        // In the exerciser's build the C ABI is not an entry point.
        let (points, _) = entry_points(&nodes, &index, false);
        assert!(points.iter().all(|(k, _)| *k == Kind::Rust), "{points:?}");
    }

    // req: VER-011
    #[test]
    fn the_worst_case_is_the_deepest_chain_of_frames() {
        let nodes = vec![
            node("aldis_x_call", "aldis_ffi", Some(100), &[2, 3]),
            node("aldis_core::f", "aldis_core", Some(40), &[3]),
            node("helper", "aldis_ffi", Some(10), &[3]),
            node("leaf", "core", Some(8), &[]),
        ];
        let points = [(Kind::CAbi, 0), (Kind::Rust, 1)];
        let (a, problems) = analyse(&nodes, &points, "t", &[]);
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
        // The largest worst case of an entry point across builds.
        let mut all = measured("ffi", &nodes, &a);
        all.push(Measured {
            worst: 60,
            unit: "exerciser",
            ..all[1].clone()
        });
        let merged = merge(all);
        assert_eq!(merged[1].worst, 60);
        assert_eq!(merged[1].unit, "exerciser");
        assert_eq!(merged[0].below, "helper (10) → leaf (8)");
        let budgets = BTreeMap::from([(
            "t".to_owned(),
            BTreeMap::from([("rust".to_owned(), 48), ("c-abi".to_owned(), 117)]),
        )]);
        let over = over_budget("t", &merged, &budgets);
        assert_eq!(over.len(), 2, "{over:?}");
        assert!(
            over[0].contains(
                "`aldis_x_call` needs 118 bytes of stack, 1 over the c-abi budget of 117 bytes on t"
            ),
            "{over:?}"
        );
        assert!(
            over[1].contains("`aldis_core::f` needs 60 bytes of stack, 12 over the rust budget"),
            "{over:?}"
        );
        assert_eq!(
            over_budget("u", &merged, &budgets),
            ["xtask/stack-budgets.toml: no [[budget]] for u"]
        );
    }

    fn exemption(function: &str, targets: &[&str], bound: Bound, line: usize) -> Exemption {
        Exemption {
            function: function.to_owned(),
            targets: targets.iter().map(|t| (*t).to_owned()).collect(),
            bound,
            line,
        }
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
        nodes[2].ir = None;
        nodes[3].ir = Some(IrFacts {
            indirect: 1,
            ..IrFacts::default()
        });
        nodes[3].undefined.insert("missing".to_owned());
        let points = [(Kind::Rust, 0)];
        let (_, problems) = analyse(&nodes, &points, "t", &[]);
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
            exemption("asm_fn", &["t"], Bound::Frame(24), 1),
            exemption("dyn_caller", &[], Bound::Indirect(100), 2),
            exemption("nowhere", &[], Bound::Frame(1), 3),
            exemption("asm_fn", &["other"], Bound::Frame(1), 4),
        ];
        let (a, problems) = analyse(&nodes, &points, "t", &exemptions);
        assert!(problems.is_empty(), "{problems:?}");
        // 16 (a) + 4 (dyn_caller) + 100 (its indirect calls), more than 16 + 24 (asm_fn).
        assert_eq!(a.entries[0].worst, 120);
        assert_eq!(a.entries[0].path, [0, 3]);
        assert_eq!(a.used, BTreeSet::from([0, 1]));
        assert_eq!(
            stale(&exemptions, "t", &a.used),
            [
                "xtask/stack-budgets.toml:3: the exemption of `nowhere` applies to nothing \
              reachable on t; remove it, or name the targets it applies to"
            ]
        );
        // An `indirect` exemption without an indirect call of its own is stale.
        nodes[3].ir = Some(IrFacts::default());
        let (a, _) = analyse(&nodes, &points, "t", &exemptions[..2]);
        let stale = stale(&exemptions[..2], "t", &a.used);
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert!(
            stale[0].contains(":2: the exemption of `dyn_caller`"),
            "{stale:?}"
        );
    }

    // req: GEN-022, VER-011
    #[test]
    fn an_indirect_exemption_covers_only_its_own_indirect_calls() {
        let mut nodes = vec![
            node("aldis_core::a", "aldis_core", Some(16), &[1]),
            node("callee", "core", Some(8), &[]),
        ];
        nodes[0].ir = Some(IrFacts {
            indirect: 1,
            asm: 1,
            ..IrFacts::default()
        });
        nodes[1].ir = Some(IrFacts {
            indirect: 2,
            ..IrFacts::default()
        });
        let exemptions = [exemption("aldis_core::a", &[], Bound::Indirect(0), 1)];
        let (a, problems) = analyse(&nodes, &[(Kind::Rust, 0)], "t", &exemptions);
        let text = problems.join("\n");
        // Its own indirect call is accepted ...
        assert!(!text.contains("`aldis_core::a` makes"), "{text}");
        assert_eq!(a.used, BTreeSet::from([0]));
        // ... but not its inline assembly, nor its callee's indirect calls.
        assert!(
            text.contains("`aldis_core::a` runs inline assembly (1)"),
            "{text}"
        );
        assert!(text.contains("`callee` makes 2 indirect call(s)"), "{text}");
        assert_eq!(problems.len(), 2, "{text}");
    }

    // req: VER-011
    #[test]
    fn reads_only_the_ir_of_the_compilations_reported() {
        let dir = repo::root().join("target/stack-ir-test");
        let out = dir.join("out");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&out).unwrap();
        for f in [
            "libaldis_tc-new.rlib",
            "aldis_tc-new.ll",
            "aldis_tc-old.ll",
            "aldis_tc-new.aldis_tc.x-cgu.0.rcgu.ll",
            "libaldis_ffi.rlib",
            "aldis_ffi.ll",
            "aldis_ffi_extra.ll",
            "aldis_tc-newer.ll",
        ] {
            std::fs::write(out.join(f), "").unwrap();
        }
        let ir = ir_files(&[
            out.join("libaldis_tc-new.rlib"),
            out.join("libaldis_ffi.rlib"),
        ]);
        let names: Vec<String> = ir
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .collect();
        assert_eq!(
            names,
            [
                "aldis_ffi.ll",
                "aldis_tc-new.aldis_tc.x-cgu.0.rcgu.ll",
                "aldis_tc-new.ll"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
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
