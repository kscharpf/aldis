# aldis-testkit

Tooling crate: the data model of the shared test-vector files, their loader, and the runner that checks a crate against them.

## Where it sits

Not part of the layered library. It is a tooling crate (RS-017): `std` only, never in a flight build, and used by the library crates only as a dev-dependency, which GEN-001 permits. Unlike the library crates it has third-party dependencies (`serde`, `serde_json`, and optionally `schemars` and `proptest`) and uses the tooling lint profile ([Rust profile](../../docs/profiles/rust.md) RS-017).

## Features

| Feature | Effect |
|---|---|
| `schema` | JSON Schema generation for the model, through `schemars`; used by `xtask` |
| `strategies` | The property-test strategies module and `strategies::common`, through `proptest` and `aldis-core`; used by the property tests of `aldis-core` |
| `strategies-time`, `strategies-spp`, `strategies-epp`, `strategies-tm`, `strategies-tc`, `strategies-aos` | `strategies` and `strategies::time`, `::spp`, `::epp`, `::tm`, `::tc`, or `::aos`, through that codec crate; each crate's property tests enable only its own, so that they link no other codec crate (`aldis-core`'s coverage would otherwise count the code its macros expand to in them, VER-024). `difftest/` is to enable all three (C1, CPP-060) |

## Specification

- [docs/07-test-vectors.md](../../docs/07-test-vectors.md): requirement area TV, the vector format, and the provenance rules.
- [docs/06-verification.md](../../docs/06-verification.md): VER-021 (every vector passes on every target), VER-023 (property tests), and VER-054 (property-test strategies defined once, in a tooling crate).
- [docs/README.md](../../docs/README.md): the index of the specification.
