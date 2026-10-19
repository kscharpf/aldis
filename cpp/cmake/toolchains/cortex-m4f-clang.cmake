# Cortex-M4F with the pinned Clang (D2): the CPU and ABI of Rust's thumbv7em-none-eabihf
# (Armv7E-M, Thumb, FPv4-SP-D16, hard-float ABI), with the Arm GNU Toolchain's headers.
set(CMAKE_SYSTEM_PROCESSOR arm)
set(ALDIS_CLANG_TARGET thumbv7em-unknown-none-eabihf)
set(ALDIS_TARGET_FLAGS "-mcpu=cortex-m4 -mthumb -mfpu=fpv4-sp-d16 -mfloat-abi=hard")
set(ALDIS_GNU_PREFIX arm-none-eabi-)
set(ALDIS_GNU_FLAGS "${ALDIS_TARGET_FLAGS}")
include("${CMAKE_CURRENT_LIST_DIR}/clang-baremetal.cmake")
