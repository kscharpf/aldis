//! `cargo xtask spec-lint`, `trace`, `vectors`, `fields`, and `links`: the requirement tables and
//! their references, the traceability matrix (VER-020), the test vectors (TV-*), the field
//! catalogue (API-082), and the Markdown links. The verify check `spec` runs them.

mod fields;
mod links;
mod md;
mod spec;
mod trace;
mod vectors;

use xtask_lib::{Tool, repo};

/// The commands of this crate (VER-056).
pub const TOOL: Tool = Tool {
    package: env!("CARGO_PKG_NAME"),
    commands: &["spec-lint", "trace", "vectors", "fields", "links"],
    help: &[HELP],
    run,
};

const HELP: &str =
    "  spec-lint                    check requirement tables and every requirement-ID reference
  trace [--scope <m>] [--strict]
                               requirement -> artefact matrix in target/spec/ (VER-020);
                               --strict fails on in-scope requirements without artefacts
  vectors validate             check test-vectors/ against the model and the spec (TV-*)
  vectors schema [--check]     (re)generate test-vectors/schema/, or check it is current
  fields [--write]             check the field catalogue (03-api-model.md §9.1) and that
                               aldis_core::field and test-vectors/core/fields.json match it,
                               or regenerate both (API-082)
  links                        relative Markdown links resolve inside the repository
";

fn run(args: &[&str]) -> Option<Result<usize, String>> {
    Some(match args {
        ["spec-lint"] => spec::lint(),
        ["trace", rest @ ..] => {
            let scope = rest
                .iter()
                .position(|a| *a == "--scope")
                .and_then(|i| rest.get(i + 1))
                .copied();
            trace::run(scope, rest.contains(&"--strict"))
        }
        ["vectors", "validate"] => vectors::validate(),
        ["vectors", "schema"] => vectors::schema(false),
        ["vectors", "schema", "--check"] => vectors::schema(true),
        ["fields"] => fields::run(false),
        ["fields", "--write"] => fields::run(true),
        ["links"] => links::check(),
        _ => return None,
    })
}
