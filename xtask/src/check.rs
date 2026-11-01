//! `xtask check`: the checks to run while working, in one command (AGENTS.md).
//!
//! Three tiers (VER-068); the gate (`cargo xtask verify`) stays the only authority on a merge.
//!
//! - `check --quick`: format of the four workspaces and `cargo check --all-targets` of each, which
//!   runs every step and prints every error, for the edit loop (about a minute).
//! - `check` (default): format, clippy, rustdoc, tests, the bare-metal clippy build, and
//!   `xtask ci`, then the extras that the verification run would run for the change. Stops at the
//!   first failure. An extra whose tool is not installed is skipped and reported. It does not
//!   promise the gate.
//! - `check --full`: the default tier plus coverage, `stack`, and `big-endian` when the
//!   verification run would run them for the change, once before the final commit.
//!
//! Which extras and which of the `--full` steps run is decided by the gate's own data, not by a
//! list of directories here: a step runs when `verify::inputs_differ` (the rule of VER-056) says
//! that the changed paths make the check it mirrors run. The changed paths are those that differ
//! from `origin/main`, committed or not.
//!
//! One run at a time per worktree (`check_lock`). `check --from <step>` skips the steps before the
//! named one, to go on after a failure without redoing what passed; such a run ends `partial`,
//! never `passed` (VER-069).
//!
//! Every step runs under the environment of the gate's checks (`gate_env`, derived from
//! `verify::CHECK_ENV`): a step that is an xtask check runs as a child process of this binary, so
//! that every tool it spawns inherits the flags. The tests `every_gate_line_has_a_step` and
//! `every_gate_check_is_covered_or_gate_only` fail when a line of a covered check's script has no
//! step, or when a check of `cargo xtask verify list` is neither covered by a step of `--full`
//! (`COVERED`) nor named with a reason in `GATE_ONLY` (VER-068).

use std::process::Command;
use std::time::{Duration, Instant};

use crate::verify::{CHECK_ENV, all_checks, inputs_differ};
use crate::{check_lock, repo};

/// What a step does.
enum Action {
    /// Run a program in a directory below the repository root (`""` for the root).
    Cmd {
        program: &'static str,
        args: &'static [&'static str],
        dir: &'static str,
    },
    /// Run this binary with `args`: an xtask check, in a process of its own so that it and the
    /// tools it spawns get the environment of every other step (`command`).
    Xtask(Vec<String>),
}

struct Item {
    name: &'static str,
    action: Action,
    /// Variables beyond the gate's flags: the exports of the check's script.
    env: Vec<(String, String)>,
    /// How the step differs from the whole of its gate check, shown in the last line.
    note: Option<String>,
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
        env: Vec::new(),
        note: None,
    }
}

const fn cmd(name: &'static str, program: &'static str, args: &'static [&'static str]) -> Item {
    cmd_in(name, "", program, args)
}

fn xtask(name: &'static str, args: &[&str]) -> Item {
    Item {
        name,
        action: Action::Xtask(args.iter().map(|a| (*a).to_owned()).collect()),
        env: Vec::new(),
        note: None,
    }
}

/// The variables of the gate's `CHECK_ENV` that `check` does not set, each with the reason. A
/// variable of `CHECK_ENV` that is not listed here is set for every step, and
/// `the_gates_environment_is_what_check_sets` fails for one it has not seen, so that a new
/// variable needs a decision.
const NOT_LOCAL: &[(&str, &str)] = &[("CARGO_TERM_COLOR", "only changes how the output looks")];

/// The environment of every step: the gate's `CHECK_ENV` without `NOT_LOCAL`. `RUSTFLAGS` makes
/// a warning an error, as it is at the gate, so that an `unexpected cfg` warning in a test module
/// fails here and not only in the `big-endian` check; `RUSTDOCFLAGS` does the same for a link to
/// a private item. The flags are part of cargo's cache key: the first run after a change of them
/// rebuilds (VER-068).
fn gate_env() -> Vec<(&'static str, &'static str)> {
    CHECK_ENV
        .iter()
        .filter(|(k, _)| !NOT_LOCAL.iter().any(|(n, _)| n == k))
        .copied()
        .collect()
}

/// The command that runs `item`, with the gate's environment and the item's own.
fn command(item: &Item) -> Result<Command, String> {
    let mut c = match &item.action {
        Action::Cmd { program, args, dir } => {
            let mut c = Command::new(program);
            c.args(*args).current_dir(repo::root().join(dir));
            c
        }
        Action::Xtask(args) => {
            let exe = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
            let mut c = Command::new(exe);
            c.args(args).current_dir(repo::root());
            c
        }
    };
    c.envs(gate_env());
    c.envs(item.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    Ok(c)
}

/// How much of the gate a run covers, from the least to the most (VER-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    /// Format and compile of the four workspaces, every error at once.
    Quick,
    /// Format, lints, rustdoc, tests, the bare-metal build, `xtask ci`, and the extras that the
    /// verification run would run for the change.
    Default,
    /// The default tier plus coverage, `stack`, and `big-endian`.
    Full,
}

/// The workspaces that `cargo fmt --all` at the root does not reach, each with its own manifest,
/// and the name of the step that formats it. The gate formats the same directories (`FMT` of
/// `verify/checks.rs`).
const SEPARATE_WORKSPACES: [(&str, &str); 3] = [
    ("exerciser", "exerciser fmt"),
    ("fuzz", "fuzz fmt"),
    ("crosscheck", "crosscheck fmt"),
];

/// What a change makes the verification run do.
#[derive(Debug, Default, PartialEq, Eq)]
struct Touched {
    /// The checks of the verification run that the changed paths make it run (VER-056).
    gate: Vec<String>,
    /// The crates that coverage measures for the change, or `None` for every crate.
    coverage: Option<Vec<String>>,
}

impl Touched {
    /// Whether the verification run would run the check `name` for the change.
    fn gate_runs(&self, name: &str) -> bool {
        self.gate.iter().any(|g| g == name)
    }
}

/// What the paths `changed` (repository-relative) make the verification run do.
fn touched(changed: &[String]) -> Touched {
    let checks = all_checks();
    let gate = checks
        .iter()
        .filter(|c| inputs_differ(&checks, c, changed).is_some())
        .map(|c| c.name.clone())
        .collect();
    // The crates whose source is all that coverage reads among the changed paths; any other path
    // that the coverage check reads makes it measure every crate.
    let coverage = checks
        .iter()
        .find(|c| c.name == "coverage")
        .and_then(|cov| {
            let mut crates = Vec::new();
            for p in changed {
                if inputs_differ(&checks, cov, std::slice::from_ref(p)).is_none() {
                    continue;
                }
                let (name, _) = p.strip_prefix("crates/")?.split_once('/')?;
                crates.push(name.to_owned());
            }
            crates.sort();
            crates.dedup();
            crate::coverage::select(
                &crates,
                &crate::layers::gated().ok()?,
                crate::coverage::REPORTED,
            )
        });
    Touched { gate, coverage }
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
    /// `shellcheck` is installed (`shell`).
    shellcheck: bool,
}

/// A step left out because its tool is not installed.
struct Skip {
    /// The step.
    step: &'static str,
    /// The check of the verification run it mirrors.
    check: &'static str,
    /// Why, in a few words, for the last line.
    short: &'static str,
    /// Why, and what to install.
    reason: String,
}

/// The steps to run and the steps skipped.
struct Plan {
    items: Vec<Item>,
    skipped: Vec<Skip>,
}

/// What `check --quick` runs: the format of the four workspaces and `cargo check` of each,
/// whatever the change touches. The compile steps keep going after an error (`--keep-going`),
/// and the run goes on after a failed step, so that one run lists every error. The exerciser is
/// a `no_std` binary: it is checked for the target it is linked for, as its clippy step is.
/// `refapp/` is a workspace of its own that the quick tier does not check (`run` says so).
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

/// The `export NAME=value` lines of a check's script, as the variables it sets.
fn exports(script: &str) -> Vec<(String, String)> {
    script
        .lines()
        .filter_map(|l| l.strip_prefix("export ")?.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.trim_matches('\'').to_owned()))
        .collect()
}

/// The steps to run for `t` and the ones skipped, each with the reason.
///
/// The base steps run for every change. The others run when the verification run would run the
/// check they mirror for the change (`Touched::gate_runs`); `--full` adds coverage, `stack`, and
/// `big-endian` on the same rule.
#[expect(clippy::too_many_lines, reason = "one entry per check step")]
fn plan(t: &Touched, tools: Tools, mode: Mode) -> Plan {
    if mode == Mode::Quick {
        return Plan {
            items: quick_plan(),
            skipped: Vec::new(),
        };
    }
    let mut run = vec![cmd("fmt", "cargo", &["fmt", "--all", "--check"])];
    if t.gate_runs("fmt") {
        run.extend(
            SEPARATE_WORKSPACES
                .iter()
                .map(|(dir, name)| cmd_in(name, dir, "cargo", &["fmt", "--all", "--check"])),
        );
    }
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
        xtask("xtask ci", &["ci"]),
    ]);
    let mut skipped = Vec::new();
    if t.gate_runs("exerciser-thumbv7em-none-eabihf") {
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
    if t.gate_runs("clippy") {
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
    if t.gate_runs("shell") {
        if tools.shellcheck {
            run.push(cmd("shell", "scripts/check-shell", &[]));
        } else {
            skipped.push(Skip {
                step: "shell",
                check: "shell",
                short: "shellcheck not installed",
                reason: "shellcheck not installed (brew install shellcheck; Debian: apt install \
                         shellcheck)"
                    .into(),
            });
        }
    }
    if t.gate_runs("header") {
        if tools.cbindgen {
            run.push(xtask("xtask header", &["header"]));
            run.push(xtask(
                "xtask header c-smoke",
                &["header", "c-smoke", "cc", "c++"],
            ));
        } else {
            skipped.push(Skip {
                step: "xtask header",
                check: "header",
                short: "cbindgen not installed",
                reason: "cbindgen not installed (cargo install --locked cbindgen@0.29.4); \
                         this also skips xtask header c-smoke"
                    .into(),
            });
        }
    }
    if t.gate_runs("refapp") {
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
    if mode == Mode::Full {
        full_steps(t, tools, &mut run, &mut skipped);
    }
    Plan {
        items: run,
        skipped,
    }
}

/// The steps that only `--full` runs: coverage, `stack`, and `big-endian`, each when the
/// verification run would run its check for the change, and skipped with the reason when its tool
/// is missing.
fn full_steps(t: &Touched, tools: Tools, run: &mut Vec<Item>, skipped: &mut Vec<Skip>) {
    if t.gate_runs("coverage") {
        if tools.coverage {
            let mut item = xtask("xtask coverage", &["coverage"]);
            if let (Some(crates), Action::Xtask(args)) = (&t.coverage, &mut item.action) {
                for c in crates {
                    args.extend(["-p".to_owned(), c.clone()]);
                }
                item.note = Some(format!("scoped to {}", crates.join(", ")));
            }
            run.push(item);
        } else {
            skipped.push(Skip {
                step: "xtask coverage",
                check: "coverage",
                short: "tooling nightly or cargo-llvm-cov not installed",
                reason: "tooling nightly or cargo-llvm-cov not installed".into(),
            });
        }
    }
    if t.gate_runs("stack") {
        if tools.nightly {
            run.push(xtask("xtask stack", &["stack"]));
        } else {
            skipped.push(Skip {
                step: "xtask stack",
                check: "stack",
                short: "tooling nightly not installed",
                reason: "tooling nightly not installed".into(),
            });
        }
    }
    if t.gate_runs("big-endian") {
        if tools.big_endian {
            let script = all_checks()
                .into_iter()
                .find(|c| c.name == "big-endian")
                .map(|c| c.script)
                .unwrap_or_default();
            let mut item = cmd(
                "big-endian",
                "cargo",
                &[
                    "test",
                    "--locked",
                    "--workspace",
                    "--all-features",
                    "--target",
                    "s390x-unknown-linux-gnu",
                ],
            );
            item.env = exports(&script);
            run.push(item);
        } else {
            skipped.push(Skip {
                step: "big-endian",
                check: "big-endian",
                short: "s390x tools not installed",
                reason: "needs the s390x-unknown-linux-gnu target, s390x-linux-gnu-gcc-14, and \
                         qemu-s390x (rustup target add s390x-unknown-linux-gnu; Debian: apt \
                         install gcc-14-s390x-linux-gnu qemu-user)"
                    .into(),
            });
        }
    }
}

/// The gate checks that `check` runs in full, each with the steps that run all its lines and the
/// first tier that runs them all. A step is scheduled only when the verification run would run
/// the check for the change (VER-056), so a covered check that the change does not make run is
/// not something the gate also runs.
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
    ("shell", &["shell"], Mode::Default),
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
        "partly: check runs the header and the C smoke with the host `cc`, not gcc-14 and clang",
    ),
    (
        "no-std-thumbv7em-none-eabihf",
        "partly: check runs only its clippy line (`clippy (thumbv7em)`)",
    ),
    (
        "no-std-armv7r-none-eabihf",
        "not mirrored: only the thumbv7em clippy line runs, so a failure that is specific to this target is found only by the gate",
    ),
    (
        "no-std-riscv32imac-unknown-none-elf",
        "not mirrored: only the thumbv7em clippy line runs, so a failure that is specific to this target is found only by the gate",
    ),
    (
        "layers",
        "partly: `xtask ci` runs `layers` and `refapp-deps`, not `symbols`",
    ),
    (
        "refapp",
        "partly: check runs its fmt, clippy, tests, and flight build, not the scenario and golden checks",
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
        "not mirrored: the build, clippy, canary, and QEMU run for this target; the thumbv7em steps check the same code for another target",
    ),
    (
        "exerciser-riscv32imac-unknown-none-elf",
        "not mirrored: the build, clippy, canary, and QEMU run for this target; the thumbv7em steps check the same code for another target",
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

/// The gating checks of the verification run that the change makes it run and that a run of
/// `mode` with this `plan` does not run in full, for the last line: a check of `GATE_ONLY`; a
/// covered check of a later tier; a covered check whose step was skipped, with the reason; and a
/// covered check whose step is scoped, with the scope.
fn gate_also_runs(mode: Mode, t: &Touched, plan: &Plan) -> Vec<String> {
    let gating: Vec<String> = all_checks()
        .into_iter()
        .filter(|c| c.gating && !c.nightly)
        .map(|c| c.name)
        .collect();
    let mut out = Vec::new();
    let named = |n: &str| gating.iter().any(|g| g == n) && t.gate_runs(n);
    let skip_of = |n: &str| plan.skipped.iter().find(|s| s.check == n);
    for (name, steps, first) in COVERED.iter().filter(|(n, _, _)| named(n)) {
        let ran: Vec<&Item> = plan
            .items
            .iter()
            .filter(|i| steps.contains(&i.name))
            .collect();
        if mode < *first {
            out.push((*name).to_owned());
        } else if let Some(s) = skip_of(name) {
            out.push(format!("{name} (skipped: {})", s.short));
        } else if let Some(note) = ran.iter().find_map(|i| i.note.as_deref()) {
            out.push(format!("{name} ({note})"));
        } else if ran.len() < steps.len() {
            out.push((*name).to_owned());
        }
    }
    for (name, _) in GATE_ONLY.iter().filter(|(n, _)| named(n)) {
        out.push(skip_of(name).map_or_else(
            || (*name).to_owned(),
            |s| format!("{name} (skipped: {})", s.short),
        ));
    }
    out
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
/// `None` when `origin/main` is unknown or no path differs: the verification run then runs every
/// check (VER-056), and so does `check`.
fn changed_paths() -> Option<Vec<String>> {
    let mut paths = git_lines(&["diff", "--name-only", "origin/main...HEAD"])?;
    paths.extend(git_lines(&["diff", "--name-only", "HEAD"])?);
    paths.extend(git_lines(&["ls-files", "--others", "--exclude-standard"])?);
    (!paths.is_empty()).then_some(paths)
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
        shellcheck: works("shellcheck", &["--version"]),
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
                    return Err(format!("check: --from needs a step name\n{USAGE}"));
                };
                rest = tail;
                if from.replace(*step).is_some() {
                    return Err(format!("check: --from at most once\n{USAGE}"));
                }
            }
            other => return Err(format!("check: unknown argument `{other}`\n{USAGE}")),
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
                gate: all_checks().into_iter().map(|c| c.name).collect(),
                coverage: None,
            },
            Some("origin/main unknown or no path differs: running what every check needs"),
        ),
    };
    if let Some(note) = note {
        println!("check: {note}");
    }
    let plan = plan(&touched, tools(), mode);
    let items = &plan.items;
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
        let ok = command(item)?
            .status()
            .map_err(|e| format!("{}: {e}", item.name))?
            .success();
        let took = fmt_duration(started.elapsed());
        println!("check: [{}/{total}] {} ({took})", i + 1, item.name);
        let label = item
            .note
            .as_ref()
            .map_or_else(String::new, |n| format!(" ({n})"));
        if !ok {
            // The quick tier lists every error: it goes on to the next step.
            failed.push(format!("{}{label} ({took})", item.name));
            if mode != Mode::Quick {
                break;
            }
            continue;
        }
        ran.push(format!("{}{label} ({took})", item.name));
    }
    if !failed.is_empty() {
        println!(
            "check: FAILED at {}; ran: {}{partial}",
            failed.join(", "),
            ran.join(", ")
        );
        return Ok(1);
    }
    Ok(verdict(mode, from, &touched, &plan, &ran, &before))
}

/// Prints what a run that passed skipped and the verdict, which is the last line; `0`.
fn verdict(
    mode: Mode,
    from: Option<&str>,
    touched: &Touched,
    plan: &Plan,
    ran: &[String],
    before: &[&str],
) -> usize {
    if let Some(step) = from {
        for s in &plan.skipped {
            println!("check: skipped: {}: {}", s.step, s.reason);
        }
        println!(
            "check: partial, not a pass: ran from {step}: {}; skipped by --from: {}",
            ran.join(", "),
            before.join(", ")
        );
        return 0;
    }
    // The last line is the verdict: what the run skipped and what to run next come before it.
    for s in &plan.skipped {
        println!("check: skipped: {}: {}", s.step, s.reason);
    }
    match mode {
        Mode::Quick => {
            println!("check: not checked by the quick tier: refapp/ (a workspace of its own)");
            println!("check: next: cargo xtask check");
        }
        Mode::Default => {
            println!("check: next: cargo xtask check --full, once before the final commit");
        }
        Mode::Full => {}
    }
    let also = gate_also_runs(mode, touched, plan).join(", ");
    let steps = ran.join(", ");
    match mode {
        Mode::Quick => {
            println!("check: quick tier ok (not a gate pass; the gate also runs: {also}): {steps}");
        }
        Mode::Default => {
            println!(
                "check: default tier ok (not a gate pass; the gate also runs: {also}): {steps}"
            );
        }
        Mode::Full if plan.skipped.is_empty() => {
            println!("check: passed (full tier; the gate also runs: {also}): {steps}");
        }
        Mode::Full => {
            println!("check: passed with skips (full tier; the gate also runs: {also}): {steps}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{
        Action, COVERED, GATE_ONLY, Item, Mode, NOT_LOCAL, Options, Plan, SEPARATE_WORKSPACES,
        Tools, Touched, command, exports, fmt_duration, from_index, gate_also_runs, gate_env,
        parse_args, plan, touched,
    };
    use crate::verify::{CHECK_ENV, all_checks};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(t: &Touched, tools: Tools, mode: Mode) -> Vec<&'static str> {
        plan(t, tools, mode).items.iter().map(|i| i.name).collect()
    }

    const ALL: Tools = Tools {
        nightly: true,
        coverage: true,
        big_endian: true,
        cbindgen: true,
        shellcheck: true,
    };
    const NONE: Tools = Tools {
        nightly: false,
        coverage: false,
        big_endian: false,
        cbindgen: false,
        shellcheck: false,
    };

    /// A change that makes the verification run run every check.
    fn everything() -> Touched {
        Touched {
            gate: all_checks().into_iter().map(|c| c.name).collect(),
            coverage: None,
        }
    }

    /// The steps for a change of the files `changed` alone.
    fn steps_for(changed: &[&str], tools: Tools, mode: Mode) -> Vec<&'static str> {
        names(&touched(&paths(changed)), tools, mode)
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
    }

    #[test]
    fn an_unknown_flag_is_named() {
        // req: VER-068
        let err = parse_args(&["--quick", "--slow"]).unwrap_err();
        assert!(err.contains("`--slow`") && err.contains("usage:"), "{err}");
        let err = parse_args(&["--from"]).unwrap_err();
        assert!(err.contains("--from needs a step name"), "{err}");
    }

    #[test]
    fn from_names_a_step_of_the_plan() {
        // req: VER-069
        let n = names(&Touched::default(), NONE, Mode::Default);
        assert_eq!(from_index(&n, "fmt"), Ok(0));
        assert_eq!(from_index(&n, "test"), Ok(3));
        let err = from_index(&n, "tset").unwrap_err();
        assert!(
            err.contains("`tset`") && err.contains("fmt, clippy, doc, test"),
            "{err}"
        );
        // A step of another tier is not a step of this plan.
        let quick = names(&Touched::default(), NONE, Mode::Quick);
        assert!(from_index(&quick, "test").is_err());
        assert_eq!(from_index(&quick, "check"), Ok(4));
    }

    #[test]
    fn base_steps_come_first_and_alone_for_a_change_of_documentation() {
        // req: VER-068
        // Documentation is read by `spec` alone: no step beyond the base ones, in any tier.
        let docs = touched(&paths(&["docs/x.md"]));
        assert_eq!(docs.gate, ["spec"]);
        let base = [
            "fmt",
            "clippy",
            "doc",
            "test",
            "clippy (thumbv7em)",
            "xtask ci",
        ];
        assert_eq!(names(&docs, ALL, Mode::Default), base);
        assert_eq!(names(&docs, ALL, Mode::Full), base);
        assert_eq!(names(&Touched::default(), NONE, Mode::Default), base);
    }

    #[test]
    fn the_quick_tier_formats_and_compiles_the_four_workspaces() {
        // req: VER-068
        // The same steps whatever the change touches, and whatever tools are installed.
        let n = names(&everything(), ALL, Mode::Quick);
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
        assert_eq!(names(&Touched::default(), NONE, Mode::Quick), n);
        let p = plan(&Touched::default(), NONE, Mode::Quick);
        assert_eq!(p.skipped.len(), 0);
        for item in &p.items {
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
        let default = names(&everything(), ALL, Mode::Default);
        for step in ["xtask coverage", "xtask stack", "big-endian"] {
            assert!(!default.contains(&step), "{step}");
        }
        let full = names(&everything(), ALL, Mode::Full);
        for step in ["xtask coverage", "xtask stack", "big-endian"] {
            assert!(full.contains(&step), "{step}");
        }
        assert_eq!(plan(&everything(), ALL, Mode::Full).skipped.len(), 0);
        // Every step of the default tier is in the full tier, in the same order.
        let mut rest = full.iter();
        assert!(default.iter().all(|d| rest.any(|f| f == d)));
    }

    #[test]
    fn the_steps_follow_the_gates_inputs_not_a_list_of_directories() {
        // req: VER-068
        // Each of these files is read by checks of the verification run that no directory test
        // would find: the steps that mirror those checks must run for it, and only those.
        let step_for = |file: &str, step: &str| steps_for(&[file], ALL, Mode::Full).contains(&step);
        // coverage-waivers.toml: `RUST` inputs (coverage, big-endian), not `STACK`.
        assert!(step_for("coverage-waivers.toml", "xtask coverage"));
        assert!(step_for("coverage-waivers.toml", "big-endian"));
        assert!(!step_for("coverage-waivers.toml", "xtask stack"));
        // test-vectors/: `RUST` and `STACK`.
        for step in ["xtask coverage", "xtask stack", "big-endian"] {
            assert!(step_for("test-vectors/x", step), "test-vectors: {step}");
        }
        // The files that the whole run reads: `RUN`.
        for file in ["Cargo.lock", "rust-toolchain.toml", "xtask/src/x.rs"] {
            for step in [
                "xtask coverage",
                "xtask stack",
                "big-endian",
                "fuzz clippy",
                "crosscheck clippy",
                "exerciser clippy",
                "xtask header",
                "refapp test",
                "exerciser fmt",
            ] {
                assert!(step_for(file, step), "{file}: {step}");
            }
        }
        // And the same derivation, check by check: a covered check's steps are scheduled exactly
        // when `all_checks()` says its inputs contain a changed path.
        let checks = all_checks();
        for file in [
            "coverage-waivers.toml",
            "test-vectors/x",
            "Cargo.lock",
            "rust-toolchain.toml",
            "xtask/src/x.rs",
            "crates/aldis-spp/src/lib.rs",
            "refapp/src/x.rs",
            "fuzz/fuzz_targets/x.rs",
            "include/aldis/aldis.h",
        ] {
            let n = steps_for(&[file], ALL, Mode::Full);
            for (check, steps, _) in COVERED {
                let c = checks
                    .iter()
                    .find(|c| c.name == *check)
                    .expect("a gate check");
                let reads = c.inputs.contains(file);
                for s in *steps {
                    assert_eq!(n.contains(s), reads, "{file}: `{s}` of `{check}`");
                }
            }
        }
    }

    #[test]
    fn a_change_of_a_crate_outside_the_measured_ones_measures_every_crate() {
        // req: VER-068
        let own = touched(&paths(&["crates/aldis-spp/src/lib.rs"]));
        assert_eq!(own.coverage, Some(vec!["aldis-spp".to_owned()]));
        let testkit = touched(&paths(&["crates/aldis-testkit/src/lib.rs"]));
        assert_eq!(testkit.coverage, None);
        let core = touched(&paths(&["crates/aldis-core/src/lib.rs"]));
        assert_eq!(core.coverage, None);
        // A file that coverage reads outside `crates/` too.
        let both = touched(&paths(&[
            "crates/aldis-spp/src/lib.rs",
            "coverage-waivers.toml",
        ]));
        assert_eq!(both.coverage, None);
        // The scoped step says so, and the last line lists the scope.
        let p = plan(&own, ALL, Mode::Full);
        let cov = p.items.iter().find(|i| i.name == "xtask coverage").unwrap();
        assert_eq!(cov.note.as_deref(), Some("scoped to aldis-spp"));
        let Action::Xtask(args) = &cov.action else {
            panic!("coverage is an xtask step");
        };
        assert_eq!(args, &["coverage", "-p", "aldis-spp"]);
        let also = gate_also_runs(Mode::Full, &own, &p);
        assert!(
            also.contains(&"coverage (scoped to aldis-spp)".to_owned()),
            "{also:?}"
        );
        // An unscoped step runs the plain command.
        let p = plan(&testkit, ALL, Mode::Full);
        let cov = p.items.iter().find(|i| i.name == "xtask coverage").unwrap();
        assert!(cov.note.is_none());
        assert!(matches!(&cov.action, Action::Xtask(a) if a == &["coverage"]));
    }

    #[test]
    fn a_missing_tool_skips_a_full_step_and_the_last_line_says_so() {
        // req: VER-068
        let p = plan(&everything(), NONE, Mode::Full);
        let n: Vec<&str> = p.items.iter().map(|i| i.name).collect();
        for step in [
            "xtask coverage",
            "xtask stack",
            "big-endian",
            "xtask header",
            "shell",
        ] {
            assert!(!n.contains(&step), "{step}");
        }
        let skipped: Vec<&str> = p.skipped.iter().map(|s| s.step).collect();
        assert_eq!(
            skipped,
            [
                "shell",
                "xtask header",
                "xtask coverage",
                "xtask stack",
                "big-endian"
            ]
        );
        assert!(
            p.skipped
                .iter()
                .any(|s| s.step == "big-endian" && s.reason.contains("rustup target add")),
            "the way to install it"
        );
        let also = gate_also_runs(Mode::Full, &everything(), &p);
        for entry in [
            "coverage (skipped: tooling nightly or cargo-llvm-cov not installed)",
            "stack (skipped: tooling nightly not installed)",
            "big-endian (skipped: s390x tools not installed)",
            "header (skipped: cbindgen not installed)",
        ] {
            assert!(also.iter().any(|a| a == entry), "{entry} in {also:?}");
        }
        // With the tools, the full tier lists none of the three.
        let p = plan(&everything(), ALL, Mode::Full);
        let also = gate_also_runs(Mode::Full, &everything(), &p);
        assert!(!also.iter().any(|a| a.starts_with("coverage")
            || a.starts_with("stack")
            || a.starts_with("big-endian")));
    }

    #[test]
    fn the_last_line_names_what_the_gate_also_runs_for_the_change() {
        // req: VER-068
        let e = everything();
        let also = |mode| gate_also_runs(mode, &e, &plan(&e, ALL, mode));
        let (quick, default, full) = (also(Mode::Quick), also(Mode::Default), also(Mode::Full));
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
        // A check that the change does not make the gate run is not listed.
        let docs = touched(&paths(&["docs/x.md"]));
        let p = plan(&docs, ALL, Mode::Full);
        assert_eq!(gate_also_runs(Mode::Full, &docs, &p), ["spec"]);
        let none = Plan {
            items: Vec::new(),
            skipped: Vec::new(),
        };
        assert_eq!(
            gate_also_runs(Mode::Quick, &Touched::default(), &none),
            <[String; 0]>::default()
        );
    }

    #[test]
    fn separate_workspaces_are_formatted_whenever_the_gate_formats_them() {
        // req: VER-068
        // The gate's `fmt` check formats all four workspaces for any change it runs for.
        for file in [
            "exerciser/src/ffi.rs",
            "crates/aldis-ffi/src/lib.rs",
            "xtask/src/x.rs",
        ] {
            let n = steps_for(&[file], NONE, Mode::Default);
            for (_, step) in SEPARATE_WORKSPACES {
                assert!(n.contains(&step), "{file}: {step}");
            }
        }
        let n = steps_for(&["docs/x.md"], NONE, Mode::Default);
        assert!(!n.contains(&"exerciser fmt"));
    }

    #[test]
    fn the_formatted_workspaces_are_the_gates() {
        // req: VER-068
        let fmt = all_checks().into_iter().find(|c| c.name == "fmt").unwrap();
        let gate: Vec<&str> = fmt
            .script
            .lines()
            .filter_map(|l| l.split("--manifest-path ").nth(1))
            .collect();
        let ours: Vec<String> = SEPARATE_WORKSPACES
            .iter()
            .map(|(dir, _)| format!("{dir}/Cargo.toml"))
            .collect();
        assert_eq!(gate, ours);
    }

    /// The environment that `item` is spawned with.
    fn spawn_env(item: &Item) -> BTreeMap<String, String> {
        command(item)
            .unwrap()
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect()
    }

    #[test]
    fn the_gates_environment_is_what_check_sets() {
        // req: VER-068
        // Every variable of the gate's CHECK_ENV is set or excused: a new one fails here, so that
        // someone decides whether `check` sets it.
        let set: Vec<&str> = gate_env().iter().map(|(k, _)| *k).collect();
        let excused: Vec<&str> = NOT_LOCAL.iter().map(|(k, _)| *k).collect();
        let mut all: Vec<&str> = set.iter().chain(&excused).copied().collect();
        all.sort_unstable();
        let mut gate: Vec<&str> = CHECK_ENV.iter().map(|(k, _)| *k).collect();
        gate.sort_unstable();
        assert_eq!(all, gate);
        assert_eq!(
            set,
            ["RUSTFLAGS", "RUSTDOCFLAGS"],
            "the flags VER-068 names"
        );
        assert_eq!(
            gate_env().iter().find(|(k, _)| *k == "RUSTFLAGS"),
            Some(&("RUSTFLAGS", "-D warnings"))
        );
    }

    #[test]
    fn every_step_of_every_kind_is_spawned_under_the_gates_environment() {
        // req: VER-068
        let gate = gate_env();
        let mut seen = (false, false);
        for mode in [Mode::Quick, Mode::Default, Mode::Full] {
            for item in &plan(&everything(), ALL, mode).items {
                let env = spawn_env(item);
                for (k, v) in &gate {
                    assert_eq!(
                        env.get(*k).map(String::as_str),
                        Some(*v),
                        "step `{}` is spawned without {k}",
                        item.name
                    );
                }
                match item.action {
                    Action::Cmd { .. } => seen.0 = true,
                    Action::Xtask(_) => seen.1 = true,
                }
            }
        }
        assert_eq!(seen, (true, true), "both kinds of step were checked");
    }

    #[test]
    fn the_big_endian_step_sets_what_the_gates_script_exports() {
        // req: VER-068
        let script = all_checks()
            .into_iter()
            .find(|c| c.name == "big-endian")
            .unwrap()
            .script;
        let wanted = exports(&script);
        assert!(
            wanted
                .iter()
                .any(|(k, _)| k == "CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_RUNNER")
        );
        let p = plan(&everything(), ALL, Mode::Full);
        let item = p.items.iter().find(|i| i.name == "big-endian").unwrap();
        let env = spawn_env(item);
        for (k, v) in &wanted {
            assert_eq!(env.get(k), Some(v), "{k}");
        }
        assert_eq!(
            env.get("RUSTFLAGS").map(String::as_str),
            Some("-D warnings")
        );
    }

    /// A command as the directory it runs in and its words, with the manifest path of
    /// `--manifest-path <dir>/Cargo.toml` folded into the directory. `--locked` stays: a step that drops it differs
    /// from the gate's line.
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
            } else {
                kept.push(*w);
            }
        }
        (dir, kept.join(" "))
    }

    /// Lines of the scripts of covered checks that `check` does not run, with the reason.
    const NOT_A_STEP: &[(&str, &str)] = &[];

    #[test]
    fn every_gate_line_has_a_step() {
        // req: VER-068
        let (items, checks) = (plan(&everything(), ALL, Mode::Full).items, all_checks());
        let steps: Vec<(String, String)> = items
            .iter()
            .map(|i| match &i.action {
                Action::Cmd { program, args, dir } => {
                    let mut words = vec![*program];
                    words.extend(args.iter().copied());
                    normal(dir, &words)
                }
                Action::Xtask(args) => {
                    let mut words = vec!["cargo", "xtask"];
                    words.extend(args.iter().map(String::as_str));
                    normal("", &words)
                }
            })
            .collect();
        for (name, _, _) in COVERED {
            let check = checks.iter().find(|c| c.name == *name).unwrap();
            for line in check.script.lines().filter(|l| l.starts_with("cargo ")) {
                let words: Vec<&str> = line.split_whitespace().collect();
                assert!(
                    steps.contains(&normal("", &words))
                        || NOT_A_STEP.iter().any(|(l, _)| *l == line),
                    "no step of `check --full` runs the line `{line}` of the gate check `{name}`: add it to `plan`, or list it in NOT_A_STEP with the reason"
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
            let n = names(&everything(), ALL, *first);
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
                let n = names(&everything(), ALL, earlier);
                assert!(
                    !steps.iter().all(|s| n.contains(s)),
                    "`{check}` already runs in the {earlier:?} tier"
                );
            }
        }
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
    fn extras_follow_the_gates_inputs() {
        // req: VER-068
        let n = steps_for(&["crates/aldis-ffi/src/lib.rs"], ALL, Mode::Default);
        for step in [
            "exerciser clippy",
            "exerciser build",
            "fuzz clippy",
            "crosscheck clippy",
            "xtask header",
            "xtask header c-smoke",
            "refapp test",
        ] {
            assert!(n.contains(&step), "{step}");
        }
        assert!(!n.contains(&"xtask coverage"));
        // `crosscheck/` is a Rust input of the lints and the exerciser, but the header reads
        // `crates/**` and `include/**` only.
        let n = steps_for(&["crosscheck/src/main.rs"], ALL, Mode::Default);
        assert!(n.contains(&"crosscheck clippy") && n.contains(&"crosscheck fmt"));
        assert!(!n.contains(&"xtask header"));
    }

    #[test]
    fn missing_tools_skip_with_a_reason() {
        // req: VER-068
        let p = plan(&everything(), NONE, Mode::Default);
        let n: Vec<&str> = p.items.iter().map(|i| i.name).collect();
        assert!(!n.contains(&"xtask header"));
        assert!(n.contains(&"exerciser clippy") && n.contains(&"fuzz clippy"));
        assert!(!n.contains(&"shell"));
        let checks: Vec<&str> = p.skipped.iter().map(|s| s.check).collect();
        assert_eq!(checks, ["shell", "header"]);
    }
}
