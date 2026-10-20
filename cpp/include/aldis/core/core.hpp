// Umbrella header of aldis_core, Layer 0 of the native C++ implementation
// (docs/milestones/c0-foundations.md D7; docs/profiles/cpp.md §2). Mirrors crates/aldis-core.
//
// | Header | Contents | Requirements |
// |---|---|---|
// | config.hpp | Language-standard check, version, ALDIS_LIFETIMEBOUND | CPP-001 |
// | error.hpp | Error catalogue, Error, Result | GEN-040, GEN-041, CPP-021 |
// | arith.hpp | Wrapping, checked, saturating helpers (C++ only) | CPP-032, GEN-024 |
// | bytes.hpp | Checked element access, OctetRange, ByteReader/ByteWriter (C++ only) | CPP-031 |
// | mode.hpp | DecodeMode (strict/lenient) | API-030…API-032 |
// | anomaly.hpp | Lenient-mode anomaly flags, AnomalySet | GEN-043, GEN-081 |
// | bits.hpp | Big-endian bit-field primitives | GEN-050, GEN-051, CPP-033 |
// | ids.hpp | Range-checked identifiers | API-001, API-002, CPP-020 |
// | time.hpp | Instant, Duration | API-003 |
// | crc.hpp | FECF CRC-16 | CMN-001…CMN-004 |
// | oid.hpp | OID pseudo-noise generators | CMN-020…CMN-023 |
// | counter.hpp | Modular counters, gap classification | CMN-030, CMN-031 |
// | container.hpp | Fixed-capacity queues and vectors | GEN-010…GEN-012, API-061 |
// | security.hpp | SDLS reservation hooks | API-070…API-075, CPP-024, CPP-025 |
// | layout.hpp | Frame field layout | CMN-040, CMN-041 |
//
// No heap, no exceptions, no RTTI, and no undefined behavior on any input (GEN-010, GEN-020,
// CPP-002). Every public function is noexcept (CPP-026).

#ifndef ALDIS_CORE_CORE_HPP
#define ALDIS_CORE_CORE_HPP

#include "aldis/core/anomaly.hpp"   // IWYU pragma: export
#include "aldis/core/arith.hpp"     // IWYU pragma: export
#include "aldis/core/bits.hpp"      // IWYU pragma: export
#include "aldis/core/bytes.hpp"     // IWYU pragma: export
#include "aldis/core/config.hpp"    // IWYU pragma: export
#include "aldis/core/container.hpp" // IWYU pragma: export
#include "aldis/core/counter.hpp"   // IWYU pragma: export
#include "aldis/core/crc.hpp"       // IWYU pragma: export
#include "aldis/core/error.hpp"     // IWYU pragma: export
#include "aldis/core/ids.hpp"       // IWYU pragma: export
#include "aldis/core/layout.hpp"    // IWYU pragma: export
#include "aldis/core/mode.hpp"      // IWYU pragma: export
#include "aldis/core/oid.hpp"       // IWYU pragma: export
#include "aldis/core/security.hpp"  // IWYU pragma: export
#include "aldis/core/time.hpp"      // IWYU pragma: export

#endif // ALDIS_CORE_CORE_HPP
