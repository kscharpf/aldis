// Tests of aldis/core/security.hpp and layout.hpp, ported from aldis-core `security.rs` and
// `layout.rs`. The Rust property test of the layout becomes a seeded randomized test (D4).

#include "aldis/core/layout.hpp"
#include "aldis/core/security.hpp"
#include "aldis/core/testing/fixed_pattern_security.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <type_traits>

namespace {

using aldis::core::ErrorKind;
using aldis::core::FieldId;
using aldis::core::FrameCtx;
using aldis::core::FrameLayout;
using aldis::core::LayoutSpec;
using aldis::core::MapId;
using aldis::core::NoSecurity;
using aldis::core::OctetRange;
using aldis::core::Protocol;
using aldis::core::SecuredFields;
using aldis::core::SecurityHook;
using aldis::core::SecurityLayout;
using aldis::core::SpacecraftId;
using aldis::core::VcId;
using aldis::core::testing::FixedPatternSecurity;
using aldis::test::octets;
using aldis::test::Rng;
using aldis::test::unwrap;
using aldis::test::unwrap_err;
using aldis::test::vec;

OctetRange range(std::size_t start, std::size_t end) {
    return unwrap(OctetRange::make(start, end));
}

const FrameCtx ctx = FrameCtx::tm(unwrap(SpacecraftId<10>::make(42)), unwrap(VcId<3>::make(1)));

TEST(Security, FrameContextsCarryTypedIds) {
    // req: API-002, API-073, CPP-023
    const FrameCtx tc = FrameCtx::tc(unwrap(SpacecraftId<10>::make(1023)),
                                     unwrap(VcId<6>::make(63)), unwrap(MapId<6>::make(63)));
    const FrameCtx aos =
        FrameCtx::aos(unwrap(SpacecraftId<10>::make(255)), unwrap(VcId<6>::make(62)));
    const FrameCtx uslp = FrameCtx::uslp(unwrap(SpacecraftId<16>::make(65'535)),
                                         unwrap(VcId<6>::make(5)), unwrap(MapId<4>::make(15)));
    const FrameCtx uslp_vc =
        FrameCtx::uslp(unwrap(SpacecraftId<16>::make(7)), unwrap(VcId<6>::make(6)), std::nullopt);
    const FrameCtx tc_vc =
        FrameCtx::tc(unwrap(SpacecraftId<10>::make(9)), unwrap(VcId<6>::make(2)), std::nullopt);
    EXPECT_EQ(ctx.protocol(), Protocol::Tm);
    EXPECT_EQ(ctx.scid(), 42U);
    EXPECT_EQ(ctx.vcid(), 1U);
    EXPECT_EQ(ctx.map_id(), std::nullopt);
    EXPECT_EQ(tc.protocol(), Protocol::Tc);
    EXPECT_EQ(tc.scid(), 1023U);
    EXPECT_EQ(tc.vcid(), 63U);
    EXPECT_EQ(tc.map_id(), std::uint8_t{63});
    EXPECT_EQ(aos.protocol(), Protocol::Aos);
    EXPECT_EQ(aos.scid(), 255U);
    EXPECT_EQ(aos.vcid(), 62U);
    EXPECT_EQ(aos.map_id(), std::nullopt);
    EXPECT_EQ(uslp.protocol(), Protocol::Uslp);
    EXPECT_EQ(uslp.scid(), 65'535U);
    EXPECT_EQ(uslp.vcid(), 5U);
    EXPECT_EQ(uslp.map_id(), std::uint8_t{15});
    EXPECT_EQ(uslp_vc.map_id(), std::nullopt);
    EXPECT_EQ(tc_vc.map_id(), std::nullopt);
    EXPECT_EQ(tc_vc.vcid(), 2U);
    EXPECT_EQ(uslp_vc.scid(), 7U);
    EXPECT_NE(tc, tc_vc);
}

TEST(Security, SecuredFieldsAreOrderedAndAdjacent) {
    // req: API-070, API-073, CPP-023
    const SecuredFields f = unwrap(SecuredFields::make(range(1, 3), range(3, 5), range(5, 7)));
    EXPECT_EQ(f.header(), range(1, 3));
    EXPECT_EQ(f.data(), range(3, 5));
    EXPECT_EQ(f.trailer(), range(5, 7));
    EXPECT_TRUE(SecuredFields::make(range(0, 0), range(0, 4), range(4, 4)).is_ok());
    // Reversed ranges cannot be built at all (OctetRange, CPP-023).
    EXPECT_FALSE(OctetRange::make(3, 1).is_ok());
    struct Case {
        OctetRange h;
        OctetRange d;
        OctetRange t;
    };
    for (const Case c : {Case{range(1, 3), range(4, 5), range(5, 7)},
                         Case{range(1, 3), range(3, 5), range(6, 7)}}) {
        const auto e = unwrap_err(SecuredFields::make(c.h, c.d, c.t));
        EXPECT_EQ(e.kind(), ErrorKind::InvalidField);
        EXPECT_EQ(e.field(), FieldId{"secured_fields"});
    }
    const FrameLayout layout = unwrap(FrameLayout::compute(LayoutSpec{
        .frame_len = 20,
        .primary_header_len = 6,
        .secondary_len = 0,
        .security = unwrap(SecurityLayout::make(2, 3)),
        .ocf = false,
        .fecf = true,
    }));
    const SecuredFields from_layout = layout.secured_fields();
    EXPECT_EQ(from_layout.header(), range(6, 8));
    EXPECT_EQ(from_layout.data(), range(8, 15));
    EXPECT_EQ(from_layout.trailer(), range(15, 18));
    EXPECT_EQ(from_layout, unwrap(SecuredFields::make(range(6, 8), range(8, 15), range(15, 18))));
}

TEST(Security, LayoutLimits) {
    // req: API-070, API-071, API-075, CPP-025
    static_assert(sizeof(SecurityLayout) == 4);
    static_assert(std::is_trivially_copyable_v<SecurityLayout>);
    static_assert(SecurityLayout::make(6, 16).value_or(SecurityLayout{}) ==
                  SecurityLayout::uslp_truncated());
    EXPECT_TRUE(SecurityLayout::none().is_none());
    EXPECT_EQ(SecurityLayout{}, SecurityLayout::none());
    const SecurityLayout l = unwrap(
        SecurityLayout::make(SecurityLayout::max_header_len, SecurityLayout::max_trailer_len));
    EXPECT_EQ(l.header_len(), 64U);
    EXPECT_EQ(l.trailer_len(), 0xFFFFU);
    EXPECT_FALSE(l.is_none());
    // Either length alone makes the layout non-empty.
    EXPECT_FALSE(unwrap(SecurityLayout::make(6, 0)).is_none());
    EXPECT_FALSE(unwrap(SecurityLayout::make(0, 16)).is_none());
    const auto header = unwrap_err(SecurityLayout::make(65, 0));
    EXPECT_EQ(header.kind(), ErrorKind::InvalidConfig);
    EXPECT_EQ(header.field(), FieldId{"security_header_len"});
    EXPECT_EQ(unwrap_err(SecurityLayout::make(300, 0)).kind(), ErrorKind::InvalidConfig);
    const auto trailer = unwrap_err(SecurityLayout::make(0, 70'000));
    EXPECT_EQ(trailer.kind(), ErrorKind::InvalidConfig);
    EXPECT_EQ(trailer.field(), FieldId{"security_trailer_len"});
    EXPECT_EQ(SecurityLayout::uslp_truncated(), unwrap(SecurityLayout::make(6, 16)));
}

TEST(Security, Hooks) {
    // req: API-073, CPP-024
    static_assert(SecurityHook<NoSecurity>);
    static_assert(SecurityHook<FixedPatternSecurity>);
    static_assert(!SecurityHook<int>);
    const SecuredFields fields = unwrap(SecuredFields::make(range(1, 3), range(3, 5), range(5, 7)));
    std::array<std::byte, 8> frame{};
    NoSecurity none;
    ASSERT_TRUE(none.protect(ctx, frame, fields).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{frame}), vec(octets(0, 0, 0, 0, 0, 0, 0, 0)));
    EXPECT_EQ(unwrap(none.verify(ctx, frame, fields)), range(3, 5));
    EXPECT_EQ(none, NoSecurity{});
    FixedPatternSecurity hook;
    ASSERT_TRUE(hook.protect(ctx, frame, fields).is_ok());
    EXPECT_EQ(vec(std::span<const std::byte>{frame}),
              vec(octets(0, 0xA5, 0xA5, 0, 0, 0x5A, 0x5A, 0)));
    EXPECT_EQ(unwrap(hook.verify(ctx, frame, fields)), range(3, 5));
    frame.at(6) = std::byte{0};
    EXPECT_EQ(unwrap_err(hook.verify(ctx, frame, fields)).kind(), ErrorKind::Security);
    frame.at(6) = std::byte{0x5A};
    frame.at(1) = std::byte{0};
    EXPECT_EQ(unwrap_err(hook.verify(ctx, frame, fields)).kind(), ErrorKind::Security);
    const SecuredFields outside =
        unwrap(SecuredFields::make(range(7, 9), range(9, 9), range(9, 9)));
    EXPECT_EQ(unwrap_err(hook.protect(ctx, frame, outside)).kind(), ErrorKind::Security);
    EXPECT_EQ(unwrap_err(hook.verify(ctx, frame, outside)).kind(), ErrorKind::Security);
    const SecuredFields tail_outside =
        unwrap(SecuredFields::make(range(1, 3), range(3, 7), range(7, 9)));
    EXPECT_EQ(unwrap_err(hook.protect(ctx, frame, tail_outside)).kind(), ErrorKind::Security);
    EXPECT_EQ(unwrap_err(hook.verify(ctx, frame, tail_outside)).kind(), ErrorKind::Security);
}

TEST(Layout, TmFrameWithEverything) {
    // req: CMN-040
    const LayoutSpec spec{
        .frame_len = 1115,
        .primary_header_len = 6,
        .secondary_len = 8,
        .security = unwrap(SecurityLayout::make(6, 8)),
        .ocf = true,
        .fecf = true,
    };
    const FrameLayout l = unwrap(FrameLayout::compute(spec));
    const std::array expected{range(0, 6),      range(6, 14),      range(14, 20),
                              range(20, 1101),  range(1101, 1109), range(1109, 1113),
                              range(1113, 1115)};
    EXPECT_EQ(l.ranges(), expected);
    EXPECT_EQ(l.primary_header(), range(0, 6));
    EXPECT_EQ(l.secondary(), range(6, 14));
    EXPECT_EQ(l.security_header(), range(14, 20));
    EXPECT_EQ(l.data(), range(20, 1101));
    EXPECT_EQ(l.security_trailer(), range(1101, 1109));
    EXPECT_EQ(l.ocf(), range(1109, 1113));
    EXPECT_EQ(l.fecf(), range(1113, 1115));
    EXPECT_EQ(l.frame_len(), 1115U);
    EXPECT_EQ(l, unwrap(FrameLayout::compute(spec)));
}

TEST(Layout, RejectsEmptyDataField) {
    // req: CMN-041
    LayoutSpec spec{
        .frame_len = 12,
        .primary_header_len = 6,
        .secondary_len = 0,
        .security = SecurityLayout::none(),
        .ocf = true,
        .fecf = true,
    };
    const auto e = unwrap_err(FrameLayout::compute(spec));
    EXPECT_EQ(e.kind(), ErrorKind::InvalidConfig);
    EXPECT_EQ(e.field(), FieldId{"data"});
    spec.frame_len = 13;
    EXPECT_EQ(unwrap(FrameLayout::compute(spec)).data(), range(6, 7));
    spec.frame_len = 3;
    EXPECT_EQ(unwrap_err(FrameLayout::compute(spec)).kind(), ErrorKind::InvalidConfig);
    spec.frame_len = 6;
    EXPECT_EQ(unwrap_err(FrameLayout::compute(spec)).kind(), ErrorKind::InvalidConfig);
    spec.primary_header_len = SIZE_MAX;
    EXPECT_EQ(unwrap_err(FrameLayout::compute(spec)).kind(), ErrorKind::InvalidConfig);
    EXPECT_NE(spec, LayoutSpec{});
}

TEST(Layout, RangesAreContiguousAndCoverTheFrame) {
    // req: CMN-040, CMN-041
    Rng rng{0x1A7};
    for (int i = 0; i < 20'000; ++i) {
        const std::size_t frame_len = rng.range(0, 1999);
        const std::size_t ph = rng.range(0, 15);
        const std::size_t sec = rng.range(0, 69);
        const std::size_t sh = rng.range(0, 64);
        const std::size_t st = rng.range(0, 39);
        const bool ocf = rng.range(0, 1) == 1;
        const bool fecf = rng.range(0, 1) == 1;
        const LayoutSpec spec{frame_len, ph, sec, unwrap(SecurityLayout::make(sh, st)), ocf, fecf};
        const std::size_t fixed = ph + sec + sh + st + (ocf ? 4 : 0) + (fecf ? 2 : 0);
        const auto result = FrameLayout::compute(spec);
        if (result.is_ok()) {
            const FrameLayout l = unwrap(result);
            const auto r = l.ranges();
            EXPECT_EQ(r.at(0).start(), 0U);
            for (std::size_t k = 0; k + 1 < r.size(); ++k) {
                EXPECT_EQ(r.at(k).end(), r.at(k + 1).start());
            }
            EXPECT_EQ(r.at(6).end(), frame_len);
            EXPECT_EQ(l.data().len(), frame_len - fixed);
            EXPECT_EQ(l.frame_len(), frame_len);
            EXPECT_FALSE(l.data().is_empty());
        } else {
            EXPECT_EQ(unwrap_err(result).kind(), ErrorKind::InvalidConfig);
            EXPECT_LE(frame_len, fixed);
        }
    }
}

TEST(Layout, ConstantEvaluation) {
    // req: CPP-034, CMN-040
    constexpr LayoutSpec spec{.frame_len = 10, .primary_header_len = 6, .fecf = true};
    static_assert(FrameLayout::compute(spec).is_ok());
    static_assert(
        !FrameLayout::compute(LayoutSpec{.frame_len = 8, .primary_header_len = 6, .fecf = true})
             .is_ok());
    SUCCEED();
}

} // namespace
