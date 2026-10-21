//! The crate graph of the workspace, from `cargo metadata` (VER-056): for each package its
//! directory and the workspace packages it depends on, so that the inputs of a check can include
//! exactly the xtask crates it runs and their local dependencies.
//!
//! The graph is read offline from the manifests (`--no-deps`), and it is refused, so that every
//! xtask path stays an input of every check, when an xtask crate could read or run something at
//! build time that its directory does not hold: a build script, a procedural macro, a source
//! file that includes another file (`include!`, `include_str!`, `include_bytes!`, or a `path`
//! attribute), or a manifest that does not forbid `unsafe` code (docs/06-verification.md §7).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Where the xtask crates live; every package below it is one.
pub const XTASK_DIR: &str = "xtask/";

/// What an xtask source file may not contain: each reads a file at build time.
const BUILD_READS: &[&str] = &[
    concat!("include", "!("),
    concat!("include_", "str!("),
    concat!("include_", "bytes!("),
    concat!("#[", "path"),
];

/// The line every xtask manifest has (RS-017): without `unsafe`, a crate's code runs only when it
/// is called (docs/06-verification.md §7).
const FORBID_UNSAFE: &str = "unsafe_code = \"forbid\"";

/// One package of the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// Its directory relative to the workspace root, with forward slashes.
    pub dir: String,
    /// The workspace packages it depends on through normal and build dependencies.
    pub deps: Vec<String>,
}

/// The packages of the workspace, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// Every workspace member.
    pub packages: BTreeMap<String, Package>,
}

impl Graph {
    /// The graph of the workspace at `root`, read with `cargo metadata --no-deps --offline` and
    /// checked as the module documentation says.
    pub fn load(root: &Path) -> Result<Self, String> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let out = Command::new(cargo)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let meta: Value =
            serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
        let graph = Self::from_metadata(&meta)?;
        graph.check_xtask_files(root)?;
        Ok(graph)
    }

    /// The graph described by the output of `cargo metadata --no-deps`.
    pub fn from_metadata(meta: &Value) -> Result<Self, String> {
        let root = meta
            .get("workspace_root")
            .and_then(Value::as_str)
            .ok_or("cargo metadata names no workspace root")?;
        let rel = |path: &str| -> Result<String, String> {
            let path = path.replace('\\', "/");
            let root = root.replace('\\', "/");
            path.strip_prefix(&root)
                .map(|p| p.trim_start_matches('/').to_owned())
                .filter(|p| !p.is_empty())
                .ok_or_else(|| format!("{path} is not below the workspace root {root}"))
        };
        let packages = meta
            .get("packages")
            .and_then(Value::as_array)
            .ok_or("cargo metadata lists no packages")?;
        let mut dirs: BTreeMap<String, String> = BTreeMap::new();
        for p in packages {
            let name = p["name"].as_str().ok_or("a package has no name")?;
            let manifest = p["manifest_path"]
                .as_str()
                .ok_or_else(|| format!("{name} has no manifest path"))?;
            let dir = rel(manifest)?
                .strip_suffix("/Cargo.toml")
                .ok_or_else(|| format!("{name}: {manifest} is not a Cargo.toml"))?
                .to_owned();
            dirs.insert(dir, name.to_owned());
        }
        let mut graph = Self::default();
        for p in packages {
            let name = p["name"].as_str().unwrap_or_default().to_owned();
            let dir = dirs
                .iter()
                .find(|(_, n)| **n == name)
                .map(|(d, _)| d.clone())
                .unwrap_or_default();
            if dir.starts_with(XTASK_DIR) {
                for kind in p["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|t| t["kind"].as_array().into_iter().flatten())
                    .filter_map(Value::as_str)
                {
                    if matches!(kind, "custom-build" | "proc-macro") {
                        return Err(format!("the xtask crate {name} has a {kind} target"));
                    }
                }
            }
            let mut deps = Vec::new();
            for d in p["dependencies"].as_array().into_iter().flatten() {
                if d["kind"].as_str() == Some("dev") {
                    continue;
                }
                let Some(path) = d["path"].as_str() else {
                    continue;
                };
                let dep_dir = rel(path)?;
                let dep = dirs.get(&dep_dir).ok_or_else(|| {
                    format!("{name} depends on {dep_dir}, which is not a workspace member")
                })?;
                deps.push(dep.clone());
            }
            deps.sort();
            deps.dedup();
            graph.packages.insert(name, Package { dir, deps });
        }
        Ok(graph)
    }

    /// `name` and every workspace package it depends on, directly or not.
    pub fn closure(&self, name: &str) -> Result<BTreeSet<String>, String> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![name.to_owned()];
        while let Some(cur) = todo.pop() {
            let package = self
                .packages
                .get(&cur)
                .ok_or_else(|| format!("no workspace package {cur}"))?;
            if seen.insert(cur) {
                todo.extend(package.deps.iter().cloned());
            }
        }
        Ok(seen)
    }

    /// The xtask crates: the packages below `xtask/`.
    pub fn xtask(&self) -> impl Iterator<Item = &String> {
        self.packages
            .iter()
            .filter(|(_, p)| p.dir.starts_with(XTASK_DIR))
            .map(|(n, _)| n)
    }

    /// Checks that no xtask crate reads, at build time, a file outside its directory, and that
    /// each forbids `unsafe` code.
    fn check_xtask_files(&self, root: &Path) -> Result<(), String> {
        for name in self.xtask() {
            let dir = &self.packages[name].dir;
            let manifest = root.join(dir).join("Cargo.toml");
            let text =
                std::fs::read_to_string(&manifest).map_err(|e| format!("{dir}/Cargo.toml: {e}"))?;
            if !text.lines().any(|l| l.trim() == FORBID_UNSAFE) {
                return Err(format!("{dir}/Cargo.toml does not say {FORBID_UNSAFE}"));
            }
            for file in rust_files(&root.join(dir))? {
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                if let Some(word) = BUILD_READS.iter().find(|w| text.contains(*w)) {
                    return Err(format!(
                        "{} reads a file at build time ({word})",
                        file.strip_prefix(root).unwrap_or(&file).display()
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The `.rs` files under `dir`, build output (`target/`) left out.
fn rust_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "target") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn package(name: &str, dir: &str, deps: &[(&str, Option<&str>)]) -> Value {
        let deps: Vec<Value> = deps
            .iter()
            .map(|(d, kind)| json!({"name": d, "kind": kind, "path": format!("/w/{d}")}))
            .collect();
        json!({"name": name, "manifest_path": format!("/w/{dir}/Cargo.toml"),
            "targets": [{"kind": ["lib"]}], "dependencies": deps})
    }

    // req: VER-056
    #[test]
    fn closures_follow_normal_and_build_dependencies_only() {
        let meta = json!({"workspace_root": "/w", "packages": [
            package("a", "a", &[("b", None), ("c", Some("dev"))]),
            package("b", "b", &[("d", Some("build"))]),
            package("c", "c", &[]),
            package("d", "d", &[]),
        ]});
        let mut meta = meta;
        meta["packages"][0]["dependencies"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "serde_json", "kind": null}));
        let g = Graph::from_metadata(&meta).unwrap();
        assert_eq!(
            g.closure("a").unwrap().into_iter().collect::<Vec<_>>(),
            ["a", "b", "d"]
        );
        assert_eq!(g.packages["b"].dir, "b");
        assert!(g.closure("x").is_err());
    }

    // req: VER-056
    #[test]
    fn what_an_xtask_crate_may_not_have() {
        // A path dependency that is not a workspace member.
        let outside = json!({"workspace_root": "/w", "packages": [
            package("xtask-a", "xtask/a", &[("elsewhere", None)]),
        ]});
        assert!(
            Graph::from_metadata(&outside)
                .unwrap_err()
                .contains("not a workspace member")
        );
        // A build script or a procedural macro in an xtask crate, but not elsewhere.
        for kind in ["custom-build", "proc-macro"] {
            let mut meta = json!({"workspace_root": "/w", "packages": [
                package("xtask-a", "xtask/a", &[]),
                package("other", "crates/other", &[]),
            ]});
            meta["packages"][1]["targets"] = json!([{"kind": [kind]}]);
            assert!(Graph::from_metadata(&meta).is_ok());
            meta["packages"][0]["targets"] = json!([{"kind": [kind]}]);
            assert_eq!(
                Graph::from_metadata(&meta),
                Err(format!("the xtask crate xtask-a has a {kind} target"))
            );
        }
    }
}
