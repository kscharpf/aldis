// Check of the `crc16` operation of test-vectors/core/crc16.json (CMN-001, CMN-002, CMN-004;
// CPP-053, TV-012). Counterpart of the `crc16` arm of crates/aldis-core/tests/vectors.rs, which
// also requires the bitwise and table forms to agree.

#ifndef ALDIS_VECTORS_CHECKS_CRC16_HPP
#define ALDIS_VECTORS_CHECKS_CRC16_HPP

#include "aldis/core/crc.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"

#include <cstdint>
#include <span>

namespace aldis::vectors {

/// `crc16`: the CRC-16 of `input`; expects `value_hex` (four upper-case hex digits).
struct Crc16Case {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("crc16");

    /// `input_hex`.
    Octets input;
    /// `expect.value_hex`.
    Expect<std::uint16_t> expect;
};

/// The CRC-16 of the form selected at build time (`ALDIS_CRC_TABLE`).
[[nodiscard]] constexpr core::Result<std::uint16_t> run(const Crc16Case& c) noexcept {
    return core::crc::crc16(c.input.view());
}

/// The verdict on `c` (TV-012); `FormsDisagree` if the bitwise and table forms differ.
[[nodiscard]] constexpr Verdict check(const Crc16Case& c) noexcept {
    const std::uint16_t bitwise = core::crc::crc16_bitwise(c.input.view());
    const std::uint16_t table = core::crc::crc16_table(c.input.view());
    const core::Result<std::uint16_t> selected = run(c);
    if (bitwise != table || selected.value_or(bitwise) != bitwise) {
        return Verdict::fail(Mismatch::FormsDisagree);
    }
    return compare(c.expect, selected);
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_CRC16_HPP
