//! `cargo xtask symbols` and `stack-sizes`: checks of built binaries, no frame-protocol code in a
//! build of `aldis-spp` alone (VER-031) and the stack usage of the exerciser (VER-011). The verify
//! check `layers` runs `symbols`; the nightly workflow runs `stack-sizes`.

mod binaries;

use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["symbols", "stack-sizes"],
    help: &[HELP],
    run,
};

const HELP: &str =
    "  symbols                      no frame-protocol code in a build of aldis-spp alone (VER-031)
  stack-sizes <target>         per-function stack usage of the exerciser (VER-011; tooling nightly)
";

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    Some(match args {
        ["symbols"] => binaries::symbols(),
        ["stack-sizes", target] => binaries::stack_sizes(target),
        _ => return None,
    })
}
