# Analysis and inspection records

Requirements whose verification method is **A** (analysis, usually an automated CI check) or **I** (inspection) are traced here. Tests and vectors carry their own annotations. `cargo xtask trace` reads this table (VER-020).

List a requirement here only when the artefact actually checks it. Remove the row if the check is removed.

| ID | Method | Artefact |
|---|---|---|
| VER-001 | A | CI job `clippy`: `cargo clippy --workspace --all-features --all-targets` with `-D warnings` and the workspace lint set (`Cargo.toml`) |
| VER-002 | A | `unsafe_code = "forbid"` in `[workspace.lints.rust]`, enforced by every CI build |
| VER-003 | A | CI job `deny`: `cargo deny --all-features check` with `deny.toml` |
| VER-004 | A | CI job `fmt`: `cargo fmt --all --check` |
| GEN-001, GEN-002, GEN-003, GEN-004 | A | CI job `layers`: `cargo xtask layers` (`xtask/src/layers.rs`) |
| GEN-005 | A | CI job `no-std`: `-p aldis --no-default-features --features full` for thumbv7em, armv7r, riscv32imac |
| GEN-070 | A | `unsafe_code = "forbid"` in the workspace lint table, inherited by all library crates; `aldis-ffi` uses `deny` (GEN-071) |
| RS-001 | A | `rust-toolchain.toml` pins 1.99.1; `.github/actions/setup-rust` installs only the pinned toolchain; `rust-version` in `Cargo.toml` |
| RS-002 | A | All CI build jobs use the pinned stable toolchain; nightly appears only in `nightly.yml` tooling jobs |
| RS-016, RS-017, RS-018 | I | `clippy.toml` (test exemptions) and lint tables in `Cargo.toml`, `crates/aldis-ffi/Cargo.toml`, `crates/aldis-testkit/Cargo.toml`, `xtask/Cargo.toml`; reviewed in each PR touching them |
| VER-020 | A | CI job `spec`: `cargo xtask trace --scope m0`; matrix uploaded as artefact |
| TV-001, TV-002, TV-003, TV-020 | A | CI job `spec`: `cargo xtask vectors validate` (model in `crates/aldis-testkit/src/model.rs`; schemas in `test-vectors/schema/`) |
| TV-021 | I | Review of every PR that touches `test-vectors/` (PR template checklist) |
| GEN-022, GEN-023 | I | Review of `aldis-core` (M0 WP-4, branch `m0/core`): no recursion; every loop is bounded by an input length, a const-generic capacity, or a constant (CRC: 8 steps per octet; bit fields: ≤ 9 octets; OID: 8 bits per octet; containers: record or buffer length ≤ N). To be confirmed by the PR reviewer |
| RS-015 | A, I | `missing_docs = "deny"` in every library crate (analysis); clause and requirement citations in `aldis-core` module and item docs reviewed in M0 WP-4 (inspection) |
| VER-030, GEN-007 | A | CI job `big-endian`: full test suite on `s390x-unknown-linux-gnu`; becomes a full vector check as crates add vector-driven tests |
