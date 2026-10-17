# Code review: `m0/post-review`

- **Date:** 2026-10-17
- **Scope:** commits `86540e4` (Add the M1 plan: packets and time) and `4e087f3` (Address M0 post-review findings and M1 readiness gaps), 44 files, diffed against `main`
- **Tests:** `aldis-core` and `aldis-testkit` tests pass

None of the findings is a crash or a wrong wire value in `aldis-core`. The most serious are a
layering rule that is now stricter than the spec (1) and the removal of the `aldis` crate's default feature
without a note (2).

## Findings

### 1. GEN-001 check now applies to boundary crates

`xtask/src/layers.rs:95` · correctness

Boundary crates (`aldis`, `aldis-ffi`) now fail GEN-001 for any third-party dependency that is not a
dev-dependency, build-dependencies included. GEN-001 only covers Layers 0–2
(`docs/01-architecture.md` §2.1).

**Failure scenario:** CABI-002 says the C header is generated with cbindgen. Adding `cbindgen` as a
build-dependency of `aldis-ffi` gives `kind: "build"` in cargo metadata, which is not `dev`.
`cargo xtask layers` (and `xtask ci`) then reports `GEN-001 aldis-ffi has third-party dependency cbindgen` and CI fails. The spec allows the dependency, but the tool forbids it.

### 2. Facade default feature removed without a note

`crates/aldis/Cargo.toml:25` · correctness

`default = ["core"]` was removed, so `aldis` with default features now re-exports nothing. Neither
CHANGELOG nor the docs record this breaking change.

**Failure scenario:** A user with `aldis = { path = ... }` and default features who writes
`use aldis::core::crc::crc16;` could build before this commit. After it, the build fails with
``could not find `core` in `aldis` ``, and nothing explains why.

### 3. FixedVec equality check compares a value to its own copy

`exerciser/src/exercise.rs:308` · test-coverage

`let other = vec;` copies the `FixedVec` (it derives `Copy`), so `vec == other` is always true. The
new `PartialEq`, which is meant to compare only elements `0..len`, is never really exercised.

**Failure scenario:** Change `FixedVec::eq` to compare the whole `items` array, or only `len`. The
exerciser check still passes, because both sides are bit-identical copies. A regression in the new
equality semantics goes unnoticed on target.

### 4. `std` feature not passed through in `aldis-framework` and `aldis-ffi`

`crates/aldis-framework/Cargo.toml:17` · correctness

The PR passes `std` through to dependencies in spp, link, cop1 and aldis, but misses
`aldis-framework` (its `std` only enables `aldis-core/std`, not `aldis-link/std` or `aldis-cop1/std`)
and `aldis-ffi` (its `std = []` does not enable `aldis/std`).

**Failure scenario:** A host build of `aldis-ffi` with its default `std` feature, or
`aldis-framework --features std`, compiles `aldis-link`, `aldis-cop1` and `aldis-tc` without
`std`. Once those crates gate `std::error::Error` impls or std adapters behind `std` (RS-011), the
impls are missing in exactly the hosted builds that enable `std`.

### 5. Error-detail matching in test vectors is undocumented

`crates/aldis-testkit/src/runner.rs:18` · correctness

Matching on error details (`ErrWith` and the `needed`/`expected`/`found`/`field` keys) was added to
the shared vector contract without documentation. `Expect.values` is still documented as "Expected
output values when `ok` is true", `docs/07-test-vectors.md` does not define the detail keys, and no
vector uses them.

**Failure scenario:** A C or binding implementation reading docs/07 and the schema has no
definition of these keys. Once a vector adds `"needed": 7`, those implementations fail it or ignore
it in different ways. `Outcome::Err(kind)` also duplicates `ErrWith(kind, empty)`, giving two code
paths for one result.

### 6. `xtask ci` does a clean release rebuild on every run

`xtask/src/main.rs:63` · efficiency

`xtask ci` now runs `binaries::symbols`, which deletes `target/symbols` and does a fresh
`cargo build --release --all-features` every time. CI already runs `cargo xtask symbols` as its own
step in the `layers` job.

**Failure scenario:** Every local `cargo xtask ci` (which AGENTS.md requires before work is declared
done) pays for a full clean release rebuild. The deletion only exists to avoid stale rlibs. Listing
the produced rlibs from `cargo build --message-format=json` artifacts would avoid both the
deletion and the rebuild.

### 7. VER-031 `aldis_core` symbol check always passes

`xtask/src/binaries.rs:190` · correctness

The new VER-031 check that `aldis_core::` symbols are present is always met by
`libaldis_core-*.rlib` itself, which is always in `deps/`. It says nothing about whether `aldis-spp`
uses or links `aldis-core`.

**Failure scenario:** Even if `aldis-spp` stopped referencing `aldis-core` entirely, `core_hits`
stays above 0 because `aldis_core`'s own rlib defines those symbols. So the "aldis_core symbols
present" evidence cited in `analysis-records.md` proves nothing, and the rlib-presence check only
repeats the `layers` dependency-closure check.

### 8. `FrameSeqNum::new` returns `Option` but never fails

`crates/aldis-core/src/ids.rs:190` · simplification

`FrameSeqNum::new` returns `Option` but always returns `Some`, and it duplicates the new
`From<u8>`. Making the field private also breaks the public `FrameSeqNum(n)` constructor.

**Failure scenario:** Callers must handle a `None` that cannot happen. Under the no-unwrap rule
that means dead fallback branches, which lower branch coverage or need waivers. Downstream code
that built `FrameSeqNum(5)` or read `.0` no longer compiles, and nothing records the API break.
The simpler form is `const fn new(value: u8) -> FrameSeqNum`, or just `From<u8>`.

### 9. Convoluted `SeqCount` fallback with an infinite loop

`exerciser/src/exercise.rs:200` · simplification

`SeqCount::new(..).unwrap_or(SeqCount::new(0).unwrap_or_else(|| loop { spin_loop() }))` is a
convoluted fallback with an infinite-loop branch, where `.unwrap_or_default()` (`SeqCount` derives
`Default`) does the same job.

**Failure scenario:** The inner `SeqCount::new(0)` is evaluated eagerly every time. The spin-loop
arm is unreachable, and if it ever ran it would hang instead of reporting a failure. It costs
readability and adds an unreachable path to the panic-link and stack-size analysis. The simpler
form is `SeqCount::new(bb(SeqCount::MAX)).unwrap_or_default()`.

### 10. usize-to-u64 conversion written three different ways

`crates/aldis-core/src/container.rs:209` · simplification

The PR converts `usize` to `u64` three ways: a private `as_u64` in `bits.rs`,
`u64::try_from(..).unwrap_or(u64::MAX)` in `container.rs`, and separate `as` casts, each with its
own `#[expect]`, in `ids.rs` and `security.rs`.

**Failure scenario:** The same conversion is written and justified with
`#[expect(clippy::as_conversions)]` in several places, with different semantics (saturating versus
plain cast). One `pub(crate)` `as_u64` helper in `error.rs`, used by every `Error::out_of_range`
caller, would remove the duplication and the scattered lint exceptions.

## Minor

- **Inspection doc call chain is wrong.** `docs/verification/inspections/gen-022-023-aldis-core.md`
  says crc16 → `crc16_table` → `crc16_update_table`. The code goes
  crc16 → `crc16_update` → `crc16_update_table`.
- **Unused linker symbol.** `_stack_top` in `exerciser/link-generic.x` is never referenced.
- **Coverage gate can pass with no data.** In `xtask/src/coverage.rs`, if `repo::rel` fails to strip
  the repo root from the paths llvm-cov reports, the per-crate file filter skips every file. Totals
  become 0/0, and only the "no branch data" check would catch it.
