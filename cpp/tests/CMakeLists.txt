# Host tests of the native C++ library (GoogleTest; docs/milestones/c0-foundations.md D4).
# Every test names the requirements it covers with `// req: <ID>` (CPP-057, VER-020).

include(Dependencies)
include(GoogleTest)
aldis_fetch_googletest()

# aldis_add_test(<name> SOURCES <file>... LIBS <target>...)
function(aldis_add_test name)
  cmake_parse_arguments(PARSE_ARGV 1 arg "" "" "SOURCES;LIBS")
  add_executable(${name} ${arg_SOURCES})
  target_link_libraries(${name} PRIVATE ${arg_LIBS} GTest::gtest_main)
  aldis_target_warnings(${name} TEST)
  aldis_target_instrumentation(${name})
  # Test discovery runs the binary before each ctest run, so it also works under an emulator.
  gtest_discover_tests(${name} DISCOVERY_MODE PRE_TEST)
endfunction()

aldis_add_test(aldis_core_tests
  SOURCES
    core/api_test.cpp
    core/arith_test.cpp
    core/bits_test.cpp
    core/bytes_test.cpp
    core/config_test.cpp
    core/container_test.cpp
    core/crc_oid_test.cpp
    core/error_test.cpp
    core/ids_time_counter_test.cpp
    core/mode_anomaly_test.cpp
    core/security_layout_test.cpp
  LIBS aldis::core aldis::core_test_hooks)
target_include_directories(aldis_core_tests PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

# CPP-001, OI-28: the headers reject a standard older than C++20 at compile time.
add_test(NAME aldis_core_rejects_cxx17
  COMMAND "${CMAKE_CXX_COMPILER}" -std=c++17 -fsyntax-only
          -I "${ALDIS_SOURCE_DIR}/include" -x c++ "${ALDIS_SOURCE_DIR}/include/aldis/core/config.hpp")
set_tests_properties(aldis_core_rejects_cxx17 PROPERTIES
  PASS_REGULAR_EXPRESSION "Aldis requires C\\+\\+20")
