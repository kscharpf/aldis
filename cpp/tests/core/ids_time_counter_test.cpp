// Tests of aldis/core/ids.hpp, time.hpp, and counter.hpp, ported from aldis-core `ids.rs`,
// `time.rs`, and `counter.rs`. The Rust property test of `ModCounter` becomes a seeded
// randomized test against a model that needs no 128-bit integer (D4).

#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"
#include "aldis/core/ids.hpp"
#include "aldis/core/time.hpp"

#include "support/support.hpp"

#include <gtest/gtest.h>

#include <cstdint>
#include <limits>
#include <optional>
#include <type_traits>

namespace {

using aldis::core::Apid;
using aldis::core::Continuity;
using aldis::core::Duration;
using aldis::core::ErrorDetail;
using aldis::core::ErrorKind;
using aldis::core::FieldId;
using aldis::core::FrameSeqNum;
using aldis::core::Instant;
using aldis::core::MapId;
using aldis::core::ModCounter;
using aldis::core::PacketVersion;
using aldis::core::SeqCount;
using aldis::core::SpacecraftId;
using aldis::core::VcId;
using aldis::test::Rng;
using aldis::test::unwrap;
using aldis::test::unwrap_err;

constexpr std::uint64_t u64_max = std::numeric_limits<std::uint64_t>::max();

TEST(Ids, WidthsAreEnforced) {
    // req: API-001, API-002, CPP-020
    EXPECT_EQ(SpacecraftId<10>::max_value, 1023U);
    EXPECT_EQ(SpacecraftId<8>::max_value, 255U);
    EXPECT_EQ(SpacecraftId<16>::max_value, 0xFFFFU);
    EXPECT_TRUE(SpacecraftId<10>::make(1023).is_ok());
    EXPECT_FALSE(SpacecraftId<10>::make(1024).is_ok());
    EXPECT_TRUE(SpacecraftId<16>::make(0xFFFF).is_ok());
    EXPECT_EQ(VcId<3>::max_value, 7U);
    EXPECT_FALSE(VcId<3>::make(8).is_ok());
    EXPECT_EQ(VcId<6>::max_value, 63U);
    EXPECT_EQ(MapId<4>::max_value, 15U);
    EXPECT_TRUE(MapId<6>::make(63).is_ok());
    EXPECT_FALSE(MapId<6>::make(64).is_ok());
    const auto e = unwrap_err(SpacecraftId<10>::make(2000));
    EXPECT_EQ(e.kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(e.field(), FieldId{"spacecraft_id"});
    EXPECT_EQ(e.detail(), ErrorDetail::expected(1023, 2000));
    EXPECT_EQ(unwrap_err(VcId<3>::make(8)).field(), FieldId{"vc_id"});
    EXPECT_EQ(unwrap_err(MapId<4>::make(16)).field(), FieldId{"map_id"});
    EXPECT_EQ(unwrap(SpacecraftId<10>::make(42)).value(), 42U);
    EXPECT_EQ(unwrap(VcId<6>::make(5)).value(), 5U);
    EXPECT_EQ(unwrap(MapId<4>::make(3)).value(), 3U);
    EXPECT_EQ(unwrap(VcId<6>::make(5)), unwrap(VcId<6>::make(5)));
    EXPECT_NE(unwrap(VcId<6>::make(5)), unwrap(VcId<6>::make(6)));
}

TEST(Ids, NoImplicitIntegerConversion) {
    // req: CPP-020, CPP-023
    static_assert(!std::is_convertible_v<std::uint16_t, Apid>);
    static_assert(!std::is_convertible_v<Apid, std::uint16_t>);
    static_assert(!std::is_constructible_v<Apid, std::uint16_t>);
    static_assert(!std::is_default_constructible_v<Apid>);
    static_assert(!std::is_constructible_v<SpacecraftId<10>, std::uint16_t>);
    static_assert(!std::is_convertible_v<std::uint8_t, FrameSeqNum>);
    static_assert(std::is_constructible_v<FrameSeqNum, std::uint8_t>); // full width (CPP-020)
    static_assert(std::is_trivially_copyable_v<Apid> && std::is_trivially_copyable_v<SeqCount>);
    SUCCEED();
}

TEST(Ids, PacketIdentifiers) {
    // req: API-001, CMN-031
    constexpr Continuity seq_cont = SeqCount::make(SeqCount::max_value)
                                        .value_or(SeqCount{})
                                        .classify(SeqCount::make(0).value_or(SeqCount{}));
    constexpr Continuity fsn_cont = FrameSeqNum{FrameSeqNum::max_value}.classify(FrameSeqNum{0});
    static_assert(seq_cont == Continuity::in_sequence());
    static_assert(fsn_cont == Continuity::in_sequence());
    EXPECT_TRUE(unwrap(Apid::make(2047)).is_idle());
    EXPECT_FALSE(unwrap(Apid::make(0)).is_idle());
    EXPECT_FALSE(Apid::make(2048).is_ok());
    EXPECT_EQ(Apid::idle().value(), 2047U);
    EXPECT_EQ(unwrap_err(Apid::make(4096)).kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(unwrap_err(Apid::make(4096)).field(), FieldId{"apid"});
    EXPECT_EQ(unwrap(Apid::make(7)).value(), 7U);
    const SeqCount s_max = unwrap(SeqCount::make(SeqCount::max_value));
    const SeqCount s0 = unwrap(SeqCount::make(0));
    EXPECT_EQ(s_max.next(), s0);
    EXPECT_EQ(s_max.classify(s0), Continuity::in_sequence());
    EXPECT_EQ(s_max.counter().value(), SeqCount::max_value);
    EXPECT_EQ(s_max.counter().bits(), 14U);
    EXPECT_EQ(unwrap(SeqCount::make(5)).next().value(), 6U);
    EXPECT_FALSE(SeqCount::make(16384).is_ok());
    EXPECT_EQ(unwrap_err(SeqCount::make(20000)).field(), FieldId{"sequence_count"});
    EXPECT_EQ(unwrap(SeqCount::make(1)).value(), 1U);
    EXPECT_EQ(SeqCount{}.value(), 0U);
    const FrameSeqNum f_max{FrameSeqNum::max_value};
    EXPECT_EQ(f_max.value(), 255U);
    EXPECT_EQ(f_max.next(), FrameSeqNum{0});
    EXPECT_EQ(f_max.next().value(), 0U);
    EXPECT_EQ(f_max.classify(FrameSeqNum{0}), Continuity::in_sequence());
    EXPECT_EQ(f_max.counter().value(), 255U);
    EXPECT_EQ(FrameSeqNum{}.value(), 0U);
    EXPECT_EQ(PacketVersion::space_packet().value(), 0U);
    EXPECT_EQ(PacketVersion::encapsulation().value(), 7U);
    EXPECT_FALSE(PacketVersion::make(8).is_ok());
    EXPECT_EQ(unwrap_err(PacketVersion::make(9)).field(), FieldId{"packet_version"});
    EXPECT_EQ(unwrap(PacketVersion::make(7)), PacketVersion::encapsulation());
}

TEST(Time, SaturatingAndMonotonic) {
    // req: API-003
    const Instant t = Instant::from_micros(1'000);
    EXPECT_EQ(t.as_micros(), 1'000U);
    EXPECT_EQ(t.saturating_add(Duration::from_millis(2)), Instant::from_micros(3'000));
    EXPECT_EQ(Instant::from_micros(u64_max).saturating_add(Duration::from_secs(1)).as_micros(),
              u64_max);
    EXPECT_EQ(Instant::from_micros(5).duration_since(Instant::from_micros(2)),
              Duration::from_micros(3));
    // Time going backwards is zero elapsed time, never a wrap.
    EXPECT_EQ(Instant::from_micros(2).duration_since(Instant::from_micros(5)), Duration::zero());
    EXPECT_EQ(Duration::from_secs(u64_max), Duration::max_value());
    EXPECT_EQ(Duration::from_millis(u64_max), Duration::max_value());
    EXPECT_EQ(Duration::from_secs(2).as_micros(), 2'000'000U);
    EXPECT_EQ(Duration::max_value().saturating_add(Duration::from_micros(1)),
              Duration::max_value());
    EXPECT_EQ(Duration::from_micros(1).saturating_add(Duration::from_micros(2)).as_micros(), 3U);
    EXPECT_EQ(Instant{}, Instant::from_micros(0));
    EXPECT_EQ(Duration{}, Duration::zero());
}

Continuity model(std::uint64_t d, std::uint64_t half) {
    if (d == 0) {
        return Continuity::repeat();
    }
    if (d == 1) {
        return Continuity::in_sequence();
    }
    if (d <= half) {
        return Continuity::gap(d - 1);
    }
    return Continuity::backwards();
}

TEST(Counter, Exhaustive8Bit) {
    // req: CMN-030, CMN-031
    for (std::uint64_t prev = 0; prev <= 255; ++prev) {
        const ModCounter c = unwrap(ModCounter::make(8, prev));
        for (std::uint64_t next = 0; next <= 255; ++next) {
            const std::uint64_t d = (next + 256 - prev) % 256;
            EXPECT_EQ(unwrap(c.distance_to(next)), d);
            EXPECT_EQ(unwrap(c.classify(next)), model(d, 128)) << prev << " " << next;
        }
    }
}

TEST(Counter, WidthsWrapAndValidate) {
    // req: CMN-030, GEN-024
    ModCounter c = unwrap(ModCounter::make(64, u64_max));
    c.increment();
    EXPECT_EQ(c.bits(), 64U);
    EXPECT_EQ(c.value(), 0U);
    EXPECT_EQ(unwrap(ModCounter::make(1, 1)).next().value(), 0U);
    EXPECT_EQ(unwrap(ModCounter::make(14, 16383)).next().value(), 0U);
    EXPECT_EQ(unwrap(unwrap(ModCounter::make(64, 0)).classify(1ULL << 63U)),
              Continuity::gap((1ULL << 63U) - 1));
    EXPECT_EQ(unwrap(unwrap(ModCounter::make(64, 0)).classify((1ULL << 63U) + 1)),
              Continuity::backwards());
    struct Case {
        std::uint8_t bits;
        std::uint64_t value;
        std::uint64_t expected;
        std::uint64_t found;
    };
    for (const Case k : {
             Case{.bits = 0, .value = 0, .expected = 1, .found = 0},
             Case{.bits = 65, .value = 0, .expected = 64, .found = 65},
             Case{.bits = 8, .value = 256, .expected = 255, .found = 256},
         }) {
        const auto e = unwrap_err(ModCounter::make(k.bits, k.value));
        EXPECT_EQ(e.kind(), ErrorKind::OutOfRange);
        EXPECT_EQ(e.detail(), ErrorDetail::expected(k.expected, k.found));
    }
    EXPECT_EQ(unwrap_err(unwrap(ModCounter::make(8, 1)).distance_to(300)).kind(),
              ErrorKind::OutOfRange);
    EXPECT_EQ(unwrap_err(unwrap(ModCounter::make(8, 1)).classify(300)).kind(),
              ErrorKind::OutOfRange);
    const Continuity gap = Continuity::gap(4);
    EXPECT_EQ(gap.kind(), Continuity::Kind::Gap);
    EXPECT_EQ(gap.missing(), std::uint64_t{4});
    EXPECT_EQ(Continuity::repeat().missing(), std::nullopt);
    EXPECT_EQ(Continuity::backwards().kind(), Continuity::Kind::Backwards);
}

TEST(Counter, WiderCountersMatchModel) {
    // req: CMN-030, CMN-031, GEN-024, VER-023
    Rng rng{0xC0'0A7E};
    for (int i = 0; i < 20'000; ++i) {
        const auto bits = static_cast<std::uint8_t>(rng.range(1, 64));
        const std::uint64_t mask = u64_max >> (64U - bits);
        const std::uint64_t prev = rng.next() & mask;
        const std::uint64_t next = rng.next() & mask;
        const ModCounter c = unwrap(ModCounter::make(bits, prev));
        // (prev + 1) mod 2^bits and (next - prev) mod 2^bits, without overflow.
        EXPECT_EQ(c.next().value(), prev == mask ? 0 : prev + 1);
        const std::uint64_t dist = next >= prev ? next - prev : mask - (prev - next) + 1;
        EXPECT_EQ(unwrap(c.distance_to(next)), dist);
        const std::uint64_t half = 1ULL << (bits - 1U);
        EXPECT_EQ(unwrap(c.classify(next)), model(dist, half));
    }
}

} // namespace
