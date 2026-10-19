// Tests of aldis/core/bits.hpp, ported from aldis-core `bits.rs`. The Rust property test becomes
// a seeded randomized test (D4).

#include "aldis/core/bits.hpp"
#include "aldis/core/error.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace {

namespace bits = aldis::core::bits;
using aldis::core::ErrorDetail;
using aldis::core::ErrorKind;
using aldis::core::FieldId;
using aldis::test::octets;
using aldis::test::Rng;
using aldis::test::unwrap;
using aldis::test::unwrap_err;
using aldis::test::vec;

TEST(Bits, SpacePacketHeaderFields) {
    // req: GEN-050, GEN-051
    const auto h = octets(0x18, 0x01, 0xC0, 0x00, 0x00, 0x00);
    EXPECT_EQ(unwrap(bits::get(h, 0, 3)), 0U);
    EXPECT_EQ(unwrap(bits::get(h, 3, 1)), 1U);
    EXPECT_EQ(unwrap(bits::get(h, 4, 1)), 1U);
    EXPECT_EQ(unwrap(bits::get(h, 5, 11)), 1U);
    EXPECT_EQ(unwrap(bits::get(h, 16, 2)), 3U);
}

TEST(Bits, FullWidthFields) {
    // req: GEN-050, GEN-051
    const auto b = octets(0x81, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xF1);
    EXPECT_EQ(unwrap(bits::get(b, 0, 64)), 0x8123'4567'89AB'CDEFULL);
    EXPECT_EQ(unwrap(bits::get(b, 4, 64)), 0x1234'5678'9ABC'DEFFULL);
    EXPECT_EQ(unwrap(bits::get(b, 7, 64)), 0x91A2'B3C4'D5E6'F7F8ULL);
    std::array<std::byte, 9> out{};
    ASSERT_TRUE(bits::set(out, 4, 64, 0x1234'5678'9ABC'DEFFULL).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{out}),
              vec(octets(0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xF0)));
}

TEST(Bits, RejectsBadArgumentsWithoutWriting) {
    // req: GEN-020, GEN-042
    auto b = octets(0xAA, 0xAA);
    struct Case {
        std::size_t offset;
        std::uint8_t width;
        std::uint64_t value;
    };
    for (const Case c : {
             Case{.offset = 0, .width = 0, .value = 0},
             Case{.offset = 0, .width = 65, .value = 0},
             Case{.offset = 9, .width = 8, .value = 0},
             Case{.offset = SIZE_MAX, .width = 8, .value = 0},
             Case{.offset = 0, .width = 4, .value = 16},
         }) {
        const auto e = unwrap_err(bits::set(b, c.offset, c.width, c.value));
        EXPECT_EQ(e.kind(), ErrorKind::OutOfRange);
        EXPECT_EQ(vec(std::span<const std::byte>{b}), vec(octets(0xAA, 0xAA)));
    }
    EXPECT_EQ(unwrap_err(bits::set(b, 0, 4, 16)).field(), FieldId{"value"});
    EXPECT_EQ(unwrap_err(bits::set(b, 0, 4, 16)).detail(), ErrorDetail::expected(15, 16));
    EXPECT_EQ(unwrap_err(bits::get(b, 0, 0)).detail(), ErrorDetail::expected(1, 0));
    EXPECT_EQ(unwrap_err(bits::get(b, 0, 0)).field(), FieldId{"width"});
    EXPECT_EQ(unwrap_err(bits::get(b, 0, 65)).detail(), ErrorDetail::expected(64, 65));
    EXPECT_EQ(unwrap_err(bits::get(b, 9, 8)).detail(), ErrorDetail::expected(8, 9));
    EXPECT_EQ(unwrap_err(bits::get(b, 9, 8)).field(), FieldId{"bit_offset"});
    const auto short_bits = unwrap_err(bits::get(octets(0), 0, 16));
    EXPECT_EQ(short_bits.field(), FieldId{"buf_bits"});
    EXPECT_EQ(short_bits.detail(), ErrorDetail::expected(16, 8));
    EXPECT_EQ(unwrap_err(bits::get(b, SIZE_MAX, 1)).kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(unwrap_err(bits::get(std::span<const std::byte>{}, 0, 1)).kind(),
              ErrorKind::OutOfRange);
}

TEST(Bits, OctetHelpers) {
    // req: GEN-050, GEN-051
    std::array<std::byte, 9> b{};
    ASSERT_TRUE(bits::write_u16(b, 1, 0xABCD).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{b}.first(3)), vec(octets(0x00, 0xAB, 0xCD)));
    EXPECT_EQ(unwrap(bits::read_u16(b, 1)), 0xABCDU);
    const auto e16 = unwrap_err(bits::read_u16(b, 8));
    EXPECT_EQ(e16.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(e16.field(), FieldId{"offset"});
    EXPECT_EQ(e16.detail(), ErrorDetail::expected(7, 8));
    EXPECT_EQ(unwrap_err(bits::write_u16(b, SIZE_MAX, 0)).kind(), ErrorKind::OutOfRange);

    ASSERT_TRUE(bits::write_u32(b, 1, 0x1234'5678).is_ok());
    EXPECT_EQ(unwrap(bits::read_u32(b, 1)), 0x1234'5678U);
    EXPECT_EQ(unwrap_err(bits::read_u32(b, 6)).kind(), ErrorKind::OutOfRange);
    const auto short32 = unwrap_err(bits::read_u32(octets(0, 0), 0));
    EXPECT_EQ(short32.field(), FieldId{"buf_len"});
    EXPECT_EQ(short32.detail(), ErrorDetail::expected(4, 2));
    std::array<std::byte, 2> two{};
    EXPECT_EQ(unwrap_err(bits::write_u32(two, 0, 0)).detail(), ErrorDetail::expected(4, 2));
    EXPECT_EQ(unwrap_err(bits::write_u32(b, SIZE_MAX, 0)).kind(), ErrorKind::OutOfRange);

    ASSERT_TRUE(bits::write_u64(b, 1, 0x0123'4567'89AB'CDEFULL).is_ok());
    EXPECT_EQ(unwrap(bits::read_u64(b, 1)), 0x0123'4567'89AB'CDEFULL);
    EXPECT_EQ(unwrap_err(bits::read_u64(b, 2)).kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(unwrap_err(bits::write_u64(b, SIZE_MAX, 0)).kind(), ErrorKind::OutOfRange);
}

TEST(Bits, SetThenGetRoundTripsAndPreservesOtherBits) {
    // req: GEN-050, GEN-051, GEN-042
    Rng rng{0xB175};
    for (int i = 0; i < 20'000; ++i) {
        const std::vector<std::byte> buf = rng.bytes(rng.range(1, 15));
        const std::size_t offset = rng.range(0, 127);
        const auto width = static_cast<std::uint8_t>(rng.range(1, 64));
        const std::uint64_t value = rng.next() & (~0ULL >> (64U - width));
        std::vector<std::byte> out = buf;
        if (bits::set(out, offset, width, value).is_ok()) {
            EXPECT_EQ(unwrap(bits::get(out, offset, width)), value);
            for (std::size_t bit = 0; bit < buf.size() * 8; ++bit) {
                if (bit < offset || bit >= offset + width) {
                    EXPECT_EQ(unwrap(bits::get(out, bit, 1)), unwrap(bits::get(buf, bit, 1)));
                }
            }
        } else {
            EXPECT_EQ(out, buf);
        }
    }
}

TEST(Bits, ConstantEvaluation) {
    // req: CPP-034, GEN-050
    constexpr auto h = octets(0x18, 0x01, 0xC0, 0x00);
    static_assert(bits::get(h, 5, 11).value_or(0) == 1);
    static_assert(!bits::get(h, 30, 8).is_ok());
    SUCCEED();
}

} // namespace
