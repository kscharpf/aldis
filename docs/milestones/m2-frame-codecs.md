# Milestone 2 — Frame codecs

**Goal:** the M1 codecs' part of the C ABI, the field-code catalogue, and the shared property-test strategies (WP-0), then complete, verified Layer 1 codecs for the four data link protocols: `aldis-tm`, `aldis-tc` (with the CLCW, frame delimiting, and frame validation), `aldis-aos` (with the FHEC and the editions), and `aldis-uslp` (with truncated frames). Each codec has its C ABI functions, and every vector passes through the C ABI as well as through the Rust API. They are verified to the standard of M1: vectors, panic freedom on bare metal, 100 % coverage, fuzzing, mutation testing, and cross-checks.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M1](m1-packets-time.md) (`v0.2.0-m1`) · **Release:** `v0.3.0-m2`

**Status: planned (2026-10-20).** Entry gate: M1 (roadmap §1), met by the M1 exit, PR #54 (branch `m1/exit`), release `v0.2.0-m1`; this plan merges after that PR. Decisions D1–D16 are proposed, for the maintainer to accept. `cargo xtask trace --scope m2` puts 121 requirements in scope; the 49 traced so far are the M0 and M1 artefacts of the shared codec contract, gates, and vectors. Three new open issues come from planning: OI-33, OI-34, and OI-35; OI-04 is updated with the reading of 732.0-B-4. Revised after the review of PR #56: D2, D5, D11, D14, and D15 changed, D17 added (USLP-014, USLP-015), and §3 and §5 cover the generated files shared by parallel pull requests.

## 1. Starting point

| In place after M1 | Missing for M2 |
|---|---|
| `aldis-core`: errors with the catalogue codes, bit fields, IDs, `DecodeMode`, `AnomalySet`, modular counters, containers; CRC-16 with `verify_fecf`, `write_fecf`, and `syndrome` (CMN-001…004); `OidGenerator` in Fibonacci and Galois form (CMN-020…023); `FrameLayout` and `SecurityLayout` (CMN-040, CMN-041, API-070, API-071, API-073, API-075) | An `Ocf` type: CMN-010 and CMN-011 were moved from M0 to M2. Numeric **field codes**: `FieldId` holds only a static name, which cannot cross the C ABI (CABI-010; OI-27, resolved for M2 WP-0) |
| `aldis-time`, `aldis-spp`, `aldis-epp`, complete and verified | Their C ABI (OI-21, resolved: M2 WP-0). The C++ track's C1 waits for it (roadmap §2) |
| `aldis-ffi`: skeleton with the ABI version constants, `cbindgen.toml`, and `capacities.toml` (FOP-1, FARM-1, link engines); no exported function | Every exported function, the generated header `include/aldis/aldis.h` (CABI-002), and `cbindgen` in the verification image (VER-056) |
| `aldis-testkit`: vector model, loader, exact-match runner (TV-012); 11 property tests in four crates use `proptest` directly | The shared strategies of VER-054, used by the property tests (VER-023) and the differential tests (VER-053) |
| Skeletons `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp` with the module layout of [profiles/rust.md §4](../profiles/rust.md#4-module-layout-convention); module headers only | All frame codec code, and the vector files under `test-vectors/{tm,tc,aos,uslp}/` |
| Exerciser on thumbv7em, armv7r, and riscv32imac under QEMU with every time, SPP, and EPP case; 20 fuzz targets; coverage gate for four crates; nightly mutants and stack sizes | The same gates for the four frame crates; the exerciser does not call `aldis-ffi`, whose own `panic-handler` feature only satisfies the linker |
| `crosscheck/` against `spacepackets` 0.18.0 (SPP, CUC, CDS, P-field, ASCII formatting) | Any cross-check of a frame; the candidates of OI-32, targeted at M2 |
| `trace --scope m0 --strict`, `--scope m1 --strict`, `--scope c0` | A scope for M2 in the gate |
| Specifications: [TM](../standards/tm-frame.md), [TC](../standards/tc-frame.md), [AOS](../standards/aos-frame.md), [USLP](../standards/uslp-frame.md), [common link features](../standards/common-link-features.md) | Open: OI-04 (AOS Issue ≤4), OI-05 (USLP TFDF header length), OI-06 (SANA registries, USLP UPIDs). 732.0-B-4, the superseded Issue 4, is now a `ref` document of `specs/` ([specs/README.md](../../specs/README.md)) and answers OI-04 (D11) |

## 2. Decisions

All proposed. Where a decision would settle an open issue, the issue is updated when the decision is accepted, not before.

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Trace scope | Add `--scope m2` from §4 to `cargo xtask ci`, the `spec` check of `cargo xtask verify`, and `ci.yml`, without `--strict`, in the first WP-0 pull request, so that progress is measured from the start (M1 added its scope only in WP-6); `--strict` at the M2 exit. This plan itself changes no `xtask` code, as M1's plan did not | Proposed |
| D2 | Field codes (API-082; OI-27 resolved) | `FieldId` becomes `{ code: NonZeroU16, name: &'static str }`, built only by `pub const` items, so no code can be made up at a call site. The catalogue in [03 §9.1](../03-api-model.md#91-field-codes) is the registry: one row per field, numbered in order from 1; code 0 means "no field" in the C ABI. One code per Blue Book field, shared by the codecs that carry it (for example the Spacecraft ID of TM, TC, AOS, and USLP). Every constant lives in one module, `aldis_core::field`, generated from the catalogue by `cargo xtask fields --write`; a codec crate re-exports the constants it uses, so a constant never moves when a second crate starts to use its field. `cargo xtask fields` (part of `cargo xtask ci`) fails when the generated module or `test-vectors/core/fields.json` (for the C++ and binding runners) differs from the catalogue, or a code is used twice. `FieldId::new(name)` goes away, which breaks the 0.x API (changelog **Breaking:**). Alternative: per-crate code blocks; they avoid number clashes between parallel pull requests but contradict API-082's "next free code", and roadmap §6 rule 7 already settles clashes | Proposed |
| D3 | Field codes in vectors | The error expectation of a codec case gains an optional `field` (the code). New cases give it wherever the error names a field, and runners compare it when present (TV-012). Released M1 cases are not edited (TV-021); for them the field is compared by the differential tests (VER-053) and the unit tests | Proposed |
| D4 | C ABI shape for the M1 codecs (WP-0; OI-21 resolved) | Per codec: `aldis_<std>_decode`, `_peek_length`, `_encode`, and the two-phase writer as two stateless calls, `_writer_begin` (payload offset and capacity in the caller's buffer) and `_writer_finish`, so no call keeps a pointer (API-023, API-060). Views are POD structs of decoded values plus offset/length pairs into the input (CABI-012). Configurations are C structs that start with `struct_size` (CABI-033); EPI sets are bit masks. Mode is a `uint32_t` (`ALDIS_MODE_STRICT`, `ALDIS_MODE_LENIENT`); anomalies are a `uint32_t` mask with the bit indices of the `Anomaly` trait. `aldis_error_detail_t` carries the kind, the field code (D2), and the `ErrorDetail` values (CABI-021). CUC fine time crosses as `uint8_t[10]`, left-aligned as on the wire, with its octet count, because the C ABI has no 128-bit type (CABI-010; M1 D2). ASCII time codes are octet buffers with an output length, no terminating NUL. The header is generated by a pinned `cbindgen` (version and licence checked against VER-055 in WP-0) into `include/aldis/aldis.h`; a verify check regenerates and diffs it (CABI-002) and compiles it as C99 and C++11 with the pinned GCC and Clang (CABI-003; VER-050's full matrix stays in M5). **Vectors through the C ABI:** an `aldis-ffi` integration test runs every vector file through the `extern "C"` functions with the same testkit handlers and exact comparison. Alternative: a C runner with the vectors compiled into C arrays; that is the C test harness of M5 (VER-026) | Proposed |
| D5 | Stateful M1 helpers in the C ABI | `SequenceTracker`, `Segmenter`, and `Reassembler` have fuzz targets, so C1 must compare them (VER-053). Each gets caller-owned opaque storage with generated `_SIZE` and `_ALIGN` constants and an `_init` call; the SDU or reassembly buffer is passed on every call and never stored (API-060). The tracker's capacity comes from a new `[spp]` section of `capacities.toml` (`ALDIS_SPP_TRACKER_MAX_APIDS`). The handle pattern of CABI-030…032 is set here, once, so that M3 reuses it for FOP-1 and FARM-1 instead of revising it: `aldis_limits` reports the capacity (CABI-030); `_init` checks the storage's size and alignment and fails with `ALDIS_ERR_BUFFER_TOO_SMALL` or `ALDIS_ERR_INVALID_FIELD` (CABI-031); the storage carries a magic and generation tag, which every call checks and `_init` sets, failing with `ALDIS_ERR_INVALID_HANDLE` on uninitialized or foreign storage (CABI-032). Alternative: leave them to M3's handle machinery and list them in the C1 plan as VER-053 exceptions until then | Proposed |
| D6 | Property-test strategies (VER-054) | A `strategies` module in `aldis-testkit`, behind a feature, depending on `proptest` and on the codec crates (normal dependencies of a tooling crate, RS-017). It generates valid typed values (identifiers, configurations, headers, whole data units) and invalid octet strings next to each boundary. Property tests that use it are integration tests (`tests/properties.rs` per crate): a unit test inside a crate would see two copies of the crate's types, its own and the one testkit depends on. The 11 M1 property tests move there in WP-0. `difftest/` uses the same module (CPP-060). Alternative: strategies over plain data with no dependency on the codec crates, converted in each test and in `difftest/`; no duplicate-crate issue, but every test repeats the conversion | Proposed |
| D7 | Crate order and shared code | No frame crate depends on another (GEN-002). Priority TM, TC, AOS, USLP: A2 needs TM and TC, M3 needs TC and the CLCW, and USLP waits on OI-05 and OI-06; after WP-1 the four can run in parallel. Shared code goes into `aldis-core` only when two crates need it: the new `Ocf` (CMN-010, CMN-011), the existing FECF functions (CMN-001, CMN-003), `OidGenerator`, and `FrameLayout` (CMN-040). The CLCW stays in `aldis-tc` (CMN-011): TM, AOS, and USLP views return an `Ocf`, and the caller decodes a Type-1 report with `aldis-tc`. Pointer classification (FHP, LVO, Bitstream Data Pointer) stays per crate, because widths and reserved values differ. Alternative: a shared `aldis-frame` crate, which would need a GEN-002 exception for each frame crate | Proposed |
| D8 | Requirements that cross crates (new OI-34) | USLP-010 asks the USLP writer to complete a partly filled TFDZ with an Encapsulation Idle Packet, which needs `aldis-epp`; USLP-013 asks for the TC control-command decoder, which is in `aldis-tc`. GEN-002 forbids both dependencies. Proposal: the USLP writer takes the TFDZ as given, and the idle fill is LNK-013's, in `aldis-link`, which may depend on both; USLP-013 is verified by an `aldis-uslp` integration test with `aldis-tc` as a dev-dependency, and again in `aldis-cop1` (M3). On acceptance USLP-010 drops the fill sentence and points to LNK-013, and OI-34 is resolved. Alternative: GEN-002 exceptions `aldis-uslp → aldis-epp` and `→ aldis-tc` behind optional features, as `aldis-spp → aldis-time` | Proposed |
| D9 | Frame anomaly flags and API-032 (new OI-33) | Several flags in the frame documents fall outside API-032, or would make a strict decoder's anomaly set non-empty ([03 §2.1](../03-api-model.md#21-views-decoding)). Proposal: `FhpBeyondDataField` (TM) and `PointerBeyondZone` (AOS) are dropped and the pointer is `Malformed` in both modes, as USLP-006 already has it; `FhecCorrected` is dropped and the view reports `corrected_symbols()` in both modes (AOS-014); `VcfLengthMismatchForQos` is dropped and the mismatch is an error in both modes, because the VCF Count Length sets where the header ends; `UnknownUpid` is dropped, since `Upid::Other(n)` already says it (USLP-007); `SegmentLengthIdNot11` (TM) stays and API-032 names the Segment Length Identifier as a fixed-value field (132.0 §4.1.2.7.5, notes 1–2). The rest stay. Each frame WP applies the accepted list; the standard documents and API-032 change with it | Proposed |
| D10 | FHEC (AOS-013, AOS-014) | 732.0-B-5 EC1 §4.1.2.6.5 specifies a Reed-Solomon (15,11) code over GF(2⁴), shortened by 5 symbols to a binary (40,24) code; AOS-013 calls the same code (10,6). Encoder: division by g(x) over the six systematic symbols. Decoder: four syndromes, a closed-form error locator for at most two errors, a root search over the ten transmitted positions only (a fixed loop, GEN-022), and Forney's formula; a locator with no valid roots, or roots in the virtual fill, is `HeaderUncorrectable`. GF(16) log and antilog tables of 15 entries are `const`. The independent model, in a unit test and a fuzz target, is a table built by enumerating every pattern of at most two symbol errors (10 · 15 + 45 · 15² = 10 275 patterns, plus none): the algebraic decoder must agree with it on all 65 536 syndromes. Alternative: the table itself in flight code; it costs ROM and is not needed. Three or more symbol errors can be miscorrected (OI-35) | Proposed |
| D11 | AOS editions (AOS-001…003, GEN-062; OI-04) | `Issue5` and `Issue4Compatible` are both complete in M2. 732.0-B-4, the Silver Book of Issue 4 (now a `ref` row of `specs/manifest.tsv`), confirms what AOS-002 and AOS-003 assume: an 8-bit SCID (§4.1.2.2.3), bits 42–43 a reserved spare set to `00` (§4.1.2.5.4), and an M_PDU header of a 5-bit reserved spare followed by an 11-bit First Header Pointer, with all ones for no packet start and all ones minus one for idle data only (§4.1.4.2.2, §4.1.4.2.3). Its FHEC (§4.1.2.6.5) is the code Issue 5 specifies: the same symbol size, correction capability, field and code generator polynomials, and mapping of header bits to symbols. Issue 5 describes it as the (15,11) code shortened by five leading symbols of zero virtual fill. Leading zero symbols leave a systematic code's check symbols unchanged, so the "upgrade" in the Issue 5 history changes the description, not the code, and one FHEC implementation serves both editions. On acceptance OI-04 is resolved and AOS-003 drops "to be confirmed" | Proposed |
| D12 | USLP truncated frames (USLP-002, USLP-011, USLP-012) | `UslpFrame::decode` returns an enum of a full and a truncated view, sharing the accessors of the first six header fields (Annex D1.3.1 note 2). For End of Frame Primary Header Flag = 1 the length comes from the VC configuration (6–32 octets, D1.3.2); the VC configuration rejects a truncated length on a fixed-length VC (D1.2.1, API-040) and SDLS with a length below 28 (D1.2.4, USLP-012, API-075). `peek_length` takes the VC lookup, because a truncated frame has no length field (API-013). Whether the decoder checks the construction rule and UPID that Annex D1.4.1.2 gives only in notes is OI-35; until it is answered they are exposed and not checked. USLP-011 is enforced by the multiplexing engine, so it is traced in M4 | Proposed |
| D13 | UPID values (USLP-007, USLP-009; OI-06) | A `cargo xtask sana import uslp_protocol_id` step downloads the SANA registry named in 732.1 Annex D1.4.1.2 once and commits a snapshot with its URL, retrieval date, and SHA-256; a generator writes `crates/aldis-uslp/src/upid/table.rs` from the snapshot. `cargo xtask ci` checks, offline, that the generated file matches the snapshot; a nightly job compares the snapshot with the live registry and opens an issue on a change (as `fetch.sh --check-urls`). The code names the values it needs, "Only Idle Data" (USLP-009) and "Mission Specific Information-1" (Annex D1.4.1.2), by registry name. The registry's export format and terms are checked in WP-5. Updates OI-06 for UPIDs on acceptance; PVNs (LNK-010) and EPIs (APP-045) stay open. Alternative: caller-supplied UPID values, as M1 D4 did for EPIs; that contradicts USLP-007's generated table, which would have to change | Proposed |
| D14 | C ABI for the frame codecs, and channel lookups | The D4 shape. Rust takes per-VC configuration through a `VcLookup` trait (AOS-010) with a fixed `VcTable<const N: usize>`; the C ABI takes an array of VC configuration structs and its count, so no call takes a callback (CABI-040). The TC frame delimiter is a stateless call that returns the offset and length of the next frame, or why the rest was discarded, so a C caller loops over it (TC-020). TC validation reasons (TC-021, TC-022) get stable numeric codes, listed in [tc-frame.md](../standards/tc-frame.md). The CLCW crosses as a `uint32_t` and a POD struct. `peek_length` (API-013): SPP, EPP, and TC read the length from the header and take only the prefix; TM and AOS, whose frame length is a managed parameter, take the physical channel configuration and return its frame length; USLP takes the physical channel configuration and the VC array, because a truncated frame's length is a VC parameter (D12). The frame functions share one shape, `aldis_<std>_peek_length(const uint8_t *prefix, size_t prefix_len, const aldis_<std>_config_t *cfg, size_t *out_len, aldis_error_detail_t *detail)` (with the VC array and its count after `cfg` for USLP); a prefix too short to hold the fields the length depends on returns `-ALDIS_ERR_INCOMPLETE` with `needed` in the detail (CABI-020, CABI-021). On acceptance API-013 states that a codec whose length depends on managed parameters takes the configuration `aldis_crc16` and the OID generator join the C ABI in WP-1, because the frame differential tests of C2 compare FECFs and OID data | Proposed |
| D15 | Independent cross-checks (VER-040; OI-32) | WP-6 searches, per frame codec, as M1 did for packets ([cross-checks.md](../verification/cross-checks.md) §4): open-source licence that allows use as a test oracle, independent of Aldis, read at source level. Projects to evaluate first, because M1 found them for other features: `eu.dariolucia.ccsds` (Java), Yamcs (Java, AGPL-3.0, used only as an oracle), NASA CryptoLib (C), `spacepackets` (Rust, for whatever frame support it has), gr-satellites, the Wireshark dissectors, and the `ccsds` crate. Their frame coverage is unchecked. Because `eu.dariolucia.ccsds` and Orekit also cover the OI-32 features (EPP, CUC octet 2, Level 2), a JVM harness in `crosscheck/` with a pinned JDK serves both: EPP first, then the frames. The JVM harness runs every night (VER-059), and in `cargo xtask verify` only when a pull request changes `crosscheck/` or `test-vectors/`, the way the C++ checks run only when their paths change, so most runs neither start a JVM nor take longer. The pinned headless JDK adds to the image once, next to the C++ toolchains and CodeQL it already holds; its size is measured in WP-6, and if it is out of proportion the harness becomes nightly only. Alternative: nightly only from the start, at the cost that a vector an independent implementation disagrees with can merge and is found the next night. A FHEC or OI-05 result is recorded against AOS-013 or OI-05. What stays without an independent implementation keeps `hand-derived` vectors with written derivations and a second review (M1 §5). Updates OI-32 on acceptance | Proposed |
| D16 | Gates for the new code | Exerciser: one group per frame crate, and one for `aldis-ffi` that calls every exported function, so the panic-link proof (VER-010) covers the C ABI layer; `build.rs` embeds every live case of `test-vectors/{tm,tc,aos,uslp}/`. Frames longer than the exerciser's buffers (USLP and AOS allow 65 536 octets) are reported as not representable, as one M1 case is, and the limit is recorded. Coverage: `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp` join `GATED` at 100 %; `aldis-ffi` is measured and reported, not gated (VER-024 covers Layers 0–2). Fuzz targets per WP-7. Mutation testing over the four crates and the changed `aldis-core`. Stack sizes with the FHEC decoder and the largest frame writers (VER-011) | Proposed |
| D17 | USLP receiving checks (OI-35 item 4) | New requirements USLP-014 and USLP-015 give USLP what TC-021 and TC-020 give TC, for a physical channel over TC Synchronization and Channel Coding (732.1 §4.3.11.1.6): `validate_frame` runs the five USLP Frame Validation Checks of §4.3.11.3.4, each failure with its own reason and stable numeric code (D14), test (d) comparing the octets present with the Frame Length field or, for a truncated frame, with the VC's truncated frame length; and a `FrameDelimiter` delimits frames by Frame Length and discards fill (§4.3.11.2.2–4). §4.3.11.2 delimits only by the Frame Length field, which a truncated frame does not have; how a data unit of truncated frames is delimited is OI-35 item 8. Both join WP-5 and §4. On acceptance OI-35 item 4 is answered. Alternative: no USLP validation in the codec, leaving it to the caller or to `aldis-link` (M4), so that USLP and TC differ | Proposed |

## 3. Work packages

Size is relative effort: **S** ≈ small, **M** ≈ medium, **L** ≈ large. WP-0 goes first, in three pull requests in this order; **C1 (native C++ packets and time) starts when WP-0c merges** (roadmap §2, C1 entry gate "M2 WP-0"). WP-1 needs only WP-0a. The four codecs then run in parallel; each adds its C ABI functions once WP-0c has set the shape. WP-6 and WP-7 grow with each codec rather than coming at the end.

The parallel codec pull requests share two generated files: `aldis_core::field`, generated from the catalogue in 03 §9.1 (D2), and `include/aldis/aldis.h`, generated from `aldis-ffi` (CABI-002). Neither is merged by hand. On a conflict, merge the source (the catalogue table, or the `aldis-ffi` sources) and regenerate (`cargo xtask fields --write`, and the header command WP-0c adds); the verify checks fail on a stale file. `aldis-ffi` has one module per codec, so two codecs' C ABI sources meet only in its module list. A codec's C ABI functions may also follow its Rust codec in a separate pull request, so that only one open pull request changes the header at a time.

```
WP-0a field codes, m2 scope ─┬─▶ WP-0b testkit strategies ─┐
                             └─▶ WP-0c M1 C ABI, header ───┴─▶ (C1 may start)
WP-0a ─▶ WP-1 core: Ocf, crc16/OID in C ABI ─┬─▶ WP-2 aldis-tm ───┐
                                             ├─▶ WP-3 aldis-tc ───┤
                                             ├─▶ WP-4 aldis-aos ──┤
                                             └─▶ WP-5 aldis-uslp ─┤
                 WP-6 vectors & cross-checks (alongside each codec) ┤
                 WP-7 gates: exerciser, QEMU ×3, coverage, fuzz, mutants ┴─▶ WP-8 exit review
```

Each codec module is done when:
- its requirements are covered by `// req:` tests and by vectors where the Verif. column says V;
- its vectors pass through the Rust API and through the C ABI (D4);
- it has a fuzz target (VER-022) and a property-based round-trip test drawn from the shared strategies (VER-023, VER-054);
- it reaches 100 % region, function, line, and branch coverage;
- it passes the full lint set with no new exceptions (RS-016), and its public items cite their clauses and requirement IDs (RS-015).

### WP-0 C ABI for the M1 codecs, field codes, strategies (L) — CABI-001…004, CABI-010…012, CABI-020…022, CABI-030…033, API-082, VER-054, VER-023

| Part | Task | Output |
|---|---|---|
| 0a (S) | `--scope m2` in the gate (D1). `FieldId` with its code (D2); the catalogue rows for every field the M1 crates and `aldis-core` name today; the generated `aldis_core::field` and `cargo xtask fields`; `test-vectors/core/fields.json`; the optional `field` of error expectations in the vector model and schema (D3). `aldis-core` gates run again (coverage, mutants, GEN-022/023 inspection updated) | `aldis-core::error`, `aldis-core::field`, 03 §9.1, `xtask`, `crates/aldis-testkit/src/model.rs` |
| 0b (M) | `aldis-testkit` strategies for `aldis-core` identifiers and the time, SPP, and EPP data units and configurations (D6); the 11 M1 property tests move to integration tests that use them | `aldis-testkit::strategies`, `crates/*/tests/properties.rs` |
| 0c (L) | `aldis-ffi` functions for every public decoder, encoder, writer, idle builder, conversion, and stateful helper of `aldis-time`, `aldis-spp`, and `aldis-epp` (D4, D5), each pointer and length checked (CABI-022) and every `unsafe` block with a `// SAFETY:` comment (GEN-070); `capacities.toml` `[spp]` and `aldis_limits`; the storage checks and handle tags of the stateful helpers (CABI-031, CABI-032); the generated header and its verify check; the vector runner through the C ABI; the static library built for the three bare-metal targets without `std` or a heap, and the static and shared libraries on the host (CABI-001, CABI-004) | `crates/aldis-ffi/`, `include/aldis/aldis.h`, `xtask/verify/Containerfile` (`cbindgen`) |

**Done when:** `aldis-core` is back at 100 % coverage with no new surviving mutants; every live case of `test-vectors/{time,spp,epp}/` passes through the C ABI; the header is committed and the regenerate-and-diff check passes; the field catalogue lists every field the M1 crates name and `cargo xtask fields` passes; the M1 property tests draw from the shared strategies. The C1 owner is told the header and catalogue are ready.

### WP-1 `aldis-core` additions (S) — CMN-010, CMN-011

| Task | Output |
|---|---|
| `Ocf` from four octets: `Clcw(raw)`, `ProjectSpecific(raw)`, `SdlsFsr(raw)` by bits 0–1 (CMN-010); the CLCW stays opaque (CMN-011) | `aldis-core::ocf` |
| Catalogue rows for the fields the frame crates name, regenerated into `aldis_core::field` (D2); each codec WP adds its own | 03 §9.1, `aldis-core::field` |
| C ABI: `aldis_crc16`, `aldis_ocf_classify`, the OID generator in caller storage (D14) | `aldis-ffi` |
| Unit tests, vector file `core/ocf.json`, a fuzz target `ocf_classify`, the exerciser group, coverage, mutants | core gates stay green |

**Done when:** CMN-010 and CMN-011 are traced, and `aldis-core` is at 100 % coverage with no new surviving mutants.

### WP-2 `aldis-tm` (M) — TM-001…011

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `config` | Physical, master, and virtual channel configuration (132.0 §5): fixed frame length, FECF, SCID, VCIDs 0–7, FSH 2–64 octets, OCF, data field content, `SecurityLayout` per VC; constructors reject what Table 5-2 forbids | API-040, API-041, API-070, GEN-060, GEN-061, RS-012 |
| 2 | `header`, `view` | Primary header and FSH ID octet; decode requires the configured length (API-012); version check; FSH version and overrun; OCF as `Ocf`; FECF checked before the view, plus `decode_unchecked`; data field length through `FrameLayout`; FHP classification; security header and trailer as slices; `peek_length` returns the configured length | TM-001…006, TM-008, API-010, API-012…014, API-072, GEN-013, GEN-053 |
| 3 | `anomaly` | Sync Flag 0: Packet Order Flag and Segment Length Identifier checks; Sync Flag 1: exposed raw. The flag list as D9 settles it | TM-007, API-030…033, GEN-043 |
| 4 | `writer` | One-shot and two-phase writers: FSH ID octet, OCF flag from the configuration, FECF; counts, sync flag, and FHP from the caller | TM-009, API-020…023, GEN-052 |
| 5 | OID writer | FHP `0x7FE`, data field from a caller's `OidGenerator`, VC configured for packets | TM-010 |
| 6 | `validate` | Frame against the master channel configuration: SCID, VCID, OCF flag, FSH presence and length, Sync Flag against data field content | TM-011 |
| 7 | C ABI | `aldis_tm_*` (D14) | CABI-010…012, CABI-033 |

**Done when:** every TM requirement is traced, and `tm/minimal`, `fsh_ocf_fecf`, `fhp_values`, `oid_sequence`, `security_layout`, and `invalid` pass through the Rust API and the C ABI.

### WP-3 `aldis-tc` (L) — TC-001…008, TC-020…023, TC-030, TC-031

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `config` | Physical, master, virtual channel, and MAP configuration (232.0 §5): maximum length ≤ 1024, FECF, VCIDs 0–63, Segment Header per VC, MAP IDs, `SecurityLayout` per VC or MAP | API-040, API-070, GEN-060, GEN-061, RS-012 |
| 2 | `header`, `view`, `writer` | Length from the Frame Length field, consuming exactly one frame; version and the reserved flag combination; spare bits; Segment Header only on Type-D frames of a VC that has one; FECF; writer limits (1024 total, data field 1019, or 1017 with the FECF); N(S) accepted as given on Type-B | TC-001…004, TC-006…008, API-011, API-013, API-020…023, GEN-053 |
| 3 | `control` | Unlock and Set V(R); other patterns `ReservedValue` in strict mode and raw in lenient mode | TC-005 |
| 4 | `anomaly` | `ReservedSpareSet`, `UnknownControlCommand`, `ClcwSpareSet` | TC-003, TC-005, API-032 |
| 5 | `clcw` | `Clcw` encode and decode, all fields; strict checks type, version, and spares; any COP in Effect value exposed; Status Field passed through | TC-030, TC-031 |
| 6 | `delimit` | Frame Delimiting and Fill Removal over one clean decoded data unit (232.0 §4.4.9.2; Cor. 1 adds that a decoding failure discards every frame of the CLTU, which the caller enforces by passing only clean units) | TC-020 |
| 7 | `validate` | Frame Validation Checks a–e, each with a distinct reason; invalid VCID and MAP ID; identical for Type-A and Type-B | TC-021…023 |
| 8 | C ABI | `aldis_tc_*`, `aldis_clcw_*`, the delimiter as a stateless call (D14) | CABI-010…012, CABI-033 |

**Done when:** every TC requirement is traced, and `tc/ad`, `bd`, `bc_unlock`, `bc_setvr`, `segment_header`, `max_length`, `delimit_multiple_frames_with_fill`, `validation_failures`, and `clcw` pass through the Rust API and the C ABI.

### WP-4 `aldis-aos` (L) — AOS-001…003, AOS-010…019, GEN-062

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `fhec` | Encoder and decoder of D10, with the table model in tests and fuzzing | AOS-013, AOS-014 |
| 2 | `config` | Physical channel (frame length, FHEC, Insert Zone, FECF, edition, default `Issue5`), master channel (SCID, VCIDs 0–62, 63 for OID), virtual channel (content, OCF, `SecurityLayout`) | AOS-001, GEN-062, API-040, API-070, GEN-060 |
| 3 | `header` | `AosPrimaryHeader::decode` with the physical channel configuration: version, 10-bit or 8-bit SCID by edition, VC Frame Count Cycle and its Use Flag, FHEC correction before any field is read | AOS-002, AOS-010…012 |
| 4 | `view` | `AosFrame::decode` with the `VcLookup` (D14): Insert Zone, security slices, OCF as `Ocf` by the VC configuration, FECF, fixed length | AOS-010, AOS-015, API-012, API-072 |
| 5 | `mpdu`, `bpdu` | First Header Pointer and Bitstream Data Pointer classification; the Issue ≤4 M_PDU header (D11); B_PDU zone limit (OI-35) | AOS-003, AOS-017, AOS-018 |
| 6 | `writer` | FHEC and FECF computed; counts, cycle, replay flag, and data field from the caller; OID frames on VCID 63 from the `OidGenerator` | AOS-016, AOS-019, API-020…023 |
| 7 | `anomaly` | The flag list as D9 settles it | API-032, API-033 |
| 8 | C ABI | `aldis_aos_*` (D14) | CABI-010…012, CABI-033 |

**Done when:** every AOS requirement is traced, and `aos/minimal`, `fhec_encode`, `fhec_correct_1_symbol`, `fhec_correct_2_symbols`, `fhec_uncorrectable`, `insert_zone`, `mpdu_pointers`, `bpdu_pointers`, `oid`, `issue4_compat`, and `scid_10bit` pass through the Rust API and the C ABI.

### WP-5 `aldis-uslp` (L) — USLP-001…010, USLP-012…015

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `upid` | SANA import and generated table (D13); `Upid` raw plus typed, unknown values as `Other(n)` | USLP-007 |
| 2 | `config` | Physical, master, virtual channel, and MAP configuration (732.1 §5): fixed or variable frames, Insert Zone only on fixed-length channels, VCF Count lengths per QoS, truncated length 6–32 and only on variable-length VCs, OCF allowed or required | USLP-004, USLP-008, USLP-012, API-040, API-070, API-075, GEN-060 |
| 3 | `header`, `view` | Version and reserved flag combination; fixed length from the configuration, variable length from the Frame Length field (consuming exactly one frame); VCF Count with its length checked against the QoS; Insert Zone, security slices, OCF, FECF | USLP-001, USLP-003, USLP-004, API-011, API-012, API-072 |
| 4 | `truncated` | The truncated view of D12 | USLP-002, USLP-012 |
| 5 | `tfdf` | TFDF header length by construction rule (USLP-005, OI-05), FHP and LVO classification, TFDZ; COP-1 control commands decodable with `aldis-tc` (D8) | USLP-005, USLP-006, USLP-013 |
| 6 | `writer` | Frame Length, VCF Count Length, TFDF header, FECF; OID frames on VCID 63, MAP 0, rule `001`, UPID "Only Idle Data", LVO at the last TFDZ octet, fixed-length channels only | USLP-009, USLP-010 (as D8 settles it), API-020…023 |
| 7 | `anomaly` | The flag list as D9 settles it | API-032, API-033 |
| 8 | `delimit`, `validate` | Frame Delimiting and Fill Data Removal, and the USLP Frame Validation Checks a–e, for a physical channel over TC Synchronization and Channel Coding (D17) | USLP-014, USLP-015 |
| 9 | C ABI | `aldis_uslp_*`, the delimiter as a stateless call (D14) | CABI-010…012, CABI-033 |

USLP-011 (no mixing of truncated and full frames in one C&S data unit) belongs to the multiplexing engine and is traced in M4; the codec documents it.

**Done when:** every USLP requirement is traced, and `uslp/fixed_packets_rule000`, `fixed_mapa_rules001_010`, `variable_rules011_111`, `vcf_count_lengths_0_to_7`, `truncated`, `truncated_sdls_layout`, `oid`, `insert_zone`, `protocol_control_cop1`, `delimit_multiple_frames_with_fill`, `validation_failures`, and `invalid` pass through the Rust API and the C ABI.

### WP-6 Vectors and cross-checks (M) — TV-001…003, TV-020, TV-021, VER-040, API-020, API-031, API-032

| Task | Output |
|---|---|
| Vector files for every category named in the four standard documents; every field the view exposes (TV-002); every error kind and every validation reason each decoder can return, with field codes (D3) | `test-vectors/{tm,tc,aos,uslp}/`, `core/ocf.json` |
| Blue Book material first where it exists (the OID pattern, CMN-020; the FHEC code parameters of 732.0 §4.1.2.6.5); otherwise hand-derived with a worked note | provenance per file |
| Cross-check search and harness for the frames, and the OI-32 candidates (D15). Every discrepancy is resolved against the Blue Book text and recorded | `crosscheck/`, [cross-checks.md](../verification/cross-checks.md) |
| A second review of the hand-derived frame vectors, independent of the sessions that wrote them (M1 §5) | `docs/verification/reviews/m2-vectors.md` |
| Testkit handlers per crate, registered in each crate's tests, in the C ABI runner, and in the exerciser codegen | handlers |

**Done when:** every vector file names its provenance, the cross-check run has no disagreement, each standard with no independent implementation is listed in `cross-checks.md` with what was searched, and the review is recorded.

### WP-7 Verification gates (M) — VER-010, VER-011, VER-022…025, GEN-020…024, VER-030, VER-031

| Task | Output |
|---|---|
| Exerciser groups for the four crates and `aldis-ffi` (D16); QEMU on the three targets as in M1 (M1 D6) | `exerciser/`, verify checks `exerciser-<target>` |
| Coverage gate for the four crates; `aldis-ffi` reported | `xtask/src/coverage.rs` |
| Fuzz targets: `tm_decode`, `tm_roundtrip`, `tc_decode`, `tc_roundtrip`, `tc_delimit`, `tc_clcw`, `aos_fhec_model`, `aos_decode`, `aos_roundtrip`, `uslp_decode`, `uslp_truncated`, `uslp_roundtrip`, `ocf_classify`, and `ffi_decode` (the C ABI decoders agree with the Rust decoders on the same octets). Each asserts no panic, `decode(encode(x)) == x`, and GEN-042 | `fuzz/`, `nightly.yml` matrix |
| Mutation testing of the four crates and `aldis-core`; survivors triaged before the release (VER-025) | [mutants-triage.md](../verification/mutants-triage.md) |
| Stack sizes per target with the new crates (VER-011) | nightly artefacts |
| GEN-022/023 inspection of the four crates and `aldis-ffi` | `docs/verification/inspections/` |
| VER-031: the minimal `aldis-spp` build still has no frame symbols | verify check `layers` |
| 1 CPU-hour per fuzz target before the release (VER-022) | run link in the exit table |

**Done when:** every gate is green on `main` for all M2 crates.

### WP-8 Docs, trace, release (S) — VER-020, RS-015

| Task | Output |
|---|---|
| `--scope m2 --strict` at the exit (D1) | `spec` check, `cargo xtask ci`, `ci.yml` |
| Crate docs cite clauses and requirement IDs (RS-015); the facade's `tm`, `tc`, `aos`, `uslp` features documented; the C ABI documented in the header comments | rustdoc, header |
| `README.md` status, `CHANGELOG.md` `0.3.0-m2` (`cargo xtask changelog release 0.3.0-m2 <date>`), roadmap row, workspace version `0.3.0` | docs |
| Open issues: OI-04, OI-05, OI-06, OI-32…35 updated with the accepted decisions; new spec questions as new OIs | [09-open-issues.md](../09-open-issues.md) |

## 4. Exit criteria

M2 is complete when every item below is green on `main`:

| Area | Criteria |
|---|---|
| C ABI for the M1 codecs (WP-0) | CABI-001…004, CABI-010…012, CABI-020…022, CABI-030…033 (for the SPP helpers; M3 adds FOP-1 and FARM-1), API-082. Every live case of `time/`, `spp/`, and `epp/` passes through the C ABI |
| Shared strategies | VER-054, VER-023 (property tests of the seven codec crates draw from `aldis-testkit`) |
| Common link features | CMN-010, CMN-011 |
| TM Transfer Frame | TM-001…011 |
| TC Transfer Frame and CLCW | TC-001…008, TC-020…023, TC-030, TC-031 |
| AOS Transfer Frame | AOS-001…003, AOS-010…019, GEN-062 |
| USLP Transfer Frame | USLP-001…010, USLP-012…015 |
| Codec contract | API-010…014, API-020…023, API-030…033, API-040, API-041, API-070, API-072, API-075, GEN-013, GEN-014, GEN-043, GEN-052, GEN-053, GEN-060, GEN-061, RS-012. Every live case of `tm/`, `tc/`, `aos/`, and `uslp/` passes through the Rust API and through the C ABI |
| Panic freedom and resources | VER-010, GEN-020…024 (the exerciser, with `aldis-ffi`, runs on all three bare-metal targets), VER-011 |
| Portability | GEN-005, GEN-006, GEN-007, VER-030, VER-031 |
| Gates | VER-022 (all WP-7 fuzz targets, ≥ 1 CPU-hour each before the release), VER-024 (100 % for the four frame crates and `aldis-core`), VER-025 (triaged) |
| Vectors and trace | TV-001…003, TV-020, TV-021, VER-020 (`xtask trace --scope m2 --strict`), VER-021, VER-040 |
| Docs | Roadmap updated; `CHANGELOG.md` entry for 0.3.0-m2; open issues updated |

Not in M2: CMN-024 (an optional OID check with no verification method); API-074 and the engines' security-hook order (M4); USLP-011 and the LNK requirements (M4); COP-1 (M3); the C ABI handles of FOP-1, FARM-1, and the link engines (M3, M4), which reuse the handle pattern D5 sets; CABI-040, CABI-041 (M3, M4); CABI-050, CABI-060, VER-026, and VER-050's full compiler matrix (M5); CABI-070…072 (track C and M5); synchronization and channel coding (roadmap §7, OI-12).

## 5. Risks

| Risk | Mitigation |
|---|---|
| WP-0 is on the critical path of two tracks: C1 cannot start, and the frame C ABI has no shape, until it merges | Split into three pull requests (WP-0a, 0b, 0c), 0a first; 0c starts with the header check and the SPP functions so the C1 owner can review the shape early |
| The C ABI layer is new `unsafe` code that the Rust gates did not cover in M1 | CABI-022 checks on every pointer and length, `// SAFETY:` on every block (GEN-070), the exerciser group and `ffi_decode` fuzz target (D16), and the C ABI vector runner; Miri over that runner is cheap to add early, ahead of VER-026 in M5 |
| The SANA registry changes, or its export format is not machine-readable | D13: committed snapshot, offline check, nightly comparison; if no export exists, the snapshot is transcribed by hand from the registry page with a second review, and its retrieval date recorded |
| No independent implementation covers the FHEC, USLP truncated frames, or TC delimiting | The FHEC model of D10 is exhaustive over all syndromes; vectors are written from the Blue Book text before the code; a second review of the hand-derived vectors (WP-6) |
| Frame anomaly lists change under OI-33 after a codec has shipped them | Settle D9 before WP-2 starts; each codec applies the accepted list from the start |
| Large frames (up to 65 536 octets) exceed the exerciser's buffers or stack on QEMU | D16: such cases are reported as not representable with the limit recorded; the host, s390x, and C ABI runs still cover them |
| Field-code clashes between parallel frame pull requests | Roadmap §6 rule 7: the pull request that merges second renumbers; `cargo xtask fields` reports duplicates |
| Merge conflicts in the generated `aldis_core::field` and `aldis.h` while four codecs run in parallel | §3: never merged by hand, regenerated from the merged source; one module per codec in `aldis-ffi`; a codec's C ABI may land in a separate pull request after its Rust codec |
| Ambiguities found during implementation | Resolve against the Blue Book text, record a new OI, and add a vector that pins the chosen reading |

## 6. Hand-off to the other tracks

| Track | Needs from M2 | When |
|---|---|---|
| C — C1 (packets and time, native C++) | The generated `aldis.h` with the M1 functions; the field catalogue and `test-vectors/core/fields.json` (the C++ `FieldId` gains its code, c0 plan D8); the testkit strategies for `difftest/` | WP-0c merged |
| C — C2 (frame codecs, native C++) | The frame functions in `aldis.h`, the frame vectors, `aldis_crc16` and the OID generator | M2 exit |
| A — A2 (TM and TC frames) | `aldis-tm`, `aldis-tc` with the CLCW; the OCF as `Ocf` | M2 exit (A2's entry gate); WP-2 and WP-3 merged earlier let A2 prepare |
| R — M3 (COP-1) | `aldis-tc` control commands and CLCW; USLP-013's TFDZ access (D8) | M2 exit |

When the C++ build starts reading `include/`, `CPP_PATHS` in `xtask/src/verify/mod.rs` gains it (AGENTS.md).
