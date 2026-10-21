//! Shared helpers of the xtask crates: repository paths and file walking, and [`Tool`], what a
//! tool crate tells the `xtask` dispatcher about its commands (docs/06-verification.md §7).

pub mod repo;

/// The commands of one tool crate, as the `xtask` dispatcher runs and lists them.
///
/// The verification run reads `package` and `commands` to know which crate a check's
/// `cargo xtask <command>` runs (VER-056).
#[derive(Clone, Copy, Debug)]
pub struct Tool {
    /// The package whose `run` this is: `env!("CARGO_PKG_NAME")` of the crate that defines it.
    pub package: &'static str,
    /// The first word of each command it runs.
    pub commands: &'static [&'static str],
    /// Its part of `cargo xtask help`, in pieces that are printed one after the other.
    pub help: &'static [&'static str],
    /// Runs the command `args`, whose first word is one of `commands`; `None` when the arguments
    /// name no command of the tool. `Ok` holds the number of problems found.
    pub run: fn(&[&str]) -> Option<Result<usize, String>>,
}
