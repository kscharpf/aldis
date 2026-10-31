# Rust profile

This profile binds the language-neutral specification to Rust. The Rust crates are the **reference implementation**.

**Terms and acronyms:** see the [glossary](../glossary.md).

## 1. Toolchain and edition

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| RS-001 | Crates SHALL use Rust edition 2024. The toolchain SHALL be pinned in `rust-toolchain.toml` to stable **1.99.1**, and the workspace `rust-version` SHALL equal the pinned version ([open issues](../09-open-issues.md) OI-07). Changing the pinned version is a configuration-controlled change recorded in the changelog. | — | A |
| RS-002 | Crates SHALL build on the pinned stable toolchain. Nightly SHALL be used only for tooling (fuzzing, Miri, `-Z build-std` for tier-3 targets, stack-size emission) and SHALL NOT be needed to build any crate. | — | A |

## 2. Crates and features

| Crate | Default features | Optional features |
|---|---|---|
| `aldis-core` | — | `alloc`, `std`, `crc-table` (default bitwise CRC otherwise) |
| `aldis-time` | — | `alloc`, `std` |
| `aldis-spp` | — | `time` (typed secondary-header time codes), `alloc`, `std` |
| `aldis-epp`, `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp` | — | `alloc`, `std` |
| `aldis-link` | — | `tm`, `tc`, `aos`, `uslp`, `spp`, `epp`, `alloc`, `std` |
| `aldis-cop1` | `tc` | `uslp`, `alloc`, `std` |
| `aldis-framework` | — | `alloc`, `std`, `std-udp`, `std-tcp`, `embedded-nal`, `trace`, `async` |
| `aldis` (facade) | — | one feature per crate, `spp-time` (`spp` and `time` with `aldis-spp/time`, for the time secondary header of SPP-008), `alloc`, `std`, `full` |
| `aldis-ffi` | `std` | `std`, `panic-handler`; enables `aldis/full`; capacities via build-time config ([C ABI §5](c-abi.md#5-memory-model)) |

## 3. API shapes

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| RS-010 | Identifier types (API-001) SHALL be newtypes with private fields. Sub-width identifiers SHALL be constructed only through validating functions (`TryFrom<u8/u16/u32>`, `const fn new(..) -> Option<Self>`); full-width identifiers (such as `FrameSeqNum`) SHALL provide `const fn new(..) -> Self` and `From`. The public API SHALL NOT provide unchecked constructors that can produce an out-of-range value. | API-001 | T |
| RS-011 | `Error` SHALL be a small `Copy` struct `{ kind: ErrorKind, field: Option<FieldId>, detail: ErrorDetail }` with `#[non_exhaustive]` enums (GEN-041, GEN-081). With `std`, it SHALL implement `std::error::Error`. With `core`, it SHALL implement `core::error::Error` (Rust ≥ 1.81). | GEN-040 | T |
| RS-012 | Configuration types SHALL have private fields, validated constructors or builders, and `Clone + PartialEq + Debug`. They SHALL be constructible in `const` context where the validation is `const`-evaluable. | GEN-060, API-041 | T |
| RS-013 | Engines SHALL implement `Send` when their contents do. They SHALL NOT implement `Sync`-dependent interior mutability (GEN-033). | GEN-033 | A |
| RS-014 | Withdrawn: engines take no security hook type parameter (API-050, API-074; OI-56). SDLS is applied by the caller, and RS-023 gives its order a typestate. | — | — |
| RS-015 | Public items SHALL be documented. Each item's documentation SHALL cite the CCSDS clause(s) and requirement IDs it implements. `#![deny(missing_docs)]` SHALL be set. | VER-020 | A |
| RS-020 | **Parse, don't validate.** A public type SHALL be able to hold only values valid for it. A field with a restricted range or set of values SHALL be a type that enforces it (RS-010 newtypes, enums). A constraint between fields SHALL be enforced by the constructor of the type that holds them, and such a type SHALL keep its fields private unless every field type is valid by construction and no constraint spans fields. Untrusted input (wire octets, caller integers, configuration values) SHALL be checked once, where it is turned into the type, by a constructor, `TryFrom`, or decoder that returns the type or an error; code that receives the type SHALL rely on it rather than re-check. A constructor that combines independently valid values MAY check only the relation between them (for example, that a value fits a format, or a packet against managed parameters, EPP-010). A function that takes an already-built value of the crate's own type and returns `Result<()>` SHALL NOT exist for a constraint the type could enforce. Verified by review, and for one pattern also mechanically: `cargo xtask rs020` flags every bare integer in a public field or enum payload of a codec crate that `xtask/rs020-exemptions.toml` does not exempt with a reason. | GEN-020, API-001, GEN-060 | I/A |
| RS-021 | **Invalid states are unrepresentable.** A state machine SHALL make every state and every transition the specification does not allow impossible to represent. Where the caller's sequence of calls determines the next state (a lifecycle such as unconfigured, configured, and active; a two-phase writer; a builder; a segmentation or reassembly session), it SHALL use the **typestate** pattern: each state is a distinct type, and an operation is available only on the states that allow it and consumes the value, returning the next state's type. Where an external event determines the next state (the protocol states of FOP-1 and FARM-1, driven by received frames, CLCWs, and timers), the state SHALL be a closed `enum`, and the transition function SHALL be generated from the state table or checked against it cell by cell, with an exhaustive `match` and no wildcard arm. A flag that the Blue Book defines as a function of the protocol state SHALL be derived from the state, never stored beside it; only variables that are independent of the state are stored, and a check SHALL confirm that the actions the state table prints for such a flag agree with its next state. A C ABI handle (CABI-030) MAY hold either form behind its opaque storage. | GEN-020, API-001 | I |
| RS-022 | **Engine step.** An engine SHALL be a plain-data state type with a `step` (or `decide` and `commit`) of API-057 and API-058: `&self` for the pure part, the state returned or committed only on success, `Actions<A, N>` a fixed-capacity `#[must_use]` list with `N` a `const`, and the input an enum with one variant per event. Where an engine's protocol state is event-driven (COP-1), RS-021 applies to its state enum, and the table-driven transition is the pure function that is verified (a Kani target and the cell-by-cell check, COP-003). `decide` SHALL be used only where copying the state is too costly, and then every vector and property of the engine SHALL run through `decide` and `commit`, so that the pair is held to the one-function form's results (API-058). `commit` SHALL take the decision by value, so that a decision is applied once. | API-057, API-058, RS-021 | I/T |
| RS-023 | **Frame finishing typestate.** The codec writers' frame SHALL be a typestate `Frame<Unprotected>` (the engine's action: every field final except the Security Header and Trailer contents, the FHEC, and the FECF; with the ranges of API-074) → `Frame<Protected>` (after `SecurityHook::protect`, or `NoSecurity`, consuming the unprotected frame) → `Frame<Finalized>` (FHEC and FECF computed last, consuming the protected frame), and only a `Finalized` frame SHALL expose its octets for transmission. On reception, `Received` → `Checked` (FECF checked, AOS FHEC corrected) → `Plain` (SDLS verified, or no SDLS), and an engine's frame input SHALL take only `Plain`. An operation out of order SHALL not compile (RS-021). In the codecs, `Frame<S>` is each codec's writer, `TmFrameWriter<'b, S>` and so on, with the states of `aldis_core::finishing`. `protect` takes a `SecurityHook`, whose mandatory `layout()` it compares with the channel's Security Header and Trailer lengths (API-073). In the codecs, the receiving states are types, not marker parameters (API-015, API-016): the view that a decode returns is `Checked` when the decoder verified the FECF (the FHEC is corrected by the AOS decoder); a decode that skips the FECF returns a different type (`UncheckedTmFrame`) that becomes `Checked` only through the check; `validate` returns the refined `Validated…` type for a frame that also matches its channel's configuration. `Plain` is the SDLS step of the caller and of the engine work packages, which take the engine input. | API-074; 355.0 §2.3; 232.0 Cor. 1 §6.5.2 | I |

## 4. Module layout convention

Each codec crate uses the layout `src/{lib.rs, header.rs, view.rs, writer.rs, config.rs, anomaly.rs}`, plus protocol-specific modules (e.g. `clcw.rs`, `delimit.rs` in `aldis-tc`; `fhec.rs`, `mpdu.rs`, `bpdu.rs` in `aldis-aos`; `tfdf.rs`, `truncated.rs` in `aldis-uslp`).

## 5. Lint configuration

The workspace `Cargo.toml` defines `[workspace.lints]`, and every crate inherits them (`[lints] workspace = true`). `aldis-ffi` repeats the same table with `unsafe_code = "deny"` (GEN-071).

The Clippy `all` and `pedantic` groups are denied at the lowest priority (`-1`). The individual restriction lints and selected nursery lints listed after them refine those groups.

```toml
[workspace.lints.rust]
unsafe_code = "forbid"          # "deny" in aldis-ffi only, with SAFETY comments
missing_docs = "deny"
unreachable_pub = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
# Selected restriction lints (GEN-010, GEN-020, GEN-030, GEN-070)
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "deny"
arithmetic_side_effects = "deny"
cast_possible_truncation = "deny"
as_conversions = "deny"
unreachable = "deny"
todo = "deny"
unimplemented = "deny"
undocumented_unsafe_blocks = "deny"
alloc_instead_of_core = "deny"
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
string_slice = "deny"
infinite_loop = "deny"
mem_forget = "deny"
as_pointer_underscore = "deny"
lossy_float_literal = "deny"
# Selected nursery lints (RS-019). The nursery group itself stays off.
missing_const_for_fn = "deny"
derive_partial_eq_without_eq = "deny"
too_long_first_doc_paragraph = "deny"
redundant_clone = "deny"
fallible_impl_from = "deny"
non_send_fields_in_send_ty = "deny"
transmute_undefined_repr = "deny"
debug_assert_with_mut_call = "deny"
needless_pass_by_ref_mut = "deny"
large_stack_frames = "deny"
collection_is_never_read = "deny"
```

The Clippy `nursery` group holds lints that are still under development and may report false positives, so it is not enabled as a group. Individual nursery lints are adopted once they have been checked against this code base (RS-019):

| Nursery lint | Decision | Reason |
|---|---|---|
| `missing_const_for_fn` | Denied | Extends the `const` constructors of RS-010 and RS-012: functions are `const fn` wherever the language allows. If a function can be `const` only for some feature combinations, it stays non-`const` and carries a `cfg_attr`-gated `#[expect]`, because constness must not depend on a feature. |
| `derive_partial_eq_without_eq` | Denied | Types with total equality derive `Eq`, so callers can use them as keys and in `assert_eq!` of containing types. |
| `too_long_first_doc_paragraph` | Denied | rustdoc shows the first paragraph as the item summary, so it is one short sentence. |
| `redundant_clone` | Denied | A clone whose original is never used again is dead work. |
| `fallible_impl_from` | Denied | `From` must be infallible (RS-010, GEN-020). Fallible conversions implement `TryFrom`. |
| `non_send_fields_in_send_ty` | Denied | Prevents manual `Send` implementations on types whose fields are not `Send` (RS-013, GEN-033). |
| `transmute_undefined_repr` | Denied | Transmuting types without defined representation (`#[repr(C)]`) is immediate undefined behavior (GEN-070, CABI-004). |
| `debug_assert_with_mut_call` | Denied | Mutating calls inside `debug_assert!` diverge between debug and release builds, violating determinism (GEN-032). |
| `needless_pass_by_ref_mut` | Denied | Avoids unnecessary `&mut` parameters that restrict caller concurrency without mutating state. |
| `large_stack_frames` | Denied | Embedded bare-metal targets (Cortex-M, SPARC LEON3) have strictly bounded stack sizes (GEN-011, VER-010). |
| `collection_is_never_read` | Denied | Containers populated but never read waste storage and cycles (GEN-011). |
| `redundant_pub_crate` | Not adopted | Conflicts with `unreachable_pub`: in a private module it asks for `pub`, which `unreachable_pub` then reports. |
| `or_fun_call` | Not adopted | The arguments it flags are `const fn` error constructors with no side effects that return a small `Copy` value (RS-011). Eager construction costs nothing, and a closure adds a coverage region (VER-024). |
| `option_if_let_else` | Not adopted | The suggested `map_or_else` takes closures, which a `const fn` cannot call, so it conflicts with `missing_const_for_fn`. Elsewhere it does not improve readability. |
| `use_self` | Deferred | Style only. The code mostly names the type rather than writing `Self`. Adopting it is a separate, mechanical change. |
| `suspicious_operation_groupings` | Not adopted | False positives on contiguous interval validation chains. |
| `trailing_empty_array` | Not adopted | Flags zero-sized C ABI storage handle types (`aldis_oid_generator_t`). |
| `useless_let_if_seq` | Not adopted | Flags multi-stage conditional state machine parsing in `aldis-spp`. |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| RS-016 | A lint SHALL be relaxed only locally, on the smallest item, with `#[expect(clippy::<lint>, reason = "<justification>")]`. Crate-wide or workspace-wide `allow`s of `all`, `pedantic`, the restriction lints, or the selected nursery lints above SHALL NOT be added without updating this section. `#[allow]` SHALL NOT be used for these lints, because `#[expect]` fails when the exception becomes unnecessary. | VER-001 | A |
| RS-017 | **Tooling crates** (`aldis-testkit`, `xtask`, and future test/fuzz support crates) are `std`-only, `publish = false`, and never part of a flight build. They SHALL use the *tooling lint profile*: `unsafe_code = "forbid"` (`"deny"` in the differential-test bridge, which calls native implementations through the C ABI, and in `crosscheck/`, whose cFE bindings call C sources, GEN-071), `missing_docs = "deny"`, and Clippy `all` + `pedantic` + the selected nursery lints (RS-019) denied, without the flight-code restriction lints (tools and tests fail loudly by design). Library crates SHALL use tooling crates only as dev-dependencies (checked by `cargo xtask layers`). | GEN-001 | A |
| RS-018 | **Test code** may fail loudly. `clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules from `unwrap_used`, `expect_used`, `panic`, and `indexing_slicing`. Test modules that need integer arithmetic, and integration-test crates whose helper functions sit outside `#[test]`, SHALL use a module- or crate-level `#[expect(…, reason = "… (RS-018)")]`. Library code remains subject to every lint (GEN-020). | GEN-020 | A |
| RS-019 | The Clippy `nursery` group SHALL NOT be enabled as a group. Each nursery lint marked *Denied* in the table above SHALL be denied in every lint table: the workspace, `aldis-ffi`, the exerciser, and the tooling crates. A nursery lint SHALL be adopted only after its findings on this code base have been checked to be correct, and this section SHALL record each one considered and the decision. When the pinned toolchain changes (RS-001), the workspace SHALL be checked once with `-W clippy::nursery` to review newly matured lints. | VER-001 | A |
