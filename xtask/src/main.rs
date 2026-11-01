//! Project tooling for Aldis (docs/milestones/m0-foundations.md WP-2).
//!
//! Run with `cargo xtask <command>`; see `cargo xtask help`.

mod api;
mod assurance;
mod binaries;
mod changelog;
mod check;
mod check_lock;
mod consistency;
mod coverage;
mod features;
mod fields;
mod fuzz;
mod header;
mod kani;
mod layers;
mod links;
mod loops;
mod md;
mod metrics;
mod models;
mod records;
mod refapp;
mod repo;
mod restricted;
mod rs020;
mod sana;
mod spec;
mod stack;
mod status;
mod trace;
mod vectors;
mod verify;
mod wp_status;

use std::process::ExitCode;

const HELP: &str = "\
usage: cargo xtask <command>

commands:
  spec-lint                    check requirement tables and every requirement-ID reference
  trace [--scope <m>] [--strict]
                               requirement -> artefact matrix in target/spec/ (VER-020);
                               --strict fails on in-scope requirements without artefacts
  vectors validate             check test-vectors/ against the model and the spec (TV-*), and run
                               the independent model each provenance_note names
                               (python3 -I test-vectors/models/<m>.py --check, TV-023)
  vectors generated [--write]  check that test-vectors/cop1/ is what the independent model writes
                               (python3 -I test-vectors/models/cop1.py --write), or rewrite it
  vectors schema [--check]     (re)generate test-vectors/schema/, or check it is current
  fields [--write]             check the field catalogue (03-api-model.md §9.1) and that
                               aldis_core::field and test-vectors/core/fields.json match it,
                               or regenerate both (API-082)
  status [--write]             check the error catalogue (03-api-model.md §9) and that the
                               aldis-ffi status codes match it, or regenerate them (CABI-020)
  header [--write]             check that include/aldis/aldis.h and the aldis-ffi capacities
                               match a fresh generation with the pinned cbindgen, or
                               regenerate them (CABI-002, CABI-003, CABI-030)
  header c-smoke <cc> <c++>    build the aldis-ffi static and shared libraries and run a C
                               program with them, as C99 and C++11 (CABI-001, CABI-003)
  sana [--write]               the SANA registry snapshots in specs/sana/ match their SHA-256
                               and the tables generated from them, or regenerate the tables
                               (M2 plan D13, OI-06)
  sana import <registry>       download a SANA registry, write its snapshot and its table
  sana check-live              fail when a SANA registry differs from its snapshot (nightly)
  links                        relative Markdown links resolve inside the repository
  fix                          run the generators (fields, status, vectors schema, sana, header),
                               then consistency
  layers                       crate dependency rules (GEN-001..004)
  consistency                  the registrations agree: crates with code in coverage, features,
                               mutants, the exerciser, layers, READMEs, and the facade; fuzz
                               targets in fuzz/Cargo.toml and the nightly matrix; every vector
                               file in test-vectors/README.md, a test, and the exerciser;
                               every aldis_ or ALDIS_ name of docs/profiles/c-abi.md and the
                               ffi README in include/aldis/aldis.h (CABI-002)
  loops [list [<crate>...]]    every while, while let, and loop in a library crate, and every
                               iteration over an open range or endless source, has a `// bound:`
                               comment above it, and every such comment is above a loop
                               (GEN-022); `list` prints the loops found
  assurance [--strict]         every reference of the hazard analysis and the assurance case
                               resolves, both follow their formats, and every hazard supports
                               a claim; --strict also fails on anything left as TODO
                               (VER-065, VER-066)
  rs020                        no bare integer in a public field or enum payload of a codec
                               crate unless xtask/rs020-exemptions.toml exempts it with a
                               reason, and no exemption that matches nothing (RS-020)
  restricted [list]            every restricted type of a codec crate (a checked constructor)
                               has a property test with a `// restricted: <Type>` marker, and
                               every marker names one; `list` prints the types (RS-020, VER-023)
  refapp-deps                  refapp/ uses Aldis only through the `aldis` facade and
                               forbids unsafe code (APP-100, APP-105)
  symbols                      no frame-protocol code in a build of aldis-spp alone (VER-031)
  ffi-imports <target>         the bare-metal aldis-ffi static library needs nothing from the
                               integrator beyond memcpy/memset equivalents (CABI-004)
  stack [<target>...]          the worst-case stack of every entry point of the library on each
                               bare-metal target (or those named) within the budgets of
                               xtask/stack-budgets.toml, the call graph complete; writes
                               target/spec/stack-<target>.md (VER-011, GEN-022; tooling nightly)
  features                     `alloc` and `std` are additive: no API gated on them, and every
                               test passes alike without them, with `alloc`, and with `std`
                               (GEN-006)
  coverage [-p <crate>]...     100 % region/function/line/branch gate for the crates of Layers
                               0-2 (VER-024; tooling nightly + cargo-llvm-cov >= 0.9); -p
                               runs only the named crates; a failure lists the missed lines
                               and branches as path:line ranges
  fuzz smoke [<target>...]     run a smoke pass (-runs=1 -max_total_time=2) on all fuzz targets
                               (or those named) to catch broken assertions before merge (VER-022)
  kani                         run the Kani proof harnesses and print each one's result and
                               time; a spike, not a gate (docs/verification/formal-proofs.md,
                               OI-42; needs the pinned Kani)
  changelog                    print the unreleased changes, generated from the titles of the
                               merged pull requests after the base of CHANGELOG.md (GEN-080)
  changelog release <version> <YYYY-MM-DD>
                               write them as a new section of CHANGELOG.md
  status-table [--write]       derive the State and PRs columns of the work-package status
                               tables of docs/milestones/ from the pull requests of GitHub
                               (needs gh; a coordinator tool, no check of the gate)
  check [--quick | --full] [--from <step>]
                               the checks to run while working, in three tiers (VER-068); the
                               gate decides a merge, not this. --quick: format and clippy
                               (the gate's lints) of the four workspaces, every error at once
                               (edit loop). Default:
                               fmt, clippy, doc, test, the bare-metal clippy build, ci, then the
                               extras of each touched directory; stops at the first failure.
                               --full: the default tier plus coverage of the touched crates,
                               stack, big-endian, and the exerciser's QEMU run (once, before the
                               final commit). The last
                               line names the gate checks the tier did not run. One run at a
                               time per worktree; --from <step> skips the steps before the
                               named one and ends `partial`, which is not a pass
  ci                           the quick checks of the verification run, without a build:
                               spec-lint, links, layers, consistency, rs020, loops,
                               restricted, assurance --strict, refapp-deps, vectors validate,
                               vectors generated, vectors schema --check, fields, header (needs the pinned
                               cbindgen), sana, trace --scope m0 --strict,
                               trace --scope m1 --strict, trace --scope m2 --strict,
                               trace --scope m3 --strict; a step that cannot run
                               counts as a problem, and the others still run

";

/// One step of `cargo xtask ci`: its name, as `cargo xtask help` lists it, and the check.
type Step = (&'static str, fn() -> Result<usize, String>);

/// The steps of `cargo xtask ci`, in order: the checks of the verification run that need no
/// build, so that a stale generated file fails here, in seconds, before any verification run.
/// The generated files are checked by `fields` (the field codes of `aldis-core`, the vectors,
/// and `aldis-ffi`, API-082, CABI-021), `status` (the status codes of `aldis-ffi`, CABI-020) and
/// `header` (the C header and the `aldis-ffi`
/// capacities, CABI-002, CABI-030), which needs the pinned `cbindgen`.
pub(crate) const CI: &[Step] = &[
    ("spec-lint", spec::lint),
    ("links", links::check),
    ("layers", layers::check),
    ("consistency", consistency::check),
    ("rs020", rs020::check),
    ("loops", || loops::run(&[])),
    ("restricted", || restricted::run(false)),
    ("assurance --strict", || assurance::run(true)),
    ("refapp-deps", refapp::check),
    ("vectors validate", vectors::validate),
    ("vectors generated", || vectors::generated(false)),
    ("vectors schema --check", || vectors::schema(true)),
    ("fields", || fields::run(false)),
    ("status", || status::run(false)),
    ("header", || header::run(false)),
    ("sana", || sana::run(&[])),
    // M0 and M1 are closed: every in-scope requirement must keep an artefact.
    ("trace --scope m0 --strict", || trace::run(Some("m0"), true)),
    ("trace --scope m1 --strict", || trace::run(Some("m1"), true)),
    // M2 is closed: every in-scope requirement must keep an artefact (D1).
    ("trace --scope m2 --strict", || trace::run(Some("m2"), true)),
    // M3 is closed: every in-scope requirement must keep an artefact (D1).
    ("trace --scope m3 --strict", || trace::run(Some("m3"), true)),
];

/// Runs `steps`; returns the number of problems. A step that cannot run (a missing tool, an
/// unreadable file) counts as one problem and is reported, and the other steps still run.
fn run_steps(steps: &[Step]) -> usize {
    let mut failed = 0;
    for (name, step) in steps {
        match step() {
            Ok(problems) => failed += problems,
            Err(e) => {
                println!("ci: {name}: {e}");
                failed += 1;
            }
        }
    }
    failed
}

/// `xtask fix`: the generators, then `consistency`, which reports what is still missing.
fn fix() -> Result<usize, String> {
    Ok(fields::run(true)?
        + status::run(true)?
        + vectors::schema(false)?
        + vectors::generated(true)?
        + sana::run(&["--write"])?
        + header::run(true)?
        + consistency::check()?)
}

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
        ["vectors", "generated"] => vectors::generated(false),
        ["vectors", "generated", "--write"] => vectors::generated(true),
        ["vectors", "schema"] => vectors::schema(false),
        ["vectors", "schema", "--check"] => vectors::schema(true),
        ["fields"] => fields::run(false),
        ["fields", "--write"] => fields::run(true),
        ["status"] => status::run(false),
        ["status", "--write"] => status::run(true),
        ["header"] => header::run(false),
        ["header", "--write"] => header::run(true),
        ["header", "c-smoke", cc, cxx] => header::c_smoke(cc, cxx),
        ["sana", rest @ ..] => sana::run(rest),
        ["fix"] => fix(),
        ["links"] => links::check(),
        ["layers"] => layers::check(),
        ["refapp-deps"] => refapp::check(),
        ["symbols"] => binaries::symbols(),
        ["ffi-imports", target] => binaries::ffi_imports(target),
        ["stack", rest @ ..] => stack::run(rest),
        ["coverage", rest @ ..] => coverage::main(rest),
        ["fuzz", "smoke", rest @ ..] => fuzz::smoke(rest),
        ["kani"] => kani::run(),
        ["features"] => features::check(),
        ["consistency"] => consistency::check(),
        ["rs020"] => rs020::check(),
        ["restricted"] => restricted::run(false),
        ["restricted", "list"] => restricted::run(true),
        ["loops", rest @ ..] => loops::run(rest),
        ["assurance"] => assurance::run(false),
        ["assurance", "--strict"] => assurance::run(true),
        ["verify", rest @ ..] => verify::run(rest),
        ["metrics", rest @ ..] => metrics::run(rest),
        ["changelog"] => changelog::show(),
        ["changelog", "release", version, date] => changelog::release(version, date),
        ["status-table"] => wp_status::run(false),
        ["status-table", "--write"] => wp_status::run(true),
        ["ci"] => Ok(run_steps(CI)),
        ["check", rest @ ..] => check::run(rest),
        [] | ["help" | "-h" | "--help"] => {
            print!("{HELP}{}\n{}", verify::HELP, metrics::HELP);
            Ok(0)
        }
        _ => Err(format!(
            "unknown command: {}\n\n{HELP}{}\n{}",
            args.join(" "),
            verify::HELP,
            metrics::HELP
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

#[cfg(test)]
mod tests {
    use super::*;

    // req: API-082, CABI-002, CABI-021
    #[test]
    fn ci_checks_the_generated_files() {
        let names: Vec<&str> = CI.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"fields"), "{names:?}");
        assert!(names.contains(&"status"), "{names:?}");
        assert!(names.contains(&"header"), "{names:?}");
        // The help lists every step.
        let help: String = HELP.split_whitespace().collect::<Vec<_>>().join(" ");
        for name in names {
            assert!(help.contains(name), "{name}");
        }
        // `fields` covers the field-code constants of the C ABI.
        assert_eq!(fields::FFI, "crates/aldis-ffi/src/field.rs");
        // `status` covers the status codes of the C ABI.
        assert_eq!(status::FFI, "crates/aldis-ffi/src/status.rs");
    }

    // req: CABI-002
    #[test]
    fn a_step_that_fails_or_cannot_run_fails_ci_and_the_others_still_run() {
        static RAN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let steps: &[Step] = &[
            ("stale", || {
                RAN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(1)
            }),
            ("no tool", || {
                RAN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("cbindgen: not found".to_owned())
            }),
            ("fine", || {
                RAN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(0)
            }),
        ];
        assert_eq!(run_steps(steps), 2);
        assert_eq!(RAN.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
}
