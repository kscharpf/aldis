//! Repository paths and file walking.

use std::path::{Path, PathBuf};

/// Repository root (parent of the `xtask` crate).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Directories never scanned: build output, VCS data, and the git-ignored CCSDS documents.
const SKIP: &[&str] = &[
    "target",
    ".git",
    "specs/pdf",
    "exerciser/target",
    "fuzz/target",
    "fuzz/corpus",
    "fuzz/artifacts",
    "refapp/target",
    "crosscheck/target",
    "mutants.out",
    "mutants.out.old",
    // Git worktrees that Claude Code creates for parallel sessions: other branches, not this one.
    ".claude/worktrees",
    // Agent skills and local state
    ".agents",
];

/// All files under `dir` (relative to the root) whose extension is in `exts`, sorted.
pub fn files(dir: &str, exts: &[&str]) -> Vec<PathBuf> {
    let root = root();
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if SKIP
                .iter()
                .any(|s| rel == *s || rel.starts_with(&format!("{s}/")))
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// All files under `dir` (relative to the root) whatever their name, sorted, skipping the
/// same directories as [`files`] (used by `xtask cpp unsafe-optouts` for `.clang-tidy` and
/// `CMakeLists.txt`).
pub fn all_files(dir: &str) -> Vec<PathBuf> {
    let root = root();
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = rel(&path);
            if SKIP
                .iter()
                .any(|s| rel == *s || rel.starts_with(&format!("{s}/")))
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Path relative to the repository root, with forward slashes.
pub fn rel(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Reads a file as UTF-8, reporting the path on failure.
pub fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", rel(path)))
}

/// The pinned nightly used for tooling only (RS-002), from the `tooling-nightly` file.
pub fn tooling_nightly() -> Result<String, String> {
    let pin = read(&root().join("tooling-nightly"))?;
    let pin = pin.trim();
    if pin.starts_with("nightly-") && !pin.contains(char::is_whitespace) {
        Ok(pin.to_owned())
    } else {
        Err(format!(
            "tooling-nightly: expected `nightly-YYYY-MM-DD`, found `{pin}`"
        ))
    }
}
