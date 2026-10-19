// Fixed-capacity containers for heapless engines: aldis-core `container.rs` (GEN-010, GEN-011,
// GEN-012, API-061).
//
// Capacities are template parameters, so an engine's memory footprint is `sizeof` (GEN-012).
// Nothing allocates, and no operation has undefined behavior: a full container rejects the item
// (engines report `WouldBlock`, API-052), and a too-small output buffer leaves the item queued
// (API-061). Placement new is not allowed outside the C ABI layer (CPP-030), so elements live in a
// `std::array<T, N>` and `T` must be trivially copyable and default-constructible (D11).
// `FixedQueue::push` reports a full queue with `WouldBlock` where Rust hands the item back; the
// caller still holds its copy (an API difference, not a behaviour divergence).

#ifndef ALDIS_CORE_CONTAINER_HPP
#define ALDIS_CORE_CONTAINER_HPP

#include "aldis/core/arith.hpp"
#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <optional>
#include <span>
#include <type_traits>

namespace aldis::core {

/// An element type of the fixed containers: trivially copyable and default-constructible (D11).
template <typename T>
concept Storable = std::is_trivially_copyable_v<T> && std::is_default_constructible_v<T>;

namespace detail {

/// `(i + k) mod n` for `i < n` and `k <= n`, without division and without overflow. Callers
/// guarantee `n > 0` (a zero-capacity container is always full and always empty, so it never
/// indexes).
[[nodiscard]] constexpr std::size_t wrap_add(std::size_t i, std::size_t k, std::size_t n) noexcept {
    const std::size_t room = arith::saturating_sub(n, i);
    return k >= room ? arith::saturating_sub(k, room) : arith::saturating_add(i, k);
}

} // namespace detail

/// FIFO queue of up to `N` items (GEN-010, GEN-011).
template <Storable T, std::size_t N> class FixedQueue {
public:
    /// An empty queue.
    constexpr FixedQueue() noexcept = default;

    /// Number of queued items.
    [[nodiscard]] constexpr std::size_t len() const noexcept { return len_; }

    /// Whether the queue is empty.
    [[nodiscard]] constexpr bool is_empty() const noexcept { return len_ == 0; }

    /// Whether the queue is full.
    [[nodiscard]] constexpr bool is_full() const noexcept { return len_ >= N; }

    /// The capacity `N`.
    [[nodiscard]] static constexpr std::size_t capacity() noexcept { return N; }

    /// Appends `item`; `WouldBlock` and no change if the queue is full (API-052).
    [[nodiscard]] constexpr Result<void> push(const T& item) noexcept {
        // With N == 0 there is no slot at all; when full, the tail wraps onto the head.
        T* slot = bytes::at(std::span<T>{slots_}, detail::wrap_add(head_, len_, N));
        if (slot == nullptr || is_full()) {
            return Error{ErrorKind::WouldBlock};
        }
        *slot = item;
        len_ = arith::saturating_add(len_, std::size_t{1});
        return {};
    }

    /// Removes and returns the oldest item.
    constexpr std::optional<T> pop() noexcept {
        const T* front = peek();
        if (front == nullptr) {
            return std::nullopt;
        }
        const T item = *front;
        head_ = detail::wrap_add(head_, 1, N);
        len_ = arith::saturating_sub(len_, std::size_t{1});
        return item;
    }

    /// The oldest item, without removing it; `nullptr` if the queue is empty.
    [[nodiscard]] constexpr const T* peek() const noexcept {
        if (is_empty()) {
            return nullptr;
        }
        return bytes::at(std::span<const T>{slots_}, head_);
    }

    /// Removes all items.
    constexpr void clear() noexcept {
        head_ = 0;
        len_ = 0;
    }

private:
    std::array<T, N> slots_{};
    std::size_t head_{0};
    std::size_t len_{0};
};

/// FIFO of variable-length octet records in a ring of `N` octets (API-060, API-061).
///
/// Each record costs its length plus a 4-octet length prefix.
template <std::size_t N> class ByteQueue {
    static_assert(N <= std::numeric_limits<std::uint32_t>::max(),
                  "ByteQueue capacity must fit in the 4-octet length prefix");

public:
    /// Octets of the length prefix of each record.
    static constexpr std::size_t prefix_len = 4;

    /// An empty queue.
    constexpr ByteQueue() noexcept = default;

    /// Number of queued records.
    [[nodiscard]] constexpr std::size_t records() const noexcept { return records_; }

    /// Whether no record is queued.
    [[nodiscard]] constexpr bool is_empty() const noexcept { return records_ == 0; }

    /// Free octets, including the space a new record's prefix needs.
    [[nodiscard]] constexpr std::size_t free() const noexcept {
        return arith::saturating_sub(N, used_);
    }

    /// The capacity `N` in octets.
    [[nodiscard]] static constexpr std::size_t capacity() noexcept { return N; }

    /// Appends a record.
    ///
    /// Errors: `OutOfRange` if the record can never fit (`len + 4 > N`: field `record_len`, or
    /// `capacity` if `N < 4`); `WouldBlock` if it does not fit now. The queue is unchanged on
    /// error (GEN-042).
    [[nodiscard]] constexpr Result<void> push(std::span<const std::byte> data) noexcept {
        const Result<std::size_t> max_len = arith::checked_sub(N, prefix_len);
        if (max_len.ok() == nullptr || data.size() > *max_len.ok()) {
            return record_len_error(data.size());
        }
        const std::size_t need = arith::saturating_add(data.size(), prefix_len); // <= N
        if (need > free()) {
            return Error{ErrorKind::WouldBlock};
        }
        // need <= N <= UINT32_MAX, so the length always fits the prefix.
        std::array<std::byte, prefix_len> prefix{};
        ByteWriter writer{prefix};
        static_cast<void>(writer.write_u32(arith::truncate<std::uint32_t>(data.size())));
        const std::size_t tail = detail::wrap_add(head_, used_, N);
        write_at(write_at(tail, prefix), data);
        used_ = arith::saturating_add(used_, need);
        records_ = arith::saturating_add(records_, std::size_t{1});
        return {};
    }

    /// Length of the oldest record.
    [[nodiscard]] constexpr std::optional<std::size_t> peek_len() const noexcept {
        if (is_empty()) {
            return std::nullopt;
        }
        std::array<std::byte, prefix_len> prefix{};
        read_at(head_, prefix);
        ByteReader reader{prefix};
        return arith::widen<std::size_t>(reader.read_u32().value_or(0));
    }

    /// Removes the oldest record into `out`, returning its length; `nullopt` if no record is
    /// queued.
    ///
    /// Errors: `BufferTooSmall` (needed: the record length) if `out` is shorter than the record;
    /// the record stays queued (API-061).
    [[nodiscard]] constexpr Result<std::optional<std::size_t>>
    pop_into(std::span<std::byte> out) noexcept {
        const std::optional<std::size_t> len = peek_len();
        if (!len.has_value()) {
            return std::optional<std::size_t>{};
        }
        if (out.size() < *len) {
            return Error::buffer_too_small(*len);
        }
        read_at(detail::wrap_add(head_, prefix_len, N), bytes::subspan_or_empty(out, 0, *len));
        consume(*len);
        return len;
    }

    /// Discards the oldest record, returning its length.
    constexpr std::optional<std::size_t> discard() noexcept {
        const std::optional<std::size_t> len = peek_len();
        if (len.has_value()) {
            consume(*len);
        }
        return len;
    }

    /// Removes all records.
    constexpr void clear() noexcept {
        head_ = 0;
        used_ = 0;
        records_ = 0;
    }

private:
    static constexpr Error record_len_error(std::size_t len) noexcept {
        const Result<std::size_t> max_len = arith::checked_sub(N, prefix_len);
        if (max_len.ok() != nullptr) {
            return Error::out_of_range(FieldId{"record_len"},
                                       arith::widen<std::uint64_t>(*max_len.ok()),
                                       arith::widen<std::uint64_t>(len));
        }
        return Error::out_of_range(FieldId{"capacity"}, arith::widen<std::uint64_t>(prefix_len),
                                   arith::widen<std::uint64_t>(N));
    }

    /// Copies `out.size()` (<= N) octets starting at ring offset `at` (< N), wrapping.
    constexpr void read_at(std::size_t at, std::span<std::byte> out) const noexcept {
        std::size_t index = at;
        for (std::byte& o : out) {
            o = bytes::get_or(std::span<const std::byte>{ring_}, index, std::byte{0});
            index = detail::wrap_add(index, 1, N);
        }
    }

    /// Writes `data` (<= N octets) starting at ring offset `at` (< N), wrapping; returns the
    /// offset after the last octet written.
    constexpr std::size_t write_at(std::size_t at, std::span<const std::byte> data) noexcept {
        std::size_t index = at;
        for (const std::byte d : data) {
            // index < N: wrap_add keeps it inside the ring.
            static_cast<void>(bytes::put(std::span<std::byte>{ring_}, index, d));
            index = detail::wrap_add(index, 1, N);
        }
        return index;
    }

    constexpr void consume(std::size_t len) noexcept {
        const std::size_t consumed = arith::saturating_add(len, prefix_len);
        head_ = detail::wrap_add(head_, consumed, N);
        used_ = arith::saturating_sub(used_, consumed);
        records_ = arith::saturating_sub(records_, std::size_t{1});
    }

    std::array<std::byte, N> ring_{};
    std::size_t head_{0};
    std::size_t used_{0};
    std::size_t records_{0};
};

/// Vector of up to `N` copyable items (GEN-010, GEN-011).
template <Storable T, std::size_t N> class FixedVec {
public:
    /// An empty vector.
    constexpr FixedVec() noexcept = default;

    /// Appends `item`; `CapacityExceeded` if `N` items are already stored.
    [[nodiscard]] constexpr Result<void> push(const T& item) noexcept {
        T* slot = bytes::at(std::span<T>{items_}, len_);
        if (slot == nullptr) {
            return Error{ErrorKind::CapacityExceeded};
        }
        *slot = item;
        len_ = arith::saturating_add(len_, std::size_t{1});
        return {};
    }

    /// Removes and returns the last item.
    constexpr std::optional<T> pop() noexcept {
        const Result<std::size_t> last = arith::checked_sub(len_, std::size_t{1});
        if (last.ok() == nullptr) {
            return std::nullopt;
        }
        len_ = *last.ok();
        return bytes::get_or(std::span<const T>{items_}, len_, T{});
    }

    /// The stored items (Rust `as_slice`).
    [[nodiscard]] constexpr std::span<const T> as_span() const noexcept {
        return bytes::subspan_or_empty(std::span<const T>{items_}, 0, len_);
    }

    /// Mutable access to the stored items (Rust `as_mut_slice`).
    [[nodiscard]] constexpr std::span<T> as_mut_span() noexcept {
        return bytes::subspan_or_empty(std::span<T>{items_}, 0, len_);
    }

    /// Number of stored items.
    [[nodiscard]] constexpr std::size_t len() const noexcept { return len_; }

    /// The capacity `N`.
    [[nodiscard]] static constexpr std::size_t capacity() noexcept { return N; }

    /// Whether no item is stored.
    [[nodiscard]] constexpr bool is_empty() const noexcept { return len_ == 0; }

    /// Whether `N` items are stored.
    [[nodiscard]] constexpr bool is_full() const noexcept { return len_ >= N; }

    /// Removes all items.
    constexpr void clear() noexcept { len_ = 0; }

    /// Equal when the stored items are equal; slots beyond `len()` are ignored. Needs `T == T`
    /// only where it is used.
    friend constexpr bool operator==(const FixedVec& a, const FixedVec& b) noexcept {
        return bytes::equal(a.as_span(), b.as_span());
    }

private:
    std::array<T, N> items_{};
    std::size_t len_{0};
};

} // namespace aldis::core

#endif // ALDIS_CORE_CONTAINER_HPP
