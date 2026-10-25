### Added
- `cargo xtask assurance [--strict]` (M2 WP-8; VER-065, VER-066, specified in
  [docs/verification/assurance-evidence.md](docs/verification/assurance-evidence.md)): every
  reference of the hazard analysis and the assurance case resolves (a file, test, proof, vector
  case, record, gating or nightly-only check, step of `cargo xtask ci`, nightly job, fuzz target,
  open issue, hazard, claim, assumption, defeater, or proposal), evidence that runs only at night
  says so by its kind, both documents follow their formats, and every hazard supports a claim;
  `--strict` also fails on anything left as TODO. It runs in `cargo xtask ci` and the verify
  check `spec`, and writes `target/spec/assurance.json` and `target/spec/assurance.md`.
