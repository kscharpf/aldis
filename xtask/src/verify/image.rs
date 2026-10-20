//! The verification image and the container engine (VER-056): podman or Docker Engine.
//!
//! The image is built from the files of the commit under test, read with `git show`, never from
//! the working tree. Its tag is a hash of those files, so a change to any pin builds a new image
//! and an unchanged commit reuses the one already built.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::{Options, git};
use crate::repo;

/// The files the image is built from, at their repository paths.
pub const INPUTS: &[&str] = &[
    ".github/actions/setup-cpp/setup_cpp.py",
    "cpp/toolchains.lock",
    "rust-toolchain.toml",
    "tooling-nightly",
    "xtask/verify/Containerfile",
    "xtask/verify/entrypoint.sh",
];

/// Named volume holding the Cargo registry and the Rust build output between runs.
const CACHE_VOLUME: &str = "aldis-verify-cache";

/// A container engine command line.
pub struct Engine {
    program: String,
}

impl Engine {
    /// `ALDIS_VERIFY_ENGINE`, else the first of podman and docker that answers.
    pub fn detect() -> Result<Self, String> {
        let candidates: Vec<String> = match std::env::var("ALDIS_VERIFY_ENGINE") {
            Ok(e) => vec![e],
            Err(_) => vec!["podman".to_owned(), "docker".to_owned()],
        };
        candidates
            .into_iter()
            .map(|program| Self { program })
            .find(|e| {
                Command::new(&e.program)
                    .arg("info")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            })
            .ok_or_else(|| {
                "no container engine answers: start podman (`podman machine start`) or Docker \
                 Engine, or name one in ALDIS_VERIFY_ENGINE (VER-056)"
                    .to_owned()
            })
    }

    /// `<engine> --version`, for the record.
    pub fn version(&self) -> String {
        super::inside::first_line(
            &[self.program.clone(), "--version".to_owned()],
            &repo::root(),
        )
    }

    fn output(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(&self.program)
            .args(args)
            .output()
            .map_err(|e| format!("{}: {e}", self.program))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        } else {
            Err(format!(
                "{} {}: {}",
                self.program,
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// Builds the image for `commit` unless it exists; returns its tag and image ID.
    pub fn ensure_image(&self, commit: &str) -> Result<(String, String), String> {
        let mut hasher = Sha256::new();
        let mut files = Vec::new();
        for path in INPUTS {
            let out = Command::new("git")
                .args(["show", &format!("{commit}:{path}")])
                .current_dir(repo::root())
                .output()
                .map_err(|e| format!("git: {e}"))?;
            if !out.status.success() {
                return Err(format!("{path} is not in {commit}"));
            }
            hasher.update(path.as_bytes());
            hasher.update([0]);
            hasher.update(
                u64::try_from(out.stdout.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            hasher.update(&out.stdout);
            files.push((*path, out.stdout));
        }
        let hash = super::record::hex(&hasher.finalize());
        let short = hash.get(..16).unwrap_or(&hash);
        let tag = format!("localhost/aldis-verify:{short}");
        let inspect = ["image", "inspect", "--format", "{{.Id}}", tag.as_str()];
        if let Ok(id) = self.output(&inspect) {
            return Ok((tag, id));
        }
        let context = repo::root()
            .join("target/verify")
            .join(format!("image-{short}"));
        let _ = std::fs::remove_dir_all(&context);
        for (path, bytes) in &files {
            let dest = context.join(path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        println!("verify: building {tag}; the first build downloads several GB");
        let arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            other => return Err(format!("unsupported host architecture {other}")),
        };
        let status = Command::new(&self.program)
            .args([
                "build",
                "--build-arg",
                &format!("TARGETARCH={arch}"),
                "-t",
                &tag,
                "-f",
            ])
            .arg(context.join("xtask/verify/Containerfile"))
            .arg(&context)
            .status()
            .map_err(|e| format!("{}: {e}", self.program))?;
        if !status.success() {
            return Err(format!("building {tag} failed"));
        }
        Ok((tag.clone(), self.output(&inspect)?))
    }

    /// Runs the container checks of `commit` in `tag`; returns `<out>/container.json`.
    pub fn run_checks(
        &self,
        tag: &str,
        commit: &str,
        out: &Path,
        opts: &Options,
    ) -> Result<Value, String> {
        let git_dir = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let mut args = vec![
            "run".to_owned(),
            "--rm".to_owned(),
            "-v".to_owned(),
            format!("{git_dir}:/repo:ro"),
            "-v".to_owned(),
            format!("{CACHE_VOLUME}:/cache"),
            "-v".to_owned(),
            format!("{}:/out", out.display()),
            tag.to_owned(),
            commit.to_owned(),
            "--out".to_owned(),
            "/out".to_owned(),
        ];
        if let (false, Some(base)) = (opts.skip.is_empty(), &opts.base) {
            args.extend([
                "--skip".to_owned(),
                opts.skip.join(","),
                "--base".to_owned(),
                base.clone(),
            ]);
        }
        if let Some(only) = &opts.only {
            args.extend(["--only".to_owned(), only.join(",")]);
        }
        // The exit status reports gating failures, which the record carries; only a missing
        // report is an error here.
        let _ = Command::new(&self.program)
            .args(&args)
            .status()
            .map_err(|e| format!("{}: {e}", self.program))?;
        let path = out.join("container.json");
        let text = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "{}: {e} (the container did not finish; see its output above)",
                path.display()
            )
        })?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}
