# Hazard and failure-mode analysis

How a failure of Aldis could harm a mission, what controls each failure mode, and what is left. Requirement: VER-065 ([06 §11](../06-verification.md#11-hazard-analysis-and-assurance-case)). The [assurance case](assurance-case.md) uses each hazard's controls as evidence for its claims (VER-066), and `cargo xtask assurance` checks every reference in both ([assurance-evidence.md](assurance-evidence.md)).

This file holds the method, the index, and the failure modes common to all codecs. Each frame codec has its own file, so that they can be written and reviewed in parallel:

| Codec | File | Standard document |
|---|---|---|
| TM | [hazards/tm.md](hazards/tm.md) | [tm-frame.md](../standards/tm-frame.md) |
| TC and CLCW | [hazards/tc.md](hazards/tc.md) | [tc-frame.md](../standards/tc-frame.md) |
| AOS | [hazards/aos.md](hazards/aos.md) | [aos-frame.md](../standards/aos-frame.md) |
| USLP | [hazards/uslp.md](hazards/uslp.md) | [uslp-frame.md](../standards/uslp-frame.md) |

## 1. Method

### 1.1 System boundary

Aldis is a library. It turns octets into typed views and typed values into octets, and it returns. It does not decide what a mission does with a frame, and it cannot know what a wrong frame costs. The analysis therefore stops at the library's contract: what the library returns, writes, or fails to return. **The mission's own hazard analysis owns the mission effects**, and takes this file as an input. The column "Mission effect" names the kind of harm a wrong result can lead to, so that the mission analysis can find it; it does not rate it.

Inside the boundary: every public function of `aldis-core`, `aldis-time`, `aldis-spp`, `aldis-epp`, `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`, and the C ABI of `aldis-ffi`. Outside it: the synchronization and channel coding sublayer below the frames (roadmap §7, OI-12), the engines of M3 and M4, the caller's code, and the platform. The assumptions the analysis makes of what is outside are the `ASM-n` rows of the [assurance case](assurance-case.md#assumptions-and-context).

### 1.2 Failure-mode categories

Every hazard has one category. Each codec file covers every category, or says why one does not apply (§2).

| Slug | Failure mode |
|---|---|
| `corrupt-accepted` | A frame with errors is accepted as a valid frame |
| `valid-rejected` | A valid frame is rejected |
| `misrouting` | A frame is delivered as belonging to another spacecraft, virtual channel, or MAP |
| `truncation` | A frame or a field is silently shortened or extended |
| `wrong-value` | A field value passed up differs from the one on the wire |
| `nonconformant-output` | An encoder or writer produces octets that do not conform to the Blue Book |
| `unbounded-time` | A call does not return, or takes time not bounded by its input and capacities |
| `memory` | A call overruns the stack, or reads or writes outside its buffers |
| `panic` | A call panics, aborts, or traps |
| `state-corruption` | State kept between calls (configurations, writer buffers, helper storage) becomes invalid |
| `abi-misuse` | A C caller's misuse of the C ABI leads to anything but an error |
| `hidden-deviation` | A deviation from the Blue Book is accepted without being reported (lenient mode, or a listed exception of API-031) |

### 1.3 Severity

A project choice, relative to the library's contract, not to a mission. Lower numbers are worse.

| Severity | Meaning | Why ranked here |
|---|---|---|
| S1 | **Undetected wrong result.** The library returns success with content that differs from what was sent or asked for (corrupt data, wrong channel, truncation, a non-conformant frame), or reads or writes memory it was not given | The caller has no way to know, and acts on wrong data |
| S2 | **Loss of control.** The call never returns, overruns its stack, or panics | The platform can detect it (a watchdog, a trap), the caller cannot; the task or the whole software stops |
| S3 | **Availability.** A valid input is rejected with an error | The data is lost, but the loss is reported |
| S4 | **Diagnostics.** The outcome (accepted or rejected) is right, but the error kind, field code, detail, or anomaly flags are wrong or missing | Only reporting and statistics suffer |

### 1.4 Controls and evidence

A **control** is a requirement whose implementation prevents the failure mode or turns it into a reported error. A control counts only with **evidence**: a test, a vector case, a fuzz target, a verification record, or a check of `cargo xtask verify` or `cargo xtask ci` that shows the requirement holds. Requirements are named by ID in the `Requirements` row; evidence is named in the `Evidence` row by the references of [assurance-evidence.md](assurance-evidence.md) §2, which `cargo xtask assurance` resolves. Evidence that runs only at night says so by its kind (`nightly-test:`, `nightly-check:`, `job:`, `fuzz:`, `proof:`).

### 1.5 Residual risk

What the controls leave, stated as a condition under which the failure mode still occurs, not as a probability: for example "with no FECF configured, an error in the VCID that names another configured VC is not detected". Where the residual is the integrator's to close, the `Assumptions` row names the `ASM-n` obligation of the assurance case. A residual that the Blue Book itself leaves (a code's distance, an optional FECF) is stated as such: Aldis cannot remove it.

### 1.6 Gaps and proposals

A **gap** is a failure mode without an adequate control: no requirement covers it, or a requirement does but no evidence shows it, or the residual is larger than the Blue Book requires. Every gap is written as a **proposal** in the `## Proposals` table of the file that found it, and the hazard's status is `gap` until the proposal is decided. A proposal is a draft requirement, a draft OI, a test, a documentation change, or a tooling change. **The analysis never adds a requirement or an OI itself**: the coordinator takes each proposal to whoever the [decision rights](../process/decision-rights.md) give it to (a reading the Blue Book does not settle, or a new requirement, is the maintainer's), and the accepted proposal is then written where it belongs, in its own pull request.

### 1.7 Format

Each hazard is a section `### HZ-<codec>-NN <title>`, where `<codec>` is `X` in this file and `TM`, `TC`, `AOS`, or `USLP` in the codec files, and `NN` is two digits, numbered in order and never reused. The section holds one table with exactly these rows, in this order (the format `cargo xtask assurance` checks, [assurance-evidence.md](assurance-evidence.md) §3):

| Row | Content |
|---|---|
| Category | One slug of §1.2 |
| Status | `todo` (not yet analysed), `controlled` (adequate controls, no residual beyond the Blue Book's), `residual` (controlled, with a residual stated), or `gap` (at least one proposal) |
| Severity | S1–S4 of §1.3, the severity of the failure mode if it occurred uncontrolled |
| Failure mode | What goes wrong, at the library's interface |
| Cause | What makes it happen: a channel error, a caller's choice, a defect |
| Mission effect | The kind of harm, for the mission's analysis (§1.1) |
| Requirements | The requirement IDs of the controls |
| Evidence | References (§1.4) |
| Residual risk | §1.5, or `—` |
| Assumptions | `asm:` references to the assurance case, or `—` |
| Proposals | `proposal:` references, or `—` |
| Claims | `ac:` references to the claims of the assurance case this hazard's controls support |

Each codec file ends with a `## Coverage` table (one row per category of §1.2) and a `## Proposals` table (§2).

## 2. Codec files

Each codec file has:
- a `## Coverage` table, `| Category | Hazards | State |`, with one row per slug of §1.2 in the order of §1.2. `Hazards` lists the hazard IDs of that category, or `—`; `State` is `done`, `TODO`, or `n/a: <reason>` (for example, a codec with no stateful part);
- one section per hazard, in the format of §1.7;
- a `## Proposals` table, `| ID | Kind | Proposal | Status |`, with IDs `P-<codec>-NN`. `Kind` is `requirement`, `oi`, `test`, `doc`, or `tooling`; `Status` begins with `proposed`, `accepted`, or `rejected` in a code span, and after `accepted` says where the proposal was written (a requirement ID, an OI, a pull request), after `rejected` why. The table may have no rows.

## 3. Failure modes common to all codecs

### HZ-X-01 A single-event upset changes state held between calls

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A configuration, a channel table, or C ABI helper storage changes after it was constructed and checked. A restricted value then holds a value its type excludes, or an enum a discriminant that names no variant |
| Cause | A single-event upset in memory the platform does not protect |
| Mission effect | Any: a corrupted enum is undefined behaviour, so a `match` on it may take any path ([hardware assumptions](../hardware-assumptions.md) §2 item 1) |
| Requirements | GEN-015, RS-012, RS-020, GEN-020 |
| Evidence | `record:GEN-015`; `file:docs/hardware-assumptions.md` |
| Residual risk | Aldis does not re-check a value after construction (RS-020), and no software check can catch an invalid discriminant. Bounds checks still hold: a corrupted length or offset gives a wrong result or an error, not an access outside a buffer (GEN-020; [hardware assumptions](../hardware-assumptions.md) §2 item 3). The codecs keep no state between calls; configurations do, for the whole mission |
| Assumptions | `asm:ASM-1` |
| Proposals | — |
| Claims | `ac:AC-4.4` |

### HZ-X-02 A call panics

| | |
|---|---|
| Category | `panic` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A public function panics on some input, buffer content, or call sequence |
| Cause | An unchecked index, overflow, division, `unwrap`, or a panic of `core` reached through the library |
| Mission effect | The calling task stops. Through the C ABI's shipped static library, whose panic handler executes a trapping instruction (`crates/aldis-ffi/src/lib.rs:72`–`86`, `:108`; OI-49), the target's fault handler, a debugger, or a watchdog sees the failure where it happened; on a target without the instruction (SPARC V8) it still spins (`lib.rs:138`) |
| Requirements | GEN-020, GEN-021, VER-010, VER-001, GEN-070, VER-022 |
| Evidence | `record:VER-010`; `check:exerciser-thumbv7em-none-eabihf`; `check:exerciser-armv7r-none-eabihf`; `check:exerciser-riscv32imac-unknown-none-elf`; `nightly-check:rust-sparc-leon3`; `record:VER-001`; `check:clippy`; `record:GEN-070`; `record:VER-022`; `fuzz:tm_decode`; `fuzz:tc_decode`; `fuzz:aos_decode`; `fuzz:uslp_decode`; `fuzz:ffi_decode`; `job:fuzz`; `test:crates/aldis-tc/tests/properties.rs::decoder_never_panics` |
| Residual risk | The link check proves no panic path in the exerciser's release build with LTO, for the generic instantiations the exerciser makes (`record:VER-010`). It does not cover the shipped `aldis-ffi` static library, which is built without LTO (`gh:#114`), nor instantiations with other capacities that an integrator makes. `cargo xtask stack` already walks the shipped library's call graph and reports every panic path it reaches (`record:VER-011`: on 2026-10-25 one panic path, in the exerciser build only), but a reached panic path does not fail it. The panic handler of the shipped library traps (`udf` on Arm, `brk` on AArch64, `ebreak` on RISC-V, `ud2` on x86; OI-49) instead of spinning, on every gating bare-metal target, so a panic that GEN-020 failed to exclude is a fault, not a hang; the fallback on a target without a stable trapping instruction here, SPARC V8, still spins. The handler is not a control against the panic, which is the link check's; a check of the shipped library's panic paths is tracked with #114 (P-X-01) |
| Assumptions | — |
| Proposals | `proposal:P-X-01`; `proposal:P-X-02` |
| Claims | `ac:AC-2.1`; `ac:AC-2.2`; `ac:AC-2.3` |

### HZ-X-03 A call does not terminate, or blocks

| | |
|---|---|
| Category | `unbounded-time` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A call loops without bound, recurses, or waits |
| Cause | A loop bounded by a value that can be corrupted or unbounded, recursion, a blocking construct, or I/O |
| Mission effect | The calling task hangs until a watchdog acts |
| Requirements | GEN-022, GEN-023, GEN-031, GEN-032, VER-061 |
| Evidence | `record:GEN-022`; `ci:loops`; `check:spec`; `check:stack`; `file:docs/verification/inspections/gen-022-023-aldis-tm.md`; `file:docs/verification/inspections/gen-022-023-aldis-tc.md`; `file:docs/verification/inspections/gen-022-023-aldis-aos.md`; `file:docs/verification/inspections/gen-022-023-aldis-uslp.md`; `file:docs/verification/inspections/gen-022-023-aldis-ffi.md`; `proof:crates/aldis-core/src/proofs.rs::error_kind_lookup_is_bounded`; `record:CABI-004`; `record:GEN-031` |
| Residual risk | The bound of each loop rests on the inspections, whose revisions for `aldis-tm`, `aldis-tc`, `aldis-uslp`, and `aldis-ffi` are pending the exit review (VER-061, `df:DF-4`); only one bound is proven (`proof:` above, nightly). GEN-031 and GEN-032 (no clocks, no I/O) are traced by `record:GEN-031`; no check searches for a new global or a platform call in library code, so a later change relies on review and on the `no-std-<target>` builds |
| Assumptions | — |
| Proposals | `proposal:P-X-03` |
| Claims | `ac:AC-3.1`; `ac:AC-3.2`; `ac:AC-3.3` |

### HZ-X-04 A call overruns its stack

| | |
|---|---|
| Category | `memory` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A call needs more stack than its task has |
| Cause | A large frame (the channel tables built on the stack, OI-45), deep calls, or a caller that gives too small a stack |
| Mission effect | On a target without a stack guard the overrun silently overwrites other memory (S1); with a guard it traps (S2) |
| Requirements | VER-011, GEN-022, GEN-012 |
| Evidence | `record:VER-011`; `check:stack`; `file:xtask/stack-budgets.toml`; `job:stack-sizes`; `oi:OI-45` |
| Residual risk | The bound is per entry point and leaves out interrupts and the caller's own frames (`record:VER-011`). The budgets are 4 096 bytes (Rust) and 10 240 bytes (C ABI) on every target; the largest C ABI function needs about 8.6 KB, mostly a channel table rebuilt on each call (OI-45, open). Whether a task can afford it is the integrator's |
| Assumptions | `asm:ASM-3` |
| Proposals | — |
| Claims | `ac:AC-4.2` |

### HZ-X-05 A C caller misuses the C ABI

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A C caller passes a null pointer, an inconsistent length, a struct of the wrong size, misaligned or uninitialised helper storage, or a stale handle, and the library does anything but return an error |
| Cause | A defect in the caller |
| Mission effect | Undefined behaviour in the caller's process |
| Requirements | CABI-022, CABI-021, CABI-031, CABI-032, CABI-033, GEN-071 |
| Evidence | `test:crates/aldis-ffi/tests/tc.rs::writer_arguments_are_checked`; `test:crates/aldis-ffi/tests/tm.rs::vc_arrays_step_by_their_struct_size`; `test:crates/aldis-ffi/tests/uslp.rs::content_and_fields_are_checked_as_arguments`; `test:crates/aldis-ffi/tests/tc.rs::the_delimiter_is_stateless`; `fuzz:ffi_decode`; `record:CABI-010`; `record:CABI-030` |
| Residual risk | C cannot tell a valid pointer from a wrong non-null one, or a length from the true size of the buffer behind it: CABI-022 checks null pointers and the consistency of lengths, and a wrong non-null pointer or an overstated length stays undefined behaviour the library cannot detect. No check runs the C ABI under Miri (GEN-071; `gh:#71`, `df:DF-7`), and the C harness with sanitizers is M5's (VER-026) |
| Assumptions | `asm:ASM-6` |
| Proposals | — |
| Claims | `ac:AC-6.2` |

### HZ-X-06 The CRC-16 misses an error

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A frame with bit errors passes the FECF check and is accepted |
| Cause | An error pattern the CRC-16 does not detect |
| Mission effect | Corrupt data or a wrong header passed up as valid |
| Requirements | CMN-001, CMN-003, TM-005, TC-006, AOS-014, USLP-014, VER-064 |
| Evidence | `file:docs/verification/derivations/crc16-bit-flips.md`; `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `nightly-test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_longest_frame_is_rejected`; `test:crates/aldis-aos/tests/bit_flips.rs::a_double_bit_error_32767_bits_apart_passes_the_fecf`; `test:crates/aldis-uslp/tests/bit_flips.rs::a_double_bit_error_32767_bits_apart_passes_the_fecf`; `job:bit-flips` |
| Residual risk | Every single-bit error is detected, and every double-bit error in a frame of at most 4 095 octets (VER-064). In AOS and USLP frames longer than that, two errors 32 767 bits apart pass (the two tests above show one); a TM channel is refused above 4 095 octets (OI-51, `hz:HZ-TM-04`). Patterns of three or more bit errors are outside VER-064. An error in the TC or USLP Frame Length field changes which octets the CRC covers, so the guarantee does not apply to it (`hz:HZ-TC-01`, `hz:HZ-USLP-01`). A truncated USLP frame has no FECF at all (`hz:HZ-USLP-04`) |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-5.5` |

### HZ-X-07 The AOS FHEC miscorrects a header

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | An AOS header with three or more symbol errors is "corrected" to a different valid header |
| Cause | An error pattern within two symbols of another codeword (the code's minimum distance is 5, AOS-014) |
| Mission effect | A frame passed up with a wrong SCID, VCID, or signalling field, so possibly on the wrong channel |
| Requirements | AOS-013, AOS-014 |
| Evidence | `vector:aos-fhec-mis-001`; `vector:aos-fhec-un-001`; `vector:aos-fhecf-dec-002-s`; `fuzz:aos_fhec_model`; `file:docs/verification/derivations/aos-fhec.md` |
| Residual risk | Inherent in the code of 732.0-B-5 §4.1.2.6.5; no decoder can prevent it. With an FECF the frame then fails `CrcMismatch` unless the CRC also misses the difference (AOS-014); with no FECF it is accepted. The detailed rows are in [hazards/aos.md](hazards/aos.md) |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-5.5` |

### HZ-X-08 Lenient mode or a listed exception hides a deviation

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | A frame that deviates from the Blue Book is accepted, and the caller does not learn of the deviation |
| Cause | The caller decodes in lenient mode and ignores the anomaly set (API-032, GEN-043); or the deviation is one of API-031's listed exceptions, accepted in strict mode too: a CLCW whose COP in Effect is not `01` (TC-030), and OID data that is not the pseudo-noise sequence (CMN-021) |
| Mission effect | A frame with set spare bits, or a CLCW of another COP, treated as ordinary. In M2 no FOP-1 exists yet to reject the CLCW (TC-030 relies on it, M3) |
| Requirements | API-030, API-031, API-032, API-033, GEN-043, TC-030 |
| Evidence | `vector:tc-val-007`; `vector:tc-dec-err-008`; `vector:tc-clcw-dec-004`; `vector:aos-iz-dec-003`; `test:crates/aldis-tc/tests/properties.rs::decoder_never_panics`; `record:VER-022` |
| Residual risk | Lenient mode accepts only the fields API-032 lists, and reports each as an anomaly; the strict mode is the default (API-030). Whether anomalies are acted on is the caller's |
| Assumptions | `asm:ASM-5` |
| Proposals | — |
| Claims | `ac:AC-5.1`; `ac:AC-5.2` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-X-01 | `tooling` | Make `cargo xtask stack` fail when a panic path (a diverging function of `core`'s panic machinery, where its walk already stops and which it already reports) is reachable from an entry point of the `ffi` build, the static library as it ships. That turns the walk into a panic-freedom check of the shipped library, which VER-010's exerciser link does not cover while it is built without LTO (`gh:#114`). It enforces GEN-020, a rule already written, and loosens no gate, so the coordinator may decide it ([decision rights](../process/decision-rights.md) §2.7) | `accepted` tracked with #114 (LTO of the shipped library) |
| P-X-02 | `oi` | Draft OI: "What should the panic handler of the shipped `no_std` static library do? It is `loop {}` (`crates/aldis-ffi/src/lib.rs:84`–`86`), so a panic that GEN-020 failed to exclude hangs the task without a trace. Options: (a) keep it, since VER-010 and P-X-01 exclude panics; (b) execute a trapping instruction, so that the platform's trap handler sees it; (c) call an integrator-supplied `extern \"C\"` hook, at the cost of a symbol the integrator must define (CABI-004)." Recommendation (b). A C ABI convention, so the maintainer's (§2.4) | `accepted` OI-49, `m2/hazard-controls` |
| P-X-03 | `doc` | A verification record for GEN-031 and GEN-032 (method I, with the A of CABI-004): the library crates are `no_std` without `std` or `alloc` in the flight build (`no-std-<target>` checks), forbid `unsafe` (GEN-070), have no dependency (GEN-001), and the shipped static library imports nothing but `memcpy`/`memset` equivalents (`cargo xtask ffi-imports`, CABI-004), so it can reach no clock, random source, I/O, or global mutable state of the platform; plus a review that no `static mut` or interior-mutable `static` exists in library code. Closes the untraced GEN-031 and GEN-032 | `accepted` this branch, records/GEN-031.md |
