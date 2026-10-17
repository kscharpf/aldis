# AGENTS.md

Guide for coding agents working on **Aldis**. Read [README.md](README.md) first for the project overview, crate layout, and build commands. This file adds the rules you need before changing anything.

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh` to download them into `specs/pdf/` (see [specs/README.md](specs/README.md)). If this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code, never the other way round. **Never commit the PDFs or text copied from them.** Cite them by document number and clause.
2. **The specification** in [`docs/`](docs/README.md) defines everything else. Start with `docs/README.md`. Every requirement has an ID (e.g. `SPP-002`, `GEN-020`) and cites its CCSDS clause.
3. **Open decisions** are tracked in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not silently resolve an open issue in code: update the issue.

Never write protocol values from memory. Look them up in the Blue Book. Registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).

## Non-negotiable rules (Layers 0–2)

- `no_std`, **no heap**: fixed capacities via const generics (GEN-010, GEN-011).
- **No panics** on any input: no `unwrap`/`expect`, no unchecked indexing or arithmetic (GEN-020).
- No I/O, clocks, randomness, logging, or global state. Engines are sans-I/O and deterministic (GEN-030–032).
- No `unsafe` outside `aldis-ffi` (GEN-070). Every `unsafe` block there needs a `// SAFETY:` comment.
- No third-party runtime dependencies in Layers 0–2 (GEN-001).
- Layering: a crate may depend only on lower layers ([docs/01-architecture.md](docs/01-architecture.md) §2).

## Toolchain and checks

The toolchain is pinned to Rust **1.99.1** in `rust-toolchain.toml`. Do not change it without updating RS-001 and OI-07.

Run all of these before declaring work done. All must pass with **zero warnings**:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets
cargo test --workspace --all-features
cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf
```

Clippy runs with the `all` and `pedantic` groups denied, plus restriction lints ([docs/profiles/rust.md](docs/profiles/rust.md) §5). To relax a lint, use a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016). Never use crate-wide `allow`.

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared with future C/C++ and binding implementations. Follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`.
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
| Requirements and design | `docs/` |
| Conformance data | `test-vectors/` |
