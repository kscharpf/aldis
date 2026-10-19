// Integer helpers with an explicit modulus or bound (C++ only; CPP-032, GEN-024,
// docs/milestones/c0-foundations.md D10).
//
// Integer promotion turns arithmetic on unsigned types narrower than `int` into signed `int`
// arithmetic, so Aldis code never computes with them directly: it goes through these helpers,
// which state the field width or the bound. Every helper is total and is written so that no
// unsigned operation overflows (compare, then add or subtract), so none needs an exemption from
// the unsigned-overflow sanitizer (CPP-051). Together with bits.hpp, this is the only place
// with shifts and integer `static_cast`s, as `as` appears only in the aldis-core bit primitives.

#ifndef ALDIS_CORE_ARITH_HPP
#define ALDIS_CORE_ARITH_HPP

#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <bit>
#include <concepts>
#include <cstdint>
#include <limits>

namespace aldis::core::arith {

/// An unsigned integer type other than `bool`.
template <typename T>
concept Unsigned = std::unsigned_integral<T> && !std::same_as<T, bool>;

/// Number of value bits of `T`.
template <Unsigned T>
inline constexpr unsigned digits = static_cast<unsigned>(std::numeric_limits<T>::digits);

/// Bits in an octet.
inline constexpr unsigned octet_bits = 8;

/// The low `bits` bits set: `2^bits - 1`, all ones for `bits >= 64`, 0 for `bits == 0`.
[[nodiscard]] constexpr std::uint64_t low_mask(unsigned bits) noexcept {
    if (bits == 0) {
        return 0;
    }
    if (bits >= digits<std::uint64_t>) {
        return std::numeric_limits<std::uint64_t>::max();
    }
    return std::numeric_limits<std::uint64_t>::max() >> (digits<std::uint64_t> - bits);
}

/// `value` reduced modulo `2^digits<To>`: the low bits, kept deliberately (GEN-024).
template <Unsigned To, Unsigned From> [[nodiscard]] constexpr To truncate(From value) noexcept {
    return static_cast<To>(static_cast<std::uint64_t>(value) & low_mask(digits<To>));
}

/// `value` converted to a type at least as wide.
template <Unsigned To, Unsigned From>
    requires(digits<To> >= digits<From>)
[[nodiscard]] constexpr To widen(From value) noexcept {
    return static_cast<To>(value);
}

/// `value` converted to `To`, or `OutOfRange` if it does not fit (checked narrowing).
template <Unsigned To, Unsigned From>
[[nodiscard]] constexpr Result<To> narrow(From value) noexcept {
    const auto wide = static_cast<std::uint64_t>(value);
    if (wide > std::numeric_limits<To>::max()) {
        return Error{ErrorKind::OutOfRange}.with_detail(
            ErrorDetail::expected(std::numeric_limits<To>::max(), wide));
    }
    return static_cast<To>(value);
}

/// `value` converted to `To` if it is at most `max` (`max` must fit in `To`).
template <Unsigned To, Unsigned From>
[[nodiscard]] constexpr std::optional<To> narrow_to_max(From value, std::uint64_t max) noexcept {
    const auto wide = static_cast<std::uint64_t>(value);
    if (wide > max || wide > std::numeric_limits<To>::max()) {
        return std::nullopt;
    }
    return static_cast<To>(value);
}

/// `value` shifted left by `n`, keeping the low `digits<T>` bits; 0 if `n >= digits<T>`.
template <Unsigned T> [[nodiscard]] constexpr T shl(T value, unsigned n) noexcept {
    if (n >= digits<T>) {
        return 0;
    }
    const std::uint64_t kept = static_cast<std::uint64_t>(value) & low_mask(digits<T> - n);
    return static_cast<T>(kept << n);
}

/// `value` shifted right by `n`; 0 if `n >= digits<T>`.
template <Unsigned T> [[nodiscard]] constexpr T shr(T value, unsigned n) noexcept {
    if (n >= digits<T>) {
        return 0;
    }
    return static_cast<T>(static_cast<std::uint64_t>(value) >> n);
}

/// Number of bits set in `value`.
template <Unsigned T> [[nodiscard]] constexpr unsigned popcount(T value) noexcept {
    return static_cast<unsigned>(std::popcount(value));
}

/// Bitwise and, without promotion.
template <Unsigned T> [[nodiscard]] constexpr T bit_and(T a, T b) noexcept {
    return static_cast<T>(static_cast<std::uint64_t>(a) & static_cast<std::uint64_t>(b));
}

/// Bitwise or, without promotion.
template <Unsigned T> [[nodiscard]] constexpr T bit_or(T a, T b) noexcept {
    return static_cast<T>(static_cast<std::uint64_t>(a) | static_cast<std::uint64_t>(b));
}

/// Bitwise exclusive or, without promotion.
template <Unsigned T> [[nodiscard]] constexpr T bit_xor(T a, T b) noexcept {
    return static_cast<T>(static_cast<std::uint64_t>(a) ^ static_cast<std::uint64_t>(b));
}

/// Bitwise complement within `digits<T>` bits.
template <Unsigned T> [[nodiscard]] constexpr T bit_not(T a) noexcept {
    return static_cast<T>(~static_cast<std::uint64_t>(a) & low_mask(digits<T>));
}

/// `(a + b) mod 2^bits` for a field of `bits` bits (1–64); the operands are first reduced to the
/// field. GEN-024.
[[nodiscard]] constexpr std::uint64_t wrapping_add(std::uint64_t a, std::uint64_t b,
                                                   unsigned bits) noexcept {
    const std::uint64_t max = low_mask(bits);
    const std::uint64_t x = a & max;
    const std::uint64_t y = b & max;
    const std::uint64_t room = max - x; // x <= max
    return y > room ? y - room - 1U : x + y;
}

/// `(a - b) mod 2^bits` for a field of `bits` bits (1–64); the operands are first reduced to the
/// field. GEN-024.
[[nodiscard]] constexpr std::uint64_t wrapping_sub(std::uint64_t a, std::uint64_t b,
                                                   unsigned bits) noexcept {
    const std::uint64_t max = low_mask(bits);
    const std::uint64_t x = a & max;
    const std::uint64_t y = b & max;
    return x >= y ? x - y : max - (y - x) + 1U; // y - x >= 1, so no overflow
}

/// `(a + b) mod 2^Bits`, the field width stated at compile time (GEN-024).
template <unsigned Bits, Unsigned T>
    requires(Bits >= 1 && Bits <= digits<T>)
[[nodiscard]] constexpr T wrapping_add(T a, T b) noexcept {
    return static_cast<T>(
        wrapping_add(static_cast<std::uint64_t>(a), static_cast<std::uint64_t>(b), Bits));
}

/// `(a - b) mod 2^Bits`, the field width stated at compile time (GEN-024).
template <unsigned Bits, Unsigned T>
    requires(Bits >= 1 && Bits <= digits<T>)
[[nodiscard]] constexpr T wrapping_sub(T a, T b) noexcept {
    return static_cast<T>(
        wrapping_sub(static_cast<std::uint64_t>(a), static_cast<std::uint64_t>(b), Bits));
}

/// `a + b`, or `OutOfRange` if it exceeds the range of `T`.
template <Unsigned T> [[nodiscard]] constexpr Result<T> checked_add(T a, T b) noexcept {
    const auto x = static_cast<std::uint64_t>(a);
    const auto y = static_cast<std::uint64_t>(b);
    if (y > std::uint64_t{std::numeric_limits<T>::max()} - x) {
        return Error{ErrorKind::OutOfRange};
    }
    return static_cast<T>(x + y);
}

/// `a - b`, or `OutOfRange` if `b > a`.
template <Unsigned T> [[nodiscard]] constexpr Result<T> checked_sub(T a, T b) noexcept {
    const auto x = static_cast<std::uint64_t>(a);
    const auto y = static_cast<std::uint64_t>(b);
    if (y > x) {
        return Error{ErrorKind::OutOfRange};
    }
    return static_cast<T>(x - y);
}

/// `a * b`, or `OutOfRange` if it exceeds the range of `T`.
template <Unsigned T> [[nodiscard]] constexpr Result<T> checked_mul(T a, T b) noexcept {
    const auto x = static_cast<std::uint64_t>(a);
    const auto y = static_cast<std::uint64_t>(b);
    if (x != 0 && y > std::uint64_t{std::numeric_limits<T>::max()} / x) {
        return Error{ErrorKind::OutOfRange};
    }
    return static_cast<T>(x * y);
}

/// `a + b`, or the largest value of `T`.
template <Unsigned T> [[nodiscard]] constexpr T saturating_add(T a, T b) noexcept {
    const Result<T> sum = checked_add(a, b);
    return sum.ok() != nullptr ? *sum.ok() : std::numeric_limits<T>::max();
}

/// `a - b`, or 0 if `b > a`.
template <Unsigned T> [[nodiscard]] constexpr T saturating_sub(T a, T b) noexcept {
    const Result<T> difference = checked_sub(a, b);
    return difference.ok() != nullptr ? *difference.ok() : T{0};
}

/// `a * b`, or the largest value of `T`.
template <Unsigned T> [[nodiscard]] constexpr T saturating_mul(T a, T b) noexcept {
    const Result<T> product = checked_mul(a, b);
    return product.ok() != nullptr ? *product.ok() : std::numeric_limits<T>::max();
}

} // namespace aldis::core::arith

#endif // ALDIS_CORE_ARITH_HPP
