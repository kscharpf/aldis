// Lenient-mode anomaly flags: aldis-core `anomaly.rs` (GEN-043, API-032, GEN-081).
//
// A decoder in `DecodeMode::Lenient` returns an `AnomalySet` alongside a successful result. Each
// codec declares its flags as an `enum class` and specializes `AnomalyTraits` with one table of
// stable bit indices, the counterpart of `anomaly_flags!`. The indices are listed in each
// standard document (API-033) and shared with the C ABI, so a flag is never renumbered. The
// `Anomaly` concept accepts only a table whose indices are below 32 and unique, and whose flags
// are distinct, so the invariants of the Rust trait hold at compile time. A strict decoder
// always returns the empty set.

#ifndef ALDIS_CORE_ANOMALY_HPP
#define ALDIS_CORE_ANOMALY_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/config.hpp"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <type_traits>

namespace aldis::core {

namespace anomaly {

/// Number of distinct bit indices an `AnomalySet` can hold (GEN-043).
inline constexpr std::uint8_t capacity = 32;

} // namespace anomaly

/// One flag of an anomaly enum and its stable bit index (GEN-043).
template <typename A> struct AnomalyFlag {
    /// The flag.
    A flag;
    /// Its bit index, below `anomaly::capacity`; never renumbered.
    std::uint8_t bit;
};

/// The table of an anomaly flag enum. A codec specializes it with
/// `static constexpr std::array<AnomalyFlag<A>, N> flags`, every flag in declaration order
/// (the counterpart of `anomaly_flags!`; GEN-043, GEN-081).
template <typename A> struct AnomalyTraits;

namespace detail {

/// Whether the table of `A` keeps the invariants: every index below 32, no index or flag twice.
template <typename A> constexpr bool valid_anomaly_table() noexcept {
    std::uint32_t seen = 0;
    const auto& flags = AnomalyTraits<A>::flags;
    std::size_t i = 0;
    for (const AnomalyFlag<A>& f : flags) {
        if (f.bit >= anomaly::capacity) {
            return false;
        }
        const std::uint32_t mask = arith::shl(std::uint32_t{1}, f.bit);
        if ((seen & mask) != 0U) {
            return false;
        }
        seen |= mask;
        std::size_t j = 0;
        for (const AnomalyFlag<A>& g : flags) {
            if (j != i && g.flag == f.flag) {
                return false;
            }
            ++j;
        }
        ++i;
    }
    return true;
}

} // namespace detail

/// A lenient-mode anomaly flag: an `enum class` with a valid `AnomalyTraits` table (GEN-043).
template <typename A>
concept Anomaly = std::is_enum_v<A> && requires {
                                           {
                                               AnomalyTraits<A>::flags.size()
                                               } -> std::convertible_to<std::size_t>;
                                       } && detail::valid_anomaly_table<A>();

namespace anomaly {

/// The bit index of `flag` (GEN-043).
template <Anomaly A> [[nodiscard]] constexpr std::uint8_t bit(A flag) noexcept {
    for (const AnomalyFlag<A>& f : AnomalyTraits<A>::flags) {
        if (f.flag == flag) {
            return f.bit;
        }
    }
    return capacity; // a value outside the enumerators: not representable
}

/// The flag with bit index `bit`, or `nullopt` if no flag uses it (GEN-043).
template <Anomaly A> [[nodiscard]] constexpr std::optional<A> from_bit(std::uint8_t bit) noexcept {
    for (const AnomalyFlag<A>& f : AnomalyTraits<A>::flags) {
        if (f.bit == bit) {
            return f.flag;
        }
    }
    return std::nullopt;
}

/// The single-bit mask for `flag`; 0 if it is not representable.
template <Anomaly A> [[nodiscard]] constexpr std::uint32_t mask(A flag) noexcept {
    return arith::shl(std::uint32_t{1}, anomaly::bit(flag));
}

} // namespace anomaly

/// A set of up to 32 anomaly flags of type `A`, stored as a 32-bit set (GEN-043).
template <Anomaly A> class AnomalySet {
public:
    /// Iterates the flags that are set, in ascending bit order.
    class Iterator {
    public:
        // The member types an iterator has, named as the standard library names them.
        /// Flags.
        using value_type = A; // NOLINT(readability-identifier-naming)
        /// Distance between iterators.
        using difference_type = std::ptrdiff_t; // NOLINT(readability-identifier-naming)

        /// The end iterator.
        constexpr Iterator() noexcept = default;

        /// The current flag.
        [[nodiscard]] constexpr A operator*() const noexcept { return current_; }

        /// Advances to the next flag that is set.
        constexpr Iterator& operator++() noexcept {
            advance();
            return *this;
        }

        /// Advances to the next flag that is set.
        constexpr Iterator operator++(int) noexcept {
            Iterator before = *this;
            advance();
            return before;
        }

        /// Equal when both have visited the same indices.
        friend constexpr bool operator==(const Iterator& a, const Iterator& b) noexcept {
            return a.next_ == b.next_ && a.bits_ == b.bits_;
        }

    private:
        friend class AnomalySet;

        constexpr explicit Iterator(std::uint32_t bits) noexcept : bits_{bits}, next_{0} {
            advance();
        }

        // Visits each index at most once: `next_` only rises, up to 32 (GEN-022).
        constexpr void advance() noexcept {
            while (next_ < anomaly::capacity) {
                const std::uint8_t index = next_;
                next_ =
                    arith::truncate<std::uint8_t>(arith::saturating_add(next_, std::uint8_t{1}));
                if ((bits_ & arith::shl(std::uint32_t{1}, index)) != 0U) {
                    // Only valid flags are ever inserted, and the table maps each index back
                    // (`Anomaly`), so the fallback is never taken.
                    current_ = anomaly::from_bit<A>(index).value_or(A{});
                    return;
                }
            }
            bits_ = 0; // the end
            next_ = anomaly::capacity;
        }

        std::uint32_t bits_{0};
        std::uint8_t next_{anomaly::capacity};
        A current_{};
    };

    /// The empty set, which every strict decode returns.
    constexpr AnomalySet() noexcept = default;

    /// The empty set.
    [[nodiscard]] static constexpr AnomalySet empty() noexcept { return AnomalySet{}; }

    /// The set holding each of `flags`.
    [[nodiscard]] static constexpr AnomalySet from_flags(std::span<const A> flags) noexcept {
        AnomalySet set;
        for (const A flag : flags) {
            set.insert(flag);
        }
        return set;
    }

    /// Adds `flag`.
    constexpr void insert(A flag) noexcept { bits_ |= anomaly::mask(flag); }

    /// Whether `flag` is set.
    [[nodiscard]] constexpr bool contains(A flag) const noexcept {
        return (bits_ & anomaly::mask(flag)) != 0U;
    }

    /// Whether no flag is set.
    [[nodiscard]] constexpr bool is_empty() const noexcept { return bits_ == 0; }

    /// Number of flags set (Rust `len`).
    [[nodiscard]] constexpr std::size_t size() const noexcept {
        return arith::widen<std::size_t>(arith::popcount(bits_));
    }

    /// The raw bit set: bit `n` is set when the flag with index `n` is present.
    [[nodiscard]] constexpr std::uint32_t bits() const noexcept { return bits_; }

    /// The first flag that is set.
    [[nodiscard]] constexpr Iterator begin() const noexcept { return Iterator{bits_}; }

    /// Past the last flag.
    [[nodiscard]] constexpr Iterator end() const noexcept { return Iterator{}; }

    friend constexpr bool operator==(AnomalySet, AnomalySet) noexcept = default;

private:
    std::uint32_t bits_{0};
};

} // namespace aldis::core

#endif // ALDIS_CORE_ANOMALY_HPP
