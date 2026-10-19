# Cortex-R5F with the pinned Clang (D2): the CPU and ABI of Rust's armv7r-none-eabihf
# (Armv7-R, Arm state, VFPv3-D16, hard-float ABI), with the Arm GNU Toolchain's headers.
set(CMAKE_SYSTEM_PROCESSOR arm)
set(ALDIS_CLANG_TARGET armv7r-unknown-none-eabihf)
set(ALDIS_TARGET_FLAGS "-mcpu=cortex-r5 -marm -mfpu=vfpv3-d16 -mfloat-abi=hard")
set(ALDIS_GNU_PREFIX arm-none-eabi-)
set(ALDIS_GNU_FLAGS "${ALDIS_TARGET_FLAGS}")
include("${CMAKE_CURRENT_LIST_DIR}/clang-baremetal.cmake")
