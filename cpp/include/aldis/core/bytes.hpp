// Byte and element access: the one audited module with raw buffer access (C++ only; CPP-031,
// GEN-013, GEN-020; docs/milestones/c0-foundations.md D9).
//
// This is the only place in the C++ implementation with pointer arithmetic, subscripting of
// spans and arrays, and the `unsafe_buffer_usage` opt-out from Clang's -Wunsafe-buffer-usage
// (checked by `cargo xtask cpp unsafe-optouts`). Every function checks its bounds first and
// is total: out-of-range access returns `nullptr`, `false`, or an error, never undefined
// behavior. All other code reads and writes elements and octets through these functions.
//
// `OctetRange` is the counterpart of Rust's `Range<usize>` (start <= end by construction). It
// lives here, beside the `subspan` that consumes it, so that both security.hpp and layout.hpp
// can use it.

#ifndef ALDIS_CORE_BYTES_HPP
#define ALDIS_CORE_BYTES_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <type_traits>

namespace aldis::core {

/// A range of octet offsets `[start, end)` with `start <= end` (the counterpart of Rust's
/// `Range<usize>`; CPP-023).
class OctetRange {
public:
    /// The empty range at 0.
    constexpr OctetRange() noexcept = default;

    /// The range `[start, end)`, or `InvalidField` (`range`) if `start > end`.
    [[nodiscard]] static constexpr Result<OctetRange> make(std::size_t start,
                                                           std::size_t end) noexcept {
        if (start > end) {
            return Error{ErrorKind::InvalidField}.with_field(FieldId{"range"});
        }
        return OctetRange{start, end};
    }

    /// The range of `len` octets from `start`, its end saturating at the largest offset.
    [[nodiscard]] static constexpr OctetRange from_len(std::size_t start,
                                                       std::size_t len) noexcept {
        return OctetRange{start, arith::saturating_add(start, len)};
    }

    /// First offset.
    [[nodiscard]] constexpr std::size_t start() const noexcept { return start_; }

    /// One past the last offset.
    [[nodiscard]] constexpr std::size_t end() const noexcept { return end_; }

    /// Number of octets, `end - start`.
    [[nodiscard]] constexpr std::size_t len() const noexcept {
        return arith::saturating_sub(end_, start_);
    }

    /// Whether the range is empty.
    [[nodiscard]] constexpr bool is_empty() const noexcept { return start_ == end_; }

    friend constexpr bool operator==(OctetRange, OctetRange) noexcept = default;

private:
    constexpr OctetRange(std::size_t start, std::size_t end) noexcept : start_{start}, end_{end} {}

    std::size_t start_{0};
    std::size_t end_{0};
};

namespace bytes {

/// The element at `index`, or `nullptr` if `index >= s.size()`.
template <typename T> [[nodiscard]] constexpr T* at(std::span<T> s, std::size_t index) noexcept {
    if (index >= s.size()) {
        return nullptr;
    }
#ifdef __clang__
#pragma clang unsafe_buffer_usage begin
#endif
    // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access): checked above
    return &s[index];
#ifdef __clang__
#pragma clang unsafe_buffer_usage end
#endif
}

/// The element at `index`, or `fallback` if `index >= s.size()`.
template <typename T>
[[nodiscard]] constexpr T get_or(std::span<const T> s, std::size_t index, T fallback) noexcept {
    const T* element = at(s, index);
    return element != nullptr ? *element : fallback;
}

/// Sets the element at `index` to `value`; `false` and no write if `index >= s.size()`.
template <typename T>
[[nodiscard]] constexpr bool put(std::span<T> s, std::size_t index, const T& value) noexcept {
    T* element = at(s, index);
    if (element == nullptr) {
        return false;
    }
    *element = value;
    return true;
}

/// The `count` elements from `offset`, or `OutOfRange` (`range`) if they are not all inside
/// `s` (detail: the size, and the end found).
template <typename T>
[[nodiscard]] constexpr Result<std::span<T>> subspan(std::span<T> s, std::size_t offset,
                                                     std::size_t count) noexcept {
    const std::size_t end = arith::saturating_add(offset, count);
    if (offset > s.size() || count > s.size() - offset) {
        return Error::out_of_range(FieldId{"range"}, arith::widen<std::uint64_t>(s.size()),
                                   arith::widen<std::uint64_t>(end));
    }
    return s.subspan(offset, count);
}

/// The elements of `range`, or `OutOfRange` (`range`) if they are not all inside `s`.
template <typename T>
[[nodiscard]] constexpr Result<std::span<T>> subspan(std::span<T> s, OctetRange range) noexcept {
    return subspan(s, range.start(), range.len());
}

/// The `count` elements from `offset`, or the empty span if they are not all inside `s` (the
/// counterpart of Rust's `get(range).unwrap_or_default()`).
template <typename T>
[[nodiscard]] constexpr std::span<T> subspan_or_empty(std::span<T> s, std::size_t offset,
                                                      std::size_t count) noexcept {
    if (offset > s.size() || count > s.size() - offset) {
        return {};
    }
    return s.subspan(offset, count);
}

/// Whether `a` and `b` have equal sizes and equal elements.
template <typename T>
[[nodiscard]] constexpr bool equal(std::span<const T> a, std::span<const T> b) noexcept {
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
#ifdef __clang__
#pragma clang unsafe_buffer_usage begin
#endif
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access): i < size
        if (!(a[i] == b[i])) {
            return false;
        }
#ifdef __clang__
#pragma clang unsafe_buffer_usage end
#endif
    }
    return true;
}

/// Copies `src` into the start of `dst`; `BufferTooSmall` (needed: `src.size()`) and no write if
/// `dst` is shorter (GEN-042).
template <typename T>
[[nodiscard]] constexpr Result<void> copy(std::span<T> dst, std::span<const T> src) noexcept {
    if (dst.size() < src.size()) {
        return Error::buffer_too_small(src.size());
    }
    for (std::size_t i = 0; i < src.size(); ++i) {
#ifdef __clang__
#pragma clang unsafe_buffer_usage begin
#endif
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access): i <
        // src.size() <= dst.size()
        dst[i] = src[i];
#ifdef __clang__
#pragma clang unsafe_buffer_usage end
#endif
    }
    return {};
}

/// Sets every element of `dst` to `value`.
template <typename T> constexpr void fill(std::span<T> dst, const T& value) noexcept {
    for (T& element : dst) {
        element = value;
    }
}

} // namespace bytes

namespace detail {

/// The big-endian value of `octets` (GEN-050), or their error.
template <arith::Unsigned T>
constexpr Result<T> from_be(const Result<std::span<const std::byte>>& octets) noexcept {
    if (octets.error() != nullptr) {
        return *octets.error();
    }
    std::uint64_t value = 0;
    for (const std::byte b : *octets.ok()) {
        value = arith::bit_or(arith::shl(value, 8U),
                              arith::widen<std::uint64_t>(std::to_integer<std::uint8_t>(b)));
    }
    return arith::truncate<T>(value);
}

/// The big-endian octets of `value` (GEN-050).
template <arith::Unsigned T> constexpr std::array<std::byte, sizeof(T)> to_be(T value) noexcept {
    std::array<std::byte, sizeof(T)> octets{};
    unsigned shift = arith::digits<T>;
    for (std::byte& b : octets) {
        shift -= 8U; // digits<T> is a multiple of 8, so this stops at 0
        b = std::byte{arith::truncate<std::uint8_t>(arith::shr(value, shift))};
    }
    return octets;
}

} // namespace detail

/// Reads octets from the front of a buffer; every read is checked and returns `Result`
/// (`Incomplete`, with the octets needed, past the end). A failed read does not advance.
class ByteReader {
public:
    /// A reader at the start of `data`.
    constexpr explicit ByteReader(std::span<const std::byte> data ALDIS_LIFETIMEBOUND) noexcept
        : data_{data} {}

    /// Octets read so far.
    [[nodiscard]] constexpr std::size_t position() const noexcept { return pos_; }

    /// Octets not yet read.
    [[nodiscard]] constexpr std::size_t remaining() const noexcept {
        return arith::saturating_sub(data_.size(), pos_);
    }

    /// The next `count` octets.
    [[nodiscard]] constexpr Result<std::span<const std::byte>> read(std::size_t count) noexcept {
        if (count > remaining()) {
            return Error::incomplete(count);
        }
        // count <= remaining(), so the part is never cut short.
        const std::span<const std::byte> part = bytes::subspan_or_empty(data_, pos_, count);
        pos_ = arith::saturating_add(pos_, count);
        return part;
    }

    /// The next octet.
    [[nodiscard]] constexpr Result<std::uint8_t> read_u8() noexcept {
        return detail::from_be<std::uint8_t>(read(sizeof(std::uint8_t)));
    }

    /// The next two octets, big-endian (GEN-050).
    [[nodiscard]] constexpr Result<std::uint16_t> read_u16() noexcept {
        return detail::from_be<std::uint16_t>(read(sizeof(std::uint16_t)));
    }

    /// The next four octets, big-endian (GEN-050).
    [[nodiscard]] constexpr Result<std::uint32_t> read_u32() noexcept {
        return detail::from_be<std::uint32_t>(read(sizeof(std::uint32_t)));
    }

    /// The next eight octets, big-endian (GEN-050).
    [[nodiscard]] constexpr Result<std::uint64_t> read_u64() noexcept {
        return detail::from_be<std::uint64_t>(read(sizeof(std::uint64_t)));
    }

private:
    std::span<const std::byte> data_;
    std::size_t pos_{0};
};

/// Writes octets to the front of a buffer; every write is checked and returns `Result`
/// (`BufferTooSmall`, with the octets needed, past the end). A failed write changes nothing
/// (GEN-042).
class ByteWriter {
public:
    /// A writer at the start of `out`.
    constexpr explicit ByteWriter(std::span<std::byte> out ALDIS_LIFETIMEBOUND) noexcept
        : out_{out} {}

    /// Octets written so far.
    [[nodiscard]] constexpr std::size_t position() const noexcept { return pos_; }

    /// Octets still free.
    [[nodiscard]] constexpr std::size_t remaining() const noexcept {
        return arith::saturating_sub(out_.size(), pos_);
    }

    /// Writes `data`.
    [[nodiscard]] constexpr Result<void> write(std::span<const std::byte> data) noexcept {
        if (data.size() > remaining()) {
            return Error::buffer_too_small(arith::saturating_add(pos_, data.size()));
        }
        // data.size() <= remaining(), so the destination is exactly data.size() octets and the
        // copy cannot fail.
        const Result<void> copied =
            bytes::copy(bytes::subspan_or_empty(out_, pos_, data.size()), data);
        pos_ = arith::saturating_add(pos_, data.size());
        return copied;
    }

    /// Writes one octet.
    [[nodiscard]] constexpr Result<void> write_u8(std::uint8_t value) noexcept {
        return write(detail::to_be(value));
    }

    /// Writes two octets, big-endian (GEN-050).
    [[nodiscard]] constexpr Result<void> write_u16(std::uint16_t value) noexcept {
        return write(detail::to_be(value));
    }

    /// Writes four octets, big-endian (GEN-050).
    [[nodiscard]] constexpr Result<void> write_u32(std::uint32_t value) noexcept {
        return write(detail::to_be(value));
    }

    /// Writes eight octets, big-endian (GEN-050).
    [[nodiscard]] constexpr Result<void> write_u64(std::uint64_t value) noexcept {
        return write(detail::to_be(value));
    }

private:
    std::span<std::byte> out_;
    std::size_t pos_{0};
};

} // namespace aldis::core

#endif // ALDIS_CORE_BYTES_HPP
