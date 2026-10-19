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
    core/config_test.cpp
  LIBS aldis::core)

# CPP-001, OI-28: the headers reject a standard older than C++20 at compile time.
add_test(NAME aldis_core_rejects_cxx17
  COMMAND "${CMAKE_CXX_COMPILER}" -std=c++17 -fsyntax-only
          -I "${ALDIS_SOURCE_DIR}/include" -x c++ "${ALDIS_SOURCE_DIR}/include/aldis/core/config.hpp")
set_tests_properties(aldis_core_rejects_cxx17 PROPERTIES
  PASS_REGULAR_EXPRESSION "Aldis requires C\\+\\+20")
