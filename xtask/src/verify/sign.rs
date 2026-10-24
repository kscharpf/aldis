//! Signed verification evidence (docs/06-verification.md §7, VER-064, VER-065).
//!
//! A published record (VER-057) is signed with SSH, `ssh-keygen -Y sign`, over its exact bytes
//! (the canonical JSON of the note) in the namespace [`RECORD_NAMESPACE`]; `publish.rs` keeps the
//! signature as a sibling note on the same commit. A release tag is signed by git (`git tag -s`
//! with `gpg.format=ssh`), in git's own namespace [`TAG_NAMESPACE`]. Both are verified with
//! `ssh-keygen -Y verify` against the allowed signers [`ALLOWED_SIGNERS`] of the tip of
//! `origin/main`, which only a reviewed, merged commit changes: a branch that adds a key to its
//! own copy cannot sign its own evidence with it. Only while `origin/main` has no such file yet,
//! that is for the change that introduces it, is the file of the commit under check used.
//!
//! No private key is read here: the key is the one git signs with (`user.signingKey`), a path
//! or, with `key::`, a public key whose private half is in the SSH agent; `ssh-keygen` reads it.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::git_in;

/// The allowed signers file, at the root of the repository: one key per line, with the
/// namespaces it may sign in (`ssh-keygen(1)`, "ALLOWED SIGNERS").
pub const ALLOWED_SIGNERS: &str = ".github/allowed_signers";

/// The namespace of a record's signature (VER-064).
pub const RECORD_NAMESPACE: &str = "aldis-verify-record";

/// The namespace git signs tags and commits in (VER-065); git sets it, not this tool.
pub const TAG_NAMESPACE: &str = "git";

/// The first line of an armoured SSH signature.
const ARMOUR: &str = "-----BEGIN SSH SIGNATURE-----";

/// A directory under the temporary directory, removed when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty directory named after `what`.
    pub fn new(what: &str) -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("aldis-{what}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `bytes` to the file `name` in the directory; returns its path.
    pub fn file(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        let path = self.0.join(name);
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What `ssh-keygen` printed, and whether it succeeded.
struct Output {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// Runs `ssh-keygen` with `args`, `stdin` on its standard input. With `prompt`, its standard
/// error is the terminal's, for a passphrase or a security key's touch.
fn ssh_keygen(args: &[&std::ffi::OsStr], stdin: &[u8], prompt: bool) -> Result<Output, String> {
    use std::io::Write as _;
    let mut child = Command::new("ssh-keygen")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if prompt {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .spawn()
        .map_err(|e| format!("ssh-keygen: {e} (OpenSSH 8.2 or later is needed)"))?;
    if let Some(mut input) = child.stdin.take() {
        input
            .write_all(stdin)
            .map_err(|e| format!("ssh-keygen: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("ssh-keygen: {e}"))?;
    Ok(Output {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    })
}

/// Who made a signature: the principal the allowed signers name for its key, and the key's
/// fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signer {
    /// The principal, as the allowed signers file names it.
    pub principal: String,
    /// The key's fingerprint, `SHA256:...`, as `ssh-keygen` prints it.
    pub key: String,
}

impl fmt::Display for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.principal, self.key)
    }
}

/// The allowed signers that signatures are verified against, and where they were read.
#[derive(Clone, Debug)]
pub struct Signers {
    text: String,
    /// Where the file was read: `origin/main:.github/allowed_signers`, or the commit's own.
    pub from: String,
}

impl Signers {
    /// The allowed signers of the tip of `origin/main` as last fetched in the repository at
    /// `dir`; only when that tip has no such file, those of `commit` (VER-064).
    pub fn of(dir: &Path, commit: &str) -> Result<Self, String> {
        let show = |rev: &str| git_in(dir, &["show", &format!("{rev}:{ALLOWED_SIGNERS}")]);
        if git_in(
            dir,
            &["rev-parse", "--verify", "-q", "refs/remotes/origin/main"],
        )
        .is_err()
        {
            return Err(format!(
                "origin/main is not known, so neither is its {ALLOWED_SIGNERS}"
            ));
        }
        if let Ok(text) = show("refs/remotes/origin/main") {
            return Ok(Self {
                text,
                from: format!("origin/main:{ALLOWED_SIGNERS}"),
            });
        }
        if super::scope::is_hash(commit)
            && let Ok(text) = show(commit)
        {
            return Ok(Self {
                text,
                from: format!("{commit}:{ALLOWED_SIGNERS}, as origin/main has none yet"),
            });
        }
        Err(format!(
            "neither origin/main nor {commit} has {ALLOWED_SIGNERS}"
        ))
    }

    /// Allowed signers with the text `text`, read from `from`.
    #[cfg(test)]
    pub fn from_text(text: &str, from: &str) -> Self {
        Self {
            text: text.to_owned(),
            from: from.to_owned(),
        }
    }

    /// Who made `signature` of `data` in `namespace`, when it verifies against these allowed
    /// signers: its key is listed, for that namespace, and the signature is of exactly `data`.
    pub fn verify(&self, data: &[u8], signature: &str, namespace: &str) -> Result<Signer, String> {
        if !signature.trim_start().starts_with(ARMOUR) {
            return Err("is not an SSH signature".to_owned());
        }
        let scratch = Scratch::new("verify-signature")?;
        let allowed = scratch.file("allowed_signers", self.text.as_bytes())?;
        let sig = scratch.file("signature", signature.as_bytes())?;
        let find = ssh_keygen(
            &[
                "-Y".as_ref(),
                "find-principals".as_ref(),
                "-s".as_ref(),
                sig.as_os_str(),
                "-f".as_ref(),
                allowed.as_os_str(),
            ],
            b"",
            false,
        )?;
        if !find.ok {
            return Err(format!("is by a key that is not in {}", self.from));
        }
        let mut last = String::new();
        for principal in find
            .stdout
            .lines()
            .flat_map(|l| l.split(','))
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let out = ssh_keygen(
                &[
                    "-Y".as_ref(),
                    "verify".as_ref(),
                    "-f".as_ref(),
                    allowed.as_os_str(),
                    "-I".as_ref(),
                    principal.as_ref(),
                    "-n".as_ref(),
                    namespace.as_ref(),
                    "-s".as_ref(),
                    sig.as_os_str(),
                ],
                data,
                false,
            )?;
            if out.ok {
                let key = out
                    .stdout
                    .split_whitespace()
                    .find(|w| w.starts_with("SHA256:"))
                    .unwrap_or("?")
                    .to_owned();
                return Ok(Signer {
                    principal: principal.to_owned(),
                    key,
                });
            }
            last = out.stderr.replace('\n', "; ");
        }
        Err(format!("does not verify against {}: {last}", self.from))
    }
}

/// The key that git signs with in the repository at `dir` (`user.signingKey`, with
/// `gpg.format=ssh`), as a file `ssh-keygen -f` takes: the path, `~/` expanded, or a public key
/// given as `key::...` written to `scratch`, whose private half the SSH agent holds.
fn signing_key(dir: &Path, scratch: &Scratch) -> Result<PathBuf, String> {
    let setup = "set it up as AGENTS.md says under \"Toolchain and checks\" (VER-064)";
    let format = git_in(dir, &["config", "--get", "gpg.format"]).unwrap_or_default();
    if format != "ssh" {
        return Err(format!(
            "git does not sign with SSH here (gpg.format is `{format}`, not `ssh`): {setup}"
        ));
    }
    let key = git_in(dir, &["config", "--get", "user.signingKey"])
        .map_err(|_| format!("git has no signing key (user.signingKey): {setup}"))?;
    if let Some(public) = key.strip_prefix("key::") {
        return scratch.file("signing-key.pub", format!("{public}\n").as_bytes());
    }
    match key.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .ok_or_else(|| format!("user.signingKey is {key}, but HOME is not set")),
        None => Ok(PathBuf::from(key)),
    }
}

/// The SSH signature of `data` in `namespace`, made with the key git signs with in the
/// repository at `dir` (VER-064).
pub fn sign(dir: &Path, data: &[u8], namespace: &str) -> Result<String, String> {
    let scratch = Scratch::new("sign")?;
    let key = signing_key(dir, &scratch)?;
    let out = ssh_keygen(
        &[
            "-Y".as_ref(),
            "sign".as_ref(),
            "-n".as_ref(),
            namespace.as_ref(),
            "-f".as_ref(),
            key.as_os_str(),
        ],
        data,
        true,
    )?;
    if out.ok && out.stdout.starts_with(ARMOUR) {
        Ok(out.stdout)
    } else {
        Err(format!(
            "ssh-keygen could not sign with {}: is the key, or the agent holding it, available?",
            key.display()
        ))
    }
}

/// The commit that the release tag `tag` names, and who signed the tag, when it is an annotated
/// tag with an SSH signature that verifies against the allowed signers in git's namespace
/// (VER-065). `origin/main` is fetched first, for its allowed signers.
pub fn verify_tag(dir: &Path, tag: &str) -> Result<(String, Signer, String), String> {
    let name = format!("refs/tags/{tag}");
    let kind = git_in(dir, &["cat-file", "-t", &name])?;
    if kind != "tag" {
        return Err(format!(
            "{tag} is a lightweight tag: a release tag is annotated and signed (`git tag -s`)"
        ));
    }
    let raw = super::git_bytes(dir, &["cat-file", "tag", &name])?;
    let text = String::from_utf8(raw).map_err(|_| format!("{tag}: the tag is not UTF-8"))?;
    let Some(at) = text.find(ARMOUR) else {
        return Err(if text.contains("-----BEGIN PGP SIGNATURE-----") {
            format!("{tag} is signed with OpenPGP, not SSH")
        } else {
            format!("{tag} is not signed: a release tag is signed (`git tag -s`)")
        });
    };
    let (payload, signature) = text.split_at(at);
    let commit = git_in(dir, &["rev-parse", &format!("{name}^{{commit}}")])?;
    git_in(dir, &["fetch", "-q", "origin", "main"])?;
    let signers = Signers::of(dir, &commit)?;
    let signer = signers
        .verify(payload.as_bytes(), signature, TAG_NAMESPACE)
        .map_err(|why| format!("the signature of {tag} {why}"))?;
    Ok((commit, signer, signers.from))
}

/// `verify tag <tag>`: whether the release tag `tag` is signed by an allowed signer (VER-065).
pub fn tag(tag: &str) -> usize {
    tag_in(&crate::repo::root(), tag)
}

/// [`tag`] in the repository at `dir`; the number of problems.
pub(super) fn tag_in(dir: &Path, tag: &str) -> usize {
    match verify_tag(dir, tag) {
        Ok((commit, signer, from)) => {
            println!(
                "verify: tag {tag} names {commit}; signed by {signer}, verified against {from} (VER-065)"
            );
            0
        }
        Err(why) => {
            println!("verify: tag: {why}");
            1
        }
    }
}

#[cfg(test)]
pub mod testing {
    //! Throwaway SSH keys for the tests, made in temporary directories; never a real key.

    use std::path::PathBuf;
    use std::process::Command;

    use super::{RECORD_NAMESPACE, Scratch, TAG_NAMESPACE};

    /// An ed25519 key without a passphrase, made for one test and removed with it.
    pub struct TestKey {
        dir: Scratch,
        /// Its principal in the allowed signers.
        pub principal: String,
    }

    impl TestKey {
        /// A new key for `principal`.
        pub fn new(principal: &str) -> Self {
            let dir = Scratch::new("test-key").unwrap();
            let out = Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-C", principal, "-f"])
                .arg(dir.path().join("key"))
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
            Self {
                dir,
                principal: principal.to_owned(),
            }
        }

        /// The private key file.
        pub fn path(&self) -> PathBuf {
            self.dir.path().join("key")
        }

        /// The public key, `ssh-ed25519 AAAA...`.
        pub fn public(&self) -> String {
            let text = std::fs::read_to_string(self.dir.path().join("key.pub")).unwrap();
            text.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        }

        /// Its line in the allowed signers, for records and tags.
        pub fn line(&self) -> String {
            self.line_for(&format!("{TAG_NAMESPACE},{RECORD_NAMESPACE}"))
        }

        /// Its line in the allowed signers, for the namespaces `namespaces`.
        pub fn line_for(&self, namespaces: &str) -> String {
            format!(
                "{} namespaces=\"{namespaces}\" {}\n",
                self.principal,
                self.public()
            )
        }

        /// Its signature of `data` in the record namespace.
        pub fn sign(&self, data: &[u8]) -> String {
            self.sign_in(data, RECORD_NAMESPACE)
        }

        /// Its signature of `data` in `namespace`.
        pub fn sign_in(&self, data: &[u8], namespace: &str) -> String {
            use std::io::Write as _;
            let mut child = Command::new("ssh-keygen")
                .args(["-q", "-Y", "sign", "-n", namespace, "-f"])
                .arg(self.path())
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(data).unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success());
            String::from_utf8(out.stdout).unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TestKey;
    use super::*;
    use crate::verify::testing::Repo;

    // req: VER-064
    #[test]
    fn a_signature_verifies_only_for_its_data_its_namespace_and_a_listed_key() {
        let key = TestKey::new("maintainer@example.org");
        let other = TestKey::new("other@example.org");
        let signers = Signers::from_text(&key.line(), "the test's allowed signers");
        let data = br#"{"commit":"c"}"#;
        let sig = key.sign(data);
        assert_eq!(
            signers
                .verify(data, &sig, RECORD_NAMESPACE)
                .map(|s| s.principal),
            Ok("maintainer@example.org".to_owned())
        );
        assert!(
            signers
                .verify(data, &sig, RECORD_NAMESPACE)
                .unwrap()
                .key
                .starts_with("SHA256:")
        );
        // Altered data, another namespace, an unlisted key, and no signature all fail.
        let altered = signers.verify(br#"{"commit":"d"}"#, &sig, RECORD_NAMESPACE);
        assert!(altered.unwrap_err().starts_with("does not verify against"));
        assert!(signers.verify(data, &sig, TAG_NAMESPACE).is_err());
        let by_other = signers.verify(data, &other.sign(data), RECORD_NAMESPACE);
        assert_eq!(
            by_other,
            Err("is by a key that is not in the test's allowed signers".to_owned())
        );
        assert_eq!(
            signers.verify(data, "", RECORD_NAMESPACE),
            Err("is not an SSH signature".to_owned())
        );
        // A key listed for tags only signs no record.
        let tags_only = Signers::from_text(&key.line_for(TAG_NAMESPACE), "tags only");
        assert!(tags_only.verify(data, &sig, RECORD_NAMESPACE).is_err());
        // An empty file, or one with comments only, lists no signer.
        let empty = Signers::from_text("# no keys yet\n", "empty");
        assert!(empty.verify(data, &sig, RECORD_NAMESPACE).is_err());
        // A principal listed with others on one line verifies.
        let line = key.line().replacen(
            "maintainer@example.org",
            "a@example.org,maintainer@example.org",
            1,
        );
        let shared = Signers::from_text(&line, "shared");
        assert_eq!(
            shared
                .verify(data, &sig, RECORD_NAMESPACE)
                .map(|s| s.principal),
            Ok("a@example.org".to_owned())
        );
    }

    // req: VER-064
    #[test]
    fn the_allowed_signers_are_those_of_main_and_the_commits_own_only_before_main_has_them() {
        let origin = Repo::new("signers-origin");
        origin.commit("start");
        let clone = Repo::clone_of(&origin, "signers-clone");
        let head = clone.git(&["rev-parse", "HEAD"]);
        let of = |commit: &str| Signers::of(&clone.0, commit).map(|s| s.from);
        assert_eq!(of(&head), Ok(format!("origin/main:{ALLOWED_SIGNERS}")));
        // A branch that changes its own file still is checked against main's.
        clone.git(&["checkout", "-q", "-b", "topic"]);
        clone.write(
            ALLOWED_SIGNERS,
            "someone namespaces=\"aldis-verify-record\" x\n",
        );
        let topic = clone.commit("my own key");
        let signers = Signers::of(&clone.0, &topic).unwrap();
        assert_eq!(signers.from, format!("origin/main:{ALLOWED_SIGNERS}"));
        assert_eq!(signers.text, origin.key().line().trim());
        // Before main has the file, the commit's own is used, and said so.
        origin.git(&["rm", "-q", ALLOWED_SIGNERS]);
        origin.commit("no signers on main");
        clone.git(&["fetch", "-q", "origin"]);
        assert_eq!(
            of(&topic),
            Ok(format!(
                "{topic}:{ALLOWED_SIGNERS}, as origin/main has none yet"
            ))
        );
        assert!(of("HEAD").is_err());
        clone.git(&["checkout", "-q", "--detach", "origin/main"]);
        let bare = clone.git(&["rev-parse", "HEAD"]);
        assert_eq!(
            of(&bare),
            Err(format!(
                "neither origin/main nor {bare} has {ALLOWED_SIGNERS}"
            ))
        );
        clone.git(&["remote", "remove", "origin"]);
        assert!(
            of(&topic)
                .unwrap_err()
                .starts_with("origin/main is not known")
        );
    }

    // req: VER-064
    #[test]
    fn git_signs_with_its_ssh_signing_key() {
        let repo = Repo::new("signing-key");
        let key = TestKey::new("k@example.org");
        let signers = Signers::from_text(&key.line(), "test");
        let data = b"record";
        let err = sign(&repo.0, data, RECORD_NAMESPACE).unwrap_err();
        assert!(err.contains("gpg.format"), "{err}");
        repo.git(&["config", "gpg.format", "ssh"]);
        let err = sign(&repo.0, data, RECORD_NAMESPACE).unwrap_err();
        assert!(err.contains("user.signingKey"), "{err}");
        repo.git(&["config", "user.signingKey", key.path().to_str().unwrap()]);
        let sig = sign(&repo.0, data, RECORD_NAMESPACE).unwrap();
        assert!(signers.verify(data, &sig, RECORD_NAMESPACE).is_ok());
        // A key that is not there.
        repo.git(&["config", "user.signingKey", "/nonexistent/aldis-key"]);
        assert!(sign(&repo.0, data, RECORD_NAMESPACE).is_err());
        // `key::` names a public key; without an agent that holds it, signing fails.
        let literal = format!("key::{}", key.public());
        repo.git(&["config", "user.signingKey", &literal]);
        let scratch = Scratch::new("test-literal").unwrap();
        let file = signing_key(&repo.0, &scratch).unwrap();
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            format!("{}\n", key.public())
        );
        repo.git(&["config", "user.signingKey", "~/aldis-key"]);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(home) = home {
            assert_eq!(signing_key(&repo.0, &scratch), Ok(home.join("aldis-key")));
        }
    }

    // req: VER-065
    #[test]
    fn a_release_tag_verifies_when_signed_with_ssh_by_an_allowed_signer() {
        let origin = Repo::new("tag-origin");
        origin.commit("start");
        let clone = Repo::clone_of(&origin, "tag-clone");
        let key = origin.key();
        let signed_tag = |key: &TestKey, name: &str| {
            let config = format!("user.signingKey={}", key.path().display());
            let args = [
                "-c",
                "gpg.format=ssh",
                "-c",
                &config,
                "tag",
                "-s",
                "-m",
                "release",
                name,
            ];
            clone.git(&args)
        };
        signed_tag(key, "v1.0.0");
        let (commit, signer, from) = verify_tag(&clone.0, "v1.0.0").unwrap();
        assert_eq!(commit, clone.git(&["rev-parse", "HEAD"]));
        assert_eq!(signer.principal, key.principal);
        assert_eq!(from, format!("origin/main:{ALLOWED_SIGNERS}"));
        assert_eq!(tag_in(&clone.0, "v1.0.0"), 0);
        // Lightweight, unsigned, and by a key that is not allowed.
        clone.git(&["tag", "v1.0.1"]);
        assert!(
            verify_tag(&clone.0, "v1.0.1")
                .unwrap_err()
                .contains("lightweight")
        );
        clone.git(&["tag", "-a", "-m", "release", "v1.0.2"]);
        assert!(
            verify_tag(&clone.0, "v1.0.2")
                .unwrap_err()
                .contains("is not signed")
        );
        signed_tag(&TestKey::new("stranger@example.org"), "v1.0.3");
        let refused = verify_tag(&clone.0, "v1.0.3").unwrap_err();
        assert!(refused.contains("is by a key that is not in"), "{refused}");
        assert_eq!(tag_in(&clone.0, "v1.0.3"), 1);
        assert!(verify_tag(&clone.0, "no-such-tag").is_err());
    }
}
