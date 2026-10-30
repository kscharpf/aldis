//! `xtask consistency`: the per-crate and per-file registrations agree (no discovery, only a
//! check; part of `cargo xtask ci`). The definition of done it supports is
//! `docs/milestones/checklists.md`.
//!
//! **Crates.** A library crate of Layers 0–2 *has code* when a non-comment line of its `src/`
//! declares a function (`fn `); the skeletons declare modules only. Every crate with code is in
//! the coverage gate and the `features` check ([`crate::layers::gated`], VER-024, GEN-006),
//! which derive their crates from that set. The same set must appear in the `-p` list of the
//! nightly `mutants` job (`.github/workflows/nightly.yml`, VER-025), and the exerciser (`exerciser/Cargo.toml` dependencies, VER-010), and each crate other than
//! `aldis-core`, whose vectors the exerciser names one by one, has a vector group in
//! `exerciser/build.rs`. Every crate of the layers check ([`crate::layers::LAYERS`], GEN-002) is
//! a workspace member, and every gated crate is in it. A boundary crate other than the facade
//! that declares a function (`aldis-ffi`) is in [`crate::coverage::REPORTED`], and every
//! reported crate is exercised too, with its `ffi` group (`mod ffi;` in `exerciser/src/main.rs`,
//! run by `exerciser/src/exercise.rs`; M2 plan D16). Every workspace crate under `crates/`
//! has a `README.md` linked from the root `README.md`, and every layered crate has a facade
//! feature in `crates/aldis` that enables it, forwards `alloc` and `std`, and is part of `full`.
//!
//! **Fuzz targets.** The `[[bin]]` targets of `fuzz/Cargo.toml` and the matrix of the nightly
//! `fuzz` job are the same set (VER-022).
//!
//! **C names.** Every `aldis_` or `ALDIS_` name that `docs/profiles/c-abi.md` and
//! `crates/aldis-ffi/README.md` mention is declared by `include/aldis/aldis.h` (CABI-002), or is
//! listed with its reason in [`NOT_IN_HEADER`] (a removed name that a history row recalls, an
//! illustration, a name of a module not yet built). A name followed by `*` or ending in `_` is a
//! prefix: some name of the header starts with it. A placeholder (`ALDIS_<STD>_REASON_*`,
//! `aldis_<module>_<name>_t`, a bare `ALDIS_*`) is not checked, and `consistency` counts them.
//! The header is read without its comments, so a name that only a comment mentions is not
//! declared. A listed name that the header now declares, or that no document mentions, is
//! reported too, so the list shrinks when the code catches up (CABI-005).
//!
//! **Vector files.** Every vector file (`test-vectors/<area>/*.json`, except the schemas and the
//! generated field catalogue, [`crate::vectors::is_vector_file`]) is matched by a path in the
//! first column of a table of `test-vectors/README.md` (braces and `*` expand), and every such
//! path matches a file. A host test runs the file: some `.rs` file under `crates/*/tests/`
//! (which includes the C ABI runner in `crates/aldis-ffi/tests/`) contains the string literal
//! `"<area>/<name>.json"`, or both `concat!("<area>/"` and the literal `"<name>"`, the form of
//! the `vector_test!` macros. This is a textual check: it shows that a test names the file, and
//! the test itself fails when the file does not pass. When the area's crate is exercised, the
//! exerciser embeds the file: `exerciser/build.rs` names `"<name>.json"` for `core`, and lists
//! the area as a vector group otherwise (it then embeds every file of the directory). An area
//! of a codec with C ABI functions (`crates/aldis-ffi/src/<area>.rs` exists), or that the C ABI
//! runner `crates/aldis-ffi/tests/vectors.rs` already covers (it names one of its files in a
//! `vector_test!` invocation), is covered whole by that runner, and the runner names no missing file
//! (M2 plan D4). `aldis-core` has a C ABI for part of its modules only (M2 plan D14): the runner
//! runs the `core/` files of those modules (`FFI_FILES`), and `core/` is not required whole.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::layers::{gated, has_code};
use crate::md::tables;
use crate::repo;

/// The registrations of the library crates, as read from the repository.
#[derive(Debug, Default, Clone)]
struct Crates {
    /// Layered library crates (layers check) with their layer.
    layers: Vec<(String, u8)>,
    /// Workspace members under `crates/`.
    members: BTreeSet<String>,
    /// Layered crates whose `src/` declares a function.
    with_code: BTreeSet<String>,
    coverage: BTreeSet<String>,
    /// Crates measured but not gated (`REPORTED`, xtask/src/coverage.rs).
    reported: BTreeSet<String>,
    /// Boundary crates (`BOUNDARY`, xtask/src/layers.rs).
    boundary: BTreeSet<String>,
    mutants: BTreeSet<String>,
    exerciser: BTreeSet<String>,
    /// Vector groups of `exerciser/build.rs`.
    groups: BTreeSet<String>,
    /// Vector directories of the codecs with C ABI functions: `crates/aldis-ffi/src/<area>.rs`
    /// exists. Every file in them runs through the C ABI runner (M2 plan D4, WP-0c).
    ffi_areas: BTreeSet<String>,
    /// Whether the exerciser has its `ffi` group: `mod ffi;` in `exerciser/src/main.rs`, run
    /// by `exerciser/src/exercise.rs` (M2 plan D16).
    ffi_group: bool,
    /// Crates under `crates/` with a `README.md`.
    readmes: BTreeSet<String>,
    /// The root `README.md`.
    root_readme: String,
    /// Features of the facade `aldis`: name → entries.
    facade: Vec<(String, Vec<String>)>,
    /// Dependencies of the facade.
    facade_deps: BTreeSet<String>,
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The `crate` matrix of the workflow job `job`.
fn matrix_crates(workflow: &str, job: &str) -> Option<BTreeSet<String>> {
    let mut lines = workflow
        .lines()
        .skip_while(|l| l.trim_end() != format!("  {job}:"))
        .skip(1)
        .take_while(|l| l.is_empty() || l.starts_with("   "));
    let mut rest = lines
        .by_ref()
        .skip_while(|l| !l.trim().starts_with("crate:"));
    let first = rest.next()?;
    if let Some(list) = first.trim().strip_prefix("crate:") {
        let trimmed = list.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return Some(
                trimmed[1..trimmed.len() - 1]
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
            );
        }
    }
    let block: BTreeSet<String> = rest
        .map(str::trim)
        .take_while(|l| l.starts_with("- "))
        .filter_map(|l| l.strip_prefix("- ").map(|n| n.trim().to_owned()))
        .collect();
    if block.is_empty() { None } else { Some(block) }
}

/// The crates of the nightly `mutants` job: from its `matrix.crate` list, or `-p` on the `cargo mutants` line.
fn mutants_crates(workflow: &str) -> Result<BTreeSet<String>, String> {
    if let Some(crates) = matrix_crates(workflow, "mutants") {
        return Ok(crates);
    }
    let line = workflow
        .lines()
        .find(|l| l.contains("cargo mutants"))
        .ok_or("no `cargo mutants` line")?;
    let words: Vec<&str> = line.split_whitespace().collect();
    Ok(words
        .windows(2)
        .filter(|w| w.first() == Some(&"-p"))
        .filter_map(|w| w.get(1).map(|s| (*s).to_owned()))
        .collect())
}

/// The `target` matrix of the workflow job `job` (a list of `- name` lines).
fn matrix_targets(workflow: &str, job: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = workflow
        .lines()
        .skip_while(|l| l.trim_end() != format!("  {job}:"))
        .skip(1)
        .take_while(|l| l.is_empty() || l.starts_with("   "));
    if !lines.any(|l| l.trim() == "target:") {
        return Err(format!("job `{job}`: no `target:` matrix"));
    }
    Ok(lines
        .map(str::trim)
        .take_while(|l| l.starts_with("- "))
        .filter_map(|l| l.strip_prefix("- ").map(|n| n.trim().to_owned()))
        .collect())
}

/// The names of the `[[bin]]` targets of a manifest.
pub(crate) fn bin_targets(manifest: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut in_bin = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_bin = line == "[[bin]]";
        } else if in_bin
            && let Some(v) = line
                .strip_prefix("name")
                .and_then(|v| v.trim_start().strip_prefix('='))
        {
            out.insert(v.trim().trim_matches('"').to_owned());
        }
    }
    out
}

/// The `aldis-*` names in the `[dependencies]` table of a manifest.
fn aldis_dependencies(manifest: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
        } else if in_deps
            && line.starts_with("aldis-")
            && let Some((name, _)) = line.split_once('=')
        {
            out.insert(name.trim().to_owned());
        }
    }
    out
}

/// The vector groups of `exerciser/build.rs`: the first strings of the tuples in the
/// `for (group, variant) in [...]` list.
fn exerciser_groups(build_rs: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = build_rs
        .lines()
        .skip_while(|l| !l.contains("for (group, variant) in ["))
        .skip(1);
    let mut out = BTreeSet::new();
    for line in lines.by_ref() {
        let line = line.trim();
        if line.starts_with(']') {
            return Ok(out);
        }
        let group = line
            .strip_prefix("(\"")
            .and_then(|r| r.split_once('"'))
            .map(|(g, _)| g.to_owned())
            .ok_or_else(|| format!("exerciser/build.rs: unexpected group line `{line}`"))?;
        out.insert(group);
    }
    Err("exerciser/build.rs: no `for (group, variant) in [...]` list".to_owned())
}

fn diff(problems: &mut Vec<String>, what: &str, want: &BTreeSet<String>, have: &BTreeSet<String>) {
    for c in want.difference(have) {
        problems.push(format!("{c}: missing from {what}"));
    }
    for c in have.difference(want) {
        problems.push(format!("{c}: in {what}, but not a crate with code"));
    }
}

/// Problems with the crate registrations.
fn crate_problems(c: &Crates) -> Vec<String> {
    let mut p = Vec::new();
    let layered: BTreeSet<String> = c.layers.iter().map(|(n, _)| n.clone()).collect();
    for n in layered.difference(&c.members) {
        p.push(format!(
            "{n}: in the layers check (xtask/src/layers.rs) but not a workspace member"
        ));
    }
    // The C ABI crate, once it exports functions, is measured (M2 plan D16) and exercised.
    for n in c.boundary.intersection(&c.with_code) {
        if n != "aldis" && !c.reported.contains(n) {
            p.push(format!(
                "{n}: has code but is neither a gated crate (Layers 0-2) nor in `REPORTED` (xtask/src/coverage.rs)"
            ));
        }
    }
    for n in c.reported.difference(&c.with_code) {
        p.push(format!(
            "{n}: in `REPORTED` (xtask/src/coverage.rs) but declares no function"
        ));
    }
    if c.exerciser.contains("aldis-ffi") && !c.ffi_group {
        p.push(
            "aldis-ffi: the exerciser has no `ffi` group (`mod ffi;` in exerciser/src/main.rs, \
             `crate::ffi::run` in exerciser/src/exercise.rs)"
                .to_owned(),
        );
    }
    let gated = &c.coverage;
    let exercised: BTreeSet<String> = gated.union(&c.reported).cloned().collect();
    diff(
        &mut p,
        "the nightly mutants job (`-p`, .github/workflows/nightly.yml)",
        gated,
        &c.mutants,
    );
    diff(
        &mut p,
        "the exerciser (dependencies, exerciser/Cargo.toml)",
        &exercised,
        &c.exerciser,
    );
    let groups: BTreeSet<String> = gated
        .iter()
        .filter(|n| n.as_str() != "aldis-core")
        .filter_map(|n| n.strip_prefix("aldis-").map(str::to_owned))
        .collect();
    for g in groups.difference(&c.groups) {
        p.push(format!(
            "aldis-{g}: no vector group `{g}` in exerciser/build.rs"
        ));
    }
    for g in c.groups.difference(&groups) {
        p.push(format!(
            "exerciser/build.rs: vector group `{g}` of a crate that is not gated"
        ));
    }
    p.extend(packaging_problems(c, &layered));
    p
}

/// Problems with the READMEs and the facade features of the crates.
fn packaging_problems(c: &Crates, layered: &BTreeSet<String>) -> Vec<String> {
    let mut p = Vec::new();
    for n in &c.members {
        if !c.readmes.contains(n) {
            p.push(format!("{n}: no crates/{n}/README.md"));
        }
        if !c.root_readme.contains(&format!("crates/{n}/README.md")) {
            p.push(format!("{n}: README.md does not link crates/{n}/README.md"));
        }
    }
    let feature = |name: &str| c.facade.iter().find(|(f, _)| f == name).map(|(_, e)| e);
    for n in layered {
        let short = n.strip_prefix("aldis-").unwrap_or(n);
        if !c.facade_deps.contains(n) {
            p.push(format!("{n}: not a dependency of the facade crates/aldis"));
        }
        if !feature(short).is_some_and(|e| e.contains(&format!("dep:{n}"))) {
            p.push(format!(
                "{n}: the facade has no feature `{short}` that enables `dep:{n}`"
            ));
        }
        if !feature("full").is_some_and(|e| e.iter().any(|x| x == short)) {
            p.push(format!(
                "{n}: the facade feature `full` does not enable `{short}`"
            ));
        }
        for f in ["alloc", "std"] {
            if !feature(f).is_some_and(|e| e.contains(&format!("{n}?/{f}"))) {
                p.push(format!(
                    "{n}: the facade feature `{f}` does not forward `{n}?/{f}`"
                ));
            }
        }
    }
    p
}

/// Problems with the fuzz registrations.
fn fuzz_problems(manifest: &BTreeSet<String>, nightly: &BTreeSet<String>) -> Vec<String> {
    let mut p = Vec::new();
    for t in manifest.difference(nightly) {
        p.push(format!(
            "fuzz target {t}: in fuzz/Cargo.toml but not in the nightly fuzz matrix"
        ));
    }
    for t in nightly.difference(manifest) {
        p.push(format!(
            "fuzz target {t}: in the nightly fuzz matrix but not in fuzz/Cargo.toml"
        ));
    }
    p
}

/// Expands the braces of a path pattern: `a/{b,c}.json` → `a/b.json`, `a/c.json`.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some((head, rest)) = pattern.split_once('{') else {
        return vec![pattern.to_owned()];
    };
    let Some((alts, tail)) = rest.split_once('}') else {
        return vec![pattern.to_owned()];
    };
    alts.split(',')
        .flat_map(|alt| expand_braces(&format!("{head}{alt}{tail}")))
        .collect()
}

/// Whether `path` matches `pattern`, where `*` matches any run of characters except `/`.
fn glob(pattern: &str, path: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == path,
        Some((head, tail)) => path.strip_prefix(head).is_some_and(|rest| {
            (0..=rest.len())
                .take_while(|&i| !rest.get(..i).is_some_and(|s| s.contains('/')))
                .any(|i| rest.get(i..).is_some_and(|r| glob(tail, r)))
        }),
    }
}

/// The path patterns of the first column of every `File` table of `test-vectors/README.md`.
fn readme_patterns(readme: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in tables(readme) {
        if t.header.first().map(String::as_str) != Some("File") {
            continue;
        }
        for (_, row) in t.rows {
            let cell = row.first().map_or("", String::as_str);
            for (i, part) in cell.split('`').enumerate() {
                if i % 2 == 1 {
                    out.extend(expand_braces(part));
                }
            }
        }
    }
    out
}

/// The vector files the C ABI runner names: the first two string literals of each
/// `vector_test!(name, "<area>", "<name>", run)` invocation, as `<area>/<name>.json`.
fn ffi_runner_files(runner: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for chunk in runner.split("vector_test!(").skip(1) {
        let call = chunk.split(");").next().unwrap_or_default();
        let literals: Vec<&str> = call.split('"').skip(1).step_by(2).collect();
        if let [area, name, ..] = literals.as_slice() {
            out.insert(format!("{area}/{name}.json"));
        }
    }
    out
}

/// Whether one of the test sources names the vector file `<area>/<name>.json`.
fn named_by_test(file: &str, tests: &[String]) -> bool {
    let Some((area, name)) = file.strip_suffix(".json").and_then(|f| f.split_once('/')) else {
        return false;
    };
    tests.iter().any(|t| {
        t.contains(&format!("\"{file}\""))
            || (t.contains(&format!("concat!(\"{area}/\"")) && t.contains(&format!("\"{name}\"")))
    })
}

/// Problems with the vector files: `files` relative to `test-vectors/`.
fn vector_problems(
    files: &[String],
    readme: &str,
    tests: &[String],
    build_rs: &str,
    ffi_runner: &str,
    c: &Crates,
) -> Vec<String> {
    let mut p = Vec::new();
    // An area the C ABI runner covers is covered whole (M2 plan D4).
    // An area with a C ABI for some of its files only (`FFI_FILES`) needs those files alone.
    let ffi = ffi_runner_files(ffi_runner);
    let partial: BTreeSet<&str> = FFI_FILES
        .iter()
        .filter_map(|f| f.split('/').next())
        .collect();
    let mut ffi_areas: BTreeSet<&str> = ffi
        .iter()
        .filter_map(|f| f.split('/').next())
        .filter(|area| !partial.contains(area))
        .collect();
    ffi_areas.extend(c.ffi_areas.iter().map(String::as_str));
    for f in &ffi {
        if !files.contains(f) {
            p.push(format!(
                "{FFI_RUNNER}: names test-vectors/{f}, which is not a vector file"
            ));
        }
    }
    let patterns = readme_patterns(readme);
    for f in files {
        if !patterns.iter().any(|pat| glob(pat, f)) {
            p.push(format!(
                "test-vectors/{f}: not listed in test-vectors/README.md"
            ));
        }
        if !named_by_test(f, tests) && !AWAITING_RUNNER.iter().any(|dir| f.starts_with(dir)) {
            p.push(format!(
                "test-vectors/{f}: no test in crates/*/tests/ runs it"
            ));
        }
        let Some((area, name)) = f.split_once('/') else {
            continue;
        };
        if ffi_areas.contains(area) && !ffi.contains(f) {
            p.push(format!(
                "test-vectors/{f}: the C ABI runner {FFI_RUNNER} covers `{area}/` but not this file"
            ));
        }
        if FFI_FILES.contains(&f.as_str()) && !ffi.contains(f) {
            p.push(format!(
                "test-vectors/{f}: has C ABI functions (`FFI_FILES`), but the C ABI runner {FFI_RUNNER} does not run it"
            ));
        }
        if !c.exerciser.contains(&format!("aldis-{area}")) {
            continue;
        }
        let embedded = if area == "core" {
            build_rs.contains(&format!("\"{name}\""))
        } else {
            c.groups.contains(area)
        };
        if !embedded {
            p.push(format!(
                "test-vectors/{f}: aldis-{area} is exercised but exerciser/build.rs does not embed it"
            ));
        }
    }
    for pat in &patterns {
        if !files.iter().any(|f| glob(pat, f)) {
            p.push(format!(
                "test-vectors/README.md: `{pat}` matches no vector file"
            ));
        }
    }
    p
}

/// Vector directories whose files exist before the engines that run them (M3 plan WP-2 before
/// WP-3 and WP-4): the test requirement is waived until the runner lands, and the entry is
/// removed in the pull request that adds it.
const AWAITING_RUNNER: &[&str] = &["cop1/"];

/// The vector runner through the C ABI (M2 plan D4).
const FFI_RUNNER: &str = "crates/aldis-ffi/tests/vectors.rs";

/// The vector files of an area whose crate has a C ABI for some of its modules only: each runs
/// through [`FFI_RUNNER`], and the rest of the area need not (M2 plan D14, WP-1: the CRC-16,
/// the OCF, and the OID generator of `aldis-core`).
const FFI_FILES: &[&str] = &["core/crc16.json", "core/ocf.json", "core/oid.json"];

fn metadata() -> Result<Value, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

/// The vector areas with a C ABI module: a directory of `test-vectors/` other than `core`
/// (whose C ABI covers some modules only, `FFI_FILES`) with a file `<area>.rs` in
/// `crates/aldis-ffi/src/`.
fn ffi_areas() -> BTreeSet<String> {
    let src: BTreeSet<String> = repo::files("crates/aldis-ffi/src", &["rs"])
        .iter()
        .filter_map(|p| p.file_stem().and_then(|n| n.to_str()).map(str::to_owned))
        .collect();
    repo::files("test-vectors", &["json"])
        .iter()
        .filter_map(|p| repo::rel(p).split('/').nth(1).map(str::to_owned))
        .filter(|area| area != "core" && src.contains(area))
        .collect()
}

/// The documents whose `aldis_` and `ALDIS_` names the C header must declare.
const NAMED_IN_DOCS: [&str; 2] = ["docs/profiles/c-abi.md", "crates/aldis-ffi/README.md"];

/// The generated C header (CABI-002).
const C_HEADER: &str = "include/aldis/aldis.h";

/// Names the documents mention that the header does not declare, each with the reason.
const NOT_IN_HEADER: &[(&str, &str)] = &[
    (
        "aldis_limits",
        "removed by OI-57 convention 5; recalled by the history rows",
    ),
    (
        "aldis_limits_t",
        "removed by OI-57 convention 5; recalled by the history rows",
    ),
    (
        "ALDIS_ERROR_DETAIL_MIN_SIZE",
        "removed by OI-57 convention 5; recalled by the history rows",
    ),
    ("aldis_x", "illustrative storage struct of convention 4"),
    (
        "aldis_x_t",
        "illustrative storage struct of convention 4 and spike 2a",
    ),
    ("aldis_kind_t", "scratch enum of spike 1"),
    ("ALDIS_KIND_T_ALPHA", "scratch enum of spike 1"),
    ("aldis_action_t", "scratch type of spike 2b"),
    ("aldis_fop1_t", "FOP-1 C module (M3), not yet built"),
    ("aldis_fop1_input_t", "FOP-1 C module (M3), not yet built"),
    ("aldis_fop1_actions_t", "FOP-1 C module (M3), not yet built"),
    ("aldis_fop1_step", "FOP-1 C module (M3), not yet built"),
    (
        "aldis_fop1_handle_clcw",
        "FOP-1 C module (M3), not yet built",
    ),
    (
        "aldis_instant_t",
        "illustrative type of the engine call of CABI-040",
    ),
    (
        "aldis_security_ops_t",
        "the withdrawn CABI-041 says the library has no such type",
    ),
    (
        "aldis_panic_is_unreachable",
        "undefined on purpose: its link failure is the panic check (VER-010)",
    ),
];

/// How a document writes a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Form {
    /// A whole name: the header declares it.
    Whole,
    /// A prefix (it ends in `_` or is followed by `*`): the header declares a name that starts
    /// with it.
    Prefix,
    /// A placeholder (`ALDIS_<STD>_REASON_*`, `aldis_<module>_<name>_t`, a bare `ALDIS_*`): not
    /// checked.
    Placeholder,
}

/// The `aldis_` and `ALDIS_` names in `text`, each with how it is written. A name starts where no
/// identifier character precedes it.
fn c_names(text: &str) -> BTreeSet<(String, Form)> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut found = BTreeSet::new();
    for start in ["aldis_", "ALDIS_"] {
        for (at, _) in text.match_indices(start) {
            if text
                .get(..at)
                .and_then(|b| b.chars().next_back())
                .is_some_and(ident)
            {
                continue;
            }
            let rest = text.get(at..).unwrap_or_default();
            let end = rest.find(|c| !ident(c)).unwrap_or(rest.len());
            let name = rest.get(..end).unwrap_or_default();
            let after = rest.get(end..).unwrap_or_default();
            let form = if name == start || after.starts_with('<') {
                Form::Placeholder
            } else if name.ends_with('_') || after.starts_with('*') {
                Form::Prefix
            } else {
                Form::Whole
            };
            found.insert((name.to_owned(), form));
        }
    }
    found
}

/// `text` without its `/* */` and `//` comments.
fn without_c_comments(text: &str) -> String {
    let mut rest = text;
    let mut out = String::new();
    // bound: each pass consumes one block comment of `rest`, which is finite.
    while let Some(at) = rest.find("/*") {
        out.push_str(rest.get(..at).unwrap_or_default());
        let tail = rest.get(at + 2..).unwrap_or_default();
        rest = tail
            .find("*/")
            .map_or("", |end| tail.get(end + 2..).unwrap_or_default());
    }
    out.push_str(rest);
    out.lines()
        .map(|l| l.find("//").map_or(l, |at| l.get(..at).unwrap_or(l)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The names the documents mention (`(path, text)` pairs) that the header does not declare and
/// `allowed` does not list, and the entries of `allowed` that are stale.
fn header_name_problems(
    docs: &[(&str, &str)],
    header: &str,
    allowed: &[(&str, &str)],
) -> Vec<String> {
    let declared: BTreeSet<String> = c_names(&without_c_comments(header))
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let mut problems = Vec::new();
    let mut mentioned = BTreeSet::new();
    for (path, text) in docs {
        for (name, form) in c_names(text) {
            mentioned.insert(name.clone());
            let ok = match form {
                Form::Placeholder => true,
                Form::Prefix => declared.iter().any(|d| d.starts_with(&name)),
                Form::Whole => declared.contains(&name) || allowed.iter().any(|(a, _)| *a == name),
            };
            if !ok {
                let what = if form == Form::Prefix {
                    "no name of the header starts with"
                } else {
                    "the header does not declare"
                };
                problems.push(format!(
                    "{path}: {what} `{name}` (correct the name, or list it in NOT_IN_HEADER with its reason)"
                ));
            }
        }
    }
    for (name, reason) in allowed {
        if declared.contains(*name) {
            problems.push(format!(
                "NOT_IN_HEADER: `{name}` ({reason}) is declared by the header now; remove the entry"
            ));
        } else if !mentioned.contains(*name) {
            problems.push(format!(
                "NOT_IN_HEADER: `{name}` ({reason}) is mentioned by none of {}; remove the entry",
                NAMED_IN_DOCS.join(", ")
            ));
        }
    }
    problems
}

fn read(rel: &str) -> Result<String, String> {
    repo::read(&repo::root().join(rel))
}

/// Reads the crate registrations from the repository.
fn crates() -> Result<Crates, String> {
    let meta = metadata()?;
    let root = repo::root();
    let mut c = Crates {
        layers: crate::layers::LAYERS
            .iter()
            .map(|(n, l)| ((*n).to_owned(), *l))
            .collect(),
        coverage: names(&gated()?),
        reported: names(crate::coverage::REPORTED),
        boundary: names(crate::layers::BOUNDARY),
        mutants: mutants_crates(&read(".github/workflows/nightly.yml")?)
            .map_err(|e| format!(".github/workflows/nightly.yml: {e}"))?,
        exerciser: aldis_dependencies(&read("exerciser/Cargo.toml")?),
        groups: exerciser_groups(&read("exerciser/build.rs")?)?,
        ffi_group: read("exerciser/src/main.rs")?.contains("mod ffi;")
            && read("exerciser/src/exercise.rs")?.contains("crate::ffi::run("),
        root_readme: read("README.md")?,
        ffi_areas: ffi_areas(),
        ..Crates::default()
    };
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        let name = pkg["name"].as_str().unwrap_or_default().to_owned();
        let manifest = Path::new(pkg["manifest_path"].as_str().unwrap_or_default());
        let dir = manifest.parent().unwrap_or(&root);
        if !repo::rel(dir).starts_with("crates/") {
            continue;
        }
        if dir.join("README.md").exists() {
            c.readmes.insert(name.clone());
        }
        let src = format!("{}/src", repo::rel(dir));
        let mut code = false;
        for file in repo::files(&src, &["rs"]) {
            code |= has_code(&repo::read(&file)?);
        }
        if code {
            c.with_code.insert(name.clone());
        }
        if name == "aldis" {
            if let Some(features) = pkg["features"].as_object() {
                for (f, entries) in features {
                    let entries = entries
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    c.facade.push((f.clone(), entries));
                }
            }
            c.facade_deps = pkg["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["name"].as_str().map(str::to_owned))
                .collect();
        }
        c.members.insert(name);
    }
    Ok(c)
}

/// Runs every check; returns the number of problems.
pub fn check() -> Result<usize, String> {
    let c = crates()?;
    let mut problems = crate_problems(&c);

    let nightly = read(".github/workflows/nightly.yml")?;
    let matrix = matrix_targets(&nightly, "fuzz")
        .map_err(|e| format!(".github/workflows/nightly.yml: {e}"))?;
    let bins = bin_targets(&read("fuzz/Cargo.toml")?);
    problems.extend(fuzz_problems(&bins, &matrix));

    let files: Vec<String> = repo::files("test-vectors", &["json"])
        .iter()
        .map(|p| repo::rel(p))
        .filter(|rel| crate::vectors::is_vector_file(rel))
        .filter_map(|rel| rel.strip_prefix("test-vectors/").map(str::to_owned))
        .collect();
    let mut tests = Vec::new();
    for path in repo::files("crates", &["rs"]) {
        if repo::rel(&path).split('/').nth(2) == Some("tests") {
            tests.push(repo::read(&path)?);
        }
    }
    problems.extend(vector_problems(
        &files,
        &read("test-vectors/README.md")?,
        &tests,
        &read("exerciser/build.rs")?,
        &read(FFI_RUNNER).unwrap_or_default(),
        &c,
    ));

    let docs = NAMED_IN_DOCS
        .iter()
        .map(|d| read(d).map(|text| (*d, text)))
        .collect::<Result<Vec<_>, _>>()?;
    let docs: Vec<(&str, &str)> = docs.iter().map(|(d, t)| (*d, t.as_str())).collect();
    problems.extend(header_name_problems(&docs, &read(C_HEADER)?, NOT_IN_HEADER));

    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "consistency: {} crate(s) with code, {} fuzz target(s), {} vector file(s), {} document(s) of C names, {} problem(s)",
        c.coverage.len(),
        bins.len(),
        files.len(),
        docs.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::{
        Crates, Form, aldis_dependencies, bin_targets, c_names, crate_problems, exerciser_groups,
        expand_braces, ffi_runner_files, fuzz_problems, glob, has_code, header_name_problems,
        matrix_targets, mutants_crates, named_by_test, names, readme_patterns, vector_problems,
        without_c_comments,
    };

    /// A consistent registration of the M1 crates and `aldis-aos`, with two skeletons.
    fn consistent() -> Crates {
        let code = [
            "aldis-core",
            "aldis-time",
            "aldis-spp",
            "aldis-epp",
            "aldis-aos",
        ];
        let layered = [
            ("aldis-core", 0),
            ("aldis-time", 1),
            ("aldis-spp", 1),
            ("aldis-epp", 1),
            ("aldis-aos", 1),
            ("aldis-tm", 1),
            ("aldis-link", 2),
        ];
        let mut facade = vec![
            ("full".to_owned(), Vec::new()),
            ("alloc".to_owned(), Vec::new()),
            ("std".to_owned(), Vec::new()),
        ];
        for (n, _) in layered {
            let short = n.strip_prefix("aldis-").unwrap();
            facade.push((short.to_owned(), vec![format!("dep:{n}")]));
            facade[0].1.push(short.to_owned());
            facade[1].1.push(format!("{n}?/alloc"));
            facade[2].1.push(format!("{n}?/std"));
        }
        let members: Vec<&str> = layered.iter().map(|(n, _)| *n).collect();
        Crates {
            layers: layered.iter().map(|(n, l)| ((*n).to_owned(), *l)).collect(),
            members: names(&members),
            with_code: names(&code),
            coverage: names(&code),
            mutants: names(&code),
            exerciser: names(&code),
            groups: names(&["time", "spp", "epp", "aos"]),
            readmes: names(&members),
            root_readme: members
                .iter()
                .map(|n| format!("[{n}](crates/{n}/README.md)"))
                .collect::<Vec<_>>()
                .join(" "),
            facade,
            facade_deps: names(&members),
            ffi_areas: names(&["time", "spp", "epp", "tm", "tc", "aos", "uslp"]),
            ..Crates::default()
        }
    }

    #[test]
    fn consistent_crates_pass() {
        // req: VER-024, VER-025, VER-010, GEN-006
        assert_eq!(crate_problems(&consistent()), Vec::<String>::new());
    }

    #[test]
    fn a_crate_missing_from_one_registration_is_reported() {
        // req: VER-024, VER-025, VER-010, GEN-006
        // An M1 crate dropped from each list in turn is caught as well.
        for list in 0..2 {
            let mut c = consistent();
            let set = [&mut c.mutants, &mut c.exerciser];
            for (i, s) in set.into_iter().enumerate() {
                if i == list {
                    s.remove("aldis-spp");
                }
            }
            assert!(!crate_problems(&c).is_empty(), "list {list}");
        }
        // A crate in a list that is not a crate with code.
        let mut c = consistent();
        c.mutants.insert("aldis-tm".to_owned());
        assert_eq!(
            crate_problems(&c),
            [
                "aldis-tm: in the nightly mutants job (`-p`, .github/workflows/nightly.yml), but not a crate with code"
            ]
        );
        // The exerciser group, the READMEs, and the facade.
        let mut c = consistent();
        c.groups.remove("aos");
        c.readmes.remove("aldis-tm");
        c.root_readme = c.root_readme.replace("crates/aldis-link/README.md", "");
        c.facade.retain(|(f, _)| f != "link");
        c.facade_deps.remove("aldis-tm");
        assert_eq!(crate_problems(&c).len(), 5, "{:?}", crate_problems(&c));
        let mut c = consistent();
        c.members.remove("aldis-link");
        assert!(crate_problems(&c)[0].contains("not a workspace member"));
    }

    #[test]
    fn source_scan_tells_code_from_skeletons() {
        // req: VER-024
        assert!(!has_code(
            "//! Skeleton.\n/// fn in a doc comment\npub mod header;\n"
        ));
        assert!(has_code("pub const fn decode() {}\n"));
        assert!(has_code("fn helper() {}\n"));
        assert!(has_code("    pub(crate) fn f(x: u8) -> u8 { x }\n"));
    }

    #[test]
    fn workflow_and_manifest_parsing() {
        // req: VER-022, VER-025, VER-010
        let yml = "jobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - a_decode\n          - b_roundtrip\n    runs-on: x\n  mutants:\n    steps:\n      - run: |\n          cargo mutants -p aldis-core -p aldis-aos --all-features || code=$?\n";
        assert_eq!(
            matrix_targets(yml, "fuzz").unwrap(),
            names(&["a_decode", "b_roundtrip"])
        );
        assert!(matrix_targets(yml, "mutants").is_err());
        assert_eq!(
            mutants_crates(yml).unwrap(),
            names(&["aldis-core", "aldis-aos"])
        );
        let yml_matrix = "jobs:\n  mutants:\n    strategy:\n      matrix:\n        crate: [aldis-core, aldis-aos]\n";
        assert_eq!(
            mutants_crates(yml_matrix).unwrap(),
            names(&["aldis-core", "aldis-aos"])
        );
        assert!(mutants_crates("no such line").is_err());
        let toml = "[package]\nname = \"aldis-fuzz\"\n\n[dependencies]\naldis-core = { path = \"x\" }\nlibfuzzer-sys = \"1\"\n\n[[bin]]\nname = \"a_decode\"\npath = \"a.rs\"\n\n[[bin]]\nname = \"b_roundtrip\"\n";
        assert_eq!(bin_targets(toml), names(&["a_decode", "b_roundtrip"]));
        assert_eq!(aldis_dependencies(toml), names(&["aldis-core"]));
        assert_eq!(
            fuzz_problems(&names(&["a", "b"]), &names(&["b", "c"])),
            [
                "fuzz target a: in fuzz/Cargo.toml but not in the nightly fuzz matrix",
                "fuzz target c: in the nightly fuzz matrix but not in fuzz/Cargo.toml"
            ]
        );
        let build = "    for (group, variant) in [\n        (\"time\", \"Time\"),\n        (\"aos\", \"Aos\"),\n    ] {\n";
        assert_eq!(exerciser_groups(build).unwrap(), names(&["time", "aos"]));
        assert!(exerciser_groups("fn main() {}").is_err());
        assert!(exerciser_groups("for (group, variant) in [\n    bad,\n]").is_err());
    }

    #[test]
    fn readme_patterns_and_globs() {
        // req: TV-020
        assert_eq!(
            expand_braces("time/{cuc,cds}.json"),
            ["time/cuc.json", "time/cds.json"]
        );
        assert_eq!(
            expand_braces("aos/fhec_{a,b}_{x,y}.json"),
            [
                "aos/fhec_a_x.json",
                "aos/fhec_a_y.json",
                "aos/fhec_b_x.json",
                "aos/fhec_b_y.json"
            ]
        );
        assert!(glob("epp/*.json", "epp/idle.json"));
        assert!(!glob("epp/*.json", "epp/sub/idle.json"));
        assert!(!glob("epp/*.json", "spp/idle.json"));
        assert!(glob("core/crc16.json", "core/crc16.json"));
        let readme = "| File | Content |\n|---|---|\n| `core/crc16.json` | x |\n| `time/{cuc,cds}.json`, `epp/*.json` | y |\n\n| Other | x |\n|---|---|\n| `nope.json` | z |\n";
        assert_eq!(
            readme_patterns(readme),
            [
                "core/crc16.json",
                "time/cuc.json",
                "time/cds.json",
                "epp/*.json"
            ]
        );
    }

    /// A C ABI runner that runs `time/cuc.json` and `core/crc16.json`.
    const FFI_CUC: &str =
        "vector_test!(t, \"time\", \"cuc\", r);\nvector_test!(c, \"core\", \"crc16\", r);";

    #[test]
    fn vector_files_need_a_readme_row_a_test_and_the_exerciser() {
        // req: TV-020, VER-021, VER-010
        let tests = [
            "run_codec_file(\"core/crc16.json\", handle)".to_owned(),
            "run_codec_file(concat!(\"time/\", $file, \".json\"), handle)\nvector_test!(cuc, \"cuc\");".to_owned(),
        ];
        assert!(named_by_test("core/crc16.json", &tests));
        assert!(named_by_test("time/cuc.json", &tests));
        assert!(!named_by_test("time/cds.json", &tests));
        assert!(!named_by_test("spp/cuc.json", &tests));
        let c = consistent();
        let build = "load(&dir, \"crc16.json\")";
        let readme = "| File |\n|---|\n| `core/crc16.json`, `time/*.json`, `aos/gone.json` |\n";
        let files = [
            "core/crc16.json".to_owned(),
            "core/layout.json".to_owned(),
            "time/cuc.json".to_owned(),
            "uslp/frame.json".to_owned(),
        ];
        assert_eq!(
            vector_problems(&files, readme, &tests, build, FFI_CUC, &c),
            [
                "test-vectors/core/layout.json: not listed in test-vectors/README.md",
                "test-vectors/core/layout.json: no test in crates/*/tests/ runs it",
                "test-vectors/core/layout.json: aldis-core is exercised but exerciser/build.rs does not embed it",
                "test-vectors/uslp/frame.json: not listed in test-vectors/README.md",
                "test-vectors/uslp/frame.json: no test in crates/*/tests/ runs it",
                // ---- USLP (M2 WP-5 module 9): `uslp` is an area of the C ABI.
                "test-vectors/uslp/frame.json: the C ABI runner crates/aldis-ffi/tests/vectors.rs covers `uslp/` but not this file",
                "test-vectors/README.md: `aos/gone.json` matches no vector file",
            ]
        );
        let mut c = consistent();
        c.groups.remove("time");
        assert_eq!(
            vector_problems(
                &[files[0].clone(), files[2].clone()],
                readme.replace(", `aos/gone.json`", "").as_str(),
                &tests,
                build,
                FFI_CUC,
                &c
            ),
            [
                "test-vectors/time/cuc.json: aldis-time is exercised but exerciser/build.rs does not embed it"
            ]
        );
    }

    #[test]
    fn the_c_abi_runner_covers_whole_areas() {
        // req: CABI-011, VER-021, VER-010
        let runner = "vector_test!(time_cuc, \"time\", \"cuc\", time_run);\nvector_test!(\n    spp_x,\n    \"spp\",\n    \"split\",\n    spp_run\n);\n";
        assert_eq!(
            ffi_runner_files(runner),
            names(&["time/cuc.json", "spp/split.json"])
        );
        let tests = [
            "run_codec_file(concat!(\"time/\", $file, \".json\"), h)\n(\"cuc\") (\"cds\")"
                .to_owned(),
        ];
        let readme = "| File |\n|---|\n| `time/*.json` |\n";
        let files = ["time/cuc.json".to_owned(), "time/cds.json".to_owned()];
        let mut c = consistent();
        c.exerciser.clear();
        assert_eq!(
            vector_problems(&files, readme, &tests, "", runner, &c),
            [
                "crates/aldis-ffi/tests/vectors.rs: names test-vectors/spp/split.json, which is not a vector file",
                "test-vectors/time/cds.json: the C ABI runner crates/aldis-ffi/tests/vectors.rs covers `time/` but not this file",
            ]
        );
        // The M1 codec directories are required even when the runner names none of their files.
        assert_eq!(
            vector_problems(&files, readme, &tests, "", "", &c),
            [
                "test-vectors/time/cuc.json: the C ABI runner crates/aldis-ffi/tests/vectors.rs covers `time/` but not this file",
                "test-vectors/time/cds.json: the C ABI runner crates/aldis-ffi/tests/vectors.rs covers `time/` but not this file",
            ]
        );
        // Another directory joins only when the runner names one of its files (a fictitious one,
        // since every frame codec's directory has a C ABI module).
        let readme = "| File |\n|---|\n| `demo/*.json` |\n";
        let demo = ["demo/fhec.json".to_owned(), "demo/other.json".to_owned()];
        let tests = ["(\"demo/fhec.json\") (\"demo/other.json\")".to_owned()];
        assert_eq!(
            vector_problems(&demo, readme, &tests, "", "", &c),
            Vec::<String>::new()
        );
        assert_eq!(
            vector_problems(
                &demo,
                readme,
                &tests,
                "",
                "vector_test!(a, \"demo\", \"fhec\", r);",
                &c
            ),
            [
                "test-vectors/demo/other.json: the C ABI runner crates/aldis-ffi/tests/vectors.rs covers `demo/` but not this file"
            ]
        );
    }

    #[test]
    fn the_c_abi_runner_covers_the_core_files_with_a_c_abi() {
        // req: CABI-011, VER-021
        let readme = "| File |\n|---|\n| `core/*.json` |\n";
        let core = [
            "core/bits.json".to_owned(),
            "core/crc16.json".to_owned(),
            "core/ocf.json".to_owned(),
            "core/oid.json".to_owned(),
        ];
        let tests = [
            "(\"core/bits.json\") (\"core/crc16.json\") (\"core/ocf.json\") (\"core/oid.json\")"
                .to_owned(),
        ];
        let mut c = consistent();
        c.exerciser.clear();
        // The runner names some `core/` files: those of FFI_FILES are needed, the rest are not.
        let runner =
            "vector_test!(a, \"core\", \"crc16\", r);\nvector_test!(b, \"core\", \"ocf\", r);";
        assert_eq!(
            vector_problems(&core, readme, &tests, "", runner, &c),
            [
                "test-vectors/core/oid.json: has C ABI functions (`FFI_FILES`), but the C ABI runner crates/aldis-ffi/tests/vectors.rs does not run it"
            ]
        );
        let runner = format!("{runner}\nvector_test!(c, \"core\", \"oid\", r);");
        assert_eq!(
            vector_problems(&core, readme, &tests, "", &runner, &c),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_c_abi_crate_is_measured_and_exercised() {
        // req: VER-010, VER-024
        let mut c = consistent();
        c.boundary = names(&["aldis", "aldis-ffi"]);
        c.with_code.insert("aldis-ffi".to_owned());
        c.with_code.insert("aldis".to_owned());
        assert_eq!(
            crate_problems(&c),
            [
                "aldis-ffi: has code but is neither a gated crate (Layers 0-2) nor in `REPORTED` (xtask/src/coverage.rs)"
            ]
        );
        c.reported.insert("aldis-ffi".to_owned());
        assert_eq!(
            crate_problems(&c),
            ["aldis-ffi: missing from the exerciser (dependencies, exerciser/Cargo.toml)"]
        );
        c.exerciser.insert("aldis-ffi".to_owned());
        assert_eq!(
            crate_problems(&c),
            [
                "aldis-ffi: the exerciser has no `ffi` group (`mod ffi;` in exerciser/src/main.rs, `crate::ffi::run` in exerciser/src/exercise.rs)"
            ]
        );
        c.ffi_group = true;
        assert_eq!(crate_problems(&c), Vec::<String>::new());
        c.with_code.remove("aldis-ffi");
        assert_eq!(
            crate_problems(&c),
            ["aldis-ffi: in `REPORTED` (xtask/src/coverage.rs) but declares no function"]
        );
    }

    const HEADER: &str = "\
#define ALDIS_TC_DISCARD_NONE 0
typedef struct aldis_tc_config_t { int x; } aldis_tc_config_t;
enum aldis_status { ALDIS_OK = 0, ALDIS_ERR_INCOMPLETE = 2 };
uint32_t aldis_tc_encode(void);
";

    #[test]
    fn names_are_found_whole_and_prefixes_are_marked() {
        // req: CABI-005
        let found = c_names(
            "`aldis_tc_encode` and ALDIS_ERR_*, `ALDIS_TC_DISCARD_` x_aldis_no ALDIS_OK, \
             ALDIS_<STD>_REASON_*, aldis_<module>_<name>_t, and ALDIS_*.",
        );
        let want = vec![
            ("ALDIS_", Form::Placeholder),
            ("ALDIS_ERR_", Form::Prefix),
            ("ALDIS_OK", Form::Whole),
            ("ALDIS_TC_DISCARD_", Form::Prefix),
            ("aldis_", Form::Placeholder),
            ("aldis_tc_encode", Form::Whole),
        ];
        let got: Vec<(&str, Form)> = found.iter().map(|(n, f)| (n.as_str(), *f)).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn comments_of_the_header_declare_nothing() {
        // req: CABI-005
        let header = "/* ALDIS_IN_BLOCK and\n   aldis_in_block_too */\nint aldis_f(void); // ALDIS_IN_LINE\n#define ALDIS_X 1 /* ALDIS_AFTER */\n";
        assert_eq!(
            without_c_comments(header),
            "\nint aldis_f(void); \n#define ALDIS_X 1 "
        );
        let docs = [(
            "a.md",
            "`aldis_f` `ALDIS_X` `ALDIS_IN_BLOCK` `ALDIS_IN_LINE`",
        )];
        let problems = header_name_problems(&docs, header, &[]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains("`ALDIS_IN_BLOCK`") && problems[1].contains("`ALDIS_IN_LINE`")
        );
    }

    #[test]
    fn a_placeholder_is_not_checked() {
        // req: CABI-005
        let docs = [(
            "a.md",
            "Use ALDIS_<STD>_REASON_* and aldis_<module>_<name>_t.",
        )];
        assert_eq!(header_name_problems(&docs, "", &[]), Vec::<String>::new());
    }

    #[test]
    fn names_the_header_declares_pass() {
        // req: CABI-005
        let docs = [(
            "a.md",
            "Call `aldis_tc_encode`; it returns ALDIS_OK or ALDIS_ERR_*, see ALDIS_TC_DISCARD_*.",
        )];
        assert_eq!(
            header_name_problems(&docs, HEADER, &[]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_name_the_header_lacks_is_reported() {
        // req: CABI-005
        let docs = [
            (
                "a.md",
                "Call `aldis_tc_encode_all`, or set ALDIS_ERR_INVALID_FIELD.",
            ),
            ("b.md", "Use ALDIS_USLP_DISCARD_* values."),
        ];
        let problems = header_name_problems(&docs, HEADER, &[]);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems[0].starts_with("a.md:") && problems[0].contains("`ALDIS_ERR_INVALID_FIELD`")
        );
        assert!(problems[1].contains("`aldis_tc_encode_all`"));
        assert!(
            problems[2].starts_with("b.md:")
                && problems[2].contains("starts with `ALDIS_USLP_DISCARD_`")
        );
    }

    #[test]
    fn a_listed_name_passes_until_it_is_stale() {
        // req: CABI-005
        let allowed = [("aldis_limits", "removed")];
        let docs = [("a.md", "`aldis_limits` is gone.")];
        assert_eq!(
            header_name_problems(&docs, HEADER, &allowed),
            Vec::<String>::new()
        );
        // Declared by the header now: the entry is stale.
        let header = format!("{HEADER}void aldis_limits(void);\n");
        let problems = header_name_problems(&docs, &header, &allowed);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("declared by the header now"));
        // Mentioned by no document: stale as well.
        let problems = header_name_problems(&[("a.md", "nothing")], HEADER, &allowed);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("mentioned by none"));
    }
}
