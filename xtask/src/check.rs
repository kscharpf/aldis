//! `xtask check`: the checks to run while working, in one command (AGENTS.md).
//!
//! Format, clippy, rustdoc, tests, the bare-metal clippy build, and `xtask ci`, then the extras of
//! each directory the change touches (the paths that differ from `origin/main`, committed or
//! not). Stops at the first failure. An extra whose tool is not installed is skipped and reported.
//!
//! Every step that the merge gate (`verify`) also runs is the gate's command: a change that
//! passes here does not fail the gate on formatting or rustdoc. Tests keep the two in step.

use std::process::Command;

use crate::repo;

/// What a step does.
enum Action {
    /// Run a program in a directory below the repository root (`""` for the root).
    Cmd {
        program: &'static str,
        args: &'static [&'static str],
        dir: &'static str,
        env: &'static [(&'static str, &'static str)],
    },
    /// Run an xtask check in this process.
    Xtask(fn() -> Result<usize, String>),
}

struct Item {
    name: &'static str,
    action: Action,
}

const fn cmd(name: &'static str, program: &'static str, args: &'static [&'static str]) -> Item {
    Item {
        name,
        action: Action::Cmd {
            program,
            args,
            dir: "",
            env: &[],
        },
    }
}

const fn cmd_env(
    name: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
) -> Item {
    Item {
        name,
        action: Action::Cmd {
            program,
            args,
            dir: "",
            env,
        },
    }
}

const fn cmd_in(
    name: &'static str,
    dir: &'static str,
    program: &'static str,
    args: &'static [&'static str],
) -> Item {
    Item {
        name,
        action: Action::Cmd {
            program,
            args,
            dir,
            env: &[],
        },
    }
}

/// The workspaces that `cargo fmt --all` at the root does not reach, each with its own manifest,
/// and the name of the step that formats it. The gate formats the same directories (`FMT` of
/// `verify/checks.rs`).
const SEPARATE_WORKSPACES: [(&str, &str); 3] = [
    ("exerciser", "exerciser fmt"),
    ("fuzz", "fuzz fmt"),
    ("crosscheck", "crosscheck fmt"),
];

/// The environment of the rustdoc step: the gate's `RUSTDOCFLAGS` (`CHECK_ENV` of `verify`), so
/// that a link to a private item fails here and not only at the gate.
const DOC_ENV: &[(&str, &str)] = &[("RUSTDOCFLAGS", "-D warnings")];

/// Which directories with extra checks a change touches.
#[derive(Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags, one per directory with extra checks"
)]
struct Touched {
    /// A library crate or the exerciser: the exerciser calls every public API (VER-010).
    exerciser: bool,
    core: bool,
    ffi: bool,
    refapp: bool,
    /// The separate workspaces (`SEPARATE_WORKSPACES`) with a changed file: their format check.
    separate: Vec<(&'static str, &'static str)>,
}

/// The directories among `changed` (repository-relative paths) that have extra checks.
fn touched(changed: &[String]) -> Touched {
    let any = |prefix: &str| changed.iter().any(|p| p.starts_with(prefix));
    Touched {
        exerciser: any("crates/") || any("exerciser/"),
        core: any("crates/aldis-core/"),
        ffi: any("crates/aldis-ffi/"),
        refapp: any("refapp/"),
        separate: SEPARATE_WORKSPACES
            .into_iter()
            .filter(|(dir, _)| any(&format!("{dir}/")))
            .collect(),
    }
}

/// What can run on this machine.
#[derive(Clone, Copy)]
struct Tools {
    /// The pinned tooling nightly and `cargo-llvm-cov` are installed.
    coverage: bool,
    /// `cbindgen` is installed.
    cbindgen: bool,
}

/// The steps to run and the extras skipped, each with the reason.
#[expect(clippy::too_many_lines, reason = "one entry per check step")]
fn plan(t: &Touched, tools: Tools) -> (Vec<Item>, Vec<String>) {
    let mut run = vec![cmd("fmt", "cargo", &["fmt", "--all", "--check"])];
    run.extend(
        t.separate
            .iter()
            .map(|(dir, name)| cmd_in(name, dir, "cargo", &["fmt", "--all", "--check"])),
    );
    run.extend([
        cmd(
            "clippy",
            "cargo",
            &["clippy", "--workspace", "--all-features", "--all-targets"],
        ),
        cmd_env(
            "doc",
            "cargo",
            &["doc", "--workspace", "--all-features", "--no-deps"],
            DOC_ENV,
        ),
        cmd("test", "cargo", &["test", "--workspace", "--all-features"]),
        cmd(
            "clippy (thumbv7em)",
            "cargo",
            &[
                "clippy",
                "-p",
                "aldis",
                "--no-default-features",
                "--features",
                "full",
                "--target",
                "thumbv7em-none-eabihf",
            ],
        ),
        Item {
            name: "xtask ci",
            action: Action::Xtask(|| Ok(crate::run_steps(crate::CI))),
        },
    ]);
    let mut skipped = Vec::new();
    if t.exerciser {
        run.push(cmd_in(
            "exerciser clippy",
            "exerciser",
            "cargo",
            &["clippy", "--release", "--target", "thumbv7em-none-eabihf"],
        ));
        run.push(cmd_in(
            "exerciser build",
            "exerciser",
            "cargo",
            &["build", "--release", "--target", "thumbv7em-none-eabihf"],
        ));
    }
    if t.core {
        run.push(cmd(
            "fuzz clippy",
            "cargo",
            &[
                "clippy",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--all-targets",
            ],
        ));
        if tools.coverage {
            run.push(Item {
                name: "xtask coverage",
                action: Action::Xtask(crate::coverage::run),
            });
        } else {
            skipped.push("xtask coverage: tooling nightly or cargo-llvm-cov not installed".into());
        }
    }
    if t.ffi {
        if tools.cbindgen {
            run.push(Item {
                name: "xtask header",
                action: Action::Xtask(|| crate::header::run(false)),
            });
            run.push(Item {
                name: "xtask header c-smoke",
                action: Action::Xtask(|| crate::header::c_smoke("cc", "c++")),
            });
        } else {
            skipped.push("xtask header, header c-smoke: cbindgen not installed".into());
        }
    }
    if t.refapp {
        run.extend([
            cmd_in(
                "refapp fmt",
                "refapp",
                "cargo",
                &["fmt", "--all", "--check"],
            ),
            cmd_in(
                "refapp clippy",
                "refapp",
                "cargo",
                &["clippy", "--workspace", "--all-targets"],
            ),
            cmd_in("refapp test", "refapp", "cargo", &["test", "--workspace"]),
            cmd_in(
                "refapp flight build",
                "refapp",
                "cargo",
                &[
                    "build",
                    "-p",
                    "refapp-flight",
                    "--target",
                    "thumbv7em-none-eabihf",
                ],
            ),
        ]);
    }
    (run, skipped)
}

/// Lines of the output of `git` with `args`, or `None` when git fails.
fn git_lines(args: &[&str]) -> Option<Vec<String>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo::root())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
    })
}

/// Paths that differ from `origin/main`: committed on this branch, staged, modified, or new.
/// `None` when `origin/main` is unknown.
fn changed_paths() -> Option<Vec<String>> {
    let mut paths = git_lines(&["diff", "--name-only", "origin/main...HEAD"])?;
    paths.extend(git_lines(&["diff", "--name-only", "HEAD"])?);
    paths.extend(git_lines(&["ls-files", "--others", "--exclude-standard"])?);
    Some(paths)
}

/// Whether `program` with `args` starts and succeeds.
fn works(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn tools() -> Tools {
    let nightly = repo::tooling_nightly().is_ok_and(|n| {
        Command::new("rustup")
            .args(["toolchain", "list"])
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.starts_with(&n))
            })
    });
    Tools {
        coverage: nightly && works("cargo", &["llvm-cov", "--version"]),
        cbindgen: works("cbindgen", &["--version"]),
    }
}

/// Runs the plan; `Ok(0)` when every step passed.
pub fn run() -> Result<usize, String> {
    let (touched, note) = match changed_paths() {
        Some(paths) => (touched(&paths), None),
        None => (
            Touched {
                exerciser: true,
                core: true,
                ffi: true,
                refapp: true,
                separate: SEPARATE_WORKSPACES.to_vec(),
            },
            Some("origin/main unknown: running every directory's extras"),
        ),
    };
    if let Some(note) = note {
        println!("check: {note}");
    }
    let (items, skipped) = plan(&touched, tools());
    let total = items.len();
    let mut ran = Vec::new();
    for (i, item) in items.iter().enumerate() {
        println!("check: [{}/{total}] {}", i + 1, item.name);
        let ok = match &item.action {
            Action::Cmd {
                program,
                args,
                dir,
                env,
            } => Command::new(program)
                .args(*args)
                .envs(env.iter().copied())
                .current_dir(repo::root().join(dir))
                .status()
                .map_err(|e| format!("{program}: {e}"))?
                .success(),
            Action::Xtask(f) => f()? == 0,
        };
        if !ok {
            println!("check: FAILED at {}; ran: {}", item.name, ran.join(", "));
            return Ok(1);
        }
        ran.push(item.name);
    }
    println!("check: passed: {}", ran.join(", "));
    for s in &skipped {
        println!("check: skipped: {s}");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{Action, DOC_ENV, SEPARATE_WORKSPACES, Tools, Touched, plan, touched};
    use crate::verify::{CHECK_ENV, CLIPPY, FMT};

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(t: &Touched, tools: Tools) -> (Vec<&'static str>, Vec<String>) {
        let (items, skipped) = plan(t, tools);
        (items.iter().map(|i| i.name).collect(), skipped)
    }

    const ALL: Tools = Tools {
        coverage: true,
        cbindgen: true,
    };
    const NONE: Tools = Tools {
        coverage: false,
        cbindgen: false,
    };

    #[test]
    fn paths_select_directories() {
        assert_eq!(touched(&paths(&["docs/x.md"])), Touched::default());
        assert_eq!(
            touched(&paths(&["crates/aldis-spp/a.rs"])),
            Touched {
                exerciser: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            touched(&paths(&["crates/aldis-core/src/lib.rs"])),
            Touched {
                exerciser: true,
                core: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            touched(&paths(&[
                "crates/aldis-ffi/src/lib.rs",
                "refapp/Cargo.toml"
            ])),
            Touched {
                exerciser: true,
                ffi: true,
                refapp: true,
                ..Touched::default()
            }
        );
        // A sibling whose name only starts alike does not count.
        assert_eq!(
            touched(&paths(&["crates/aldis-core-x/a.rs", "refapp-x/a"])),
            Touched {
                exerciser: true,
                ..Touched::default()
            }
        );
    }

    #[test]
    fn base_steps_come_first_and_alone() {
        let (n, skipped) = names(&Touched::default(), NONE);
        assert_eq!(
            n,
            [
                "fmt",
                "clippy",
                "doc",
                "test",
                "clippy (thumbv7em)",
                "xtask ci"
            ]
        );
        assert_eq!(skipped.len(), 0);
    }

    #[test]
    fn separate_workspaces_are_formatted_when_touched() {
        // req: VER-004
        assert_eq!(
            touched(&paths(&["exerciser/src/ffi.rs"])).separate,
            [("exerciser", "exerciser fmt")]
        );
        assert_eq!(
            touched(&paths(&[
                "fuzz/fuzz_targets/a.rs",
                "crosscheck/src/main.rs"
            ]))
            .separate,
            [("fuzz", "fuzz fmt"), ("crosscheck", "crosscheck fmt")]
        );
        // A crate change reaches the exerciser's clippy, not its formatting; a sibling whose
        // name only starts alike does not count.
        let none = touched(&paths(&["crates/aldis-ffi/src/lib.rs", "fuzz-x/a.rs"]));
        assert_eq!(none.separate, Touched::default().separate);
        let t = Touched {
            separate: SEPARATE_WORKSPACES.to_vec(),
            ..Touched::default()
        };
        let (n, _) = names(&t, NONE);
        assert_eq!(
            n,
            [
                "fmt",
                "exerciser fmt",
                "fuzz fmt",
                "crosscheck fmt",
                "clippy",
                "doc",
                "test",
                "clippy (thumbv7em)",
                "xtask ci"
            ]
        );
    }

    #[test]
    fn the_formatted_workspaces_are_the_gates() {
        // req: VER-004
        let gate: Vec<&str> = FMT
            .lines()
            .filter_map(|l| l.split("--manifest-path ").nth(1))
            .collect();
        let ours: Vec<String> = SEPARATE_WORKSPACES
            .iter()
            .map(|(dir, _)| format!("{dir}/Cargo.toml"))
            .collect();
        assert_eq!(gate, ours);
    }

    #[test]
    fn the_doc_step_is_the_gates() {
        // req: VER-001
        let (items, _) = plan(&Touched::default(), NONE);
        let Some(Action::Cmd { args, env, .. }) =
            items.iter().find(|i| i.name == "doc").map(|i| &i.action)
        else {
            panic!("no doc step");
        };
        // The gate adds `--locked` to every cargo call of its checks.
        let gate = format!("cargo doc --locked {}", args[1..].join(" "));
        assert!(CLIPPY.lines().any(|l| l == gate), "{gate}");
        assert_eq!(*env, DOC_ENV);
        assert!(DOC_ENV.iter().all(|e| CHECK_ENV.contains(e)));
    }

    #[test]
    fn extras_follow_the_touched_directories() {
        let t = Touched {
            exerciser: true,
            core: true,
            ffi: true,
            refapp: true,
            ..Touched::default()
        };
        let (n, skipped) = names(&t, ALL);
        assert_eq!(n.len(), 6 + 2 + 2 + 2 + 4);
        assert!(n.contains(&"xtask coverage") && n.contains(&"xtask header c-smoke"));
        assert_eq!(skipped.len(), 0);
    }

    #[test]
    fn missing_tools_skip_with_a_reason() {
        let t = Touched {
            exerciser: true,
            core: true,
            ffi: true,
            refapp: false,
            ..Touched::default()
        };
        let (n, skipped) = names(&t, NONE);
        assert!(!n.contains(&"xtask coverage") && !n.contains(&"xtask header"));
        assert!(n.contains(&"exerciser clippy") && n.contains(&"fuzz clippy"));
        assert_eq!(skipped.len(), 2);
    }
}
