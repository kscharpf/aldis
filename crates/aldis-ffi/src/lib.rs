//! C ABI for Aldis (docs/profiles/c-abi.md; M2 plan D4, D5).
//!
//! The functions of the M1 codecs: `aldis-spp` ([`spp`]), its stateful helpers in caller-owned
//! storage ([`spp_state`]), `aldis-time` ([`time`]), and `aldis-epp` ([`epp`]); and the parts of
//! `aldis-core` the frame codecs share (M2 plan D14): the CRC-16 ([`crc`]), the Operational
//! Control Field ([`ocf`]), and the OID generator in caller-owned storage ([`oid`]); and the
//! frame codec `aldis-tm` ([`tm`]; M2 plan WP-2 module 7, D14); the frame codec `aldis-aos`
//! ([`aos`]; WP-4 module 8, D14); and the frame codec `aldis-uslp` ([`uslp`]; WP-5 module 9,
//! D14). The header
//! `include/aldis/aldis.h` is generated from these sources by the pinned `cbindgen`
//! (`cargo xtask header --write`; CABI-002, CABI-003), and build-time capacities come from
//! `capacities.toml` ([`capacities`], CABI-030).
//!
//! **Conventions** (c-abi.md §3–5):
//! - Every fallible function returns a `uint32_t`: [`ALDIS_OK`] or an `ALDIS_ERR_*` code
//!   of the error catalogue (CABI-020), and takes an optional [`aldis_error_detail_t`] as its
//!   last parameter (CABI-021).
//! - Every pointer and length is checked (CABI-022, [`abi`]): `NULL` is allowed only for an
//!   empty buffer and for the detail; a violation is `ALDIS_ERR_INVALID_FIELD` naming no
//!   field.
//! - Buffers are `(const uint8_t *, size_t)` inputs and `(uint8_t *, size_t cap, size_t
//!   *out_len)` outputs (CABI-011); views are POD structs with `aldis_range_t` offset/length
//!   ranges into the input (CABI-012).
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
pub mod aos;
pub mod capacities;
pub mod crc;
pub mod epp;
pub mod error;
pub mod field;
pub mod handle;
mod mirror;
pub mod ocf;
pub mod oid;
pub mod range;
pub mod spp;
pub mod spp_state;
pub mod status;
pub mod tc;
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
pub const ALDIS_ABI_VERSION_MINOR: u32 = 6;
/// ABI patch version (CABI-060).
pub const ALDIS_ABI_VERSION_PATCH: u32 = 0;

#[cfg(all(
    not(feature = "std"),
    feature = "panic-handler",
    feature = "panic-unreachable"
))]
compile_error!("features `panic-handler` and `panic-unreachable` each define the panic handler");

/// The panic handler of a `no_std` static library (OI-49; c-abi.md §5). No panic path is
/// reachable (GEN-020, VER-010), so it never runs. If one runs anyway, it traps at once
/// ([`trap`]) instead of spinning, so that the target's fault handler, a debugger, or a
/// watchdog sees the failure where it happened.
#[cfg(all(
    not(feature = "std"),
    feature = "panic-handler",
    not(feature = "panic-unreachable")
))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    trap();
    // Reached only if the fault handler returns, or on a target with no trap instruction here.
    // bound: none, by intent. This is the panic handler of a firmware build, after the trap; no
    // API call reaches it (GEN-020, GEN-021), and an endless halt is the safe state.
    loop {}
}

/// Executes the permanently undefined instruction of the target (OI-49): `udf` on Arm
/// (Cortex-M and Cortex-R included), `brk` on `AArch64`, `ebreak` on `RISC-V`, and `ud2` on x86.
/// `core::intrinsics::abort` is unstable, and compiles to the same on these targets.
#[cfg(all(
    not(feature = "std"),
    feature = "panic-handler",
    not(feature = "panic-unreachable"),
    any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv32",
        target_arch = "riscv64",
        target_arch = "x86",
        target_arch = "x86_64"
    )
))]
#[expect(
    unsafe_code,
    reason = "inline assembly of the trapping instruction (GEN-070; OI-49)"
)]
fn trap() {
    // SAFETY: each instruction reads and writes no memory and uses no stack; it only raises an
    // exception, which the target's fault handler, if any, takes.
    unsafe {
        #[cfg(target_arch = "arm")]
        core::arch::asm!("udf #0", options(nomem, nostack));
        #[cfg(target_arch = "aarch64")]
        core::arch::asm!("brk #0", options(nomem, nostack));
        #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
        core::arch::asm!("ebreak", options(nomem, nostack));
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        core::arch::asm!("ud2", options(nomem, nostack));
    }
}

/// A target without a stable `core::arch::asm!` (for example SPARC V8, the LEON3 build) has no
/// trap here: the handler spins, and only a watchdog ends it (OI-49).
#[cfg(all(
    not(feature = "std"),
    feature = "panic-handler",
    not(feature = "panic-unreachable"),
    not(any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv32",
        target_arch = "riscv64",
        target_arch = "x86",
        target_arch = "x86_64"
    ))
))]
fn trap() {
    core::hint::spin_loop();
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
