//! `xtask refapp-deps`: rules for the reference application workspace `refapp/`
//! (docs/reference-application.md §9).
//!
//! APP-100  Aldis only through the `aldis` facade, by path, default features off; no other
//!          `aldis-*` crate (also not `aldis-testkit`), no `test-hooks` feature, also not as
//!          dev-dependencies.
//! APP-105  `#![forbid(unsafe_code)]` at the root of every library and binary target.

use std::process::Command;

use serde_json::Value;

use crate::repo;

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
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "refapp-deps: {} refapp/ crate(s), {} violation(s)",
        packages.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::dependency_problems;
    use serde_json::json;

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
