//! Test fixtures of the verification run: throwaway git repositories, each with a throwaway SSH
//! key in its allowed signers, and a [`publish::Forge`] that records what it is asked.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use serde_json::{Map, Value, json};

use super::publish::{self, Run};
use super::record;
use super::sign::testing::TestKey;
use super::sign::{ALLOWED_SIGNERS, Scratch};

/// The notes ref of the records' signatures (VER-064).
pub const SIGNATURES: &str = "refs/notes/verify-signature";

/// A throwaway git repository under the temporary directory, and the key its allowed signers
/// list, shared with its clones.
pub struct Repo(pub PathBuf, Rc<TestKey>);

impl Repo {
    /// A new repository on `main`, whose first commit will hold allowed signers that list a new
    /// key (written, not yet committed).
    pub fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("aldis-verify-scope-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Self(dir, Rc::new(TestKey::new(&format!("{name}@example.org"))));
        // `init -b` needs git 2.28; the nightly big-endian job runs the tests in a `cross`
        // image with an older git.
        repo.git(&["init", "-q"]);
        repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
        repo.write(ALLOWED_SIGNERS, &repo.1.line());
        repo
    }

    /// A clone of `origin`, with its key.
    pub fn clone_of(origin: &Self, name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("aldis-verify-scope-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = Self(dir.clone(), Rc::clone(&origin.1));
        origin.git(&[
            "clone",
            "-q",
            origin.0.to_str().unwrap(),
            dir.to_str().unwrap(),
        ]);
        repo
    }

    /// The key that the allowed signers of the first commit list.
    pub fn key(&self) -> &TestKey {
        &self.1
    }

    /// Runs git in the repository, isolated from the user's configuration.
    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
            ])
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Configures the repository as a contributor's: an identity, no commit signing, and `key`
    /// as the SSH key that git, and so `verify publish`, signs with.
    pub fn sign_with(&self, key: &TestKey) {
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@t"),
            ("commit.gpgsign", "false"),
            ("gpg.format", "ssh"),
            ("user.signingKey", key.path().to_str().unwrap()),
        ] {
            self.git(&["config", k, v]);
        }
    }

    /// Writes `text` to `path` in the working tree.
    pub fn write(&self, path: &str, text: &str) {
        let file = self.0.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    /// Commits everything; returns the commit.
    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// A record of `commit` in `format` with the result entries `checks`, and a scope that
    /// decides as they say.
    pub fn record_for(&self, format: &str, commit: &str, checks: &Value) -> Value {
        let tree = self.git(&["rev-parse", &format!("{commit}^{{tree}}")]);
        let decisions: Map<String, Value> = checks
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let run = !matches!(e["result"].as_str(), Some("skipped" | "reused"));
                (e["name"].as_str().unwrap().to_owned(), json!({"run": run}))
            })
            .collect();
        let base = "0".repeat(40);
        json!({"format": format, "commit": commit, "tree": tree, "checks": checks,
            "scope": {"base": base, "base_record": base, "checks": decisions}})
    }

    /// Publishes `rec` as a note on this repository's notes ref, signed with its key; returns
    /// its SHA-256.
    pub fn publish_record(&self, rec: &Value) -> String {
        let sha = self.publish_unsigned(rec);
        let text = record::canonical(rec);
        self.add_signature(
            rec["commit"].as_str().unwrap(),
            &self.1.sign(text.as_bytes()),
        );
        sha
    }

    /// Publishes `rec` as a note on this repository's notes ref, without a signature; returns
    /// its SHA-256.
    pub fn publish_unsigned(&self, rec: &Value) -> String {
        let text = record::canonical(rec);
        let commit = rec["commit"].as_str().unwrap();
        self.git(&[
            "notes",
            "--ref=refs/notes/verify",
            "add",
            "-f",
            "-m",
            &text,
            commit,
        ]);
        record::sha256_hex(text.as_bytes())
    }

    /// Adds `signature` as the signature note of `commit`.
    pub fn add_signature(&self, commit: &str, signature: &str) {
        let scratch = Scratch::new("test-signature").unwrap();
        let file = scratch.file("sig", signature.as_bytes()).unwrap();
        self.git(&[
            "notes",
            &format!("--ref={SIGNATURES}"),
            "add",
            "-f",
            "-F",
            file.to_str().unwrap(),
            commit,
        ]);
    }

    /// Publishes a format 3 record of `commit` with the result entries `checks`; returns its
    /// SHA-256.
    pub fn publish(&self, commit: &str, checks: &Value) -> String {
        self.publish_record(&self.record_for(record::FORMAT, commit, checks))
    }

    /// The note of `commit` under `notes`, if it has one.
    pub fn note(&self, notes: &str, commit: &str) -> Option<String> {
        Command::new("git")
            .args(["notes", &format!("--ref={notes}"), "show", commit])
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A [`publish::Forge`] that has every commit, records the statuses set, and knows the GitHub
/// Actions runs it is given.
#[derive(Default)]
pub struct Forge {
    /// The commits whose status was set, with the link of each.
    pub statuses: RefCell<Vec<(String, Option<String>)>>,
    /// The runs, by URL.
    pub runs: RefCell<Vec<(String, Run)>>,
}

impl Forge {
    /// The commits whose status was set.
    pub fn commits(&self) -> Vec<String> {
        self.statuses
            .borrow()
            .iter()
            .map(|(c, _)| c.clone())
            .collect()
    }
}

impl publish::Forge for Forge {
    fn has_commit(&self, _: &Path, _: &str) -> Result<bool, String> {
        Ok(true)
    }

    fn set_status(&self, _: &Path, commit: &str, _: &str, url: Option<&str>) -> Result<(), String> {
        self.statuses
            .borrow_mut()
            .push((commit.to_owned(), url.map(str::to_owned)));
        Ok(())
    }

    fn run(&self, _: &Path, url: &str, _: &str) -> Result<Run, String> {
        self.runs
            .borrow()
            .iter()
            .find(|(u, _)| u == url)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| format!("no run {url}"))
    }
}
