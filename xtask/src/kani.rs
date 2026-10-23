//! `cargo xtask kani`: runs the Kani proof harnesses and prints each one's result and time
//! (docs/verification/formal-proofs.md, OI-42). A spike: no check of the verification run calls
//! it, and nothing gates on it.
//!
//! The harnesses are `#[kani::proof]` functions in a `#[cfg(kani)] mod proofs` of each crate in
//! [`RUNS`]. Kani is pinned to [`KANI_VERSION`]; another version is refused, since the trusted
//! base of a proof includes the tool (formal-proofs.md §4).

use std::process::Command;
use std::time::Instant;

use crate::repo;

/// The pinned Kani release (`cargo install --locked kani-verifier@0.68.0`, then
/// `cargo kani setup`).
pub const KANI_VERSION: &str = "0.68.0";

/// One `cargo kani` invocation: crate, features, and an exact harness name to restrict it to.
struct Run {
    krate: &'static str,
    features: &'static [&'static str],
    harness: Option<&'static str>,
}

/// Every run, in order. The table CRC is the same function under feature `crc-table`, so only
/// the CRC harness runs again with it (CMN-004).
const RUNS: &[Run] = &[
    Run {
        krate: "aldis-core",
        features: &[],
        harness: None,
    },
    Run {
        krate: "aldis-core",
        features: &["crc-table"],
        harness: Some("proofs::crc16_matches_reference"),
    },
    Run {
        krate: "aldis-spp",
        features: &[],
        harness: None,
    },
    Run {
        krate: "aldis-time",
        features: &[],
        harness: None,
    },
];

/// The result of one harness, as Kani reports it.
#[derive(Debug, PartialEq)]
struct Outcome {
    harness: String,
    verified: bool,
    /// CBMC's "Verification Time", in seconds.
    seconds: Option<f64>,
}

/// The harness outcomes in Kani's output: each `Checking harness <name>...` line followed by its
/// `VERIFICATION:- SUCCESSFUL` or `FAILED` line and its `Verification Time: <s>s` line.
fn parse(output: &str) -> Vec<Outcome> {
    let mut out: Vec<Outcome> = Vec::new();
    for line in output.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Checking harness ") {
            out.push(Outcome {
                harness: rest.trim_end_matches("...").to_owned(),
                verified: false,
                seconds: None,
            });
        } else if let Some(current) = out.last_mut() {
            if line == "VERIFICATION:- SUCCESSFUL" {
                current.verified = true;
            } else if let Some(t) = line.strip_prefix("Verification Time: ") {
                current.seconds = t.trim_end_matches('s').parse().ok();
            }
        }
    }
    out
}

/// The version `cargo kani --version` reports.
fn kani_version() -> Result<String, String> {
    let out = Command::new("cargo")
        .args(["kani", "--version"])
        .output()
        .map_err(|e| format!("cargo kani: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .nth(3)
        .map(str::to_owned)
        .filter(|_| out.status.success())
        .ok_or_else(|| {
            format!(
                "cargo kani not found; install it with `cargo install --locked \
                 kani-verifier@{KANI_VERSION}` and `cargo kani setup`"
            )
        })
}

/// Runs every harness of [`RUNS`]; the problems are the harnesses that failed and the runs that
/// reported no harness.
pub fn run() -> Result<usize, String> {
    let version = kani_version()?;
    if version != KANI_VERSION {
        return Err(format!(
            "Kani {version} found, {KANI_VERSION} pinned; install it with `cargo install \
             --locked kani-verifier@{KANI_VERSION}` and `cargo kani setup`"
        ));
    }
    let mut problems = 0;
    let mut rows = Vec::new();
    for r in RUNS {
        let mut cmd = Command::new("cargo");
        cmd.args(["kani", "-p", r.krate, "--output-format", "terse"])
            .current_dir(repo::root());
        if !r.features.is_empty() {
            cmd.arg("--features").arg(r.features.join(","));
        }
        if let Some(h) = r.harness {
            cmd.args(["--exact", "--harness", h]);
        }
        let label = if r.features.is_empty() {
            r.krate.to_owned()
        } else {
            format!("{} +{}", r.krate, r.features.join(","))
        };
        println!("kani: {label}");
        let start = Instant::now();
        let out = cmd.output().map_err(|e| format!("cargo kani: {e}"))?;
        let wall = start.elapsed().as_secs_f64();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let outcomes = parse(&stdout);
        if outcomes.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            println!("error: {label}: no harness reported (exit {})", out.status);
            problems += 1;
        }
        for o in &outcomes {
            if !o.verified {
                problems += 1;
            }
            rows.push((label.clone(), o.harness.clone(), o.verified, o.seconds));
        }
        println!("kani: {label}: {wall:.1} s wall-clock, including the build");
    }
    println!("\n| Crate | Harness | Result | CBMC time |\n|---|---|---|---:|");
    for (label, harness, verified, seconds) in rows {
        let result = if verified { "verified" } else { "FAILED" };
        let time = seconds.map_or_else(|| "?".to_owned(), |s| format!("{s:.1} s"));
        println!("| {label} | `{harness}` | {result} | {time} |");
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::{KANI_VERSION, Outcome, parse};

    #[test]
    fn outcomes_are_read_from_the_terse_output() {
        let text = "Checking harness proofs::a...\n\
                    VERIFICATION:- SUCCESSFUL\n\
                    Verification Time: 1.25s\n\n\
                    Checking harness proofs::b...\n\
                    Failed Checks: assertion failed\n\
                    VERIFICATION:- FAILED\n\
                    Verification Time: 0.5s\n";
        assert_eq!(
            parse(text),
            [
                Outcome {
                    harness: "proofs::a".to_owned(),
                    verified: true,
                    seconds: Some(1.25),
                },
                Outcome {
                    harness: "proofs::b".to_owned(),
                    verified: false,
                    seconds: Some(0.5),
                },
            ]
        );
        assert_eq!(parse("error: could not compile"), []);
    }

    #[test]
    fn the_spike_documents_the_pinned_version() {
        let doc =
            crate::repo::read(&crate::repo::root().join("docs/verification/formal-proofs.md"))
                .unwrap();
        assert!(doc.contains(&format!("Kani {KANI_VERSION}")));
    }
}
