//! `cargo xtask verify`: the verification run and the merge gate (docs/06-verification.md §7,
//! VER-056…VER-058).
//!
//! The host side checks that the working tree is clean, decides which checks the change needs
//! (`scope.rs`, from the inputs of each check in `checks.rs`), builds the verification image if
//! it is missing (`image.rs`), runs the container checks in it (`inside.rs`), runs the host-bound
//! checks, and writes the evidence record (`record.rs`) to `target/verify/<commit>/record.json`.
//! `--publish` stores it as a git note and sets the commit status (`publish.rs`).
//!
//! The checks run quickest first (`checks::run_order`). A run that would publish stops at the
//! first gating check that fails (`--fail-fast`), since such a run can never publish; the checks
//! it did not reach are recorded as not run. When no gating check needs to run in the container,
//! because the input rule skips or reuses all of them, no image is pulled or built and no
//! container is started.

mod checks;
mod image;
mod inside;
mod parts;
mod publish;
mod record;
mod scope;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{Map, Value, json};

use checks::{Check, Group, Host};
use record::Outcome;

use crate::repo;

// For `cargo xtask metrics` (VER-063), which reads the published records as `verify check` does.
pub use publish::{fetch_notes, published};
pub use record::utc;
// For `cargo xtask assurance` (VER-066), which resolves `check:` references against the checks.
pub(crate) use checks::all as all_checks;
// For the tests of `cargo xtask check`, which run the same format and rustdoc commands.
#[cfg(test)]
pub(crate) use checks::{BIG_ENDIAN, CLIPPY, FMT, TEST};

/// Flags shared with the RUSTFLAGS of every check (the former `ci.yml` `env`).
pub(crate) const CHECK_ENV: &[(&str, &str)] = &[
    ("CARGO_TERM_COLOR", "never"),
    ("RUSTFLAGS", "-D warnings"),
    ("RUSTDOCFLAGS", "-D warnings"),
];

pub const HELP: &str = "\
Verification run and merge gate (docs/06-verification.md §7):
  verify [--only <check>,...] [--all | --all-cpp] [--publish] [--fail-fast | --no-fail-fast]
         [--push-image <repository>] [--stale-ok]
                               run the checks the change needs on HEAD in the verification
                               image (podman or Docker; ALDIS_VERIFY_ENGINE picks one) and write
                               the evidence record to target/verify/<commit>/record.json
                               (VER-056, VER-057): a check whose inputs are those of an earlier
                               commit whose published record it passed in is not run and takes
                               that result: `skipped` when its inputs did not change against
                               the merge base with origin/main either, `reused` when they are
                               those of an earlier commit of the branch (`verify list` shows
                               which); --all runs every check, the nightly-only kani too,
                               which every other run records as not run; --all-cpp runs the C++
                               checks even if their inputs are untouched; --only runs a subset (such a
                               record never passes the gate); --publish then publishes it, as
                               `verify publish` does. The checks run quickest first; with
                               --fail-fast, the default with --publish, the run stops at the
                               first gating check that fails and records the rest as not run
                               (such a run never publishes); --no-fail-fast runs every check.
                               When no gating check needs the container, none is started and
                               no image is pulled or built. --push-image pushes the image the
                               run builds to <repository>, as `verify image --push` does.
                               A run with --publish, and a part, refuses a HEAD that does not
                               contain the tip of origin/main before running any check
                               (VER-058); --stale-ok runs it anyway, for a development run, and
                               any other run only warns
  verify part <group> [--all] [--no-fail-fast] [--push-image <repository>] [--stale-ok]
                               the part of a hosted run (VER-067): run the checks of one group
                               (host, cross, coverage, embedded) as `verify --only` does, and
                               write the part record to target/verify/<commit>/record.json; it
                               never publishes, and fails when a gating check of the group
                               failed
  verify assemble [--publish] <part record>...
                               the last step of a hosted run: make the one record of a run from
                               the part records of all four groups, which must fit together
                               (VER-067), and judge and publish it as `verify` does
  verify publish [<rev>]       push the passing record of <rev> (default HEAD) as a git note
                               under refs/notes/verify and, when <rev> contains the tip of
                               origin/main, set the commit status aldis/verify (VER-058); a
                               record of a commit behind main is published without the status,
                               as a source for later commits, and the command fails
  verify nightly [<rev>]       fail unless every nightly-only check passed in the local record
                               of <rev> (default HEAD), made with --all (VER-059)
  verify check [<rev>]         check the published record of <rev> against the merge gate,
                               re-deriving its skipped and reused checks from git and the
                               sources' published records, and that <rev> contains the tip of
                               origin/main
  verify image --ref <repository>
                               print the reference in <repository> of the image for HEAD's
                               image inputs (VER-060), from git alone: no engine, no network
  verify image [--push <repository>]
                               build the verification image for HEAD, or pull it from the
                               registry repository named in ALDIS_VERIFY_PULL when its labels
                               hold (VER-060); --push then pushes an image built here to
                               <repository>, from GitHub Actions only
  verify list                  list the checks and their inputs, which of them HEAD needs, and
                               whether its run would start a container
  verify scope [<rev>] [--main <rev>]
                               which checks <rev> (default HEAD) needs, against origin/main or
                               against the given tip of main (for past commits)
";

/// Which checks a run executes.
#[derive(Debug, Default)]
pub struct Options {
    only: Option<Vec<String>>,
    /// Skipped by the input rule (VER-056).
    skip: Vec<String>,
    /// The base the skips are measured against.
    base: Option<String>,
    /// Whether the run stops at the first gating check that fails: `--fail-fast` or
    /// `--no-fail-fast`, else the default (on with `--publish`).
    fail_fast: Option<bool>,
    /// The registry repository to push the image the run builds to (`--push-image`).
    push_image: Option<String>,
    /// `--all`: every check runs, the nightly-only ones too (VER-059).
    all: bool,
    /// `--stale-ok`: a run that would publish, or a part, goes ahead although the head does not
    /// contain the tip of `origin/main` (VER-058).
    stale_ok: bool,
}

impl Options {
    /// Whether the run stops at the first gating check that fails.
    fn fails_fast(&self) -> bool {
        self.fail_fast == Some(true)
    }
}

/// Dispatches `cargo xtask verify ...`.
pub fn run(args: &[&str]) -> Result<usize, String> {
    match args {
        ["inside", "--out", out, rest @ ..] => {
            let opts = parse_options(rest)?.0;
            inside::run(Path::new(out), &opts)
        }
        ["part", group, rest @ ..] => {
            let group = Group::parse(group).ok_or_else(|| {
                format!("unknown group `{group}`: one of host, cross, coverage, embedded")
            })?;
            let (mut opts, force, publish) = run_options(rest)?;
            if publish || opts.only.is_some() {
                return Err("a part takes neither --publish nor --only".to_owned());
            }
            opts.fail_fast.get_or_insert(true);
            opts.only = Some(
                checks::all()
                    .into_iter()
                    .filter(|c| c.group == Some(group))
                    .map(|c| c.name)
                    .collect(),
            );
            verify(opts, force, false, Some(group))
        }
        ["assemble", rest @ ..] => assemble(rest),
        ["publish"] => publish::publish(&git(&["rev-parse", "HEAD"])?),
        ["publish", rev] => publish::publish(&git(&["rev-parse", rev])?),
        ["check"] => publish::check("HEAD"),
        ["nightly"] => nightly("HEAD"),
        ["nightly", rev] => nightly(rev),
        ["check", rev] => publish::check(rev),
        ["image", "--ref", repository] => {
            println!(
                "{}",
                image::reference(&git(&["rev-parse", "HEAD"])?, repository)?
            );
            Ok(0)
        }
        ["image", rest @ ..] => {
            let engine = image::Engine::detect()?;
            let commit = git(&["rev-parse", "HEAD"])?;
            let image = match rest {
                [] => engine.ensure_image(&commit)?,
                ["--push", repository] => engine.push(&commit, repository)?,
                _ => return Err(format!("unknown arguments {rest:?}\n\n{HELP}")),
            };
            println!(
                "verify: image {} ({}), {}",
                image.tag, image.id, image.source
            );
            Ok(0)
        }
        ["list" | "scope"] => show("HEAD", scope::Main::Origin),
        ["scope", "--main", main] => show("HEAD", scope::Main::Rev(main)),
        ["scope", rev] => show(rev, scope::Main::Origin),
        ["scope", rev, "--main", main] => show(rev, scope::Main::Rev(main)),
        rest => {
            let (opts, force, publish) = run_options(rest)?;
            verify(opts, force, publish, None)
        }
    }
}

/// Parses the options of a run: the options, what `--all` and `--all-cpp` force, and whether it
/// publishes. A run that would publish stops at its first gating failure, which it could not
/// publish, unless `--no-fail-fast` says otherwise; a development run reports every failure
/// unless `--fail-fast` says otherwise.
fn run_options(args: &[&str]) -> Result<(Options, scope::Force, bool), String> {
    let (mut opts, flags) = parse_options(args)?;
    let force = scope::Force {
        all: flags.contains(&"--all"),
        cpp: flags.contains(&"--all-cpp"),
    };
    let publish = flags.contains(&"--publish");
    opts.fail_fast.get_or_insert(publish);
    Ok((opts, force, publish))
}

/// Parses a comma-separated list of check names.
fn check_names(list: Option<&&str>, flag: &str) -> Result<Vec<String>, String> {
    let list = list.ok_or_else(|| format!("{flag} needs a list of checks"))?;
    let names: Vec<String> = list
        .split(',')
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .collect();
    let known: Vec<String> = checks::all().into_iter().map(|c| c.name).collect();
    if let Some(bad) = names.iter().find(|n| !known.contains(n)) {
        return Err(format!(
            "unknown check `{bad}`; see `cargo xtask verify list`"
        ));
    }
    Ok(names)
}

/// Parses `--only a,b`, `--fail-fast`, `--no-fail-fast`, `--push-image <repository>`, and, for
/// `verify inside`, `--skip a,b --base <commit>`; returns the other flags.
fn parse_options<'a>(args: &[&'a str]) -> Result<(Options, Vec<&'a str>), String> {
    let mut opts = Options::default();
    let mut flags = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--only" => opts.only = Some(check_names(it.next(), "--only")?),
            "--skip" => opts.skip = check_names(it.next(), "--skip")?,
            "--base" => opts.base = Some((*it.next().ok_or("--base needs a commit")?).to_owned()),
            "--fail-fast" | "--no-fail-fast" => {
                let on = *arg == "--fail-fast";
                if opts.fail_fast == Some(!on) {
                    return Err("--fail-fast and --no-fail-fast exclude each other".to_owned());
                }
                opts.fail_fast = Some(on);
            }
            "--push-image" => {
                let repository = it.next().ok_or("--push-image needs a repository")?;
                opts.push_image = Some((*repository).to_owned());
            }
            "--all" => {
                opts.all = true;
                flags.push(*arg);
            }
            "--stale-ok" => opts.stale_ok = true,
            "--all-cpp" | "--publish" => flags.push(*arg),
            other => return Err(format!("unknown argument `{other}`\n\n{HELP}")),
        }
    }
    if !opts.skip.is_empty() && opts.base.is_none() {
        return Err("--skip needs --base".to_owned());
    }
    Ok((opts, flags))
}

/// Runs git in `dir`; returns its standard output.
fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Runs git in `dir`; returns its trimmed standard output.
fn git_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_bytes(dir, args).map(|out| String::from_utf8_lossy(&out).trim().to_owned())
}

/// Runs git in the repository root; returns its trimmed standard output.
fn git(args: &[&str]) -> Result<String, String> {
    git_in(&repo::root(), args)
}

/// Prints the checks, their inputs, and which of them `rev` needs (VER-056).
fn show(rev: &str, main: scope::Main) -> Result<usize, String> {
    let commit = git(&["rev-parse", rev])?;
    let root = repo::root();
    let checks = checks::all();
    let change = scope::change(&root, &commit, main);
    let tip = match main {
        scope::Main::Origin => "origin/main",
        scope::Main::Rev(rev) => rev,
    };
    let plan = scope::plan(&checks, &change, scope::Force::default(), |b| {
        scope::sources(&root, &commit, b, tip, &checks)
    });
    println!("verify: scope of {commit}");
    for line in scope::describe(&plan, &checks) {
        println!("verify: {line}");
    }
    let x86_64 = std::env::consts::ARCH == "x86_64";
    match no_container(&checks, &plan, &Options::default(), x86_64) {
        Some(why) => println!("verify: no container would be started: {why}"),
        None => println!("verify: the run would start a container"),
    }
    Ok(0)
}

/// The result entry of a check that did not run.
fn outcome(check: &Check, outcome: Outcome, reason: Option<&str>) -> Value {
    json!({
        "name": check.name,
        "result": outcome.name(),
        "reason": reason,
        "duration_ms": 0,
        "log_sha256": null,
    })
}

/// The result entry of a check that the input rule does not run: skipped or reused, with the
/// record it takes its result from (VER-056, VER-057).
fn carried(check: &Check, decision: &scope::Decision) -> Option<Value> {
    let (outcome, source) = (decision.outcome()?, decision.source.as_ref()?);
    Some(json!({
        "name": check.name,
        "result": outcome.name(),
        "reason": decision.why,
        "source": source.to_json(),
        "duration_ms": 0,
        "log_sha256": null,
    }))
}

/// The reason recorded for a check that a run did not reach because it stopped at the gating
/// check `failed`.
fn stop_reason(failed: &str) -> String {
    format!(
        "the run stopped after the gating check {failed} failed (--fail-fast, the default with \
         --publish): a run in which a check failed is never published (VER-057)"
    )
}

/// The reason recorded for a non-gating check that the input rule would run, in a run that
/// starts no container.
const NO_CONTAINER_REASON: &str = "no gating check needs the container, and a non-gating \
    check alone does not start one (VER-056)";

/// Runs `checks` in their order with `run`, except those that `opts` leaves out. With
/// `--fail-fast`, a run that has stopped, at a gating check that failed here or before
/// (`stopped`), runs no further check and records each as not run with [`stop_reason`]; a
/// non-gating check never stops it. Returns the result entries, in the order of `checks`, and
/// the first gating check that failed, if one did.
fn run_in_order(
    checks: &[Check],
    opts: &Options,
    mut stopped: Option<String>,
    mut run: impl FnMut(&Check) -> Result<Value, String>,
) -> Result<(Vec<Value>, Option<String>), String> {
    let mut results = Vec::new();
    for check in checks {
        let result = match (skipped(check, opts), &stopped) {
            (Some(r), _) => r,
            (None, Some(failed)) if opts.fails_fast() => {
                outcome(check, Outcome::NotRun, Some(&stop_reason(failed)))
            }
            (None, _) => run(check)?,
        };
        let failed = result.get("result").and_then(Value::as_str) == Some(Outcome::Fail.name());
        if check.gating && failed && stopped.is_none() {
            stopped = Some(check.name.clone());
        }
        results.push(result);
    }
    Ok((results, stopped))
}

/// The first gating check, in run order, that failed among the result entries `results`.
fn first_gating_failure(results: &Value, checks: &[Check]) -> Option<String> {
    let order: Vec<String> = checks::run_order().into_iter().map(|c| c.name).collect();
    let mut failed: Vec<&str> = results
        .as_array()?
        .iter()
        .filter(|r| r.get("result").and_then(Value::as_str) == Some(Outcome::Fail.name()))
        .filter_map(|r| r.get("name").and_then(Value::as_str))
        .filter(|n| checks.iter().any(|c| c.gating && c.name == *n))
        .collect();
    failed.sort_by_key(|n| order.iter().position(|o| o == n));
    failed.first().map(|n| (*n).to_owned())
}

/// Whether the run executes `check` in the container, given the input rule's decision for it:
/// the rule runs it, `--only` selects it, and the host can run it (`x86_64`: whether it is an
/// `x86_64` host).
fn runs_in_container(
    check: &Check,
    decision: &scope::Decision,
    opts: &Options,
    x86_64: bool,
) -> bool {
    check.host != Host::MacOs
        && decision.run
        && opts.only.as_ref().is_none_or(|o| o.contains(&check.name))
        && (check.host != Host::ContainerX64 || x86_64)
}

/// Why the run starts no container, or `None` when it starts one: it does when a gating check
/// runs in it, or a check that `--only` selects (VER-056). Otherwise the input rule skips or
/// reuses every gating check of the container, and a non-gating check alone, which the rule
/// runs when it has no passing source (`cpp-leon3` never passes, OI-08), pulls no image.
fn no_container(
    checks: &[Check],
    plan: &scope::Plan,
    opts: &Options,
    x86_64: bool,
) -> Option<String> {
    let needed = plan
        .checks
        .iter()
        .zip(checks)
        .filter(|(d, c)| runs_in_container(c, d, opts, x86_64))
        .any(|(_, c)| c.gating || opts.only.is_some());
    (!needed).then(|| {
        "no gating check runs in the container: the input rule skips or reuses each of them \
         (VER-056)"
            .to_owned()
    })
}

/// The result entries of the container checks of a run that starts no container
/// ([`no_container`]): each skipped, reused, or left out by `--only` as `opts` says, and each
/// that the rule would run, a non-gating check or one for an `x86_64` host on another host, not
/// run.
fn without_container(opts: &Options, x86_64: bool) -> Result<Vec<Value>, String> {
    let order: Vec<Check> = checks::run_order()
        .into_iter()
        .filter(|c| c.host != Host::MacOs)
        .collect();
    let (results, _) = run_in_order(&order, opts, None, |check| {
        if check.host == Host::ContainerX64 && !x86_64 {
            Ok(outcome(
                check,
                Outcome::NotRun,
                Some("needs an x86_64 host"),
            ))
        } else if !check.gating && opts.only.is_none() {
            Ok(outcome(check, Outcome::NotRun, Some(NO_CONTAINER_REASON)))
        } else {
            Err(format!(
                "{} needs to run, but the run starts no container",
                check.name
            ))
        }
    })?;
    Ok(results)
}

/// The result entry of a check that `opts` leaves out, if it does: a nightly-only check in a run
/// without `--all` (VER-059), a check that `--only` does not select, or one the input rule skips.
fn skipped(check: &Check, opts: &Options) -> Option<Value> {
    if check.nightly && !opts.all {
        Some(outcome(check, Outcome::NotRun, Some(checks::NIGHTLY_ONLY)))
    } else if opts.only.as_ref().is_some_and(|o| !o.contains(&check.name)) {
        Some(outcome(
            check,
            Outcome::NotRun,
            Some("not selected with --only"),
        ))
    } else if let (true, Some(base)) = (opts.skip.contains(&check.name), &opts.base) {
        Some(outcome(
            check,
            Outcome::Skipped,
            Some(&scope::skip_reason(base)),
        ))
    } else {
        None
    }
}

/// Runs one check from `cwd` with an empty `RUNNER_TEMP` at `temp`, its output in
/// `<logs>/<name>.log`; returns its result entry.
fn run_check(
    check: &Check,
    cwd: &Path,
    temp: &Path,
    logs: &Path,
    env: &[(&str, &str)],
) -> Result<Value, String> {
    let _ = std::fs::remove_dir_all(temp);
    std::fs::create_dir_all(temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    let log_path = logs.join(format!("{}.log", check.name));
    let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("{}: {e}", log_path.display()))?;
    print!("verify: {:42} ", check.name);
    let start = Instant::now();
    let status = Command::new("bash")
        .args(["-euo", "pipefail", "-c", &check.script])
        .current_dir(cwd)
        .env("RUNNER_TEMP", temp)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err)
        .status()
        .map_err(|e| format!("bash: {e}"))?;
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let bytes = std::fs::read(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let result = if status.success() {
        Outcome::Pass
    } else {
        Outcome::Fail
    };
    println!("{:7} {:>7.1} s", result.name(), Duration(ms));
    if result == Outcome::Fail {
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines[lines.len().saturating_sub(30)..] {
            println!("    | {line}");
        }
        println!("    (full log: {})", log_path.display());
    }
    Ok(json!({
        "name": check.name,
        "result": result.name(),
        "reason": null,
        "duration_ms": ms,
        "log_sha256": record::sha256_hex(&bytes),
    }))
}

/// Milliseconds shown as seconds with one decimal.
struct Duration(u64);

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.0 / 1000, self.0 % 1000 / 100)
    }
}

/// Where the run is made (VER-057): the GitHub Actions run of VER-060, or the contributor's
/// machine.
fn source() -> Value {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if var("GITHUB_ACTIONS").as_deref() == Some("true")
        && let (Some(server), Some(repo), Some(run), Some(attempt)) = (
            var("GITHUB_SERVER_URL"),
            var("GITHUB_REPOSITORY"),
            var("GITHUB_RUN_ID"),
            var("GITHUB_RUN_ATTEMPT"),
        )
    {
        return json!({
            "kind": "github-actions",
            "run": format!("{server}/{repo}/actions/runs/{run}/attempts/{attempt}"),
            "workflow": var("GITHUB_WORKFLOW"),
        });
    }
    json!({"kind": "local"})
}

/// `target/verify/<commit>`, emptied.
fn out_dir(commit: &str) -> Result<PathBuf, String> {
    let out = repo::root().join("target/verify").join(commit);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(out.join("logs")).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}

/// The host-bound checks run outside the image: a clean checkout of `commit` under
/// `target/verify/host-src`, built into `target/verify/host-target`. They run after the
/// container's, and with `--fail-fast` not at all once a gating check has failed (`stopped`).
/// Returns their result entries, the tools, and the first gating check that failed, if one did.
fn host_checks(
    commit: &str,
    out: &Path,
    opts: &Options,
    stopped: Option<String>,
) -> Result<(Vec<Value>, Value, Option<String>), String> {
    let root = repo::root();
    let src = root.join("target/verify/host-src");
    let host_checks: Vec<Check> = checks::run_order()
        .into_iter()
        .filter(|c| c.host == Host::MacOs)
        .collect();
    let mut prepared = false;
    let (results, stopped) = run_in_order(&host_checks, opts, stopped, |check| {
        if std::env::consts::OS != "macos" {
            return Ok(outcome(check, Outcome::NotRun, Some("needs a macOS host")));
        }
        if !prepared {
            let _ = std::fs::remove_dir_all(&src);
            let src_str = src.to_string_lossy();
            git(&["clone", "-q", "--shared", "--no-checkout", ".", &src_str])?;
            git(&[
                "-C",
                &src_str,
                "-c",
                "advice.detachedHead=false",
                "checkout",
                "-q",
                commit,
            ])?;
            prepared = true;
        }
        let target = root.join("target/verify/host-target");
        let target = target.to_string_lossy();
        let mut env = CHECK_ENV.to_vec();
        env.push(("CARGO_TARGET_DIR", &target));
        let temp = root.join("target/verify/host-tmp").join(&check.name);
        run_check(check, &src, &temp, &out.join("logs"), &env)
    })?;
    let rustc = if prepared {
        inside::first_line(&["rustc".to_owned(), "-V".to_owned()], &src)
    } else {
        "unused".to_owned()
    };
    Ok((results, json!({ "host-rustc": rustc }), stopped))
}

/// Why the nightly-only checks of `record` did not pass, if one did not: each must have run and
/// passed in a nightly run (`--all`, VER-059).
fn nightly_problems(record: &Value, checks: &[Check]) -> Vec<String> {
    let entries = record
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    checks
        .iter()
        .filter(|c| c.nightly)
        .filter_map(|c| {
            let entry = entries.iter().find(|e| e["name"] == c.name.as_str());
            let result = entry
                .and_then(|e| e["result"].as_str())
                .unwrap_or("missing");
            (result != Outcome::Pass.name()).then(|| format!("{}: {result}", c.name))
        })
        .collect()
}

/// `verify nightly [<rev>]`: checks that the nightly-only checks passed in the local record of
/// `rev`, made by `verify --all`; the nightly workflow runs it after the run, so that a failure of
/// one of them fails the nightly run and opens its issue (VER-059). Returns the number of
/// problems.
fn nightly(rev: &str) -> Result<usize, String> {
    let commit = git(&["rev-parse", rev])?;
    let path = repo::root()
        .join("target/verify")
        .join(&commit)
        .join("record.json");
    let record: Value = serde_json::from_str(&repo::read(&path)?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let problems = nightly_problems(&record, &checks::all());
    for p in &problems {
        println!("verify: nightly-only check {p} (VER-059)");
    }
    if problems.is_empty() {
        println!("verify: every nightly-only check passed in the record of {commit}");
    }
    Ok(problems.len())
}

/// The container part of a run, for the record.
struct ContainerRun {
    /// `container.json`: the result entries of the container checks, the container's
    /// architecture, tools, and packages.
    report: Value,
    /// The record's `image`.
    image: Value,
    /// The engine's version, or null when no container was started.
    engine: Value,
    /// When the checks started: after the image was pulled or built.
    started: String,
}

/// Runs the container checks of `commit` with `opts`, in the image pulled or built for it, and
/// pushed to the repository of `--push-image` when given; or, when no gating check needs the
/// container ([`no_container`]), records their results without engine, image, or container.
fn container_run(
    commit: &str,
    out: &Path,
    opts: &Options,
    plan: &scope::Plan,
    all: &[Check],
) -> Result<ContainerRun, String> {
    let x86_64 = std::env::consts::ARCH == "x86_64";
    if let Some(why) = no_container(all, plan, opts, x86_64) {
        println!("verify: {commit}: no container is started: {why}");
        return Ok(ContainerRun {
            started: record::utc_now(),
            report: json!({
                "arch": null,
                "checks": without_container(opts, x86_64)?,
                "packages": null,
                "tools": {},
            }),
            image: image::unused(commit, &why)?,
            engine: Value::Null,
        });
    }
    let engine = image::Engine::detect()?;
    let image = match &opts.push_image {
        Some(repository) => engine.push(commit, repository)?,
        None => engine.ensure_image(commit)?,
    };
    let started = record::utc_now();
    println!("verify: {commit} in {}", image.tag);
    let report = engine.run_checks(&image.tag, commit, out, opts)?;
    Ok(ContainerRun {
        report,
        image: image.to_json(),
        engine: json!(engine.version()),
        started,
    })
}

/// The whole run on HEAD, or with `part` the part of a hosted run that makes the checks of that
/// group (VER-067); returns the number of merge-gate problems, or of failed checks of the part.
fn verify(
    mut opts: Options,
    force: scope::Force,
    publish: bool,
    part: Option<Group>,
) -> Result<usize, String> {
    let dirty = git(&["status", "--porcelain"])?;
    if !dirty.is_empty() {
        return Err(format!(
            "the working tree has uncommitted or untracked changes; commit them first \
             (VER-056):\n{dirty}"
        ));
    }
    let root = repo::root();
    let commit = git(&["rev-parse", "HEAD"])?;
    let tree = git(&["rev-parse", "HEAD^{tree}"])?;
    // A head behind main cannot merge: a run that would publish, or a part of one, refuses it
    // before the checks; any other run only says so (VER-058).
    if publish || part.is_some() {
        publish::preflight(&root, &commit, opts.stale_ok)?;
    } else if !publish::contains_main(&root, &commit)? {
        println!("verify: warning: {commit} does not contain the tip of origin/main (VER-058)");
    }
    let all = checks::all();
    let change = scope::change(&root, &commit, scope::Main::Origin);
    let plan = scope::plan(&all, &change, force, |b| {
        scope::sources(&root, &commit, b, "origin/main", &all)
    });
    for line in scope::describe(&plan, &all) {
        println!("verify: {line}");
    }
    opts.skip = plan
        .checks
        .iter()
        .filter(|d| !d.run && d.source.is_some())
        .map(|d| d.name.clone())
        .collect();
    opts.base.clone_from(&plan.base);
    let out = out_dir(&commit)?;
    let ContainerRun {
        report: container,
        image,
        engine,
        started,
    } = container_run(&commit, &out, &opts, &plan, &all)?;
    let stopped = container
        .get("checks")
        .and_then(|c| first_gating_failure(c, &all));
    let (host_results, host_tools, stopped) = host_checks(&commit, &out, &opts, stopped)?;
    if let (true, Some(failed)) = (opts.fails_fast(), &stopped) {
        println!(
            "verify: the gating check {failed} failed: the run stopped there, and the checks it \
             did not reach are recorded as not run (--fail-fast; --no-fail-fast runs them all)"
        );
    }
    let mut tools = container.get("tools").cloned().unwrap_or_else(|| json!({}));
    if let (Some(t), Value::Object(h)) = (tools.as_object_mut(), host_tools) {
        t.extend(h);
    }
    // A check the rule does not run takes its result from its source; one that --only leaves
    // out stays not run, so that such a record never passes (VER-058).
    let carried = plan
        .checks
        .iter()
        .zip(&all)
        .filter(|(_, c)| opts.only.as_ref().is_none_or(|o| o.contains(&c.name)))
        .filter_map(|(d, c)| carried(c, d));
    let results = merge_results(
        &container,
        host_results.into_iter().chain(carried).collect(),
    )?;
    let mut record = json!({
        "format": record::FORMAT,
        "commit": commit,
        "tree": tree,
        "started": started,
        "finished": record::utc_now(),
        "source": source(),
        "host": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "container_arch": container.get("arch"),
            "engine": engine,
        },
        "image": image,
        "tools": tools,
        "packages": container.get("packages"),
        "scope": scope::to_json(&plan, &all),
        "checks": results,
    });
    if let (Some(group), Some(obj)) = (part, record.as_object_mut()) {
        obj.insert("part".to_owned(), json!({"group": group.name()}));
    }
    write_record(&out, &record)?;
    if let Some(group) = part {
        let failed = parts::failures(&record, group, &all);
        for f in &failed {
            println!("verify: part {}: {f}", group.name());
        }
        return Ok(failed.len());
    }
    conclude(&root, &record, &commit, &all, publish)
}

/// Writes `record` as `record.json` in `out` and prints its path and SHA-256.
fn write_record(out: &Path, record: &Value) -> Result<(), String> {
    let text = record::canonical(record);
    let path = out.join("record.json");
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "verify: record {} (sha256 {})",
        repo::rel(&path),
        record::sha256_hex(text.as_bytes())
    );
    Ok(())
}

/// Judges the record of `commit` against the merge gate (VER-058) and, with `publish`, publishes
/// it when it passes; returns the number of problems.
fn conclude(
    root: &Path,
    record: &Value,
    commit: &str,
    all: &[Check],
    publish: bool,
) -> Result<usize, String> {
    let mut problems = record::gate_problems(record);
    problems.extend(scope::recheck(root, record, all));
    if problems.is_empty() {
        println!("verify: the record passes the merge gate (VER-058)");
        if publish {
            return publish::publish(commit);
        }
    } else {
        for p in &problems {
            println!("verify: gate: {p}");
        }
    }
    Ok(problems.len())
}

/// Whether `record` is of the commit `head`.
fn of_head(record: &Value, head: &str) -> Result<(), String> {
    if record.get("commit").and_then(Value::as_str) == Some(head) {
        Ok(())
    } else {
        Err(format!("the part records are not of HEAD, {head}"))
    }
}

/// `verify assemble`: the record of a hosted run from the part records `args` name (VER-067).
fn assemble(args: &[&str]) -> Result<usize, String> {
    let (publish, files): (Vec<&str>, Vec<&str>) = args.iter().partition(|a| **a == "--publish");
    let records = files
        .iter()
        .map(|f| {
            let text = repo::read(Path::new(f))?;
            serde_json::from_str(&text).map_err(|e| format!("{f}: {e}"))
        })
        .collect::<Result<Vec<Value>, String>>()?;
    let all = checks::all();
    let record = parts::assemble(&records, &all).map_err(|p| {
        format!(
            "the part records do not fit together:\n  {}",
            p.join("\n  ")
        )
    })?;
    let commit = git(&["rev-parse", "HEAD"])?;
    of_head(&record, &commit)?;
    let out = out_dir(&commit)?;
    write_record(&out, &record)?;
    conclude(&repo::root(), &record, &commit, &all, !publish.is_empty())
}

/// Every check's result entry, in table order, with its host, gating, and script added.
fn merge_results(container: &Value, host: Vec<Value>) -> Result<Vec<Value>, String> {
    let mut by_name: Map<String, Value> = Map::new();
    let from_container = container
        .get("checks")
        .and_then(Value::as_array)
        .ok_or("container.json has no checks")?;
    for r in from_container.iter().cloned().chain(host) {
        if let Some(name) = r.get("name").and_then(Value::as_str) {
            by_name.insert(name.to_owned(), r);
        }
    }
    checks::all()
        .into_iter()
        .map(|c| {
            let mut entry = by_name
                .remove(&c.name)
                .ok_or_else(|| format!("no result for check {}", c.name))?;
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("host".to_owned(), json!(c.host.name()));
                obj.insert("gating".to_owned(), json!(c.gating));
                obj.insert("script".to_owned(), json!(c.script));
            }
            Ok(entry)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-067
    #[test]
    fn a_part_takes_no_publish_no_only_and_a_known_group() {
        for args in [["part", "host", "--publish"], ["part", "host", "--only"]] {
            assert!(run(&args).is_err(), "{args:?}");
        }
        assert!(
            run(&["part", "host", "--only", "fmt"])
                .unwrap_err()
                .contains("neither")
        );
        assert!(
            run(&["part", "kani"])
                .unwrap_err()
                .contains("unknown group")
        );
    }

    // req: VER-067
    #[test]
    fn assembled_parts_must_be_of_head() {
        let record = json!({"commit": "abc"});
        assert!(of_head(&record, "abc").is_ok());
        assert!(
            of_head(&record, "def")
                .unwrap_err()
                .contains("not of HEAD, def")
        );
        assert!(of_head(&json!({}), "abc").is_err());
    }

    /// A result entry of `check` with `result`.
    fn entry(check: &Check, result: Outcome) -> Value {
        outcome(check, result, None)
    }

    /// The result of each entry, by check name.
    fn results(entries: &[Value]) -> Vec<(String, String, Option<String>)> {
        entries
            .iter()
            .map(|e| {
                (
                    e["name"].as_str().unwrap().to_owned(),
                    e["result"].as_str().unwrap().to_owned(),
                    e["reason"].as_str().map(str::to_owned),
                )
            })
            .collect()
    }

    /// Runs every check of the run order with `opts`; `fails` names the checks that fail, the
    /// others pass. Returns the entries, the first gating failure, and the checks that ran.
    fn simulate(opts: &Options, fails: &[&str]) -> (Vec<Value>, Option<String>, Vec<String>) {
        let mut ran = Vec::new();
        let (entries, stopped) = run_in_order(&checks::run_order(), opts, None, |c| {
            ran.push(c.name.clone());
            let failed = fails.contains(&c.name.as_str());
            Ok(entry(c, if failed { Outcome::Fail } else { Outcome::Pass }))
        })
        .unwrap();
        (entries, stopped, ran)
    }

    fn fail_fast(on: bool) -> Options {
        Options {
            fail_fast: Some(on),
            ..Options::default()
        }
    }

    // req: VER-056, VER-057
    #[test]
    fn a_run_that_fails_fast_stops_at_the_first_gating_failure() {
        let order: Vec<String> = checks::run_order().into_iter().map(|c| c.name).collect();
        let at = order.iter().position(|n| n == "header").unwrap();
        let (entries, stopped, ran) = simulate(&fail_fast(true), &["header", "clippy"]);
        assert_eq!(stopped.as_deref(), Some("header"));
        // The checks before it ran, and nothing after it.
        assert_eq!(ran, order[..=at]);
        let rows = results(&entries);
        assert_eq!(rows.len(), order.len());
        assert_eq!(rows[at].1, "fail");
        assert!(rows[..at].iter().all(|r| r.1 == "pass"));
        for row in &rows[at + 1..] {
            assert_eq!(row.1, "not-run", "{}", row.0);
            let why = if row.0 == "kani" {
                checks::NIGHTLY_ONLY.to_owned()
            } else {
                stop_reason("header")
            };
            assert_eq!(row.2.as_deref(), Some(why.as_str()), "{}", row.0);
        }
        assert!(
            stop_reason("header")
                .starts_with("the run stopped after the gating check header failed (--fail-fast")
        );
    }

    // req: VER-056
    #[test]
    fn a_non_gating_failure_never_stops_a_run() {
        let order = runs_without_all();
        let (entries, stopped, ran) = simulate(&fail_fast(true), &["rust-sparc-leon3", "kani"]);
        assert_eq!(stopped, None);
        assert_eq!(ran, order);
        let failed: Vec<String> = results(&entries)
            .into_iter()
            .filter(|r| r.1 == "fail")
            .map(|r| r.0)
            .collect();
        assert_eq!(failed, ["rust-sparc-leon3"]);
    }

    /// The checks of the run order that a run without `--all` runs: all but the nightly-only.
    fn runs_without_all() -> Vec<String> {
        checks::run_order()
            .into_iter()
            .filter(|c| !c.nightly)
            .map(|c| c.name)
            .collect()
    }

    // req: VER-056, VER-057, VER-059
    #[test]
    fn a_nightly_only_check_runs_only_with_all() {
        for publish in [
            &["--publish"][..],
            &[],
            &["--only", "kani"],
            &["--no-fail-fast"],
        ] {
            let (opts, _, _) = run_options(publish).unwrap();
            let (entries, _, ran) = {
                let mut ran = Vec::new();
                let (entries, stopped) = run_in_order(&checks::run_order(), &opts, None, |c| {
                    ran.push(c.name.clone());
                    Ok(entry(c, Outcome::Pass))
                })
                .unwrap();
                (entries, stopped, ran)
            };
            assert!(!ran.contains(&"kani".to_owned()), "{publish:?}");
            let kani = results(&entries)
                .into_iter()
                .find(|r| r.0 == "kani")
                .unwrap();
            assert_eq!(kani.1, "not-run");
            assert_eq!(kani.2.as_deref(), Some(checks::NIGHTLY_ONLY));
        }
        // --all runs it, and passes --all on to the container.
        let (opts, force, _) = run_options(&["--all"]).unwrap();
        assert!(opts.all && force.all);
        let (_, _, ran) = simulate(&opts, &[]);
        assert!(ran.contains(&"kani".to_owned()));
        assert!(parse_options(&["--all"]).unwrap().0.all);
    }

    // req: VER-059
    #[test]
    fn a_nightly_run_fails_when_a_nightly_only_check_did_not_pass() {
        let all = checks::all();
        let rec = |result: &str| {
            json!({"checks": [{"name": "fmt", "result": "pass"},
            {"name": "kani", "result": result}, {"name": "cpp-leon3", "result": "fail"}]})
        };
        assert_eq!(nightly_problems(&rec("pass"), &all), Vec::<String>::new());
        assert_eq!(nightly_problems(&rec("fail"), &all), ["kani: fail"]);
        assert_eq!(nightly_problems(&rec("not-run"), &all), ["kani: not-run"]);
        assert_eq!(
            nightly_problems(&json!({"checks": []}), &all),
            ["kani: missing"]
        );
    }

    // req: VER-056
    #[test]
    fn a_run_that_does_not_fail_fast_runs_every_check() {
        let order = runs_without_all();
        let (entries, stopped, ran) = simulate(&fail_fast(false), &["header", "clippy"]);
        assert_eq!(stopped.as_deref(), Some("header"));
        assert_eq!(ran, order);
        let failed = results(&entries).iter().filter(|r| r.1 == "fail").count();
        assert_eq!(failed, 2);
    }

    // req: VER-056, VER-057
    #[test]
    fn a_stopped_run_still_records_skips_and_runs_no_host_check() {
        let mut opts = fail_fast(true);
        opts.skip = vec!["coverage".to_owned()];
        opts.base = Some("b".repeat(40));
        // The container stopped at fmt: the host checks do not run.
        let host: Vec<Check> = checks::run_order()
            .into_iter()
            .filter(|c| c.host == Host::MacOs || c.name == "coverage")
            .collect();
        let (entries, stopped) = run_in_order(&host, &opts, Some("fmt".to_owned()), |c| {
            panic!("{} ran after the run stopped", c.name)
        })
        .unwrap();
        assert_eq!(stopped.as_deref(), Some("fmt"));
        for row in results(&entries) {
            let want = if row.0 == "coverage" {
                "skipped"
            } else {
                "not-run"
            };
            assert_eq!(row.1, want, "{}", row.0);
        }
        // Without --fail-fast they run.
        let mut ran = 0;
        let _ = run_in_order(&host, &fail_fast(false), Some("fmt".to_owned()), |c| {
            ran += 1;
            Ok(entry(c, Outcome::Pass))
        })
        .unwrap();
        assert_eq!(ran, host.len());
    }

    // req: VER-056
    #[test]
    fn only_a_run_that_publishes_fails_fast_by_default() {
        let stops = |args: &[&str]| run_options(args).map(|(o, _, _)| o.fails_fast());
        assert_eq!(stops(&["--publish"]), Ok(true));
        assert_eq!(stops(&[]), Ok(false));
        assert_eq!(stops(&["--only", "fmt,clippy"]), Ok(false));
        assert_eq!(stops(&["--publish", "--no-fail-fast"]), Ok(false));
        assert_eq!(stops(&["--fail-fast"]), Ok(true));
        assert_eq!(stops(&["--only", "fmt", "--fail-fast"]), Ok(true));
        assert!(stops(&["--fail-fast", "--no-fail-fast"]).is_err());
        let (opts, _, publish) = run_options(&["--publish", "--push-image", "r/x"]).unwrap();
        assert!(publish);
        assert_eq!(opts.push_image.as_deref(), Some("r/x"));
        assert!(run_options(&["--push-image"]).is_err());
        // `verify inside` gets --fail-fast from the host only when the run fails fast.
        let (inside, _) = parse_options(&["--fail-fast"]).unwrap();
        assert!(inside.fails_fast());
        assert!(!parse_options(&[]).unwrap().0.fails_fast());
    }

    // req: VER-056
    #[test]
    fn the_first_gating_failure_is_found_in_run_order() {
        let all = checks::all();
        let r = |name: &str, result: &str| json!({"name": name, "result": result});
        let entries = json!([
            r("clippy", "fail"),
            r("cpp-leon3", "fail"),
            r("header", "fail")
        ]);
        assert_eq!(
            first_gating_failure(&entries, &all).as_deref(),
            Some("header")
        );
        let non_gating = json!([r("cpp-leon3", "fail"), r("fmt", "pass")]);
        assert_eq!(first_gating_failure(&non_gating, &all), None);
    }

    /// A plan in which the rule runs exactly the checks `run` and takes every other check's
    /// result from the record of `cccc…`.
    fn plan_running(run: &[&str]) -> scope::Plan {
        let source = scope::Source {
            commit: "c".repeat(40),
            record: "d".repeat(64),
        };
        scope::Plan {
            base: Some("b".repeat(40)),
            base_record: Some(source.commit.clone()),
            branch: Vec::new(),
            changed: vec!["crates/x/src/lib.rs".to_owned()],
            every: None,
            checks: checks::all()
                .into_iter()
                .map(|c| {
                    let runs = run.contains(&c.name.as_str());
                    if c.nightly {
                        // As `scope::plan` decides without --all.
                        return scope::Decision {
                            name: c.name,
                            run: false,
                            why: checks::NIGHTLY_ONLY.to_owned(),
                            source: None,
                            reused: false,
                        };
                    }
                    scope::Decision {
                        why: if runs { "changes x" } else { "same inputs" }.to_owned(),
                        source: (!runs).then(|| source.clone()),
                        reused: !runs,
                        run: runs,
                        name: c.name,
                    }
                })
                .collect(),
        }
    }

    /// `opts` with the skips of `plan`, as `verify` sets them.
    fn opts_of(plan: &scope::Plan, only: Option<&[&str]>) -> Options {
        Options {
            only: only.map(|o| o.iter().map(|n| (*n).to_owned()).collect()),
            skip: plan
                .checks
                .iter()
                .filter(|d| !d.run && d.source.is_some())
                .map(|d| d.name.clone())
                .collect(),
            base: plan.base.clone(),
            ..Options::default()
        }
    }

    // req: VER-056, VER-060
    #[test]
    fn no_container_when_no_gating_check_runs_in_it() {
        let all = checks::all();
        let none = |run: &[&str], only: Option<&[&str]>, x86_64: bool| {
            let plan = plan_running(run);
            no_container(&all, &plan, &opts_of(&plan, only), x86_64).is_some()
        };
        // Every check skipped or reused, or only the non-gating checks without a passing source,
        // or a gating check on the host alone: no container.
        assert!(none(&[], None, true));
        assert!(none(&["rust-sparc-leon3"], None, true));
        assert!(none(&["rust-sparc-leon3"], None, false));
        assert!(none(&["test-macos"], None, true));
        // The nightly-only kani never starts one on its own, even selected with --only.
        assert!(none(&[], Some(&["kani"]), true));
        // A gating container check, or a check that --only selects, starts one.
        assert!(!none(&["rust-sparc-leon3", "clippy"], None, true));
        assert!(!none(&["spec"], None, false));
        assert!(!none(
            &["rust-sparc-leon3"],
            Some(&["rust-sparc-leon3"]),
            true
        ));
        assert!(none(&["rust-sparc-leon3"], Some(&["fmt"]), true));
    }

    // req: VER-056, VER-057, VER-058
    #[test]
    fn the_record_of_a_run_without_a_container_passes_the_gate() {
        let all = checks::all();
        let plan = plan_running(&["rust-sparc-leon3", "test-macos"]);
        let opts = opts_of(&plan, None);
        for x86_64 in [true, false] {
            let checks = without_container(&opts, x86_64).unwrap();
            let rows = results(&checks);
            assert_eq!(
                rows.len(),
                all.iter().filter(|c| c.host != Host::MacOs).count()
            );
            for (name, result, reason) in &rows {
                let want = match name.as_str() {
                    "rust-sparc-leon3" => ("not-run", Some(NO_CONTAINER_REASON)),
                    "kani" => ("not-run", Some(checks::NIGHTLY_ONLY)),
                    _ => ("skipped", None),
                };
                assert_eq!(result, want.0, "{name}");
                if let Some(why) = want.1 {
                    assert_eq!(reason.as_deref(), Some(why), "{name}");
                }
            }
            // The record that `verify` assembles from them, the carried results, and the host
            // check that this host cannot run.
            let container = json!({"arch": null, "checks": checks});
            let mac = all.iter().find(|c| c.host == Host::MacOs).unwrap();
            let host = outcome(mac, Outcome::NotRun, Some("needs a macOS host"));
            let carried = plan
                .checks
                .iter()
                .zip(&all)
                .filter_map(|(d, c)| carried(c, d));
            let entries =
                merge_results(&container, std::iter::once(host).chain(carried).collect()).unwrap();
            let record = json!({
                "format": record::FORMAT,
                "scope": scope::to_json(&plan, &all),
                "image": {"tag": null, "id": null, "inputs": "e".repeat(64),
                    "source": {"kind": "none", "reason": "no gating check runs in the container"}},
                "checks": entries,
            });
            assert_eq!(record::gate_problems(&record), Vec::<String>::new());
            let reused = record["checks"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["result"] == "reused")
                .count();
            assert_eq!(reused, all.len() - 3);
        }
    }

    // req: VER-057
    #[test]
    fn a_run_without_a_container_names_the_image_inputs_and_no_image() {
        let head = git(&["rev-parse", "HEAD"]).unwrap();
        let image = image::unused(&head, "why").unwrap();
        assert_eq!(image["tag"], Value::Null);
        assert_eq!(image["id"], Value::Null);
        assert_eq!(image["inputs"].as_str().map(str::len), Some(64));
        assert_eq!(image["source"], json!({"kind": "none", "reason": "why"}));
    }
}
