// Frame Error Control Field CRC-16: aldis-core `crc.rs` (CMN-001…CMN-004).
//
// Defined identically in 132.0-B-3 §4.1.6, 232.0-B-4 §4.1.4, 732.0-B-5 §4.1.6, and 732.1-B-3
// Annex B: generator G(X) = X¹⁶ + X¹² + X⁵ + 1 (`0x1021`), register preset to all ones, MSB first,
// no reflection, no final XOR (catalogue name CRC-16/IBM-3740). The FECF is the final 16 bits of
// the frame, MSB first.
//
// The bitwise and the table-driven forms are both always available and produce identical
// results (CMN-004). `crc16` and `crc16_update` use the bitwise form unless `ALDIS_CRC_TABLE` is
// defined (CMake option `ALDIS_CRC_TABLE`, the counterpart of the `crc-table` feature). The
// 256-entry table is a `constexpr` array, emitted in read-only memory only if used.

#ifndef ALDIS_CORE_CRC_HPP
#define ALDIS_CORE_CRC_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/bits.hpp"
#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>

namespace aldis::core::crc {

/// Generator polynomial without the X¹⁶ term (132.0 §4.1.6.2; CMN-001).
inline constexpr std::uint16_t poly = 0x1021;

/// Initial CRC-16 register value: all ones (132.0 §4.1.6.2; CMN-001).
inline constexpr std::uint16_t preset = 0xFFFF;

/// Length of the FECF in octets.
inline constexpr std::size_t fecf_len = 2;

namespace detail {

/// Entries of the lookup table: one per octet value.
inline constexpr std::size_t table_len = 256;

/// Bit 15, the register bit shifted out next.
inline constexpr unsigned msb = 15;

/// One bitwise CRC step for one octet.
[[nodiscard]] constexpr std::uint16_t step_bitwise(std::uint16_t crc, std::uint8_t octet) noexcept {
    crc = arith::bit_xor(crc, arith::shl(arith::widen<std::uint16_t>(octet), arith::octet_bits));
    for (unsigned bit = 0; bit < arith::octet_bits; ++bit) {
        const bool top = arith::shr(crc, msb) != 0U;
        crc = arith::shl(crc, 1U);
        if (top) {
            crc = arith::bit_xor(crc, poly);
        }
    }
    return crc;
}

/// The CRC register contribution of each possible leading octet.
[[nodiscard]] consteval std::array<std::uint16_t, table_len> make_table() noexcept {
    std::array<std::uint16_t, table_len> table{};
    std::uint8_t index = 0;
    for (std::uint16_t& entry : table) {
        entry = step_bitwise(0, index);
        index = arith::wrapping_add<8>(index, std::uint8_t{1});
    }
    return table;
}

/// The 256-entry lookup table (CMN-004).
inline constexpr std::array<std::uint16_t, table_len> table = make_table();

} // namespace detail

/// Updates an in-progress CRC-16 `crc` (initially `preset`) over `data`, bit by bit (CMN-001).
[[nodiscard]] constexpr std::uint16_t
crc16_update_bitwise(std::uint16_t crc, std::span<const std::byte> data) noexcept {
    for (const std::byte octet : data) {
        crc = detail::step_bitwise(crc, std::to_integer<std::uint8_t>(octet));
    }
    return crc;
}

/// CRC-16 of `data`, computed bit by bit (CMN-001, CMN-004).
[[nodiscard]] constexpr std::uint16_t crc16_bitwise(std::span<const std::byte> data) noexcept {
    return crc16_update_bitwise(preset, data);
}

/// Updates an in-progress CRC-16 `crc` (initially `preset`) over `data` with the 256-entry table
/// (CMN-001, CMN-004).
[[nodiscard]] constexpr std::uint16_t crc16_update_table(std::uint16_t crc,
                                                         std::span<const std::byte> data) noexcept {
    for (const std::byte octet : data) {
        const std::uint8_t index =
            arith::bit_xor(arith::truncate<std::uint8_t>(arith::shr(crc, arith::octet_bits)),
                           std::to_integer<std::uint8_t>(octet));
        // A uint8_t index is always inside the 256-entry table.
        const std::uint16_t entry =
            bytes::get_or(std::span<const std::uint16_t>{detail::table}, index, std::uint16_t{0});
        crc = arith::bit_xor(arith::shl(crc, arith::octet_bits), entry);
    }
    return crc;
}

/// CRC-16 of `data`, computed with the 256-entry table (CMN-001, CMN-004).
[[nodiscard]] constexpr std::uint16_t crc16_table(std::span<const std::byte> data) noexcept {
    return crc16_update_table(preset, data);
}

/// Updates an in-progress CRC-16 `crc` (initially `preset`) over `data` with the form selected at
/// build time (CMN-001, CMN-004).
[[nodiscard]] constexpr std::uint16_t crc16_update(std::uint16_t crc,
                                                   std::span<const std::byte> data) noexcept {
#ifdef ALDIS_CRC_TABLE
    return crc16_update_table(crc, data);
#else
    return crc16_update_bitwise(crc, data);
#endif
}

/// CRC-16 of `data` with the form selected at build time (CMN-001, CMN-004).
[[nodiscard]] constexpr std::uint16_t crc16(std::span<const std::byte> data) noexcept {
    return crc16_update(preset, data);
}

/// Verifies the FECF in the last two octets of `frame` by recomputing the CRC over the rest
/// (CMN-003, method 1).
///
/// Errors: `Incomplete` (needed: 2) if `frame` is shorter than 2 octets; `CrcMismatch` if the FECF
/// is wrong.
[[nodiscard]] constexpr Result<void> verify_fecf(std::span<const std::byte> frame) noexcept {
    const Result<std::size_t> body_len = arith::checked_sub(frame.size(), fecf_len);
    if (body_len.ok() == nullptr) {
        return Error::incomplete(fecf_len);
    }
    // The body is frame.size() - 2 octets, and the FECF the 2 after it: both inside the frame.
    const std::span<const std::byte> covered =
        bytes::subspan_or_empty(frame, std::size_t{0}, *body_len.ok());
    const std::uint16_t found = bits::read_u16(frame, *body_len.ok()).value_or(0);
    if (crc16(covered) != found) {
        return Error{ErrorKind::CrcMismatch};
    }
    return {};
}

/// The error-detection syndrome over a whole frame including its FECF: zero exactly when no error
/// is detected (132.0 §4.1.6.3; CMN-003, method 2).
[[nodiscard]] constexpr std::uint16_t syndrome(std::span<const std::byte> frame) noexcept {
    return crc16(frame);
}

/// Writes the FECF over all but the last two octets into the last two octets of `frame` (CMN-002).
///
/// Errors: `BufferTooSmall` (needed: 2) if `frame` is shorter than 2 octets.
[[nodiscard]] constexpr Result<void> write_fecf(std::span<std::byte> frame) noexcept {
    const Result<std::size_t> body_len = arith::checked_sub(frame.size(), fecf_len);
    if (body_len.ok() == nullptr) {
        return Error::buffer_too_small(fecf_len);
    }
    const std::uint16_t fecf = crc16(
        bytes::subspan_or_empty(std::span<const std::byte>{frame}, std::size_t{0}, *body_len.ok()));
    ByteWriter writer{bytes::subspan_or_empty(frame, *body_len.ok(), fecf_len)};
    return writer.write_u16(fecf);
}

} // namespace aldis::core::crc

#endif // ALDIS_CORE_CRC_HPP
