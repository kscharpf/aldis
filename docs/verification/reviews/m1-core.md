# Code review: `m1/core`

> Dated review: identifiers reflect the code at the time.

## Round 1

- **Date:** 2026-10-17
- **Scope:** commits `3b51e87` (M1 WP-0: DecodeMode and AnomalySet in aldis-core) and `529499c`
  (Record maintainer approval of the revised GEN-022/023 inspection), 20 files, diffed against
  `main`. Reviewed against [M1](../../milestones/m1-packets-time.md) WP-0 and the requirements it
  names (API-030…033, GEN-022, GEN-043, GEN-081), plus the readiness of the anomaly tables for
  WP-1…3.
- **Checks at `529499c`, all passing with zero warnings:**
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-features --all-targets`
  - `cargo test --workspace --all-features`
  - `cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf`
  - `cargo xtask ci` (layers 0 violations; 6 vector files, 83 cases; trace `m0` 60/60)
  - `cargo xtask coverage`: `aldis-core` regions 0/977, functions 0/189, lines 0/846, branches 0/78
    missed
  - exerciser release build for `thumbv7em-none-eabihf` (panic-freedom link check)
  - `cargo clippy --manifest-path fuzz/Cargo.toml --all-targets`
  - `cargo mutants` on `anomaly.rs` and `mode.rs`: 41 mutants, 19 caught, 20 unviable, 2 timeouts,
    0 missed. This matches `mutants-triage.md`.
  - `cargo xtask trace --scope m1`: 27/87 traced (API-030, API-033, and GEN-043 are new)
- **Blue Book citations:** each Source cell in the three new anomaly tables was checked against the
  PDFs: 301.0 §3.1.1, §3.3.2, §3.4.1.2; 133.0 §4.1.3.3.3.4; 133.1 §4.1.2.6.3, §4.1.2.7.2,
  §4.1.3.1.4–5. All are correct.

The WP-0 "done when" criteria are met: 100 % coverage, no surviving mutants, and the three skeleton
enums compile against the trait. The code is small and clean. The most serious problem is a loop
that GEN-022 does not allow, recorded as compliant in an inspection that was approved during this
review (1). Findings 2 and 3 are spec contradictions that the new bit tables freeze. They should
become open issues before WP-1 and WP-3 implement them.

### Findings

#### 1. `FromIterator` loop is not bounded as GEN-022 requires

`crates/aldis-core/src/anomaly.rs:105` · correctness (GEN-022)

GEN-022 allows a loop bounded by "the length of an input buffer, a compile-time capacity, or a
constant". `from_iter` loops over any caller-supplied `IntoIterator`, which can be infinite. The
inspection records its bound as "input length (the caller's iterator)", and the approved GEN-022
row in `analysis-records.md` (`529499c`) says "every loop is bounded by an input length, a
const-generic capacity, or a constant". That statement is now false. No other loop in
`aldis-core` library code consumes a caller iterator.

**Failure scenario:** `core::iter::repeat(SppAnomaly::IdleWithSecondaryHeader).collect::<AnomalySet<_>>()`
never returns. The set is full after the first item, but `from_iter` cannot know that. The trace
matrix still reports GEN-022 as traced by an approved inspection.

**Fix:** `FromIterator` is not in the WP-0 API list. Only tests, the exerciser, and the fuzz target
use it, through `collect()`. Replace it with a slice constructor such as
`AnomalySet::from_flags(&[A])`, whose loop is bounded by the input buffer. Update the inspection
row, then re-approve. Do not add `Extend` either: it has the same problem.

#### 2. Two time flags cannot be decoded leniently, and a third has no requirement

`docs/standards/time-codes.md:72-74` · spec

- **`ReservedSubmsCode` (bit 1)** cites TIME-020, but TIME-020 says sub-ms code `11` "SHALL be
  `ReservedValue`" in every mode. The table makes it a lenient anomaly, so the spec contradicts
  itself. Lenient acceptance also cannot be implemented as specified: the code sets the length of
  the sub-millisecond segment, and 301.0 §3.3.2 only says "reserved for future use". A lenient
  decoder therefore cannot know the T-field length, and cannot report the consumed length that
  TIME-003 requires.
- **`ReservedTimeCodeId` (bit 0)** has the same length problem. TIME-002 says reserved IDs are
  "decoded as `Agency/Unknown` raw" in lenient mode. With an explicit P-field, nothing says how many
  T-field octets the raw value has, so `decode_with_pfield` has no defined consumed length.
- **`NonZeroDoyHighNibble` (bit 2)** cites TIME-030, but TIME-030 does not mention the nibble.
  301.0 §3.4.1.2 is descriptive ("are not used and are set to zero"), not a SHALL, so API-031 does
  not make it a strict error either. No requirement says what strict mode does with it.

**Failure scenario:** In WP-1, the CDS decoder must either reject code `11` in lenient mode, which
breaks the flag table, or accept it with a guessed sub-ms length, which has no Blue Book basis and
cannot be pinned by a vector. Both readings are defensible, and AGENTS.md forbids choosing one
silently in code.

**Fix:** Open an OI before WP-1 module 1 (`pfield`) starts. Likely resolution: code `11` is an error
in both modes, and bit 1 stays reserved and unused. Lenient decoding of a reserved ID is defined
only for `decode_implicit`, or as "P-field only, T-field = rest of input". Add a TIME requirement
for the DOY nibble (strict `ReservedValue`, lenient flag). A bit never used costs nothing, and
renumbering is not allowed later, so decide now.

#### 3. API-032 limits leniency to fields that two flags do not cover

`docs/03-api-model.md:74` · spec

API-032 says lenient mode accepts deviations "limited to **reserved/spare bits, fields CCSDS
declares 'undefined', and conventional-zero fields**". Two flags tolerate violations of a SHALL on
a defined field:

- `IdleWithSecondaryHeader`: 133.0 §4.1.3.3.3.4, "The Secondary Header Flag shall be set to '0' for
  Idle Packets".
- `EmptyNonIdle`: 133.1 §4.1.3.1.5, "If the Encapsulated Data field is absent, then the
  Encapsulation Protocol ID field shall contain the value '000'". This also borders on a length
  check, which API-032 keeps an error in both modes.

SPP-004 and EPP-005 only say that strict mode rejects these cases. Read together with the flag
tables they imply lenient acceptance, which API-032's "limited to" rules out. `mode.rs` copies
API-032's wording, so the rustdoc contradicts the flags too.

**Failure scenario:** A WP-4 vector author who follows API-032 writes a `lenient` case that
expects `ReservedValue` for an idle packet with the secondary header flag set. A WP-2
implementer who follows the SPP table returns `Ok` with bit 0 set. One of them is wrong, and no
document says which.

**Fix:** Either widen API-032 so it names these two cases (or "fixed-value fields of idle data
units"), or drop the flags and make SPP-004 and EPP-005 apply in both modes. Record the choice in
an OI or in the API-032 row.

#### 4. The `Anomaly` contract is not documented, and violating it fails silently

`crates/aldis-core/src/anomaly.rs:15` · correctness

The trait doc says only that `bit` is in 0…31 and stable. `AnomalySet` also relies on two
unstated invariants: `bit` is injective, and `from_bit(f.bit()) == Some(f)`. A violation is not
caught. Either the anomaly is silently dropped (index ≥ 32), or the set disagrees with itself:
`len`, `is_empty`, `bits`, and `==` count raw bits, while `iter` and `Debug` show only bits that
`from_bit` maps back. The test `iterator_skips_unknown_bits` asserts this today: `len() == 4`
while `iter()` yields 2 flags, and a set with `is_empty() == false` prints as `{}`.

**Failure scenario:** WP-3 adds `EppAnomaly::Foo` with `bit() => 3` but forgets the `from_bit`
arm. A lenient decode that raises it returns `anomalies().is_empty() == false`, yet iterating
yields nothing and the log shows `{}`. The C ABI sees bit 3 set, but the Rust API cannot name it.
The hand-written `stable_bit_indices` test catches this only if the new variant is also added to
its `all` array.

**Fix:** State all three invariants in the trait doc. Add one shared conformance check (for
example `aldis_testkit::check_anomaly::<A>()`, which loops over 0…=255 and checks that `from_bit`
and `bit` are inverse and every index is below 32). Call it from each codec crate instead of the
three hand-written tests. Say in the `len` doc that it counts raw bits.

#### 5. API-033 is traced by tests that do not check it

`crates/aldis-spp/src/anomaly.rs:39` · traceability (VER-020)

API-033 (method I) requires each standard document to list the flags its decoder can raise. The
m1 trace matrix marks it traced by the three `stable_bit_indices` tests. Those tests check the Rust
enums and never read the documents. Nothing checks that a document table matches its enum, yet
the tables are declared "shared with the C ABI".

**Failure scenario:** A later change adds `SppAnomaly::Foo => 1` and its test, but not the row in
`space-packet.md` §3. The trace still shows API-033 as traced, and the C header (M5), if it is
written from the document, misses the flag.

**Fix:** Remove `API-033` from the test `req:` tags. Add an `analysis-records.md` row for the three
tables (I: reviewed in each PR that touches an `anomaly.rs`). Optionally, an `xtask` check can parse
the tables and compare them to `bit()`.

#### 6. Records not updated

`docs/verification/analysis-records.md:31` · docs

- The VER-022 row lists six fuzz targets and leaves out `anomaly_set_model`, although the nightly
  matrix runs it and its file carries `req: VER-022`.
- The M1 status line (`m1-packets-time.md:6`) still says "the 24 traced so far" (it is now 27) and
  "WP-0 under way", although WP-0's done-when criteria are met.

#### 7. Two mutants hang instead of failing

`crates/aldis-core/src/anomaly.rs:142` · test-coverage (VER-025)

The two `Iter::next` mutants are "caught" only by timing out. The triage is correct. However, the
nightly `mutants` job tolerates exit code 3 (timeouts) only "in M0" (`nightly.yml:135`), and these
two now time out on every run. If M1 or the release tightens that policy, the job fails on
mutants that are fine. Each one also costs 20 s per night.

**Fix (optional):** Bound the loop structurally, for example with
`for _ in 0..AnomalySet::<A>::CAPACITY { if self.bits == 0 { break; } … }`. The GEN-022 bound
then holds by construction rather than by two operators, and the mutants fail quickly instead of
hanging. Revisit the "in M0" comment either way.

### Minor

- **`len` returns `u32`.** Rust collections return `usize` from `len`; `u32` makes
  `set.len() == v.len()` comparisons need a conversion. `bits()` already exposes the raw width.
- **`Iter` has no `size_hint` and does not implement `FusedIterator`.** It is fused in practice,
  since `bits` stays 0, and `self.bits.count_ones()` is a valid upper bound.
- **`CAPACITY` needs a turbofish.** It sits on `impl<A> AnomalySet<A>`, so callers write
  `AnomalySet::<Flag>::CAPACITY`, while the trait doc refers to it as `AnomalySet::CAPACITY`. A
  module constant `anomaly::CAPACITY` would read better.
- **The SPP test is thinner than the other two.** It does not check that `iter()` returns the flag
  (`crates/aldis-spp/src/anomaly.rs:39`). Finding 4's shared check would remove this difference.
- **The exerciser only uses a test double.** `apis_mode_anomaly` uses a local `Flag`, so the three
  real enums are not linked. This is fine for WP-0 under D7. WP-5's per-crate groups should
  exercise `TimeAnomaly`, `SppAnomaly`, and `EppAnomaly` through a lenient decode.

### Milestone status

WP-0 is complete except for findings 1 and 4–6. WP-1 to WP-6 have not started: `aldis-time`,
`aldis-spp`, and `aldis-epp` are still skeletons, there are no vectors under
`test-vectors/{time,spp,epp}/`, there is no `crosscheck/` workspace, and there is no `--scope m1`
in CI. `xtask trace --scope m1` reports 27 of 87 requirements traced.

### Recommendation

Fix 1 before merging. The approved GEN-022 record is untrue until it is fixed, and the fix is
small. Do 4 to 6 in the same change. Open OIs for 2 and 3 now, before WP-1 `pfield`/`cds` and
WP-2/WP-3 `anomaly` code depends on either reading. 7 and the minor items can wait for WP-5.
