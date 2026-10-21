//! `cargo xtask verify`: the verification run and the merge gate (docs/06-verification.md §7,
//! VER-056…VER-058).
//!
//! The host side checks that the working tree is clean, decides which checks the change needs
//! (`scope.rs`, from the inputs of each check in `checks.rs`), builds the verification image if
//! it is missing (`image.rs`), runs the container checks in it (`inside.rs`), runs the host-bound
//! checks, and writes the evidence record (`record.rs`) to `target/verify/<commit>/record.json`.
//! `--publish` stores it as a git note and sets the commit status (`publish.rs`).

mod checks;
mod image;
mod inside;
mod publish;
mod record;
mod scope;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{Map, Value, json};

use checks::{Check, Host};
use record::Outcome;

use crate::repo;

/// Flags shared with the RUSTFLAGS of every check (the former `ci.yml` `env`).
const CHECK_ENV: &[(&str, &str)] = &[
    ("CARGO_TERM_COLOR", "never"),
    ("RUSTFLAGS", "-D warnings"),
    ("RUSTDOCFLAGS", "-D warnings"),
];

/// Usage of `cargo xtask verify`, the end of the main help text.
pub const HELP: &str = "\
Verification run and merge gate (docs/06-verification.md §7):
  verify [--only <check>,...] [--all | --all-cpp] [--publish]
                               run the checks the change needs on HEAD in the verification
                               image (podman or Docker; ALDIS_VERIFY_ENGINE picks one) and write
                               the evidence record to target/verify/<commit>/record.json
                               (VER-056, VER-057): a check whose inputs are those of an earlier
                               commit whose published record it passed in is not run and takes
                               that result: `skipped` when its inputs did not change against
                               the merge base with origin/main either, `reused` when they are
                               those of an earlier commit of the branch (`verify list` shows
                               which); --all runs every check; --all-cpp runs the C++ checks
                               even if their inputs are untouched; --only runs a subset (such a
                               record never passes the gate); --publish then publishes it, as
                               `verify publish` does
  verify publish [<rev>]       push the passing record of <rev> (default HEAD) as a git note
                               under refs/notes/verify and set the commit status aldis/verify
                               (VER-058)
  verify check [<rev>]         check the published record of <rev> against the merge gate,
                               re-deriving its skipped and reused checks from git and the
                               sources' published records
  verify image [--push <repository>]
                               build the verification image for HEAD, or pull it from the
                               registry repository named in ALDIS_VERIFY_PULL when its labels
                               hold (VER-060); --push then pushes an image built here to
                               <repository>, from GitHub Actions only
  verify list                  list the checks and their inputs, and which of them HEAD needs
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
}

/// Dispatches `cargo xtask verify ...`; returns the number of problems found.
///
/// # Errors
///
/// When the arguments are not understood, or the run cannot be made.
pub fn run(args: &[&str]) -> Result<usize, String> {
    match args {
        ["inside", "--out", out, rest @ ..] => {
            let opts = parse_options(rest)?.0;
            inside::run(Path::new(out), &opts)
        }
        ["publish"] => publish::publish(&git(&["rev-parse", "HEAD"])?),
        ["publish", rev] => publish::publish(&git(&["rev-parse", rev])?),
        ["check"] => publish::check("HEAD"),
        ["check", rev] => publish::check(rev),
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
            let (opts, flags) = parse_options(rest)?;
            let force = scope::Force {
                all: flags.contains(&"--all"),
                cpp: flags.contains(&"--all-cpp"),
            };
            verify(opts, force, flags.contains(&"--publish"))
        }
    }
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

/// Parses `--only a,b` and, for `verify inside`, `--skip a,b --base <commit>`; returns the
/// other flags.
fn parse_options<'a>(args: &[&'a str]) -> Result<(Options, Vec<&'a str>), String> {
    let mut opts = Options::default();
    let mut flags = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--only" => opts.only = Some(check_names(it.next(), "--only")?),
            "--skip" => opts.skip = check_names(it.next(), "--skip")?,
            "--base" => opts.base = Some((*it.next().ok_or("--base needs a commit")?).to_owned()),
            "--all" | "--all-cpp" | "--publish" => flags.push(*arg),
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

/// The result entry of a check that `opts` leaves out, if it does.
fn skipped(check: &Check, opts: &Options) -> Option<Value> {
    if opts.only.as_ref().is_some_and(|o| !o.contains(&check.name)) {
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
/// `target/verify/host-src`, built into `target/verify/host-target`.
fn host_checks(commit: &str, out: &Path, opts: &Options) -> Result<(Vec<Value>, Value), String> {
    let root = repo::root();
    let src = root.join("target/verify/host-src");
    let host_checks: Vec<Check> = checks::all()
        .into_iter()
        .filter(|c| c.host == Host::MacOs)
        .collect();
    let mut results = Vec::new();
    let mut prepared = false;
    for check in &host_checks {
        if let Some(r) = skipped(check, opts) {
            results.push(r);
            continue;
        }
        if std::env::consts::OS != "macos" {
            results.push(outcome(check, Outcome::NotRun, Some("needs a macOS host")));
            continue;
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
        results.push(run_check(check, &src, &temp, &out.join("logs"), &env)?);
    }
    let rustc = if prepared {
        inside::first_line(&["rustc".to_owned(), "-V".to_owned()], &src)
    } else {
        "unused".to_owned()
    };
    Ok((results, json!({ "host-rustc": rustc })))
}

/// The whole run on HEAD; returns the number of merge-gate problems.
fn verify(mut opts: Options, force: scope::Force, publish: bool) -> Result<usize, String> {
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
        .filter(|d| !d.run)
        .map(|d| d.name.clone())
        .collect();
    opts.base.clone_from(&plan.base);
    let engine = image::Engine::detect()?;
    let image = engine.ensure_image(&commit)?;
    let tag = image.tag.as_str();
    let out = out_dir(&commit)?;
    let started = record::utc_now();
    println!("verify: {commit} in {tag}");
    let container = engine.run_checks(tag, &commit, &out, &opts)?;
    let (host_results, host_tools) = host_checks(&commit, &out, &opts)?;
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
    let record = json!({
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
            "engine": engine.version(),
        },
        "image": image.to_json(),
        "tools": tools,
        "packages": container.get("packages"),
        "scope": scope::to_json(&plan, &all),
        "checks": results,
    });
    let text = record::canonical(&record);
    let path = out.join("record.json");
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut problems = record::gate_problems(&record);
    problems.extend(scope::recheck(&root, &record, &all));
    println!(
        "verify: record {} (sha256 {})",
        repo::rel(&path),
        record::sha256_hex(text.as_bytes())
    );
    if problems.is_empty() {
        println!("verify: the record passes the merge gate (VER-058)");
        if publish {
            return publish::publish(&commit);
        }
    } else {
        for p in &problems {
            println!("verify: gate: {p}");
        }
    }
    Ok(problems.len())
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
