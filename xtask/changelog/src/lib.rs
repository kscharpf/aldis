//! `cargo xtask changelog`: the fragments of `changelog.d/` and the release of `CHANGELOG.md`
//! (GEN-080). The verify check `spec` runs `changelog check`.

mod changelog;

use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["changelog"],
    help: &[HELP],
    run,
};

const HELP: &str =
    "  changelog check              every fragment in changelog.d/ is well formed (GEN-080)
  changelog show               print the unreleased changes as the next release will list them
  changelog release <version> <YYYY-MM-DD>
                               move every fragment into a new section of CHANGELOG.md
";

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    Some(match args {
        ["changelog", "check"] => changelog::check(),
        ["changelog", "show"] => changelog::show(),
        ["changelog", "release", version, date] => changelog::release(version, date),
        _ => return None,
    })
}
