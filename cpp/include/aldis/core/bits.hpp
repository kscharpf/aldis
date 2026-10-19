// Big-endian bit-field read/write primitives: aldis-core `bits.rs` (GEN-050, GEN-051, CPP-033).
//
// CCSDS convention (e.g. 133.0-B-2 §1.6.3): Bit 0 of a field is the first transmitted bit and the
// most significant bit; multi-octet fields are big-endian on the wire. All codecs access wire
// fields through this module, never through C++ bit-field members or by casting structs onto
// octets (CPP-033). With arith.hpp, it is the only place with shifts (D10).
//
// A field is addressed by its bit offset from the start of the buffer and its width in bits
// (1–64). Every function is total: out-of-range arguments return `OutOfRange` and never have
// undefined behavior (GEN-020). The Rust version accumulates the octets in a 128-bit integer;
// C++ has none, so each octet contributes only the bits that belong to the field, which never
// needs more than 64 bits.

#ifndef ALDIS_CORE_BITS_HPP
#define ALDIS_CORE_BITS_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <cstddef>
#include <cstdint>
#include <span>

namespace aldis::core::bits {

/// Widest field, in bits.
inline constexpr std::uint8_t max_width = 64;

namespace detail {

/// Mask of the bit position within an octet.
inline constexpr std::size_t octet_bit_mask = arith::octet_bits - 1U;

/// Octets covered by a field: the first and last octet, and the bits of the first octet before
/// the field (`lead`) and of the last octet after it (`trail`).
struct FieldSpan {
    std::size_t first;
    std::size_t last;
    unsigned lead;
    unsigned trail;
};

/// Validates a field of `width` bits at `bit_offset` in a buffer of `buf_len` octets
/// (aldis-core `Span::new`).
constexpr Result<FieldSpan> field_span(std::size_t buf_len, std::size_t bit_offset,
                                       std::uint8_t width) noexcept {
    if (width == 0) {
        return Error::out_of_range(FieldId{"width"}, 1, 0);
    }
    if (width > max_width) {
        return Error::out_of_range(FieldId{"width"}, max_width, width);
    }
    const std::size_t total_bits = arith::saturating_mul(buf_len, std::size_t{arith::octet_bits});
    const auto width_bits = arith::widen<std::size_t>(width);
    const Result<std::size_t> end = arith::checked_add(bit_offset, width_bits);
    if (end.ok() == nullptr || *end.ok() > total_bits) {
        const Result<std::size_t> max_offset = arith::checked_sub(total_bits, width_bits);
        if (max_offset.ok() != nullptr) {
            return Error::out_of_range(FieldId{"bit_offset"},
                                       arith::widen<std::uint64_t>(*max_offset.ok()),
                                       arith::widen<std::uint64_t>(bit_offset));
        }
        return Error::out_of_range(FieldId{"buf_bits"}, arith::widen<std::uint64_t>(width_bits),
                                   arith::widen<std::uint64_t>(total_bits));
    }
    // end >= width >= 1 and end <= total_bits, so every value below is in range.
    const std::size_t last_bit = arith::saturating_sub(*end.ok(), std::size_t{1});
    return FieldSpan{
        .first = arith::shr(bit_offset, 3U),
        .last = arith::shr(last_bit, 3U),
        .lead = arith::truncate<unsigned>(arith::bit_and(bit_offset, octet_bit_mask)),
        .trail = arith::saturating_sub(
            arith::octet_bits - 1U,
            arith::truncate<unsigned>(arith::bit_and(last_bit, octet_bit_mask))),
    };
}

/// The octets the field covers: never empty for a valid span.
template <typename Byte>
constexpr std::span<Byte> covered(std::span<Byte> buf, const FieldSpan& span) noexcept {
    return bytes::subspan_or_empty(
        buf, span.first,
        arith::saturating_add(arith::saturating_sub(span.last, span.first), std::size_t{1}));
}

/// Bits of octet `index` that belong to the field: the count, and the shift that right-aligns
/// them.
struct OctetPart {
    unsigned count;
    unsigned shift;
};

constexpr OctetPart octet_part(const FieldSpan& span, std::size_t index) noexcept {
    const unsigned high = index == span.first ? span.lead : 0U;
    const unsigned low = index == span.last ? span.trail : 0U;
    return OctetPart{
        .count = arith::saturating_sub(arith::saturating_sub(arith::octet_bits, high), low),
        .shift = low,
    }; // high + low <= 7
}

} // namespace detail

/// Reads the `width`-bit field at `bit_offset` as an unsigned value (GEN-050, GEN-051).
///
/// Errors: `OutOfRange` if `width` is not 1–64 (field `width`) or the field extends past the
/// buffer (`bit_offset`, or `buf_bits` if the buffer is shorter than the field).
[[nodiscard]] constexpr Result<std::uint64_t>
get(std::span<const std::byte> buf, std::size_t bit_offset, std::uint8_t width) noexcept {
    const Result<detail::FieldSpan> span = detail::field_span(buf.size(), bit_offset, width);
    if (span.error() != nullptr) {
        return *span.error();
    }
    const detail::FieldSpan& s = *span.ok();
    std::uint64_t value = 0;
    std::size_t index = s.first;
    for (const std::byte octet : detail::covered(buf, s)) {
        const detail::OctetPart part = detail::octet_part(s, index);
        const std::uint64_t field_bits = arith::bit_and(
            arith::shr(arith::widen<std::uint64_t>(std::to_integer<std::uint8_t>(octet)),
                       part.shift),
            arith::low_mask(part.count));
        value = arith::bit_or(arith::shl(value, part.count), field_bits);
        index = arith::saturating_add(index, std::size_t{1});
    }
    return value;
}

/// Writes `value` into the `width`-bit field at `bit_offset`, leaving all other bits unchanged
/// (GEN-050, GEN-051).
///
/// Errors: as `get`, and `OutOfRange` (field `value`) if `value` does not fit in `width` bits.
/// On error the buffer is unchanged (GEN-042).
[[nodiscard]] constexpr Result<void> set(std::span<std::byte> buf, std::size_t bit_offset,
                                         std::uint8_t width, std::uint64_t value) noexcept {
    const Result<detail::FieldSpan> span = detail::field_span(buf.size(), bit_offset, width);
    if (span.error() != nullptr) {
        return *span.error();
    }
    const std::uint64_t mask = arith::low_mask(width);
    if (arith::bit_and(value, arith::bit_not(mask)) != 0U) {
        return Error::out_of_range(FieldId{"value"}, mask, value);
    }
    const detail::FieldSpan& s = *span.ok();
    unsigned remaining = width; // bits of `value` not yet written, counted from the low end
    std::size_t index = s.first;
    for (std::byte& octet : detail::covered(buf, s)) {
        const detail::OctetPart part = detail::octet_part(s, index);
        remaining = arith::saturating_sub(remaining, part.count); // the parts add up to `width`
        const std::uint64_t chunk =
            arith::bit_and(arith::shr(value, remaining), arith::low_mask(part.count));
        const auto keep = arith::bit_not(
            arith::truncate<std::uint8_t>(arith::shl(arith::low_mask(part.count), part.shift)));
        const auto updated =
            arith::bit_or(arith::bit_and(std::to_integer<std::uint8_t>(octet), keep),
                          arith::truncate<std::uint8_t>(arith::shl(chunk, part.shift)));
        octet = std::byte{updated};
        index = arith::saturating_add(index, std::size_t{1});
    }
    return {};
}

namespace detail {

/// `OutOfRange` for `width` octets at `offset` in a buffer of `buf_len` octets
/// (aldis-core `octet_err`).
constexpr Error octet_error(std::size_t buf_len, std::size_t offset, std::size_t width) noexcept {
    const Result<std::size_t> max_offset = arith::checked_sub(buf_len, width);
    if (max_offset.ok() != nullptr) {
        return Error::out_of_range(FieldId{"offset"}, arith::widen<std::uint64_t>(*max_offset.ok()),
                                   arith::widen<std::uint64_t>(offset));
    }
    return Error::out_of_range(FieldId{"buf_len"}, arith::widen<std::uint64_t>(width),
                               arith::widen<std::uint64_t>(buf_len));
}

template <arith::Unsigned T>
constexpr Result<T> read_be(std::span<const std::byte> buf, std::size_t offset) noexcept {
    const Result<std::span<const std::byte>> octets = bytes::subspan(buf, offset, sizeof(T));
    if (octets.error() != nullptr) {
        return octet_error(buf.size(), offset, sizeof(T));
    }
    ByteReader reader{*octets.ok()};
    if constexpr (sizeof(T) == 2) {
        return reader.read_u16();
    } else if constexpr (sizeof(T) == 4) {
        return reader.read_u32();
    } else {
        return reader.read_u64();
    }
}

template <arith::Unsigned T>
constexpr Result<void> write_be(std::span<std::byte> buf, std::size_t offset, T value) noexcept {
    const Result<std::span<std::byte>> octets = bytes::subspan(buf, offset, sizeof(T));
    if (octets.error() != nullptr) {
        return octet_error(buf.size(), offset, sizeof(T));
    }
    ByteWriter writer{*octets.ok()};
    if constexpr (sizeof(T) == 2) {
        return writer.write_u16(value);
    } else if constexpr (sizeof(T) == 4) {
        return writer.write_u32(value);
    } else {
        return writer.write_u64(value);
    }
}

} // namespace detail

/// Reads a big-endian `uint16_t` starting at octet `offset` (GEN-050, GEN-051).
/// Errors: `OutOfRange` if the octets are not inside the buffer.
[[nodiscard]] constexpr Result<std::uint16_t> read_u16(std::span<const std::byte> buf,
                                                       std::size_t offset) noexcept {
    return detail::read_be<std::uint16_t>(buf, offset);
}

/// Reads a big-endian `uint32_t` starting at octet `offset` (GEN-050, GEN-051).
/// Errors: `OutOfRange` if the octets are not inside the buffer.
[[nodiscard]] constexpr Result<std::uint32_t> read_u32(std::span<const std::byte> buf,
                                                       std::size_t offset) noexcept {
    return detail::read_be<std::uint32_t>(buf, offset);
}

/// Reads a big-endian `uint64_t` starting at octet `offset` (GEN-050, GEN-051).
/// Errors: `OutOfRange` if the octets are not inside the buffer.
[[nodiscard]] constexpr Result<std::uint64_t> read_u64(std::span<const std::byte> buf,
                                                       std::size_t offset) noexcept {
    return detail::read_be<std::uint64_t>(buf, offset);
}

/// Writes a big-endian `uint16_t` starting at octet `offset` (GEN-050, GEN-051).
/// Errors: `OutOfRange` if the octets are not inside the buffer; the buffer is then unchanged.
[[nodiscard]] constexpr Result<void> write_u16(std::span<std::byte> buf, std::size_t offset,
                                               std::uint16_t value) noexcept {
    return detail::write_be(buf, offset, value);
}

/// Writes a big-endian `uint32_t` starting at octet `offset` (GEN-050, GEN-051).
/// Errors: `OutOfRange` if the octets are not inside the buffer; the buffer is then unchanged.
[[nodiscard]] constexpr Result<void> write_u32(std::span<std::byte> buf, std::size_t offset,
                                               std::uint32_t value) noexcept {
    return detail::write_be(buf, offset, value);
}

/// Writes a big-endian `uint64_t` starting at octet `offset` (GEN-050, GEN-051).
/// Errors: `OutOfRange` if the octets are not inside the buffer; the buffer is then unchanged.
[[nodiscard]] constexpr Result<void> write_u64(std::span<std::byte> buf, std::size_t offset,
                                               std::uint64_t value) noexcept {
    return detail::write_be(buf, offset, value);
}

} // namespace aldis::core::bits

#endif // ALDIS_CORE_BITS_HPP
