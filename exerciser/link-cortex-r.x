/* Cortex-R (armv7r-none-eabihf). One image for two QEMU machines (M1 D6):
   - `none` with -cpu cortex-r5f -m 1G (QEMU 8.2, the gate): RAM from 0x0;
   - mps3-an536 (Cortex-R52, QEMU >= 9.0): ATCM at 0x0, DDR from 0x20000000.
   The vector table sits at 0x0 (R5F low vectors, SCTLR.V = 0; R52 HVBAR = 0 at reset); code,
   data, and the stack sit at 0x20000000. QEMU loads the ELF and zero-fills memory, so no copy or
   clear loop is needed. */
MEMORY
{
  VECTORS : ORIGIN = 0x00000000, LENGTH = 256
  CODE    : ORIGIN = 0x20000000, LENGTH = 4M
  RAM     : ORIGIN = 0x20400000, LENGTH = 256K
}
ENTRY(_start)
_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
  .vector_table ORIGIN(VECTORS) : { KEEP(*(.vector_table)) } > VECTORS
  .text : { KEEP(*(.text._start)) *(.text .text.*) } > CODE
  .rodata : { *(.rodata .rodata.* .data.rel.ro .data.rel.ro.*) } > CODE
  .data : { *(.data .data.*) } > CODE
  .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } > CODE
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab.*) }
}
