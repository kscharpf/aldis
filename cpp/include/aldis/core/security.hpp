// SDLS reservation: `SecurityLayout` and the `SecurityHook` concept: aldis-core `security.rs`
// (API-070…API-075, CPP-023, CPP-024, CPP-025).
//
// v1 implements no cryptography. Every frame codec reserves the Security Header and Trailer of the
// Space Data Link Security Protocol (355.0-B-2 §4.1) through these types, so SDLS can be added
// later without breaking APIs. Engines call the hook's `protect` after every other frame field
// except the FECF is final, then compute the FECF; on reception the FECF is checked before
// `verify` (API-074; 355.0 §2.3, 232.0 Cor. 1 §6.5.2). Hooks are a template parameter
// constrained by `SecurityHook`, without virtual dispatch (CPP-024).

#ifndef ALDIS_CORE_SECURITY_HPP
#define ALDIS_CORE_SECURITY_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"
#include "aldis/core/ids.hpp"

#include <concepts>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>

namespace aldis::core {

namespace security {

/// Identifier widths per protocol, in bits (API-002): TM SCID and VCID (132.0 §4.1.2.2–3), TC
/// SCID, VCID, and MAP ID (232.0 §4.1.2.5–6, §4.2.2.2), AOS (732.0 §4.1.2.2.3), USLP (732.1
/// §4.1.2.2.3, §4.1.2.4–5).
inline constexpr unsigned tm_scid_bits = 10;
inline constexpr unsigned tm_vcid_bits = 3;
inline constexpr unsigned tc_scid_bits = 10;
inline constexpr unsigned tc_vcid_bits = 6;
inline constexpr unsigned tc_map_id_bits = 6;
inline constexpr unsigned aos_scid_bits = 10;
inline constexpr unsigned aos_vcid_bits = 6;
inline constexpr unsigned uslp_scid_bits = 16;
inline constexpr unsigned uslp_vcid_bits = 6;
inline constexpr unsigned uslp_map_id_bits = 4;

/// Security Header and Trailer of SDLS on USLP truncated frames, in octets (732.1 Annex D1.2.3).
inline constexpr std::uint8_t uslp_truncated_header_len = 6;
inline constexpr std::uint16_t uslp_truncated_trailer_len = 16;

} // namespace security

/// Lengths of the Security Header and Trailer on one VC or MAP (API-070, API-071). A
/// configuration type: private data, a validating factory, trivially copyable (CPP-025).
/// Value-initialized, it is `none()`.
class SecurityLayout {
public:
    /// Largest Security Header, in octets (355.0 §4.1.1.1.4; API-070, API-071).
    static constexpr std::size_t max_header_len = 64;
    /// Largest Security Trailer, in octets (API-070, API-071).
    static constexpr std::size_t max_trailer_len = 0xFFFF;

    /// No Security Header or Trailer.
    constexpr SecurityLayout() noexcept = default;

    /// No Security Header or Trailer.
    [[nodiscard]] static constexpr SecurityLayout none() noexcept { return SecurityLayout{}; }

    /// Fixed layout for SDLS on USLP truncated frames: 6-octet header, 16-octet trailer
    /// (732.1 Annex D1.2.3; API-075).
    [[nodiscard]] static constexpr SecurityLayout uslp_truncated() noexcept {
        return SecurityLayout{security::uslp_truncated_header_len,
                              security::uslp_truncated_trailer_len};
    }

    /// A layout with the given lengths; `InvalidConfig` if the header exceeds 64 octets (field
    /// `security_header_len`) or the trailer 65 535 octets (`security_trailer_len`).
    [[nodiscard]] static constexpr Result<SecurityLayout> make(std::size_t header_len,
                                                               std::size_t trailer_len) noexcept {
        const std::optional<std::uint8_t> header =
            arith::narrow_to_max<std::uint8_t>(header_len, max_header_len);
        if (!header.has_value()) {
            return Error{ErrorKind::InvalidConfig}.with_field(FieldId{"security_header_len"});
        }
        const std::optional<std::uint16_t> trailer =
            arith::narrow_to_max<std::uint16_t>(trailer_len, max_trailer_len);
        if (!trailer.has_value()) {
            return Error{ErrorKind::InvalidConfig}.with_field(FieldId{"security_trailer_len"});
        }
        return SecurityLayout{*header, *trailer};
    }

    /// Security Header length in octets.
    [[nodiscard]] constexpr std::size_t header_len() const noexcept {
        return arith::widen<std::size_t>(header_len_);
    }

    /// Security Trailer length in octets.
    [[nodiscard]] constexpr std::size_t trailer_len() const noexcept {
        return arith::widen<std::size_t>(trailer_len_);
    }

    /// Whether both lengths are zero.
    [[nodiscard]] constexpr bool is_none() const noexcept {
        return header_len_ == 0 && trailer_len_ == 0;
    }

    friend constexpr bool operator==(SecurityLayout, SecurityLayout) noexcept = default;

private:
    constexpr SecurityLayout(std::uint8_t header_len, std::uint16_t trailer_len) noexcept
        : header_len_{header_len}, trailer_len_{trailer_len} {}

    std::uint8_t header_len_{0};
    std::uint16_t trailer_len_{0};
};

/// Data link protocol of a frame (355.0-B-2 §4.1; API-073, GEN-081).
enum class Protocol : std::uint8_t {
    /// TM Space Data Link Protocol (132.0-B-3).
    Tm,
    /// TC Space Data Link Protocol (232.0-B-4).
    Tc,
    /// AOS Space Data Link Protocol (732.0-B-5).
    Aos,
    /// Unified Space Data Link Protocol (732.1-B-3).
    Uslp,
};

/// Identifies the channel a frame belongs to, for selecting a Security Association (355.0-B-2
/// §2.2.3, §4.1; API-073).
///
/// One factory per protocol takes that protocol's identifier widths, so a context holds only
/// identifiers its protocol allows (API-002, CPP-023).
class FrameCtx {
public:
    /// TM frame: 10-bit SCID, 3-bit VCID (132.0 §4.1.2.2–3).
    [[nodiscard]] static constexpr FrameCtx tm(SpacecraftId<security::tm_scid_bits> scid,
                                               VcId<security::tm_vcid_bits> vcid) noexcept {
        return FrameCtx{Protocol::Tm, scid.value(), vcid.value(), std::nullopt};
    }

    /// TC frame: 10-bit SCID, 6-bit VCID, and the MAP ID when SDLS is applied per MAP (232.0
    /// §4.1.2.5–6, §4.2.2.2).
    [[nodiscard]] static constexpr FrameCtx
    tc(SpacecraftId<security::tc_scid_bits> scid, VcId<security::tc_vcid_bits> vcid,
       std::optional<MapId<security::tc_map_id_bits>> map_id) noexcept {
        return FrameCtx{Protocol::Tc, scid.value(), vcid.value(), map_value(map_id)};
    }

    /// AOS frame: 6-bit VCID and a 10-bit SCID (732.0 §4.1.2.2.3); Issue ≤4 SCIDs are 8 bits and
    /// fit unchanged.
    [[nodiscard]] static constexpr FrameCtx aos(SpacecraftId<security::aos_scid_bits> scid,
                                                VcId<security::aos_vcid_bits> vcid) noexcept {
        return FrameCtx{Protocol::Aos, scid.value(), vcid.value(), std::nullopt};
    }

    /// USLP frame: 16-bit SCID, 6-bit VCID, and the MAP ID when SDLS is applied per MAP (732.1
    /// §4.1.2.2.3, §4.1.2.4–5).
    [[nodiscard]] static constexpr FrameCtx
    uslp(SpacecraftId<security::uslp_scid_bits> scid, VcId<security::uslp_vcid_bits> vcid,
         std::optional<MapId<security::uslp_map_id_bits>> map_id) noexcept {
        return FrameCtx{Protocol::Uslp, scid.value(), vcid.value(), map_value(map_id)};
    }

    /// The data link protocol (API-073).
    [[nodiscard]] constexpr Protocol protocol() const noexcept { return protocol_; }

    /// The Spacecraft ID as a number (API-073).
    [[nodiscard]] constexpr std::uint16_t scid() const noexcept { return scid_; }

    /// The Virtual Channel ID as a number (API-073).
    [[nodiscard]] constexpr std::uint8_t vcid() const noexcept { return vcid_; }

    /// The MAP ID as a number, for TC and USLP contexts that carry one (API-073).
    [[nodiscard]] constexpr std::optional<std::uint8_t> map_id() const noexcept { return map_id_; }

    friend constexpr bool operator==(const FrameCtx&, const FrameCtx&) noexcept = default;

private:
    template <typename Map>
    static constexpr std::optional<std::uint8_t> map_value(std::optional<Map> map_id) noexcept {
        if (map_id.has_value()) {
            return map_id->value();
        }
        return std::nullopt;
    }

    constexpr FrameCtx(Protocol protocol, std::uint16_t scid, std::uint8_t vcid,
                       std::optional<std::uint8_t> map_id) noexcept
        : protocol_{protocol}, scid_{scid}, vcid_{vcid}, map_id_{map_id} {}

    Protocol protocol_;
    std::uint16_t scid_;
    std::uint8_t vcid_;
    std::optional<std::uint8_t> map_id_;
};

class FrameLayout;

/// Octet ranges of the secured fields within a frame buffer: Security Header, data field, and
/// Security Trailer, in order and adjacent (355.0-B-2 §4.1; API-070, API-073, CPP-023).
class SecuredFields {
public:
    /// The secured fields from three ranges; `InvalidField` (`secured_fields`) unless the data
    /// field starts where the header ends and the trailer starts where the data field ends.
    [[nodiscard]] static constexpr Result<SecuredFields> make(OctetRange header, OctetRange data,
                                                              OctetRange trailer) noexcept {
        if (header.end() != data.start() || data.end() != trailer.start()) {
            return Error{ErrorKind::InvalidField}.with_field(FieldId{"secured_fields"});
        }
        return SecuredFields{header, data, trailer};
    }

    /// Security Header (355.0-B-2 §4.1.1).
    [[nodiscard]] constexpr OctetRange header() const noexcept { return header_; }

    /// Data field protected by the hook (355.0-B-2 §4.1).
    [[nodiscard]] constexpr OctetRange data() const noexcept { return data_; }

    /// Security Trailer (355.0-B-2 §4.1.2).
    [[nodiscard]] constexpr OctetRange trailer() const noexcept { return trailer_; }

    friend constexpr bool operator==(SecuredFields, SecuredFields) noexcept = default;

private:
    friend class FrameLayout; // its ranges are adjacent by construction (CMN-040)

    constexpr SecuredFields(OctetRange header, OctetRange data, OctetRange trailer) noexcept
        : header_{header}, data_{data}, trailer_{trailer} {}

    OctetRange header_;
    OctetRange data_;
    OctetRange trailer_;
};

/// Applies and checks link-layer security (API-073, API-074, CPP-024).
///
/// `protect` fills the Security Header and Trailer and transforms the data field in place;
/// `Security` if protection fails, and the frame must then not be transmitted. `verify` checks
/// the Security Header and Trailer, transforms the data field in place, and returns the range of
/// the plain data; `Security` if verification fails, and the frame must then be discarded.
template <typename H>
concept SecurityHook = requires(H& hook, const FrameCtx& ctx, std::span<std::byte> frame,
                                const SecuredFields& fields) {
    { hook.protect(ctx, frame, fields) } noexcept -> std::same_as<Result<void>>;
    { hook.verify(ctx, frame, fields) } noexcept -> std::same_as<Result<OctetRange>>;
};

/// The identity hook for channels without SDLS (layout 0/0; API-073).
class NoSecurity {
public:
    /// Does nothing.
    [[nodiscard]] static constexpr Result<void> protect(const FrameCtx& /*ctx*/,
                                                        std::span<std::byte> /*frame*/,
                                                        const SecuredFields& /*fields*/) noexcept {
        return {};
    }

    /// Returns the data field unchanged.
    [[nodiscard]] static constexpr Result<OctetRange> verify(const FrameCtx& /*ctx*/,
                                                             std::span<std::byte> /*frame*/,
                                                             const SecuredFields& fields) noexcept {
        return fields.data();
    }

    friend constexpr bool operator==(NoSecurity, NoSecurity) noexcept = default;
};

static_assert(SecurityHook<NoSecurity>);

} // namespace aldis::core

#endif // ALDIS_CORE_SECURITY_HPP
