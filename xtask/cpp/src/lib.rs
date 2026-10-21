//! `cargo xtask cpp`: repository checks of the native C++ implementation
//! (docs/milestones/c0-foundations.md D12). The verify check `cpp-layers` runs `cpp layers` and
//! `cpp unsafe-optouts`.

mod cpp;

use xtask_layers as layers;
use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["cpp"],
    help: &[
        "\nC++ checks (docs/milestones/c0-foundations.md D12):\n",
        cpp::HELP,
    ],
    run,
};

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    match args {
        ["cpp", rest @ ..] => Some(cpp::run(rest)),
        _ => None,
    }
}
