// Tests of aldis/core/arith.hpp, the C++-only arithmetic helpers (D10).

#include "aldis/core/arith.hpp"
#include "aldis/core/error.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <cstddef>
#include <cstdint>
#include <limits>
#include <optional>

namespace {

namespace arith = aldis::core::arith;
using aldis::core::ErrorKind;
using aldis::core::Expected;
using aldis::test::Rng;
using aldis::test::unwrap;
using aldis::test::unwrap_err;

constexpr std::uint64_t u64_max = std::numeric_limits<std::uint64_t>::max();

TEST(Arith, MasksAndConversions) {
    // req: CPP-032, GEN-024
    EXPECT_EQ(arith::low_mask(0), 0U);
    EXPECT_EQ(arith::low_mask(1), 1U);
    EXPECT_EQ(arith::low_mask(14), 0x3FFFU);
    EXPECT_EQ(arith::low_mask(64), u64_max);
    EXPECT_EQ(arith::low_mask(200), u64_max);
    EXPECT_EQ(arith::truncate<std::uint8_t>(std::uint32_t{0x1234}), 0x34U);
    EXPECT_EQ(arith::widen<std::uint64_t>(std::uint8_t{0xFF}), 0xFFU);
    EXPECT_EQ(unwrap(arith::narrow<std::uint8_t>(std::uint64_t{255})), 255U);
    const auto too_big = unwrap_err(arith::narrow<std::uint8_t>(std::uint64_t{256}));
    EXPECT_EQ(too_big.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(too_big.detail().expected(), (Expected{255, 256}));
    EXPECT_EQ(arith::narrow_to_max<std::uint8_t>(std::size_t{64}, 64), std::uint8_t{64});
    EXPECT_EQ(arith::narrow_to_max<std::uint8_t>(std::size_t{65}, 64), std::nullopt);
    EXPECT_EQ(arith::narrow_to_max<std::uint8_t>(std::size_t{300}, 1000), std::nullopt);
    EXPECT_EQ(arith::popcount(std::uint32_t{0x8000'0011}), 3U);
}

TEST(Arith, ShiftsAndBitOps) {
    // req: CPP-032
    EXPECT_EQ(arith::shl(std::uint16_t{0x8001}, 1), 0x0002U);
    EXPECT_EQ(arith::shl(std::uint16_t{1}, 15), 0x8000U);
    EXPECT_EQ(arith::shl(std::uint16_t{1}, 16), 0U);
    EXPECT_EQ(arith::shl(std::uint64_t{1}, 63), 1ULL << 63U);
    EXPECT_EQ(arith::shl(std::uint64_t{1}, 64), 0U);
    EXPECT_EQ(arith::shr(std::uint32_t{0x8000'0000}, 31), 1U);
    EXPECT_EQ(arith::shr(std::uint32_t{0x8000'0000}, 32), 0U);
    EXPECT_EQ(arith::bit_and(std::uint8_t{0xF0}, std::uint8_t{0x3C}), 0x30U);
    EXPECT_EQ(arith::bit_or(std::uint8_t{0xF0}, std::uint8_t{0x0C}), 0xFCU);
    EXPECT_EQ(arith::bit_xor(std::uint16_t{0xFFFF}, std::uint16_t{0x1021}), 0xEFDEU);
    EXPECT_EQ(arith::bit_not(std::uint8_t{0x0F}), 0xF0U);
}

TEST(Arith, WrappingFollowsTheFieldModulus) {
    // req: GEN-024, CPP-032
    EXPECT_EQ(arith::wrapping_add<14>(std::uint16_t{0x3FFF}, std::uint16_t{1}), 0U);
    EXPECT_EQ(arith::wrapping_add<8>(std::uint8_t{250}, std::uint8_t{10}), 4U);
    EXPECT_EQ(arith::wrapping_sub<8>(std::uint8_t{3}, std::uint8_t{5}), 254U);
    EXPECT_EQ(arith::wrapping_sub<8>(std::uint8_t{5}, std::uint8_t{3}), 2U);
    EXPECT_EQ(arith::wrapping_add(u64_max, 1, 64), 0U);
    EXPECT_EQ(arith::wrapping_sub(0, 1, 64), u64_max);
    EXPECT_EQ(arith::wrapping_add(7, 1, 3), 0U);
    EXPECT_EQ(arith::wrapping_add(0xFF, 0x100, 8), 0xFFU); // operands reduced to the field first
    // Against a 128-bit-free model on random widths.
    Rng rng{0x00A1'D150};
    for (int i = 0; i < 10'000; ++i) {
        const auto bits = static_cast<unsigned>(rng.range(1, 63));
        const std::uint64_t modulus = 1ULL << bits;
        const std::uint64_t a = rng.next() % modulus;
        const std::uint64_t b = rng.next() % modulus;
        EXPECT_EQ(arith::wrapping_add(a, b, bits), (a + b) % modulus);
        EXPECT_EQ(arith::wrapping_sub(a, b, bits), (a + modulus - b) % modulus);
    }
}

TEST(Arith, CheckedAndSaturating) {
    // req: CPP-032, GEN-020
    EXPECT_EQ(unwrap(arith::checked_add(std::uint8_t{200}, std::uint8_t{55})), 255U);
    EXPECT_EQ(unwrap_err(arith::checked_add(std::uint8_t{200}, std::uint8_t{56})).kind(),
              ErrorKind::OutOfRange);
    EXPECT_EQ(unwrap(arith::checked_sub(std::size_t{5}, std::size_t{5})), 0U);
    EXPECT_FALSE(arith::checked_sub(std::size_t{4}, std::size_t{5}).is_ok());
    EXPECT_EQ(unwrap(arith::checked_mul(std::uint16_t{255}, std::uint16_t{257})), 65535U);
    EXPECT_FALSE(arith::checked_mul(std::uint16_t{256}, std::uint16_t{256}).is_ok());
    EXPECT_EQ(unwrap(arith::checked_mul(std::uint64_t{0}, u64_max)), 0U);
    EXPECT_EQ(arith::saturating_add(u64_max, std::uint64_t{1}), u64_max);
    EXPECT_EQ(arith::saturating_add(std::uint64_t{1}, std::uint64_t{2}), 3U);
    EXPECT_EQ(arith::saturating_sub(std::size_t{2}, std::size_t{5}), 0U);
    EXPECT_EQ(arith::saturating_sub(std::size_t{5}, std::size_t{2}), 3U);
    EXPECT_EQ(arith::saturating_mul(u64_max, std::uint64_t{2}), u64_max);
    EXPECT_EQ(arith::saturating_mul(std::uint64_t{3}, std::uint64_t{2}), 6U);
}

TEST(Arith, ConstantEvaluation) {
    // req: CPP-034, CPP-032
    static_assert(arith::wrapping_add(arith::low_mask(24), 1, 24) == 0);
    static_assert(arith::shl(std::uint8_t{0x81}, 1) == 0x02);
    static_assert(arith::saturating_sub(std::uint8_t{1}, std::uint8_t{2}) == 0);
    static_assert(!arith::checked_add(std::uint32_t{0xFFFF'FFFF}, std::uint32_t{1}).is_ok());
    SUCCEED();
}

} // namespace
