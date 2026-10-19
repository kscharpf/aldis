# Configure-time check that the compiler is the one pinned in cpp/toolchains.lock
# (docs/milestones/c0-foundations.md D1; CPP-001).
#
# Every preset sets ALDIS_TOOLCHAIN to the lock entry its compiler must match (LLVM, GCC,
# GCC_S390X, ARM_GNU, RISCV_GNU, or BCC2). aldis_check_toolchain() then fails the configure if
# the C or C++ compiler's identity or version differs from <ALDIS_TOOLCHAIN>_VERSION in the lock.
#
# ALDIS_ALLOW_UNPINNED_COMPILER turns the failure into a warning for local experiments with
# another compiler (for example Apple Clang). Results from such a build verify nothing, and CI
# refuses the option.

include_guard(GLOBAL)

set(ALDIS_TOOLCHAINS_LOCK "${CMAKE_CURRENT_LIST_DIR}/../toolchains.lock"
    CACHE FILEPATH "Pinned toolchain versions (cpp/toolchains.lock)")
set(ALDIS_TOOLCHAIN "" CACHE STRING
    "Lock entry the compiler must match: LLVM, GCC, GCC_S390X, ARM_GNU, RISCV_GNU, or BCC2")
set_property(CACHE ALDIS_TOOLCHAIN PROPERTY STRINGS LLVM GCC GCC_S390X ARM_GNU RISCV_GNU BCC2)
option(ALDIS_ALLOW_UNPINNED_COMPILER
       "Local use only: warn instead of failing when the compiler is not the pinned one" OFF)

# Reads every KEY=VALUE line of the lock file into variables named <prefix><KEY>.
function(aldis_read_toolchains_lock prefix)
  if(NOT EXISTS "${ALDIS_TOOLCHAINS_LOCK}")
    message(FATAL_ERROR "Aldis: toolchain lock file not found: ${ALDIS_TOOLCHAINS_LOCK}")
  endif()
  file(STRINGS "${ALDIS_TOOLCHAINS_LOCK}" lines REGEX "^[A-Z][A-Z0-9_]*=")
  foreach(line IN LISTS lines)
    string(FIND "${line}" "=" eq)
    string(SUBSTRING "${line}" 0 ${eq} key)
    math(EXPR start "${eq} + 1")
    string(SUBSTRING "${line}" ${start} -1 value)
    set(${prefix}${key} "${value}" PARENT_SCOPE)
  endforeach()
endfunction()

function(aldis_check_toolchain)
  aldis_read_toolchains_lock(lock_)
  set(known LLVM GCC GCC_S390X ARM_GNU RISCV_GNU BCC2)
  if(NOT ALDIS_TOOLCHAIN IN_LIST known)
    message(FATAL_ERROR
      "Aldis: ALDIS_TOOLCHAIN is '${ALDIS_TOOLCHAIN}', expected one of ${known}. "
      "Configure through a preset of cpp/CMakePresets.json (CPP-001).")
  endif()
  if(NOT DEFINED lock_${ALDIS_TOOLCHAIN}_VERSION)
    message(FATAL_ERROR
      "Aldis: ${ALDIS_TOOLCHAINS_LOCK} has no ${ALDIS_TOOLCHAIN}_VERSION")
  endif()
  set(expected_version "${lock_${ALDIS_TOOLCHAIN}_VERSION}")
  if(ALDIS_TOOLCHAIN STREQUAL "LLVM")
    set(expected_id Clang)
  else()
    set(expected_id GNU)
  endif()

  get_property(languages GLOBAL PROPERTY ENABLED_LANGUAGES)
  foreach(lang IN ITEMS C CXX)
    if(NOT lang IN_LIST languages)
      continue()
    endif()
    set(id "${CMAKE_${lang}_COMPILER_ID}")
    set(version "${CMAKE_${lang}_COMPILER_VERSION}")
    if(id STREQUAL expected_id AND version STREQUAL expected_version)
      message(STATUS
        "Aldis: ${lang} compiler ${id} ${version} matches cpp/toolchains.lock (${ALDIS_TOOLCHAIN})")
      continue()
    endif()
    set(msg
      "Aldis: the ${lang} compiler is ${id} ${version} (${CMAKE_${lang}_COMPILER}), but "
      "cpp/toolchains.lock pins ${expected_id} ${expected_version} for ${ALDIS_TOOLCHAIN} "
      "(CPP-001). Install the pinned toolchain (.github/actions/setup-cpp).")
    if(NOT ALDIS_ALLOW_UNPINNED_COMPILER)
      message(FATAL_ERROR ${msg})
    endif()
    if(DEFINED ENV{CI} OR DEFINED ENV{GITHUB_ACTIONS})
      message(FATAL_ERROR ${msg} " ALDIS_ALLOW_UNPINNED_COMPILER is refused in CI.")
    endif()
    message(WARNING ${msg} " Continuing because ALDIS_ALLOW_UNPINNED_COMPILER is ON: "
                    "this build verifies nothing.")
  endforeach()
endfunction()
