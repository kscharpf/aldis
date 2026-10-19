// Caller-supplied monotonic time: aldis-core `time.rs` (API-003).
//
// Engines never read clocks (GEN-031): the caller passes an `Instant`, an unsigned count of
// microseconds since an arbitrary epoch. Arithmetic saturates instead of wrapping, and time going
// backwards is treated as zero elapsed time.

#ifndef ALDIS_CORE_TIME_HPP
#define ALDIS_CORE_TIME_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/config.hpp"

#include <cstdint>
#include <limits>

namespace aldis::core {

/// A span of time in microseconds (API-003). Value-initialized, it is zero.
class Duration {
public:
    /// Zero duration.
    constexpr Duration() noexcept = default;

    /// Zero duration.
    [[nodiscard]] static constexpr Duration zero() noexcept { return Duration{}; }

    /// Largest duration.
    [[nodiscard]] static constexpr Duration max_value() noexcept {
        return from_micros(std::numeric_limits<std::uint64_t>::max());
    }

    /// A duration of `micros` microseconds.
    [[nodiscard]] static constexpr Duration from_micros(std::uint64_t micros) noexcept {
        Duration d;
        d.micros_ = micros;
        return d;
    }

    /// A duration of `millis` milliseconds, saturating.
    [[nodiscard]] static constexpr Duration from_millis(std::uint64_t millis) noexcept {
        return from_micros(arith::saturating_mul(millis, std::uint64_t{1'000}));
    }

    /// A duration of `secs` seconds, saturating.
    [[nodiscard]] static constexpr Duration from_secs(std::uint64_t secs) noexcept {
        return from_micros(arith::saturating_mul(secs, std::uint64_t{1'000'000}));
    }

    /// The duration in microseconds.
    [[nodiscard]] constexpr std::uint64_t as_micros() const noexcept { return micros_; }

    /// Sum, saturating.
    [[nodiscard]] constexpr Duration saturating_add(Duration other) const noexcept {
        return from_micros(arith::saturating_add(micros_, other.micros_));
    }

    friend constexpr bool operator==(Duration, Duration) noexcept = default;

private:
    std::uint64_t micros_{0};
};

/// A point in caller-supplied monotonic time, in microseconds since an arbitrary epoch (API-003).
/// Value-initialized, it is the epoch.
class Instant {
public:
    /// The epoch.
    constexpr Instant() noexcept = default;

    /// The instant `micros` microseconds after the epoch.
    [[nodiscard]] static constexpr Instant from_micros(std::uint64_t micros) noexcept {
        Instant t;
        t.micros_ = micros;
        return t;
    }

    /// Microseconds since the epoch.
    [[nodiscard]] constexpr std::uint64_t as_micros() const noexcept { return micros_; }

    /// This instant plus `d`, saturating at the largest representable instant.
    [[nodiscard]] constexpr Instant saturating_add(Duration d) const noexcept {
        return from_micros(arith::saturating_add(micros_, d.as_micros()));
    }

    /// Time elapsed since `earlier`; zero if `earlier` is later than this instant (API-003).
    [[nodiscard]] constexpr Duration duration_since(Instant earlier) const noexcept {
        return Duration::from_micros(arith::saturating_sub(micros_, earlier.micros_));
    }

    friend constexpr bool operator==(Instant, Instant) noexcept = default;

private:
    std::uint64_t micros_{0};
};

} // namespace aldis::core

#endif // ALDIS_CORE_TIME_HPP
