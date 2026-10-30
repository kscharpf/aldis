# aldis-testkit

Tooling crate: the data model of the shared test-vector files, their loader, and the runner that checks a crate against them.

## Where it sits

Not part of the layered library. It is a tooling crate (RS-017): `std` only, never in a flight build, and used by the library crates only as a dev-dependency, which GEN-001 permits. Unlike the library crates it has third-party dependencies (`serde`, `serde_json`, and optionally `schemars` and `proptest`) and uses the tooling lint profile ([Rust profile](../../docs/profiles/rust.md) RS-017).
