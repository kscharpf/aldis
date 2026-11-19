//! `xtask features`: the feature sets of the crates build and pass their tests (test hygiene, no
//! requirement), and the facade's codec features reach the engines (GEN-008).
//!
//! For every crate in [`crates`] the command runs the crate's tests, among them the shared vector
//! suite run by the exact-match runner of `aldis-testkit` (TV-012), with `--no-default-features`
//! on each base feature set in [`crates`]: no features, and a deliberate extra set where a
//! crate has an optional feature the empty set leaves out. Every build runs under `-D warnings`,
//! so a warning of a subset, or a `cfg_attr(…, expect(…))` of a rustc lint that no longer holds,
//! fails the check; the `clippy::` expectations of a subset are not checked here, because the
//! check runs `cargo test`, not clippy.
//! `aldis-link` has the subsets of its packet codecs (GEN-009): a frame protocol alone (`tm`,
//! `tc`, `aos`, `uslp`), with either packet codec (`tm,spp`, `tm,epp`), and USLP with either (`uslp,spp`,
//! `uslp,epp`; USLP's packet generator and extractor need `epp`).
//! `aldis-ffi` has the subsets of [`FFI_SUBSETS`] (GEN-009): the library is built and tested with the
//! features that the header's functions are grouped by, and each test runs only in the subsets
//! that have what it calls (`required-features`); the header stays whole, and
//! `tests/storage_layout.rs` runs in every subset and checks that every storage struct keeps the
//! size of the full build. Its `[features]` must also offer every feature of the facade, each
//! forwarding to `aldis/<x>` ([`ffi_feature_gaps`]).
//!
//! A second check covers the facade (GEN-008): `aldis` built and tested with exactly
//! `link,cop1,tm,tc,spp` and no default features, under the gate's `-D warnings`, must run the
//! test of `crates/aldis/tests/engine_features.rs`, so the codec features reach the engines
//! without `full`.

use std::fs;
use std::process::Command;

use toml_edit::DocumentMut;

use crate::repo;

/// The feature sets of `aldis-ffi` that are built and tested (GEN-009), as `--no-default-features
/// --features <set>`: the maintainer's matrix, each with `std`, because a `no_std` build runs
/// no test (the embedded build of the library is the `no-std-<target>` checks, with `full`).
/// The first has the frame engines of TM and TC and the packet codec `spp` (no AOS, USLP, or
/// `epp`); the second is the whole library with the embedded feature.
const FFI_SUBSETS: &[&str] = &["std,core,spp,tm,tc,link,cop1", "std,full,panic-handler"];

/// A test of `aldis-ffi` that runs in every subset (`tests/storage_layout.rs`).
const FFI_STORAGE_TEST: &str = "oid_generator";

/// Base feature sets a crate's tests run on besides the empty one (a deliberate choice).
const EXTRA_BASES: &[(&str, &str)] = &[
    ("aldis-spp", "time"),
    ("aldis-link", "tm"),
    ("aldis-link", "tc"),
    ("aldis-link", "aos"),
    ("aldis-link", "uslp"),
    ("aldis-link", "tm,spp"),
    ("aldis-link", "tm,epp"),
    ("aldis-link", "uslp,spp"),
    ("aldis-link", "uslp,epp"),
];

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
        .env("RUSTFLAGS", "-D warnings")
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
    problems += check_ffi_feature_names()?;
    for set in FFI_SUBSETS {
        problems += check_ffi_subset(set)?;
    }
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

/// Builds and tests `aldis-ffi` with exactly `set` under `-D warnings` (GEN-009); the number of
/// problems. The test of [`FFI_STORAGE_TEST`] must have run, so a subset that runs no test of the
/// library fails.
fn check_ffi_subset(set: &str) -> Result<usize, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args([
            "test",
            "-p",
            "aldis-ffi",
            "--no-default-features",
            "--features",
        ])
        .arg(set)
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("cargo test: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ran = passed_tests(&stdout)
        .iter()
        .any(|t| t.ends_with(FFI_STORAGE_TEST));
    if out.status.success() && ran {
        println!("features aldis-ffi: the library builds and its tests pass with `{set}`");
        return Ok(0);
    }
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    println!(
        "error: GEN-009 aldis-ffi: the build or the tests fail with `{set}` (no default features, \
         -D warnings), or `{FFI_STORAGE_TEST}` of tests/storage_layout.rs did not run"
    );
    Ok(1)
}

/// Facade features that `aldis-ffi` need not offer, each with its reason (none: every feature of the
/// facade has C functions or is the `full` set).
const FFI_EXCLUDED_FEATURES: &[(&str, &str)] = &[];

/// The `[features]` table of a manifest as `(name, members)`; an error when the text is not a
/// manifest.
fn feature_table(manifest: &str, who: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let doc = manifest
        .parse::<DocumentMut>()
        .map_err(|e| format!("{who}: Cargo.toml does not parse: {e}"))?;
    let table = doc
        .get("features")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(|| format!("{who}: Cargo.toml has no [features] table"))?;
    Ok(table
        .iter()
        .map(|(name, item)| {
            let members = item.as_array().map_or_else(Vec::new, |a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            });
            (name.to_owned(), members)
        })
        .collect())
}

/// The ways the features of `aldis-ffi` (manifest text `ffi`) fall short of those of the facade
/// (`facade`), one message each, naming the feature (GEN-009: `aldis-ffi` offers the features of
/// the facade). Every facade feature, but the ones of [`FFI_EXCLUDED_FEATURES`], is an `aldis-ffi`
/// feature that forwards to `aldis/<x>`; `full`, the set, instead names every other facade feature
/// that is not excluded.
fn ffi_feature_gaps(facade: &str, ffi: &str) -> Result<Vec<String>, String> {
    let facade = feature_table(facade, "aldis")?;
    let ffi = feature_table(ffi, "aldis-ffi")?;
    let mut gaps = Vec::new();
    for (name, _) in &facade {
        if FFI_EXCLUDED_FEATURES.iter().any(|(x, _)| x == name) {
            continue;
        }
        let Some((_, members)) = ffi.iter().find(|(n, _)| n == name) else {
            gaps.push(format!(
                "aldis-ffi has no feature `{name}`, which the facade has"
            ));
            continue;
        };
        if name == "full" {
            for (other, _) in &facade {
                let excluded = FFI_EXCLUDED_FEATURES.iter().any(|(x, _)| x == other);
                if other != "full" && !excluded && !members.contains(other) {
                    gaps.push(format!("aldis-ffi feature `full` does not name `{other}`"));
                }
            }
        } else if !members.iter().any(|m| *m == format!("aldis/{name}")) {
            gaps.push(format!(
                "aldis-ffi feature `{name}` does not forward to `aldis/{name}`"
            ));
        }
    }
    Ok(gaps)
}

/// Reads both manifests and reports the gaps of [`ffi_feature_gaps`]; the number of problems.
fn check_ffi_feature_names() -> Result<usize, String> {
    let read = |path: &str| {
        fs::read_to_string(repo::root().join(path)).map_err(|e| format!("{path}: {e}"))
    };
    let gaps = ffi_feature_gaps(
        &read("crates/aldis/Cargo.toml")?,
        &read("crates/aldis-ffi/Cargo.toml")?,
    )?;
    for gap in &gaps {
        println!("error: GEN-009 {gap}");
    }
    if gaps.is_empty() {
        println!("features aldis-ffi: offers every feature of the facade");
    }
    Ok(gaps.len())
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
    use super::{ffi_feature_gaps, passed_tests};

    const FACADE: &str =
        "[features]\ncore = [\"dep:a\"]\ntm = [\"dep:b\"]\nfull = [\"core\", \"tm\"]\n";

    #[test]
    fn matching_features_have_no_gaps() {
        // req: GEN-009
        let ffi = "[features]\ncore = [\"aldis/core\"]\ntm = [\"core\", \"aldis/tm\"]\n\
                   full = [\"core\", \"tm\"]\npanic-handler = []\n";
        assert_eq!(ffi_feature_gaps(FACADE, ffi).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_missing_or_unforwarded_feature_is_named() {
        // req: GEN-009
        let missing = "[features]\ncore = [\"aldis/core\"]\nfull = [\"core\", \"tm\"]\n";
        let gaps = ffi_feature_gaps(FACADE, missing).unwrap();
        assert_eq!(
            gaps,
            ["aldis-ffi has no feature `tm`, which the facade has"]
        );
        let unforwarded = "[features]\ncore = [\"aldis/core\"]\ntm = [\"core\"]\n\
                           full = [\"core\", \"tm\"]\n";
        let gaps = ffi_feature_gaps(FACADE, unforwarded).unwrap();
        assert_eq!(
            gaps,
            ["aldis-ffi feature `tm` does not forward to `aldis/tm`"]
        );
        let short_full = "[features]\ncore = [\"aldis/core\"]\ntm = [\"aldis/tm\"]\n\
                          full = [\"core\"]\n";
        let gaps = ffi_feature_gaps(FACADE, short_full).unwrap();
        assert_eq!(gaps, ["aldis-ffi feature `full` does not name `tm`"]);
    }

    #[test]
    fn a_manifest_without_features_is_an_error() {
        // req: GEN-009
        assert!(ffi_feature_gaps(FACADE, "[package]\n").is_err());
    }

    #[test]
    fn passing_tests_are_collected() {
        // req: GEN-008
        let out = "running 2 tests\ntest b ... ok\ntest a::x ... ok\ntest c ... FAILED\n\n\
                   test result: ok. 2 passed\n";
        assert_eq!(passed_tests(out), ["a::x", "b"]);
    }
}
