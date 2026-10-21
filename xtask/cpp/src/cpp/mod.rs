//! Repository checks for the native C++ implementation, run as `cargo xtask cpp <check>`
//! (docs/milestones/c0-foundations.md D12). Building and testing the C++ code needs only `CMake`,
//! Ninja, and `CTest`; these checks parse tool output and live beside their Rust counterparts.
//!
//! | Check | Module | Requirements |
//! |---|---|---|
//! | `layers [<build-dir>]` | [`layers`] | CPP-010 (GEN-002 exceptions apply) |
//! | `link-check <elf>` | [`binary`] | CPP-050 (D6) |
//! | `statics <elf>` | [`binary`] | CPP-030 |
//! | `unsafe-optouts` | [`optouts`] | CPP-031, CPP-042 (D9) |
//!
//! `coverage` (D14), `stack-usage` (D15), and `deviations` (D13) join with their work packages.

pub mod binary;
pub mod layers;
pub mod optouts;

/// Usage of `cargo xtask cpp`, appended to the main help text.
pub const HELP: &str = "\
  cpp layers [<build-dir>]     CMake target graph against the layer rules (CPP-010); the build
                               directory must be configured (default target/cpp/host-gcc)
  cpp link-check <elf>         no allocation, exception, abort, assert, guard, or unwinder
                               symbol in a linked exerciser (CPP-050); nm: $ALDIS_NM or llvm-nm
  cpp statics <elf>            no writable aldis:: data in a linked exerciser (CPP-030)
  cpp unsafe-optouts           bounds-check opt-outs only in aldis/core/bytes.hpp, and every
                               NOLINT names its check (CPP-031, CPP-042)
";

/// Runs `cargo xtask cpp <args>`.
pub fn run(args: &[&str]) -> Result<usize, String> {
    match args {
        ["layers"] => layers::check("target/cpp/host-gcc"),
        ["layers", dir] => layers::check(dir),
        ["link-check", elf] => binary::link_check(elf),
        ["statics", elf] => binary::statics(elf),
        ["unsafe-optouts"] => optouts::check(),
        _ => Err(format!(
            "unknown command: cpp {}\n\nusage:\n{HELP}",
            args.join(" ")
        )),
    }
}
