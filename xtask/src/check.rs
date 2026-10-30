//! `xtask check`: the checks to run while working, in one command (AGENTS.md).
//!
//! Three tiers (VER-068); the gate (`cargo xtask verify`) stays the only authority on a merge.
//!
//! - `check --quick`: format of the four workspaces and `cargo check --all-targets` of each, which
//!   runs every step and prints every error, for the edit loop (about a minute).
//! - `check` (default): format, clippy, rustdoc, tests, the bare-metal clippy build, and
//!   `xtask ci`, then the extras of each directory the change touches (the paths that differ from
//!   `origin/main`, committed or not). Stops at the first failure. An extra whose tool is not
//!   installed is skipped and reported. It does not promise the gate.
//! - `check --full`: the default tier plus coverage of the touched crates, `stack`, and
//!   `big-endian`, once before the final commit.
//!
//! One run at a time per worktree (`check_lock`). `check --from <step>` skips the steps before the
//! named one, to go on after a failure without redoing what passed; such a run ends `partial`,
//! never `passed` (VER-069).
//!
//! The steps that mirror a line of the gate's `fmt`, `clippy`, and `test` scripts
//! (`verify/checks.rs`) run it with `--locked` and under the gate's `RUSTFLAGS` and
//! `RUSTDOCFLAGS` (`GATE_ENV`). The test `every_gate_line_has_a_step` fails when such a line has
//! no step, and `every_gate_check_is_covered_or_gate_only` fails when a check of
//! `cargo xtask verify list` is neither covered by a step of `--full` (`COVERED`) nor named with a
//! reason in `GATE_ONLY` (VER-068).

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
    /// Run the coverage gate for the named crates, or for every crate when `aldis-core` is among
    /// them (`coverage::run_for`).
    Coverage(Vec<String>),
}

/// How much of the gate a run covers, from the least to the most (VER-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    /// Format and compile of the four workspaces, every error at once.
    Quick,
    /// Format, lints, rustdoc, tests, the bare-metal build, `xtask ci`, and the extras of the
    /// touched directories.
    Default,
    /// The default tier plus coverage, `stack`, and `big-endian`.
    Full,
}

impl Mode {
    const fn name(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Default => "default",
            Self::Full => "full",
        }
    }
}

struct Item {
    name: &'static str,
    action: Action,
}

const fn cmd(name: &'static str, program: &'static str, args: &'static [&'static str]) -> Item {
    cmd_in(name, "", program, args)
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
            env: GATE_ENV,
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

/// The environment of every step: the gate's `CHECK_ENV` (`verify`) without `CARGO_TERM_COLOR`,
/// which only changes how the output looks. `RUSTFLAGS` makes a warning an error, as it is at
/// the gate, so that an `unexpected cfg` warning in a test module fails here and not only in the
/// `big-endian` check; `RUSTDOCFLAGS` does the same for a link to a private item. The flags are
/// part of cargo's cache key: the first run after a change of them rebuilds (VER-068).
const GATE_ENV: &[(&str, &str)] = &[
    ("RUSTFLAGS", "-D warnings"),
    ("RUSTDOCFLAGS", "-D warnings"),
];

/// Which directories with extra checks a change touches.
#[derive(Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags, one per directory with extra checks"
)]
struct Touched {
    /// A library crate or the exerciser: the exerciser calls every public API (VER-010).
    exerciser: bool,
    /// The crates (the directories of `crates/`) with a changed file, sorted.
    crates: Vec<String>,
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
    let mut crates: Vec<String> = changed
        .iter()
        .filter_map(|p| p.strip_prefix("crates/")?.split_once('/'))
        .map(|(name, _)| name.to_owned())
        .collect();
    crates.sort();
    crates.dedup();
    Touched {
        exerciser: any("crates/") || any("exerciser/"),
        crates,
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags, one per tool that an extra step needs"
)]
struct Tools {
    /// The pinned tooling nightly is installed (`stack` builds on it).
    nightly: bool,
    /// The pinned tooling nightly and `cargo-llvm-cov` are installed.
    coverage: bool,
    /// The s390x target, its cross linker, and QEMU user mode are installed (`big-endian`).
    big_endian: bool,
    /// `cbindgen` is installed.
    cbindgen: bool,
}

/// The environment of `big-endian`: the gate's flags and the exports of its script (`BIG_ENDIAN`
/// of `verify/checks.rs`; a test compares them).
const BIG_ENDIAN_ENV: &[(&str, &str)] = &[
    ("RUSTFLAGS", "-D warnings"),
    ("RUSTDOCFLAGS", "-D warnings"),
    (
        "CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_LINKER",
        "s390x-linux-gnu-gcc-14",
    ),
    (
        "CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_RUNNER",
        "qemu-s390x -L /usr/s390x-linux-gnu",
    ),
];

/// What `check --quick` runs: the format of the four workspaces and `cargo check` of each, whatever
/// the change touches. The compile steps keep going after an error (`--keep-going`), and the run
/// goes on after a failed step, so that one run lists every error. The exerciser is a `no_std`
/// binary: it is checked for the target it is linked for, as its clippy step is.
fn quick_plan() -> Vec<Item> {
    let mut run = vec![cmd("fmt", "cargo", &["fmt", "--all", "--check"])];
    run.extend(
        SEPARATE_WORKSPACES
            .iter()
            .map(|(dir, name)| cmd_in(name, dir, "cargo", &["fmt", "--all", "--check"])),
    );
    run.extend([
        cmd(
            "check",
            "cargo",
            &[
                "check",
                "--locked",
                "--workspace",
                "--all-features",
                "--all-targets",
                "--keep-going",
            ],
        ),
        cmd_in(
            "exerciser check",
            "exerciser",
            "cargo",
            &[
                "check",
                "--locked",
                "--release",
                "--target",
                "thumbv7em-none-eabihf",
                "--keep-going",
            ],
        ),
        cmd_in(
            "fuzz check",
            "fuzz",
            "cargo",
            &["check", "--locked", "--all-targets", "--keep-going"],
        ),
        cmd_in(
            "crosscheck check",
            "crosscheck",
            "cargo",
            &["check", "--locked", "--all-targets", "--keep-going"],
        ),
    ]);
    run
}

/// The steps to run and the extras skipped, each with the reason.
#[expect(clippy::too_many_lines, reason = "one entry per check step")]
fn plan(t: &Touched, tools: Tools, mode: Mode) -> (Vec<Item>, Vec<String>) {
    if mode == Mode::Quick {
        return (quick_plan(), Vec::new());
    }
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
        cmd(
            "doc",
            "cargo",
            &[
                "doc",
                "--locked",
                "--workspace",
                "--all-features",
                "--no-deps",
            ],
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
    if mode == Mode::Full {
        full_steps(t, tools, &mut run, &mut skipped);
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

/// The steps that only `--full` runs: coverage of the touched crates, `stack`, and `big-endian`.
/// Each is scheduled when the gate would run the check for the change (`COVERAGE`, `STACK`, and
/// `RUST_TEST_INPUTS` of `verify/checks.rs`: a crate or the exerciser), and skipped with the
/// reason when its tool is missing.
fn full_steps(t: &Touched, tools: Tools, run: &mut Vec<Item>, skipped: &mut Vec<String>) {
    if !t.crates.is_empty() {
        if tools.coverage {
            run.push(Item {
                name: "xtask coverage",
                action: Action::Coverage(t.crates.clone()),
            });
        } else {
            skipped.push("xtask coverage: tooling nightly or cargo-llvm-cov not installed".into());
        }
    }
    if t.exerciser {
        if tools.nightly {
            run.push(Item {
                name: "xtask stack",
                action: Action::Xtask(|| crate::stack::run(&[])),
            });
        } else {
            skipped.push("xtask stack: tooling nightly not installed".into());
        }
        if tools.big_endian {
            run.push(Item {
                name: "big-endian",
                action: Action::Cmd {
                    program: "cargo",
                    args: &[
                        "test",
                        "--locked",
                        "--workspace",
                        "--all-features",
                        "--target",
                        "s390x-unknown-linux-gnu",
                    ],
                    dir: "",
                    env: BIG_ENDIAN_ENV,
                },
            });
        } else {
            skipped.push(
                "big-endian: needs the s390x-unknown-linux-gnu target, s390x-linux-gnu-gcc-14, and \
                 qemu-s390x (rustup target add s390x-unknown-linux-gnu; Debian: apt install \
                 gcc-14-s390x-linux-gnu qemu-user)"
                    .into(),
            );
        }
    }
}

/// The gate checks that `check` runs in full, each with the steps that run all its lines and the
/// first tier that runs them all. A step is scheduled only when the change touches what the check
/// reads, as the gate skips a check whose inputs did not change (VER-056).
const COVERED: &[(&str, &[&str], Mode)] = &[
    (
        "fmt",
        &["fmt", "exerciser fmt", "fuzz fmt", "crosscheck fmt"],
        Mode::Quick,
    ),
    (
        "clippy",
        &["clippy", "doc", "fuzz clippy", "crosscheck clippy"],
        Mode::Default,
    ),
    ("test", &["test"], Mode::Default),
    ("coverage", &["xtask coverage"], Mode::Full),
    ("stack", &["xtask stack"], Mode::Full),
    ("big-endian", &["big-endian"], Mode::Full),
];

/// The gate checks that no tier of `check` runs in full, each with the reason. A new check of
/// `cargo xtask verify list` fails `every_gate_check_is_covered_or_gate_only` until it is added
/// here or to `COVERED`.
const GATE_ONLY: &[(&str, &str)] = &[
    (
        "test-macos",
        "host-bound: needs a macOS host, where `test` is the same command",
    ),
    (
        "features",
        "`cargo xtask features` builds the feature combinations, about 150 s; not mirrored",
    ),
    (
        "header",
        "partly: with aldis-ffi touched, check runs the header and the C smoke with the host `cc`, not gcc-14 and clang",
    ),
    (
        "no-std-thumbv7em-none-eabihf",
        "partly: check runs only its clippy line (`clippy (thumbv7em)`)",
    ),
    (
        "no-std-armv7r-none-eabihf",
        "not mirrored: the thumbv7em steps stand for the bare-metal targets",
    ),
    (
        "no-std-riscv32imac-unknown-none-elf",
        "not mirrored: the thumbv7em steps stand for the bare-metal targets",
    ),
    (
        "layers",
        "partly: `xtask ci` runs `layers` and `refapp-deps`, not `symbols`",
    ),
    (
        "refapp",
        "partly: with refapp/ touched, check runs its fmt, clippy, tests, and flight build, not the scenario and golden checks",
    ),
    (
        "fuzz-smoke",
        "not mirrored: runs the fuzz targets briefly on the tooling nightly",
    ),
    (
        "exerciser-thumbv7em-none-eabihf",
        "partly: check runs the exerciser's clippy and release build, not the canary or the QEMU run",
    ),
    (
        "exerciser-armv7r-none-eabihf",
        "not mirrored: needs the tooling nightly and QEMU for the target",
    ),
    (
        "exerciser-riscv32imac-unknown-none-elf",
        "not mirrored: needs the tooling nightly and QEMU for the target",
    ),
    (
        "rust-sparc-leon3",
        "non-gating: needs the LEON3 toolchain (OI-08)",
    ),
    ("kani", "nightly-only (VER-059)"),
    (
        "spec",
        "partly: `xtask ci` runs all of its commands but the cross-check run",
    ),
    ("deny", "not mirrored: needs cargo-deny"),
];

/// The gating checks of the gate that a run of `mode` does not run in full, for the last line.
fn gate_also_runs(mode: Mode) -> Vec<String> {
    let gating: Vec<String> = crate::verify::all_checks()
        .into_iter()
        .filter(|c| c.gating && !c.nightly)
        .map(|c| c.name)
        .collect();
    let beyond = COVERED
        .iter()
        .filter(|(_, _, first)| *first > mode)
        .map(|(name, _, _)| *name);
    let only = GATE_ONLY.iter().map(|(name, _)| *name);
    beyond
        .chain(only)
        .filter(|n| gating.iter().any(|g| g == n))
        .map(str::to_owned)
        .collect()
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

/// Whether the Rust target `name` is installed for the active toolchain.
fn target_installed(name: &str) -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l == name)
        })
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
        nightly,
        coverage: nightly && works("cargo", &["llvm-cov", "--version"]),
        big_endian: target_installed("s390x-unknown-linux-gnu")
            && works("qemu-s390x", &["--version"])
            && works("s390x-linux-gnu-gcc-14", &["--version"]),
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

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct Options<'a> {
    mode: Mode,
    /// The step `check --from` names, or `None` for a whole run.
    from: Option<&'a str>,
}

const USAGE: &str = "usage: cargo xtask check [--quick | --full] [--from <step>]";

fn parse_args<'a>(args: &[&'a str]) -> Result<Options<'a>, String> {
    let mut mode = None;
    let mut from = None;
    let mut rest = args;
    // bound: each pass consumes one or two words of `rest`.
    while let [first, tail @ ..] = rest {
        rest = tail;
        match *first {
            "--quick" | "--full" => {
                let m = if *first == "--quick" {
                    Mode::Quick
                } else {
                    Mode::Full
                };
                if mode.replace(m).is_some() {
                    return Err(format!(
                        "check: at most one of --quick and --full, once\n{USAGE}"
                    ));
                }
            }
            "--from" => {
                let [step, tail @ ..] = rest else {
                    return Err(USAGE.into());
                };
                rest = tail;
                if from.replace(*step).is_some() {
                    return Err(format!("check: --from at most once\n{USAGE}"));
                }
            }
            _ => return Err(USAGE.into()),
        }
    }
    Ok(Options {
        mode: mode.unwrap_or(Mode::Default),
        from,
    })
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
    let Options { mode, from } = parse_args(args)?;
    let (touched, note) = match changed_paths() {
        Some(paths) => (touched(&paths), None),
        None => (
            Touched {
                exerciser: true,
                // Stands for every crate: `coverage::select` takes `aldis-core` as all of them.
                crates: vec!["aldis-core".to_owned()],
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
    let (items, skipped) = plan(&touched, tools(), mode);
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
    let mut failed: Vec<String> = Vec::new();
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
            Action::Coverage(crates) => crate::coverage::run_for(crates)? == 0,
        };
        let took = fmt_duration(started.elapsed());
        println!("check: [{}/{total}] {} ({took})", i + 1, item.name);
        if !ok {
            // The quick tier lists every error: it goes on to the next step.
            failed.push(format!("{} ({took})", item.name));
            if mode != Mode::Quick {
                break;
            }
            continue;
        }
        ran.push(format!("{} ({took})", item.name));
    }
    if !failed.is_empty() {
        println!(
            "check: FAILED at {}; ran: {}{partial}",
            failed.join(", "),
            ran.join(", ")
        );
        return Ok(1);
    }
    if let Some(step) = from {
        println!(
            "check: partial, not a pass: ran from {step}: {}; skipped by --from: {}",
            ran.join(", "),
            before.join(", ")
        );
    } else {
        println!(
            "check: passed ({} tier; the gate also runs: {}): {}",
            mode.name(),
            gate_also_runs(mode).join(", "),
            ran.join(", ")
        );
        match mode {
            Mode::Quick => println!("check: next: cargo xtask check"),
            Mode::Default => {
                println!("check: next: cargo xtask check --full, once before the final commit");
            }
            Mode::Full => {}
        }
    }
    for s in &skipped {
        println!("check: skipped: {s}");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{
        Action, BIG_ENDIAN_ENV, COVERED, GATE_ENV, GATE_ONLY, Mode, Options, SEPARATE_WORKSPACES,
        Tools, Touched, fmt_duration, from_index, gate_also_runs, parse_args, plan, touched,
    };
    use crate::verify::{BIG_ENDIAN, CHECK_ENV, CLIPPY, FMT, TEST, all_checks};
    use std::time::Duration;

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(t: &Touched, tools: Tools, mode: Mode) -> (Vec<&'static str>, Vec<String>) {
        let (items, skipped) = plan(t, tools, mode);
        (items.iter().map(|i| i.name).collect(), skipped)
    }

    const ALL: Tools = Tools {
        nightly: true,
        coverage: true,
        big_endian: true,
        cbindgen: true,
    };
    const NONE: Tools = Tools {
        nightly: false,
        coverage: false,
        big_endian: false,
        cbindgen: false,
    };

    /// A change that touches every directory with extra checks.
    fn everything() -> Touched {
        Touched {
            exerciser: true,
            crates: vec!["aldis-core".to_owned(), "aldis-ffi".to_owned()],
            fuzz: true,
            crosscheck: true,
            ffi: true,
            refapp: true,
            separate: SEPARATE_WORKSPACES.to_vec(),
        }
    }

    fn opts(mode: Mode, from: Option<&str>) -> Options<'_> {
        Options { mode, from }
    }

    #[test]
    fn durations_print_as_minutes_seconds_or_tenths() {
        // req: VER-068
        assert_eq!(fmt_duration(Duration::from_secs(102)), "1m42s");
        assert_eq!(fmt_duration(Duration::from_secs(60)), "1m00s");
        assert_eq!(fmt_duration(Duration::from_secs(42)), "42s");
        assert_eq!(fmt_duration(Duration::from_millis(340)), "0.3s");
    }

    #[test]
    fn the_command_line_picks_a_tier_and_a_step() {
        // req: VER-068
        // req: VER-069
        assert_eq!(parse_args(&[]), Ok(opts(Mode::Default, None)));
        assert_eq!(parse_args(&["--quick"]), Ok(opts(Mode::Quick, None)));
        assert_eq!(parse_args(&["--full"]), Ok(opts(Mode::Full, None)));
        assert_eq!(
            parse_args(&["--from", "test"]),
            Ok(opts(Mode::Default, Some("test")))
        );
        assert_eq!(
            parse_args(&["--full", "--from", "test"]),
            Ok(opts(Mode::Full, Some("test")))
        );
        assert_eq!(
            parse_args(&["--from", "fmt", "--quick"]),
            Ok(opts(Mode::Quick, Some("fmt")))
        );
        assert!(parse_args(&["--from"]).is_err());
        assert!(parse_args(&["--quick", "--full"]).is_err());
        assert!(parse_args(&["--full", "--full"]).is_err());
        assert!(parse_args(&["--from", "a", "--from", "b"]).is_err());
        assert!(parse_args(&["--slow"]).is_err());
    }

    #[test]
    fn from_names_a_step_of_the_plan() {
        // req: VER-069
        let (n, _) = names(&Touched::default(), NONE, Mode::Default);
        assert_eq!(from_index(&n, "fmt"), Ok(0));
        assert_eq!(from_index(&n, "test"), Ok(3));
        let err = from_index(&n, "tset").unwrap_err();
        assert!(
            err.contains("`tset`") && err.contains("fmt, clippy, doc, test"),
            "{err}"
        );
        // A step of another tier is not a step of this plan.
        let (quick, _) = names(&Touched::default(), NONE, Mode::Quick);
        assert!(from_index(&quick, "test").is_err());
        assert_eq!(from_index(&quick, "check"), Ok(4));
    }

    #[test]
    fn paths_select_directories() {
        assert_eq!(touched(&paths(&["docs/x.md"])), Touched::default());
        assert_eq!(
            touched(&paths(&["crates/aldis-spp/a.rs"])),
            Touched {
                exerciser: true,
                crates: vec!["aldis-spp".to_owned()],
                fuzz: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            touched(&paths(&["crates/aldis-core/src/lib.rs"])),
            Touched {
                exerciser: true,
                crates: vec!["aldis-core".to_owned()],
                fuzz: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            touched(&paths(&[
                "crates/aldis-ffi/src/lib.rs",
                "crates/aldis-ffi/tests/a.rs",
                "refapp/Cargo.toml"
            ])),
            Touched {
                exerciser: true,
                crates: vec!["aldis-ffi".to_owned()],
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
        // A sibling whose name only starts alike is another crate, and a file directly under
        // `crates/` is no crate.
        assert_eq!(
            touched(&paths(&["crates/aldis-core-x/a.rs", "refapp-x/a"])).crates,
            ["aldis-core-x"]
        );
        assert_eq!(
            touched(&paths(&["crates/README.md"])).crates,
            <[String; 0]>::default()
        );
    }

    #[test]
    fn base_steps_come_first_and_alone() {
        let (n, skipped) = names(&Touched::default(), NONE, Mode::Default);
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
        // The full tier adds nothing for a change that touches no crate.
        assert_eq!(names(&Touched::default(), ALL, Mode::Full).0, n);
    }

    #[test]
    fn the_quick_tier_formats_and_compiles_the_four_workspaces() {
        // req: VER-068
        // The same steps whatever the change touches, and whatever tools are installed.
        let (n, skipped) = names(&everything(), ALL, Mode::Quick);
        assert_eq!(
            n,
            [
                "fmt",
                "exerciser fmt",
                "fuzz fmt",
                "crosscheck fmt",
                "check",
                "exerciser check",
                "fuzz check",
                "crosscheck check"
            ]
        );
        assert_eq!(names(&Touched::default(), NONE, Mode::Quick).0, n);
        assert_eq!(skipped, <[String; 0]>::default());
        let (items, _) = plan(&Touched::default(), NONE, Mode::Quick);
        for item in &items {
            if let Action::Cmd { args, .. } = &item.action
                && args.first() == Some(&"check")
            {
                // One run lists every error of the workspace.
                assert!(args.contains(&"--keep-going") && args.contains(&"--locked"));
            }
        }
    }

    #[test]
    fn only_the_full_tier_runs_coverage_stack_and_big_endian() {
        // req: VER-068
        let (default, _) = names(&everything(), ALL, Mode::Default);
        for step in ["xtask coverage", "xtask stack", "big-endian"] {
            assert!(!default.contains(&step), "{step}");
        }
        let (full, skipped) = names(&everything(), ALL, Mode::Full);
        for step in ["xtask coverage", "xtask stack", "big-endian"] {
            assert!(full.contains(&step), "{step}");
        }
        assert_eq!(skipped, <[String; 0]>::default());
        // Every step of the default tier is in the full tier, in the same order.
        let mut rest = full.iter();
        assert!(default.iter().all(|d| rest.any(|f| f == d)));
    }

    #[test]
    fn the_full_tier_follows_the_touched_directories() {
        // req: VER-068
        // A crate change reaches coverage, stack, and big-endian; a change of `crosscheck/` alone
        // reaches none of them.
        let cross = Touched {
            crosscheck: true,
            ..Touched::default()
        };
        let (n, _) = names(&cross, ALL, Mode::Full);
        assert!(!n.contains(&"xtask coverage") && !n.contains(&"xtask stack"));
        assert!(!n.contains(&"big-endian"));
        // The exerciser alone reaches stack and big-endian, not coverage.
        let exerciser = Touched {
            exerciser: true,
            ..Touched::default()
        };
        let (n, _) = names(&exerciser, ALL, Mode::Full);
        assert!(n.contains(&"xtask stack") && n.contains(&"big-endian"));
        assert!(!n.contains(&"xtask coverage"));
    }

    #[test]
    fn a_missing_tool_skips_a_full_step_with_the_way_to_install_it() {
        // req: VER-068
        let (n, skipped) = names(&everything(), NONE, Mode::Full);
        for step in ["xtask coverage", "xtask stack", "big-endian"] {
            assert!(!n.contains(&step), "{step}");
        }
        assert_eq!(skipped.len(), 4, "{skipped:?}");
        assert!(
            skipped
                .iter()
                .any(|s| s.starts_with("big-endian: ") && s.contains("rustup target add")),
            "{skipped:?}"
        );
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
        let (n, _) = names(&t, NONE, Mode::Default);
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
    fn every_step_runs_under_the_gates_flags() {
        // req: VER-068
        assert!(GATE_ENV.iter().all(|e| CHECK_ENV.contains(e)));
        assert!(
            BIG_ENDIAN_ENV
                .iter()
                .all(|e| { CHECK_ENV.contains(e) || e.0.starts_with("CARGO_TARGET_S390X_") })
        );
        for mode in [Mode::Quick, Mode::Default, Mode::Full] {
            let (items, _) = plan(&everything(), ALL, mode);
            for item in &items {
                if let Action::Cmd { env, .. } = &item.action {
                    let wanted = if item.name == "big-endian" {
                        BIG_ENDIAN_ENV
                    } else {
                        GATE_ENV
                    };
                    assert_eq!(*env, wanted, "step `{}` lacks the gate's flags", item.name);
                }
            }
        }
    }

    #[test]
    fn the_big_endian_step_exports_what_the_gate_exports() {
        // req: VER-068
        for line in BIG_ENDIAN.lines().filter_map(|l| l.strip_prefix("export ")) {
            let (key, value) = line.split_once('=').expect("export KEY=VALUE");
            let value = value.trim_matches('\'');
            assert!(
                BIG_ENDIAN_ENV.contains(&(key, value)),
                "the gate exports {key}={value}, the step does not"
            );
        }
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
        let (items, _) = plan(&everything(), ALL, Mode::Full);
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
                Action::Xtask(_) | Action::Coverage(_) => None,
            })
            .collect();
        for script in [FMT, CLIPPY, TEST, BIG_ENDIAN] {
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
    fn every_gate_check_is_covered_or_gate_only() {
        // req: VER-068
        let gate: Vec<String> = all_checks().into_iter().map(|c| c.name).collect();
        let covered = |n: &str| COVERED.iter().any(|(c, _, _)| *c == n);
        let only = |n: &str| GATE_ONLY.iter().any(|(c, _)| *c == n);
        for name in &gate {
            assert!(
                covered(name) != only(name),
                "the gate check `{name}` of `cargo xtask verify list` must be in exactly one of \
                 COVERED (a step of `check --full` runs it) and GATE_ONLY (with the reason)"
            );
        }
        // No entry names a check that is gone.
        for name in COVERED
            .iter()
            .map(|(c, _, _)| c)
            .chain(GATE_ONLY.iter().map(|(c, _)| c))
        {
            assert!(gate.iter().any(|g| g == name), "`{name}` is no gate check");
        }
        for (name, reason) in GATE_ONLY {
            assert!(!reason.is_empty(), "no reason for `{name}`");
        }
    }

    #[test]
    fn the_covered_steps_exist_in_the_tier_that_claims_them() {
        // req: VER-068
        for (check, steps, first) in COVERED {
            let (n, _) = names(&everything(), ALL, *first);
            for step in *steps {
                assert!(
                    n.contains(step),
                    "`{check}`: no step `{step}` in the {first:?} tier"
                );
            }
            // And not earlier: the tier is the first that runs it.
            if *first > Mode::Quick {
                let earlier = if *first == Mode::Full {
                    Mode::Default
                } else {
                    Mode::Quick
                };
                let (n, _) = names(&everything(), ALL, earlier);
                assert!(
                    !steps.iter().all(|s| n.contains(s)),
                    "`{check}` already runs in the {earlier:?} tier"
                );
            }
        }
    }

    #[test]
    fn the_last_line_names_what_the_gate_also_runs() {
        // req: VER-068
        let quick = gate_also_runs(Mode::Quick);
        let default = gate_also_runs(Mode::Default);
        let full = gate_also_runs(Mode::Full);
        for name in ["clippy", "test", "coverage", "stack", "big-endian", "spec"] {
            assert!(quick.iter().any(|n| n == name), "quick: {name}");
        }
        assert!(!quick.iter().any(|n| n == "fmt"));
        assert!(!default.iter().any(|n| n == "clippy" || n == "test"));
        for name in ["coverage", "stack", "big-endian", "spec"] {
            assert!(default.iter().any(|n| n == name), "default: {name}");
        }
        assert!(
            !full
                .iter()
                .any(|n| n == "coverage" || n == "stack" || n == "big-endian")
        );
        assert!(full.iter().any(|n| n == "spec"));
        // A non-gating or nightly-only check is not something the gate fails on.
        assert!(!full.iter().any(|n| n == "kani" || n == "rust-sparc-leon3"));
    }

    #[test]
    fn xtask_is_a_member_of_the_workspace_that_check_tests() {
        // req: VER-068
        // `cargo test --workspace` compiles and runs the tests of xtask, so that an error in them
        // fails `check` and not only the gate.
        let manifest = std::fs::read_to_string(crate::repo::root().join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("\"xtask\""),
            "xtask is not a workspace member"
        );
    }

    #[test]
    fn extras_follow_the_touched_directories() {
        let t = Touched {
            exerciser: true,
            crates: vec!["aldis-core".to_owned()],
            fuzz: true,
            crosscheck: true,
            ffi: true,
            refapp: true,
            ..Touched::default()
        };
        let (n, skipped) = names(&t, ALL, Mode::Default);
        assert_eq!(n.len(), 6 + 2 + 1 + 1 + 2 + 4);
        assert!(n.contains(&"xtask header c-smoke") && !n.contains(&"xtask coverage"));
        assert_eq!(skipped.len(), 0);
    }

    #[test]
    fn missing_tools_skip_with_a_reason() {
        let t = Touched {
            exerciser: true,
            crates: vec!["aldis-core".to_owned()],
            fuzz: true,
            ffi: true,
            refapp: false,
            ..Touched::default()
        };
        let (n, skipped) = names(&t, NONE, Mode::Default);
        assert!(!n.contains(&"xtask header"));
        assert!(n.contains(&"exerciser clippy") && n.contains(&"fuzz clippy"));
        assert_eq!(skipped.len(), 1);
    }
}
