//! Coverage gate (VER-024): 100 % of regions, functions, lines, and branches for the crates in
//! [`GATED`], measured with `cargo llvm-cov --branch` on the pinned tooling nightly (RS-002).
//!
//! Test modules carry `#[cfg_attr(coverage_nightly, coverage(off))]`, so only library code is
//! measured. Uncovered code is justified in `coverage-waivers.toml`; the gate accepts a crate
//! only if every miss is waived there. The crates in [`REPORTED`] are measured and reported the
//! same way but not gated: VER-024 covers Layers 0–2, and `aldis-ffi` is above them (M2 plan
//! D16).

use std::fmt::Write as _;
use std::process::Command;

use serde_json::Value;

use crate::repo;

/// Crates gated in M0 (Layer 0). Layers 1–2 join as they gain code (VER-024).
pub const GATED: &[&str] = &[
    "aldis-core",
    "aldis-time",
    "aldis-epp",
    "aldis-spp",
    "aldis-aos",
];

/// Crates measured and reported, not gated (M2 plan D16).
pub const REPORTED: &[&str] = &["aldis-ffi"];

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
        let value = value.trim();
        let clean_val = if let Some(rest) = value.strip_prefix('"') {
            rest.split_once('"')
                .map(|(s, _)| s)
                .ok_or_else(|| format!("coverage-waivers.toml:{}: unterminated string", n + 1))?
        } else {
            value.split('#').next().unwrap_or("").trim()
        };
        let entry = entries.last_mut().ok_or_else(|| {
            format!(
                "coverage-waivers.toml:{}: key outside a [[waiver]] table",
                n + 1
            )
        })?;
        entry.push((key.trim().to_owned(), clean_val.to_owned()));
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
        let target_crate = get("crate")?;
        if !GATED.contains(&target_crate) {
            return Err(format!(
                "coverage-waivers.toml: unknown or ungated crate `{target_crate}`"
            ));
        }
        if target_crate != krate {
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
    let cov_dir = repo::root().join("target/llvm-cov-target");

    let mut problems = 0;
    for krate in GATED {
        problems += check_crate(krate, &nightly, &out_dir, &cov_dir, true)?;
    }
    for krate in REPORTED {
        check_crate(krate, &nightly, &out_dir, &cov_dir, false)?;
    }
    Ok(problems)
}

fn check_crate(
    krate: &str,
    nightly: &str,
    out_dir: &std::path::Path,
    cov_dir: &std::path::Path,
    gated: bool,
) -> Result<usize, String> {
    // Objects left by an older build or a previous crate in the loop are still passed to
    // `llvm-cov` and would bleed into this crate's report, so each crate starts clean.
    if cov_dir.exists() {
        std::fs::remove_dir_all(cov_dir).map_err(|e| format!("{}: {e}", repo::rel(cov_dir)))?;
    }
    let json = out_dir.join(format!("coverage-{krate}.json"));
    let status = Command::new("rustup")
        .args(["run", nightly, "cargo", "llvm-cov", "-p", krate])
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
    let waived = if gated { waivers(krate)? } else { [0; 4] };

    let status = if gated {
        "gated at 100 %"
    } else {
        "reported, not gated (M2 plan D16)"
    };
    let mut md = format!(
        "# Coverage — `{krate}` (VER-024)\n\n\
         `cargo llvm-cov --branch` on `{nightly}`; test modules excluded; {status}.\n\n\
         | File | Regions | Functions | Lines | Branches |\n|---|---:|---:|---:|---:|\n"
    );
    let cell = |s: &Value, m: &str| format!("{}/{}", count(s, m, "covered"), count(s, m, "count"));
    let prefix = format!("crates/{krate}/");
    let mut matched_files = 0_usize;
    let mut covered_totals = [0_u64; 4];
    let mut count_totals = [0_u64; 4];
    for file in data
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = file.get("filename").and_then(Value::as_str).unwrap_or("?");
        let name = repo::rel(std::path::Path::new(name));
        if !name.starts_with(&prefix) {
            continue;
        }
        matched_files += 1;
        let s = file.get("summary").unwrap_or(&Value::Null);
        for (i, m) in METRICS.iter().enumerate() {
            covered_totals[i] += count(s, m, "covered");
            count_totals[i] += count(s, m, "count");
        }
        let cells: Vec<String> = METRICS.iter().map(|m| cell(s, m)).collect();
        let _ = writeln!(md, "| `{name}` | {} |", cells.join(" | "));
    }
    if matched_files == 0 {
        return Err(format!(
            "VER-024 {krate}: no source files matched `{prefix}` in {}",
            repo::rel(&json)
        ));
    }
    let cells: Vec<String> = (0..METRICS.len())
        .map(|i| format!("{}/{}", covered_totals[i], count_totals[i]))
        .collect();
    let _ = writeln!(md, "| **total** | {} |", cells.join(" | "));

    let (mut problems, line) = judge(krate, covered_totals, count_totals, waived, gated);
    // An instrumentation without branch data (old cargo-llvm-cov or stable rustc) would
    // pass the branch gate vacuously.
    if count_totals[3] == 0 {
        println!(
            "error: VER-024 {krate}: no branch data; needs cargo-llvm-cov >= 0.9 and {nightly}"
        );
        problems += 1;
    }
    println!("{line}");
    let file = out_dir.join(format!("coverage-{krate}.md"));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    println!("coverage report: {}", repo::rel(&file));
    Ok(problems)
}

/// The summary line of a crate and, when it is `gated`, the number of metrics whose misses
/// differ from the waived count.
fn judge(
    krate: &str,
    covered_totals: [u64; 4],
    count_totals: [u64; 4],
    waived: [u64; 4],
    gated: bool,
) -> (usize, String) {
    let mut problems = 0;
    let mut line = format!(
        "coverage {krate}{}:",
        if gated { "" } else { " (reported)" }
    );
    for (i, (metric, allowed)) in METRICS.iter().zip(waived).enumerate() {
        let total = count_totals[i];
        let missed = total.saturating_sub(covered_totals[i]);
        let _ = write!(line, " {metric} {missed}/{total} missed");
        if allowed > 0 {
            let _ = write!(line, " ({allowed} waived)");
        }
        if !gated {
            continue;
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
    (problems, line)
}
