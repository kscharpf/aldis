/* RISC-V (riscv32imac-unknown-none-elf) on QEMU virt with -bios none: DRAM from 0x80000000,
   where QEMU starts the hart. `_start` comes first, so the image starts at the reset address
   whether QEMU jumps to the DRAM base or to the ELF entry (M1 D6). QEMU loads the ELF and
   zero-fills memory, so no copy or clear loop is needed. */
MEMORY
{
  CODE : ORIGIN = 0x80000000, LENGTH = 4M
  RAM  : ORIGIN = 0x80400000, LENGTH = 256K
}
ENTRY(_start)
_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
  .text : { KEEP(*(.text._start)) *(.text .text.*) } > CODE
  .rodata : { *(.rodata .rodata.* .srodata .srodata.* .data.rel.ro .data.rel.ro.*) } > CODE
  .data : { *(.data .data.* .sdata .sdata.*) } > CODE
  .bss (NOLOAD) : { *(.bss .bss.* .sbss .sbss.*) *(COMMON) } > CODE
}
