//! Bare-metal exerciser for Aldis (docs/milestones/m0-foundations.md WP-5,
//! docs/milestones/m1-packets-time.md WP-5, D6, D7).
//!
//! **Panic-freedom proof (VER-010, GEN-021):** the panic handler calls
//! `aldis_panic_is_unreachable`, a symbol that is defined nowhere. A release build (LTO,
//! `panic = "abort"`) links only if no panic path survives optimization in any code the
//! exerciser reaches, which is every public API of `aldis-core`, `aldis-time`, `aldis-spp`
//! (with `time`), and `aldis-epp`, and every function `aldis-ffi` exports (M2 plan D16). The
//! handler is `aldis-ffi`'s, with its feature `panic-unreachable`, because `aldis-ffi` is also
//! built as a static library and so needs one of its own. Feature `inject-panic` plants a
//! reachable panic to show that the check fails when it should.
//!
//! **On-target run (M1 D6):** on every bare-metal target the exerciser also checks every
//! embedded vector case and reports the result through semihosting: one line with the case
//! count, then `SYS_EXIT` with success only if no case failed. `cargo run --release --target <t>`
//! runs it under QEMU (`exerciser/.cargo/config.toml`).
#![no_std]
#![no_main]

mod cases;
mod epp;
mod exercise;
mod ffi;
mod spp;
mod target;
mod time;
mod vectors;

/// The Rust entry point: the reset handler on Cortex-M, called by the start-up code of
/// `target.rs` on Cortex-R and RISC-V once the stack is set.
#[expect(unsafe_code, reason = "the entry symbol must not be mangled")]
// SAFETY: `aldis_exerciser_main` is the only definition of this symbol in the image.
#[unsafe(no_mangle)]
pub extern "C" fn aldis_exerciser_main() -> ! {
    let tally = exercise::run();
    target::report(tally.cases, tally.failures, tally.not_representable);
    target::exit(tally.failures == 0 && tally.cases > 0)
}

/// Every unexpected exception or trap lands here (vector tables in `target.rs`), so a crash
/// exits with failure instead of hanging the run.
#[expect(
    unsafe_code,
    reason = "the fault symbol is referenced from the vector tables"
)]
// SAFETY: `aldis_exerciser_fault` is the only definition of this symbol in the image.
#[unsafe(no_mangle)]
pub extern "C" fn aldis_exerciser_fault() -> ! {
    target::exit(false)
}
