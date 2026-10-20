# Native C++ implementation of Aldis (docs/profiles/cpp.md; docs/milestones/c0-foundations.md).
#
# Configure through a preset of CMakePresets.json; every preset pins its compiler to
# toolchains.lock (CPP-001). Build output goes to ../target/cpp/<preset>.
cmake_minimum_required(VERSION 3.28)

# The version lives in aldis/core/config.hpp, which consumers can test in #if directives.
file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/include/aldis/core/config.hpp" version_lines
     REGEX "^#define ALDIS_VERSION_(MAJOR|MINOR|PATCH) [0-9]+$")
foreach(part IN ITEMS MAJOR MINOR PATCH)
  string(REGEX MATCH "ALDIS_VERSION_${part} ([0-9]+)" _ "${version_lines}")
  set(aldis_version_${part} "${CMAKE_MATCH_1}")
endforeach()

project(aldis
  VERSION ${aldis_version_MAJOR}.${aldis_version_MINOR}.${aldis_version_PATCH}
  DESCRIPTION "CCSDS space data link protocols, native C++ implementation"
  HOMEPAGE_URL "https://github.com/kscharpf/aldis"
  LANGUAGES CXX)

set(ALDIS_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")
include(GNUInstallDirs)
include(AldisToolchainCheck)
include(AldisWarnings)
include(AldisModule)

# Every Aldis build is exactly C++20 (CPP-001, OI-28).
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
# No C++20 modules: scanning for them would only add module-map flags that clang-tidy rejects.
set(CMAKE_CXX_SCAN_FOR_MODULES OFF)

option(ALDIS_BUILD_TESTS "Build the host tests (GoogleTest)" ${PROJECT_IS_TOP_LEVEL})
option(ALDIS_BUILD_EXERCISER "Build the freestanding exerciser (CPP-050, WP-5)" OFF)
option(ALDIS_HEADER_CHECKS "Compile every public header on its own (D7)" ${PROJECT_IS_TOP_LEVEL})
option(ALDIS_CRC_TABLE "Table-driven CRC-16, the counterpart of the crc-table feature (CMN-004)" OFF)
option(ALDIS_TEST_HOOKS "Test-only security hook, the counterpart of the test-hooks feature; never installed" OFF)
option(ALDIS_SANITIZE "Clang sanitizers of CPP-051 on all Aldis code and tests (D16)" OFF)
option(ALDIS_COVERAGE "Clang source-based coverage of the host tests (CPP-052, D14)" OFF)

if(PROJECT_IS_TOP_LEVEL)
  aldis_check_toolchain()
endif()

# Instrumentation (D14, D16). Dependencies get address and undefined only: the integer checks
# would fire inside them on intended unsigned wrap-around.
if(ALDIS_SANITIZE)
  if(NOT CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    message(FATAL_ERROR "ALDIS_SANITIZE needs Clang (D16)")
  endif()
  add_compile_options(-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer)
  add_link_options(-fsanitize=address,undefined)
  add_compile_definitions(_GLIBCXX_ASSERTIONS)
endif()
if(ALDIS_COVERAGE)
  if(NOT CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    message(FATAL_ERROR "ALDIS_COVERAGE needs Clang source-based coverage (D14)")
  endif()
  add_link_options(-fprofile-instr-generate)
endif()

# Instrumentation of Aldis's own code (library TUs and tests).
function(aldis_target_instrumentation target)
  if(ALDIS_SANITIZE)
    target_compile_options(${target} PRIVATE -fsanitize=integer,implicit-conversion)
  endif()
  if(ALDIS_COVERAGE)
    target_compile_options(${target} PRIVATE -fprofile-instr-generate -fcoverage-mapping -fcoverage-mcdc)
  endif()
endfunction()

# --- Modules (one target per Rust crate, cpp.md §2) ---

aldis_add_module(core
  HEADERS
    include/aldis/core/anomaly.hpp
    include/aldis/core/arith.hpp
    include/aldis/core/bits.hpp
    include/aldis/core/bytes.hpp
    include/aldis/core/config.hpp
    include/aldis/core/container.hpp
    include/aldis/core/core.hpp
    include/aldis/core/counter.hpp
    include/aldis/core/crc.hpp
    include/aldis/core/error.hpp
    include/aldis/core/ids.hpp
    include/aldis/core/layout.hpp
    include/aldis/core/mode.hpp
    include/aldis/core/oid.hpp
    include/aldis/core/security.hpp
    include/aldis/core/time.hpp)
if(ALDIS_CRC_TABLE)
  target_compile_definitions(aldis_core INTERFACE ALDIS_CRC_TABLE)
endif()

# Test-only security hook, the counterpart of the test-hooks feature (API-073). Never installed.
if(ALDIS_TEST_HOOKS OR ALDIS_BUILD_TESTS)
  add_library(aldis_core_test_hooks INTERFACE)
  add_library(aldis::core_test_hooks ALIAS aldis_core_test_hooks)
  target_include_directories(aldis_core_test_hooks INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/testing/include")
  target_link_libraries(aldis_core_test_hooks INTERFACE aldis_core)
endif()

# --- Tests ---

if(ALDIS_BUILD_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()

# --- Install: find_package(aldis) exports aldis::core (CPP-012) ---

include(CMakePackageConfigHelpers)
install(TARGETS aldis_core EXPORT aldisTargets FILE_SET HEADERS)
install(EXPORT aldisTargets NAMESPACE aldis:: DESTINATION "${CMAKE_INSTALL_DATADIR}/aldis/cmake")
configure_package_config_file(cmake/aldisConfig.cmake.in
  "${CMAKE_CURRENT_BINARY_DIR}/aldisConfig.cmake"
  INSTALL_DESTINATION "${CMAKE_INSTALL_DATADIR}/aldis/cmake")
write_basic_package_version_file("${CMAKE_CURRENT_BINARY_DIR}/aldisConfigVersion.cmake"
  COMPATIBILITY SameMinorVersion
  ARCH_INDEPENDENT)
install(FILES
  "${CMAKE_CURRENT_BINARY_DIR}/aldisConfig.cmake"
  "${CMAKE_CURRENT_BINARY_DIR}/aldisConfigVersion.cmake"
  DESTINATION "${CMAKE_INSTALL_DATADIR}/aldis/cmake")
install(FILES README.md DESTINATION "${CMAKE_INSTALL_DOCDIR}")
