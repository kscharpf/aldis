//! Bare-metal exerciser for Aldis (docs/milestones/m0-foundations.md WP-5).
//!
//! **Panic-freedom proof (VER-010, GEN-021):** the panic handler calls
//! `aldis_panic_is_unreachable`, a symbol that is defined nowhere. A release build (LTO,
//! `panic = "abort"`) links only if no panic path survives optimization in any code the
//! exerciser reaches, which is every public `aldis-core` API. Feature `inject-panic` plants a
//! reachable panic to show that the check fails when it should.
//!
//! **On-target run:** on Cortex-M the exerciser also checks every core vector case and reports
//! the result through semihosting: `cargo run --release --target thumbv7em-none-eabihf` (QEMU).
#![no_std]
#![no_main]

mod cases;
mod exercise;

/// Entry point (reset handler on Cortex-M).
#[expect(unsafe_code, reason = "the entry symbol must not be mangled")]
// SAFETY: `_start` is the only definition of this symbol in the image.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let tally = exercise::run();
    exit(tally.failures == 0 && tally.cases > 0)
}

#[cfg(target_arch = "arm")]
fn exit(success: bool) -> ! {
    // ARM semihosting SYS_EXIT (0x18): ADP_Stopped_ApplicationExit or RunTimeErrorUnknown.
    let reason: u32 = if success { 0x2_0026 } else { 0x2_0023 };
    #[expect(unsafe_code, reason = "semihosting call")]
    // SAFETY: `bkpt 0xAB` with r0 = SYS_EXIT is the semihosting exit call; it touches no memory.
    unsafe {
        core::arch::asm!("bkpt 0xAB", in("r0") 0x18_u32, in("r1") reason, options(nomem, nostack));
    }
    halt()
}

#[cfg(not(target_arch = "arm"))]
fn exit(_success: bool) -> ! {
    halt()
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Cortex-M exception vectors after the initial stack pointer: reset, then NMI … `SysTick`.
/// Every fault exits with failure, so a crash cannot hang the run.
#[cfg(cortex_m)]
#[expect(
    unsafe_code,
    reason = "the vector table must be placed in its linker section"
)]
// SAFETY: the section is placed at the vector table address by link-cortex-m.x.
#[unsafe(link_section = ".vector_table.exceptions")]
#[used]
static EXCEPTIONS: [extern "C" fn() -> !; 15] = [
    _start, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault,
    fault, fault,
];

#[cfg(cortex_m)]
extern "C" fn fault() -> ! {
    exit(false)
}

#[panic_handler]
#[expect(
    unsafe_code,
    reason = "declares the deliberately undefined symbol of the link check"
)]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    // SAFETY: never called in a successful build: the symbol does not exist, so a reachable
    // call fails the link.
    unsafe extern "C" {
        /// Defined nowhere: any reachable panic makes the final link fail (VER-010).
        safe fn aldis_panic_is_unreachable() -> !;
    }
    aldis_panic_is_unreachable()
}
