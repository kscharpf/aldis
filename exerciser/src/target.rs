//! Start-up, fault vectors, and console for the bare-metal targets (M1 D6; LEON3: OI-08).
//!
//! Each target module provides the start-up code that QEMU enters and the console: on the three
//! Arm and RISC-V targets `call(op, arg)`, the semihosting trap of its architecture (the Arm
//! semihosting specification; RISC-V reuses its operations with its own trap sequence); on
//! LEON3, which QEMU gives no semihosting, `write0` and `exit` of its own:
//!
//! | Target | QEMU machine | Entry | Console and exit |
//! |---|---|---|---|
//! | `thumbv7em-none-eabihf` | `mps2-an386` (Cortex-M4) | vector table, reset = [`aldis_exerciser_main`](crate::aldis_exerciser_main) | semihosting, `bkpt 0xAB` |
//! | `armv7r-none-eabihf` | `none` with `-cpu cortex-r5f`; also `mps3-an536` (Cortex-R52, QEMU ≥ 9.0) | `_start`: stack, VFP on | semihosting, `svc 0x123456` (A32) |
//! | `riscv32imac-unknown-none-elf` | `virt` (RV32) | `_start`: stack, `mtvec` | semihosting, `slli`/`ebreak`/`srai` |
//! | `sparc-unknown-none-elf` (`-C target-cpu=leon3`) | `leon3_generic` | `_start`: PSR, WIM, TBR, stack | APBUART; `ta` with traps disabled |
//!
//! QEMU loads the ELF and zero-fills memory, so no copy or clear loop is needed.

/// Writes `aldis-exerciser: <cases> cases, <failures> failed, <n> not representable` to
/// the console.
pub fn report(cases: u32, failures: u32, not_representable: u32) {
    let mut line = Line::new();
    line.push(b"aldis-exerciser: ");
    line.number(cases);
    line.push(b" cases, ");
    line.number(failures);
    line.push(b" failed, ");
    line.number(not_representable);
    line.push(b" not representable\n");
    line.write();
}

/// Writes `FAILED <id>` for a vector case that failed.
pub fn failed(id: &str) {
    let mut line = Line::new();
    line.push(b"FAILED ");
    line.push(id.as_bytes());
    line.push(b"\n");
    line.write();
}

/// Ends the run; QEMU's exit status is 0 only on `success`.
pub fn exit(success: bool) -> ! {
    console::exit(success)
}

/// A fixed-size text buffer (no allocation, no `core::fmt`).
struct Line {
    buf: [u8; 96],
    len: usize,
}

impl Line {
    const fn new() -> Line {
        Line {
            buf: [0; 96],
            len: 0,
        }
    }

    /// Writes the line. The buffer is zero-initialized and never filled to its last octet, so
    /// the text is NUL-terminated as `console::write0` requires.
    fn write(&self) {
        if self.len < self.buf.len() {
            console::write0(&self.buf);
        }
    }

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
#[cfg(sparc)]
use leon3 as console;
#[cfg(riscv)]
use riscv as arch;
#[cfg(any(cortex_m, cortex_r, riscv))]
use semihosting as console;

/// The console and exit of the Arm and RISC-V targets: semihosting through `arch::call`.
#[cfg(any(cortex_m, cortex_r, riscv))]
mod semihosting {
    use super::arch;

    /// `SYS_WRITE0`: write a NUL-terminated string to the debug console.
    const SYS_WRITE0: usize = 0x04;
    /// `SYS_EXIT`: on a 32-bit target the argument is the reason code itself.
    const SYS_EXIT: usize = 0x18;
    /// `ADP_Stopped_ApplicationExit`: QEMU exits with status 0.
    const APPLICATION_EXIT: usize = 0x2_0026;
    /// `ADP_Stopped_RunTimeErrorUnknown`: QEMU exits with status 1.
    const RUNTIME_ERROR: usize = 0x2_0023;

    /// Writes `text`, which holds a NUL, up to that NUL (`SYS_WRITE0`).
    pub fn write0(text: &[u8]) {
        let _ = arch::call(SYS_WRITE0, text.as_ptr().addr());
    }

    /// Ends the run through semihosting `SYS_EXIT`.
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
}

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

/// LEON3 (SPARC V8) on QEMU `leon3_generic` (non-gating, OI-08). QEMU has no semihosting for
/// SPARC: the console is the first APBUART, and the run ends with a trap taken while traps are
/// disabled, which puts a SPARC V8 processor in error mode. QEMU's LEON3 shuts down cleanly
/// (exit status 0) on `ta 0` in that state and aborts on any other trap (non-zero status;
/// QEMU 8.2 `target/sparc/int32_helper.c`, feature `TA0_SHUTDOWN`).
///
/// `_start` sets the PSR (supervisor, traps off, interrupts masked, FPU on, window 0), marks
/// window 1 invalid, points TBR at the trap table, sets the stack, and enables traps. The trap
/// table sends window overflow and underflow (trap types 5 and 6) to the usual spill and fill
/// handlers, which read the number of windows from ASR17 (LEON), and every other trap to
/// `aldis_exerciser_fault` with fresh window state and stack.
#[cfg(sparc)]
mod leon3 {
    /// APBUART data register: QEMU `hw/sparc/leon3.c` (`LEON3_UART_OFFSET`) and
    /// `hw/char/grlib_apbuart.c` (`DATA_OFFSET`).
    const UART_DATA: usize = 0x8000_0100;
    /// APBUART status register (`STATUS_OFFSET`).
    const UART_STATUS: usize = 0x8000_0104;
    /// Status bit TE: transmitter FIFO or holding register empty (`UART_TRANSMIT_FIFO_EMPTY`).
    const UART_TE: u32 = 1 << 2;

    /// Writes `text`, which holds a NUL, up to that NUL to the APBUART. QEMU's boot loader
    /// enables the transmitter when no PROM image is given (`write_bootloader`).
    pub fn write0(text: &[u8]) {
        let data = core::ptr::with_exposed_provenance_mut::<u32>(UART_DATA);
        let status = core::ptr::with_exposed_provenance::<u32>(UART_STATUS);
        for &octet in text.iter().take_while(|&&b| b != 0) {
            #[expect(unsafe_code, reason = "memory-mapped UART registers")]
            // SAFETY: both addresses are the APBUART's registers on `leon3_generic`; the
            // accesses are aligned 32-bit device accesses that touch no Rust object.
            unsafe {
                while core::ptr::read_volatile(status) & UART_TE == 0 {
                    core::hint::spin_loop();
                }
                core::ptr::write_volatile(data, u32::from(octet));
            }
        }
    }

    /// Ends the run: traps off, then `ta 0` (success) or `ta 1` (failure) puts the processor
    /// in error mode, which QEMU turns into its exit status.
    pub fn exit(success: bool) -> ! {
        let trap = u32::from(!success);
        #[expect(unsafe_code, reason = "error-mode exit")]
        // SAFETY: never returns; it only clears PSR.ET before the trap that halts the
        // processor, so no Rust state is observed afterwards.
        unsafe {
            core::arch::asm!(
                "rd %psr, %g1",
                "andn %g1, 0x20, %g1",
                "wr %g1, %g0, %psr",
                "nop",
                "nop",
                "nop",
                "ta %o0",
                "1: ba 1b",
                "nop",
                in("o0") trap,
                options(noreturn, nostack),
            );
        }
    }

    #[expect(unsafe_code, reason = "start-up code and trap table")]
    mod startup {
        /// PSR with traps disabled: EF (FPU on), PIL 15, S, PS; ET = 0, CWP = 0 (SPARC V8 §4.2).
        const PSR_TRAPS_OFF: u32 = 0x1FC0;
        /// The same with ET = 1.
        const PSR_TRAPS_ON: u32 = 0x1FE0;
        /// Window 1 invalid: with CWP = 0, the next six `save`s use windows 7 to 2.
        const WIM_START: u32 = 2;

        // SAFETY: `_start` runs once from reset with nothing else live: it sets the processor
        // state, the stack (top of RAM, link-leon3.x, with the 96-byte minimum frame), and the
        // trap table, and never returns from the Rust entry point. The window handlers run with
        // traps disabled in the invalid window, touch only its locals, %g1 (saved and restored),
        // WIM, and the 64-byte register save area of the window they spill or fill, which the
        // SPARC ABI reserves at that window's %sp. Every other trap restarts from a fresh stack
        // in `aldis_exerciser_fault`, which never returns.
        core::arch::global_asm!(
            ".section .text._start, \"ax\", @progbits",
            ".global _start",
            "_start:",
            "  set {psr_off}, %g1",
            "  wr %g1, %g0, %psr",
            "  nop",
            "  nop",
            "  nop",
            "  wr %g0, {wim}, %wim",
            "  set aldis_traps, %g1",
            "  wr %g1, %g0, %tbr",
            "  set _stack_top, %g1",
            "  sub %g1, 96, %sp",
            "  mov %g0, %fp",
            "  set {psr_on}, %g1",
            "  wr %g1, %g0, %psr",
            "  nop",
            "  nop",
            "  nop",
            "  call aldis_exerciser_main",
            "  nop",
            "1: ba 1b",
            "  nop",
            "aldis_fault_entry:",
            "  set {psr_off}, %g1",
            "  wr %g1, %g0, %psr",
            "  nop",
            "  nop",
            "  nop",
            "  wr %g0, {wim}, %wim",
            "  set _stack_top, %g1",
            "  sub %g1, 96, %sp",
            "  mov %g0, %fp",
            "  set {psr_on}, %g1",
            "  wr %g1, %g0, %psr",
            "  nop",
            "  nop",
            "  nop",
            "  call aldis_exerciser_fault",
            "  nop",
            "2: ba 2b",
            "  nop",
            // Window overflow: entered in the invalid window. Rotate WIM right by one, spill
            // the next window (the oldest valid one) to its stack, return to the `save`.
            "aldis_window_overflow:",
            "  rd %wim, %l3",
            "  rd %asr17, %l4",
            "  and %l4, 0x1f, %l4",
            "  mov %g1, %l7",
            "  srl %l3, 1, %g1",
            "  sll %l3, %l4, %l4",
            "  or %l4, %g1, %g1",
            "  save",
            "  wr %g1, %g0, %wim",
            "  nop",
            "  nop",
            "  nop",
            "  std %l0, [%sp + 0]",
            "  std %l2, [%sp + 8]",
            "  std %l4, [%sp + 16]",
            "  std %l6, [%sp + 24]",
            "  std %i0, [%sp + 32]",
            "  std %i2, [%sp + 40]",
            "  std %i4, [%sp + 48]",
            "  std %i6, [%sp + 56]",
            "  restore",
            "  mov %l7, %g1",
            "  jmp %l1",
            "  rett %l2",
            // Window underflow: entered one window below the `restore` that trapped. Rotate
            // WIM left by one, fill the window above that one from its stack, return.
            "aldis_window_underflow:",
            "  rd %wim, %l3",
            "  rd %asr17, %l4",
            "  and %l4, 0x1f, %l4",
            "  sll %l3, 1, %l5",
            "  srl %l3, %l4, %l4",
            "  or %l5, %l4, %l5",
            "  wr %l5, %g0, %wim",
            "  nop",
            "  nop",
            "  nop",
            "  restore",
            "  restore",
            "  ldd [%sp + 0], %l0",
            "  ldd [%sp + 8], %l2",
            "  ldd [%sp + 16], %l4",
            "  ldd [%sp + 24], %l6",
            "  ldd [%sp + 32], %i0",
            "  ldd [%sp + 40], %i2",
            "  ldd [%sp + 48], %i4",
            "  ldd [%sp + 56], %i6",
            "  save",
            "  save",
            "  jmp %l1",
            "  rett %l2",
            // Trap table: 256 entries of four instructions, 4 KiB aligned (TBR.TBA).
            ".section .text.aldis_traps, \"ax\", @progbits",
            ".balign 4096",
            "aldis_traps:",
            ".rept 5",
            "  ba aldis_fault_entry",
            "  nop",
            "  nop",
            "  nop",
            ".endr",
            "  ba aldis_window_overflow",
            "  nop",
            "  nop",
            "  nop",
            "  ba aldis_window_underflow",
            "  nop",
            "  nop",
            "  nop",
            ".rept 249",
            "  ba aldis_fault_entry",
            "  nop",
            "  nop",
            "  nop",
            ".endr",
            psr_off = const PSR_TRAPS_OFF,
            psr_on = const PSR_TRAPS_ON,
            wim = const WIM_START,
        );
    }
}
