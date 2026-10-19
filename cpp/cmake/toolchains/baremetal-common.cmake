# Settings shared by every bare-metal toolchain file (CPP-011; docs/milestones/c0-foundations.md
# D2, D6). Not a toolchain file on its own: the per-target files set ALDIS_TARGET_FLAGS (the CPU
# and ABI flags of the matching Rust target) and include this file.
#
# Freestanding, without exceptions, RTTI, or unwind tables (so that nothing references the
# unwinder, D6), one section per function and object so the linker drops what is unused.

set(CMAKE_SYSTEM_NAME Generic)
# No C library or start-up code to link a test program against: configure checks build a
# static library instead.
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

set(ALDIS_FREESTANDING_FLAGS
    "-ffreestanding -fno-exceptions -fno-rtti -fno-unwind-tables -fno-asynchronous-unwind-tables -ffunction-sections -fdata-sections")

foreach(lang IN ITEMS C CXX ASM)
  set(CMAKE_${lang}_FLAGS_INIT "${ALDIS_TARGET_FLAGS} ${ALDIS_FREESTANDING_FLAGS}")
endforeach()
