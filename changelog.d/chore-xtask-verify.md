### Added
- `cargo xtask verify` (`xtask/src/verify/`): the verification run that gates merges (VER-056…VER-058, OI-31). It runs one check per former pull-request job of `ci.yml` and `cpp.yml`, under the same names, in a pinned image (`xtask/verify/Containerfile`) on a fresh checkout of HEAD. It writes an evidence record to `target/verify/<commit>/record.json`; `--publish` pushes the record as a git note under `refs/notes/verify` and sets the commit status `aldis/verify`. `verify check` re-checks a published record.
- `setup_cpp.py versions`: checks the installed C++ tools against `cpp/toolchains.lock` without reinstalling them; the script no longer calls `sudo` when it runs as root.

### Changed
- `ci.yml` and `cpp.yml` run nightly and on manual dispatch only; `nightly.yml` adds a `verify` job on both Linux architectures; every workflow opens an issue when a scheduled run fails (VER-059). `cpp.yml` drops its `changes` and `cpp-ok` jobs, which existed only for pull requests.
- `docs/verification/analysis-records.md` names the checks of `cargo xtask verify` instead of CI jobs; VER-056, VER-057, and VER-059 are recorded.
