//! Project tooling for Aldis (docs/milestones/m0-foundations.md WP-2).
//!
//! Run with `cargo xtask <command>`; see `cargo xtask help`.

mod binaries;
mod changelog;
mod consistency;
mod coverage;
mod cpp;
mod features;
mod fields;
mod layers;
mod links;
mod md;
mod records;
mod refapp;
mod repo;
mod spec;
mod trace;
mod vectors;
mod verify;

use std::process::ExitCode;

const HELP: &str = "\
usage: cargo xtask <command>

commands:
  spec-lint                    check requirement tables and every requirement-ID reference
  trace [--scope <m>] [--strict]
                               requirement -> artefact matrix in target/spec/ (VER-020);
                               --strict fails on in-scope requirements without artefacts
  vectors validate             check test-vectors/ against the model and the spec (TV-*)
  vectors schema [--check]     (re)generate test-vectors/schema/, or check it is current
  fields [--write]             check the field catalogue (03-api-model.md §9.1) and that
                               aldis_core::field and test-vectors/core/fields.json match it,
                               or regenerate both (API-082)
  links                        relative Markdown links resolve inside the repository
  layers                       crate dependency rules (GEN-001..004)
  consistency                  the registrations agree: crates with code in coverage, features,
                               mutants, the exerciser, layers, READMEs, and the facade; fuzz
                               targets in fuzz/Cargo.toml and the nightly matrix; every vector
                               file in test-vectors/README.md, a test, and the exerciser
  refapp-deps                  refapp/ uses Aldis only through the `aldis` facade and
                               forbids unsafe code (APP-100, APP-105)
  symbols                      no frame-protocol code in a build of aldis-spp alone (VER-031)
  stack-sizes <target>         per-function stack usage of the exerciser (VER-011; tooling nightly)
  features                     `alloc` and `std` are additive: no API gated on them, and every
                               test passes alike without them, with `alloc`, and with `std`
                               (GEN-006)
  coverage                     100 % region/function/line/branch gate for aldis-core (VER-024;
                               tooling nightly + cargo-llvm-cov >= 0.9)
  changelog check              every fragment in changelog.d/ is well formed (GEN-080)
  changelog show               print the unreleased changes as the next release will list them
  changelog release <version> <YYYY-MM-DD>
                               move every fragment into a new section of CHANGELOG.md
  ci                           everything CI runs without a C++ build: spec-lint, links,
                               layers, consistency, refapp-deps, vectors validate, vectors schema --check,
                               fields, trace --scope m0 --strict, trace --scope m1 --strict,
                               trace --scope m2, trace --scope c0, changelog check,
                               cpp unsafe-optouts

C++ checks (docs/milestones/c0-foundations.md D12):
";

fn run(args: &[&str]) -> Result<usize, String> {
    match args {
        ["spec-lint"] => spec::lint(),
        ["trace", rest @ ..] => {
            let scope = rest
                .iter()
                .position(|a| *a == "--scope")
                .and_then(|i| rest.get(i + 1))
                .copied();
            trace::run(scope, rest.contains(&"--strict"))
        }
        ["vectors", "validate"] => vectors::validate(),
        ["vectors", "schema"] => vectors::schema(false),
        ["vectors", "schema", "--check"] => vectors::schema(true),
        ["fields"] => fields::run(false),
        ["fields", "--write"] => fields::run(true),
        ["links"] => links::check(),
        ["layers"] => layers::check(),
        ["refapp-deps"] => refapp::check(),
        ["symbols"] => binaries::symbols(),
        ["stack-sizes", target] => binaries::stack_sizes(target),
        ["coverage"] => coverage::run(),
        ["features"] => features::check(),
        ["consistency"] => consistency::check(),
        ["cpp", rest @ ..] => cpp::run(rest),
        ["verify", rest @ ..] => verify::run(rest),
        ["changelog", "check"] => changelog::check(),
        ["changelog", "show"] => changelog::show(),
        ["changelog", "release", version, date] => changelog::release(version, date),
        ["ci"] => {
            let mut failed = 0;
            for step in [
                spec::lint as fn() -> Result<usize, String>,
                links::check,
                layers::check,
                consistency::check,
                refapp::check,
                vectors::validate,
                || vectors::schema(true),
                || fields::run(false),
                // M0 and M1 are closed: every in-scope requirement must keep an artefact.
                || trace::run(Some("m0"), true),
                || trace::run(Some("m1"), true),
                // M2 is open: report its trace without failing (strict at the M2 exit, D1).
                || trace::run(Some("m2"), false),
                // C0 is open: report its trace without failing (strict at the C0 exit, D17).
                || trace::run(Some("c0"), false),
                changelog::check,
                cpp::optouts::check,
            ] {
                failed += step()?;
            }
            Ok(failed)
        }
        [] | ["help" | "-h" | "--help"] => {
            print!("{HELP}{}\n{}", cpp::HELP, verify::HELP);
            Ok(0)
        }
        _ => Err(format!(
            "unknown command: {}\n\n{HELP}{}\n{}",
            args.join(" "),
            cpp::HELP,
            verify::HELP
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => {
            eprintln!("xtask: {n} problem(s)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
