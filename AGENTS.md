# AGENTS.md

Guide for coding agents working on **Aldis**. Read [README.md](README.md) first for the project overview, crate layout, and build commands. This file adds the rules you need before changing anything.

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh` to download them into `specs/pdf/` (see [specs/README.md](specs/README.md)). If this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code, never the other way round. **Never commit the PDFs or text copied from them.** Cite them by document number and clause.
2. **The specification** in [`docs/`](docs/README.md) defines everything else. Start with `docs/README.md`. Every requirement has an ID (e.g. `SPP-002`, `GEN-020`) and cites its CCSDS clause.
3. **Open decisions** are tracked in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not silently resolve an open issue in code: update the issue.

Never write protocol values from memory. Look them up in the Blue Book. Registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).

## Tracks

v1 is built in three tracks: **R** (Rust reference, milestones M0–M6), **C** (native C++, C0–C5), and **A** (reference application, A0–A6). Before starting work, find your track and milestone in [docs/08-roadmap.md](docs/08-roadmap.md), check that its entry gate is met on `main`, and follow the parallel-work rules in §6: one track's directories per PR, branch prefix `m<N>/`, `c<N>/`, or `a<N>/`, and the Rust crates as the reference when implementations disagree.

The rules and checks below are written for the Rust crates. The native C++ code follows [docs/profiles/cpp.md](docs/profiles/cpp.md) instead; its checks are set up in milestone C0.

## Non-negotiable rules (Layers 0–2)

- `no_std`, **no heap**: fixed capacities via const generics (GEN-010, GEN-011).
- **No panics** on any input: no `unwrap`/`expect`, no unchecked indexing or arithmetic (GEN-020).
- No I/O, clocks, randomness, logging, or global state. Engines are sans-I/O and deterministic (GEN-030–032).
- No `unsafe` outside `aldis-ffi` (GEN-070). Every `unsafe` block there needs a `// SAFETY:` comment.
- No third-party runtime dependencies in Layers 0–2 (GEN-001).
- Layering: a crate may depend only on lower layers ([docs/01-architecture.md](docs/01-architecture.md) §2).
- **Parse, don't validate** (RS-020): make invalid values unrepresentable. Give every field with a restricted range its own type with a checked constructor, keep fields private when a constraint spans them, and check untrusted input once, where it becomes the type. A constructor may check only the relation between values that are each already valid, such as a value fitting a format. Do not add `validate`/`check` functions that re-examine a value the type could have guaranteed.

## Toolchain and checks

The toolchain is pinned to Rust **1.99.1** in `rust-toolchain.toml`. Do not change it without updating RS-001 and OI-07.

Run all of these before declaring work done. All must pass with **zero warnings**:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets
cargo test --workspace --all-features
cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf
cargo xtask ci        # spec lint, links, layering, vector validation, schema check, M0 trace
```

When you change `aldis-core`, also run:

```sh
cargo xtask coverage                                   # 100 % regions/functions/lines/branches (VER-024)
(cd exerciser && cargo build --release --target thumbv7em-none-eabihf)  # panic-freedom link check (VER-010)
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets               # fuzz targets still compile
```

A failed exerciser link with `undefined symbol: aldis_panic_is_unreachable` means a panic path is reachable in `aldis-core`. Remove it; do not work around the check. Mark new test modules `#[cfg_attr(coverage_nightly, coverage(off))]`.

When you change `cpp/` (the native C++ implementation, [C++ profile](docs/profiles/cpp.md)), use the toolchains pinned in `cpp/toolchains.lock`; every preset refuses another compiler. Also run:

```sh
(cd cpp && cmake --preset host-gcc && cmake --build --preset host-gcc && ctest --preset host-gcc)  # and host-clang
cargo xtask cpp unsafe-optouts                 # bounds opt-outs only in aldis/core/bytes.hpp (CPP-031)
cargo xtask cpp layers target/cpp/host-gcc     # CMake target graph (CPP-010)
```

CI (`.github/workflows/cpp.yml`) also runs clang-format, clang-tidy, the sanitizers, s390x, and the bare-metal builds. Suppress a warning or clang-tidy finding only on the smallest region, naming the check (CPP-042).

When you change `refapp/` (track A), also run, from `refapp/` (see [refapp/README.md](refapp/README.md)):

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace
cargo build -p refapp-flight --target thumbv7em-none-eabihf
```

Clippy runs with the `all` and `pedantic` groups denied, plus restriction lints and selected nursery lints ([docs/profiles/rust.md](docs/profiles/rust.md) §5). Do not enable the whole `nursery` group; adopt a nursery lint only after checking its findings, and record the decision there (RS-019). To relax a lint, use a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016). Never use crate-wide `allow`.

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Verification records:** requirements verified by a CI check or by review (method A/I) are listed in [docs/verification/analysis-records.md](docs/verification/analysis-records.md).
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared with future C/C++ and binding implementations. Follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`.
- **Changelog:** never edit `CHANGELOG.md` in a pull request. Add one fragment file to [`changelog.d/`](changelog.d/README.md), named after the branch; `cargo xtask changelog check` validates it.
- **Errors** use the stable catalogue in [docs/03-api-model.md](docs/03-api-model.md) §9. Do not renumber error codes.
- **C ABI names** use the `aldis_` / `ALDIS_` prefix ([docs/profiles/c-abi.md](docs/profiles/c-abi.md)).
- **Spec changes:** when behavior changes, update the requirement table in the same change. New requirements get the next free ID in their area; IDs are never reused.

## Where things go

| Change | Location |
|---|---|
| Shared algorithm or type (CRC, counters, IDs, security hooks) | `crates/aldis-core` |
| Codec for one standard | `crates/aldis-<std>` (`time`, `spp`, `epp`, `tm`, `tc`, `aos`, `uslp`) |
| Stateful protocol behavior | `crates/aldis-link`, or `crates/aldis-cop1` for COP-1 |
| Transports, run loop, adapters | `crates/aldis-framework` |
| C interface | `crates/aldis-ffi` (capacities in `capacities.toml`) |
| Native C++ implementation (track C) | `cpp/` |
| Differential tests, Rust vs. native (VER-053) | `difftest/` |
| Reference application (track A) | `refapp/` |
| Requirements and design | `docs/` |
| Conformance data | `test-vectors/` |
| Native C++ implementation (track C) | `cpp/` ([cpp/README.md](cpp/README.md)) |
