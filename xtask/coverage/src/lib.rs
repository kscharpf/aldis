//! `cargo xtask coverage`: the 100 % coverage gate (VER-024). The verify check `coverage` runs it.

mod coverage;

use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["coverage"],
    help: &[HELP],
    run,
};

const HELP: &str =
    "  coverage                     100 % region/function/line/branch gate for aldis-core (VER-024;
                               tooling nightly + cargo-llvm-cov >= 0.9)
";

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    Some(match args {
        ["coverage"] => coverage::run(),
        _ => return None,
    })
}
