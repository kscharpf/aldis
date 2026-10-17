# Rust profile

This profile binds the language-neutral specification to Rust. The Rust crates are the **reference implementation**.

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
| `aldis-cop1` | `tc` | `uslp`, `std` |
| `aldis-framework` | — | `std`, `std-udp`, `std-tcp`, `embedded-nal`, `trace`, `async` |
| `aldis` (facade) | — | one feature per crate, plus `full` |
| `aldis-ffi` | — | mirrors the facade; capacities via build-time config ([C ABI §5](c-abi.md#5-memory-model)) |

## 3. API shapes

```rust
// Views borrow input; decode never panics (GEN-020).
pub struct SpacePacket<'a> { /* … */ }
impl<'a> SpacePacket<'a> {
    pub fn decode(bytes: &'a [u8], mode: DecodeMode) -> Result<Self, Error>;
    pub fn peek_length(prefix: &[u8]) -> Result<usize, Error>;
    pub fn apid(&self) -> Apid;
    pub fn data_field(&self) -> &'a [u8];
    pub fn anomalies(&self) -> AnomalySet<SppAnomaly>;
    pub fn len(&self) -> usize;
    pub fn header(&self) -> SpacePacketHeader;          // owned (API-014)
}

// Writers write into caller buffers.
impl SpacePacketHeader {
    pub fn encode(&self, data: &[u8], out: &mut [u8]) -> Result<usize, Error>;
}
pub struct SpacePacketWriter<'b> { /* two-phase, API-020..023 */ }

// Fixed-length frames require configuration.
impl<'a> TmFrame<'a> {
    pub fn decode(bytes: &'a [u8], pc: &TmPhysicalChannelConfig, mode: DecodeMode) -> Result<Self, Error>;
}

// Engines: capacities are const generics (GEN-011).
pub struct Fop1<const SENT: usize, const MAX_FRAME: usize> { /* … */ }
impl<const SENT: usize, const MAX_FRAME: usize> Fop1<SENT, MAX_FRAME> {
    pub fn new(cfg: Fop1Config) -> Result<Self, Error>;
    pub fn request_ad(&mut self, now: Instant, fdu: &[u8]) -> Result<RequestHandle, Error>;
    pub fn handle_clcw(&mut self, now: Instant, clcw: Clcw) -> Result<(), Error>;
    pub fn handle_timeout(&mut self, now: Instant);
    pub fn poll_transmit(&mut self, out: &mut [u8]) -> Result<Option<Transmit>, Error>;
    pub fn poll_event(&mut self) -> Option<Event>;
    pub fn poll_timeout(&self) -> Option<Instant>;
    pub fn transmit_done(&mut self, h: TransmitHandle, r: LowerResponse) -> Result<(), Error>;
    pub fn status(&self) -> Fop1Status;
}
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| RS-010 | Identifier types (API-001) SHALL be newtypes constructed only through validating functions (`TryFrom<u16/u32>`, `const fn new(..) -> Option<Self>`). The public API SHALL NOT provide unchecked constructors. | API-001 | T |
| RS-011 | `Error` SHALL be a small `Copy` struct `{ kind: ErrorKind, field: Option<FieldId>, detail: ErrorDetail }` with `#[non_exhaustive]` enums (GEN-041, GEN-081). With `std`, it SHALL implement `std::error::Error`. With `core`, it SHALL implement `core::error::Error` (Rust ≥ 1.81). | GEN-040 | T |
| RS-012 | Configuration types SHALL have private fields, validated constructors or builders, and `Clone + PartialEq + Debug`. They SHALL be constructible in `const` context where the validation is `const`-evaluable. | GEN-060, API-041 | T |
| RS-013 | Engines SHALL implement `Send` when their contents do. They SHALL NOT implement `Sync`-dependent interior mutability (GEN-033). | GEN-033 | A |
| RS-014 | Security hooks SHALL be generic type parameters (`Fop1<…, H: SecurityHook = NoSecurity>`) so that the no-security path compiles to zero overhead. | API-073 | A |
| RS-015 | Public items SHALL be documented. Each item's documentation SHALL cite the CCSDS clause(s) and requirement IDs it implements. `#![deny(missing_docs)]` SHALL be set. | VER-020 | A |

## 4. Module layout convention

Each codec crate uses the layout `src/{lib.rs, header.rs, view.rs, writer.rs, config.rs, anomaly.rs}`, plus protocol-specific modules (e.g. `clcw.rs`, `delimit.rs` in `aldis-tc`; `fhec.rs`, `mpdu.rs`, `bpdu.rs` in `aldis-aos`; `tfdf.rs`, `truncated.rs` in `aldis-uslp`).

## 5. Lint configuration

The workspace `Cargo.toml` defines `[workspace.lints]`, and every crate inherits them (`[lints] workspace = true`). `aldis-ffi` repeats the same table with `unsafe_code = "deny"` (GEN-071).

The Clippy `all` and `pedantic` groups are denied at the lowest priority (`-1`). The individual restriction lints listed after them refine those groups.

```toml
[workspace.lints.rust]
unsafe_code = "forbid"          # "deny" in aldis-ffi only, with SAFETY comments
missing_docs = "deny"
unreachable_pub = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
# Selected restriction lints (GEN-020)
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "deny"
arithmetic_side_effects = "deny"
cast_possible_truncation = "deny"
unreachable = "deny"
todo = "deny"
unimplemented = "deny"
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| RS-016 | A lint SHALL be relaxed only locally, on the smallest item, with `#[expect(clippy::<lint>, reason = "<justification>")]`. Crate-wide or workspace-wide `allow`s of `all`, `pedantic`, or the restriction lints above SHALL NOT be added without updating this section. `#[allow]` SHALL NOT be used for these lints, because `#[expect]` fails when the exception becomes unnecessary. | VER-001 | A |
