# Formal proofs with Kani (spike, OI-42)

A spike on branch `m2/kani-spike`, 2026-10-22: how far the bounded model checker Kani takes Aldis toward proven properties. It adds proof harnesses to `aldis-core`, `aldis-spp`, and `aldis-time`, and the command `cargo xtask kani`, which the non-gating check `kani` of `cargo xtask verify` runs (§7). Nothing gates on it, and no requirement's verification method changes. The choice of method and tool is [OI-42](../09-open-issues.md).

## 1. Tool

| | |
|---|---|
| Kani | Kani 0.68.0 (`kani-verifier`, crates.io, released 2026-09-16), installed with `cargo install --locked kani-verifier@0.68.0` and `cargo kani setup` |
| Its compiler toolchain | `nightly-2026-08-21` (rustc 1.100.0-nightly 8925ea358), installed by `cargo kani setup`; Kani builds the crates with it, not with the pinned 1.99.1 (RS-001) or the tooling nightly (RS-002) |
| Model checker | CBMC 6.11.0, bundled; SAT solver CaDiCaL (Kani's default); Kissat is bundled too |
| Footprint | 352 MB in `~/.kani/kani-0.68.0` (release bundle, 109 MB download for `aarch64-apple-darwin`) and 1.3 GB for the nightly in `~/.rustup/toolchains`; the Linux bundles are 139 MB (`aarch64`) and 140 MB (`x86_64`) |
| Host of the measurements | Apple silicon, 8 cores, 8 GB, macOS; other sessions shared the machine, so the times are indicative (±50 %) |

`cargo xtask kani` checks that `cargo kani --version` reports the pinned 0.68.0 and refuses another version, then runs every harness and prints a table of result and CBMC time per harness. The non-gating check `kani` of `cargo xtask verify` runs it (§7).

## 2. Harnesses

Each harness is a Kani proof function in a `#[cfg(kani)] mod proofs` of its crate, with a `// req:` comment (VER-020); `cargo xtask trace` lists them as `proof` artefacts, apart from tests, since no gate runs them. Inputs are Kani's `any`; Kani's `assume` states only preconditions of the function under proof (a slice length within the buffer, a width that a type guarantees). The expected results are written from the requirement text and the wire format tables, not from the code: CRC-16 from the shift-register definition, the counter rules from CMN-031, the header fields from the bit positions of space-packet.md §1, the CUC properties from TIME-011.

"Every input" means every value of every argument, with no bound. "Up to N" means every value of the arguments, for lengths up to N.

| Harness | Target | Property | Inputs | Requirements | CBMC time |
|---|---|---|---|---|---:|
| `proof:crates/aldis-core/src/proofs.rs::crc16_matches_reference` | `crc16`, `crc16_bitwise`, `crc16_update`; again with `crc-table` (`crc16_table`) | equals a bitwise shift-register reference; the incremental form equals the one-shot form at every split point; no panic | every input of 0–8 octets, every split point | CMN-001, CMN-004, GEN-020 | 7.3 s; table 3.5 s |
| `proof:crates/aldis-core/src/proofs.rs::fecf_methods_agree` | `verify_fecf`, `syndrome`, `write_fecf` | the two methods of CMN-003 agree on every frame; a frame with a written FECF passes both; `Incomplete`/`BufferTooSmall` exactly below 2 octets | every frame of 0–8 octets | CMN-003, GEN-020 | 4.7 s |
| `proof:crates/aldis-core/src/proofs.rs::counter_classification` | the counter constructor, `next`, `classify`, `distance_to` | `new` accepts exactly widths 1–64 and values that fit; `classify` equals the CMN-031 reference (written with a `u128` modulus); `distance_to` and `next` are modulo 2^bits | **every** width, previous, and next count | CMN-030, CMN-031, GEN-024, GEN-020 | 35 s |
| `proof:crates/aldis-core/src/proofs.rs::bit_fields` | the bit-field `get` and `set` | accept exactly the fields inside the buffer with width 1–64 (and `set` the values that fit); `get` equals a bit-by-bit reference; `set` then `get` returns the value; `set` changes no other bit; a failed `set` changes nothing | every offset (all of `usize`), width, and value, buffers of 0–9 octets | GEN-050, GEN-051, GEN-042, GEN-020 | 436 s |
| `proof:crates/aldis-core/src/proofs.rs::octet_integers` | the octet-integer `read_u32` and `write_u32` | accept exactly the offsets inside the buffer; big-endian; write then read back; a failed write changes nothing | every offset, buffers of 0–9 octets | GEN-050, GEN-051, GEN-020 | 2.7 s |
| `proof:crates/aldis-spp/src/proofs.rs::primary_header_every_input` | the header `parse` and `octets`, the packet `peek_length` | rejection exactly for version ≠ `000` with the documented error; every field read at its position; data length = PDL + 1, packet length = PDL + 7; every accepted header re-encodes to the same 6 octets; `peek_length` agrees | **every** one of the 2⁴⁸ headers | SPP-001, SPP-002, SPP-003, SPP-009, API-014, GEN-020 | 2.1 s |
| `proof:crates/aldis-spp/src/proofs.rs::decode_rejects_exactly_per_spec` | the packet decode, both modes | the outcome is exactly the one SPP-001, SPP-002, and SPP-004 give, in the documented order: `Incomplete(6)`, `UnsupportedVersion`, `Incomplete(7 + PDL)`, strict-mode `InvalidField` for an Idle Packet with the flag set; on success the length, the data field, and the lenient anomaly | every header, inputs of 0–8 octets | SPP-001, SPP-002, SPP-004, API-011, API-032, GEN-020 | 4.0 s |
| `proof:crates/aldis-spp/src/proofs.rs::header_encode_decode_every_field` | the header constructor, `octets`, `parse` | encode then decode gives the same header, with version `000` | **every** packet type, APID, flag, count, flag combination, and data length 1–65 536 | SPP-001, SPP-002, SPP-009, API-014, API-021, GEN-020 | 1.2 s |
| `proof:crates/aldis-spp/src/proofs.rs::packet_encode_decode` | the configuration constructor, its `packet`, `encode`, the packet decode | `packet` accepts exactly what SPP-005, SPP-006, and §4.1.3.3.3.4 allow; `encode` writes exactly the packet, or fails with `BufferTooSmall` and leaves the buffer unchanged; strict decode returns the fields and parts | every header field and Maximum Packet Length; secondary header and user data 0–3 octets each; output buffers of 0–12 octets | SPP-002, SPP-005, SPP-006, API-020, API-021, API-022, GEN-020 | 533 s |
| `aldis-time` `cuc_from_ns_floor_fine{1,2}` | the CUC constructor from seconds and nanoseconds | fails exactly for `nanos` ≥ 10⁹ or `seconds` too wide (TIME-012); coarse time = `seconds`; the fraction is the largest value of the format at or below `nanos` (round down, OI-40); the result is representable (the CUC constructor accepts it); back to nanoseconds is never later | **every** `seconds`, `nanos`, coarse width 1–7, epoch; 1 and 2 fine octets | TIME-011, TIME-012, GEN-020 | 3.1 s / 47–79 s |
| `aldis-time` `cuc_to_ns_truncates_fine{1,2,3}` | the conversion to seconds and nanoseconds | seconds = coarse; nanoseconds = ⌊fraction × 10⁹⌋ | **every** value; 1, 2, 3 fine octets | TIME-011, GEN-020 | 0.3 s / 2.5 s / 3.0–4.8 s |
| `aldis-time` `cuc_round_trip_value_fine{1,2}` | value → ns → value | exact if and only if the value is a multiple of 2⁻⁹ s (always with 1 fine octet), otherwise exactly one unit of the last fine octet lower: the statement of TIME-011 | **every** value and coarse width; 1 and 2 fine octets | TIME-011, GEN-020 | 1.9 s / 62–87 s |

Every harness verifies on the code of `main` at 46bfe0f; the `aldis-time` harnesses were run again on 2026-10-23, alone on the host, with a 30-minute limit each. Where two times are given, they are the two runs. Three CUC properties have no harness, because CBMC did not finish them within the limit (§6). Kani found no defect (§5).

### Bounds and unwinding

A loop is unrolled to the bound of its unwind attribute, and Kani adds an *unwinding assertion* to each loop: if a path could run a loop more often than N, the harness fails. So an insufficient bound fails the proof, and a passing proof means the bound covered every path. The bounds, and why they suffice:

| Harness | Unwind | Why |
|---|---:|---|
| `crc16_matches_reference`, `fecf_methods_agree` | 9 | at most 8 octets, and 8 bits per octet, plus the exit test |
| `bit_fields` | 73 | the reference loops run over at most 9 × 8 = 72 bits; the library folds over at most 9 octets |
| `octet_integers` | 10 | the copy runs over at most 8 octets |
| `decode_rejects_exactly_per_spec` | 9 | the slice comparisons run over at most 8 octets |
| `packet_encode_decode` | 13 | the copy loop of `encode` runs at most 12 times |
| `error_kind_lookup_is_bounded` | 19 | the lookup of the error-kind lookup by code runs over the 18 kinds of the catalogue, for every code: the GEN-022 bound of that loop (added 2026-10-24, M2 WP-7; 9 s) |
| the others | none | no loops |

## 3. What is proven, and up to what

- **For every input, with no bound:** the CMN-031 classification and the counter constructor for every width 1–64 and every count; the Space Packet primary header decode for all 2⁴⁸ headers, with decode→encode identity on the accepted ones; encode→decode for every valid header field combination; the CUC conversion to nanoseconds for 1–3 fine octets, and the conversion from nanoseconds and the value round trip of TIME-011 for 1 and 2 fine octets, every coarse width and epoch, and every `seconds` and `nanos`. Not proven: the conversion from nanoseconds and the value round trip for 3 fine octets, and the nanoseconds round trip of TIME-011 for 4 or more (§6); tests, vectors, and the `time_cuc` fuzz target still cover them.
- **For every value up to a length:** CRC-16 (0–8 octets), the FECF methods (0–8), the bit fields (buffers of 0–9 octets, but every offset and width), the packet decode (every header, 0–8 octets: the packets with a Packet Data Length of 0 or 1 are decoded in full, and all others reach the `Incomplete` branch, which is also proven), and whole-packet encode and decode (parts of 0–3 octets).
- The length bounds are not a proof for longer inputs. For the CRC the step is the same for every octet, so the bound covers each code path, but not each data length. Tests, vectors, and fuzzing still cover the long inputs.
- No panic: every harness also proves that the code it calls cannot panic, overflow, or index out of bounds on its inputs (Kani checks these by default). This is GEN-020 for those functions and inputs; the exerciser link check (VER-010) remains the whole-library argument.

## 4. Trusted base

A proof holds only as far as these are right:

1. **Kani and CBMC:** the translation of MIR to CBMC's goto programs, and CBMC with CaDiCaL. A soundness bug in either could make a false property pass. The canaries (§4.1) show that the harnesses can fail, not that the tool is sound.
2. **The compiler that Kani uses:** `nightly-2026-08-21`, not the pinned 1.99.1. The proof is about the MIR of that compiler; the shipped code is built by another.
3. **The model of `core`:** Kani verifies the standard library's own code where it can and replaces some functions (memory intrinsics, `memcmp`) with models. Kani reported two unsupported constructs in `aldis-core`, `caller_location` (panic locations) and one foreign function; a harness that reaches an unsupported construct fails, so none is on a proven path.
4. **The harness itself:** its expected results and its assumptions. An `assume` that is stronger than the precondition hides inputs, and a wrong reference proves the wrong thing. They are written from the requirement text (§2) and must be reviewed like tests. The canaries show the properties are not vacuous.
5. **Target width:** Kani models the host, a 64-bit target. `usize` properties (the offsets of `bit_fields`) are proven for 64-bit `usize`, not for the 32-bit targets of the matrix.

### 4.1 Canaries

A mutation was planted in a scratch edit of the library, the harness run, and the edit reverted; no mutation is in the tree. Counterexamples are from `-Z concrete-playback --concrete-playback=print`.

| Mutation | Harness | Result | Counterexample |
|---|---|---|---|
| `file:crates/aldis-core/src/crc.rs`: generator `0x1021` → `0x1031` | `crc16_matches_reference` | FAILED, 5.2 s | the one-octet input `[0xF7]` |
| `file:crates/aldis-core/src/counter.rs`: gap rule `d <= half` → `d < half` | `counter_classification` | FAILED, 10.4 s | 9-bit counter, previous 0, next 256: forward distance 256 = modulus/2 must be `Gap { missing: 255 }`, the mutant says `Backwards` |
| `file:crates/aldis-spp/src/view.rs`: the Idle Packet check drops `&& has_secondary_header()` | `decode_rejects_exactly_per_spec` | FAILED, 16.6 s | `17 FF C0 00 00 00 00 00` (telecommand, flag 0, APID 2047, PDL 0), 8 octets, lenient: the mutant raises `IdleWithSecondaryHeader` for a packet without the flag |

## 5. Findings

No proof failed on the real code. The CMN-031, SPP-001/002/004, and TIME-011 statements hold as written for the inputs of §3; in particular the TIME-011 statement on the value round trip ("one unit of the last fractional octet earlier") is exact: never two units.

## 6. Limits found

- **Arithmetic cost.** CBMC bit-blasts arithmetic. With the number of fine octets symbolic (1–3), the CUC harnesses shift and divide `u128` values by symbolic amounts, and CBMC did not finish `cuc_round_trip_value` within an hour or `cuc_round_trip_nanos` (4–10 octets) within 10 minutes. Fixing the count per harness (macros generate one harness per count) brought 1 and 2 fine octets to seconds or minutes. With 3 fine octets, `from_seconds_nanos` and the value round trip still did not finish within 30 minutes, nor did the nanoseconds round trip with 4 (2026-10-23, alone on the host; an earlier attempt at 4 ran more than an hour under load). The 128-bit products and divisions by 10⁹ grow with the fraction width, and so do the SAT problems; these three have no harness. 5–10 fine octets were not tried.
- **Unbounded loops.** `decode_rejects_exactly_per_spec` without an unwind bound ran 24 minutes and was stopped: CBMC kept unrolling the comparison of the data field, whose length it could not bound from the buffer. With unwind 9 it finishes in 4 s. Every harness with a slice comparison or iterator needs an explicit bound.
- **Iterator-heavy code is slow.** `packet_encode_decode` (533 s) and `bit_fields` (436 s) dominate the run: `encode`'s `zip`/`chain`/`flat_map` copy and the `u128` fold of `bits` over a symbolic octet range expand into many paths. Longer bounds grow quickly; a 65 536-octet packet is out of reach.
- **Messages.** `assert_eq!` failures report "This is a placeholder message; Kani doesn't support message formatted at runtime"; the concrete playback gives the inputs instead.
- **Unstable options.** A per-harness timeout (`--harness-timeout`) and concrete playback need `-Z unstable-options` / `-Z concrete-playback`.
- **Scope of the tool.** Kani proves properties of bounded executions. Stateful engines (COP-1, the link receivers) can be checked for a bounded number of steps from a symbolic state, but invariants over unbounded runs need loop or function contracts, which are still unstable in Kani, or a deductive tool (OI-42 option (b)).
- **Not a coverage artefact.** The harnesses compile only under `cfg(kani)`, so the coverage run (VER-024) and clippy never see them; their code is linted by the Kani compiler's rustc warnings only.

## 7. The non-gating, nightly-only check `kani`

The maintainer accepted OI-42 option (a) as far as a non-gating check (2026-10-23, #92), and on 2026-10-24 moved it to the nightly run only. The check `kani` of `cargo xtask verify` (`file:xtask/src/verify/checks.rs`) runs `cargo xtask kani` in the verification image with `gating: false` and `nightly: true`. It runs only with `--all`, which the nightly verification run uses on `x86_64` and `aarch64` (VER-059); every other run, among them the pull-request runs of `verify --publish`, records it as `not-run` with the reason "nightly only (VER-059)", never as skipped or reused, and never takes it as a source (VER-056). In the nightly run, `cargo xtask verify nightly` fails the job when it did not pass, so a failing harness opens the nightly-failure issue; the other non-gating checks fail no run. Its inputs are `RUN` and the crates with harnesses (`KANI`: `crates/aldis-core/**`, `crates/aldis-spp/**`, `crates/aldis-time/**`). It takes about 20 minutes of CPU time (§2: about 8 for `aldis-core`, 9 for `aldis-spp`, and 3 for `aldis-time`); in the GitHub-hosted runs of 2026-10-24 it took 10 to 18 minutes.

The image (`xtask/verify/Containerfile`, last layer, `KANI_HOME=/opt/kani`):

- **Driver.** `kani-verifier` 0.68.0 from crates.io with `--locked`. `file:xtask/src/kani.rs` pins the same version and refuses another; a test checks that the two agree.
- **Bundle.** The release bundle (kani-compiler and CBMC, 139–140 MB per Linux architecture) is downloaded and checked against the SHA-256 that GitHub records for the release asset, then installed with `cargo kani setup --use-local-bundle`. The project publishes no checksum file; GitHub's asset digests and the two downloads of 2026-10-23 agreed (`aarch64` `0042d268…`, `x86_64` `32e2b484…`).
- **Toolchain.** `cargo kani setup` installs the nightly the bundle is built against (`nightly-2026-08-21`) through rustup, which checks it; about 1.3 GB, a third toolchain beside 1.99.1 and the tooling nightly.

Still open in OI-42: making the check gating, and a verification method for the requirements a harness proves (the letter **F** is taken).
