# Code review: `m0/post-review`

> Dated review: identifiers reflect the code at the time.

Four rounds: [Round 1](#round-1) reviews the branch as first submitted; [Round 2](#round-2)
re-checks it after the fixes in `812deb1`; [Round 3](#round-3) re-checks the fixes for Round 2
(committed as `4162d92`); [Round 4](#round-4) re-checks the fixes for Round 3 in `2dbe2eb`.

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

## Round 3

- **Date:** 2026-10-17
- **Scope:** the fixes for Round 2, reviewed as uncommitted working-tree changes on top of `db60c2c`
  (16 files). Each was checked against R2-1 to R2-10 and the two Round 1 items left partly fixed
  (7 and 10), followed by another pass over the full branch against `main`.
- **Checks on the working tree, all passing:**
  - `cargo test -p aldis-core -p aldis-testkit --all-features` and `cargo test --workspace --all-features`
  - `cargo xtask layers` (0 violations), `cargo xtask symbols` (3 rlibs, 0 frame crates found),
    `cargo xtask ci` (6 vector files, 83 cases, trace m0 60/60)
  - `cargo xtask coverage`: 0 misses for regions, functions, lines and branches (896, 168, 768, 64)
  - `cargo fmt --check` (workspace and exerciser); workspace clippy with all features in a fresh
    target dir; bare-metal clippy for `aldis --features full` on thumbv7em. Fuzz clippy was cached.
  - **Exerciser rebuilt from scratch** (fresh target dir, `--locked`): the thumbv7em release link
    and clippy pass, the injected-panic canary fails to link with `aldis_panic_is_unreachable` as
    it should, and the QEMU mps2-an386 run exits 0. Earlier rounds only had a cached build.

Of the 12 items, 10 are fully fixed, R2-3 is fixed with caveats, and R2-5 is only partly fixed. No
crash or wrong-output bug was found in `aldis-core`. The new findings are smaller than in earlier
rounds: one remaining error-detail bug (R3-1), two gaps in the tooling (R3-2 to R3-4), and docs and
cleanup items.

### Status of Round 2 items

| # | Item | Status | Evidence |
|---|---|---|---|
| R2-1 | Released vectors edited in place | Fixed | A field-by-field JSON comparison shows all 6 released cases match the `v0.1.0-m0` tag, apart from the added `superseded { reason, by }` block. The extra traceability lives only in new cases: `crc16-check-002`, `crc16-empty-002`, `oid-start-002`, `oid-continuation-002`, `layout-uslp-truncated-sdls-002`, `layout-err-uslp-truncated-sdls-too-short-002`. The trace tool, the runner and the exerciser's `build.rs` skip superseded cases. `tests/vectors.rs:210-221` now picks the first live case. |
| R2-2 | VER-031 checks nothing | Fixed | `binaries.rs:188-203` restores the rlib-presence check next to the symbol check, and `analysis-records.md:14` no longer says "vacuous". |
| R2-3 | GEN-004 ignores default features | Fixed, with caveats | `layers.rs:46-82` follows the `default` feature through the feature graph, and `layers.rs:166` uses the result. The new code is untested and can report false errors (R3-3, R3-4). |
| R2-4 | Compile errors hidden | Fixed | `binaries.rs:149` uses `--message-format=json-render-diagnostics`. |
| R2-5 | "found" below the limit | Partly fixed | `bits.rs:39`, `:121` and `:138` now report the largest valid offset, but it saturates to 0 when the buffer is shorter than the field (R3-1). |
| R2-6 | `classify` no longer const | Fixed | `ids.rs:161` and `ids.rs:216` are `const fn` again, and a const test at `ids.rs:332-338` keeps them that way. |
| R2-7 | `rust.md` feature table | Fixed | `rust.md:21-24` lists the new `alloc` and `std` features and shows aldis-ffi's default as `std`. |
| R2-8 | FrameSeqNum has too many forms | Fixed | The field is private (`ids.rs:190`), the CHANGELOG has a **Breaking** entry, and the call sites now use `new`. |
| R2-9 | Loop in `layers.rs` that does nothing | Fixed | `layers.rs:131-146`: tooling crates are skipped first, and boundary crates have their own branch. |
| R2-10 | Facade `alloc` misses aldis-tc | Fixed | cop1 and framework gained an `alloc` feature and the facade forwards to both. `cargo tree` confirms `aldis --features cop1,alloc` turns on alloc in aldis-tc. |
| R1-7 | VER-031 check | Fixed | Resolved by the R2-2 change. |
| R1-10 | usize-to-u64 conversions | Fixed | The `as` casts in `security.rs` are gone because the fields are now `usize`. Two new widening helpers were added in `ids.rs` (R3-7, R3-8). |

### Findings

#### R3-1. Short buffer reports `expected: 0, found: 0` (R2-5 only partly fixed)

`crates/aldis-core/src/bits.rs:121` · correctness

When the buffer (or bit span) is shorter than the field, the saturating "largest valid offset"
becomes 0, so the error reports found equal to expected and looks valid.

**Failure scenario:** `read_u32(&[0; 2], 0)` returns `OutOfRange { field: offset, expected: 0,
found: 0 }`, and `get(&[0; 1], 0, 16)` (`Span::new`, line 39) also gives expected 0, found 0. Under
the new inclusive-maximum meaning, found <= expected says the call should have worked, so the
caller still can't tell why it failed, which is the problem R2-5 described. No offset is valid in
this case, so the detail should report the needed length against the buffer length (for example
`ErrorDetail::Needed`, or expected = N, found = len). No test covers the short-buffer case. The
width check at line 30 (width 0 gives expected 64, found 0) and `counter.rs:63` (bits 0) have the
same found-below-limit problem.

#### R3-2. TV-021 validation doesn't check `superseded.by` or catch edits

`xtask/src/vectors.rs:112` · correctness

`vectors validate` claims to check TV-021, but it never checks that `superseded.by` names an
existing, live case, and nothing automated stops a released case from being edited.

**Failure scenario:** A typo such as `"by": "crc16-check-02"`, or a `by` that points to a case that
is itself superseded, passes validation and `xtask ci`. The superseded case is then skipped
everywhere (runner, trace, exerciser `build.rs`), so its coverage silently disappears. Nothing
would have caught R2-1 either: editing the requirements of a released case passes every check.
Instead of relying on review, resolve each `by` against the id map validate already builds, and
compare cases that are not superseded against the last release tag
(`git show v0.1.0-m0:test-vectors/...`).

#### R3-3. GEN-004 check mixes feature names and dependency names

`xtask/src/layers.rs:74` · correctness

`default_enabled_deps` puts feature names and dependency names into one set, so a default feature
that only shares a dependency's name marks that dependency as on by default.

**Failure scenario:** With `{"default": ["serde"], "serde": [], "json": ["dep:serde"]}` the function
returns `{default, serde}`. A Layer-3 crate with a default `serde` feature that does not pull in the
optional `serde` dependency (only the non-default `json` feature does) then gets a false GEN-004
error in `cargo xtask layers` and in CI. Keep features (for the traversal) and dependency names
(from `dep:` and `x/feat` entries, plus implicit `name = ["dep:name"]` features) in separate sets.

#### R3-4. New GEN-004 feature-graph code is untested

`xtask/src/layers.rs:47` · test-coverage

The new feature-graph code has no tests, and no workspace crate exercises it, because no Layer-3
crate has a third-party dependency yet.

**Failure scenario:** `default_enabled_deps` handles `dep:`, weak `x?/f`, `x/f`, renames and
recursive features, but `cargo xtask layers` passes whatever it returns, because aldis-framework has
no third-party dependencies. A regression, such as treating `x?/f` as enabling x or ignoring
renames, would go unnoticed until M2 adds embedded-nal and the GEN-004 analysis record (method A)
is relied on. xtask already has a `#[cfg(test)]` pattern in `md.rs`; add table-driven tests over
small JSON feature maps.

#### R3-5. FrameSeqNum contradicts RS-010 as written

`docs/profiles/rust.md:69` · docs

The CHANGELOG justifies the FrameSeqNum change with RS-010, but RS-010 requires
`const fn new(..) -> Option<Self>`, and `FrameSeqNum::new` returns `Self`. `From<u8>` is also an
unchecked constructor, which RS-010 forbids in the public API.

**Failure scenario:** Someone checking the code against the spec (method T) finds that FrameSeqNum
breaks RS-010 as written, while `CHANGELOG.md:16` says it complies. The infallible design is right,
because all 256 values are valid. RS-010 should be reworded in the same change (AGENTS.md: "update
the requirement table in the same change") to exempt identifiers that use the full width of their
type.

#### R3-6. aldis-ffi row says it mirrors the facade, but it doesn't

`docs/profiles/rust.md:24` · docs

The aldis-ffi row edited for R2-7 still says it "mirrors the facade", but aldis-ffi has no
per-crate features and always turns on `aldis/full`.

**Failure scenario:** A reader of §2 expects to build aldis-ffi with only, say, `spp`, as the facade
allows. In fact `crates/aldis-ffi/Cargo.toml` enables `aldis` with `features = ["full"]` and offers
only `std` and `panic-handler`, so every FFI build compiles every protocol crate. Either drop
"mirrors the facade" or add the forwarding features.

#### R3-7. SecurityLayout fields widened to `usize` to avoid casts

`crates/aldis-core/src/security.rs:16` · efficiency

Widening SecurityLayout's fields from u8/u16 to usize to avoid `as` casts makes the struct 4x
larger on 64-bit targets and 2x larger on 32-bit targets. The type is stored per VC or MAP in
fixed-capacity configs.

**Failure scenario:** SecurityLayout grows from 4 to 16 bytes on hosts and from 4 to 8 bytes on
thumbv7em, and the growth repeats in every LayoutSpec and per-channel config table. GEN-010/011
emphasise small, fixed memory, and the change only avoids a cast. Keeping u8/u16 storage with one
checked const narrowing helper (or the existing `#[expect]`-justified casts) keeps both the size and
the const fn.

#### R3-8. A third style of integer-widening helper

`crates/aldis-core/src/ids.rs:120` · simplification

The new private `u8_to_u64` and `u16_to_u64` helpers add another way to widen integers, next to
`error::as_u64` (an `as` cast with `#[expect]`) and the `from_be_bytes` trick in `crc.rs`.

**Failure scenario:** R1-10 asked for one shared conversion helper. The crate now has `as_u64` in
`error.rs`, two byte-shuffling helpers private to `ids.rs`, and `from_be_bytes` in
`crc.rs::step_bitwise`, so the next module that needs a const widening will write its own again.
Move these next to `as_u64` in `error.rs` as `pub(crate) const fn`s (or into a small `widen` module)
and reuse them in counter, ids and crc.

#### R3-9. `MAX_TRAILER_LEN` is private while `MAX_HEADER_LEN` is public

`crates/aldis-core/src/security.rs:28` · api

Callers can see one limit of `SecurityLayout::new` but not the other.

**Failure scenario:** Code that checks SDLS trailer lengths before calling `SecurityLayout::new`
(for example an M2 configuration loader) can use `SecurityLayout::MAX_HEADER_LEN` but has to
hard-code 65535 for the trailer, and that copy can drift from the real check. Make it
`pub const MAX_TRAILER_LEN` with a doc comment citing the requirement, like `MAX_HEADER_LEN`.

## Round 4

- **Date:** 2026-10-17
- **Scope:** commit `2dbe2eb` (round 3 resolution, 13 files), checked against R3-1 to R3-9, plus
  another pass over the full branch against `main`. The behaviour of the released-vector and
  feature-graph checks was tried out on a scratch clone, not in the repo.
- **Checks at `2dbe2eb`, all passing with zero warnings:**
  - `cargo fmt --all --check`, `cargo clippy --workspace --all-features --all-targets`,
    `cargo test --workspace --all-features`, and the 4 tests in `cargo test -p xtask`
  - bare-metal clippy for `aldis --features full` on thumbv7em
  - `cargo xtask ci`: 0 layering violations; 6 vector files, 83 cases, 0 problems; trace m0 60/60
  - `cargo xtask coverage`: 0 misses (regions 899, functions 170, lines 772, branches 68)
  - exerciser built from scratch in a fresh target dir (`--locked`): the release link and clippy
    pass, and the QEMU mps2-an386 run exits 0
  - fuzz clippy

Of the 9 items, 7 are fully fixed, R3-1 is fixed with caveats, and R3-2 is only partly fixed. The
library code has settled down. Most findings in this round are in the released-vector check that
R3-2 added. The most serious is that this check never runs in CI (R4-1), so the TV-021 entry in
`analysis-records.md` claims a check that doesn't actually run.

### Status of Round 3 items

| # | Item | Status | Evidence |
|---|---|---|---|
| R3-1 | Short buffer reports `expected: 0, found: 0` | Fixed, with caveats | `bits.rs:29-46` and `:111-117` report width 0 as (1, 0) and a short buffer as `buf_len` (needed, have), and keep the largest-offset report otherwise. `counter.rs:62-67` reports bits 0 as (1, 0). Tested at `bits.rs:212-239`, `:271-286`, `counter.rs:168-172`, `exercise.rs:183-189`. Caveats: `Span::new` reports `buf_len` in bits rather than octets (R4-6), and `container.rs:207` still has the same pattern (R4-7). |
| R3-2 | TV-021 validation doesn't check `superseded.by` or catch edits | Partly fixed | On a scratch clone, a dangling `by`, a `by` pointing at a superseded case, an edit to a live released case and an edit to the body of a superseded case are each rejected. But the check silently passes in the shallow, tag-less checkout CI uses (R4-1), and misses deleted or renamed files, edits to file-level fields (R4-2) and a superseded case with no `by` (R4-5). |
| R3-3 | Feature and dependency names mixed | Fixed | `layers.rs:72-78` keeps them in separate sets; the collision case is tested at `layers.rs:221-224`. |
| R3-4 | GEN-004 feature-graph code untested | Fixed | `layers.rs:211-254` covers `dep:`, `x/f`, weak `x?/f`, recursion, the fallback, renames (via `parse_deps`) and the name collision. `x/f` turning on the feature named `x` is not covered (R4-4). |
| R3-5 | FrameSeqNum contradicts RS-010 | Fixed | `rust.md:69` rewords RS-010 for identifiers that use the full width of their type, and `FrameSeqNum::new` is a `const fn` returning `Self`. |
| R3-6 | aldis-ffi row says it mirrors the facade | Fixed | `rust.md:24` now says "enables `aldis/full`". |
| R3-7 | SecurityLayout widened to `usize` | Fixed | The fields are u8/u16 again (`security.rs:16-17`), with a size-4 assertion at `security.rs:234`. |
| R3-8 | Third style of widening helper | Fixed | Shared helpers in `error.rs:138-154` are used by ids, counter and crc. Small leftovers in R4-9 and R4-10. |
| R3-9 | `MAX_TRAILER_LEN` private | Fixed | `security.rs:28-29` makes it public with a doc comment; the CHANGELOG is updated. |

### Findings

R4-1 and R4-2 together make the TV-021 analysis record (method A, CI job `spec`) untrue, and are
worth fixing before merge. R4-4 is a real gap in the GEN-004 check, but nothing triggers it until a
Layer-3 crate takes a third-party dependency (M2). The rest can be follow-up issues.

#### R4-1. TV-021 released-vector check silently skipped in CI

`xtask/src/vectors.rs:125` · correctness

The released-vector check silently does nothing when the checkout has no tags. That is what CI's
default `actions/checkout` gives, so the CI enforcement claimed in `analysis-records.md` never runs.

**Failure scenario:** No job in `.github/workflows/ci.yml` sets `fetch-depth` or `fetch-tags`, so
the `spec` job gets a depth-1 checkout with no tags. `git tag --list v*` returns nothing,
`release_tags()` returns an empty list, and `check_released_tags` never runs. Reproduced: in
`git clone --depth 1 --no-tags`, adding CMN-004 to the requirements of released case
bits-get-spp-version gives `vectors validate: ... 0 problem(s)`, while the same edit in a full clone
is rejected. `analysis-records.md:25` says CI job `spec` performs this comparison. Fix: set
`fetch-depth: 0` and `fetch-tags: true` on that job, and fail rather than skip when no `v*` tag is
found or git fails.

#### R4-2. Deleting or renaming a released vector file passes

`xtask/src/vectors.rs:144` · correctness

The check only looks at vector files that still exist under the same path, and for codec files it
only compares cases. Deleting or renaming a released file, or editing its file-level fields, passes.

**Failure scenario:** Reproduced on a scratch clone with the `v0.1.0-m0` tag: after
`git rm test-vectors/core/oid.json`, validate reports 5 files and 0 problems; after
`git mv oid.json oid2.json`, 0 problems; after setting crc16.json's `standard` to `"bogus"`,
`provenance` to blue-book-example and changing `provenance_note`, 0 problems. All three change
released vectors, which TV-021 forbids. Fix: walk `git ls-tree -r <tag> test-vectors/` instead of
the current files, and compare the whole file with only each case's `superseded` field normalized.

#### R4-3. Unreadable released file silently skips the check

`xtask/src/vectors.rs:162` · correctness

A released file that the current testkit model can't parse is silently skipped. Any future schema
change therefore turns off immutability checking for every released file, with no warning.

**Failure scenario:** `CodecFile` and `CodecCase` use `deny_unknown_fields`. If a later milestone
adds a required field, or renames or removes one, `VectorFile::from_json` fails on the tag's copy,
the `if let Ok(released)` branch is skipped, and released cases can be edited freely while CI stays
green. The same happens when `git show` fails (lines 152-158 just `continue`). Fix: compare raw
`serde_json::Value` trees, or at least report a problem when the released copy can't be parsed.

#### R4-4. GEN-004 misses dependencies turned on through feature `x` from `x/f`

`xtask/src/layers.rs:68` · correctness

`default_enabled_deps` doesn't follow the feature named `x` when a default feature lists `x/f`, but
Cargo does activate that feature. So GEN-004 misses some dependencies that are on by default.

**Failure scenario:** Verified with a scratch crate: given `default = ["x/std"]` and
`x = ["dep:x", "dep:y"]`, `cargo tree` shows both x and y on by default, but
`default_enabled_deps` returns only `{x}`. A third-party optional dependency `y` in aldis-framework
would therefore pass `cargo xtask layers` although it is on by default. Fix: in the `x/f` branch
(when the dependency name has no `?`), also queue `x` if `table.contains_key(x)`, and add a test
case for it.

#### R4-5. Released case can be superseded without a `by`

`xtask/src/vectors.rs:58` · correctness

A case can be marked superseded without a `by`, and validation accepts it, so a released case can be
retired with no replacement and its coverage silently disappears.

**Failure scenario:** Reproduced: adding `"superseded": {"reason": "x"}` to released case
bits-get-spp-version passes validate with 0 problems. The runner, trace and the exerciser's
`build.rs` all skip superseded cases, so that vector stops running everywhere. TV-021 says
corrections SHALL add a new case. Fix: require `by` (or a separate retirement reason) when a case
released in a tag becomes superseded.

#### R4-6. `buf_len` reported in bits in one place, octets in another

`crates/aldis-core/src/bits.rs:44` · correctness

`Span::new` reports the `buf_len` field in bits, while `octet_err` reports the same field in
octets, so one `FieldId` has two units.

**Failure scenario:** `get(&[0], 0, 16)` returns `OutOfRange { field: buf_len, expected: 16,
found: 8 }` although the buffer is 1 octet long, while `read_u32(&[0; 2], 0)` returns `buf_len`
expected 4, found 2, in octets. A caller (or a future vector) that compares `found` to `buf.len()`
gets the wrong answer for the bit-field API. Fix: report octets (`width.div_ceil(8)` plus offset
bytes against `buf_len`), or name the field `buf_bits`.

#### R4-7. `ByteQueue` smaller than 4 octets still reports `expected: 0, found: 0`

`crates/aldis-core/src/container.rs:207` · correctness

`ByteQueue::push` still uses the saturating "largest valid" limit that R3-1 flagged, so a queue
smaller than the 4-octet prefix reports expected 0, found 0.

**Failure scenario:** `ByteQueue::<2>::new().push(&[])` returns `OutOfRange { record_len,
expected: 0, found: 0 }`. Under the inclusive-maximum meaning that looks like a valid call, so the
caller can't tell that the capacity itself is too small. N < 4 is not rejected at compile time
(only `N <= u32::MAX` is asserted). Fix: add a const assert that `N >= PREFIX`, or report the needed
capacity (len + 4) against N when `N < PREFIX`, as `bits.rs` now does.

#### R4-8. One `git show` per vector file per tag

`xtask/src/vectors.rs:151` · efficiency

`check_released_tags` starts one `git show` per vector file per tag on every validate run, so the
cost grows with files × tags.

**Failure scenario:** With 6 files and 1 tag that is 6 extra processes. Once milestones add dozens
of vector files and several `v*` tags, `xtask vectors validate` (run by `xtask ci` and the CI `spec`
job) spends most of its time starting git processes. One `git ls-tree -r --name-only <tag>
test-vectors` plus `git cat-file --batch` per tag reads all released blobs in two processes, and
also makes the missing-file check in R4-2 cheap.

#### R4-9. Const-only cast helper used where `u64::from` works

`crates/aldis-core/src/bits.rs:33` · simplification

`u8_to_u64`, an `as`-cast helper meant for `const` code, is now used in non-const functions that
used the lossless `u64::from` before.

**Failure scenario:** `Span::new` (`bits.rs:33`) and `ModCounter::new` (`counter.rs:66`) are not
`const fn`s, and the helper's own doc says it exists because `From` is not yet const. Swapping
`u64::from(width)` and `u64::from(bits)` for it puts an `as_conversions` cast into code that
didn't need one. Revert those two calls to `u64::from`.

#### R4-10. SecurityLayout still uses its own `as` casts

`crates/aldis-core/src/security.rs:65` · simplification

SecurityLayout's getters and `new` use their own `#[expect]`-annotated `as` casts instead of
shared helpers in `error.rs`, so some of the separate conversion styles R3-8 asked to remove are
still there.

**Failure scenario:** `security.rs:56-57`, `:65` and `:75` each carry their own
`#[expect(clippy::as_conversions)]`, next to the new `pub(crate)` widening helpers in
`error.rs:138-154`. The next u8/u16 to usize widening in Layers 1-2 will copy this pattern again.
Fix: add `u8_to_usize` and `u16_to_usize` (and one checked narrowing helper) next to `as_u64`, and
use them here.

### Recommendation

Fix R4-1 (CI checkout plus fail-instead-of-skip) before merge, since without it the TV-021 analysis
record is untrue. R4-2, R4-3 and R4-5 are worth doing in the same change if time allows. Track
R4-4 and R4-6 to R4-10 as follow-up issues rather than another review round.
