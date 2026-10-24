//! C ABI for Aldis (docs/profiles/c-abi.md; M2 plan D4, D5).
//!
//! The functions of the M1 codecs: `aldis-spp` ([`spp`]), its stateful helpers in caller-owned
//! storage ([`spp_state`]), `aldis-time` ([`time`]), and `aldis-epp` ([`epp`]); and the parts of
//! `aldis-core` the frame codecs share (M2 plan D14): the CRC-16 ([`crc`]), the Operational
//! Control Field ([`ocf`]), and the OID generator in caller-owned storage ([`oid`]); and the
//! frame codec `aldis-tm` ([`tm`]; M2 plan WP-2 module 7, D14); and the frame codec
//! `aldis-uslp` ([`uslp`]; M2 plan WP-5 module 9, D14). The header
//! `include/aldis/aldis.h` is generated from these sources by the pinned `cbindgen`
//! (`cargo xtask header --write`; CABI-002, CABI-003), and build-time capacities come from
//! `capacities.toml` ([`capacities`], CABI-030).
//!
//! **Conventions** (c-abi.md §3–5):
//! - Every fallible function returns `int32_t`: [`ALDIS_OK`] or a negative `ALDIS_ERR_*` code
//!   of the error catalogue (CABI-020), and takes an optional [`aldis_error_detail_t`] as its
//!   last parameter (CABI-021).
//! - Every pointer and length is checked (CABI-022, [`abi`]): `NULL` is allowed only for an
//!   empty buffer and for the detail; a violation is `ALDIS_ERR_INVALID_FIELD` naming no
//!   field.
//! - Buffers are `(const uint8_t *, size_t)` inputs and `(uint8_t *, size_t cap, size_t
//!   *out_len)` outputs (CABI-011); views are POD structs with offset/length pairs into the
//!   input (CABI-012); configuration structs start with `uint32_t struct_size` (CABI-033).
//! - The decode mode is a `uint32_t` (`ALDIS_MODE_STRICT`, `ALDIS_MODE_LENIENT`), and
//!   anomalies a `uint32_t` mask whose bit `n` is the flag with the `Anomaly` index `n`.
//! - No call keeps a pointer it is given (API-060); the stateful helpers keep their state in
//!   storage the caller owns (CABI-031, CABI-032).
//!
//! `no_std` without `std`, no heap, no threads (CABI-004). This is the only library crate with
//! `unsafe` code: each entry point and each pointer check carries `#[expect(unsafe_code)]`
//! and every `unsafe` block a `// SAFETY:` comment (GEN-070, GEN-071).
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod abi;
pub mod capacities;
pub mod crc;
pub mod epp;
pub mod error;
pub mod field;
pub mod handle;
pub mod limits;
pub mod ocf;
pub mod oid;
pub mod spp;
pub mod spp_state;
pub mod time;
pub mod tm;
// ---- USLP (M2 WP-5 module 9)
pub mod uslp;
// ---- end USLP

pub use abi::{ALDIS_MODE_LENIENT, ALDIS_MODE_STRICT};
pub use error::{ALDIS_OK, aldis_error_detail_t};

/// ABI major version; changes on any ABI-breaking change (CABI-060).
pub const ALDIS_ABI_VERSION_MAJOR: u32 = 0;
/// ABI minor version (CABI-060).
pub const ALDIS_ABI_VERSION_MINOR: u32 = 1;
/// ABI patch version (CABI-060).
pub const ALDIS_ABI_VERSION_PATCH: u32 = 0;

#[cfg(all(
    not(feature = "std"),
    feature = "panic-handler",
    feature = "panic-unreachable"
))]
compile_error!("features `panic-handler` and `panic-unreachable` each define the panic handler");

/// Satisfies the linker of a `no_std` static library. No panic path is reachable (GEN-020,
/// VER-010), so it never runs.
#[cfg(all(
    not(feature = "std"),
    feature = "panic-handler",
    not(feature = "panic-unreachable")
))]
#[panic_handler]
#[expect(
    clippy::missing_const_for_fn,
    reason = "a #[panic_handler] cannot be a const fn"
)]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

/// The panic handler of the exerciser's link check (VER-010, M2 plan D16): it calls
/// `aldis_panic_is_unreachable`, which is defined nowhere, so a final link succeeds only if no
/// panic path is reachable.
#[cfg(all(not(feature = "std"), feature = "panic-unreachable"))]
#[panic_handler]
#[expect(
    unsafe_code,
    reason = "declares the deliberately undefined symbol of the link check"
)]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    // SAFETY: never called in a successful link: the symbol does not exist, so a reachable
    // call fails the link (VER-010).
    unsafe extern "C" {
        /// Defined nowhere: any reachable panic makes the final link fail (VER-010).
        safe fn aldis_panic_is_unreachable() -> !;
    }
    aldis_panic_is_unreachable()
}
