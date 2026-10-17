/* Link-only targets (Cortex-R, RISC-V): enough layout to produce a fully linked image, so
   any surviving reference to the undefined panic symbol fails the link (VER-010). */
MEMORY
{
  CODE : ORIGIN = 0x00000000, LENGTH = 4M
  RAM  : ORIGIN = 0x20000000, LENGTH = 256K
}
_stack_top = ORIGIN(RAM) + LENGTH(RAM);
ENTRY(_start)

SECTIONS
{
  .text : { *(.text .text.*) } > CODE
  .rodata : { *(.rodata .rodata.* .srodata .srodata.* .data.rel.ro .data.rel.ro.*) } > CODE
  .data : { *(.data .data.* .sdata .sdata.*) } > RAM
  .bss (NOLOAD) : { *(.bss .bss.* .sbss .sbss.*) *(COMMON) } > RAM
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab.*) }
}
