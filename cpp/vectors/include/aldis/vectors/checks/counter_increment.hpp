// Check of the `counter_increment` operation of test-vectors/core/counters.json (CMN-030,
// GEN-024; CPP-053, TV-012). Counterpart of the `counter_increment` arm of
// crates/aldis-core/tests/vectors.rs.

#ifndef ALDIS_VECTORS_CHECKS_COUNTER_INCREMENT_HPP
#define ALDIS_VECTORS_CHECKS_COUNTER_INCREMENT_HPP

#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"

#include <cstdint>
#include <span>

namespace aldis::vectors {

/// `counter_increment`: the value after `value` of a `bits`-wide modular counter.
struct CounterIncrementCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("counter_increment");

    /// `params.bits`.
    std::uint8_t bits{};
    /// `params.value`.
    std::uint64_t value{};
    /// `expect.value`, or the error.
    Expect<std::uint64_t> expect;
};

/// The next value of `aldis::core::ModCounter`.
[[nodiscard]] constexpr core::Result<std::uint64_t> run(const CounterIncrementCase& c) noexcept {
    const core::Result<core::ModCounter> counter = core::ModCounter::make(c.bits, c.value);
    if (counter.error() != nullptr) {
        return *counter.error();
    }
    return counter.ok()->next().value();
}

/// The verdict on `c` (TV-012).
[[nodiscard]] constexpr Verdict check(const CounterIncrementCase& c) noexcept {
    return compare(c.expect, run(c));
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_COUNTER_INCREMENT_HPP
