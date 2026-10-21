//! Whether a merge had conflicts to resolve (VER-063): `git merge-tree --write-tree` redoes the
//! merge of its two parents in memory, without touching the working tree or the index, and names
//! the paths it could not merge (git 2.38 or later).

use std::path::Path;
use std::process::Command;

/// The paths that conflict when `theirs` is merged into `ours` in the repository at `dir`; empty
/// for a clean merge.
pub fn paths(dir: &Path, ours: &str, theirs: &str) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args([
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            ours,
            theirs,
        ])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    // Exit 0: clean; 1 with a tree ID: conflicts. Anything else, including exit 1 without a tree
    // (a commit git does not have), means the merge could not be computed.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let tree = stdout.lines().next().unwrap_or("");
    let has_tree = tree.len() >= 40 && tree.bytes().all(|b| b.is_ascii_hexdigit());
    match out.status.code() {
        Some(0) if has_tree => Ok(Vec::new()),
        Some(1) if has_tree => Ok(parse(&stdout)),
        _ => Err(format!(
            "git merge-tree {ours} {theirs}: {} (needs git 2.38 or later and both commits)",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// The conflicted paths of `git merge-tree --write-tree --name-only --no-messages` output: the
/// lines after the tree ID, up to the first empty line.
fn parse(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A throwaway git repository in the system temporary directory, removed on drop.
    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("aldis-xtask-metrics-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Self(dir);
            repo.git(&["init", "-q"]);
            repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
            repo
        }

        /// Runs git with a fixed identity, ignoring the user's and the system's configuration.
        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .args([
                    "-c",
                    "user.name=xtask",
                    "-c",
                    "user.email=xtask@example.invalid",
                ])
                .args([
                    "-c",
                    "commit.gpgSign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .current_dir(&self.0)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", self.0.join(".git/no-global-config"))
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        }

        fn commit(&self, files: &[(&str, &str)], message: &str) -> String {
            for (rel, body) in files {
                let path = self.0.join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, body).unwrap();
            }
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // req: VER-063
    #[test]
    fn finds_the_conflicted_paths_of_a_merge() {
        let repo = TempRepo::new("conflicts");
        repo.commit(
            &[("docs/plan.md", "a\nb\nc\n"), ("README.md", "x\n")],
            "base",
        );
        repo.git(&["checkout", "-q", "-b", "topic"]);
        let topic = repo.commit(
            &[("docs/plan.md", "a\nB (topic)\nc\n"), ("new.md", "n\n")],
            "topic",
        );
        repo.git(&["checkout", "-q", "main"]);
        let main = repo.commit(&[("docs/plan.md", "a\nB (main)\nc\n")], "main 1");
        // Both sides changed the same line of docs/plan.md.
        assert_eq!(paths(&repo.0, &topic, &main).unwrap(), ["docs/plan.md"]);
        // A change elsewhere merges cleanly.
        repo.git(&["checkout", "-q", "-b", "other", &format!("{main}~1")]);
        let other = repo.commit(&[("README.md", "y\n")], "other");
        assert_eq!(
            paths(&repo.0, &topic, &other).unwrap(),
            Vec::<String>::new()
        );
        // An unknown commit is an error, not a clean merge.
        assert!(paths(&repo.0, &topic, "0123456789abcdef0123456789abcdef01234567").is_err());
        // The working tree is untouched.
        assert_eq!(repo.git(&["status", "--porcelain"]), "");
    }

    // req: VER-063
    #[test]
    fn parses_merge_tree_output() {
        let out = "4b825dc642cb6eb9a060e54bf8d69288fbee4904\na.md\ndir/b.rs\n\nAuto-merging a.md\n";
        assert_eq!(parse(out), ["a.md", "dir/b.rs"]);
        assert_eq!(
            parse("4b825dc642cb6eb9a060e54bf8d69288fbee4904\n"),
            Vec::<String>::new()
        );
    }
}
