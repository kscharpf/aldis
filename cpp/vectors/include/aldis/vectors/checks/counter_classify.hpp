// Check of the `counter_classify` operation of test-vectors/core/counters.json (CMN-031; CPP-053,
// TV-012). Counterpart of the `counter_classify` arm of crates/aldis-core/tests/vectors.rs.

#ifndef ALDIS_VECTORS_CHECKS_COUNTER_CLASSIFY_HPP
#define ALDIS_VECTORS_CHECKS_COUNTER_CLASSIFY_HPP

#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"

#include <cstdint>
#include <span>

namespace aldis::vectors {

/// `counter_classify`: the continuity of `next` received after `prev` on a `bits`-wide counter.
/// The expectation `class` (`in-sequence`, `repeat`, `gap` with `missing`, `backwards`) is a
/// `Continuity`, so a gap compares its `missing` count too.
struct CounterClassifyCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("counter_classify");

    /// `params.bits`.
    std::uint8_t bits{};
    /// `params.prev`.
    std::uint64_t prev{};
    /// `params.next`.
    std::uint64_t next{};
    /// `expect.class` (and `expect.missing` for a gap), or the error.
    Expect<core::Continuity> expect;
};

/// The continuity `aldis::core::ModCounter::classify` finds.
[[nodiscard]] constexpr core::Result<core::Continuity> run(const CounterClassifyCase& c) noexcept {
    const core::Result<core::ModCounter> counter = core::ModCounter::make(c.bits, c.prev);
    if (counter.error() != nullptr) {
        return *counter.error();
    }
    return counter.ok()->classify(c.next);
}

/// The verdict on `c` (TV-012).
[[nodiscard]] constexpr Verdict check(const CounterClassifyCase& c) noexcept {
    return compare(c.expect, run(c));
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_COUNTER_CLASSIFY_HPP
