# Compiler flags of docs/profiles/cpp.md §5 (CPP-040, CPP-002, CPP-031, CPP-056), applied to
# every Aldis target by one function (docs/milestones/c0-foundations.md WP-1).
#
#   aldis_target_warnings(<target> LIBRARY)  library code (header self-containment TUs, the
#                                            exerciser): no exceptions or RTTI, and Clang's
#                                            -Wunsafe-buffer-usage (CPP-031)
#   aldis_target_warnings(<target> TEST)     tests and tools: the same warnings, but exceptions
#                                            and RTTI stay on (D4) and -Wunsafe-buffer-usage is
#                                            off (it is a library rule, CPP-031)
#
# The flags are PRIVATE: the installed aldis::core target carries none of them, so a
# consumer's own warning settings apply to the consumer's code.

include_guard(GLOBAL)

set(ALDIS_WARNINGS_COMMON
    -Wall -Wextra -Wpedantic -Werror
    -Wconversion -Wsign-conversion -Wshadow -Wold-style-cast -Wcast-align
    -Wnull-dereference -Wdouble-promotion -Wimplicit-fallthrough -Wswitch-enum -Wundef)

function(aldis_target_warnings target kind)
  if(NOT kind MATCHES "^(LIBRARY|TEST)$")
    message(FATAL_ERROR "aldis_target_warnings(${target} ${kind}): kind must be LIBRARY or TEST")
  endif()
  target_compile_options(${target} PRIVATE ${ALDIS_WARNINGS_COMMON})
  if(kind STREQUAL "LIBRARY")
    target_compile_options(${target} PRIVATE -fno-exceptions -fno-rtti)
    if(CMAKE_CXX_COMPILER_ID STREQUAL "Clang" OR CMAKE_CXX_COMPILER_ID STREQUAL "AppleClang")
      # The only opt-out is in aldis/core/bytes.hpp (CPP-031, D9).
      target_compile_options(${target} PRIVATE -Wunsafe-buffer-usage)
    endif()
    if(CMAKE_CXX_COMPILER_ID STREQUAL "GNU" AND CMAKE_SYSTEM_NAME STREQUAL "Generic")
      target_compile_options(${target} PRIVATE -fstack-usage)  # CPP-056, D15
    endif()
  endif()
endfunction()
