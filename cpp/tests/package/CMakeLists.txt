# Consumer of the installed package (CPP-012; docs/milestones/c0-foundations.md WP-1).
# CI installs the library, then configures this project with CMAKE_PREFIX_PATH pointing at the
# install prefix, builds it, and runs it.
cmake_minimum_required(VERSION 3.28)
project(aldis_package_consumer LANGUAGES CXX)

find_package(aldis 0.1 REQUIRED)
add_executable(consumer main.cpp)
target_link_libraries(consumer PRIVATE aldis::core)
