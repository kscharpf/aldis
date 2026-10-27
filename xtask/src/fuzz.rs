//! `xtask fuzz`: run fuzz targets (VER-022, VER-056).
//!
//! `cargo xtask fuzz smoke` builds the fuzz target binaries and executes each one with
//! `-runs=1 -max_total_time=2` (passing `fuzz/corpus/<target>` when that directory exists)
//! to catch broken or stale assertions quickly before merge.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::consistency::bin_targets;
use crate::repo;

/// Arguments passed to the target binary for a smoke run.
pub(crate) fn fuzz_args(corpus_dir: Option<&Path>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(dir) = corpus_dir {
        args.push(dir.to_string_lossy().into_owned());
    }
    args.push("-runs=1".to_owned());
    args.push("-max_total_time=2".to_owned());
    args
}

/// Path to the compiled fuzz target binary.
pub(crate) fn target_binary_path(target: &str) -> PathBuf {
    let name = format!("{target}{}", std::env::consts::EXE_SUFFIX);
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        let path = PathBuf::from(target_dir).join("debug").join(&name);
        if path.exists() {
            return path;
        }
    }
    repo::root().join("fuzz/target/debug").join(name)
}

/// Builds the requested fuzz target binaries.
fn build_targets(targets: &[String], all_count: usize) -> Result<(), String> {
    let root = repo::root();
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root);
    cmd.args(["build", "--manifest-path"])
        .arg(root.join("fuzz/Cargo.toml"))
        .arg("--locked");
    if targets.len() == all_count {
        cmd.arg("--bins");
    } else {
        for t in targets {
            cmd.arg("--bin").arg(t);
        }
    }
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn cargo build: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo build for fuzz targets failed with status {status}"
        ));
    }
    Ok(())
}

/// Runs a fast smoke pass on all fuzz targets (or those named) to catch stale assertions.
///
/// Returns the number of failing targets, or an error if building or manifest parsing fails.
pub fn smoke(targets: &[&str]) -> Result<usize, String> {
    let root = repo::root();
    let manifest_path = root.join("fuzz/Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let all_targets: BTreeSet<String> = bin_targets(&manifest);
    if all_targets.is_empty() {
        return Err("no fuzz targets found in fuzz/Cargo.toml".to_owned());
    }

    let selected: Vec<String> = if targets.is_empty() {
        all_targets.iter().cloned().collect()
    } else {
        for t in targets {
            if !all_targets.contains(*t) {
                return Err(format!("unknown fuzz target: {t}"));
            }
        }
        targets.iter().map(|s| (*s).to_owned()).collect()
    };

    build_targets(&selected, all_targets.len())?;

    let mut failures = Vec::new();
    for target in &selected {
        let bin = target_binary_path(target);
        let corpus_dir = root.join("fuzz/corpus").join(target);
        let corpus_arg = if corpus_dir.is_dir() {
            Some(corpus_dir.as_path())
        } else {
            None
        };
        let args = fuzz_args(corpus_arg);
        let output = Command::new(&bin)
            .current_dir(&root)
            .args(&args)
            .output()
            .map_err(|e| format!("failed to run {}: {e}", bin.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprintln!(
                "fuzz smoke: target {target} failed (status {}):",
                output.status
            );
            if !stderr.is_empty() {
                eprintln!("{stderr}");
            }
            if !stdout.is_empty() {
                eprintln!("{stdout}");
            }
            failures.push(target.clone());
        }
    }

    if failures.is_empty() {
        println!("fuzz smoke: {} target(s) passed", selected.len());
        Ok(0)
    } else {
        eprintln!(
            "fuzz smoke: {} target(s) failed: {}",
            failures.len(),
            failures.join(", ")
        );
        Ok(failures.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-022, VER-056
    #[test]
    fn fuzz_args_contain_runs_and_timeout() {
        let without_corpus = fuzz_args(None);
        assert_eq!(without_corpus, ["-runs=1", "-max_total_time=2"]);

        let corpus = Path::new("fuzz/corpus/bits_roundtrip");
        let with_corpus = fuzz_args(Some(corpus));
        assert_eq!(
            with_corpus,
            ["fuzz/corpus/bits_roundtrip", "-runs=1", "-max_total_time=2"]
        );
    }

    // req: VER-022, VER-056
    #[test]
    fn binary_path_uses_exe_suffix() {
        let path = target_binary_path("bits_roundtrip");
        let expected_name = format!("bits_roundtrip{}", std::env::consts::EXE_SUFFIX);
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(expected_name.as_str())
        );
    }

    // req: VER-022
    #[test]
    fn smoke_rejects_unknown_targets() {
        let err = smoke(&["nonexistent_target"]).unwrap_err();
        assert!(err.contains("unknown fuzz target: nonexistent_target"));
    }
}
