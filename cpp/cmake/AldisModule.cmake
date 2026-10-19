# One CMake library target per module (CPP-010; docs/milestones/c0-foundations.md D7).
#
#   aldis_add_module(<name> HEADERS <header>... [DEPENDS <module>...])
#
# Creates the header-only INTERFACE target aldis_<name> with the alias aldis::<name> and the
# C++20 compile feature (CPP-001), linked to the aldis_<module> targets of DEPENDS. The
# dependency rules between modules (CPP-010) are checked by `cargo xtask cpp layers` on the
# target graph.
#
# When ALDIS_HEADER_CHECKS is on, every header is also compiled on its own in a generated
# translation unit, so each header is self-contained and gets the full warning set of library
# code and clang-tidy (D7). These TUs form the OBJECT library aldis_<name>_headers.

include_guard(GLOBAL)
include("${CMAKE_CURRENT_LIST_DIR}/AldisWarnings.cmake")

function(aldis_add_module name)
  cmake_parse_arguments(PARSE_ARGV 1 arg "" "" "HEADERS;DEPENDS")
  if(NOT arg_HEADERS)
    message(FATAL_ERROR "aldis_add_module(${name}): no HEADERS")
  endif()
  set(headers)
  foreach(header IN LISTS arg_HEADERS)
    cmake_path(ABSOLUTE_PATH header BASE_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}" OUTPUT_VARIABLE abs)
    list(APPEND headers "${abs}")
  endforeach()
  set(target aldis_${name})
  add_library(${target} INTERFACE)
  add_library(aldis::${name} ALIAS ${target})
  set_target_properties(${target} PROPERTIES EXPORT_NAME ${name})
  target_compile_features(${target} INTERFACE cxx_std_20)
  foreach(dep IN LISTS arg_DEPENDS)
    target_link_libraries(${target} INTERFACE aldis_${dep})
  endforeach()
  # The file set puts include/ on the include path in the build tree and its install location
  # in the installed package.
  target_sources(${target} INTERFACE
    FILE_SET HEADERS
    BASE_DIRS "${ALDIS_SOURCE_DIR}/include"
    FILES ${headers})

  if(ALDIS_HEADER_CHECKS)
    set(tus)
    foreach(header IN LISTS headers)
      file(RELATIVE_PATH rel "${ALDIS_SOURCE_DIR}/include" "${header}")
      set(tu "${CMAKE_CURRENT_BINARY_DIR}/header_check/${rel}.cpp")
      file(GENERATE OUTPUT "${tu}" CONTENT
        "// Generated: ${rel} must compile on its own (docs/milestones/c0-foundations.md D7).\n#include \"${rel}\"\n")
      list(APPEND tus "${tu}")
    endforeach()
    # clang-tidy reads the configuration nearest to the translation unit, and these TUs live
    # in the build tree: give them the library configuration.
    configure_file("${ALDIS_SOURCE_DIR}/.clang-tidy"
                   "${CMAKE_CURRENT_BINARY_DIR}/header_check/.clang-tidy" COPYONLY)
    add_library(${target}_headers OBJECT ${tus})
    target_link_libraries(${target}_headers PRIVATE ${target})
    aldis_target_warnings(${target}_headers LIBRARY)
  endif()
endfunction()
