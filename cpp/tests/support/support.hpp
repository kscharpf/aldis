// Test support for the native C++ tests: octet literals, Result unwrapping, and a seeded
// generator for the randomized tests (docs/milestones/c0-foundations.md D4: no property-test
// library in C0).

#ifndef ALDIS_TESTS_SUPPORT_HPP
#define ALDIS_TESTS_SUPPORT_HPP

#include "aldis/core/error.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <vector>

// The generator wraps modulo 2^64 by design; the unsigned-overflow sanitizer (CPP-051) must not
// stop it. Test support only: library code never needs this (D10).
#ifdef __clang__
#define ALDIS_TEST_WRAPS __attribute__((no_sanitize("integer")))
#else
#define ALDIS_TEST_WRAPS
#endif

namespace aldis::test {

/// An array of octets from integer literals, e.g. `octets(0x18, 0x01)`.
template <typename... Ts> constexpr std::array<std::byte, sizeof...(Ts)> octets(Ts... values) {
    return {std::byte{static_cast<std::uint8_t>(values)}...};
}

/// A copy of `s`, for comparisons in assertions.
template <typename T, std::size_t Extent>
std::vector<std::remove_cv_t<T>> vec(std::span<T, Extent> s) {
    return {s.begin(), s.end()};
}

/// A copy of `a`, for comparisons in assertions.
template <typename T, std::size_t N> std::vector<T> vec(const std::array<T, N>& a) {
    return {a.begin(), a.end()};
}

/// A name as a string, for assertions.
inline std::string str(std::span<const char> name) { return {name.begin(), name.end()}; }

/// A string as a name, for lookups by name.
inline std::span<const char> chars(const std::string& s) { return {s.data(), s.size()}; }

/// The value of `r`; throws (failing the test) if it holds an error.
template <typename T> T unwrap(const core::Result<T>& r) {
    if (r.ok() == nullptr) {
        throw std::logic_error("Result holds an error");
    }
    return *r.ok();
}

/// The error of `r`; throws (failing the test) if it holds a value.
template <typename T> core::Error unwrap_err(const core::Result<T>& r) {
    if (r.error() == nullptr) {
        throw std::logic_error("Result holds a value");
    }
    return *r.error();
}

/// `*p`; throws (failing the test) if `p` is null. Keeps GCC's -Wnull-dereference quiet where an
/// assertion has already checked the pointer.
template <typename T> const T& deref(const T* p) {
    if (p == nullptr) {
        throw std::logic_error("null pointer");
    }
    return *p;
}

/// SplitMix64: a small, seeded, deterministic generator for randomized tests.
class Rng {
public:
    explicit Rng(std::uint64_t seed) : state_{seed} {}

    ALDIS_TEST_WRAPS std::uint64_t next() {
        state_ += 0x9E37'79B9'7F4A'7C15U;
        std::uint64_t z = state_;
        z = (z ^ (z >> 30U)) * 0xBF58'476D'1CE4'E5B9U;
        z = (z ^ (z >> 27U)) * 0x94D0'49BB'1331'11EBU;
        return z ^ (z >> 31U);
    }

    /// A value in `[lo, hi]`.
    ALDIS_TEST_WRAPS std::uint64_t range(std::uint64_t lo, std::uint64_t hi) {
        return lo + (next() % (hi - lo + 1U));
    }

    /// `n` random octets.
    std::vector<std::byte> bytes(std::size_t n) {
        std::vector<std::byte> out(n);
        for (std::byte& b : out) {
            b = std::byte{static_cast<std::uint8_t>(next())};
        }
        return out;
    }

private:
    std::uint64_t state_;
};

} // namespace aldis::test

#endif // ALDIS_TESTS_SUPPORT_HPP
