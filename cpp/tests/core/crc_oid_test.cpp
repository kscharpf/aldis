// Tests of aldis/core/crc.hpp and oid.hpp, ported from aldis-core `crc.rs` and `oid.rs`. The
// expected values are those of the Rust tests (the CRC-16/IBM-3740 check value and the first
// OID octets of 132.0 Annex D); the shared vectors check both again in WP-4.

#include "aldis/core/crc.hpp"
#include "aldis/core/oid.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string_view>
#include <vector>

namespace {

namespace crc = aldis::core::crc;
using aldis::core::ErrorDetail;
using aldis::core::ErrorKind;
using aldis::core::OidGenerator;
using aldis::core::OidGeneratorGalois;
using aldis::test::octets;
using aldis::test::Rng;
using aldis::test::unwrap_err;
using aldis::test::vec;

std::vector<std::byte> ascii(std::string_view s) {
    std::vector<std::byte> out;
    for (const char c : s) {
        out.push_back(std::byte{static_cast<std::uint8_t>(c)});
    }
    return out;
}

TEST(Crc, CheckValueAndIncremental) {
    // req: CMN-001, CMN-002
    constexpr auto check = octets('1', '2', '3', '4', '5', '6', '7', '8', '9');
    static_assert(crc::crc16_bitwise(check) == 0x29B1);
    static_assert(crc::crc16_table(check) == 0x29B1);
    const std::vector<std::byte> data = ascii("123456789");
    EXPECT_EQ(crc::crc16_bitwise(data), 0x29B1U);
    EXPECT_EQ(crc::crc16(data), 0x29B1U);
    EXPECT_EQ(crc::crc16(std::span<const std::byte>{}), crc::preset);
    const std::uint16_t mid = crc::crc16_update(crc::preset, ascii("1234"));
    EXPECT_EQ(crc::crc16_update(mid, ascii("56789")), 0x29B1U);
    EXPECT_EQ(crc::crc16_update_bitwise(crc::crc16_update_bitwise(crc::preset, ascii("1234")),
                                        ascii("56789")),
              0x29B1U);
    EXPECT_EQ(crc::crc16_update_table(crc::crc16_update_table(crc::preset, ascii("1234")),
                                      ascii("56789")),
              0x29B1U);
}

TEST(Crc, FecfWriteVerifyAndSyndrome) {
    // req: CMN-003
    std::vector<std::byte> frame = ascii("123456789");
    frame.resize(11);
    ASSERT_TRUE(crc::write_fecf(frame).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{frame}.last(2)), vec(octets(0x29, 0xB1)));
    EXPECT_TRUE(crc::verify_fecf(frame).is_ok());
    EXPECT_EQ(crc::syndrome(frame), 0U);
    frame.at(0) ^= std::byte{0x01};
    EXPECT_EQ(unwrap_err(crc::verify_fecf(frame)).kind(), ErrorKind::CrcMismatch);
    EXPECT_NE(crc::syndrome(frame), 0U);
    const auto incomplete = unwrap_err(crc::verify_fecf(octets(0x00)));
    EXPECT_EQ(incomplete.kind(), ErrorKind::Incomplete);
    EXPECT_EQ(incomplete.detail(), ErrorDetail::needed(2));
    std::array<std::byte, 1> one{};
    const auto small = unwrap_err(crc::write_fecf(one));
    EXPECT_EQ(small.kind(), ErrorKind::BufferTooSmall);
    EXPECT_EQ(small.detail(), ErrorDetail::needed(2));
    std::array<std::byte, 2> empty_body{};
    ASSERT_TRUE(crc::write_fecf(empty_body).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{empty_body}), vec(octets(0xFF, 0xFF)));
}

TEST(Crc, BothVerificationMethodsAgree) {
    // req: CMN-003
    Rng rng{0x0C0C};
    for (int i = 0; i < 2'000; ++i) {
        std::vector<std::byte> frame = rng.bytes(rng.range(0, 299));
        frame.resize(frame.size() + 2);
        ASSERT_TRUE(crc::write_fecf(frame).is_ok());
        EXPECT_EQ(crc::syndrome(frame), 0U);
        EXPECT_TRUE(crc::verify_fecf(frame).is_ok());
        const std::size_t bit = rng.range(0, frame.size() * 8 - 1);
        frame.at(bit / 8) ^= std::byte{static_cast<std::uint8_t>(0x80U >> (bit % 8))};
        EXPECT_EQ(crc::verify_fecf(frame).is_ok(), crc::syndrome(frame) == 0);
    }
}

TEST(Crc, TableEqualsBitwise) {
    // req: CMN-004
    Rng rng{0x7AB1E};
    for (int i = 0; i < 2'000; ++i) {
        const std::vector<std::byte> data = rng.bytes(rng.range(0, 299));
        EXPECT_EQ(crc::crc16_table(data), crc::crc16_bitwise(data));
    }
}

constexpr auto annex_d = octets(0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6, 0xD8, 0x61, 0x45, 0x1F, 0x11,
                                0xF1, 0x97, 0x16, 0x72, 0x3C, 0xBE, 0x7E, 0x00, 0xB1);

TEST(Oid, BothFormsReproduceAnnexD) {
    // req: CMN-020, CMN-022
    std::array<std::byte, 20> fib{};
    OidGenerator{}.fill(fib);
    EXPECT_EQ(vec(std::span<const std::byte>{fib}), vec(std::span{annex_d}));
    std::array<std::byte, 20> gal{};
    OidGeneratorGalois{}.fill(gal);
    EXPECT_EQ(vec(std::span<const std::byte>{gal}), vec(std::span{annex_d}));
}

TEST(Oid, SequenceContinuesAcrossFills) {
    // req: CMN-021, CMN-023
    OidGenerator g;
    std::array<std::byte, 7> a{};
    std::array<std::byte, 13> b{};
    g.fill(a);
    g.fill(b);
    EXPECT_EQ(vec(std::span<const std::byte>{a}), vec(std::span{annex_d}.first(7)));
    EXPECT_EQ(vec(std::span<const std::byte>{b}), vec(std::span{annex_d}.subspan(7)));
    g.fill({});
    EXPECT_NE(g, OidGenerator{});
    OidGeneratorGalois h;
    h.fill(a);
    EXPECT_NE(h, OidGeneratorGalois{});
    EXPECT_EQ(OidGeneratorGalois{}, OidGeneratorGalois{});
}

TEST(Oid, FormsAreIdenticalOverALongRun) {
    // req: CMN-022
    OidGenerator fib;
    OidGeneratorGalois gal;
    for (int i = 0; i < 1'000'000; ++i) {
        ASSERT_EQ(fib.next_octet(), gal.next_octet()) << i;
    }
}

TEST(Oid, ConstantEvaluation) {
    // req: CPP-034, CMN-020
    static_assert([] {
        OidGenerator g;
        for (int i = 0; i < 4; ++i) {
            if (g.next_octet() != 0xFF) {
                return false;
            }
        }
        return g.next_octet() == 0x6D;
    }());
    SUCCEED();
}

} // namespace
