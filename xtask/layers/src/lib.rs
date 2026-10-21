//! `cargo xtask layers` and `refapp-deps`: the crate dependency rules of the library
//! (GEN-001…GEN-004) and of `refapp/` (APP-100, APP-105). The verify check `layers` runs them;
//! `xtask-cpp` applies the same layer table to the C++ modules (CPP-010).

mod layers;
mod refapp;

pub use layers::{BOUNDARY, LAYERS, SAME_LAYER_ALLOWED};
use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["layers", "refapp-deps"],
    help: &[HELP],
    run,
};

const HELP: &str = "  layers                       crate dependency rules (GEN-001..004)
  refapp-deps                  refapp/ uses Aldis only through the `aldis` facade and
                               forbids unsafe code (APP-100, APP-105)
";

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    Some(match args {
        ["layers"] => layers::check(),
        ["refapp-deps"] => refapp::check(),
        _ => return None,
    })
}
