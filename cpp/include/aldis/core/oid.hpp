// Only-Idle-Data (OID) pseudo-noise generator: aldis-core `oid.rs` (CMN-020…CMN-023).
//
// TM (132.0-B-3 §4.1.4.6.2, Annex D), AOS (732.0-B-5 §4.1.4.1.5.2), and USLP (732.1-B-3
// §4.1.4.1.10) fill OID frame data fields with the output of a 32-cell LFSR with polynomial
// D⁰ + D¹ + D² + D²² + D³². The generator is initialized once at device start-up and never
// restarted: the sequence continues across OID frames (CMN-021). The register values below are
// those of aldis-core, whose tests reproduce the first output octets of 132.0 Annex D.

#ifndef ALDIS_CORE_OID_HPP
#define ALDIS_CORE_OID_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/config.hpp"

#include <cstddef>
#include <cstdint>
#include <span>

namespace aldis::core {

/// Fibonacci form (CMN-020): output xₙ, with x₀ … x₃₁ = 1 and xₙ = xₙ₋₁ ⊕ xₙ₋₂ ⊕ xₙ₋₂₂ ⊕ xₙ₋₃₂ for
/// n ≥ 32. The reference implementation. Value-initialized, it is in its start-up state.
class OidGenerator {
public:
    /// The generator in its device start-up state (all-ones seed).
    constexpr OidGenerator() noexcept = default;

    /// The next bit of the sequence (0 or 1).
    constexpr std::uint8_t next_bit() noexcept {
        // Bit 31 is x(n-32), the next output; bit 0 is x(n-1).
        const std::uint32_t r = register_;
        const std::uint32_t out = arith::shr(r, 31U);
        const std::uint32_t fed =
            (r ^ arith::shr(r, 1U) ^ arith::shr(r, 21U) ^ arith::shr(r, 31U)) & 1U;
        register_ = arith::shl(r, 1U) | fed;
        return arith::truncate<std::uint8_t>(out);
    }

    /// The next octet, MSB first.
    constexpr std::uint8_t next_octet() noexcept {
        std::uint32_t octet = 0;
        for (unsigned i = 0; i < arith::octet_bits; ++i) {
            octet = arith::shl(octet, 1U) | next_bit();
        }
        return arith::truncate<std::uint8_t>(octet);
    }

    /// Fills `buf` with the next `buf.size()` octets and advances by exactly that many (CMN-023).
    constexpr void fill(std::span<std::byte> buf) noexcept {
        for (std::byte& octet : buf) {
            octet = std::byte{next_octet()};
        }
    }

    friend constexpr bool operator==(const OidGenerator&, const OidGenerator&) noexcept = default;

private:
    static constexpr std::uint32_t seed = 0xFFFF'FFFFU; // x(0) … x(31) = 1

    std::uint32_t register_{seed};
};

/// Galois form (CMN-022) of the OID generator.
///
/// Seeded with `0b00000000001111111111111111111101` (132.0 Annex D, figure D-2): output the low
/// bit, shift right, and XOR `0x8020_0003` when the output bit is 1. Equal to `OidGenerator` bit
/// for bit. Value-initialized, it is in its start-up state.
class OidGeneratorGalois {
public:
    /// The generator in its device start-up state.
    constexpr OidGeneratorGalois() noexcept = default;

    /// The next bit of the sequence (0 or 1).
    constexpr std::uint8_t next_bit() noexcept {
        const std::uint32_t out = register_ & 1U;
        register_ = arith::shr(register_, 1U);
        if (out == 1U) {
            register_ ^= taps;
        }
        return arith::truncate<std::uint8_t>(out);
    }

    /// The next octet, MSB first.
    constexpr std::uint8_t next_octet() noexcept {
        std::uint32_t octet = 0;
        for (unsigned i = 0; i < arith::octet_bits; ++i) {
            octet = arith::shl(octet, 1U) | next_bit();
        }
        return arith::truncate<std::uint8_t>(octet);
    }

    /// Fills `buf` with the next `buf.size()` octets (CMN-023).
    constexpr void fill(std::span<std::byte> buf) noexcept {
        for (std::byte& octet : buf) {
            octet = std::byte{next_octet()};
        }
    }

    friend constexpr bool operator==(const OidGeneratorGalois&,
                                     const OidGeneratorGalois&) noexcept = default;

private:
    static constexpr std::uint32_t seed = 0b0000'0000'0011'1111'1111'1111'1111'1101U;
    static constexpr std::uint32_t taps = 0x8020'0003U;

    std::uint32_t register_{seed};
};

} // namespace aldis::core

#endif // ALDIS_CORE_OID_HPP
