//! `cargo xtask ci`: everything CI runs without a C++ build, through the commands of the tool
//! crates. No verify check runs it.

use xtask_lib::Tool;

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["ci"],
    help: &[HELP],
    run,
};

const HELP: &str =
    "  ci                           everything CI runs without a C++ build: spec-lint, links,
                               layers, refapp-deps, vectors validate, vectors schema --check,
                               fields, trace --scope m0 --strict, trace --scope m1 --strict,
                               trace --scope m2, trace --scope c0, changelog check,
                               cpp unsafe-optouts
";

/// The steps of `ci`, in order: the tool and the command it runs.
const STEPS: &[(&Tool, &[&str])] = &[
    (&xtask_spec::TOOL, &["spec-lint"]),
    (&xtask_spec::TOOL, &["links"]),
    (&xtask_layers::TOOL, &["layers"]),
    (&xtask_layers::TOOL, &["refapp-deps"]),
    (&xtask_spec::TOOL, &["vectors", "validate"]),
    (&xtask_spec::TOOL, &["vectors", "schema", "--check"]),
    (&xtask_spec::TOOL, &["fields"]),
    // M0 and M1 are closed: every in-scope requirement must keep an artefact.
    (&xtask_spec::TOOL, &["trace", "--scope", "m0", "--strict"]),
    (&xtask_spec::TOOL, &["trace", "--scope", "m1", "--strict"]),
    // M2 is open: report its trace without failing (strict at the M2 exit, D1).
    (&xtask_spec::TOOL, &["trace", "--scope", "m2"]),
    // C0 is open: report its trace without failing (strict at the C0 exit, D17).
    (&xtask_spec::TOOL, &["trace", "--scope", "c0"]),
    (&xtask_changelog::TOOL, &["changelog", "check"]),
    (&xtask_cpp::TOOL, &["cpp", "unsafe-optouts"]),
];

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    match args {
        ["ci"] => Some(ci()),
        _ => None,
    }
}

fn ci() -> Result<usize, String> {
    let mut failed = 0;
    for (tool, args) in STEPS {
        failed += (tool.run)(args)
            .unwrap_or_else(|| Err(format!("ci: no command {}", args.join(" "))))?;
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::STEPS;

    // req: VER-056
    #[test]
    fn every_step_is_a_command_of_its_tool() {
        for (tool, args) in STEPS {
            let first = args.first().copied().unwrap_or_default();
            assert!(tool.commands.contains(&first), "{args:?}");
        }
    }
}
