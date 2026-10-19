# Cortex-R5F with the Arm GNU Toolchain (D2): the CPU and ABI of Rust's armv7r-none-eabihf
# (Armv7-R, Arm state, VFPv3-D16, hard-float ABI).
set(CMAKE_SYSTEM_PROCESSOR arm)
set(ALDIS_GNU_PREFIX arm-none-eabi-)
set(ALDIS_TARGET_FLAGS "-mcpu=cortex-r5 -marm -mfpu=vfpv3-d16 -mfloat-abi=hard")
include("${CMAKE_CURRENT_LIST_DIR}/gnu-baremetal.cmake")
