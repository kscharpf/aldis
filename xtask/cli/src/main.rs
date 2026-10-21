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
        ["verify", rest @ ..] => xtask_verify::run(rest, TOOLS),
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

#[cfg(test)]
mod tests {
    use super::{TOOLS, help};

    /// The checks that a change to `path` alone runs, the xtask inputs derived from this
    /// repository's crate graph and the dispatcher's tools.
    fn readers(path: &str) -> Vec<String> {
        xtask_verify::readers(TOOLS, path).unwrap()
    }

    /// The checks that build, lint, test, or audit the whole workspace, `spec`, and `extra`, in
    /// table order.
    fn workspace_and(extra: &[&str]) -> Vec<String> {
        let whole = [
            "fmt",
            "clippy",
            "test",
            "test-macos",
            "big-endian",
            "deny",
            "spec",
        ];
        readers("xtask/notes.txt")
            .into_iter()
            .filter(|c| whole.contains(&c.as_str()) || extra.contains(&c.as_str()))
            .collect()
    }

    // req: VER-056
    #[test]
    fn a_tool_change_runs_the_checks_that_run_the_tool() {
        for (path, extra) in [
            ("xtask/features/src/features.rs", &["features"][..]),
            ("xtask/coverage/src/coverage.rs", &["coverage"]),
            ("xtask/binaries/src/binaries.rs", &["layers"]),
            ("xtask/layers/src/layers.rs", &["layers", "cpp-layers"]),
            ("xtask/layers/src/refapp.rs", &["layers", "cpp-layers"]),
            ("xtask/cpp/src/cpp/optouts.rs", &["cpp-layers"]),
            ("xtask/spec/src/trace.rs", &[]),
            ("xtask/changelog/src/changelog.rs", &[]),
            ("xtask/ci/src/lib.rs", &[]),
        ] {
            assert_eq!(readers(path), workspace_and(extra), "{path}");
        }
    }

    // req: VER-056
    #[test]
    fn a_harness_dispatcher_or_manifest_change_runs_every_check() {
        let every = readers("xtask/notes.txt");
        assert_eq!(every.len(), 32);
        for path in [
            "xtask/cli/src/main.rs",
            "xtask/lib/src/repo.rs",
            "xtask/verify/src/verify/scope.rs",
            "xtask/verify/Containerfile",
            "xtask/features/Cargo.toml",
            "Cargo.lock",
        ] {
            assert_eq!(readers(path), every, "{path}");
        }
    }

    // req: VER-056
    #[test]
    fn a_cpp_change_runs_the_cpp_checks_and_the_xtask_tests() {
        let cpp = readers("cpp/src/core/crc.cpp");
        let rust: Vec<&str> = cpp
            .iter()
            .map(String::as_str)
            .filter(|c| !c.starts_with("cpp-"))
            .collect();
        assert_eq!(rust, ["test", "test-macos", "big-endian", "spec"]);
        assert_eq!(cpp.len(), 4 + 15);
    }

    // req: VER-056
    #[test]
    fn every_tool_is_listed_once_and_helps() {
        let mut packages: Vec<&str> = TOOLS.iter().map(|t| t.package).collect();
        packages.sort_unstable();
        packages.dedup();
        assert_eq!(packages.len(), TOOLS.len());
        let help = help();
        for tool in TOOLS {
            for command in tool.commands {
                assert!(help.contains(&format!("\n  {command}")), "{command}");
            }
        }
        assert!(help.ends_with(xtask_verify::HELP));
    }
}
