// Check of the `bits_set` operation of test-vectors/core/bits.json (GEN-050, GEN-051; CPP-053,
// TV-012). Counterpart of the `bits_set` arm of crates/aldis-core/tests/vectors.rs.

#ifndef ALDIS_VECTORS_CHECKS_BITS_SET_HPP
#define ALDIS_VECTORS_CHECKS_BITS_SET_HPP

#include "aldis/core/bits.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"

#include <cstddef>
#include <cstdint>
#include <span>

namespace aldis::vectors {

/// `bits_set`: writes `value` into the `width`-bit field at `bit_offset` of a copy of `input`;
/// expects the buffer afterwards (`output_hex`).
struct BitsSetCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("bits_set");

    /// `input_hex`.
    Octets input;
    /// `params.bit_offset`.
    std::size_t bit_offset{};
    /// `params.width`.
    std::uint8_t width{};
    /// `params.value`.
    std::uint64_t value{};
    /// `expect.output_hex`, or the error.
    Expect<Octets> expect;
};

/// The buffer after `aldis::core::bits::set`.
[[nodiscard]] constexpr core::Result<Octets> run(const BitsSetCase& c) noexcept {
    Octets out = c.input;
    const core::Result<void> set = core::bits::set(out.view(), c.bit_offset, c.width, c.value);
    if (const core::Error* e = set.error(); e != nullptr) {
        return *e;
    }
    return out;
}

/// The verdict on `c` (TV-012).
[[nodiscard]] constexpr Verdict check(const BitsSetCase& c) noexcept {
    return compare(c.expect, run(c));
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_BITS_SET_HPP
