# 02 — Design rules

These rules apply to every crate in Layers 0–2, and to the C ABI. Layer 3 follows them too, except where [05](05-framework.md) states otherwise. Native C and C++ implementations SHALL meet the same rules in their own language (see [C ABI profile §9](profiles/c-abi.md#9-rules-for-native-cc-implementations) and the [C++ profile](profiles/cpp.md)).

## 1. Memory

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-010 | Layers 0–2 SHALL NOT allocate dynamically in any configuration. All storage SHALL be caller-provided buffers or fixed-capacity containers sized at compile time. | — | A |
| GEN-011 | Every fixed capacity, such as queue depth, maximum frame length, or number of VCs/MAPs/APIDs, SHALL be a compile-time parameter (const generic in Rust, build-time constant in C). Its value SHALL be checked against the managed parameters when a configuration is constructed. | — | T |
| GEN-012 | The memory footprint of every engine type SHALL be computable at compile time. The Rust profile exposes it through `core::mem::size_of`; the C ABI through `*_storage_size()` functions/constants. | — | A |
| GEN-013 | Decoders SHALL be zero-copy: payload accessors return sub-slices of the input buffer. | — | T |
| GEN-014 | No API SHALL require the caller to provide more buffer space than the maximum size of the data unit being produced. | — | T |
| GEN-015 | Aldis SHALL assume that memory holding its state between calls (configurations, engine state, and helper storage) is protected against single-event upsets by the platform (error-correcting memory with scrubbing, or equivalent), or holds data that does not change at run time. [Hardware assumptions](hardware-assumptions.md) SHALL state this assumption, what an upset in unprotected memory does to Aldis (an enum with an invalid discriminant is undefined behaviour; a restricted value is not checked again after construction, RS-020), and what the integrator does to meet it. | — | I |

## 2. Panic freedom and bounded execution

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-020 | No public API SHALL panic, abort, or trap for **any** input value, buffer content, or call sequence. This includes arithmetic overflow, slice indexing, and division. Misuse SHALL be reported as an error. | — | A/F |
| GEN-021 | Panic freedom SHALL be demonstrated by analysis: a release build of each crate's API exerciser, linked with a panic handler that references an undefined symbol, SHALL link successfully (see VER-010). | — | A |
| GEN-022 | Every loop SHALL be bounded by the length of an input buffer, a compile-time capacity, or a constant. There SHALL be no recursion. | — | A |
| GEN-023 | Every engine operation SHALL complete in time bounded by O(input length + configured capacities). No operation SHALL block or wait. | — | A |
| GEN-024 | Arithmetic on wire fields SHALL use explicit wrapping, checked, or saturating operations matching the field's modulus. For example, VC frame counts are modulo 2⁸, 2²⁴, or 2^(8·n), and Space Packet sequence counts are modulo 2¹⁴. | 133.0 §4.1.3.4.3.4; 132.0 §4.1.2.5–6; 732.0 §4.1.2.4; 732.1 §4.1.2.12 | T/V |

## 3. Determinism

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-030 | For identical input sequences, including the supplied `Instant` values, every engine SHALL produce identical output sequences, byte for byte and event for event. | — | V |
| GEN-031 | Layers 0–2 SHALL NOT read clocks, random sources, environment variables, or global mutable state. | — | I |
| GEN-032 | Layers 0–2 SHALL NOT perform I/O or logging. Observable behavior is expressed only through return values and engine outputs and events. | — | I |
| GEN-033 | Engines SHALL be safe to move between threads (Rust: `Send` where their contents are). They SHALL NOT be internally synchronized: callers serialize access to each instance. | — | I |

## 4. Error model

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-040 | Fallible operations SHALL return a result carrying an error **kind** from the catalogue in [03 §9](03-api-model.md#9-error-catalogue). Each kind has a stable numeric code shared by all profiles. | — | T |
| GEN-041 | Errors SHALL identify *what* failed (kind), and where useful *which field* (field identifier) and *expected vs found* values, without allocation. | — | T |
| GEN-042 | Decoding a malformed data unit SHALL NOT modify caller-visible state other than the returned result. Engines SHALL NOT change state when an input is rejected, except where the CCSDS procedure itself defines a state change (e.g. FARM-1 entering Lockout). | — | T |
| GEN-043 | Lenient-mode anomalies (see [03 §3](03-api-model.md#3-strict-and-lenient-decoding)) SHALL be reported as a bit set alongside a successful result, not as errors. | — | T |

## 5. Bit and octet conventions

All CCSDS standards in scope use the same convention, which these rules restate. Source: 133.0 §1.6.3 and the equivalent clause in each Blue Book.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-050 | Within an N-bit field, Bit 0 is the first transmitted bit and the most significant bit. Multi-octet fields are big-endian (network order) on the wire, independent of host endianness. | 133.0 §1.6.3 | V |
| GEN-051 | Field extraction and insertion SHALL be implemented by shared, tested bit-field primitives in `aldis-core`. Codecs SHALL NOT use ad-hoc shifting except through those primitives. | — | I |
| GEN-052 | Encoders SHALL set all spare and reserved bits to `0`. | e.g. 133.0 §1.6.3 ("all spare bits shall be permanently set to '0'") | V |
| GEN-053 | Length fields that CCSDS defines as "length − 1" SHALL be exposed through the API as true lengths. The −1 adjustment SHALL be confined to the codec. | 133.0 §4.1.3.5.2; 232.0 §4.1.2.7.2; 732.1 §4.1.2.7.2; 132.0 §4.1.3.2.3.2 | T |

## 6. Configuration (managed parameters)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-060 | Each set of CCSDS managed parameters (physical channel, master channel, virtual channel, MAP, packet transfer) SHALL be represented as a configuration type whose constructor validates all constraints in the corresponding Blue Book tables. An invalid configuration SHALL be unrepresentable after construction. | 132.0 §5; 232.0 §5; 732.0 §5; 732.1 §5; 133.0 §5 | T |
| GEN-061 | Configurations SHALL be immutable after construction. Changing a managed parameter means constructing a new configuration and, for engines, an explicit `reconfigure` operation whose effect on in-flight data is documented per engine. | — | T |
| GEN-062 | Where a newer Blue Book issue changes the wire format incompatibly, the configuration SHALL carry an explicit **edition** selector. The default SHALL be the issue named in [00 §2](00-introduction.md#2-v1-scope). (Example: AOS Issue ≤4 vs Issue 5, see [AOS](standards/aos-frame.md) §2.) | 732.0 §4.1.2.2.3, §4.1.4.2.1.4 | T/V |

## 7. Unsafe code

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-070 | Layers 0–3 SHALL forbid `unsafe` code (`#![forbid(unsafe_code)]`). | — | A |
| GEN-071 | `unsafe` is permitted only in `aldis-ffi`, in the dev-only differential-test bridge (`difftest/`, VER-053), which calls a native implementation through its C ABI and is never part of a flight build, and in the module of the dev-only cross-checks that calls a C oracle (`crosscheck/src/cfe_msg.rs`, VER-040). Every `unsafe` block SHALL carry a `// SAFETY:` justification. `aldis-ffi` SHALL be tested under Miri. | — | A/I |

## 8. Versioning and compatibility

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-080 | All crates SHALL follow Semantic Versioning. Wire behavior that conforms to the CCSDS standard SHALL NOT change in a minor or patch release, except to fix a deviation from the Blue Book. Every such fix SHALL be called out in the changelog. | — | I |
| GEN-081 | Public enums that may grow, such as error kinds, anomaly flags, and event kinds, SHALL be declared extensible (`#[non_exhaustive]` in Rust; C consumers SHALL handle unknown values). | — | I |
