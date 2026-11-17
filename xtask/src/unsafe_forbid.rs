//! `xtask unsafe-forbid`: where `unsafe` code may exist in the root workspace (GEN-070, GEN-071,
//! VER-002).
//!
//! The workspace lint table sets `unsafe_code = "deny"` and `aldis-ffi` inherits it; every other
//! member keeps the guarantee of a crate-level `forbid`, which no attribute inside the crate can
//! lower. This check fails when
//!
//! 1. a member other than `aldis-ffi` lacks `#![forbid(unsafe_code)]` at the root of a library or
//!    binary target (its integration tests, examples, and benches get only the workspace `deny`),
//!    or
//! 2. any `allow`, `expect`, or `warn` of `unsafe_code`, also inside `cfg_attr`, appears in the
//!    Rust source of a member other than `aldis-ffi`, which covers the targets of (1) and the
//!    files that no `forbid` reaches, or
//! 3. the manifest of such a member has neither `[lints] workspace = true` nor an `unsafe_code`
//!    of its own set to `forbid` or `deny` in `[lints.rust]`, or cannot be parsed: rustc's default
//!    level is `allow`, so without the table the targets that (1) does not reach (integration
//!    tests, examples, benches, build scripts) would compile `unsafe` code unflagged.
//!
//! The scan of (2) reads attributes, not lines, so a multi-line attribute counts and the lint's
//! name in a string inside an attribute (a `reason`, a `doc`) does not; an attribute written out
//! in a comment or a string literal is scanned like code and counts. This file needs no
//! exclusion: the attributes its tests use are assembled at run time, and no attribute of its
//! own names the lint.

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use toml_edit::DocumentMut;

use crate::repo;

/// The one member that may use `unsafe` (GEN-071).
const FFI: &str = "aldis-ffi";

/// The lint of the check.
const LINT: &str = "unsafe_code";

/// The levels that let `unsafe` code compile under a `deny` or lower a `forbid`'s neighbour.
const WEAKENING: [&str; 3] = ["allow", "expect", "warn"];

/// Whether `src`, the root of a target, has the line `#![forbid(unsafe_code)]` (GEN-070).
fn forbids_unsafe(src: &str) -> bool {
    src.lines()
        .any(|l| l.trim() == format!("#![forbid({LINT})]"))
}

/// The identifiers of the attribute body `body`, outside string literals.
fn identifiers(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut start = None;
    for (i, c) in body.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            start.get_or_insert(i);
            continue;
        }
        if let Some(s) = start.take() {
            out.extend(body.get(s..i));
        }
        in_string = c == '"';
    }
    if let Some(s) = start {
        out.extend(body.get(s..));
    }
    out
}

/// The 1-based line of every attribute in `src` that names `unsafe_code` with `allow`, `expect`,
/// or `warn`, on one line or several.
fn weakening_lines(src: &str) -> Vec<usize> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        i += 1;
        if b != b'#' {
            continue;
        }
        let mut open = i;
        if bytes.get(open) == Some(&b'!') {
            open += 1;
        }
        if bytes.get(open) != Some(&b'[') {
            continue;
        }
        // The attribute ends at the bracket that closes the one it opens; a bracket in a string
        // does not count.
        let mut depth = 0_usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut end = None;
        for (k, &c) in bytes.iter().enumerate().skip(open) {
            if in_string {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, b'\\') => escaped = true,
                    (false, b'"') => in_string = false,
                    _ => {}
                }
                continue;
            }
            match c {
                b'"' => in_string = true,
                b'[' => depth = depth.saturating_add(1),
                b']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(body) = end.and_then(|e| src.get(open..e)) else {
            continue;
        };
        let ids = identifiers(body);
        if ids.contains(&LINT) && ids.iter().any(|id| WEAKENING.contains(id)) {
            let line = bytes.iter().take(open).filter(|&&c| c == b'\n').count();
            out.push(line.saturating_add(1));
        }
    }
    out
}

/// Problems with the roots of the library and binary targets of `pkg` (GEN-070, VER-002).
fn root_problems(pkg: &Value) -> Vec<String> {
    let name = pkg["name"].as_str().unwrap_or("?");
    let mut problems = Vec::new();
    for t in pkg["targets"].as_array().into_iter().flatten() {
        let is_root = t["kind"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|k| matches!(k, "lib" | "bin"));
        if !is_root {
            continue;
        }
        let path = t["src_path"].as_str().unwrap_or_default();
        let ok = std::fs::read_to_string(path).is_ok_and(|s| forbids_unsafe(&s));
        if !ok {
            problems.push(format!(
                "GEN-070 {name}: {} lacks #![forbid({LINT})]",
                repo::rel(Path::new(path))
            ));
        }
    }
    problems
}

/// Problems with the Rust files under the directory of `pkg` (GEN-070, VER-002).
fn source_problems(pkg: &Value) -> Result<Vec<String>, String> {
    let name = pkg["name"].as_str().unwrap_or("?");
    let manifest = pkg["manifest_path"].as_str().unwrap_or_default();
    let dir = Path::new(manifest)
        .parent()
        .ok_or_else(|| format!("{name}: no manifest directory"))?;
    let rel = repo::rel(dir);
    let mut problems = Vec::new();
    for file in repo::files(&rel, &["rs"]) {
        for line in weakening_lines(&repo::read(&file)?) {
            problems.push(format!(
                "GEN-070 {name}: {}:{line} lowers {LINT}; only {FFI} may use unsafe code",
                repo::rel(&file)
            ));
        }
    }
    Ok(problems)
}

/// The way the manifest text `manifest` of the member `name` fails to keep `unsafe_code` at
/// `deny` or `forbid` for every target, if it does (GEN-070, VER-002).
fn manifest_problem(name: &str, manifest: &str) -> Option<String> {
    let doc = match manifest.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => return Some(format!("GEN-070 {name}: manifest does not parse: {e}")),
    };
    let lints = doc.get("lints");
    if lints
        .and_then(|l| l.get("workspace"))
        .and_then(toml_edit::Item::as_bool)
        == Some(true)
    {
        return None;
    }
    let level = lints
        .and_then(|l| l.get("rust"))
        .and_then(|r| r.get(LINT))
        .and_then(|u| u.as_str().or_else(|| u.get("level")?.as_str()));
    match level {
        Some("forbid" | "deny") => None,
        Some(other) => Some(format!(
            "GEN-070 {name}: [lints.rust] sets {LINT} = \"{other}\"; use forbid or deny"
        )),
        None => Some(format!(
            "GEN-070 {name}: Cargo.toml has neither [lints] workspace = true nor {LINT} in \
             [lints.rust]"
        )),
    }
}

/// Problems with the lint table of the manifest of `pkg` (GEN-070, VER-002).
fn lint_problems(pkg: &Value) -> Vec<String> {
    let name = pkg["name"].as_str().unwrap_or("?");
    let Some(path) = pkg["manifest_path"].as_str() else {
        return vec![format!(
            "GEN-070 {name}: cargo metadata gives no manifest path"
        )];
    };
    match std::fs::read_to_string(path) {
        Ok(text) => manifest_problem(name, &text).into_iter().collect(),
        Err(e) => vec![format!("GEN-070 {name}: cannot read {path}: {e}")],
    }
}

/// Checks the rules; returns the number of violations.
pub fn check() -> Result<usize, String> {
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
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages: Vec<&Value> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"].as_str() != Some(FFI))
        .collect();
    let mut problems = Vec::new();
    for p in &packages {
        problems.extend(lint_problems(p));
        problems.extend(root_problems(p));
        problems.extend(source_problems(p)?);
    }
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "unsafe-forbid: {} crate(s) besides {FFI}, {} violation(s)",
        packages.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::{forbids_unsafe, lint_problems, manifest_problem, root_problems, weakening_lines};
    use serde_json::json;

    /// An inner attribute `#![<level>(unsafe_code)]`, assembled here so that this file holds no
    /// attribute that the check would report.
    fn inner(level: &str) -> String {
        format!("#![{level}({})]", super::LINT)
    }

    #[test]
    fn a_root_without_the_forbid_fails() {
        // req: GEN-070, VER-002
        let dir = std::env::temp_dir().join(format!("xtask-unsafe-forbid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("lib.rs");
        let good = dir.join("main.rs");
        std::fs::write(&bad, format!("{}\nfn f() {{}}\n", "#![no_std]")).unwrap();
        std::fs::write(&good, format!("{}\nfn main() {{}}\n", inner("forbid"))).unwrap();
        let pkg = |path: &std::path::Path, kind: &str| {
            json!({ "name": "demo", "targets": [
                { "kind": [kind], "src_path": path.to_str().unwrap() },
                { "kind": ["test"], "src_path": "/nonexistent/tests/t.rs" }
            ]})
        };
        let problems = root_problems(&pkg(&bad, "lib"));
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].starts_with("GEN-070 demo: "), "{problems:#?}");
        assert_eq!(root_problems(&pkg(&good, "bin")), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!forbids_unsafe("// no attribute\nfn f() {}\n"));
        assert!(forbids_unsafe(&format!("//! doc\n  {}\n", inner("forbid"))));
        assert!(!forbids_unsafe(&inner("deny")));
    }

    #[test]
    fn a_lowered_lint_outside_the_ffi_crate_fails() {
        // req: GEN-070, VER-002
        for level in ["allow", "expect", "warn"] {
            let src = format!("fn a() {{}}\n{}\nfn b() {{}}\n", inner(level));
            assert_eq!(weakening_lines(&src), [2], "{level}");
        }
        // An outer attribute, inside `cfg_attr`, and on several lines.
        let lint = super::LINT;
        let outer = format!(
            r#"#[expect({lint}, reason = "x")]
fn f() {{}}
"#
        );
        assert_eq!(weakening_lines(&outer), [1]);
        let nested = format!(
            r"
#![cfg_attr(test, allow(clippy::all, {lint}))]
"
        );
        assert_eq!(weakening_lines(&nested), [2]);
        let multi = format!(
            r#"

#[expect(
    {lint},
    reason = "x"
)]
"#
        );
        assert_eq!(weakening_lines(&multi), [3]);
    }

    #[test]
    fn what_is_no_lowering_passes() {
        // req: GEN-070, VER-002
        let lint = super::LINT;
        for src in [
            inner("forbid"),
            inner("deny"),
            format!(
                r#"#[expect(dead_code, reason = "{lint}")]
fn f() {{}}
"#
            ),
            format!("#[allow(dead_code)] // {lint}\nfn f() {{}}\n"),
            format!(
                r#"#[doc = "allow({lint}) ]"]
fn f() {{}}
"#
            ),
            format!("let a = [1]; // {lint}, allow\n"),
            "#[cfg(test)]\nmod tests {}\n".to_owned(),
        ] {
            assert!(weakening_lines(&src).is_empty(), "{src}");
        }
    }

    #[test]
    fn a_member_manifest_without_the_lint_table_fails() {
        // req: GEN-070, VER-002
        let lint = super::LINT;
        let head = "[package]\nname = \"demo\"\n";
        for ok in [
            format!("{head}\n[lints]\nworkspace = true\n"),
            format!("lints.workspace = true\n{head}"),
            format!("{head}\n[lints.rust]\n{lint} = \"forbid\"\n"),
            format!("{head}\n[lints.rust]\n{lint} = {{ level = \"deny\", priority = 1 }}\n"),
        ] {
            assert_eq!(manifest_problem("demo", &ok), None, "{ok}");
        }
        let none = manifest_problem("demo", head).unwrap();
        assert!(none.starts_with("GEN-070 demo: "), "{none}");
        // A table of its own that leaves the lint out, or turns it off.
        for bad in [
            format!("{head}\n[lints.rust]\nmissing_docs = \"deny\"\n"),
            format!("{head}\n[lints.rust]\n{lint} = \"allow\"\n"),
            format!("{head}\n[lints.rust]\n{lint} = {{ level = \"warn\" }}\n"),
            format!("{head}\n[lints]\nworkspace = false\n"),
        ] {
            assert!(manifest_problem("demo", &bad).is_some(), "{bad}");
        }
        let unparsable = manifest_problem("demo", "[lints\nworkspace = true").unwrap();
        assert!(unparsable.contains("does not parse"), "{unparsable}");
    }

    #[test]
    fn a_manifest_that_cannot_be_read_fails() {
        // req: GEN-070, VER-002
        let missing = json!({ "name": "demo", "manifest_path": "/nonexistent/Cargo.toml" });
        assert_eq!(lint_problems(&missing).len(), 1);
        assert_eq!(lint_problems(&json!({ "name": "demo" })).len(), 1);
        let dir = std::env::temp_dir().join(format!("xtask-unsafe-lints-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Cargo.toml");
        std::fs::write(&path, "[lints]\nworkspace = true\n").unwrap();
        let pkg = json!({ "name": "demo", "manifest_path": path.to_str().unwrap() });
        assert_eq!(lint_problems(&pkg), Vec::<String>::new());
        std::fs::write(&path, "[lints.rust]\nunsafe_code = \"allow\"\n").unwrap();
        assert_eq!(lint_problems(&pkg).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
