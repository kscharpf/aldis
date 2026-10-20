/* Cortex-M (QEMU mps2-an386, Cortex-M4): SSRAM1 at 0x0 holds code and data; SSRAM2 holds
   the stack. QEMU loads the ELF and zero-fills memory, so no copy or clear loop is needed. */
MEMORY
{
  CODE : ORIGIN = 0x00000000, LENGTH = 4M
  RAM  : ORIGIN = 0x20000000, LENGTH = 256K
}
ENTRY(aldis_exerciser_main)
_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
  .vector_table ORIGIN(CODE) :
  {
    LONG(_stack_top);
    KEEP(*(.vector_table.exceptions));
  } > CODE
  .text : { *(.text .text.*) } > CODE
  .rodata : { *(.rodata .rodata.* .data.rel.ro .data.rel.ro.*) } > CODE
  .data : { *(.data .data.*) } > CODE
  .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } > CODE
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab.*) }
}
