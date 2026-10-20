// Strict and lenient decoding: aldis-core `mode.rs` (API-030, API-031, API-032).
//
// Every decoder takes a `DecodeMode`. In `Strict` mode a violation of a Blue Book SHALL that is
// checkable from the octets and the configuration is an error. In `Lenient` mode, deviations in
// reserved/spare bits, fields CCSDS declares "undefined", conventional-zero fields, and
// fixed-value fields of idle data units, plus a reserved time code ID (TIME-002), are accepted
// and reported in an `AnomalySet` (GEN-043). Version numbers, lengths, and FECF/FHEC failures
// are errors in both modes.

#ifndef ALDIS_CORE_MODE_HPP
#define ALDIS_CORE_MODE_HPP

#include "aldis/core/config.hpp"

#include <cstdint>

namespace aldis::core {

/// How a decoder treats deviations it can tolerate (API-030). Value-initialized, it is
/// `Strict`, the default.
enum class DecodeMode : std::uint8_t {
    /// Every checkable Blue Book violation is an error (API-031). The default.
    Strict = 0,
    /// Tolerable deviations are accepted and reported as anomaly flags (API-032, GEN-043).
    Lenient = 1,
};

} // namespace aldis::core

#endif // ALDIS_CORE_MODE_HPP
