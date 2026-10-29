//! `xtask check`: the checks to run while working, in one command (AGENTS.md).
//!
//! Format, clippy, tests, the bare-metal clippy build, and `xtask ci`, then the extras of each
//! directory the change touches (the paths that differ from `origin/main`, committed or not).
//! Stops at the first failure. An extra whose tool is not installed is skipped and reported.

use std::process::Command;

use crate::repo;

/// What a step does.
enum Action {
    /// Run a program in a directory below the repository root (`""` for the root).
    Cmd {
        program: &'static str,
        args: &'static [&'static str],
        dir: &'static str,
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
        action: Action::Cmd { program, args, dir },
    }
}

/// Which directories with extra checks a change touches.
#[derive(Debug, Default, PartialEq, Eq)]
struct Touched {
    core: bool,
    ffi: bool,
    refapp: bool,
}

/// The directories among `changed` (repository-relative paths) that have extra checks.
fn touched(changed: &[String]) -> Touched {
    let any = |prefix: &str| changed.iter().any(|p| p.starts_with(prefix));
    Touched {
        core: any("crates/aldis-core/"),
        ffi: any("crates/aldis-ffi/"),
        refapp: any("refapp/"),
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
fn plan(t: &Touched, tools: Tools) -> (Vec<Item>, Vec<String>) {
    let mut run = vec![
        cmd("fmt", "cargo", &["fmt", "--all", "--check"]),
        cmd(
            "clippy",
            "cargo",
            &["clippy", "--workspace", "--all-features", "--all-targets"],
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
    ];
    let mut skipped = Vec::new();
    if t.core {
        run.push(cmd_in(
            "exerciser build",
            "exerciser",
            "cargo",
            &["build", "--release", "--target", "thumbv7em-none-eabihf"],
        ));
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
                core: true,
                ffi: true,
                refapp: true,
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
            Action::Cmd { program, args, dir } => Command::new(program)
                .args(*args)
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
    use super::{Tools, Touched, plan, touched};

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
        assert_eq!(
            touched(&paths(&["docs/x.md", "crates/aldis-spp/a.rs"])),
            Touched::default()
        );
        assert_eq!(
            touched(&paths(&["crates/aldis-core/src/lib.rs"])),
            Touched {
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
                ffi: true,
                refapp: true,
                ..Touched::default()
            }
        );
        // A sibling whose name only starts alike does not count.
        assert_eq!(
            touched(&paths(&["crates/aldis-core-x/a.rs", "refapp-x/a"])),
            Touched::default()
        );
    }

    #[test]
    fn base_steps_come_first_and_alone() {
        let (n, skipped) = names(&Touched::default(), NONE);
        assert_eq!(
            n,
            ["fmt", "clippy", "test", "clippy (thumbv7em)", "xtask ci"]
        );
        assert_eq!(skipped.len(), 0);
    }

    #[test]
    fn extras_follow_the_touched_directories() {
        let t = Touched {
            core: true,
            ffi: true,
            refapp: true,
        };
        let (n, skipped) = names(&t, ALL);
        assert_eq!(n.len(), 5 + 3 + 2 + 4);
        assert!(n.contains(&"xtask coverage") && n.contains(&"xtask header c-smoke"));
        assert_eq!(skipped.len(), 0);
    }

    #[test]
    fn missing_tools_skip_with_a_reason() {
        let t = Touched {
            core: true,
            ffi: true,
            refapp: false,
        };
        let (n, skipped) = names(&t, NONE);
        assert!(!n.contains(&"xtask coverage") && !n.contains(&"xtask header"));
        assert!(n.contains(&"exerciser build") && n.contains(&"fuzz clippy"));
        assert_eq!(skipped.len(), 2);
    }
}
