//! `xtask check`: the checks to run while working, in one command (AGENTS.md).
//!
//! Format, clippy, rustdoc, tests, the bare-metal clippy build, and `xtask ci`, then the extras of
//! each directory the change touches (the paths that differ from `origin/main`, committed or
//! not). Stops at the first failure. An extra whose tool is not installed is skipped and reported.
//! One run at a time per worktree (`check_lock`). `check --from <step>` skips the steps before the
//! named one, to go on after a failure without redoing what passed; such a run ends `partial`,
//! never `passed` (VER-069).
//!
//! Every command of the gate's `fmt`, `clippy`, and `test` scripts (`verify/checks.rs`) is a step
//! here, with `--locked` as the gate passes it, so that a change that passes `check` does not fail
//! the gate on formatting, lints, rustdoc, a stale `Cargo.lock`, or a test (VER-068). The test
//! `every_gate_line_has_a_step` fails when a line of those scripts has no step. The gate's other
//! checks (the exerciser and the builds per target on the pinned nightly, coverage, Kani, the
//! header, `spec`) are not mirrored, except as the extras below.

use std::process::Command;
use std::time::{Duration, Instant};

use crate::{check_lock, repo};

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
    /// A library crate or `fuzz/`: the fuzz targets call the public API.
    fuzz: bool,
    /// `crosscheck/`: the independent cross-checks, a workspace of their own.
    crosscheck: bool,
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
        fuzz: any("crates/") || any("fuzz/"),
        crosscheck: any("crosscheck/"),
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
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-features",
                "--all-targets",
            ],
        ),
        cmd_env(
            "doc",
            "cargo",
            &[
                "doc",
                "--locked",
                "--workspace",
                "--all-features",
                "--no-deps",
            ],
            DOC_ENV,
        ),
        cmd(
            "test",
            "cargo",
            &["test", "--locked", "--workspace", "--all-features"],
        ),
        cmd(
            "clippy (thumbv7em)",
            "cargo",
            &[
                "clippy",
                "--locked",
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
            &[
                "clippy",
                "--locked",
                "--release",
                "--target",
                "thumbv7em-none-eabihf",
            ],
        ));
        run.push(cmd_in(
            "exerciser build",
            "exerciser",
            "cargo",
            &[
                "build",
                "--locked",
                "--release",
                "--target",
                "thumbv7em-none-eabihf",
            ],
        ));
    }
    if t.fuzz {
        run.push(cmd(
            "fuzz clippy",
            "cargo",
            &[
                "clippy",
                "--locked",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--all-targets",
            ],
        ));
    }
    if t.crosscheck {
        run.push(cmd(
            "crosscheck clippy",
            "cargo",
            &[
                "clippy",
                "--locked",
                "--manifest-path",
                "crosscheck/Cargo.toml",
                "--all-targets",
            ],
        ));
    }
    if t.core {
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
                &["clippy", "--locked", "--workspace", "--all-targets"],
            ),
            cmd_in(
                "refapp test",
                "refapp",
                "cargo",
                &["test", "--locked", "--workspace"],
            ),
            cmd_in(
                "refapp flight build",
                "refapp",
                "cargo",
                &[
                    "build",
                    "--locked",
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

/// A step's wall-clock time as `1m42s`, `42s`, or `0.3s` under a second.
fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{secs}s")
    } else {
        format!("0.{}s", d.subsec_millis() / 100)
    }
}

/// The step `check --from` names, or `None` for a whole run.
fn parse_args<'a>(args: &[&'a str]) -> Result<Option<&'a str>, String> {
    match args {
        [] => Ok(None),
        ["--from", step] => Ok(Some(step)),
        _ => Err("usage: cargo xtask check [--from <step>]".into()),
    }
}

/// The position of the step `from` in `names`, or an error that lists them.
fn from_index(names: &[&str], from: &str) -> Result<usize, String> {
    names.iter().position(|n| *n == from).ok_or_else(|| {
        format!(
            "check --from: no step named `{from}`; the steps are: {}",
            names.join(", ")
        )
    })
}

/// Runs the plan; `Ok(0)` when every step passed.
pub fn run(args: &[&str]) -> Result<usize, String> {
    let from = parse_args(args)?;
    let (touched, note) = match changed_paths() {
        Some(paths) => (touched(&paths), None),
        None => (
            Touched {
                exerciser: true,
                core: true,
                fuzz: true,
                crosscheck: true,
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
    let first = match from {
        Some(step) => from_index(&items.iter().map(|i| i.name).collect::<Vec<_>>(), step)?,
        None => 0,
    };
    let _lock = check_lock::acquire()?;
    let before: Vec<&str> = items.iter().take(first).map(|i| i.name).collect();
    let partial = from.map_or_else(String::new, |step| {
        format!(" (partial: --from {step} skipped {})", before.join(", "))
    });
    let mut ran: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate().skip(first) {
        println!("check: [{}/{total}] {}", i + 1, item.name);
        let started = Instant::now();
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
        let took = fmt_duration(started.elapsed());
        println!("check: [{}/{total}] {} ({took})", i + 1, item.name);
        if !ok {
            println!(
                "check: FAILED at {} ({took}); ran: {}{partial}",
                item.name,
                ran.join(", ")
            );
            return Ok(1);
        }
        ran.push(format!("{} ({took})", item.name));
    }
    if let Some(step) = from {
        println!(
            "check: partial, not a pass: ran from {step}: {}; skipped by --from: {}",
            ran.join(", "),
            before.join(", ")
        );
    } else {
        println!("check: passed: {}", ran.join(", "));
    }
    for s in &skipped {
        println!("check: skipped: {s}");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{
        Action, DOC_ENV, SEPARATE_WORKSPACES, Tools, Touched, fmt_duration, from_index, parse_args,
        plan, touched,
    };
    use crate::verify::{CHECK_ENV, CLIPPY, FMT, TEST};
    use std::time::Duration;

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
    fn durations_print_as_minutes_seconds_or_tenths() {
        // req: VER-068
        assert_eq!(fmt_duration(Duration::from_secs(102)), "1m42s");
        assert_eq!(fmt_duration(Duration::from_secs(60)), "1m00s");
        assert_eq!(fmt_duration(Duration::from_secs(42)), "42s");
        assert_eq!(fmt_duration(Duration::from_millis(340)), "0.3s");
    }

    #[test]
    fn from_names_a_step_of_the_plan() {
        // req: VER-069
        assert_eq!(parse_args(&[]), Ok(None));
        assert_eq!(parse_args(&["--from", "test"]), Ok(Some("test")));
        assert!(parse_args(&["--from"]).is_err());
        assert!(parse_args(&["--quick"]).is_err());
        let (n, _) = names(&Touched::default(), NONE);
        assert_eq!(from_index(&n, "fmt"), Ok(0));
        assert_eq!(from_index(&n, "test"), Ok(3));
        let err = from_index(&n, "tset").unwrap_err();
        assert!(
            err.contains("`tset`") && err.contains("fmt, clippy, doc, test"),
            "{err}"
        );
    }

    #[test]
    fn paths_select_directories() {
        assert_eq!(touched(&paths(&["docs/x.md"])), Touched::default());
        assert_eq!(
            touched(&paths(&["crates/aldis-spp/a.rs"])),
            Touched {
                exerciser: true,
                fuzz: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            touched(&paths(&["crates/aldis-core/src/lib.rs"])),
            Touched {
                exerciser: true,
                core: true,
                fuzz: true,
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
                fuzz: true,
                ffi: true,
                refapp: true,
                ..Touched::default()
            }
        );
        // `fuzz/` and `crosscheck/` alone select their own steps.
        let fuzz = touched(&paths(&["fuzz/fuzz_targets/a.rs"]));
        assert!(fuzz.fuzz && !fuzz.exerciser && !fuzz.crosscheck);
        let cross = touched(&paths(&["crosscheck/src/main.rs"]));
        assert!(cross.crosscheck && !cross.fuzz && !cross.exerciser);
        // A sibling whose name only starts alike does not count.
        assert_eq!(
            touched(&paths(&["crates/aldis-core-x/a.rs", "refapp-x/a"])),
            Touched {
                exerciser: true,
                fuzz: true,
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
        // req: VER-068
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
        // A crate change reaches the clippy of the exerciser and the fuzz targets, not their
        // formatting; a sibling whose name only starts alike does not count.
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
        // req: VER-068
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
    fn the_doc_step_has_the_gates_flags() {
        // req: VER-068
        let (items, _) = plan(&Touched::default(), NONE);
        let Some(Action::Cmd { env, .. }) =
            items.iter().find(|i| i.name == "doc").map(|i| &i.action)
        else {
            panic!("no doc step");
        };
        assert_eq!(*env, DOC_ENV);
        assert!(DOC_ENV.iter().all(|e| CHECK_ENV.contains(e)));
    }

    /// A command as the directory it runs in and its words, without `--locked` (the steps of
    /// `plan` and the lines of the gate both carry it) and with `--manifest-path <dir>/Cargo.toml`
    /// folded into the directory.
    fn normal(dir: &str, words: &[&str]) -> (String, String) {
        let mut dir = dir.to_owned();
        let mut kept = Vec::new();
        let mut manifest = false;
        for w in words {
            if manifest {
                dir = w.strip_suffix("/Cargo.toml").unwrap_or(w).to_owned();
                manifest = false;
            } else if *w == "--manifest-path" {
                manifest = true;
            } else if *w != "--locked" {
                kept.push(*w);
            }
        }
        (dir, kept.join(" "))
    }

    /// Lines of the gate's scripts that `check` does not run, with the reason.
    const NOT_A_STEP: &[(&str, &str)] = &[];

    #[test]
    fn every_gate_line_has_a_step() {
        // req: VER-068
        let all = Touched {
            exerciser: true,
            core: true,
            fuzz: true,
            crosscheck: true,
            ffi: true,
            refapp: true,
            separate: SEPARATE_WORKSPACES.to_vec(),
        };
        let (items, _) = plan(&all, ALL);
        let steps: Vec<(String, String)> = items
            .iter()
            .filter_map(|i| match &i.action {
                Action::Cmd {
                    program, args, dir, ..
                } => {
                    let mut words = vec![*program];
                    words.extend(args.iter().copied());
                    Some(normal(dir, &words))
                }
                Action::Xtask(_) => None,
            })
            .collect();
        for script in [FMT, CLIPPY, TEST] {
            for line in script.lines().filter(|l| l.starts_with("cargo ")) {
                let words: Vec<&str> = line.split_whitespace().collect();
                assert!(
                    steps.contains(&normal("", &words))
                        || NOT_A_STEP.iter().any(|(l, _)| *l == line),
                    "no step of `check` runs the gate's `{line}`: add it to `plan`, or list it in NOT_A_STEP with the reason"
                );
            }
        }
    }

    #[test]
    fn extras_follow_the_touched_directories() {
        let t = Touched {
            exerciser: true,
            core: true,
            fuzz: true,
            crosscheck: true,
            ffi: true,
            refapp: true,
            ..Touched::default()
        };
        let (n, skipped) = names(&t, ALL);
        assert_eq!(n.len(), 6 + 2 + 1 + 1 + 1 + 2 + 4);
        assert!(n.contains(&"xtask coverage") && n.contains(&"xtask header c-smoke"));
        assert_eq!(skipped.len(), 0);
    }

    #[test]
    fn missing_tools_skip_with_a_reason() {
        let t = Touched {
            exerciser: true,
            core: true,
            fuzz: true,
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
