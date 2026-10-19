// Tests of aldis/core/container.hpp, ported from aldis-core `container.rs`. The Rust property
// tests become seeded randomized tests against a std::deque model (D4).

#include "aldis/core/container.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <optional>
#include <span>
#include <vector>

namespace {

using aldis::core::ByteQueue;
using aldis::core::ErrorDetail;
using aldis::core::ErrorKind;
using aldis::core::FieldId;
using aldis::core::FixedQueue;
using aldis::core::FixedVec;
using aldis::test::octets;
using aldis::test::Rng;
using aldis::test::unwrap;
using aldis::test::unwrap_err;
using aldis::test::vec;

TEST(Container, QueueBasicsAndZeroCapacity) {
    // req: GEN-010, GEN-011, API-051
    FixedQueue<std::uint8_t, 2> q;
    EXPECT_EQ(q.capacity(), 2U);
    EXPECT_EQ(q.peek(), nullptr);
    EXPECT_FALSE(q.is_full());
    EXPECT_TRUE(q.push(1).is_ok());
    EXPECT_FALSE(q.is_full());
    EXPECT_TRUE(q.push(2).is_ok());
    EXPECT_TRUE(q.is_full());
    EXPECT_EQ(unwrap_err(q.push(3)).kind(), ErrorKind::WouldBlock);
    ASSERT_NE(q.peek(), nullptr);
    EXPECT_EQ(*q.peek(), 1U);
    EXPECT_EQ(q.pop(), std::uint8_t{1});
    EXPECT_TRUE(q.push(3).is_ok());
    EXPECT_EQ(q.len(), 2U);
    q.clear();
    EXPECT_TRUE(q.is_empty());
    EXPECT_EQ(q.pop(), std::nullopt);
    FixedQueue<std::uint8_t, 0> z;
    EXPECT_TRUE(z.is_full());
    EXPECT_EQ(unwrap_err(z.push(1)).kind(), ErrorKind::WouldBlock);
    EXPECT_EQ(z.pop(), std::nullopt);
}

TEST(Container, ByteQueueErrorsKeepRecords) {
    // req: API-061, GEN-042
    ByteQueue<16> q;
    EXPECT_EQ(q.capacity(), 16U);
    const auto too_long = unwrap_err(q.push(std::vector<std::byte>(13)));
    EXPECT_EQ(too_long.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(too_long.field(), FieldId{"record_len"});
    EXPECT_EQ(too_long.detail(), ErrorDetail::expected(12, 13));
    ASSERT_TRUE(q.push(octets(1, 2, 3)).is_ok());
    EXPECT_EQ(unwrap_err(q.push(std::vector<std::byte>(9))).kind(), ErrorKind::WouldBlock);
    std::array<std::byte, 2> small{};
    const auto e = unwrap_err(q.pop_into(small));
    EXPECT_EQ(e.kind(), ErrorKind::BufferTooSmall);
    EXPECT_EQ(e.detail(), ErrorDetail::needed(3));
    EXPECT_EQ(q.records(), 1U);
    std::array<std::byte, 8> out{};
    EXPECT_EQ(unwrap(q.pop_into(out)), std::size_t{3});
    EXPECT_EQ(vec(std::span<const std::byte>{out}.first(3)), vec(octets(1, 2, 3)));
    EXPECT_EQ(unwrap(q.pop_into(out)), std::nullopt);
    EXPECT_EQ(q.discard(), std::nullopt);
    ASSERT_TRUE(q.push(std::span<const std::byte>{}).is_ok());
    EXPECT_EQ(q.discard(), std::size_t{0});
    ASSERT_TRUE(q.push(octets(1)).is_ok());
    q.clear();
    EXPECT_TRUE(q.is_empty());
    const auto tiny = unwrap_err(ByteQueue<0>{}.push(std::span<const std::byte>{}));
    EXPECT_EQ(tiny.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(tiny.field(), FieldId{"capacity"});
    EXPECT_EQ(tiny.detail(), ErrorDetail::expected(4, 0));
}

TEST(Container, FootprintIsKnownAtCompileTime) {
    // req: GEN-012
    static_assert(sizeof(FixedQueue<std::uint32_t, 8>) >= 8 * sizeof(std::uint32_t));
    static_assert(sizeof(ByteQueue<256>) >= 256);
    static_assert(sizeof(FixedVec<std::uint16_t, 4>) >= 4 * sizeof(std::uint16_t));
    SUCCEED();
}

TEST(Container, FixedVec) {
    // req: GEN-010
    FixedVec<std::uint16_t, 2> v;
    EXPECT_EQ(v.capacity(), 2U);
    EXPECT_TRUE(v.is_empty());
    EXPECT_FALSE(v.is_full());
    ASSERT_TRUE(v.push(7).is_ok());
    ASSERT_TRUE(v.push(8).is_ok());
    EXPECT_TRUE(v.is_full());
    EXPECT_EQ(unwrap_err(v.push(9)).kind(), ErrorKind::CapacityExceeded);
    EXPECT_EQ(vec(v.as_span()), (std::vector<std::uint16_t>{7, 8}));
    EXPECT_EQ(v.len(), 2U);
    EXPECT_FALSE(v.is_empty());
    v.as_mut_span().front() = 6;
    EXPECT_EQ(vec(v.as_span()), (std::vector<std::uint16_t>{6, 8}));
    EXPECT_EQ(v.pop(), std::uint16_t{8});
    FixedVec<std::uint16_t, 2> v2;
    ASSERT_TRUE(v2.push(6).is_ok());
    // Equality compares only active elements, ignoring popped slots.
    EXPECT_EQ(v, v2);
    ASSERT_TRUE(v2.push(1).is_ok());
    EXPECT_NE(v, v2);
    v.clear();
    EXPECT_TRUE(v.is_empty());
    EXPECT_EQ(v.pop(), std::nullopt);
    EXPECT_TRUE(v.as_span().empty());
}

TEST(Container, ByteQueueMatchesModel) {
    // req: GEN-010, API-061
    Rng rng{0xB17E};
    for (int run = 0; run < 300; ++run) {
        ByteQueue<37> q;
        std::deque<std::vector<std::byte>> model;
        std::size_t used = 0;
        const auto ops = rng.range(0, 199);
        for (std::uint64_t op = 0; op < ops; ++op) {
            switch (rng.range(0, 2)) {
            case 0: {
                const std::vector<std::byte> data = rng.bytes(rng.range(0, 19));
                const std::size_t need = data.size() + 4;
                const auto r = q.push(data);
                if (need > 37) {
                    EXPECT_EQ(unwrap_err(r).kind(), ErrorKind::OutOfRange);
                } else if (used + need > 37) {
                    EXPECT_EQ(unwrap_err(r).kind(), ErrorKind::WouldBlock);
                } else {
                    EXPECT_TRUE(r.is_ok());
                    used += need;
                    model.push_back(data);
                }
                break;
            }
            case 1: {
                std::array<std::byte, 40> out{};
                const std::optional<std::size_t> got = unwrap(q.pop_into(out));
                if (model.empty()) {
                    EXPECT_EQ(got, std::nullopt);
                } else {
                    ASSERT_TRUE(got.has_value());
                    EXPECT_EQ(vec(std::span<const std::byte>{out}.first(*got)), model.front());
                    used -= model.front().size() + 4;
                    model.pop_front();
                }
                break;
            }
            default: {
                const std::optional<std::size_t> got = q.discard();
                if (model.empty()) {
                    EXPECT_EQ(got, std::nullopt);
                } else {
                    EXPECT_EQ(got, model.front().size());
                    used -= model.front().size() + 4;
                    model.pop_front();
                }
                break;
            }
            }
            EXPECT_EQ(q.records(), model.size());
            EXPECT_EQ(q.free(), 37 - used);
        }
    }
}

TEST(Container, FixedQueueMatchesModel) {
    // req: GEN-010, API-051
    Rng rng{0xF1F0};
    for (int run = 0; run < 300; ++run) {
        FixedQueue<std::uint8_t, 5> q;
        std::deque<std::uint8_t> model;
        const auto ops = rng.range(0, 199);
        for (std::uint64_t op = 0; op < ops; ++op) {
            if (rng.range(0, 1) == 0) {
                const auto v = static_cast<std::uint8_t>(rng.next());
                if (model.size() < 5) {
                    EXPECT_TRUE(q.push(v).is_ok());
                    model.push_back(v);
                } else {
                    EXPECT_EQ(unwrap_err(q.push(v)).kind(), ErrorKind::WouldBlock);
                }
            } else {
                const std::optional<std::uint8_t> expected =
                    model.empty() ? std::nullopt : std::optional{model.front()};
                EXPECT_EQ(q.pop(), expected);
                if (!model.empty()) {
                    model.pop_front();
                }
            }
            EXPECT_EQ(q.len(), model.size());
            if (model.empty()) {
                EXPECT_EQ(q.peek(), nullptr);
            } else {
                ASSERT_NE(q.peek(), nullptr);
                EXPECT_EQ(*q.peek(), model.front());
            }
        }
    }
}

TEST(Container, ConstantEvaluation) {
    // req: CPP-034, GEN-010
    static_assert([] {
        FixedQueue<int, 2> q;
        static_cast<void>(q.push(1));
        static_cast<void>(q.push(2));
        return !q.push(3).is_ok() && q.pop() == 1 && q.len() == 1;
    }());
    static_assert([] {
        ByteQueue<8> q;
        const auto data = aldis::test::octets(9, 8);
        static_cast<void>(q.push(data));
        return q.peek_len() == std::size_t{2} && q.free() == 2;
    }());
    SUCCEED();
}

} // namespace
