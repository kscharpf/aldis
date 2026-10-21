//! The verification image and the container engine (VER-056): podman or Docker Engine.
//!
//! The image is built from the files of the commit under test, read with `git show`, never from
//! the working tree. Its tag is a hash of those files, so a change to any pin builds a new image
//! and an unchanged commit reuses the one already built.
//!
//! On a GitHub-hosted runner the image may instead be pulled from the project's registry
//! (VER-060): `ALDIS_VERIFY_PULL` names the repository, and an image is used only when its labels
//! name the same input hash and a commit whose image inputs hash to it. The workflows push the
//! images they build (`verify image --push`), tagged `<hash>-<arch>`.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{Options, git};
use crate::repo;

/// Label with the full SHA-256 of the image inputs.
const LABEL_INPUTS: &str = "dev.aldis.verify.inputs";
/// Label with the commit the image was built from (OCI annotation).
const LABEL_REVISION: &str = "org.opencontainers.image.revision";
/// Label with the repository the image was built from (OCI annotation); GitHub links the
/// package to that repository when it is first pushed.
const LABEL_SOURCE: &str = "org.opencontainers.image.source";
/// Environment variable naming the registry repository to pull the image from, if set.
const PULL_VAR: &str = "ALDIS_VERIFY_PULL";

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

    /// Runs the engine with its output shown; fails unless it succeeds.
    fn status(&self, args: &[&str]) -> Result<(), String> {
        let status = Command::new(&self.program)
            .args(args)
            .status()
            .map_err(|e| format!("{}: {e}", self.program))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{} {} failed", self.program, args.join(" ")))
        }
    }

    /// `image inspect` of `image` with the Go template `format`.
    fn inspect(&self, image: &str, format: &str) -> Result<String, String> {
        self.output(&["image", "inspect", "--format", format, image])
    }

    /// The image for `commit`: the local build if there is one; else, when `ALDIS_VERIFY_PULL`
    /// names a registry repository, the image pulled from it if its labels hold (VER-060); else
    /// a new local build.
    pub fn ensure_image(&self, commit: &str) -> Result<Image, String> {
        let (inputs, files) = input_hash(commit)?;
        let arch = arch()?;
        let local = format!("localhost/aldis-verify:{}", short(&inputs));
        let built = |tag: &str, id: String| Image {
            tag: tag.to_owned(),
            id,
            inputs: inputs.clone(),
            source: json!({"kind": "built"}),
        };
        if let Ok(id) = self.inspect(&local, "{{.Id}}") {
            return Ok(built(&local, id));
        }
        if let Some(repository) = std::env::var(PULL_VAR).ok().filter(|r| !r.is_empty()) {
            match self.pull(&repository, &inputs, arch) {
                Ok(image) => return Ok(image),
                Err(why) => println!("verify: not using the image from {repository}: {why}"),
            }
        }
        let context = repo::root()
            .join("target/verify")
            .join(format!("image-{}", short(&inputs)));
        let _ = std::fs::remove_dir_all(&context);
        for (path, bytes) in &files {
            let dest = context.join(path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        println!("verify: building {local}; the first build downloads several GB");
        let mut labels = vec![
            format!("{LABEL_INPUTS}={inputs}"),
            format!("{LABEL_REVISION}={commit}"),
        ];
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let (Some(server), Some(repository)) =
            (var("GITHUB_SERVER_URL"), var("GITHUB_REPOSITORY"))
        {
            labels.push(format!("{LABEL_SOURCE}={server}/{repository}"));
        }
        let mut args = vec![
            "build".to_owned(),
            "--build-arg".to_owned(),
            format!("TARGETARCH={arch}"),
        ];
        for label in labels {
            args.extend(["--label".to_owned(), label]);
        }
        args.extend(["-t".to_owned(), local.clone(), "-f".to_owned()]);
        args.push(
            context
                .join("xtask/verify/Containerfile")
                .display()
                .to_string(),
        );
        args.push(context.display().to_string());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.status(&args)
            .map_err(|_| format!("building {local} failed"))?;
        Ok(built(&local, self.inspect(&local, "{{.Id}}")?))
    }

    /// The image for the input hash `inputs` from `repository`, pulled unless it is already
    /// here, when its labels name `inputs` and a commit whose image inputs hash to `inputs`, and
    /// it is for `arch` (VER-060).
    fn pull(&self, repository: &str, inputs: &str, arch: &str) -> Result<Image, String> {
        let remote = remote_tag(repository, inputs, arch);
        if self.inspect(&remote, "{{.Id}}").is_err() {
            println!("verify: pulling {remote}");
            self.status(&["pull", "-q", &remote])
                .map_err(|_| format!("{remote} could not be pulled"))?;
        }
        let labels: Value =
            serde_json::from_str(&self.inspect(&remote, "{{json .Config.Labels}}")?)
                .map_err(|e| format!("the labels of {remote}: {e}"))?;
        let label = |k: &str| labels.get(k).and_then(Value::as_str).unwrap_or_default();
        if label(LABEL_INPUTS) != inputs {
            return Err(format!(
                "its label {LABEL_INPUTS} is `{}`, not {inputs}",
                label(LABEL_INPUTS)
            ));
        }
        let revision = label(LABEL_REVISION);
        let rehashed = revision_hash(revision)?;
        if rehashed != inputs {
            return Err(format!(
                "the image inputs of its commit {revision} hash to {rehashed}, not {inputs}"
            ));
        }
        let image_arch = self.inspect(&remote, "{{.Architecture}}")?;
        if image_arch != arch {
            return Err(format!("it is for {image_arch}, not {arch}"));
        }
        let digests: Vec<String> =
            serde_json::from_str(&self.inspect(&remote, "{{json .RepoDigests}}")?)
                .map_err(|e| format!("the digests of {remote}: {e}"))?;
        let digest = digests
            .iter()
            .find_map(|d| d.strip_prefix(&format!("{repository}@")))
            .ok_or_else(|| format!("{remote} has no digest from {repository}"))?
            .to_owned();
        println!("verify: using {remote} ({digest}), built from {revision}");
        Ok(Image {
            tag: remote.clone(),
            id: self.inspect(&remote, "{{.Id}}")?,
            inputs: inputs.to_owned(),
            source: json!({
                "kind": "pulled",
                "ref": remote,
                "digest": digest,
                "revision": revision,
            }),
        })
    }

    /// Pushes the image for `commit` to `repository` unless it came from there; only from a
    /// GitHub Actions workflow, which logs in with its token (VER-060). Returns the image.
    pub fn push(&self, commit: &str, repository: &str) -> Result<Image, String> {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return Err(
                "the image is pushed only by the project's GitHub Actions workflows (VER-060)"
                    .to_owned(),
            );
        }
        let image = self.ensure_image(commit)?;
        if image.source["kind"] == "pulled" {
            println!("verify: {} is already in {repository}", image.tag);
            return Ok(image);
        }
        let remote = remote_tag(repository, &image.inputs, arch()?);
        self.status(&["tag", &image.tag, &remote])?;
        self.status(&["push", &remote])?;
        println!("verify: pushed {remote}");
        Ok(image)
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

/// The verification image a run uses, and where it came from (VER-057, VER-060).
pub struct Image {
    /// The tag it is run by.
    pub tag: String,
    /// The engine's image ID.
    pub id: String,
    /// The SHA-256 of its inputs ([`INPUTS`]).
    pub inputs: String,
    /// `{"kind": "built"}`, or `{"kind": "pulled"}` with the reference, digest, and the commit
    /// its label names.
    pub source: Value,
}

impl Image {
    /// The record's `image` object.
    pub fn to_json(&self) -> Value {
        json!({"tag": self.tag, "id": self.id, "inputs": self.inputs, "source": self.source})
    }
}

/// The first 16 hex digits of a hash: the tag of the image.
fn short(hash: &str) -> &str {
    hash.get(..16).unwrap_or(hash)
}

/// The registry tag of the image for `inputs` and `arch` in `repository`.
fn remote_tag(repository: &str, inputs: &str, arch: &str) -> String {
    format!("{repository}:{}-{arch}", short(inputs))
}

/// The image architecture of the host.
fn arch() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        other => Err(format!("unsupported host architecture {other}")),
    }
}

/// One image input: its repository path and content.
type File = (&'static str, Vec<u8>);

/// The SHA-256 of the image inputs of `commit` (path, NUL, length, and content of each, in the
/// order of [`INPUTS`]), and the files.
fn input_hash(commit: &str) -> Result<(String, Vec<File>), String> {
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
    Ok((super::record::hex(&hasher.finalize()), files))
}

/// The hash of the image inputs of `revision`, a commit read from an image label, recomputed
/// by this tool; an error when it is not a full commit hash or not in this repository.
fn revision_hash(revision: &str) -> Result<String, String> {
    if !super::scope::is_hash(revision) {
        return Err(format!(
            "its label {LABEL_REVISION} `{revision}` is not a commit hash"
        ));
    }
    git(&["cat-file", "-e", &format!("{revision}^{{commit}}")])
        .map_err(|_| format!("the commit {revision} it was built from is not here"))?;
    Ok(input_hash(revision)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-060
    #[test]
    fn registry_tags_and_input_hashes() {
        let inputs = "0123456789abcdef".repeat(4);
        assert_eq!(
            remote_tag("ghcr.io/o/aldis-verify", &inputs, "amd64"),
            "ghcr.io/o/aldis-verify:0123456789abcdef-amd64"
        );
        // The hash of a commit's image inputs is recomputed from git, and is that of HEAD for
        // the commit that HEAD is.
        let head = git(&["rev-parse", "HEAD"]).unwrap();
        let (hash, files) = input_hash(&head).unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(files.len(), INPUTS.len());
        assert_eq!(revision_hash(&head), Ok(hash));
        assert!(revision_hash("HEAD").is_err());
        assert!(revision_hash(&"0".repeat(40)).is_err());
    }
}
