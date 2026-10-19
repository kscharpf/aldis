# RV32IMAC with xPack RISC-V GCC (D2): the ISA and ABI of Rust's riscv32imac-unknown-none-elf
# (RV32IMAC, ilp32).
set(CMAKE_SYSTEM_PROCESSOR riscv32)
set(ALDIS_GNU_PREFIX riscv-none-elf-)
set(ALDIS_TARGET_FLAGS "-march=rv32imac -mabi=ilp32")
include("${CMAKE_CURRENT_LIST_DIR}/gnu-baremetal.cmake")
