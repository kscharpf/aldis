// Tests of aldis/core/bytes.hpp, the audited byte- and element-access module (D9).

#include "aldis/core/bytes.hpp"
#include "aldis/core/error.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>

namespace {

namespace bytes = aldis::core::bytes;
using aldis::core::ByteReader;
using aldis::core::ByteWriter;
using aldis::core::ErrorDetail;
using aldis::core::ErrorKind;
using aldis::core::FieldId;
using aldis::core::OctetRange;
using aldis::test::octets;
using aldis::test::unwrap;
using aldis::test::unwrap_err;
using aldis::test::vec;

TEST(Bytes, ElementAccessIsChecked) {
    // req: CPP-031, GEN-020
    std::array<int, 3> a{1, 2, 3};
    const std::span<int> s{a};
    ASSERT_NE(bytes::at(s, 2), nullptr);
    EXPECT_EQ(aldis::test::deref(bytes::at(s, 2)), 3);
    EXPECT_EQ(bytes::at(s, 3), nullptr);
    EXPECT_EQ(bytes::at(std::span<int>{}, 0), nullptr);
    EXPECT_EQ(bytes::get_or(std::span<const int>{a}, 1, -1), 2);
    EXPECT_EQ(bytes::get_or(std::span<const int>{a}, 9, -1), -1);
    EXPECT_TRUE(bytes::put(s, 0, 7));
    EXPECT_FALSE(bytes::put(s, 3, 7));
    EXPECT_EQ(a.at(0), 7);
}

TEST(Bytes, SubspansAreChecked) {
    // req: CPP-031, GEN-013
    const auto buf = octets(1, 2, 3, 4, 5);
    const std::span<const std::byte> s{buf};
    EXPECT_EQ(vec(unwrap(bytes::subspan(s, 1, 3))), vec(octets(2, 3, 4)));
    EXPECT_EQ(unwrap(bytes::subspan(s, 5, 0)).size(), 0U);
    const auto e = unwrap_err(bytes::subspan(s, 4, 2));
    EXPECT_EQ(e.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(e.field(), FieldId{"range"});
    EXPECT_EQ(e.detail(), ErrorDetail::expected(5, 6));
    EXPECT_FALSE(bytes::subspan(s, 6, 0).is_ok());
    EXPECT_FALSE(bytes::subspan(s, SIZE_MAX, 2).is_ok());
    EXPECT_EQ(vec(unwrap(bytes::subspan(s, unwrap(OctetRange::make(3, 5))))), vec(octets(4, 5)));
    EXPECT_EQ(bytes::subspan_or_empty(s, 2, 2).size(), 2U);
    EXPECT_TRUE(bytes::subspan_or_empty(s, 4, 2).empty());
    EXPECT_TRUE(bytes::subspan_or_empty(s, 6, 0).empty());
}

TEST(Bytes, EqualCopyFill) {
    // req: CPP-031, GEN-042
    const auto a = octets(1, 2, 3);
    const auto b = octets(1, 2, 4);
    const std::span<const std::byte> sa{a};
    EXPECT_TRUE(bytes::equal(sa, sa));
    EXPECT_FALSE(bytes::equal(sa, std::span<const std::byte>{b}));
    EXPECT_FALSE(bytes::equal(sa, sa.first(2)));

    std::array<std::byte, 4> dst{};
    ASSERT_TRUE(bytes::copy(std::span<std::byte>{dst}, sa).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{dst}), vec(octets(1, 2, 3, 0)));
    std::array<std::byte, 2> small{};
    const auto e = unwrap_err(bytes::copy(std::span<std::byte>{small}, sa));
    EXPECT_EQ(e.kind(), ErrorKind::BufferTooSmall);
    EXPECT_EQ(e.detail(), ErrorDetail::needed(3));
    EXPECT_EQ(vec(std::span<const std::byte>{small}), vec(octets(0, 0)));
    bytes::fill(std::span<std::byte>{dst}, std::byte{0xAA});
    EXPECT_EQ(vec(std::span<const std::byte>{dst}), vec(octets(0xAA, 0xAA, 0xAA, 0xAA)));
}

TEST(Bytes, OctetRange) {
    // req: CPP-023
    const OctetRange r = unwrap(OctetRange::make(3, 7));
    EXPECT_EQ(r.start(), 3U);
    EXPECT_EQ(r.end(), 7U);
    EXPECT_EQ(r.len(), 4U);
    EXPECT_FALSE(r.is_empty());
    EXPECT_TRUE(OctetRange{}.is_empty());
    const auto e = unwrap_err(OctetRange::make(7, 3));
    EXPECT_EQ(e.kind(), ErrorKind::InvalidField);
    EXPECT_EQ(e.field(), FieldId{"range"});
    EXPECT_EQ(OctetRange::from_len(2, 3), unwrap(OctetRange::make(2, 5)));
    EXPECT_EQ(OctetRange::from_len(SIZE_MAX, 3).end(), SIZE_MAX);
}

TEST(Bytes, ReaderIsBigEndianAndChecked) {
    // req: GEN-050, CPP-031, GEN-042
    const auto buf = octets(0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x10, 0x32, 0x54, 0x76,
                            0x98, 0xBA, 0xDC, 0xFE, 0x42);
    ByteReader r{buf};
    EXPECT_EQ(unwrap(r.read_u8()), 0x01U);
    EXPECT_EQ(unwrap(r.read_u16()), 0x2345U);
    EXPECT_EQ(unwrap(r.read_u32()), 0x6789'ABCDU);
    EXPECT_EQ(unwrap(r.read_u64()), 0xEF10'3254'7698'BADCULL);
    EXPECT_EQ(r.position(), 15U);
    EXPECT_EQ(r.remaining(), 2U);
    const auto e = unwrap_err(r.read_u32());
    EXPECT_EQ(e.kind(), ErrorKind::Incomplete);
    EXPECT_EQ(e.detail(), ErrorDetail::needed(4));
    EXPECT_EQ(r.position(), 15U); // a failed read does not advance
    EXPECT_EQ(vec(unwrap(r.read(2))), vec(octets(0xFE, 0x42)));
    EXPECT_EQ(r.remaining(), 0U);
    EXPECT_FALSE(r.read_u8().is_ok());
}

TEST(Bytes, WriterIsBigEndianAndChecked) {
    // req: GEN-050, GEN-042
    std::array<std::byte, 16> buf{};
    ByteWriter w{buf};
    ASSERT_TRUE(w.write_u8(0x01).is_ok());
    ASSERT_TRUE(w.write_u16(0x2345).is_ok());
    ASSERT_TRUE(w.write_u32(0x6789'ABCD).is_ok());
    ASSERT_TRUE(w.write_u64(0xEF10'3254'7698'BADCULL).is_ok());
    EXPECT_EQ(w.position(), 15U);
    EXPECT_EQ(w.remaining(), 1U);
    const auto e = unwrap_err(w.write_u16(0xFFFF));
    EXPECT_EQ(e.kind(), ErrorKind::BufferTooSmall);
    EXPECT_EQ(e.detail(), ErrorDetail::needed(17));
    EXPECT_EQ(w.position(), 15U);
    ASSERT_TRUE(w.write(octets(0x42)).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{buf}),
              vec(octets(0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x10, 0x32, 0x54, 0x76,
                         0x98, 0xBA, 0xDC, 0x42)));
}

TEST(Bytes, ConstantEvaluation) {
    // req: CPP-034, CPP-031
    constexpr auto buf = octets(0xCA, 0xFE);
    static_assert(ByteReader{buf}.read_u16().value_or(0) == 0xCAFE);
    static_assert(!bytes::subspan(std::span<const std::byte>{buf}, 1, 2).is_ok());
    SUCCEED();
}

} // namespace
