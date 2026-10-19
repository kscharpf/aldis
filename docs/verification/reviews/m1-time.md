# Code review: `m1/time`

## Round 1

- **Date:** 2026-10-18
- **Commit:** `e03ec63` (Merge branch 'm1/epp' into m1/time) on branch `m1/time`, PR #19
- **Baseline:** `main` (with `m1/epp` merged into `m1/time`)
- **Scope:** 37 files (+6,177, −22 lines vs `m1/epp`), implementing Layer 1 CCSDS 301.0-B-4 binary time codes (`aldis-time`: P-field, CUC, CDS, CCS, agency codes), shared conformance test vectors, bare-metal exerciser integration, fuzz targets, coverage gate, and inspections.
- **Reviewed Against:** CCSDS 301.0-B-4 (§3.1–3.4, §3.6, Annex A), [Milestone M1 Plan](../../milestones/m1-packets-time.md) WP-1 (part 1), [Time Codes Spec](../../standards/time-codes.md), [Design Rules](../../02-design-rules.md), and `AGENTS.md`.

---

### Automated Verification Results

All automated gates were executed locally and pass with **zero warnings and zero errors**:

| Check | Command | Result |
|---|---|---|
| **Formatting** | `cargo fmt --all --check` | Pass |
| **Workspace Clippy** | `cargo clippy --workspace --all-features --all-targets` | Pass (0 warnings; `all`, `pedantic`, restriction lints clean) |
| **Workspace Test Suite** | `cargo test --workspace --all-features` | Pass (39 core, 26 epp, 25 time unit tests, 8 epp vectors, 4 time vectors, 7 core vectors, 6 testkit tests) |
| **Bare-metal Clippy** | `cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf` | Pass |
| **Architecture & CI Suite** | `cargo xtask ci` | Pass (15 workspace crates, 0 layer violations; 18 vector files, 286 cases valid; 0 link or spec lint errors) |
| **Coverage Gate (VER-024)** | `cargo xtask coverage` | Pass (**100.0%** across regions 929/929, functions 111/111, lines 881/881, branches 104/104 in `aldis-time`; 100% in `aldis-core` and `aldis-epp`) |
| **Panic Freedom (VER-010)** | `(cd exerciser && cargo build --release --target thumbv7em-none-eabihf)` | Pass (LTO link check succeeds against undefined `aldis_panic_is_unreachable`) |
| **Fuzz Targets (VER-022)** | `cargo clippy --manifest-path fuzz/Cargo.toml --all-targets` | Pass (`time_pfield`, `time_cuc`, `time_cds`, `time_ccs` clean) |
| **M1 Trace Matrix** | `cargo xtask trace --scope m1` | 72/91 requirements traced (TIME-001…TIME-006, TIME-010…TIME-014, TIME-020…TIME-022, TIME-030, TIME-034, etc.) |

---

### Blue Book Conformance Checks

Wire definitions and Annex A tables from CCSDS 301.0-B-4 were verified:

- **P-field (§3.1.1):** Bit 0 extension flag chained parsing correctly terminates or caps at 8 octets (`MAX_PFIELD_LEN`). Reserved time code IDs (`000`, `011`, `111`) are rejected in strict mode and returned as `TimeCode::Unknown` consuming only the P-field in lenient mode (TIME-002, OI-15). Undefined P-field extension octets (CUC octet 3+, CDS/CCS octet 2+) are rejected in strict mode and retained raw in lenient mode (TIME-005).
- **CUC (§3.2):** Coarse time (1–7 octets) and fine time (0–10 octets) mapping to Level 1 / Level 2 epochs is exact. Left-aligned `u128` fine time representation with truncating `to_seconds_nanos` and ceiling `from_seconds_nanos` with carry overflow check accurately preserves precision and round-trips for $\le 3$ fine octets (TIME-011). Non-canonical octet 1/2 splits decode accurately and re-encode canonically while preserving the received P-field in `Decoded::pfield()`.
- **CDS (§3.3, Annex A):** 16/24-bit day segments, 32-bit ms-of-day, and none/$\mu$s/ps sub-millisecond segments strictly adhere to §3.3. Sub-ms code `11` is `ReservedValue` in both modes (TIME-020). Range limits (day $\le 2^{16}-1$ or $2^{24}-1$, ms-of-day $\le 86\,400\,999$, $\mu$s $\le 999$, ps $\le 999\,999\,999$) are enforced on both decode and construction in strict and lenient modes (TIME-021).
- **CCS (§3.4, Annex A):** Month/day and day-of-year variations validate every BCD nibble and calendar boundary, including Gregorian leap year rules (divisible by 4, except centuries not divisible by 400). Resolution code `111` is `ReservedValue` in both modes (TIME-030). Non-zero DOY high nibble is strict `ReservedValue` and lenient flag `TimeAnomaly::NonZeroDoyHighNibble` (TIME-034); encoder always writes zero.
- **Agency-Defined Codes (§3.6):** ID `110` with length bits 4–7 mapping to 1–16 T-field octets is strictly implemented (TIME-006).

---

### Key Architectural Strengths

1. **Zero Allocation, Deterministic Sans-I/O Arithmetic:**
   - The conversion algorithms between whole seconds/nanoseconds and fractional `u128` CUC time use fixed-point arithmetic without float or heap allocations. The nested floor division in `to_seconds_nanos` and ceiling math in `from_seconds_nanos` guarantee bounded truncation and exact round-tripping for $\le 3$ fine octets.
2. **Robust Conformance Vectors & Property-Based Testing:**
   - 111 test cases across 4 shared JSON vector files (`pfield.json`, `cuc.json`, `cds.json`, `ccs.json`) generated from an independent reference model of 301.0.
   - Property tests verify arbitrary byte inputs under strict and lenient decodes (`strict ⊆ lenient`, idempotent re-encodes, and round-trips).
   - Saved proptest regression case for CUC P-field extension chains (`proptest-regressions/code.txt`).
3. **Flawless Gate Readiness:**
   - 100.0% coverage across all metrics (regions, functions, lines, branches) with zero waivers.
   - Panic-free bare-metal link verified with LTO on Cortex-M (`thumbv7em-none-eabihf`).
   - Mutation testing (365 mutants) triaged and documented, with the 14 surviving mutants confirmed to be equivalent operations on disjoint bit fields.

---

### Detailed Findings

#### 1. Error Detail: Lower-bound violations report upper limit instead of minimum bound (GEN-041)
- **Files:**
  - `crates/aldis-time/src/cuc.rs:53-58` (`CucFormat::new` for `coarse_octets == 0`)
  - `crates/aldis-time/src/code.rs:32-38` (`AgencyFormat::new` for `t_len == 0`)
  - `crates/aldis-time/src/ccs.rs:178-196` (`Ccs::new` / `validate` for `year == 0`, `month == 0`, `day == 0`, `doy == 0`)
- **Issue:**
  When 1-based fields (requiring a value $\ge 1$) receive `0`, `out_of_range` is invoked reporting the maximum upper limit as `expected` with `found: 0`. For example, `CucFormat::new(epoch, 0, 0)` returns `Error::out_of_range("coarse_octets", 7, 0)`.
- **Impact:**
  Contradicts GEN-041 and established conventions in `aldis-core` (e.g. `bits.rs:30-33`, `counter.rs:63-66`, and PR #18 review finding 4 for `EncapsulationHeader::idle(0)`). Lower-bound violations should report the violated minimum bound (`expected: 1, found: 0`).
- **Remedy:**
  Separate lower-bound checks so that `0` returns `Error::out_of_range(field, 1, 0)`.

#### 2. Error Detail: `Agency::new` omits `ErrorDetail` on `LengthMismatch` (GEN-041)
- **File:** `crates/aldis-time/src/code.rs:94-98`
- **Issue:**
  In `Agency::new`:
  ```rust
  if t.len() != format.t_len() {
      return Err(Error::new(ErrorKind::LengthMismatch).with_field(FieldId::new("tfield")));
  }
  ```
  While the error names the field (`"tfield"`), it omits `ErrorDetail::Expected { expected: format.t_len() as u64, found: t.len() as u64 }`.
- **Impact:**
  Omits actionable diagnostic information for callers debugging length mismatches without allocation.
- **Remedy:**
  Attach `.with_detail(ErrorDetail::Expected { expected: format.t_len() as u64, found: t.len() as u64 })`.

#### 3. Traceability & Item Documentation (RS-015)
- **Files:** `crates/aldis-time/src/{pfield, cuc, cds, ccs, code}.rs`
- **Issue:**
  Per RS-015, every public item's documentation must cite the relevant CCSDS clause and requirement IDs. Several public accessors and constructors lack explicit citations:
  - `PField::first`, `PField::rest`, `PField::id`, `PField::len` (301.0 §3.1.1, TIME-001, TIME-005)
  - `TimeCodeId::from_octet`, `TimeCodeId::bits` (301.0 §3.1.1, TIME-001)
  - `PFieldTail::new`, `PFieldTail::as_bytes`, `PFieldTail::len`, `PFieldTail::is_empty` (TIME-005)
  - `CucFormat::epoch`, `CucFormat::coarse_octets`, `CucFormat::fine_octets`, `CucFormat::has_octet2`, `CucFormat::t_len` (301.0 §3.2, TIME-010)
  - `CucValue::new`, `CucValue::coarse`, `CucValue::fine` (TIME-011)
  - `CdsFormat::epoch`, `CdsFormat::day_len`, `CdsFormat::subms`, `CdsFormat::t_len` (301.0 §3.3.2, TIME-020)
  - `CcsFormat::variation`, `CcsFormat::subsecond_octets`, `CcsFormat::t_len` (301.0 §3.4.2, TIME-030)
  - `AgencyFormat::t_len` (301.0 §3.6.2, TIME-006)
  - `TimeFormat::t_len`, `TimeFormat::pfield` (TIME-001, TIME-003)
  - `TimeCode::format`, `TimeCode::pfield` (TIME-001, TIME-002)
  - `Decoded::code`, `Decoded::pfield`, `Decoded::is_empty` (TIME-001, TIME-003)
- **Remedy:**
  Add CCSDS clause and requirement citations to these getters and accessors.

#### 4. Documentation & Status Updates: Milestone M1 Plan
- **File:** `docs/milestones/m1-packets-time.md:6`
- **Issue:**
  Line 6 states: `WP-0 is implemented on m1/core (PR #17, in review).`
  PR #17 has merged into `main` at commit `b817d0f`.
- **Remedy:**
  Update the status text to record that PR #17 is merged.

#### 5. Verification Records & Process Sign-Offs
- **Files:** `docs/verification/inspections/gen-022-023-aldis-time.md`, `docs/verification/analysis-records.md:31`
- **Issue:**
  The `aldis-time` GEN-022/GEN-023 inspection is currently recorded as `awaiting maintainer approval`.
- **Remedy:**
  Record maintainer review and approval once this review round is approved.

---

### Recommendation

The `aldis-time` binary time code implementation is in outstanding condition. The automated test and verification suites are comprehensive and pass with 100% coverage and zero warnings. Addressing the diagnostic bounds reporting (GEN-041) and documentation citations (RS-015) will make WP-1 part 1 fully polished and ready for part 2 (ASCII time codes and conversions).
