# Cortex-M4F with the Arm GNU Toolchain (D2): the CPU and ABI of Rust's thumbv7em-none-eabihf
# (Armv7E-M, Thumb, FPv4-SP-D16, hard-float ABI).
set(CMAKE_SYSTEM_PROCESSOR arm)
set(ALDIS_GNU_PREFIX arm-none-eabi-)
set(ALDIS_TARGET_FLAGS "-mcpu=cortex-m4 -mthumb -mfpu=fpv4-sp-d16 -mfloat-abi=hard")
include("${CMAKE_CURRENT_LIST_DIR}/gnu-baremetal.cmake")
