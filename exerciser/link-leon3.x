/* LEON3 (sparc-unknown-none-elf, -C target-cpu=leon3) on QEMU leon3_generic: RAM from
   0x40000000 (QEMU hw/sparc/leon3.c, LEON3_RAM_OFFSET). Without a PROM image QEMU writes a small
   boot loader at 0x0 that enables the UART and jumps to the ELF entry, `_start` (target.rs).
   The trap table needs 4 KiB alignment (SPARC V8 TBR); its section asks for it. QEMU loads the
   ELF and zero-fills memory, so no copy or clear loop is needed (OI-08, non-gating). */
OUTPUT_FORMAT("elf32-sparc")
OUTPUT_ARCH(sparc)
MEMORY
{
  CODE : ORIGIN = 0x40000000, LENGTH = 4M
  RAM  : ORIGIN = 0x40400000, LENGTH = 256K
}
ENTRY(_start)
_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
  .text : { KEEP(*(.text._start)) KEEP(*(.text.aldis_traps)) *(.text .text.*) } > CODE
  .rodata : { *(.rodata .rodata.* .data.rel.ro .data.rel.ro.*) } > CODE
  .data : { *(.data .data.*) } > CODE
  .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } > CODE
}
