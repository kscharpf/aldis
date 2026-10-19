// Frame field layout calculator: aldis-core `layout.rs` (CMN-040, CMN-041).
//
// All four data link protocols place their fields in the same order, so one calculator serves
// them all (132.0 §4.1, §6.3; 232.0 §4.1, §6.3; 732.0 §4.1, §6.3; 732.1 §4.1, §6.3):
//
//   primary header | secondary | security header | data | security trailer | OCF | FECF
//
// The secondary slot is protocol-specific: the TM Transfer Frame Secondary Header, the AOS or
// USLP Insert Zone, or the TC Segment Header. TC frames have no OCF. `OctetRange`, the
// counterpart of `Range<usize>`, is defined in bytes.hpp.

#ifndef ALDIS_CORE_LAYOUT_HPP
#define ALDIS_CORE_LAYOUT_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"
#include "aldis/core/security.hpp"

#include <array>
#include <cstddef>

namespace aldis::core {

namespace layout {

/// Length of the Operational Control Field, in octets (CMN-040).
inline constexpr std::size_t ocf_len = 4;
/// Length of the Frame Error Control Field, in octets (CMN-040).
inline constexpr std::size_t fecf_len = 2;
/// Number of fields in a frame layout, in wire order (CMN-040).
inline constexpr std::size_t field_count = 7;

} // namespace layout

/// Inputs to the layout of one frame (CMN-040). Plain data: every combination is representable,
/// and `FrameLayout::compute` checks it.
struct LayoutSpec {
    /// Total frame length in octets.
    std::size_t frame_len{0};
    /// Primary header length in octets.
    std::size_t primary_header_len{0};
    /// Length of the protocol-specific secondary slot (FSH, Insert Zone, or Segment Header).
    std::size_t secondary_len{0};
    /// Security Header and Trailer lengths (API-070).
    SecurityLayout security;
    /// Whether the OCF is present.
    bool ocf{false};
    /// Whether the FECF is present.
    bool fecf{false};

    friend constexpr bool operator==(const LayoutSpec&, const LayoutSpec&) noexcept = default;
};

/// Contiguous, non-overlapping octet ranges covering a frame exactly, in wire order (CMN-040).
///
/// Built only by `compute`, so holding one proves the ranges start at 0, follow each other
/// without gaps, and leave a non-empty data field (CMN-041, CPP-023).
class FrameLayout {
public:
    /// Computes the layout; `InvalidConfig` (field `data`) if the other fields leave no octet for
    /// the data field (CMN-041).
    [[nodiscard]] static constexpr Result<FrameLayout> compute(const LayoutSpec& spec) noexcept {
        const std::size_t ocf = spec.ocf ? layout::ocf_len : 0;
        const std::size_t fecf = spec.fecf ? layout::fecf_len : 0;
        const std::size_t trailer_len =
            arith::saturating_add(arith::saturating_add(spec.security.trailer_len(), ocf), fecf);
        const std::size_t head_len = arith::saturating_add(
            arith::saturating_add(spec.primary_header_len, spec.secondary_len),
            spec.security.header_len());
        const std::size_t data_len =
            arith::saturating_sub(arith::saturating_sub(spec.frame_len, head_len), trailer_len);
        if (head_len >= spec.frame_len ||
            trailer_len >= arith::saturating_sub(spec.frame_len, head_len)) {
            return Error{ErrorKind::InvalidConfig}.with_field(FieldId{"data"});
        }
        FrameLayout l;
        std::size_t at = 0;
        l.primary_header_ = take(at, spec.primary_header_len);
        l.secondary_ = take(at, spec.secondary_len);
        l.security_header_ = take(at, spec.security.header_len());
        l.data_ = take(at, data_len);
        l.security_trailer_ = take(at, spec.security.trailer_len());
        l.ocf_ = take(at, ocf);
        l.fecf_ = take(at, fecf);
        return l;
    }

    /// Primary header (CMN-040).
    [[nodiscard]] constexpr OctetRange primary_header() const noexcept { return primary_header_; }

    /// Secondary header, Insert Zone, or Segment Header; empty if absent (CMN-040).
    [[nodiscard]] constexpr OctetRange secondary() const noexcept { return secondary_; }

    /// Security Header; empty without SDLS (CMN-040, API-070).
    [[nodiscard]] constexpr OctetRange security_header() const noexcept { return security_header_; }

    /// Data field, never empty (CMN-040, CMN-041).
    [[nodiscard]] constexpr OctetRange data() const noexcept { return data_; }

    /// Security Trailer; empty without SDLS (CMN-040, API-070).
    [[nodiscard]] constexpr OctetRange security_trailer() const noexcept {
        return security_trailer_;
    }

    /// Operational Control Field; empty if absent (CMN-040).
    [[nodiscard]] constexpr OctetRange ocf() const noexcept { return ocf_; }

    /// Frame Error Control Field; empty if absent (CMN-040).
    [[nodiscard]] constexpr OctetRange fecf() const noexcept { return fecf_; }

    /// Frame length: the end of the last range (CMN-040).
    [[nodiscard]] constexpr std::size_t frame_len() const noexcept { return fecf_.end(); }

    /// The ranges in wire order (CMN-040).
    [[nodiscard]] constexpr std::array<OctetRange, layout::field_count> ranges() const noexcept {
        return {
            primary_header_, secondary_, security_header_, data_, security_trailer_, ocf_, fecf_};
    }

    /// The secured fields of this layout, adjacent by construction (CMN-040, API-070; Rust
    /// `SecuredFields::from_layout`).
    [[nodiscard]] constexpr SecuredFields secured_fields() const noexcept {
        return SecuredFields{security_header_, data_, security_trailer_};
    }

    friend constexpr bool operator==(const FrameLayout&, const FrameLayout&) noexcept = default;

private:
    constexpr FrameLayout() noexcept = default;

    static constexpr OctetRange take(std::size_t& at, std::size_t len) noexcept {
        const OctetRange range = OctetRange::from_len(at, len);
        at = range.end();
        return range;
    }

    OctetRange primary_header_;
    OctetRange secondary_;
    OctetRange security_header_;
    OctetRange data_;
    OctetRange security_trailer_;
    OctetRange ocf_;
    OctetRange fecf_;
};

} // namespace aldis::core

#endif // ALDIS_CORE_LAYOUT_HPP
