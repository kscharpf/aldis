### Changed
- `cargo xtask verify` takes a check's result from an earlier published record when the check's
  inputs are unchanged (VER-056): besides the record covering the merge base with `origin/main`,
  it searches the records of the branch's earlier commits (first-parent ancestors not on `main`,
  nearest first, at most 20). After a branch merges `main`, a check whose inputs the commits from
  `main` did not touch is `reused` from the branch's previous record instead of run; a check whose
  inputs did not change against the base is `skipped` as before. Each such check names its
  `source`, the commit and record SHA-256 of the run in which it actually passed, and `verify
  check` and `--publish` re-derive every skip and reuse from git and that record (VER-058).
- Records are now format `aldis-verify-record/3` (VER-057). Formats 1 and 2 are still read, and
  a tool that knows only those refuses format 3. The record's `image` names the input hash and
  where the image came from: `built`, or `pulled` with its reference, digest, and revision.
- The `verify` workflow (VER-060) pulls the verification image from
  `ghcr.io/kscharpf/aldis-verify`, tagged by the hash of its inputs and architecture, instead of
  building it, when the image's labels name the same input hash and a commit whose image inputs
  hash to it; otherwise it builds the image and pushes it. The nightly run builds the image and
  pushes it for `main`. `cargo xtask verify image --push <repository>` pushes from GitHub Actions
  only; `ALDIS_VERIFY_PULL=<repository>` opts a local run in to pulling.

### Fixed
- `xtask` tests: the temporary git repositories of the vector and verify-scope tests no longer use
  `git init -b` or `git diff --no-relative` (git 2.28), which failed the nightly big-endian job,
  whose `cross` image has an older git (#62).
- `cargo xtask verify` no longer skips a check because the record covering the base skipped it
  under an older rule: a result is taken only from the record in which the check ran, with inputs
  identical to the commit's. Before, a C++ check skipped by a format 1 record (whose C++ paths did
  not list `crates/*/Cargo.toml`, an input of every check since VER-056) could keep being skipped.
