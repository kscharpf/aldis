//! `cargo xtask features`: the `alloc` and `std` features are additive (GEN-006). The verify check
//! `features` runs it.

mod features;

use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["features"],
    help: &[HELP],
    run,
};

const HELP: &str =
    "  features                     `alloc` and `std` are additive: no API gated on them, and every
                               test passes alike without them, with `alloc`, and with `std`
                               (GEN-006)
";

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    Some(match args {
        ["features"] => features::check(),
        _ => return None,
    })
}
