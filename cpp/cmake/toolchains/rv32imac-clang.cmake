# RV32IMAC with the pinned Clang (D2): the ISA and ABI of Rust's riscv32imac-unknown-none-elf
# (RV32IMAC, ilp32), with xPack RISC-V GCC's headers.
set(CMAKE_SYSTEM_PROCESSOR riscv32)
set(ALDIS_CLANG_TARGET riscv32-unknown-elf)
set(ALDIS_TARGET_FLAGS "-march=rv32imac -mabi=ilp32")
set(ALDIS_GNU_PREFIX riscv-none-elf-)
set(ALDIS_GNU_FLAGS "${ALDIS_TARGET_FLAGS}")
include("${CMAKE_CURRENT_LIST_DIR}/clang-baremetal.cmake")
