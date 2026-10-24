### Added
- `cargo xtask verify`: signed verification evidence (VER-064, proposed in OI-45). `--publish`
  signs a local record with the SSH key git signs with and pushes the signature as a note under
  `refs/notes/verify-signature`, atomically with the record; `verify check`, `verify publish`, and
  the search for reuse sources refuse a record whose signature is missing or does not verify
  against `.github/allowed_signers` of `origin/main`. A record of a GitHub-hosted run (VER-060) is
  published unsigned and counts once `cargo xtask verify sign <commit>` has checked the run and
  its artefact and countersigned it.
- `cargo xtask verify tag <tag>`: checks that a release tag is annotated and signed with SSH by
  an allowed signer (VER-065, proposed in OI-45).

### Changed
- `verify.yml` no longer sets the commit status `aldis/verify` and drops its `statuses: write`
  permission; the countersignature sets the status (VER-060, proposed).
- The verification image installs `openssh-client`, for the xtask tests of the signatures.

### Security
- The evidence no longer rests on trust in everyone with push access: a forged, altered, or
  copied record, or a branch that lists its own key, fails `verify check`.
  [docs/06-verification.md](docs/06-verification.md) §7 states what the signatures protect and
  what they do not.
