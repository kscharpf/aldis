//! `xtask layers`: crate dependency rules of docs/01-architecture.md §2.1.
//!
//! GEN-001  Layers 0–2: no third-party runtime (normal/build) dependencies.
//! GEN-002  A crate may depend only on lower layers (exception: aldis-spp → aldis-time).
//! GEN-003  aldis-spp alone pulls in no frame-protocol crate.
//! GEN-004  Layer 3: third-party runtime dependencies only if optional.
//! Tooling crates (std, dev-only) may be used by library crates only as dev-dependencies.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

const LAYERS: &[(&str, u8)] = &[
    ("aldis-core", 0),
    ("aldis-time", 1),
    ("aldis-spp", 1),
    ("aldis-epp", 1),
    ("aldis-tm", 1),
    ("aldis-tc", 1),
    ("aldis-aos", 1),
    ("aldis-uslp", 1),
    ("aldis-link", 2),
    ("aldis-cop1", 2),
    ("aldis-framework", 3),
];
const BOUNDARY: &[&str] = &["aldis", "aldis-ffi"];
const TOOLING: &[&str] = &["aldis-testkit", "xtask"];
const SAME_LAYER_ALLOWED: &[(&str, &str)] = &[("aldis-spp", "aldis-time")];
const FRAME_CRATES: &[&str] = &[
    "aldis-tm",
    "aldis-tc",
    "aldis-aos",
    "aldis-uslp",
    "aldis-link",
    "aldis-cop1",
];

struct Dep {
    name: String,
    dev: bool,
    optional: bool,
}

fn workspace() -> Result<BTreeMap<String, Vec<Dep>>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(crate::repo::root())
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let mut crates = BTreeMap::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let deps = p["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| Dep {
                name: d["name"].as_str().unwrap_or_default().to_owned(),
                dev: d["kind"].as_str() == Some("dev"),
                optional: d["optional"].as_bool().unwrap_or(false),
            })
            .collect();
        crates.insert(p["name"].as_str().unwrap_or_default().to_owned(), deps);
    }
    Ok(crates)
}

/// Checks the rules; returns the number of violations.
pub fn check() -> Result<usize, String> {
    let crates = workspace()?;
    let layer: BTreeMap<&str, u8> = LAYERS.iter().copied().collect();
    let mut errors = Vec::new();

    for (name, deps) in &crates {
        let name = name.as_str();
        if TOOLING.contains(&name) || BOUNDARY.contains(&name) {
            for d in deps
                .iter()
                .filter(|d| !d.dev && TOOLING.contains(&d.name.as_str()))
            {
                if BOUNDARY.contains(&name) {
                    errors.push(format!(
                        "{name} depends on tooling crate {} outside dev-dependencies",
                        d.name
                    ));
                }
            }
            continue;
        }
        let Some(&l) = layer.get(name) else {
            errors.push(format!("{name}: not assigned to a layer in xtask/src/layers.rs (update docs/01-architecture.md §2 too)"));
            continue;
        };
        for d in deps.iter().filter(|d| !d.dev) {
            let dn = d.name.as_str();
            if TOOLING.contains(&dn) {
                errors.push(format!(
                    "{name} depends on tooling crate {dn} outside dev-dependencies"
                ));
            } else if BOUNDARY.contains(&dn) {
                errors.push(format!("GEN-002 {name} depends on boundary crate {dn}"));
            } else if let Some(&dl) = layer.get(dn) {
                if dl > l || (dl == l && !SAME_LAYER_ALLOWED.contains(&(name, dn))) {
                    errors.push(format!("GEN-002 {name} (L{l}) depends on {dn} (L{dl})"));
                }
            } else if l <= 2 {
                errors.push(format!(
                    "GEN-001 {name} (L{l}) has third-party dependency {dn}"
                ));
            } else if !d.optional {
                errors.push(format!(
                    "GEN-004 {name} (L{l}) has non-optional third-party dependency {dn}"
                ));
            }
        }
    }

    // GEN-003: normal-dependency closure of aldis-spp (all features).
    let mut closure = BTreeSet::new();
    let mut todo = vec!["aldis-spp".to_owned()];
    while let Some(cur) = todo.pop() {
        for d in crates.get(&cur).into_iter().flatten().filter(|d| !d.dev) {
            if crates.contains_key(&d.name) && closure.insert(d.name.clone()) {
                todo.push(d.name.clone());
            }
        }
    }
    let bad: Vec<&str> = FRAME_CRATES
        .iter()
        .copied()
        .filter(|c| closure.contains(*c))
        .collect();
    if !bad.is_empty() {
        errors.push(format!(
            "GEN-003 aldis-spp pulls in frame-protocol crates: {}",
            bad.join(", ")
        ));
    }

    for e in &errors {
        println!("error: {e}");
    }
    println!(
        "layers: {} workspace crate(s), {} violation(s)",
        crates.len(),
        errors.len()
    );
    Ok(errors.len())
}
