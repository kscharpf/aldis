//! Coverage gate (VER-024): 100 % of regions, functions, lines, and branches for the crates in
//! [`gated`], measured with `cargo llvm-cov --branch` on the pinned tooling nightly (RS-002).
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

/// Crates gated: the crates of Layers 0–2 with code, derived ([`crate::layers::gated`], VER-024).
use crate::layers::gated;

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
        if !gated()?.contains(&target_crate) {
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

/// `xtask coverage`: runs the gate for every crate in [`gated`] and writes
/// `target/spec/coverage-<crate>.md`.
pub fn run() -> Result<usize, String> {
    run_selected(&gated()?, REPORTED)
}

/// The crates that `-p <crate>` arguments name, in order.
fn packages<'a>(args: &[&'a str]) -> Result<Vec<&'a str>, String> {
    let mut named = Vec::new();
    let mut rest = args;
    // bound: each pass consumes two words of `rest`.
    while let [flag, tail @ ..] = rest {
        let ("-p", [krate, tail @ ..]) = (*flag, tail) else {
            return Err("usage: cargo xtask coverage [-p <crate>]...".into());
        };
        named.push(*krate);
        rest = tail;
    }
    Ok(named)
}

/// `xtask coverage [-p <crate>]...`: the gate for every crate, or for the crates named.
pub fn main(args: &[&str]) -> Result<usize, String> {
    let named = packages(args)?;
    if named.is_empty() {
        return run();
    }
    let gated = gated()?;
    if let Some(bad) = named
        .iter()
        .find(|n| !gated.contains(n) && !REPORTED.contains(n))
    {
        return Err(format!(
            "coverage: `{bad}` is not measured; the crates are: {}",
            gated
                .iter()
                .chain(REPORTED)
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let pick = |crates: &[&str]| -> Vec<&str> {
        named
            .iter()
            .copied()
            .filter(|n| crates.contains(n))
            .collect()
    };
    run_selected(&pick(&gated), &pick(REPORTED))
}

/// `xtask check --full`: the gate for the gated crates among `names` (directories of `crates/`),
/// and the report of the reported ones; every crate when `aldis-core` is among them, since
/// every crate builds on it ([`select`]).
pub fn run_for(names: &[String]) -> Result<usize, String> {
    let (gated, reported) = select(names, &gated()?, REPORTED);
    if gated.is_empty() && reported.is_empty() {
        println!(
            "coverage: no gated crate among the changed ones: {}",
            names.join(", ")
        );
        return Ok(0);
    }
    run_selected(&gated, &reported)
}

/// The gated and the reported crates that a change to the crates `names` can affect: the ones
/// named, or all of them when `aldis-core` is named.
fn select<'a>(
    names: &[String],
    gated: &[&'a str],
    reported: &[&'a str],
) -> (Vec<&'a str>, Vec<&'a str>) {
    let all = names.iter().any(|n| n == "aldis-core");
    let pick = |crates: &[&'a str]| -> Vec<&'a str> {
        crates
            .iter()
            .copied()
            .filter(|c| all || names.iter().any(|n| n == c))
            .collect()
    };
    (pick(gated), pick(reported))
}

fn run_selected(gated: &[&str], reported: &[&str]) -> Result<usize, String> {
    let nightly = repo::tooling_nightly()?;
    let out_dir = repo::root().join("target/spec");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let cov_dir = repo::root().join("target/llvm-cov-target");

    let mut problems = 0;
    for krate in gated {
        problems += check_crate(krate, &nightly, &out_dir, &cov_dir, true)?;
    }
    for krate in reported {
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
    if gated && problems > 0 {
        // The profile data of this crate's run is still in `cov_dir`: the next crate wipes it.
        match uncovered(krate, nightly, out_dir) {
            Ok(ranges) if ranges.is_empty() => println!(
                "uncovered {krate}: no missed line or branch; the misses are regions or \
                 functions (see the report)"
            ),
            Ok(ranges) => {
                println!("uncovered {krate} (includes misses that coverage-waivers.toml waives):");
                for r in ranges {
                    println!("  {r}");
                }
            }
            Err(e) => println!("uncovered {krate}: cannot list: {e}"),
        }
    }
    let file = out_dir.join(format!("coverage-{krate}.md"));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    println!("coverage report: {}", repo::rel(&file));
    Ok(problems)
}

/// The lines and branches of `krate` that its tests missed, as `path:first-last (lines)` and
/// `path:line (branch)`, from the LCOV report of the profile data its run just left.
fn uncovered(krate: &str, nightly: &str, out_dir: &std::path::Path) -> Result<Vec<String>, String> {
    let lcov = out_dir.join(format!("coverage-{krate}.lcov"));
    let status = Command::new("rustup")
        .args([
            "run", nightly, "cargo", "llvm-cov", "report", "--branch", "--lcov",
        ])
        .arg("--output-path")
        .arg(&lcov)
        .current_dir(repo::root())
        .status()
        .map_err(|e| format!("cargo llvm-cov report: {e}"))?;
    if !status.success() {
        return Err("cargo llvm-cov report failed".into());
    }
    Ok(missed(&repo::read(&lcov)?, &format!("crates/{krate}/")))
}

/// The missed lines and branches in the LCOV text `lcov` for the files under `prefix`
/// (repository-relative), consecutive lines joined into ranges, in file and line order.
fn missed(lcov: &str, prefix: &str) -> Vec<String> {
    fn ranges(path: &str, lines: &[u32], what: &str, out: &mut Vec<String>) {
        let mut lines = lines.to_vec();
        lines.sort_unstable();
        lines.dedup();
        let mut i = 0;
        // bound: `i` grows by at least one per pass and `lines` is finite.
        while let Some(&first) = lines.get(i) {
            let mut last = first;
            i += 1;
            // bound: `i` grows by one per pass and stops at the end of `lines`.
            while lines.get(i) == Some(&(last + 1)) {
                last += 1;
                i += 1;
            }
            out.push(if last == first {
                format!("{path}:{first} ({what})")
            } else {
                format!("{path}:{first}-{last} ({what})")
            });
        }
    }
    let mut out = Vec::new();
    let mut file: Option<String> = None;
    let mut lines: Vec<u32> = Vec::new();
    let mut branches: Vec<u32> = Vec::new();
    for rec in lcov.lines() {
        if let Some(sf) = rec.strip_prefix("SF:") {
            let name = repo::rel(std::path::Path::new(sf));
            file = name.starts_with(prefix).then_some(name);
        } else if rec == "end_of_record" {
            if let Some(path) = file.take() {
                ranges(&path, &lines, "lines", &mut out);
                ranges(&path, &branches, "branch", &mut out);
            }
            lines.clear();
            branches.clear();
        } else if file.is_some() {
            if let Some(da) = rec.strip_prefix("DA:") {
                let mut f = da.split(',');
                if let (Some(n), Some("0")) = (f.next(), f.next()) {
                    lines.extend(n.parse::<u32>());
                }
            } else if let Some(br) = rec.strip_prefix("BRDA:") {
                // BRDA:<line>,<block>,<branch>,<taken>; `-` is a branch never reached.
                let f: Vec<&str> = br.split(',').collect();
                if let (Some(n), Some(&("-" | "0"))) = (f.first(), f.get(3)) {
                    branches.extend(n.parse::<u32>());
                }
            }
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::{missed, packages, select};

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_change_selects_its_own_crates() {
        // req: VER-068
        let gated = ["aldis-core", "aldis-spp", "aldis-tm"];
        let (g, r) = select(
            &names(&["aldis-spp", "aldis-testkit"]),
            &gated,
            &["aldis-ffi"],
        );
        assert_eq!((g, r), (vec!["aldis-spp"], vec![]));
        let (g, r) = select(&names(&["aldis-ffi"]), &gated, &["aldis-ffi"]);
        assert_eq!((g, r), (vec![], vec!["aldis-ffi"]));
        let (g, r) = select(&names(&["aldis-testkit"]), &gated, &["aldis-ffi"]);
        assert_eq!((g, r), (vec![], vec![]));
    }

    #[test]
    fn aldis_core_selects_every_crate() {
        // req: VER-068
        let gated = ["aldis-core", "aldis-spp", "aldis-tm"];
        let (g, r) = select(&names(&["aldis-core"]), &gated, &["aldis-ffi"]);
        assert_eq!((g, r), (gated.to_vec(), vec!["aldis-ffi"]));
    }

    #[test]
    fn packages_are_named_with_dash_p() {
        // req: VER-070
        assert_eq!(packages(&[]), Ok(vec![]));
        assert_eq!(
            packages(&["-p", "aldis-spp", "-p", "aldis-tm"]),
            Ok(vec!["aldis-spp", "aldis-tm"])
        );
        assert!(packages(&["-p"]).is_err());
        assert!(packages(&["aldis-spp"]).is_err());
        assert!(packages(&["-p", "aldis-spp", "--all"]).is_err());
    }

    #[test]
    fn missed_lines_and_branches_print_as_ranges() {
        // req: VER-070
        let lcov = "\
SF:crates/aldis-spp/src/view.rs
DA:10,3
DA:11,0
DA:12,0
DA:13,0
DA:20,0
DA:21,5
BRDA:30,0,0,2
BRDA:30,0,1,-
BRDA:31,0,0,0
end_of_record
SF:crates/aldis-tm/src/view.rs
DA:1,0
end_of_record
SF:crates/aldis-spp/src/secondary.rs
DA:5,1
end_of_record
";
        assert_eq!(
            missed(lcov, "crates/aldis-spp/"),
            [
                "crates/aldis-spp/src/view.rs:11-13 (lines)",
                "crates/aldis-spp/src/view.rs:20 (lines)",
                "crates/aldis-spp/src/view.rs:30-31 (branch)",
            ]
        );
        assert_eq!(missed(lcov, "crates/aldis-core/"), <[String; 0]>::default());
    }
}
