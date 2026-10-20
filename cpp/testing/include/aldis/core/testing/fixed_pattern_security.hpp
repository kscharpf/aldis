// Test-only security hook: aldis-core `FixedPatternSecurity` (API-073). Built only with the
// CMake option ALDIS_TEST_HOOKS or the tests, the counterpart of the `test-hooks` feature, and
// never installed. Never use it in a flight build.

#ifndef ALDIS_CORE_TESTING_FIXED_PATTERN_SECURITY_HPP
#define ALDIS_CORE_TESTING_FIXED_PATTERN_SECURITY_HPP

#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"
#include "aldis/core/security.hpp"

#include <cstddef>
#include <span>

namespace aldis::core::testing {

/// Writes `0xA5` into the Security Header and `0x5A` into the Trailer, and checks them on
/// verify. Exercises length accounting and field placement (API-073).
class FixedPatternSecurity {
public:
    /// Pattern written into every Security Header octet.
    static constexpr std::byte header_pattern{0xA5};
    /// Pattern written into every Security Trailer octet.
    static constexpr std::byte trailer_pattern{0x5A};

    /// Fills the header and trailer; `Security` and no write if either lies outside `frame`.
    [[nodiscard]] static constexpr Result<void> protect(const FrameCtx& /*ctx*/,
                                                        std::span<std::byte> frame,
                                                        const SecuredFields& fields) noexcept {
        const Result<std::span<std::byte>> header = bytes::subspan(frame, fields.header());
        const Result<std::span<std::byte>> trailer = bytes::subspan(frame, fields.trailer());
        if (header.ok() == nullptr || trailer.ok() == nullptr) {
            return Error{ErrorKind::Security};
        }
        bytes::fill(*header.ok(), header_pattern);
        bytes::fill(*trailer.ok(), trailer_pattern);
        return {};
    }

    /// Checks the header and trailer patterns and returns the data field; `Security` otherwise.
    [[nodiscard]] static constexpr Result<OctetRange> verify(const FrameCtx& /*ctx*/,
                                                             std::span<std::byte> frame,
                                                             const SecuredFields& fields) noexcept {
        if (!holds(frame, fields.header(), header_pattern) ||
            !holds(frame, fields.trailer(), trailer_pattern)) {
            return Error{ErrorKind::Security};
        }
        return fields.data();
    }

private:
    static constexpr bool holds(std::span<const std::byte> frame, OctetRange range,
                                std::byte pattern) noexcept {
        const Result<std::span<const std::byte>> part = bytes::subspan(frame, range);
        if (part.ok() == nullptr) {
            return false;
        }
        for (const std::byte b : *part.ok()) {
            if (b != pattern) {
                return false;
            }
        }
        return true;
    }
};

static_assert(SecurityHook<FixedPatternSecurity>);

} // namespace aldis::core::testing

#endif // ALDIS_CORE_TESTING_FIXED_PATTERN_SECURITY_HPP
