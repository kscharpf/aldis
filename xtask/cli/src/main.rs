//! Project tooling for Aldis (docs/milestones/m0-foundations.md WP-2).
//!
//! Run with `cargo xtask <command>`; see `cargo xtask help`. This binary only dispatches: each
//! command runs in the tool crate whose [`Tool`] claims its first word, and `verify` in the
//! harness, `xtask-verify` (docs/06-verification.md §7).

use std::process::ExitCode;

use xtask_lib::Tool;

/// The tool crates, in the order of `cargo xtask help`.
const TOOLS: &[Tool] = &[
    xtask_spec::TOOL,
    xtask_layers::TOOL,
    xtask_binaries::TOOL,
    xtask_features::TOOL,
    xtask_coverage::TOOL,
    xtask_changelog::TOOL,
    xtask_ci::TOOL,
    xtask_cpp::TOOL,
];

const USAGE: &str = "usage: cargo xtask <command>\n\ncommands:\n";

/// The text of `cargo xtask help`.
fn help() -> String {
    let mut out = USAGE.to_owned();
    for piece in TOOLS.iter().flat_map(|t| t.help) {
        out.push_str(piece);
    }
    out.push('\n');
    out.push_str(xtask_verify::HELP);
    out
}

fn run(args: &[&str]) -> Result<usize, String> {
    match args {
        [] | ["help" | "-h" | "--help"] => {
            print!("{}", help());
            Ok(0)
        }
        ["verify", rest @ ..] => xtask_verify::run(rest),
        [command, ..] => TOOLS
            .iter()
            .find(|t| t.commands.contains(command))
            .and_then(|t| (t.run)(args))
            .unwrap_or_else(|| Err(format!("unknown command: {}\n\n{}", args.join(" "), help()))),
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
