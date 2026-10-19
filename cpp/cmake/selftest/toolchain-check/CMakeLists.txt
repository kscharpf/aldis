# Self-test of cpp/cmake/AldisToolchainCheck.cmake (c0 plan WP-0, D1; CPP-001): configures
# with the given compiler and ALDIS_TOOLCHAIN, and succeeds only if the compiler is the pinned one.
cmake_minimum_required(VERSION 3.25)
project(aldis_toolchain_check LANGUAGES CXX)
include("${CMAKE_CURRENT_LIST_DIR}/../../AldisToolchainCheck.cmake")
aldis_check_toolchain()
