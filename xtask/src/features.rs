//! `xtask features`: the feature sets of the crates build and pass their tests (test hygiene, no
//! requirement), and the facade's codec features reach the engines (GEN-008).
//!
//! For every crate in [`crates`] the command runs the crate's tests, among them the shared vector
//! suite run by the exact-match runner of `aldis-testkit` (TV-012), with `--no-default-features`
//! on each base feature set in [`crates`]: no features, and a deliberate extra set where a
//! crate has an optional feature the empty set leaves out.
//!
//! A second check covers the facade (GEN-008): `aldis` built and tested with exactly
//! `link,cop1,tm,tc,spp` and no default features, under the gate's `-D warnings`, must run the
//! test of `crates/aldis/tests/engine_features.rs`, so the codec features reach the engines
//! without `full`.

use std::process::Command;

use crate::repo;

/// Base feature sets a crate's tests run on besides the empty one (a deliberate choice).
const EXTRA_BASES: &[(&str, &str)] = &[("aldis-spp", "time")];

/// The crates checked, each with the base feature sets its tests run on: the crates with code
/// ([`crate::layers::gated`]), so a crate joins when it gets code.
pub fn crates() -> Result<Vec<(&'static str, Vec<&'static str>)>, String> {
    Ok(crate::layers::gated()?
        .into_iter()
        .map(|c| {
            let mut bases = vec![""];
            bases.extend(EXTRA_BASES.iter().filter(|(n, _)| *n == c).map(|(_, b)| *b));
            (c, bases)
        })
        .collect())
}

/// The names of the passing tests in `cargo test` output, sorted (duplicates kept: a unit test
/// and an integration test may share a name).
fn passed_tests(stdout: &str) -> Vec<String> {
    let mut out: Vec<String> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("test ")?.strip_suffix(" ... ok"))
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

/// Runs the tests of `krate` with exactly `features`; whether they all passed.
fn run_tests(krate: &str, features: &str) -> Result<bool, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = Command::new(cargo);
    cmd.args(["test", "-p", krate, "--no-default-features"]);
    if !features.is_empty() {
        cmd.args(["--features", features]);
    }
    let out = cmd
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("cargo test: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        print!("{stdout}");
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        return Ok(false);
    }
    Ok(true)
}

fn label(features: &str) -> String {
    if features.is_empty() {
        "no features".to_owned()
    } else {
        format!("features `{features}`")
    }
}

/// `xtask features`: runs the checks of the module docs; returns the number of problems.
pub fn check() -> Result<usize, String> {
    let mut problems = 0;
    let crates = crates()?;
    for (krate, bases) in &crates {
        for base in bases {
            problems += check_tests(krate, base)?;
        }
    }
    problems += check_facade_engines()?;
    if problems == 0 {
        println!("features: the tests of {} crates pass", crates.len());
    }
    Ok(problems)
}

/// The features of the facade build that must reach the engines without `full` (GEN-008).
const FACADE_ENGINE_FEATURES: &str = "link,cop1,tm,tc,spp";

/// The test of the facade that names the engines (`crates/aldis/tests/engine_features.rs`).
const FACADE_ENGINE_TEST: &str = "engines_are_nameable_through_the_facade";

/// Builds and tests `aldis` with [`FACADE_ENGINE_FEATURES`] alone under `-D warnings` (GEN-008);
/// the number of problems.
fn check_facade_engines() -> Result<usize, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args(["test", "-p", "aldis", "--no-default-features", "--features"])
        .arg(FACADE_ENGINE_FEATURES)
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("cargo test: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ran = passed_tests(&stdout)
        .iter()
        .any(|t| t.ends_with(FACADE_ENGINE_TEST));
    if out.status.success() && ran {
        println!("features aldis: the engines are reachable with {FACADE_ENGINE_FEATURES}");
        return Ok(0);
    }
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    println!(
        "error: GEN-008 aldis: `{FACADE_ENGINE_TEST}` did not pass with {FACADE_ENGINE_FEATURES} \
         (no default features, -D warnings)"
    );
    Ok(1)
}

/// Runs the tests of `krate` on `base` alone.
fn check_tests(krate: &str, base: &str) -> Result<usize, String> {
    if !run_tests(krate, base)? {
        println!("error: {krate}: tests fail with {}", label(base));
        return Ok(1);
    }
    println!("features {krate}: the tests pass with {}", label(base));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::passed_tests;

    #[test]
    fn passing_tests_are_collected() {
        // req: GEN-008
        let out = "running 2 tests\ntest b ... ok\ntest a::x ... ok\ntest c ... FAILED\n\n\
                   test result: ok. 2 passed\n";
        assert_eq!(passed_tests(out), ["a::x", "b"]);
    }
}
