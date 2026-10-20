//! Start-up, fault vectors, and semihosting for the three bare-metal targets (M1 D6).
//!
//! Each target module provides `call(op, arg)`, the semihosting trap of its architecture (the
//! Arm semihosting specification; RISC-V reuses its operations with its own trap sequence),
//! plus the start-up code that QEMU enters:
//!
//! | Target | QEMU machine | Entry | Trap |
//! |---|---|---|---|
//! | `thumbv7em-none-eabihf` | `mps2-an386` (Cortex-M4) | vector table, reset = [`aldis_exerciser_main`](crate::aldis_exerciser_main) | `bkpt 0xAB` |
//! | `armv7r-none-eabihf` | `none` with `-cpu cortex-r5f`; also `mps3-an536` (Cortex-R52, QEMU ≥ 9.0) | `_start`: stack, VFP on | `svc 0x123456` (A32) |
//! | `riscv32imac-unknown-none-elf` | `virt` (RV32) | `_start`: stack, `mtvec` | `slli`/`ebreak`/`srai` |
//!
//! QEMU loads the ELF and zero-fills memory, so no copy or clear loop is needed.

/// `SYS_WRITE0`: write a NUL-terminated string to the debug console.
const SYS_WRITE0: usize = 0x04;
/// `SYS_EXIT`: on a 32-bit target the argument is the reason code itself.
const SYS_EXIT: usize = 0x18;
/// `ADP_Stopped_ApplicationExit`: QEMU exits with status 0.
const APPLICATION_EXIT: usize = 0x2_0026;
/// `ADP_Stopped_RunTimeErrorUnknown`: QEMU exits with status 1.
const RUNTIME_ERROR: usize = 0x2_0023;

/// Writes `aldis-exerciser: <cases> cases, <failures> failed` to the semihosting console.
pub fn report(cases: u32, failures: u32) {
    let mut line = Line {
        buf: [0; 64],
        len: 0,
    };
    line.push(b"aldis-exerciser: ");
    line.number(cases);
    line.push(b" cases, ");
    line.number(failures);
    line.push(b" failed\n");
    // The buffer is zero-initialized and never filled to its last octet, so the text is
    // NUL-terminated as SYS_WRITE0 requires.
    if line.len < line.buf.len() {
        let _ = arch::call(SYS_WRITE0, line.buf.as_ptr().addr());
    }
}

/// Ends the run through semihosting `SYS_EXIT`; QEMU's exit status is 0 only on `success`.
pub fn exit(success: bool) -> ! {
    let reason = if success {
        APPLICATION_EXIT
    } else {
        RUNTIME_ERROR
    };
    let _ = arch::call(SYS_EXIT, reason);
    // Only reached without a semihosting host (real hardware, no debugger).
    loop {
        core::hint::spin_loop();
    }
}

/// A fixed-size text buffer (no allocation, no `core::fmt`).
struct Line {
    buf: [u8; 64],
    len: usize,
}

impl Line {
    /// Appends `text`, keeping the last octet free for the terminating NUL.
    fn push(&mut self, text: &[u8]) {
        let Some(end) = self.len.checked_add(text.len()) else {
            return;
        };
        if end >= self.buf.len() {
            return;
        }
        if let Some(dst) = self.buf.get_mut(self.len..end) {
            dst.copy_from_slice(text);
            self.len = end;
        }
    }

    /// Appends `n` in decimal.
    fn number(&mut self, mut n: u32) {
        let mut digits = [0_u8; 10];
        let mut start = digits.len();
        for slot in digits.iter_mut().rev() {
            *slot = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or_default());
            start = start.saturating_sub(1);
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.push(digits.get(start..).unwrap_or_default());
    }
}

#[cfg(cortex_m)]
use cortex_m as arch;
#[cfg(cortex_r)]
use cortex_r as arch;
#[cfg(riscv)]
use riscv as arch;

/// Cortex-M4 on QEMU `mps2-an386`: the hardware loads the stack pointer from the vector table.
#[cfg(cortex_m)]
mod cortex_m {
    use crate::{aldis_exerciser_fault, aldis_exerciser_main};

    /// Exception vectors after the initial stack pointer: reset, then NMI … `SysTick`.
    #[expect(
        unsafe_code,
        reason = "the vector table must be placed in its linker section"
    )]
    // SAFETY: the section is placed at the vector table address by link-cortex-m.x.
    #[unsafe(link_section = ".vector_table.exceptions")]
    #[used]
    static EXCEPTIONS: [extern "C" fn() -> !; 15] = [
        aldis_exerciser_main,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
        aldis_exerciser_fault,
    ];

    /// Semihosting call: `bkpt 0xAB` with the operation in r0 and its argument in r1.
    pub fn call(op: usize, arg: usize) -> usize {
        let ret: usize;
        #[expect(unsafe_code, reason = "semihosting call")]
        // SAFETY: the semihosting host reads at most the NUL-terminated buffer `arg` points to
        // (SYS_WRITE0) and writes only r0.
        unsafe {
            core::arch::asm!(
                "bkpt 0xAB",
                inout("r0") op => ret,
                in("r1") arg,
                options(nostack, preserves_flags),
            );
        }
        ret
    }
}

/// Cortex-R in A32 state. QEMU starts the image at `_start` (ELF entry); the vector table at
/// address 0 sends every exception to `aldis_exerciser_fault` with a fresh stack. It serves
/// both the Cortex-R5F (low vectors, SCTLR.V = 0) and the Cortex-R52, which starts in Hyp
/// mode with HVBAR = 0.
#[cfg(cortex_r)]
mod cortex_r {
    #[expect(unsafe_code, reason = "start-up code and vector table")]
    mod startup {
        // SAFETY: runs once from reset with nothing else live; it sets the stack to the top of
        // RAM (link-cortex-r.x), enables the VFP, and never returns from the Rust entry point.
        core::arch::global_asm!(
            ".section .vector_table, \"ax\", %progbits",
            ".arm",
            "aldis_vectors:",
            "  ldr pc, =_start",
            "  ldr pc, =aldis_fault_entry",
            "  ldr pc, =aldis_fault_entry",
            "  ldr pc, =aldis_fault_entry",
            "  ldr pc, =aldis_fault_entry",
            "  ldr pc, =aldis_fault_entry",
            "  ldr pc, =aldis_fault_entry",
            "  ldr pc, =aldis_fault_entry",
            "  .ltorg",
            ".section .text._start, \"ax\", %progbits",
            ".global _start",
            ".arm",
            "_start:",
            "  ldr sp, =_stack_top",
            // CPACR: full access to cp10 and cp11 (VFP), then FPEXC.EN: the hard-float ABI may
            // use VFP registers.
            "  mrc p15, 0, r0, c1, c0, 2",
            "  orr r0, r0, #0xF00000",
            "  mcr p15, 0, r0, c1, c0, 2",
            "  isb",
            "  mov r0, #0x40000000",
            "  vmsr fpexc, r0",
            "  bl aldis_exerciser_main",
            "  b .",
            "aldis_fault_entry:",
            "  ldr sp, =_stack_top",
            "  b aldis_exerciser_fault",
        );
    }

    /// Semihosting call: `svc 0x123456` (A32) with the operation in r0 and its argument in r1.
    pub fn call(op: usize, arg: usize) -> usize {
        let ret: usize;
        #[expect(unsafe_code, reason = "semihosting call")]
        // SAFETY: the semihosting host reads at most the NUL-terminated buffer `arg` points to
        // (SYS_WRITE0) and writes only r0.
        unsafe {
            core::arch::asm!(
                "svc 0x123456",
                inout("r0") op => ret,
                in("r1") arg,
                options(nostack, preserves_flags),
            );
        }
        ret
    }
}

/// RV32 on QEMU `virt` (machine mode, `-bios none`): `_start` sets the stack and points `mtvec`
/// at a handler that exits with failure.
#[cfg(riscv)]
mod riscv {
    #[expect(unsafe_code, reason = "start-up code and trap vector")]
    mod startup {
        // SAFETY: runs once from reset with nothing else live; it sets the stack to the top of
        // RAM (link-riscv.x), installs the trap vector, and never returns from the Rust entry point.
        core::arch::global_asm!(
            ".section .text._start, \"ax\", @progbits",
            ".global _start",
            "_start:",
            "  la sp, _stack_top",
            "  la t0, aldis_trap",
            "  csrw mtvec, t0",
            "  call aldis_exerciser_main",
            "  j .",
            ".p2align 2",
            "aldis_trap:",
            "  la sp, _stack_top",
            "  j aldis_exerciser_fault",
        );
    }

    /// Semihosting call: the uncompressed `slli`/`ebreak`/`srai` sequence, aligned so that it
    /// never crosses a page, with the operation in a0 and its argument in a1.
    pub fn call(op: usize, arg: usize) -> usize {
        let ret: usize;
        #[expect(unsafe_code, reason = "semihosting call")]
        // SAFETY: the semihosting host reads at most the NUL-terminated buffer `arg` points to
        // (SYS_WRITE0) and writes only a0.
        unsafe {
            core::arch::asm!(
                ".option push",
                ".option norvc",
                ".p2align 4",
                "slli zero, zero, 0x1f",
                "ebreak",
                "srai zero, zero, 0x7",
                ".option pop",
                inout("a0") op => ret,
                in("a1") arg,
                options(nostack, preserves_flags),
            );
        }
        ret
    }
}
