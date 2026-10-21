# aldis-testkit

Tooling crate: the data model of the shared test-vector files, their loader, and the runner that checks a crate against them.

**Status:** in use since Milestone 0. Milestone 2, work package 0, is planned to add the shared property-test strategies of VER-054 ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Not part of the layered library. It is a tooling crate (RS-017): `std` only, never in a flight build, and used by the library crates only as a dev-dependency, which GEN-001 permits. Unlike the library crates it has third-party dependencies (`serde`, `serde_json`, and optionally `schemars`) and uses the tooling lint profile ([Rust profile](../../docs/profiles/rust.md) RS-017).

## What it provides

- `model`: the types of the vector files in [`test-vectors/`](../../test-vectors/README.md), codec files and engine scenario files, with their provenance. These types define the format: `cargo xtask vectors validate` reads every file with them, and `cargo xtask vectors schema` generates the JSON Schemas in [`test-vectors/schema/`](../../test-vectors/schema/) from them.
- `runner`: `run_codec_file` runs every case of a codec vector file through a handler the crate under test supplies, compares each `Outcome` exactly with the expected values, and returns a `Report`.
- `hex`: the hex encoding the vector files use.
- `oracle`: independent models that library code is checked against in unit tests and fuzz targets; `oracle::fhec` is the table model of the AOS Frame Header Error Control (M2 plan D10).
- `vectors_dir`: the path of `test-vectors/` in the repository.

Its users are the vector suites and unit tests of the library crates (for example [`aldis-spp/tests/vectors.rs`](../aldis-spp/tests/vectors.rs)), the fuzz targets, `xtask`, the bare-metal [exerciser](../../exerciser/), whose build script turns the vectors into data for the target, and the [cross-checks](../../crosscheck/) against independent implementations.

The crate documentation in [`src/lib.rs`](src/lib.rs) shows a handler.

## Features

| Feature | Effect |
|---|---|
| `schema` | JSON Schema generation for the model, through `schemars`; used by `xtask` |

## Specification

- [docs/07-test-vectors.md](../../docs/07-test-vectors.md): requirement area TV, the vector format, and the provenance rules.
- [docs/06-verification.md](../../docs/06-verification.md): VER-021 (every vector passes on every target) and VER-054 (property-test strategies defined once, in a tooling crate).
- [docs/README.md](../../docs/README.md): the index of the specification.
