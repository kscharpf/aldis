//! `xtask features`: the `alloc` and `std` features are strictly additive (GEN-006).
//!
//! For every crate in [`crates`] the command checks three things:
//!
//! 1. **Sources.** `feature = "alloc"` and `feature = "std"` appear in `src/` only as
//!    `#[cfg(feature = "alloc")]` directly above `extern crate alloc;` (and the same for `std`).
//!    No item, signature, trait impl, or code path is gated on either feature, so enabling one
//!    cannot change an existing API.
//! 2. **Manifest.** The `alloc` and `std` features enable nothing but the same features of
//!    crates in [`crates`] (and `std` enables `alloc`), read with `cargo metadata`. They cannot
//!    switch on another feature, such as `crc-table`, that changes behavior.
//! 3. **Tests.** The crate's tests, among them the shared vector suite run by the exact-match
//!    runner of `aldis-testkit` (TV-012), pass with `--no-default-features`, with `alloc`, and
//!    with `std`, on each base feature set in [`crates`]; the `alloc` and `std` runs pass the
//!    same tests as the run without them. Every vector output is compared with the expected
//!    octets, so the wire output is the same in each configuration.

use std::collections::BTreeMap;
use std::process::Command;

use serde_json::Value;

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

/// The features GEN-006 requires to be additive.
const ADDITIVE: &[&str] = &["alloc", "std"];

/// Problems in one source file: `cfg` uses of `alloc` or `std` other than the crate-root
/// `extern crate` declarations, as `(1-based line, text)`.
fn source_problems(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let squeezed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        for feature in ADDITIVE {
            if !squeezed.contains(&format!("feature=\"{feature}\"")) {
                continue;
            }
            let next = lines
                .iter()
                .skip(i + 1)
                .map(|l| l.trim())
                .find(|l| !l.is_empty());
            let allowed = squeezed == format!("#[cfg(feature=\"{feature}\")]")
                && next == Some(format!("extern crate {feature};").as_str());
            if !allowed {
                out.push((i + 1, line.trim().to_owned()));
            }
        }
    }
    out
}

/// Problems in the `features` table of one package from `cargo metadata`.
fn manifest_problems(
    name: &str,
    features: &BTreeMap<String, Vec<String>>,
    checked_crates: &[&str],
) -> Vec<String> {
    let checked = |dep: &str| checked_crates.contains(&dep);
    let mut out = Vec::new();
    for feature in ADDITIVE {
        let Some(enables) = features.get(*feature) else {
            out.push(format!("{name}: no `{feature}` feature"));
            continue;
        };
        for entry in enables {
            let ok = if *feature == "std" && entry == "alloc" {
                true
            } else {
                entry.split_once('/').is_some_and(|(dep, f)| {
                    let dep = dep.strip_suffix('?').unwrap_or(dep);
                    ADDITIVE.contains(&f) && (*feature == "std" || f == "alloc") && checked(dep)
                })
            };
            if !ok {
                out.push(format!(
                    "{name}: feature `{feature}` enables `{entry}`; it may enable only `alloc` \
                     or `std` of a crate this check covers"
                ));
            }
        }
    }
    out
}

fn metadata_features() -> Result<BTreeMap<String, BTreeMap<String, Vec<String>>>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let mut map = BTreeMap::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let features = p["features"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                let list = v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                (k.clone(), list)
            })
            .collect();
        map.insert(p["name"].as_str().unwrap_or_default().to_owned(), features);
    }
    Ok(map)
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

/// Runs the tests of `krate` with exactly `features`; the passing tests, or `None` on failure.
fn run_tests(krate: &str, features: &str) -> Result<Option<Vec<String>>, String> {
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
        return Ok(None);
    }
    Ok(Some(passed_tests(&stdout)))
}

fn joined(base: &str, extra: &str) -> String {
    if base.is_empty() {
        extra.to_owned()
    } else {
        format!("{base},{extra}")
    }
}

fn label(features: &str) -> String {
    if features.is_empty() {
        "no features".to_owned()
    } else {
        format!("features `{features}`")
    }
}

/// `xtask features`: runs the three checks of the module docs; returns the number of problems.
pub fn check() -> Result<usize, String> {
    let mut problems = 0;
    let metadata = metadata_features()?;
    let crates = crates()?;
    let names: Vec<&str> = crates.iter().map(|(c, _)| *c).collect();
    for (krate, bases) in &crates {
        let src = format!("crates/{krate}/src");
        for path in repo::files(&src, &["rs"]) {
            for (line, text) in source_problems(&repo::read(&path)?) {
                println!(
                    "error: GEN-006 {}:{line}: `{text}`: only `extern crate` may be gated on \
                     `alloc` or `std`",
                    repo::rel(&path)
                );
                problems += 1;
            }
        }
        let features = metadata
            .get(*krate)
            .ok_or_else(|| format!("cargo metadata: no package {krate}"))?;
        for p in manifest_problems(krate, features, &names) {
            println!("error: GEN-006 {p}");
            problems += 1;
        }
        for base in bases {
            problems += check_tests(krate, base)?;
        }
    }
    if problems == 0 {
        println!(
            "features: `alloc` and `std` are additive in {} crates",
            crates.len()
        );
    }
    Ok(problems)
}

/// Runs the tests of `krate` on `base` alone and with each additive feature.
fn check_tests(krate: &str, base: &str) -> Result<usize, String> {
    let Some(reference) = run_tests(krate, base)? else {
        println!("error: GEN-006 {krate}: tests fail with {}", label(base));
        return Ok(1);
    };
    println!(
        "features {krate}: {} tests pass with {}",
        reference.len(),
        label(base)
    );
    let mut problems = 0;
    for extra in ADDITIVE {
        let features = joined(base, extra);
        match run_tests(krate, &features)? {
            None => {
                println!(
                    "error: GEN-006 {krate}: tests fail with {}",
                    label(&features)
                );
                problems += 1;
            }
            Some(passed) if passed != reference => {
                println!(
                    "error: GEN-006 {krate}: {} pass {} tests, {} pass {}",
                    label(base),
                    reference.len(),
                    label(&features),
                    passed.len()
                );
                problems += 1;
            }
            Some(_) => println!(
                "features {krate}: the same tests pass with {}",
                label(&features)
            ),
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{passed_tests, source_problems};

    #[test]
    fn only_extern_crate_may_be_gated() {
        // req: GEN-006
        let ok = "#![no_std]\n#[cfg(feature = \"alloc\")]\nextern crate alloc;\n\
                  #[cfg(feature = \"std\")]\n\nextern crate std;\n";
        assert_eq!(source_problems(ok), []);
        let gated_item = "#[cfg(feature = \"std\")]\nimpl std::error::Error for Error {}\n";
        assert_eq!(source_problems(gated_item).len(), 1);
        let cfg_attr = "#[cfg_attr(feature=\"alloc\", derive(Debug))]\nstruct S;\n";
        assert_eq!(
            source_problems(cfg_attr),
            [(1, cfg_attr.lines().next().unwrap().to_owned())]
        );
        let branch = "fn f() -> u8 {\n    if cfg!(feature = \"alloc\") { 1 } else { 0 }\n}\n";
        assert_eq!(source_problems(branch)[0].0, 2);
        let wrong_crate = "#[cfg(feature = \"alloc\")]\nextern crate std;\n";
        assert_eq!(source_problems(wrong_crate).len(), 1);
        let not = "#[cfg(not(feature = \"std\"))]\nextern crate std;\n";
        assert_eq!(source_problems(not).len(), 1);
    }

    #[test]
    fn features_enable_only_alloc_and_std() {
        // req: GEN-006
        let manifest_problems = |name: &str, f: &BTreeMap<String, Vec<String>>| {
            super::manifest_problems(
                name,
                f,
                &[
                    "aldis-core",
                    "aldis-time",
                    "aldis-epp",
                    "aldis-spp",
                    "aldis-aos",
                    "aldis-tm",
                    "aldis-tc",
                    "aldis-uslp",
                ],
            )
        };
        let table = |alloc: &[&str], std: &[&str]| {
            let own = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect();
            BTreeMap::from([
                ("alloc".to_owned(), own(alloc)),
                ("std".to_owned(), own(std)),
            ])
        };
        let spp = table(
            &["aldis-core/alloc", "aldis-time?/alloc"],
            &["alloc", "aldis-core/std", "aldis-time?/std"],
        );
        assert_eq!(manifest_problems("aldis-spp", &spp), Vec::<String>::new());
        assert_eq!(
            manifest_problems("aldis-core", &table(&[], &["alloc"])),
            Vec::<String>::new()
        );
        let other = table(&["crc-table"], &["alloc"]);
        assert_eq!(manifest_problems("aldis-core", &other).len(), 1);
        let dep = table(&["dep:aldis-time"], &["alloc"]);
        assert_eq!(manifest_problems("aldis-spp", &dep).len(), 1);
        let foreign = table(&["serde/alloc"], &["alloc"]);
        assert_eq!(manifest_problems("aldis-spp", &foreign).len(), 1);
        let alloc_enables_std = table(&["aldis-core/std"], &["alloc"]);
        assert_eq!(manifest_problems("aldis-spp", &alloc_enables_std).len(), 1);
        let dep_feature = table(&["aldis-core/crc-table"], &["alloc"]);
        assert_eq!(manifest_problems("aldis-spp", &dep_feature).len(), 1);
        assert_eq!(manifest_problems("aldis-spp", &BTreeMap::new()).len(), 2);
    }

    #[test]
    fn passing_tests_are_collected() {
        // req: GEN-006
        let out = "running 2 tests\ntest b ... ok\ntest a::x ... ok\ntest c ... FAILED\n\n\
                   test result: ok. 2 passed\n";
        assert_eq!(passed_tests(out), ["a::x", "b"]);
    }
}
