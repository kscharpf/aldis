// Tests of aldis/core/error.hpp, ported from aldis-core `error.rs`, plus the C++-only Result
// accessors (D8).

#include "aldis/core/error.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <type_traits>

namespace {

using aldis::core::Error;
using aldis::core::ErrorDetail;
using aldis::core::ErrorKind;
using aldis::core::Expected;
using aldis::core::FieldId;
using aldis::core::Result;
namespace error = aldis::core::error;
using aldis::test::chars;
using aldis::test::str;

TEST(Error, CodesAreDenseAndRoundTrip) {
    // req: GEN-040
    std::uint16_t expected_code = 1;
    for (const ErrorKind kind : error::all_kinds) {
        EXPECT_EQ(error::code(kind), expected_code);
        EXPECT_EQ(error::from_code(error::code(kind)), kind);
        EXPECT_EQ(error::from_name(error::name(kind)), kind);
        ++expected_code;
    }
    EXPECT_EQ(error::from_code(0), std::nullopt);
    EXPECT_EQ(error::from_code(18), std::nullopt);
    EXPECT_EQ(error::from_name(chars("NoSuchKind")), std::nullopt);
    EXPECT_EQ(error::from_name(chars("Malformedx")), std::nullopt);
    EXPECT_EQ(error::from_name(chars("")), std::nullopt);
    EXPECT_EQ(str(error::name(ErrorKind::CrcMismatch)), "CrcMismatch");
    // A value outside the catalogue has no name (GEN-081).
    // An out-of-catalogue value is the point of this check.
    // NOLINTNEXTLINE(clang-analyzer-optin.core.EnumCastOutOfRange)
    EXPECT_EQ(str(error::name(static_cast<ErrorKind>(99))), "");
}

TEST(Error, CatalogueCodes) {
    // req: GEN-040, CPP-021
    // The codes of docs/03-api-model.md §9; errors.json checks them again in WP-4.
    EXPECT_EQ(error::code(ErrorKind::BufferTooSmall), 1);
    EXPECT_EQ(error::code(ErrorKind::OutOfRange), 5);
    EXPECT_EQ(error::code(ErrorKind::CrcMismatch), 8);
    EXPECT_EQ(error::code(ErrorKind::Malformed), 17);
    static_assert(std::is_same_v<std::underlying_type_t<ErrorKind>, std::uint16_t>);
}

TEST(Error, DetailAndField) {
    // req: GEN-041, RS-011, CPP-021
    const Error e = Error::out_of_range(FieldId{"apid"}, 2047, 4096);
    EXPECT_EQ(e.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(e.field(), FieldId{"apid"});
    EXPECT_EQ(e.detail(), ErrorDetail::expected(2047, 4096));
    EXPECT_EQ(e.detail().kind(), ErrorDetail::Kind::Expected);
    EXPECT_EQ(e.detail().expected(), (Expected{2047, 4096}));
    EXPECT_EQ(e.detail().needed(), std::nullopt);

    const Error small = Error::buffer_too_small(9);
    EXPECT_EQ(small.kind(), ErrorKind::BufferTooSmall);
    EXPECT_EQ(small.detail().needed(), std::size_t{9});
    EXPECT_EQ(small.detail().expected(), std::nullopt);
    EXPECT_EQ(small.field(), std::nullopt);
    EXPECT_EQ(Error::incomplete(3).detail(), ErrorDetail::needed(3));

    const Error plain{ErrorKind::Malformed};
    EXPECT_EQ(plain.detail(), ErrorDetail::none());
    EXPECT_EQ(plain.detail().kind(), ErrorDetail::Kind::None);
    EXPECT_EQ(plain.with_field(FieldId{"x"}).field(), FieldId{"x"});
    EXPECT_NE(plain, Error{ErrorKind::Security});
}

TEST(Error, FieldIdsCompareByContent) {
    // req: GEN-041
    constexpr FieldId a{"apid"};
    constexpr FieldId b{"apid"};
    static_assert(a == b);
    static_assert(!(FieldId{"apid"} == FieldId{"apix"}));
    static_assert(!(FieldId{"apid"} == FieldId{"api"}));
    EXPECT_EQ(str(a.name()), "apid");
    EXPECT_EQ(str(FieldId{""}.name()), "");
    EXPECT_EQ(FieldId{"abcdefghijklmnopqrstuvwxyz012345"}.name().size(), 32U);
    // Names longer than any catalogued name never compare equal: every comparison is bounded.
    const std::string long_name(40, 'x');
    EXPECT_FALSE(aldis::core::detail::names_equal(chars(long_name), chars(long_name)));
}

TEST(Result, AccessorsAreNullInTheWrongState) {
    // req: CPP-021
    const Result<int> ok = 7;
    ASSERT_NE(ok.ok(), nullptr);
    EXPECT_EQ(aldis::test::deref(ok.ok()), 7);
    EXPECT_EQ(ok.error(), nullptr);
    EXPECT_TRUE(ok.is_ok());
    EXPECT_EQ(ok.value_or(1), 7);

    const Result<int> err = Error{ErrorKind::Security};
    EXPECT_EQ(err.ok(), nullptr);
    ASSERT_NE(err.error(), nullptr);
    EXPECT_EQ(aldis::test::deref(err.error()).kind(), ErrorKind::Security);
    EXPECT_FALSE(err.is_ok());
    EXPECT_EQ(err.value_or(1), 1);

    const Result<void> done;
    EXPECT_TRUE(done.is_ok());
    EXPECT_EQ(done.error(), nullptr);
    const Result<void> failed = Error{ErrorKind::WouldBlock};
    EXPECT_FALSE(failed.is_ok());
    ASSERT_NE(failed.error(), nullptr);
    EXPECT_EQ(aldis::test::deref(failed.error()).kind(), ErrorKind::WouldBlock);
}

TEST(Result, IsTriviallyCopyableAndConstexpr) {
    // req: CPP-021, CPP-034
    static_assert(std::is_trivially_copyable_v<Error>);
    static_assert(std::is_trivially_copyable_v<Result<std::uint64_t>>);
    static_assert(std::is_trivially_copyable_v<Result<void>>);
    constexpr Result<int> r = Error::incomplete(2);
    static_assert(r.error() != nullptr && r.error()->detail() == ErrorDetail::needed(2));
    static_assert(Result<int>{3}.value_or(0) == 3);
    static_assert(error::from_name(error::name(ErrorKind::InvalidHandle)) ==
                  ErrorKind::InvalidHandle);
    SUCCEED();
}

} // namespace
