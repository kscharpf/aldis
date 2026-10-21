// Check of the `bits_get` operation of test-vectors/core/bits.json (GEN-050, GEN-051; CPP-053,
// TV-012). Counterpart of the `bits_get` arm of crates/aldis-core/tests/vectors.rs.

#ifndef ALDIS_VECTORS_CHECKS_BITS_GET_HPP
#define ALDIS_VECTORS_CHECKS_BITS_GET_HPP

#include "aldis/core/bits.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"

#include <cstddef>
#include <cstdint>
#include <span>

namespace aldis::vectors {

/// `bits_get`: reads the `width`-bit field at `bit_offset` of `input`; expects `value`.
struct BitsGetCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("bits_get");

    /// `input_hex`.
    Octets input;
    /// `params.bit_offset`.
    std::size_t bit_offset{};
    /// `params.width`.
    std::uint8_t width{};
    /// `expect.value`, or the error.
    Expect<std::uint64_t> expect;
};

/// The field value `aldis::core::bits::get` reads.
[[nodiscard]] constexpr core::Result<std::uint64_t> run(const BitsGetCase& c) noexcept {
    return core::bits::get(c.input.view(), c.bit_offset, c.width);
}

/// The verdict on `c` (TV-012).
[[nodiscard]] constexpr Verdict check(const BitsGetCase& c) noexcept {
    return compare(c.expect, run(c));
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_BITS_GET_HPP
