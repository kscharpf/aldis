# LEON3 (SPARC V8) with Gaisler BCC2 (D2): non-gating, reported against OI-08.
set(CMAKE_SYSTEM_PROCESSOR sparc)
set(ALDIS_GNU_PREFIX sparc-gaisler-elf-)
set(ALDIS_TARGET_FLAGS "-mcpu=leon3")
include("${CMAKE_CURRENT_LIST_DIR}/gnu-baremetal.cmake")
