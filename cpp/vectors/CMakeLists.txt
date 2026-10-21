# Vector loader, host runner, and code generation (CPP-053, CPP-034, TV-012;
# docs/milestones/c0-foundations.md D5, WP-4). Test tooling: nothing here is installed.
#
#   aldis_vectors_checks     INTERFACE  case model and check functions (freestanding, library rules)
#   aldis_vectors_loader     STATIC     host loader and code generator (nlohmann/json)
#   aldis_vectors_runner     STATIC     host runner: runs the loaded cases through the checks
#   aldis_vecgen             executable writes the cases as constexpr arrays (generated header)
#   aldis_vectors            executable host runner, one CTest test per vector file
#   aldis_vectors_generated  INTERFACE  the generated header aldis/vectors/generated/core.hpp
#   aldis_vectors_constexpr  OBJECT     static_asserts over the generated cases (CPP-034)
#
# Cross builds need aldis_vecgen for the build host (D5): a cross build with an emulator (s390x)
# builds it for the target and runs it under the emulator; a bare-metal build takes the
# ALDIS_VECGEN cache variable or, when it is empty, builds the tool first in a host build of this
# project through ExternalProject, with the pinned host GCC.

# The core vector files, named explicitly as crates/aldis-core/tests/vectors.rs names them.
# test-vectors/core/fields.json is the field catalogue (M2 plan D2), not a vector file; the C++
# FieldId gains its codes, and the runner reads the catalogue, in C1 (c0 plan D8).
set(ALDIS_CORE_VECTOR_FILES
    core/bits.json
    core/counters.json
    core/crc16.json
    core/errors.json
    core/layout.json
    core/oid.json)

set(hosted_tools OFF)
if(NOT CMAKE_SYSTEM_NAME STREQUAL "Generic" AND (ALDIS_BUILD_TESTS OR ALDIS_VECGEN_ONLY))
  set(hosted_tools ON)
endif()

# --- Case model and checks: freestanding, compiled under the library rules ---

set(check_headers
    include/aldis/vectors/case.hpp
    include/aldis/vectors/core.hpp
    include/aldis/vectors/checks/bits_get.hpp
    include/aldis/vectors/checks/bits_set.hpp
    include/aldis/vectors/checks/counter_classify.hpp
    include/aldis/vectors/checks/counter_increment.hpp
    include/aldis/vectors/checks/crc16.hpp
    include/aldis/vectors/checks/error_code.hpp
    include/aldis/vectors/checks/frame_layout.hpp
    include/aldis/vectors/checks/oid_fill.hpp
    include/aldis/vectors/checks/oid_fill_sequence.hpp)

add_library(aldis_vectors_checks INTERFACE)
target_include_directories(aldis_vectors_checks INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_link_libraries(aldis_vectors_checks INTERFACE aldis_core)
target_compile_features(aldis_vectors_checks INTERFACE cxx_std_20)

# Each header compiled on its own with the library warnings and clang-tidy configuration, as the
# module headers are (D7): the exerciser (WP-5) builds these headers freestanding.
if(ALDIS_HEADER_CHECKS AND NOT ALDIS_VECGEN_ONLY)
  set(tus)
  foreach(header IN LISTS check_headers)
    file(RELATIVE_PATH rel "${CMAKE_CURRENT_SOURCE_DIR}/include" "${CMAKE_CURRENT_SOURCE_DIR}/${header}")
    set(tu "${CMAKE_CURRENT_BINARY_DIR}/header_check/${rel}.cpp")
    file(GENERATE OUTPUT "${tu}" CONTENT
      "// Generated: ${rel} must compile on its own (docs/milestones/c0-foundations.md D7).\n#include \"${rel}\" // IWYU pragma: keep\n")
    list(APPEND tus "${tu}")
  endforeach()
  configure_file("${ALDIS_SOURCE_DIR}/.clang-tidy"
                 "${CMAKE_CURRENT_BINARY_DIR}/header_check/.clang-tidy" COPYONLY)
  add_library(aldis_vectors_checks_headers OBJECT ${tus})
  target_link_libraries(aldis_vectors_checks_headers PRIVATE aldis_vectors_checks)
  aldis_target_warnings(aldis_vectors_checks_headers LIBRARY)
endif()

# --- Host loader, runner, and generator ---

if(hosted_tools)
  include(Dependencies)
  aldis_fetch_nlohmann_json()

  # No integer sanitizer here: nlohmann/json is compiled into these sources, and D16 builds the
  # dependencies with address and undefined only.
  add_library(aldis_vectors_loader STATIC src/loader.cpp src/codegen.cpp
              include/aldis/vectors/loader.hpp include/aldis/vectors/codegen.hpp)
  target_include_directories(aldis_vectors_loader PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/include")
  target_link_libraries(aldis_vectors_loader PUBLIC aldis_vectors_checks nlohmann_json::nlohmann_json)
  aldis_target_warnings(aldis_vectors_loader TEST)

  add_executable(aldis_vecgen tools/aldis_vecgen.cpp)
  target_link_libraries(aldis_vecgen PRIVATE aldis_vectors_loader)
  aldis_target_warnings(aldis_vecgen TEST)
endif()

if(ALDIS_VECGEN_ONLY)
  return()
endif()

if(hosted_tools AND ALDIS_BUILD_TESTS)
  # The checks run here, so this library gets the full instrumentation (CPP-051, D14, D16).
  add_library(aldis_vectors_runner STATIC src/runner.cpp include/aldis/vectors/runner.hpp)
  target_link_libraries(aldis_vectors_runner PUBLIC aldis_vectors_loader)
  aldis_target_warnings(aldis_vectors_runner TEST)
  aldis_target_instrumentation(aldis_vectors_runner)

  add_executable(aldis_vectors tools/aldis_vectors.cpp)
  target_link_libraries(aldis_vectors PRIVATE aldis_vectors_runner)
  aldis_target_warnings(aldis_vectors TEST)
  aldis_target_instrumentation(aldis_vectors)
endif()

# --- Generated cases (D5) ---

set(vector_paths)
foreach(file IN LISTS ALDIS_CORE_VECTOR_FILES)
  list(APPEND vector_paths "${ALDIS_VECTORS_DIR}/${file}")
endforeach()

set(generated_dir "${CMAKE_CURRENT_BINARY_DIR}/generated")
set(generated_header "${generated_dir}/aldis/vectors/generated/core.hpp")

if(ALDIS_VECGEN)
  set(vecgen_command "${ALDIS_VECGEN}")
  set(vecgen_depends "${ALDIS_VECGEN}")
elseif(hosted_tools)
  # A target name: under an emulator (s390x) CMake runs it through CMAKE_CROSSCOMPILING_EMULATOR.
  set(vecgen_command aldis_vecgen)
  set(vecgen_depends aldis_vecgen)
else()
  include(ExternalProject)
  set(ALDIS_VECGEN_HOST_CXX "g++-14" CACHE STRING
      "Host C++ compiler of the aldis_vecgen host build of a bare-metal preset (D5)")
  set(ALDIS_VECGEN_HOST_TOOLCHAIN "GCC" CACHE STRING
      "toolchains.lock entry of ALDIS_VECGEN_HOST_CXX (D5)")
  set(host_dir "${CMAKE_BINARY_DIR}/vecgen-host")
  set(host_args
      -DCMAKE_CXX_COMPILER=${ALDIS_VECGEN_HOST_CXX}
      -DALDIS_TOOLCHAIN=${ALDIS_VECGEN_HOST_TOOLCHAIN}
      -DCMAKE_BUILD_TYPE=Release
      -DALDIS_VECGEN_ONLY=ON
      -DALDIS_BUILD_TESTS=OFF
      -DALDIS_HEADER_CHECKS=OFF)
  if(FETCHCONTENT_SOURCE_DIR_NLOHMANN_JSON)
    list(APPEND host_args -DFETCHCONTENT_SOURCE_DIR_NLOHMANN_JSON=${FETCHCONTENT_SOURCE_DIR_NLOHMANN_JSON})
  endif()
  ExternalProject_Add(aldis_vecgen_host
    SOURCE_DIR "${ALDIS_SOURCE_DIR}"
    BINARY_DIR "${host_dir}"
    CMAKE_GENERATOR "${CMAKE_GENERATOR}"
    CMAKE_ARGS ${host_args}
    BUILD_COMMAND "${CMAKE_COMMAND}" --build "${host_dir}" --target aldis_vecgen
    INSTALL_COMMAND ""
    BUILD_ALWAYS ON
    BUILD_BYPRODUCTS "${host_dir}/vectors/aldis_vecgen")
  set(vecgen_command "${host_dir}/vectors/aldis_vecgen")
  set(vecgen_depends aldis_vecgen_host "${host_dir}/vectors/aldis_vecgen")
endif()

add_custom_command(
  OUTPUT "${generated_header}"
  COMMAND ${vecgen_command} "${generated_header}" "${ALDIS_VECTORS_DIR}" ${ALDIS_CORE_VECTOR_FILES}
  DEPENDS ${vecgen_depends} ${vector_paths}
  COMMENT "Generating aldis/vectors/generated/core.hpp from test-vectors/core (D5)"
  VERBATIM)
add_custom_target(aldis_vectors_generate DEPENDS "${generated_header}")

add_library(aldis_vectors_generated INTERFACE)
target_include_directories(aldis_vectors_generated INTERFACE "${generated_dir}")
target_link_libraries(aldis_vectors_generated INTERFACE aldis_vectors_checks)
add_dependencies(aldis_vectors_generated aldis_vectors_generate)

# Constant evaluation of a subset of the cases on every preset's compiler (CPP-034), under the
# library rules, as the freestanding exerciser will compile them.
add_library(aldis_vectors_constexpr OBJECT "${ALDIS_SOURCE_DIR}/tests/constexpr_vectors.cpp")
target_link_libraries(aldis_vectors_constexpr PRIVATE aldis_vectors_generated)
aldis_target_warnings(aldis_vectors_constexpr LIBRARY)

# --- Host runner: one CTest test per vector file (CPP-053) ---

if(hosted_tools AND ALDIS_BUILD_TESTS)
  foreach(file IN LISTS ALDIS_CORE_VECTOR_FILES)
    string(REGEX REPLACE "[/.]" "_" name "${file}")
    add_test(NAME aldis_vectors_${name} COMMAND aldis_vectors "${ALDIS_VECTORS_DIR}/${file}")
  endforeach()
  # A corrupted vector fails the run (WP-4 "done when"): the runner on a copy of crc16.json with
  # one expected value changed must exit with failure, naming the case.
  add_test(NAME aldis_vectors_corrupted_fails
    COMMAND aldis_vectors "${CMAKE_CURRENT_SOURCE_DIR}/testdata/corrupted_crc16.json")
  set_tests_properties(aldis_vectors_corrupted_fails PROPERTIES
    WILL_FAIL ON)
  add_test(NAME aldis_vectors_corrupted_names_case
    COMMAND aldis_vectors "${CMAKE_CURRENT_SOURCE_DIR}/testdata/corrupted_crc16.json")
  set_tests_properties(aldis_vectors_corrupted_names_case PROPERTIES
    PASS_REGULAR_EXPRESSION "crc16-check-002: expected success \"0000\", got success \"29B1\"")
endif()
