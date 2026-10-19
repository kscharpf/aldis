// Range-checked identifier types: aldis-core `ids.rs` (API-001, API-002, CPP-020, CPP-023).
//
// Identifiers have a private representation. A sub-width identifier is constructed only through
// its `static constexpr make` factory, which returns `Result`, so an encoder can never emit an
// out-of-range field; a full-width one has a `constexpr explicit` constructor. None converts
// implicitly to or from an integer (CPP-020). Widths that differ between protocols are template
// parameters (API-002): `SpacecraftId<10>` (TM, TC, AOS Issue 5), `SpacecraftId<8>` (AOS Issue
// ≤4), and `SpacecraftId<16>` (USLP). Rust's `get()` is `value()` here.

#ifndef ALDIS_CORE_IDS_HPP
#define ALDIS_CORE_IDS_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"

#include <cstdint>

namespace aldis::core {

namespace ids {

/// Width of the APID field, in bits (133.0 §4.1.3.3.4).
inline constexpr unsigned apid_bits = 11;
/// Width of the Packet Sequence Count field, in bits (133.0 §4.1.3.4.3).
inline constexpr unsigned seq_count_bits = 14;
/// Width of the TC Frame Sequence Number, in bits (232.0 §4.1.2.8).
inline constexpr unsigned frame_seq_bits = 8;
/// Width of the Packet Version Number field, in bits (133.0 §4.1.3.2).
inline constexpr unsigned packet_version_bits = 3;
/// Packet Version Number of the Encapsulation Packet, binary `111` (SANA registry; 133.1 §4.1.2.2).
inline constexpr std::uint8_t encapsulation_version = 0b111;

} // namespace ids

namespace detail {

/// An unsigned identifier of `Bits` bits (1 ≤ Bits ≤ digits of `Rep`), valid values
/// `0 … 2^Bits − 1` (the `width_id!` macro of aldis-core).
template <typename Self, typename Rep, unsigned Bits>
    requires(Bits >= 1 && Bits <= arith::digits<Rep>)
class WidthId {
public:
    /// Largest valid value, `2^Bits − 1`.
    static constexpr Rep max_value = arith::truncate<Rep>(arith::low_mask(Bits));

    /// The numeric value.
    [[nodiscard]] constexpr Rep value() const noexcept { return raw_; }

    friend constexpr bool operator==(Self a, Self b) noexcept { return a.value() == b.value(); }

private:
    friend Self;

    constexpr explicit WidthId(Rep raw) noexcept : raw_{raw} {}

    /// `Self` holding `raw`, or `OutOfRange` (field `field`) if it needs more than `Bits` bits.
    static constexpr Result<Self> make_checked(Rep raw, FieldId field) noexcept {
        if (raw > max_value) {
            return Error::out_of_range(field, max_value, raw);
        }
        return Self{raw};
    }

    Rep raw_;
};

} // namespace detail

/// Spacecraft Identifier of `Bits` width (132.0 §4.1.2.2.3, 232.0 §4.1.2.5, 732.0 §4.1.2.2.3,
/// 732.1 §4.1.2.2.3; API-001, API-002).
template <unsigned Bits>
class SpacecraftId : public detail::WidthId<SpacecraftId<Bits>, std::uint16_t, Bits> {
    using Base = detail::WidthId<SpacecraftId<Bits>, std::uint16_t, Bits>;
    friend Base;

public:
    /// The identifier, or `OutOfRange` (`spacecraft_id`) if `raw` needs more than `Bits` bits.
    [[nodiscard]] static constexpr Result<SpacecraftId> make(std::uint16_t raw) noexcept {
        return Base::make_checked(raw, FieldId{"spacecraft_id"});
    }

private:
    using Base::Base;
};

/// Virtual Channel Identifier of `Bits` width: 3 (TM) or 6 (TC, AOS, USLP) (API-001, API-002).
template <unsigned Bits> class VcId : public detail::WidthId<VcId<Bits>, std::uint8_t, Bits> {
    using Base = detail::WidthId<VcId<Bits>, std::uint8_t, Bits>;
    friend Base;

public:
    /// The identifier, or `OutOfRange` (`vc_id`) if `raw` needs more than `Bits` bits.
    [[nodiscard]] static constexpr Result<VcId> make(std::uint8_t raw) noexcept {
        return Base::make_checked(raw, FieldId{"vc_id"});
    }

private:
    using Base::Base;
};

/// Multiplexer Access Point Identifier of `Bits` width: 6 (TC Segment Header) or 4 (USLP)
/// (API-001, API-002).
template <unsigned Bits> class MapId : public detail::WidthId<MapId<Bits>, std::uint8_t, Bits> {
    using Base = detail::WidthId<MapId<Bits>, std::uint8_t, Bits>;
    friend Base;

public:
    /// The identifier, or `OutOfRange` (`map_id`) if `raw` needs more than `Bits` bits.
    [[nodiscard]] static constexpr Result<MapId> make(std::uint8_t raw) noexcept {
        return Base::make_checked(raw, FieldId{"map_id"});
    }

private:
    using Base::Base;
};

/// Application Process Identifier, 0–2047 (133.0 §4.1.3.3.4; API-001).
class Apid : public detail::WidthId<Apid, std::uint16_t, ids::apid_bits> {
    using Base = detail::WidthId<Apid, std::uint16_t, ids::apid_bits>;
    friend Base;

public:
    /// The APID, or `OutOfRange` (`apid`) if `raw` exceeds 11 bits.
    [[nodiscard]] static constexpr Result<Apid> make(std::uint16_t raw) noexcept {
        return Base::make_checked(raw, FieldId{"apid"});
    }

    /// APID reserved for Idle Packets: all ones (133.0 §4.1.3.3.4.4).
    [[nodiscard]] static constexpr Apid idle() noexcept { return Apid{max_value}; }

    /// Whether this is the Idle Packet APID.
    [[nodiscard]] constexpr bool is_idle() const noexcept { return value() == max_value; }

private:
    using Base::Base;
};

/// Space Packet Sequence Count or Packet Name, 0–16383, wrapping (133.0 §4.1.3.4.3; API-001).
/// Value-initialized, it is 0.
class SeqCount : public detail::WidthId<SeqCount, std::uint16_t, ids::seq_count_bits> {
    using Base = detail::WidthId<SeqCount, std::uint16_t, ids::seq_count_bits>;
    friend Base;

public:
    /// Count 0.
    constexpr SeqCount() noexcept : Base{0} {}

    /// The count, or `OutOfRange` (`sequence_count`) if `raw` exceeds 14 bits.
    [[nodiscard]] static constexpr Result<SeqCount> make(std::uint16_t raw) noexcept {
        return Base::make_checked(raw, FieldId{"sequence_count"});
    }

    /// The next count, modulo 16384 (133.0 §4.1.3.4.3.4; GEN-024).
    [[nodiscard]] constexpr SeqCount next() const noexcept {
        return SeqCount{arith::wrapping_add<ids::seq_count_bits>(value(), std::uint16_t{1})};
    }

    /// Classifies `next` as received after this count, modulo 16384 (133.0 §4.1.3.4.3, CMN-031).
    [[nodiscard]] constexpr Continuity classify(SeqCount next) const noexcept {
        return detail::classify_masked(ids::seq_count_bits, value(), next.value());
    }

    /// The count as a 14-bit `ModCounter` (CMN-030).
    [[nodiscard]] constexpr ModCounter counter() const noexcept {
        return detail::counter_from_valid(ids::seq_count_bits, value());
    }

private:
    using Base::Base;
};

/// TC Frame Sequence Number N(S), and COP-1 V(S) / V(R): 0–255, wrapping (232.0 §4.1.2.8;
/// API-001). Full width, so constructed directly (CPP-020). Value-initialized, it is 0.
class FrameSeqNum {
public:
    /// Largest value.
    static constexpr std::uint8_t max_value = 255;

    /// Number 0.
    constexpr FrameSeqNum() noexcept = default;

    /// The sequence number `raw` (every value fits in 8 bits).
    constexpr explicit FrameSeqNum(std::uint8_t raw) noexcept : raw_{raw} {}

    /// The numeric value.
    [[nodiscard]] constexpr std::uint8_t value() const noexcept { return raw_; }

    /// The next number, modulo 256 (GEN-024).
    [[nodiscard]] constexpr FrameSeqNum next() const noexcept {
        return FrameSeqNum{arith::wrapping_add<ids::frame_seq_bits>(raw_, std::uint8_t{1})};
    }

    /// Classifies `next` as received after this number, modulo 256 (CMN-031).
    [[nodiscard]] constexpr Continuity classify(FrameSeqNum next) const noexcept {
        return detail::classify_masked(ids::frame_seq_bits, raw_, next.raw_);
    }

    /// The number as an 8-bit `ModCounter` (CMN-030).
    [[nodiscard]] constexpr ModCounter counter() const noexcept {
        return detail::counter_from_valid(ids::frame_seq_bits, raw_);
    }

    friend constexpr bool operator==(FrameSeqNum, FrameSeqNum) noexcept = default;

private:
    std::uint8_t raw_{0};
};

/// Packet Version Number, 0–7 (SANA Packet Version Number registry; 133.0 §4.1.3.2, 133.1
/// §4.1.2.2; API-001).
class PacketVersion
    : public detail::WidthId<PacketVersion, std::uint8_t, ids::packet_version_bits> {
    using Base = detail::WidthId<PacketVersion, std::uint8_t, ids::packet_version_bits>;
    friend Base;

public:
    /// The version, or `OutOfRange` (`packet_version`) if `raw` exceeds 3 bits.
    [[nodiscard]] static constexpr Result<PacketVersion> make(std::uint8_t raw) noexcept {
        return Base::make_checked(raw, FieldId{"packet_version"});
    }

    /// Version-1 CCSDS Packet (Space Packet), binary `000`.
    [[nodiscard]] static constexpr PacketVersion space_packet() noexcept {
        return PacketVersion{0};
    }

    /// Version-8 CCSDS Packet (Encapsulation Packet), binary `111`.
    [[nodiscard]] static constexpr PacketVersion encapsulation() noexcept {
        return PacketVersion{ids::encapsulation_version};
    }

private:
    using Base::Base;
};

} // namespace aldis::core

#endif // ALDIS_CORE_IDS_HPP
