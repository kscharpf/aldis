# GCC for a bare-metal target (D2). Included by <target>-gcc.cmake after it sets
# ALDIS_GNU_PREFIX (e.g. arm-none-eabi-) and ALDIS_TARGET_FLAGS. The GCC toolchain must be on
# PATH (setup-cpp puts it there).

include("${CMAKE_CURRENT_LIST_DIR}/baremetal-common.cmake")

set(CMAKE_C_COMPILER ${ALDIS_GNU_PREFIX}gcc)
set(CMAKE_CXX_COMPILER ${ALDIS_GNU_PREFIX}g++)
set(CMAKE_ASM_COMPILER ${ALDIS_GNU_PREFIX}gcc)
