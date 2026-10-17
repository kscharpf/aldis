//! C ABI for Aldis.
//!
//! Specification: `docs/profiles/c-abi.md`. The public header `include/aldis/aldis.h` is
//! generated with cbindgen (CABI-002) and capacities come from `capacities.toml` (CABI-030).
//!
//! **Status:** skeleton. No functions are exported yet.
#![cfg_attr(not(feature = "std"), no_std)]

// Keeps the facade linked until the first exported functions use it.
use aldis as _;

/// ABI version reported by `aldis_abi_version()` (CABI-060).
pub const ALDIS_ABI_VERSION_MAJOR: u32 = 0;
/// ABI minor version (CABI-060).
pub const ALDIS_ABI_VERSION_MINOR: u32 = 1;
/// ABI patch version (CABI-060).
pub const ALDIS_ABI_VERSION_PATCH: u32 = 0;

#[cfg(all(not(feature = "std"), feature = "panic-handler"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
