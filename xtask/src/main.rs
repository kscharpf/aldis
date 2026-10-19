//! Project tooling for Aldis (docs/milestones/m0-foundations.md WP-2).
//!
//! Run with `cargo xtask <command>`; see `cargo xtask help`.

mod binaries;
mod changelog;
mod coverage;
mod cpp;
mod layers;
mod links;
mod md;
mod repo;
mod spec;
mod trace;
mod vectors;

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
  links                        relative Markdown links resolve inside the repository
  layers                       crate dependency rules (GEN-001..004)
  symbols                      no frame-protocol code in a build of aldis-spp alone (VER-031)
  stack-sizes <target>         per-function stack usage of the exerciser (VER-011; tooling nightly)
  coverage                     100 % region/function/line/branch gate for aldis-core (VER-024;
                               tooling nightly + cargo-llvm-cov >= 0.9)
  changelog check              every fragment in changelog.d/ is well formed (GEN-080)
  changelog show               print the unreleased changes as the next release will list them
  changelog release <version> <YYYY-MM-DD>
                               move every fragment into a new section of CHANGELOG.md
  ci                           everything CI runs without a C++ build: spec-lint, links,
                               layers, vectors validate, vectors schema --check, trace --scope
                               m0 --strict, trace --scope c0, changelog check, cpp
                               unsafe-optouts

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
        ["links"] => links::check(),
        ["layers"] => layers::check(),
        ["symbols"] => binaries::symbols(),
        ["stack-sizes", target] => binaries::stack_sizes(target),
        ["coverage"] => coverage::run(),
        ["cpp", rest @ ..] => cpp::run(rest),
        ["changelog", "check"] => changelog::check(),
        ["changelog", "show"] => changelog::show(),
        ["changelog", "release", version, date] => changelog::release(version, date),
        ["ci"] => {
            let mut failed = 0;
            for step in [
                spec::lint as fn() -> Result<usize, String>,
                links::check,
                layers::check,
                vectors::validate,
                || vectors::schema(true),
                || trace::run(Some("m0"), true),
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
            print!("{HELP}{}", cpp::HELP);
            Ok(0)
        }
        _ => Err(format!(
            "unknown command: {}\n\n{HELP}{}",
            args.join(" "),
            cpp::HELP
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
