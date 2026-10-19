// Consumer of the installed aldis package: the headers are found through aldis::core and
// compile as C++20 (CPP-012, CPP-001).

#include "aldis/core/core.hpp"

static_assert(__cplusplus >= 202002L, "aldis::core requires C++20 (cxx_std_20)");

int main() { return ALDIS_VERSION_MAJOR == 0 && ALDIS_VERSION_MINOR == 1 ? 0 : 1; }
