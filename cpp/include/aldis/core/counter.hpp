// Modular frame and sequence counters with gap classification: aldis-core `counter.rs`
// (CMN-030, CMN-031, GEN-024).
//
// Used for TM MC/VC frame counts (8 bits, 132.0 §4.1.2.5–6), AOS VC frame counts (24 bits, or 28
// with the cycle field, 732.0 §4.1.2.4–5), USLP VCF counts (8–56 bits, 732.1 §4.1.2.12), and Space
// Packet sequence counts (14 bits, 133.0 §4.1.3.4.3). The width is a run-time value because USLP
// signals the count length in every frame header.

#ifndef ALDIS_CORE_COUNTER_HPP
#define ALDIS_CORE_COUNTER_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <cstdint>
#include <optional>

namespace aldis::core {

/// Continuity of a received count relative to the previous one (CMN-031).
class Continuity {
public:
    /// Which case applies.
    enum class Kind : std::uint8_t {
        /// Forward distance 1.
        InSequence,
        /// Forward distance 2 … modulus/2; `missing` = distance − 1 counts were skipped.
        Gap,
        /// Forward distance 0: the same count again.
        Repeat,
        /// Any other distance: the counter went backwards or was reset.
        Backwards,
    };

    /// Forward distance 1.
    [[nodiscard]] static constexpr Continuity in_sequence() noexcept {
        return Continuity{Kind::InSequence, 0};
    }

    /// `missing` counts skipped (forward distance `missing + 1`).
    [[nodiscard]] static constexpr Continuity gap(std::uint64_t missing) noexcept {
        return Continuity{Kind::Gap, missing};
    }

    /// The same count again.
    [[nodiscard]] static constexpr Continuity repeat() noexcept {
        return Continuity{Kind::Repeat, 0};
    }

    /// The counter went backwards or was reset.
    [[nodiscard]] static constexpr Continuity backwards() noexcept {
        return Continuity{Kind::Backwards, 0};
    }

    /// Which case applies.
    [[nodiscard]] constexpr Kind kind() const noexcept { return kind_; }

    /// Number of counts skipped, for a gap.
    [[nodiscard]] constexpr std::optional<std::uint64_t> missing() const noexcept {
        if (kind_ == Kind::Gap) {
            return missing_;
        }
        return std::nullopt;
    }

    friend constexpr bool operator==(Continuity, Continuity) noexcept = default;

private:
    constexpr Continuity(Kind kind, std::uint64_t missing) noexcept
        : kind_{kind}, missing_{missing} {}

    Kind kind_;
    std::uint64_t missing_;
};

class ModCounter;

namespace detail {

constexpr ModCounter counter_from_valid(std::uint8_t bits, std::uint64_t value) noexcept;

/// Classifies `next` as received after `prev`, both of `bits` bits (1–64; CMN-031).
[[nodiscard]] constexpr Continuity classify_masked(std::uint8_t bits, std::uint64_t prev,
                                                   std::uint64_t next) noexcept {
    const std::uint64_t d = arith::wrapping_sub(next, prev, bits);
    const std::uint64_t half =
        arith::shl(std::uint64_t{1}, arith::saturating_sub(unsigned{bits}, 1U));
    if (d == 0) {
        return Continuity::repeat();
    }
    if (d == 1) {
        return Continuity::in_sequence();
    }
    if (d <= half) {
        return Continuity::gap(arith::saturating_sub(d, std::uint64_t{1}));
    }
    return Continuity::backwards();
}

} // namespace detail

/// A counter value of 1–64 bits (CMN-030). Holds only values that fit its width (CPP-023).
class ModCounter {
public:
    /// A counter of `bits` width holding `value`; `OutOfRange` if `bits` is not 1–64 (`bits`) or
    /// `value` needs more than `bits` bits (`value`).
    [[nodiscard]] static constexpr Result<ModCounter> make(std::uint8_t bits,
                                                           std::uint64_t value) noexcept {
        if (bits == 0) {
            return Error::out_of_range(FieldId{"bits"}, 1, 0);
        }
        if (bits > 64) {
            return Error::out_of_range(FieldId{"bits"}, 64, bits);
        }
        if (value > arith::low_mask(bits)) {
            return Error::out_of_range(FieldId{"value"}, arith::low_mask(bits), value);
        }
        return ModCounter{bits, value};
    }

    /// The width in bits.
    [[nodiscard]] constexpr std::uint8_t bits() const noexcept { return bits_; }

    /// The current value.
    [[nodiscard]] constexpr std::uint64_t value() const noexcept { return value_; }

    /// The next value, modulo 2^bits (GEN-024).
    [[nodiscard]] constexpr ModCounter next() const noexcept {
        return ModCounter{bits_, arith::wrapping_add(value_, 1U, bits_)};
    }

    /// Advances the counter by one, modulo 2^bits.
    constexpr void increment() noexcept { *this = next(); }

    /// Forward distance from this value to `later`, modulo 2^bits; `OutOfRange` if `later` needs
    /// more than `bits` bits.
    [[nodiscard]] constexpr Result<std::uint64_t> distance_to(std::uint64_t later) const noexcept {
        const Result<ModCounter> checked = make(bits_, later);
        if (checked.error() != nullptr) {
            return *checked.error();
        }
        return arith::wrapping_sub(later, value_, bits_);
    }

    /// Classifies `next` as received after this value (CMN-031): distance 1 in sequence, 0
    /// repeat, 2 … modulus/2 gap, otherwise backwards. `OutOfRange` if `next` needs more than
    /// `bits` bits.
    [[nodiscard]] constexpr Result<Continuity> classify(std::uint64_t next) const noexcept {
        const Result<ModCounter> checked = make(bits_, next);
        if (checked.error() != nullptr) {
            return *checked.error();
        }
        return detail::classify_masked(bits_, value_, next);
    }

    friend constexpr bool operator==(ModCounter, ModCounter) noexcept = default;

private:
    friend constexpr ModCounter detail::counter_from_valid(std::uint8_t bits,
                                                           std::uint64_t value) noexcept;

    constexpr ModCounter(std::uint8_t bits, std::uint64_t value) noexcept
        : bits_{bits}, value_{value} {}

    std::uint8_t bits_;
    std::uint64_t value_;
};

/// A counter from a width (1–64) and value (fitting it) that the caller has already validated,
/// e.g. a `SeqCount` (aldis-core `from_valid`).
constexpr ModCounter detail::counter_from_valid(std::uint8_t bits, std::uint64_t value) noexcept {
    return ModCounter{bits, value};
}

} // namespace aldis::core

#endif // ALDIS_CORE_COUNTER_HPP
