//! `cargo xtask verify inside`: runs the container checks inside the verification image
//! (VER-056). Started by `xtask/verify/entrypoint.sh`, never by hand. Writes the results, the
//! tool versions, and the installed packages to `<out>/container.json`, and one log per check
//! to `<out>/logs/`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use super::checks::{self, Check, Host};
use super::record::{Outcome, canonical};
use super::{Options, git_in, outcome, run_check, run_in_order};
use crate::repo;

/// The tools whose versions every record lists (VER-057): name and command.
fn tool_commands() -> Result<Vec<(&'static str, Vec<String>)>, String> {
    let nightly = repo::tooling_nightly()?;
    let cmd = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    Ok(vec![
        ("rustc", cmd(&["rustc", "-V"])),
        ("cargo", cmd(&["cargo", "-V"])),
        (
            "rustc-tooling-nightly",
            vec!["rustc".to_owned(), format!("+{nightly}"), "-V".to_owned()],
        ),
        ("cargo-llvm-cov", cmd(&["cargo", "llvm-cov", "--version"])),
        ("cargo-deny", cmd(&["cargo", "deny", "--version"])),
        ("clang", cmd(&["clang++", "--version"])),
        ("gcc", cmd(&["g++-14", "--version"])),
        ("gcc-s390x", cmd(&["s390x-linux-gnu-g++-14", "--version"])),
        (
            "arm-none-eabi-gcc",
            cmd(&["arm-none-eabi-g++", "--version"]),
        ),
        (
            "riscv-none-elf-gcc",
            cmd(&["riscv-none-elf-g++", "--version"]),
        ),
        ("cmake", cmd(&["cmake", "--version"])),
        ("ninja", cmd(&["ninja", "--version"])),
        ("qemu-system-arm", cmd(&["qemu-system-arm", "--version"])),
        (
            "qemu-system-riscv32",
            cmd(&["qemu-system-riscv32", "--version"]),
        ),
        ("qemu-s390x", cmd(&["qemu-s390x", "--version"])),
        // The linker and QEMU of the LEON3 check (OI-08).
        ("sparc-ld", cmd(&["sparc64-linux-gnu-ld", "--version"])),
        (
            "qemu-system-sparc",
            cmd(&["qemu-system-sparc", "--version"]),
        ),
        ("python3", cmd(&["python3", "--version"])),
        ("git", cmd(&["git", "--version"])),
    ])
}

/// First line a command prints, or `unavailable`.
pub(super) fn first_line(args: &[String], dir: &Path) -> String {
    let Some((program, rest)) = args.split_first() else {
        return "unavailable".to_owned();
    };
    Command::new(program)
        .args(rest)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(|l| l.trim().to_owned())
        })
        .unwrap_or_else(|| "unavailable".to_owned())
}

/// Every installed Ubuntu package and its version.
fn packages() -> Result<BTreeMap<String, String>, String> {
    let out = Command::new("dpkg-query")
        .args(["-W", "-f", "${Package}=${Version}\\n"])
        .output()
        .map_err(|e| format!("dpkg-query: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(p, v)| (p.to_owned(), v.to_owned()))
        .collect())
}

/// Why the checkout no longer holds the commit's files, given `git status --porcelain
/// --untracked-files=no` run in it before any check: the entry point checked the commit out
/// clean, so a tracked file that differs was changed by building xtask, which `cargo xtask`
/// does without `--locked`. That is how a stale `Cargo.lock` shows: cargo resolves the
/// dependencies again and rewrites it, and the checks would then verify a lock file the commit
/// does not have. The links to the cache volume (`target` and the like) are untracked.
fn changed_checkout(status: &str) -> Option<String> {
    let status = status.trim_end();
    (!status.is_empty()).then(|| {
        format!(
            "the checkout differs from the commit before any check ran, so no check would \
             verify the commit's files (VER-056):\n{status}\nA stale Cargo.lock is rewritten \
             when cargo builds xtask: run a cargo command in the workspace, such as `cargo \
             check`, and commit Cargo.lock"
        )
    })
}

/// Runs every container check selected by `opts`, in run order, stopping at the first gating
/// failure with `--fail-fast` (`super::run_in_order`); returns the number of gating failures.
pub fn run(out: &Path, opts: &Options) -> Result<usize, String> {
    if std::env::var_os("ALDIS_VERIFY_INSIDE").is_none() {
        return Err(
            "`verify inside` runs only in the verification image; use `cargo xtask verify`"
                .to_owned(),
        );
    }
    let root = repo::root();
    if let Some(why) = changed_checkout(&git_in(
        &root,
        &["status", "--porcelain", "--untracked-files=no"],
    )?) {
        return Err(why);
    }
    let logs = out.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{}: {e}", logs.display()))?;
    let order: Vec<Check> = checks::run_order()
        .into_iter()
        .filter(|c| c.host != Host::MacOs)
        .collect();
    let (results, _) = run_in_order(&order, opts, None, |check| {
        if check.host == Host::ContainerX64 && std::env::consts::ARCH != "x86_64" {
            return Ok(outcome(
                check,
                Outcome::NotRun,
                Some("needs an x86_64 host"),
            ));
        }
        let temp = Path::new("/tmp/verify").join(&check.name);
        run_check(check, &root, &temp, &logs, &[])
    })?;
    let failed = order
        .iter()
        .zip(&results)
        .filter(|(c, r)| c.gating && r.get("result").and_then(Value::as_str) == Some("fail"))
        .count();
    let tools: BTreeMap<&str, String> = tool_commands()?
        .into_iter()
        .map(|(name, cmd)| (name, first_line(&cmd, &root)))
        .collect();
    let report = json!({
        "arch": std::env::consts::ARCH,
        "checks": results,
        "packages": packages()?,
        "tools": tools,
    });
    let path = out.join("container.json");
    std::fs::write(&path, canonical(&report)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::changed_checkout;

    // req: VER-056
    #[test]
    fn a_checkout_that_building_xtask_changed_is_refused() {
        assert_eq!(changed_checkout(""), None);
        assert_eq!(changed_checkout("\n"), None);
        let why = changed_checkout(" M Cargo.lock\n").unwrap();
        assert!(why.contains(" M Cargo.lock\n"), "{why}");
        assert!(why.contains("commit Cargo.lock"), "{why}");
    }
}
