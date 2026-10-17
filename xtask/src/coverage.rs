//! Coverage gate (VER-024): 100 % of regions, functions, lines, and branches for the crates in
//! [`GATED`], measured with `cargo llvm-cov --branch` on the pinned tooling nightly (RS-002).
//!
//! Test modules carry `#[cfg_attr(coverage_nightly, coverage(off))]`, so only library code is
//! measured. Uncovered code is justified in `coverage-waivers.toml`; the gate accepts a crate
//! only if every miss is waived there.

use std::fmt::Write as _;
use std::process::Command;

use serde_json::Value;

use crate::repo;

/// Crates gated in M0 (Layer 0). Layers 1–2 join as they gain code (VER-024).
const GATED: &[&str] = &["aldis-core"];

/// The four metrics reported by `llvm-cov`, in report order.
const METRICS: &[&str] = &["regions", "functions", "lines", "branches"];

/// Waived misses per metric for one crate, from `coverage-waivers.toml`.
///
/// The file is a list of `[[waiver]]` tables with `crate`, `metric`, `count`, and
/// `justification` keys; it is parsed here line by line to avoid a TOML dependency.
fn waivers(krate: &str) -> Result<[u64; 4], String> {
    let path = repo::root().join("coverage-waivers.toml");
    let text = repo::read(&path)?;
    let mut out = [0_u64; 4];
    let mut entries: Vec<Vec<(String, String)>> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[waiver]]" {
            entries.push(Vec::new());
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("coverage-waivers.toml:{}: expected `key = value`", n + 1))?;
        let entry = entries.last_mut().ok_or_else(|| {
            format!(
                "coverage-waivers.toml:{}: key outside a [[waiver]] table",
                n + 1
            )
        })?;
        entry.push((
            key.trim().to_owned(),
            value.trim().trim_matches('"').to_owned(),
        ));
    }
    for entry in &entries {
        let get = |k: &str| {
            entry
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .ok_or_else(|| format!("coverage-waivers.toml: a [[waiver]] has no `{k}`"))
        };
        if get("justification")?.is_empty() {
            return Err("coverage-waivers.toml: empty justification".to_owned());
        }
        if get("crate")? != krate {
            continue;
        }
        let metric = get("metric")?;
        let i = METRICS
            .iter()
            .position(|m| *m == metric)
            .ok_or_else(|| format!("coverage-waivers.toml: unknown metric `{metric}`"))?;
        let count: u64 = get("count")?
            .parse()
            .map_err(|e| format!("coverage-waivers.toml: count: {e}"))?;
        if let Some(slot) = out.get_mut(i) {
            *slot += count;
        }
    }
    Ok(out)
}

fn count(summary: &Value, metric: &str, key: &str) -> u64 {
    summary
        .get(metric)
        .and_then(|m| m.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// `xtask coverage`: runs the gate for every crate in [`GATED`] and writes
/// `target/spec/coverage-<crate>.md`.
pub fn run() -> Result<usize, String> {
    let nightly = repo::tooling_nightly()?;
    let out_dir = repo::root().join("target/spec");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    // Objects left by an older layout of the build directory are still passed to `llvm-cov`
    // and show up as uncovered code, so every run starts clean.
    let cov_dir = repo::root().join("target/llvm-cov-target");
    if cov_dir.exists() {
        std::fs::remove_dir_all(&cov_dir).map_err(|e| format!("{}: {e}", repo::rel(&cov_dir)))?;
    }

    let mut problems = 0;
    for krate in GATED {
        let json = out_dir.join(format!("coverage-{krate}.json"));
        let status = Command::new("rustup")
            .args(["run", &nightly, "cargo", "llvm-cov", "-p", krate])
            .args(["--all-features", "--branch", "--json", "--summary-only"])
            .arg("--output-path")
            .arg(&json)
            .current_dir(repo::root())
            .status()
            .map_err(|e| format!("cargo llvm-cov: {e} (install cargo-llvm-cov >= 0.9)"))?;
        if !status.success() {
            return Err(format!("cargo llvm-cov -p {krate} failed"));
        }
        let report: Value = serde_json::from_str(&repo::read(&json)?)
            .map_err(|e| format!("{}: {e}", repo::rel(&json)))?;
        let data = report
            .pointer("/data/0")
            .ok_or_else(|| format!("{}: no data", repo::rel(&json)))?;
        let waived = waivers(krate)?;

        let mut md = format!(
            "# Coverage — `{krate}` (VER-024)\n\n\
             `cargo llvm-cov --branch` on `{nightly}`; test modules excluded.\n\n\
             | File | Regions | Functions | Lines | Branches |\n|---|---:|---:|---:|---:|\n"
        );
        let cell =
            |s: &Value, m: &str| format!("{}/{}", count(s, m, "covered"), count(s, m, "count"));
        for file in data
            .get("files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = file.get("filename").and_then(Value::as_str).unwrap_or("?");
            let name = repo::rel(std::path::Path::new(name));
            let s = file.get("summary").unwrap_or(&Value::Null);
            let cells: Vec<String> = METRICS.iter().map(|m| cell(s, m)).collect();
            let _ = writeln!(md, "| `{name}` | {} |", cells.join(" | "));
        }
        let totals = data.get("totals").unwrap_or(&Value::Null);
        let cells: Vec<String> = METRICS.iter().map(|m| cell(totals, m)).collect();
        let _ = writeln!(md, "| **total** | {} |", cells.join(" | "));

        let mut line = format!("coverage {krate}:");
        for (metric, allowed) in METRICS.iter().zip(waived) {
            let total = count(totals, metric, "count");
            let missed = total.saturating_sub(count(totals, metric, "covered"));
            let _ = write!(line, " {metric} {missed}/{total} missed");
            if allowed > 0 {
                let _ = write!(line, " ({allowed} waived)");
            }
            if missed > allowed {
                println!(
                    "error: VER-024 {krate} {metric}: {missed} missed, {allowed} waived; \
                     see target/spec/coverage-{krate}.md"
                );
                problems += 1;
            }
            if missed < allowed {
                println!(
                    "error: VER-024 {krate} {metric}: {allowed} waived but only {missed} missed; \
                     shrink the waiver"
                );
                problems += 1;
            }
        }
        // An instrumentation without branch data (old cargo-llvm-cov or stable rustc) would
        // pass the branch gate vacuously.
        if count(totals, "branches", "count") == 0 {
            println!(
                "error: VER-024 {krate}: no branch data; needs cargo-llvm-cov >= 0.9 and {nightly}"
            );
            problems += 1;
        }
        println!("{line}");
        let file = out_dir.join(format!("coverage-{krate}.md"));
        std::fs::write(&file, md).map_err(|e| e.to_string())?;
        println!("coverage report: {}", repo::rel(&file));
    }
    Ok(problems)
}
