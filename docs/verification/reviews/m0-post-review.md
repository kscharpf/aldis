# Code review: `m0/post-review`

Two rounds: [Round 1](#round-1) reviews the branch as first submitted; [Round 2](#round-2) re-checks
it after the fixes in `812deb1`.

## Round 1

- **Date:** 2026-10-17
- **Scope:** commits `86540e4` (Add the M1 plan: packets and time) and `4e087f3` (Address M0 post-review findings and M1 readiness gaps), 44 files, diffed against `main`
- **Tests:** `aldis-core` and `aldis-testkit` tests pass

None of the findings is a crash or a wrong wire value in `aldis-core`. The most serious are a
layering rule that is now stricter than the spec (1) and the removal of the `aldis` crate's default feature
without a note (2).

### Findings

#### 1. GEN-001 check now applies to boundary crates

`xtask/src/layers.rs:95` · correctness

Boundary crates (`aldis`, `aldis-ffi`) now fail GEN-001 for any third-party dependency that is not a
dev-dependency, build-dependencies included. GEN-001 only covers Layers 0–2
(`docs/01-architecture.md` §2.1).

**Failure scenario:** CABI-002 says the C header is generated with cbindgen. Adding `cbindgen` as a
build-dependency of `aldis-ffi` gives `kind: "build"` in cargo metadata, which is not `dev`.
`cargo xtask layers` (and `xtask ci`) then reports `GEN-001 aldis-ffi has third-party dependency cbindgen` and CI fails. The spec allows the dependency, but the tool forbids it.

#### 2. Facade default feature removed without a note

`crates/aldis/Cargo.toml:25` · correctness

`default = ["core"]` was removed, so `aldis` with default features now re-exports nothing. Neither
CHANGELOG nor the docs record this breaking change.

**Failure scenario:** A user with `aldis = { path = ... }` and default features who writes
`use aldis::core::crc::crc16;` could build before this commit. After it, the build fails with
``could not find `core` in `aldis` ``, and nothing explains why.

#### 3. FixedVec equality check compares a value to its own copy

`exerciser/src/exercise.rs:308` · test-coverage

`let other = vec;` copies the `FixedVec` (it derives `Copy`), so `vec == other` is always true. The
new `PartialEq`, which is meant to compare only elements `0..len`, is never really exercised.

**Failure scenario:** Change `FixedVec::eq` to compare the whole `items` array, or only `len`. The
exerciser check still passes, because both sides are bit-identical copies. A regression in the new
equality semantics goes unnoticed on target.

#### 4. `std` feature not passed through in `aldis-framework` and `aldis-ffi`

`crates/aldis-framework/Cargo.toml:17` · correctness

The PR passes `std` through to dependencies in spp, link, cop1 and aldis, but misses
`aldis-framework` (its `std` only enables `aldis-core/std`, not `aldis-link/std` or `aldis-cop1/std`)
and `aldis-ffi` (its `std = []` does not enable `aldis/std`).

**Failure scenario:** A host build of `aldis-ffi` with its default `std` feature, or
`aldis-framework --features std`, compiles `aldis-link`, `aldis-cop1` and `aldis-tc` without
`std`. Once those crates gate `std::error::Error` impls or std adapters behind `std` (RS-011), the
impls are missing in exactly the hosted builds that enable `std`.

#### 5. Error-detail matching in test vectors is undocumented

`crates/aldis-testkit/src/runner.rs:18` · correctness

Matching on error details (`ErrWith` and the `needed`/`expected`/`found`/`field` keys) was added to
the shared vector contract without documentation. `Expect.values` is still documented as "Expected
output values when `ok` is true", `docs/07-test-vectors.md` does not define the detail keys, and no
vector uses them.

**Failure scenario:** A C or binding implementation reading docs/07 and the schema has no
definition of these keys. Once a vector adds `"needed": 7`, those implementations fail it or ignore
it in different ways. `Outcome::Err(kind)` also duplicates `ErrWith(kind, empty)`, giving two code
paths for one result.

#### 6. `xtask ci` does a clean release rebuild on every run

`xtask/src/main.rs:63` · efficiency

`xtask ci` now runs `binaries::symbols`, which deletes `target/symbols` and does a fresh
`cargo build --release --all-features` every time. CI already runs `cargo xtask symbols` as its own
step in the `layers` job.

**Failure scenario:** Every local `cargo xtask ci` (which AGENTS.md requires before work is declared
done) pays for a full clean release rebuild. The deletion only exists to avoid stale rlibs. Listing
the produced rlibs from `cargo build --message-format=json` artifacts would avoid both the
deletion and the rebuild.

#### 7. VER-031 `aldis_core` symbol check always passes

`xtask/src/binaries.rs:190` · correctness

The new VER-031 check that `aldis_core::` symbols are present is always met by
`libaldis_core-*.rlib` itself, which is always in `deps/`. It says nothing about whether `aldis-spp`
uses or links `aldis-core`.

**Failure scenario:** Even if `aldis-spp` stopped referencing `aldis-core` entirely, `core_hits`
stays above 0 because `aldis_core`'s own rlib defines those symbols. So the "aldis_core symbols
present" evidence cited in `analysis-records.md` proves nothing, and the rlib-presence check only
repeats the `layers` dependency-closure check.

#### 8. `FrameSeqNum::new` returns `Option` but never fails

`crates/aldis-core/src/ids.rs:190` · simplification

`FrameSeqNum::new` returns `Option` but always returns `Some`, and it duplicates the new
`From<u8>`. Making the field private also breaks the public `FrameSeqNum(n)` constructor.

**Failure scenario:** Callers must handle a `None` that cannot happen. Under the no-unwrap rule
that means dead fallback branches, which lower branch coverage or need waivers. Downstream code
that built `FrameSeqNum(5)` or read `.0` no longer compiles, and nothing records the API break.
The simpler form is `const fn new(value: u8) -> FrameSeqNum`, or just `From<u8>`.

#### 9. Convoluted `SeqCount` fallback with an infinite loop

`exerciser/src/exercise.rs:200` · simplification

`SeqCount::new(..).unwrap_or(SeqCount::new(0).unwrap_or_else(|| loop { spin_loop() }))` is a
convoluted fallback with an infinite-loop branch, where `.unwrap_or_default()` (`SeqCount` derives
`Default`) does the same job.

**Failure scenario:** The inner `SeqCount::new(0)` is evaluated eagerly every time. The spin-loop
arm is unreachable, and if it ever ran it would hang instead of reporting a failure. It costs
readability and adds an unreachable path to the panic-link and stack-size analysis. The simpler
form is `SeqCount::new(bb(SeqCount::MAX)).unwrap_or_default()`.

#### 10. usize-to-u64 conversion written three different ways

`crates/aldis-core/src/container.rs:209` · simplification

The PR converts `usize` to `u64` three ways: a private `as_u64` in `bits.rs`,
`u64::try_from(..).unwrap_or(u64::MAX)` in `container.rs`, and separate `as` casts, each with its
own `#[expect]`, in `ids.rs` and `security.rs`.

**Failure scenario:** The same conversion is written and justified with
`#[expect(clippy::as_conversions)]` in several places, with different semantics (saturating versus
plain cast). One `pub(crate)` `as_u64` helper in `error.rs`, used by every `Error::out_of_range`
caller, would remove the duplication and the scattered lint exceptions.

### Minor

- **Inspection doc call chain is wrong.** `docs/verification/inspections/gen-022-023-aldis-core.md`
  says crc16 → `crc16_table` → `crc16_update_table`. The code goes
  crc16 → `crc16_update` → `crc16_update_table`.
- **Unused linker symbol.** `_stack_top` in `exerciser/link-generic.x` is never referenced.
- **Coverage gate can pass with no data.** In `xtask/src/coverage.rs`, if `repo::rel` fails to strip
  the repo root from the paths llvm-cov reports, the per-crate file filter skips every file. Totals
  become 0/0, and only the "no branch data" check would catch it.

## Round 2

- **Date:** 2026-10-17
- **Scope:** commit `812deb1` (Resolve findings from m0-post-review), checked against every Round 1
  item, plus a second pass over the full branch diff against `main`
- **Checks at `812deb1`, all passing:**
  - `cargo test -p aldis-core -p aldis-testkit --all-features`
  - `cargo xtask layers` (0 violations), `cargo xtask symbols`, `cargo xtask ci` (trace m0 60/60)
  - workspace clippy with all features, `cargo fmt --check`
  - bare-metal clippy for `aldis` and the thumbv7em exerciser build. The exerciser build was cached,
    so it was not re-linked against `812deb1`.

`812deb1` fully fixes 11 of the 13 Round 1 items and partly fixes 2. It adds no new correctness
bugs in `aldis-core`. The second pass found issues the first round missed, the most serious being
in-place edits to released test vectors (finding R2-1).

### Status of Round 1 items

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | GEN-001 check on boundary crates | Fixed | `xtask/src/layers.rs:84-97`: boundary and tooling crates are only checked for non-dev tooling dependencies, so a `cbindgen` build-dependency passes. |
| 2 | Facade default removed without a note | Fixed | CHANGELOG has a **Breaking:** entry, and the `crates/aldis/src/lib.rs` docs say no features are on by default, which matches `rust.md` §2. |
| 3 | FixedVec compared to its own copy | Fixed | `exercise.rs:306-312`: `other` is `[10, 99]` with len 1 and `vec` is `[10, 0]` with len 1, so the check really tests the `0..len` rule. A separate `vec != diff` check catches a len-only comparison. |
| 4 | `std` not passed through in framework and ffi | Fixed | framework: `std = ["aldis-core/std", "aldis-link/std", "aldis-cop1/std"]`; ffi: `std = ["aldis/std"]`. |
| 5 | Undocumented error-detail matching | Fixed | Removed: `ErrWith` and `compare_err` are gone, and the runner and `tests/vectors.rs` match `main` again. |
| 6 | `xtask ci` clean rebuild on every run | Fixed | `binaries::symbols` is no longer run by `ci` (CI's `layers` job still runs it, `ci.yml:107`). The `remove_dir_all` is gone, and rlibs now come from the `--message-format=json` artifact list. |
| 7 | VER-031 `aldis_core` check always passes | Partly fixed | The `core_hits` check was removed and `analysis-records.md` says "Vacuous until M2". The rlib-presence check was removed with it, so VER-031 now checks nothing (R2-2). |
| 8 | `FrameSeqNum::new` returns `Option` | Fixed | Now `const fn new(u8) -> FrameSeqNum`, and the field is `pub` again (as on `main`). As a side effect the type has four constructors and three accessors (R2-8). |
| 9 | Convoluted `SeqCount` fallback | Fixed | Now `SeqCount::new(bb(SeqCount::MAX)).unwrap_or_default()`; if the default is ever used, the check fails visibly. |
| 10 | usize-to-u64 conversion written three ways | Partly fixed | There is now one `pub(crate) as_u64` in `error.rs`, used by `bits.rs` and `container.rs`, and the `ids.rs` casts are gone. `security.rs:39-71` still has three `as` casts, each with its own `#[expect]`. `classify` lost `const` along the way (R2-6). |
| m1 | Inspection doc call chain | Fixed | Now reads `crc16` → `crc16_update` → `crc16_update_table`, matching `crc.rs:97-111`. |
| m2 | Unused `_stack_top` | Fixed | Removed from `link-generic.x`. It is still defined and used in `link-cortex-m.x`, as it should be. |
| m3 | Coverage gate can pass with no data | Fixed | `coverage.rs` returns an error when `matched_files == 0`. |

### Findings

#### R2-1. Released test vectors edited in place (TV-021)

`test-vectors/core/crc16.json:9` · correctness · missed in Round 1

The branch edits the `requirements` arrays of vectors that shipped in 0.1.0-m0. This breaks TV-021
("Vectors SHALL be immutable once released") and the AGENTS.md rule "never edit a released vector".

**Failure scenario:** crc16-check-001, crc16-empty-001, oid-start-001, oid-continuation-001,
layout-uslp-truncated-sdls and layout-err-uslp-truncated-sdls-too-short were changed after the
2026-10-16 release. A C or binding implementation pinned to 0.1.0-m0 sees the same case IDs with
different traceability, and the trace report credits CMN-004, CMN-022 and API-075 to cases that
did not claim them at release. The rule calls for adding a new case and marking the old one
superseded.

#### R2-2. VER-031 symbols check now checks nothing

`xtask/src/binaries.rs:180` · correctness

`812deb1` removed the rlib-presence check along with the useless `aldis_core` check. So
`xtask symbols` finds nothing until the frame crates contain code, yet VER-031 is still listed in
`analysis-records.md` as verified by method A.

**Failure scenario:** If aldis-spp gains a dependency on aldis-tm (which has no symbols yet), cargo
emits `libaldis_tm-*.rlib`, `hits` is 0, and symbols reports 0 problems. The old `rlib_hit` check
would have flagged it. `analysis-records.md:5` says to list a requirement only when the artefact
actually checks it, but the VER-031 row (line 14) says "Vacuous until … M2", and `trace --scope m0`
still counts VER-031 as traced.

#### R2-3. GEN-004 check ignores default features

`xtask/src/layers.rs:118` · correctness · missed in Round 1

The GEN-004 check only tests `optional`, but GEN-004 requires Layer-3 third-party dependencies to
be "only behind non-default features".

**Failure scenario:** aldis-framework adds `embedded-nal = { version = "…", optional = true }` with
`default = ["embedded-nal"]`. The dependency is optional, so `cargo xtask layers` reports 0
violations, but every default build pulls in a third-party crate, which GEN-004 forbids.

#### R2-4. Failed symbols build no longer shows compile errors

`xtask/src/binaries.rs:149` · correctness · introduced in `812deb1`

With `--message-format=json`, rustc diagnostics go to stdout as JSON, but `run()` only reports
stderr on failure.

**Failure scenario:** A compile error in aldis-spp or aldis-core makes `cargo xtask symbols` (CI job
`layers`) print only "aldis-spp release build failed:" plus cargo's "error: could not compile …"
summary. The actual error text is in the discarded stdout JSON.
`--message-format=json-render-diagnostics` would print the diagnostics to stderr and keep the
artifact JSON on stdout.

#### R2-5. OutOfRange detail reports a "found" value below the limit

`crates/aldis-core/src/bits.rs:114` · correctness

The octet helpers (and `Span::new` at line 37) report OutOfRange with `expected = buf.len()` and
`found = offset`. Other callers report an inclusive maximum.

**Failure scenario:** `read_u16(&[0; 8], 7)` returns `OutOfRange { field: offset, expected: 8,
found: 7 }`, and `get(&[0; 2], 9, 8)` returns `expected: 16, found: 9`. In both cases found is
below expected, so a caller can't tell why the call failed. The ids, container and counter callers
use an inclusive maximum (`MAX`, `N - PREFIX`, `mask`), so the right limit here is `len - N`, or
`end_bit` against `total_bits`. The `octet_int!` macro copies the problem to u32 and u64.

#### R2-6. `classify` is no longer `const fn`

`crates/aldis-core/src/ids.rs:152` · correctness · introduced in `812deb1`

To drop the `as` casts, `SeqCount::classify` and `FrameSeqNum::classify` were changed from
`const fn` to plain `fn`. They lose the const-evaluability the rest of the ID and counter API has.

**Failure scenario:** `const C: Continuity = FrameSeqNum(255).classify(FrameSeqNum(0));` compiled
at `4e087f3` and no longer compiles, while `next`, `get`, `new` and `classify_masked` are all
still const. A const widening helper for u8/u16, like `as_u64`, would keep the code lint-clean
and const.

#### R2-7. `rust.md` feature table not updated

`docs/profiles/rust.md:23` · docs

The §2 feature table does not list the facade's new `alloc` and `std` features. Its aldis-ffi row
shows no default features, although aldis-ffi has `default = ["std"]`. The fix for item 2 cites this
table as authoritative.

**Failure scenario:** A reader of §2 sees `aldis` with "one feature per crate, plus full" and
aldis-ffi with default "—". In fact a default build of aldis-ffi turns on std across the whole
stack, and aldis exposes `alloc` and `std` features the spec doesn't list. This also breaks the
AGENTS.md rule to update the requirement table in the same change.

#### R2-8. FrameSeqNum has too many constructors and accessors

`crates/aldis-core/src/ids.rs:181` · simplification

FrameSeqNum now has four ways to construct it (`FrameSeqNum(n)`, `new`, `From<u8>`, `Default`) and
three ways to read it (`.0`, `get`, `u8::from`). It is the only ID type with a public field.

**Failure scenario:** The exerciser and tests already mix `FrameSeqNum::from(0)` and
`FrameSeqNum(0)` in the same check (`exercise.rs:208-211`, `ids.rs:346-348`). With a public field, a
later range invariant can't be added without another breaking change. Keep one form, for example a
private field with `const fn new` and `From`, as SeqCount does.

#### R2-9. Tooling crates run a dependency loop that does nothing

`xtask/src/layers.rs:84` · simplification

The merged tooling/boundary branch runs the dependency filter for tooling crates but does nothing
with the result, because the inner `if BOUNDARY.contains(&name)` is always false for them.

**Failure scenario:** For aldis-testkit and xtask the loop walks every dependency and discards the
result, so a reader has to work out that tooling crates are in fact unchecked. Clearer:
`if TOOLING.contains(&name) { continue; }`, then a separate boundary branch with the
tooling-dependency check.

#### R2-10. Facade `alloc` misses the aldis-tc that cop1 pulls in

`crates/aldis/Cargo.toml:36` · correctness

The facade's `alloc` uses the weak form `aldis-tc?/alloc`, so `aldis --features cop1,alloc` does not
enable alloc on the aldis-tc that cop1 pulls in.

**Failure scenario:** A user enables `cop1` and `alloc` on the facade expecting alloc-backed TC
types. aldis-tc is compiled without `alloc`, because `?/` features apply only when the facade's own
`tc` feature is on, and cop1 has no `alloc` feature to pass through. Once aldis-tc gates APIs behind
`alloc` (M2), they are silently missing.
