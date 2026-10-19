# Dev-only dependencies of the C++ build (CPP-003; docs/milestones/c0-foundations.md D4).
#
# Layers 0-2 have no third-party dependency (GEN-001). Tests and tools use the libraries below,
# each pinned to a commit and fetched from that commit's archive with its SHA-256, so the
# content cannot change under us. `cargo deny` (VER-003) does not see C++, so this list is
# reviewed, licences included, in every PR that changes it.
#
# | Name        | Version (commit)                                   | Licence      | Used by                |
# |-------------|----------------------------------------------------|--------------|------------------------|
# | GoogleTest  | v1.18.0 (063de7e9578f82b369302001269680b4b1553359) | BSD-3-Clause | host tests             |
# | nlohmann/json | v3.12.0 (55f93686c01528224f448c19128836e7df245f72) | MIT        | vector loader (WP-4)   |
#
# Offline builds: point FETCHCONTENT_SOURCE_DIR_GOOGLETEST / FETCHCONTENT_SOURCE_DIR_NLOHMANN_JSON
# at an unpacked copy of the same commit.

include_guard(GLOBAL)
include(FetchContent)

FetchContent_Declare(googletest
  URL https://github.com/google/googletest/archive/063de7e9578f82b369302001269680b4b1553359.tar.gz
  URL_HASH SHA256=623fe05d283020f37a579a8985ba338091ac7229da62b5591ff40acdf05b95e2
  SYSTEM
  EXCLUDE_FROM_ALL)

FetchContent_Declare(nlohmann_json
  URL https://github.com/nlohmann/json/archive/55f93686c01528224f448c19128836e7df245f72.tar.gz
  URL_HASH SHA256=67f4cdd9ca930c9c1e130af4a437c7fc98fab77a2846fc2d2a14b4943831f8ef
  SYSTEM
  EXCLUDE_FROM_ALL)

# Makes GoogleTest available (targets GTest::gtest, GTest::gtest_main).
function(aldis_fetch_googletest)
  set(BUILD_GMOCK OFF CACHE BOOL "" FORCE)
  set(INSTALL_GTEST OFF CACHE BOOL "" FORCE)
  FetchContent_MakeAvailable(googletest)
endfunction()

# Makes nlohmann/json available (target nlohmann_json::nlohmann_json).
function(aldis_fetch_nlohmann_json)
  set(JSON_BuildTests OFF CACHE BOOL "" FORCE)
  set(JSON_Install OFF CACHE BOOL "" FORCE)
  FetchContent_MakeAvailable(nlohmann_json)
endfunction()
