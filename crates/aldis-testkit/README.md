# aldis-testkit

Tooling crate: the data model of the shared test-vector files, their loader, and the runner that checks a crate against them.

**Status:** in use since Milestone 0. Milestone 2, work package 0b, added the shared property-test strategies of VER-054 ([M2 plan](../../docs/milestones/m2-frame-codecs.md) D6).

## Where it sits

Not part of the layered library. It is a tooling crate (RS-017): `std` only, never in a flight build, and used by the library crates only as a dev-dependency, which GEN-001 permits. Unlike the library crates it has third-party dependencies (`serde`, `serde_json`, and optionally `schemars` and `proptest`) and uses the tooling lint profile ([Rust profile](../../docs/profiles/rust.md) RS-017).

## What it provides

- `model`: the types of the vector files in [`test-vectors/`](../../test-vectors/README.md), codec files and engine scenario files, with their provenance. These types define the format: `cargo xtask vectors validate` reads every file with them, and `cargo xtask vectors schema` generates the JSON Schemas in [`test-vectors/schema/`](../../test-vectors/schema/) from them.
- `runner`: `run_codec_file` runs every case of a codec vector file through a handler the crate under test supplies, compares each `Outcome` exactly with the expected values, and returns a `Report`.
- `hex`: the hex encoding the vector files use.
- `vectors_dir`: the path of `test-vectors/` in the repository.
- `strategies` (features `strategies*`): the shared property-test strategies of VER-054. `common` covers `aldis-core` (identifiers and their out-of-range raw values, the decode mode, counters, bit fields, frame layouts, queue operations); `time`, `spp`, and `epp` cover the codec crates (formats, configurations, headers, and whole data units built through the crates' own checked constructors and encoders, and decoder input next to each boundary: one octet too short, a length field off by one, a version or reserved value changed). It re-exports `proptest`, and `strategies::config` sets a test's case count and regression file. Tests that use it are integration tests, `crates/<crate>/tests/properties.rs`: inside a unit test the crate's own types and the ones the strategies generate would be two copies of the crate (M2 plan D6). The codec crates are normal dependencies of this crate when their feature is on, without their default features and without the `time` feature of `aldis-spp`, so that the features change nothing in the test builds of those crates (GEN-006).

Its users are the vector suites and the property tests of the library crates (for example [`aldis-spp/tests/vectors.rs`](../aldis-spp/tests/vectors.rs) and [`aldis-spp/tests/properties.rs`](../aldis-spp/tests/properties.rs)), `xtask`, the bare-metal [exerciser](../../exerciser/), whose build script turns the vectors into data for the target, and the [cross-checks](../../crosscheck/) against independent implementations.

The crate documentation in [`src/lib.rs`](src/lib.rs) shows a handler.

## Features

| Feature | Effect |
|---|---|
| `schema` | JSON Schema generation for the model, through `schemars`; used by `xtask` |
| `strategies` | The property-test strategies module and `strategies::common`, through `proptest` and `aldis-core`; used by the property tests of `aldis-core` |
| `strategies-time`, `strategies-spp`, `strategies-epp` | `strategies` and `strategies::time`, `::spp`, or `::epp`, through that codec crate; each crate's property tests enable only its own, so that they link no other codec crate (`aldis-core`'s coverage would otherwise count the code its macros expand to in them, VER-024). `difftest/` is to enable all three (C1, CPP-060) |

## Specification

- [docs/07-test-vectors.md](../../docs/07-test-vectors.md): requirement area TV, the vector format, and the provenance rules.
- [docs/06-verification.md](../../docs/06-verification.md): VER-021 (every vector passes on every target), VER-023 (property tests), and VER-054 (property-test strategies defined once, in a tooling crate).
- [docs/README.md](../../docs/README.md): the index of the specification.
