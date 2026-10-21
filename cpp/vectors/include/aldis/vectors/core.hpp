// The check functions of the nine operations of test-vectors/core/ (CPP-053, TV-012;
// docs/milestones/c0-foundations.md D5, WP-4). Each header defines the case struct of one
// operation, `run` (the outcome of aldis_core on the case), and `check` (the verdict). The host
// runner, the generated `constexpr` cases, and the freestanding exerciser (WP-5) all call these.

#ifndef ALDIS_VECTORS_CORE_HPP
#define ALDIS_VECTORS_CORE_HPP

#include "aldis/vectors/case.hpp"                     // IWYU pragma: export
#include "aldis/vectors/checks/bits_get.hpp"          // IWYU pragma: export
#include "aldis/vectors/checks/bits_set.hpp"          // IWYU pragma: export
#include "aldis/vectors/checks/counter_classify.hpp"  // IWYU pragma: export
#include "aldis/vectors/checks/counter_increment.hpp" // IWYU pragma: export
#include "aldis/vectors/checks/crc16.hpp"             // IWYU pragma: export
#include "aldis/vectors/checks/error_code.hpp"        // IWYU pragma: export
#include "aldis/vectors/checks/frame_layout.hpp"      // IWYU pragma: export
#include "aldis/vectors/checks/oid_fill.hpp"          // IWYU pragma: export
#include "aldis/vectors/checks/oid_fill_sequence.hpp" // IWYU pragma: export

#endif // ALDIS_VECTORS_CORE_HPP
