//! `xtask refapp-deps`: rules for the reference application workspace `refapp/`
//! (docs/reference-application.md §9).
//!
//! APP-100  Aldis only through the `aldis` facade, by path, default features off; no other
//!          `aldis-*` crate (also not `aldis-testkit`), no `test-hooks` feature, also not as
//!          dev-dependencies.
//! APP-105  `#![forbid(unsafe_code)]` at the root of every library and binary target.
//! GEN-082  No `.rs` file names an item of the codec-facing tier of `aldis-core`: the modules
//!          `bits` and `container`, and the traits `FrameFormat`, `PhysicalChannel`,
//!          `FramePlan`, `FinishPlan`, `VariablePlan`, `Delimiter`, and `StreamFrame`.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

use crate::repo;
use crate::rs020::{Tok, tokens};

/// The modules of the codec-facing tier of `aldis-core` (GEN-082).
const TIER_MODULES: [&str; 2] = ["bits", "container"];

/// The traits of the codec-facing tier of `aldis-core` (GEN-082).
const TIER_TRAITS: [&str; 7] = [
    "FrameFormat",
    "PhysicalChannel",
    "FramePlan",
    "FinishPlan",
    "VariablePlan",
    "Delimiter",
    "StreamFrame",
];

/// The path segments after which a tier item names the `aldis-core` one: the facade, the crate,
/// and `core`, which a `use aldis::core::{self, ..}` makes a local name.
const TIER_ROOTS: [&str; 3] = ["aldis", "aldis_core", "core"];

fn is_tier_item(seg: &str) -> bool {
    TIER_MODULES.contains(&seg) || TIER_TRAITS.contains(&seg)
}

/// The tier item a path names: a segment after a root of [`TIER_ROOTS`] that is a tier module or
/// trait, or the `*` of a glob that imports the modules or traits of the crate.
fn tier_item_of(path: &[String]) -> Option<&str> {
    for (i, seg) in path.iter().enumerate() {
        if TIER_ROOTS.contains(&seg.as_str())
            && let Some(hit) = path[i + 1..].iter().find(|s| is_tier_item(s))
        {
            return Some(hit);
        }
    }
    let (last, head) = path.split_last()?;
    let head: Vec<&str> = head.iter().map(String::as_str).collect();
    let glob = last == "*"
        && matches!(
            head.as_slice(),
            ["aldis", "core"]
                | ["aldis_core"]
                | [
                    "aldis",
                    "core" | "aldis_core",
                    "format" | "stream" | "writer"
                ]
                | ["aldis_core", "format" | "stream" | "writer"]
        );
    glob.then_some("*")
}

/// The paths of the use tree that starts at `pos`, with the prefix `prefix` (`a::{b, c::d}` gives
/// `a::b` and `a::c::d`); `as` renames are dropped. `pos` ends after the tree.
fn use_tree(t: &[Tok], pos: &mut usize, prefix: &[String], out: &mut Vec<Vec<String>>) {
    let mut path = prefix.to_vec();
    loop {
        match t.get(*pos).map(|x| x.text.as_str()) {
            Some("::") => *pos += 1,
            Some("{") => {
                *pos += 1;
                loop {
                    use_tree(t, pos, &path, out);
                    match t.get(*pos).map(|x| x.text.as_str()) {
                        Some(",") => *pos += 1,
                        Some("}") => {
                            *pos += 1;
                            return;
                        }
                        _ => return,
                    }
                }
            }
            Some("as") => {
                *pos += 2;
                out.push(path);
                return;
            }
            Some("," | "}" | ";") | None => {
                out.push(path);
                return;
            }
            Some(seg) => {
                path.push(seg.to_owned());
                *pos += 1;
            }
        }
    }
}

fn ident_start(t: &Tok) -> bool {
    t.text
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
}

/// Problems with the source of one `refapp/` file (GEN-082): a `use` of a tier item, a path in the
/// code that names one, and a tier trait in the header of an `impl`. A textual scan of the tokens,
/// as `rs020` does: comments and literals are not code, and a renaming of the crate
/// (`use aldis::core as c;`) is not followed.
fn tier_problems(file: &str, src: &str) -> Vec<String> {
    let t = tokens(src);
    let mut found: BTreeSet<(usize, String)> = BTreeSet::new();
    let mut i = 0;
    while i < t.len() {
        let tok = &t[i];
        let follows_use = t
            .get(i + 1)
            .is_some_and(|n| n.text == "::" || n.text == "{" || ident_start(n));
        if tok.text == "use" && follows_use {
            let mut paths = Vec::new();
            let mut pos = i + 1;
            use_tree(&t, &mut pos, &[], &mut paths);
            for p in &paths {
                if let Some(item) = tier_item_of(p) {
                    found.insert((tok.line, item.to_owned()));
                }
            }
            i = pos.max(i + 1);
            continue;
        }
        if tok.text == "impl" {
            for h in t[i + 1..]
                .iter()
                .take_while(|h| !matches!(h.text.as_str(), "{" | ";" | "where"))
            {
                if TIER_TRAITS.contains(&h.text.as_str()) {
                    found.insert((h.line, h.text.clone()));
                }
            }
        }
        let after_path = i > 0 && t[i - 1].text == "::";
        if ident_start(tok) && !after_path {
            let mut chain = vec![tok.text.clone()];
            let mut j = i + 1;
            while t.get(j).is_some_and(|c| c.text == "::") && t.get(j + 1).is_some_and(ident_start)
            {
                chain.push(t[j + 1].text.clone());
                j += 2;
            }
            if let Some(item) = tier_item_of(&chain) {
                found.insert((tok.line, item.to_owned()));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    found
        .into_iter()
        .map(|(line, item)| {
            format!(
                "GEN-082 {file}:{line}: names `{item}`, an item of the codec-facing tier of aldis-core; \
                 refapp/ uses the integrator-facing items only"
            )
        })
        .collect()
}

/// Problems with one package's dependencies (APP-100).
fn dependency_problems(pkg: &Value) -> Vec<String> {
    let name = pkg["name"].as_str().unwrap_or("?");
    let mut problems = Vec::new();
    for d in pkg["dependencies"].as_array().into_iter().flatten() {
        let dep = d["name"].as_str().unwrap_or_default();
        let kind = d["kind"].as_str().unwrap_or("normal");
        let at = format!("APP-100 {name} ({kind} dependency {dep})");
        let features: Vec<&str> = d["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if features.iter().any(|f| f.contains("test-hooks")) {
            problems.push(format!("{at}: enables a test-hooks feature"));
        }
        if dep == "aldis" {
            if d["uses_default_features"].as_bool() != Some(false) {
                problems.push(format!("{at}: default features must be off"));
            }
            if !d["source"].is_null() {
                problems.push(format!("{at}: must be the path dependency ../crates/aldis"));
            }
        } else if dep.starts_with("aldis-") || dep.starts_with("aldis_") {
            problems.push(format!(
                "{at}: refapp/ uses Aldis only through the `aldis` facade"
            ));
        }
    }
    problems
}

/// Problems with the crate roots of one package (APP-105).
fn unsafe_problems(pkg: &Value) -> Vec<String> {
    let name = pkg["name"].as_str().unwrap_or("?");
    let mut problems = Vec::new();
    for t in pkg["targets"].as_array().into_iter().flatten() {
        let kinds: Vec<&str> = t["kind"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !kinds.iter().any(|k| matches!(*k, "lib" | "bin")) {
            continue;
        }
        let path = t["src_path"].as_str().unwrap_or_default();
        let ok = std::fs::read_to_string(path)
            .is_ok_and(|s| s.lines().any(|l| l.trim() == "#![forbid(unsafe_code)]"));
        if !ok {
            problems.push(format!(
                "APP-105 {name}: {} lacks #![forbid(unsafe_code)]",
                repo::rel(std::path::Path::new(path))
            ));
        }
    }
    problems
}

/// Problems with every `.rs` file under `refapp/` (GEN-082); returns them and the file count.
fn tier_problems_of_files() -> Result<(Vec<String>, usize), String> {
    let files = repo::files("refapp", &["rs"]);
    let mut problems = Vec::new();
    for f in &files {
        let src = repo::read(f)?;
        problems.extend(tier_problems(&repo::rel(f), &src));
    }
    Ok((problems, files.len()))
}

/// `xtask fix`: regenerates the scenario schema and constants of `refapp/` (APP-107) with
/// `gen-scenario`; returns 0, or an error when the generator cannot run or fails.
pub fn gen_scenario() -> Result<usize, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .current_dir(repo::root().join("refapp"))
        .args([
            "run",
            "-p",
            "refapp-harness",
            "--locked",
            "--",
            "gen-scenario",
        ])
        .status()
        .map_err(|e| format!("gen-scenario: {e}"))?;
    if status.success() {
        Ok(0)
    } else {
        Err(format!("gen-scenario (refapp/) failed: {status}"))
    }
}

/// Checks the rules; returns the number of violations.
pub fn check() -> Result<usize, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let manifest = repo::root().join("refapp/Cargo.toml");
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata (refapp/) failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let mut problems = Vec::new();
    for p in &packages {
        problems.extend(dependency_problems(p));
        problems.extend(unsafe_problems(p));
    }
    let (tier, files) = tier_problems_of_files()?;
    problems.extend(tier);
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "refapp-deps: {} refapp/ crate(s), {files} .rs file(s), {} violation(s)",
        packages.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::{dependency_problems, tier_problems};
    use serde_json::json;

    #[test]
    fn codec_facing_tier_is_refused() {
        // req: GEN-082
        let ok = "\
use aldis::core::{Error, ErrorKind, Result, counter, crc::crc16};
use aldis::core::ids::{Apid, SeqCount};
use crate::bits::pack;
// aldis::core::bits is a comment, and \"aldis::core::container\" a string
fn f() -> core::option::Option<u8> { let _ = aldis::core::time::Instant::ZERO; None }
";
        assert_eq!(tier_problems("a.rs", ok), Vec::<String>::new());

        let bad = "\
use aldis::core::bits::read_u16;
use aldis::core::{counter, container::FixedVec};
use aldis_core::format::FrameFormat as Format;
use aldis::core::*;
use aldis::core::{self};
impl aldis::core::stream::Delimiter for Mine {}
impl<'a> StreamFrame<'a> for Mine {}
fn g() { let _ = aldis::core::bits::read_bit(&[0], 0); let _ = core::container::FixedVec::<u8, 1>::new(); }
";
        let problems = tier_problems("b.rs", bad);
        let lines: Vec<usize> = problems
            .iter()
            .map(|p| {
                p.split(':')
                    .nth(1)
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0)
            })
            .collect();
        assert_eq!(lines, [1, 2, 3, 4, 6, 7, 8, 8], "{problems:#?}");
        assert!(
            problems[0].contains("GEN-082 b.rs:1: names `bits`"),
            "{problems:#?}"
        );
    }

    #[test]
    fn facade_only() {
        // req: APP-100
        let ok = json!({ "name": "refapp-flight", "dependencies": [
            { "name": "aldis", "kind": null, "uses_default_features": false,
              "features": ["core"], "source": null },
            { "name": "refapp-scenario", "kind": null, "uses_default_features": true,
              "features": [], "source": null },
            { "name": "serde", "kind": null, "uses_default_features": true,
              "features": [], "source": "registry+https://github.com/rust-lang/crates.io-index" }
        ]});
        assert_eq!(dependency_problems(&ok), Vec::<String>::new());

        let bad = json!({ "name": "refapp-ground", "dependencies": [
            { "name": "aldis", "kind": null, "uses_default_features": true,
              "features": [], "source": null },
            { "name": "aldis", "kind": "dev", "uses_default_features": false,
              "features": [], "source": "registry+https://github.com/rust-lang/crates.io-index" },
            { "name": "aldis-testkit", "kind": "dev", "uses_default_features": true,
              "features": [], "source": null },
            { "name": "aldis-core", "kind": null, "uses_default_features": false,
              "features": [], "source": null },
            { "name": "other", "kind": null, "uses_default_features": true,
              "features": ["aldis-core/test-hooks"], "source": null }
        ]});
        let problems = dependency_problems(&bad);
        assert_eq!(problems.len(), 5, "{problems:#?}");
    }
}
