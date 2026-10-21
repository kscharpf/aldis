// Check of the `frame_layout` operation of test-vectors/core/layout.json (CMN-040, CMN-041,
// API-070, API-075; CPP-053, TV-012). Counterpart of `handle_layout` in
// crates/aldis-core/tests/vectors.rs.

#ifndef ALDIS_VECTORS_CHECKS_FRAME_LAYOUT_HPP
#define ALDIS_VECTORS_CHECKS_FRAME_LAYOUT_HPP

#include "aldis/core/bytes.hpp"
#include "aldis/core/error.hpp"
#include "aldis/core/layout.hpp"
#include "aldis/core/security.hpp"
#include "aldis/vectors/case.hpp"

#include <array>
#include <cstddef>
#include <span>

namespace aldis::vectors {

/// One expected `[start, end)` pair of a layout, as the vector gives it.
struct LayoutRange {
    /// First offset.
    std::size_t start{};
    /// One past the last offset.
    std::size_t end{};

    friend constexpr bool operator==(LayoutRange, LayoutRange) noexcept = default;
};

/// The seven ranges of a frame layout in wire order: `primary_header`, `secondary`,
/// `security_header`, `data`, `security_trailer`, `ocf`, `fecf` (CMN-040).
using LayoutRanges = std::array<LayoutRange, core::layout::field_count>;

/// `frame_layout`: the field ranges of a frame of the given lengths.
struct FrameLayoutCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("frame_layout");

    /// `params.frame_len`.
    std::size_t frame_len{};
    /// `params.primary_header_len`.
    std::size_t primary_header_len{};
    /// `params.secondary_len`.
    std::size_t secondary_len{};
    /// `params.security_header_len`.
    std::size_t security_header_len{};
    /// `params.security_trailer_len`.
    std::size_t security_trailer_len{};
    /// `params.ocf`.
    bool ocf{};
    /// `params.fecf`.
    bool fecf{};
    /// The seven ranges, or the error.
    Expect<LayoutRanges> expect;
};

/// The ranges `aldis::core::FrameLayout::compute` finds.
[[nodiscard]] constexpr core::Result<LayoutRanges> run(const FrameLayoutCase& c) noexcept {
    const core::Result<core::SecurityLayout> security =
        core::SecurityLayout::make(c.security_header_len, c.security_trailer_len);
    if (security.error() != nullptr) {
        return *security.error();
    }
    const core::Result<core::FrameLayout> layout = core::FrameLayout::compute(core::LayoutSpec{
        .frame_len = c.frame_len,
        .primary_header_len = c.primary_header_len,
        .secondary_len = c.secondary_len,
        .security = *security.ok(),
        .ocf = c.ocf,
        .fecf = c.fecf,
    });
    if (layout.error() != nullptr) {
        return *layout.error();
    }
    LayoutRanges out{};
    std::size_t index = 0;
    for (const core::OctetRange r : layout.ok()->ranges()) {
        // ranges() has as many entries as LayoutRanges.
        if (!core::bytes::put(std::span<LayoutRange>{out}, index,
                              LayoutRange{.start = r.start(), .end = r.end()})) {
            return core::Error{core::ErrorKind::CapacityExceeded};
        }
        ++index;
    }
    return out;
}

/// The verdict on `c` (TV-012).
[[nodiscard]] constexpr Verdict check(const FrameLayoutCase& c) noexcept {
    return compare(c.expect, run(c));
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_FRAME_LAYOUT_HPP
