// Tests of aldis/core/mode.hpp and aldis/core/anomaly.hpp, ported from aldis-core `mode.rs` and
// `anomaly.rs`. The Rust tests of deliberately broken `Anomaly` implementations become
// compile-time checks here: the concept rejects such tables.

#include "aldis/core/anomaly.hpp"
#include "aldis/core/mode.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstdint>
#include <optional>
#include <vector>

namespace {

using aldis::core::Anomaly;
using aldis::core::AnomalyFlag;
using aldis::core::AnomalySet;
using aldis::core::DecodeMode;
namespace anomaly = aldis::core::anomaly;

/// Flags declared out of index order, as in the Rust test `Demo`.
enum class Demo : std::uint8_t { Four, Zero, Last };

/// One flag per index 0–31, the counterpart of the Rust `Flag(u8)` for valid indices.
enum class Flag : std::uint8_t {};

/// Index 32: not representable.
enum class TooHigh : std::uint8_t { Flag };

/// Index 3 used twice.
enum class Twice : std::uint8_t { First, Second };

/// The same flag listed twice.
enum class Repeated : std::uint8_t { Only };

/// No traits at all.
enum class NoTraits : std::uint8_t { X };

} // namespace

template <> struct aldis::core::AnomalyTraits<Demo> {
    static constexpr std::array<AnomalyFlag<Demo>, 3> flags{{
        {.flag = Demo::Four, .bit = 4},
        {.flag = Demo::Zero, .bit = 0},
        {.flag = Demo::Last, .bit = 31},
    }};
};

template <> struct aldis::core::AnomalyTraits<Flag> {
    static constexpr std::array<AnomalyFlag<Flag>, 32> flags = [] {
        std::array<AnomalyFlag<Flag>, 32> table{};
        std::uint8_t bit = 0;
        for (AnomalyFlag<Flag>& f : table) {
            f = {.flag = static_cast<Flag>(bit), .bit = bit};
            ++bit;
        }
        return table;
    }();
};

template <> struct aldis::core::AnomalyTraits<TooHigh> {
    static constexpr std::array<AnomalyFlag<TooHigh>, 1> flags{
        {{.flag = TooHigh::Flag, .bit = 32}}};
};

template <> struct aldis::core::AnomalyTraits<Twice> {
    static constexpr std::array<AnomalyFlag<Twice>, 2> flags{
        {{Twice::First, 3}, {Twice::Second, 3}}};
};

template <> struct aldis::core::AnomalyTraits<Repeated> {
    static constexpr std::array<AnomalyFlag<Repeated>, 2> flags{{
        {.flag = Repeated::Only, .bit = 1},
        {.flag = Repeated::Only, .bit = 2},
    }};
};

namespace {

Flag flag(std::uint8_t bit) { return static_cast<Flag>(bit); }

std::vector<Flag> flags_of(AnomalySet<Flag> set) { return {set.begin(), set.end()}; }

TEST(Mode, DefaultIsStrict) {
    // req: API-030
    EXPECT_EQ(DecodeMode{}, DecodeMode::Strict);
    EXPECT_NE(DecodeMode::Strict, DecodeMode::Lenient);
}

TEST(Anomaly, ConceptKeepsTheInvariants) {
    // req: GEN-043, GEN-081
    static_assert(Anomaly<Demo>);
    static_assert(Anomaly<Flag>);
    static_assert(!Anomaly<TooHigh>, "an index of 32 or more is rejected");
    static_assert(!Anomaly<Twice>, "an index used twice is rejected");
    static_assert(!Anomaly<Repeated>, "a flag listed twice is rejected");
    static_assert(!Anomaly<NoTraits>, "a type without a table is rejected");
    static_assert(!Anomaly<int>);
    // The same checks at run time, so that coverage sees them.
    EXPECT_TRUE(aldis::core::detail::valid_anomaly_table<Demo>());
    EXPECT_TRUE(aldis::core::detail::valid_anomaly_table<Flag>());
    EXPECT_FALSE(aldis::core::detail::valid_anomaly_table<TooHigh>());
    EXPECT_FALSE(aldis::core::detail::valid_anomaly_table<Twice>());
    EXPECT_FALSE(aldis::core::detail::valid_anomaly_table<Repeated>());
}

TEST(Anomaly, TableMapsBothWays) {
    // req: GEN-043, GEN-081
    EXPECT_EQ(anomaly::bit(Demo::Four), 4U);
    EXPECT_EQ(anomaly::bit(Demo::Zero), 0U);
    EXPECT_EQ(anomaly::bit(Demo::Last), 31U);
    for (unsigned b = 0; b < 256; ++b) {
        const auto bit = static_cast<std::uint8_t>(b);
        std::optional<Demo> expected;
        if (b == 4) {
            expected = Demo::Four;
        } else if (b == 0) {
            expected = Demo::Zero;
        } else if (b == 31) {
            expected = Demo::Last;
        }
        EXPECT_EQ(anomaly::from_bit<Demo>(bit), expected) << b;
    }
    const std::array all{Demo::Four, Demo::Zero, Demo::Last};
    const auto set = AnomalySet<Demo>::from_flags(all);
    EXPECT_EQ(set.bits(), 0x8000'0011U);
    const std::vector<Demo> in_order{set.begin(), set.end()};
    EXPECT_EQ(in_order, (std::vector{Demo::Zero, Demo::Four, Demo::Last}));
    // A value outside the enumerators has no index and cannot be inserted.
    auto none = AnomalySet<Demo>::empty();
    none.insert(static_cast<Demo>(9));
    EXPECT_TRUE(none.is_empty());
    EXPECT_EQ(anomaly::bit(static_cast<Demo>(9)), anomaly::capacity);
}

TEST(Anomaly, EmptyAndDefault) {
    // req: GEN-043
    const auto set = AnomalySet<Flag>::empty();
    EXPECT_TRUE(set.is_empty());
    EXPECT_EQ(set.size(), 0U);
    EXPECT_EQ(set.bits(), 0U);
    EXPECT_EQ(set.begin(), set.end());
    EXPECT_EQ(set, AnomalySet<Flag>{});
    EXPECT_EQ(set, AnomalySet<Flag>::from_flags({}));
    EXPECT_FALSE(set.contains(flag(0)));
}

TEST(Anomaly, InsertContainsBits) {
    // req: GEN-043
    AnomalySet<Flag> set;
    set.insert(flag(0));
    set.insert(flag(31));
    set.insert(flag(5));
    set.insert(flag(5));
    EXPECT_FALSE(set.is_empty());
    EXPECT_EQ(set.size(), 3U);
    EXPECT_EQ(set.bits(), 0x8000'0021U);
    EXPECT_TRUE(set.contains(flag(0)) && set.contains(flag(5)) && set.contains(flag(31)));
    EXPECT_FALSE(set.contains(flag(1)) || set.contains(flag(30)));
    const std::array listed{flag(5), flag(31), flag(0), flag(5)};
    EXPECT_EQ(set, AnomalySet<Flag>::from_flags(listed));
    AnomalySet<Flag> one;
    one.insert(flag(1));
    EXPECT_EQ(one.bits(), 0b10U);
    EXPECT_TRUE(one.contains(flag(1)) && !one.contains(flag(0)));
}

TEST(Anomaly, IteratesInBitOrder) {
    // req: GEN-043, GEN-022
    const std::array listed{flag(31), flag(2), flag(17), flag(0)};
    const auto set = AnomalySet<Flag>::from_flags(listed);
    auto it = set.begin();
    EXPECT_EQ(*it, flag(0));
    EXPECT_EQ(*++it, flag(2));
    const auto before = it++;
    EXPECT_EQ(*before, flag(2));
    EXPECT_EQ(*it, flag(17));
    ++it;
    EXPECT_EQ(*it, flag(31));
    ++it;
    EXPECT_EQ(it, set.end());
    EXPECT_EQ(flags_of(set), (std::vector{flag(0), flag(2), flag(17), flag(31)}));

    std::array<Flag, 32> all{};
    std::uint8_t bit = 0;
    for (Flag& f : all) {
        f = flag(bit++);
    }
    const auto full = AnomalySet<Flag>::from_flags(all);
    EXPECT_EQ(full.bits(), 0xFFFF'FFFFU);
    EXPECT_EQ(full.size(), 32U);
    EXPECT_EQ(flags_of(full), std::vector(all.begin(), all.end()));
}

TEST(Anomaly, EqualityAndCopy) {
    // req: GEN-043
    const std::array four{flag(4)};
    const auto a = AnomalySet<Flag>::from_flags(four);
    const auto b = a;
    EXPECT_EQ(a, b);
    EXPECT_NE(a, AnomalySet<Flag>::empty());
}

TEST(Anomaly, ConstantEvaluation) {
    // req: CPP-034, GEN-043
    constexpr std::array listed{Demo::Last, Demo::Zero};
    static_assert(AnomalySet<Demo>::from_flags(listed).bits() == 0x8000'0001U);
    static_assert(*AnomalySet<Demo>::from_flags(listed).begin() == Demo::Zero);
    SUCCEED();
}

} // namespace
