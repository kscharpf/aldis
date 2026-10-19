# The pinned Clang for a bare-metal target, with the matching GCC toolchain's libstdc++ headers
# and C headers (D2). Included by <target>-clang.cmake after it sets:
#
#   ALDIS_CLANG_TARGET   Clang target triple
#   ALDIS_TARGET_FLAGS   Clang CPU and ABI flags
#   ALDIS_GNU_PREFIX     prefix of the GCC toolchain whose headers are used (on PATH)
#   ALDIS_GNU_FLAGS      the same CPU and ABI in GCC's spelling, to select its multilib
#
# Only headers are taken from the GCC toolchain; neither compiler links libstdc++, libsupc++,
# or a C library (D6).

include("${CMAKE_CURRENT_LIST_DIR}/baremetal-common.cmake")

set(CMAKE_C_COMPILER clang)
set(CMAKE_CXX_COMPILER clang++)
set(CMAKE_ASM_COMPILER clang)
foreach(lang IN ITEMS C CXX ASM)
  set(CMAKE_${lang}_COMPILER_TARGET ${ALDIS_CLANG_TARGET})
endforeach()

find_program(ALDIS_GNU_GXX ${ALDIS_GNU_PREFIX}g++ REQUIRED)
cmake_path(GET ALDIS_GNU_GXX PARENT_PATH gnu_bin)
cmake_path(GET gnu_bin PARENT_PATH gnu_root)
string(REGEX REPLACE "-$" "" gnu_triple "${ALDIS_GNU_PREFIX}")
separate_arguments(gnu_flags UNIX_COMMAND "${ALDIS_GNU_FLAGS}")
execute_process(COMMAND "${ALDIS_GNU_GXX}" -dumpfullversion
                OUTPUT_VARIABLE gnu_version OUTPUT_STRIP_TRAILING_WHITESPACE
                COMMAND_ERROR_IS_FATAL ANY)
execute_process(COMMAND "${ALDIS_GNU_GXX}" ${gnu_flags} -print-multi-directory
                OUTPUT_VARIABLE gnu_multilib OUTPUT_STRIP_TRAILING_WHITESPACE
                COMMAND_ERROR_IS_FATAL ANY)

set(gnu_sysroot "${gnu_root}/${gnu_triple}")
set(libstdcxx "${gnu_sysroot}/include/c++/${gnu_version}")
if(gnu_multilib STREQUAL ".")
  set(libstdcxx_target "${libstdcxx}/${gnu_triple}")
else()
  set(libstdcxx_target "${libstdcxx}/${gnu_triple}/${gnu_multilib}")
endif()
foreach(dir IN ITEMS "${libstdcxx}" "${libstdcxx_target}" "${gnu_sysroot}/include")
  if(NOT IS_DIRECTORY "${dir}")
    message(FATAL_ERROR "Aldis: expected header directory not found: ${dir}")
  endif()
endforeach()

string(APPEND CMAKE_CXX_FLAGS_INIT
       " -nostdinc++ -isystem ${libstdcxx} -isystem ${libstdcxx_target}")
foreach(lang IN ITEMS C CXX ASM)
  string(APPEND CMAKE_${lang}_FLAGS_INIT " --sysroot=${gnu_sysroot}")
endforeach()
