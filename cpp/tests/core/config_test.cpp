// Tests of aldis/core/config.hpp: language standard, version, and portability macros.

#include "aldis/core/config.hpp"

#include <gtest/gtest.h>

namespace {

TEST(Config, BuildsAsCpp20) {
    // req: CPP-001
    static_assert(__cplusplus >= 202002L);
    EXPECT_GE(__cplusplus, 202002L);
}

TEST(Config, VersionMatchesTheRustBaseline) {
    // req: CPP-001
    // The port follows aldis-core 0.1.0 (docs/08-roadmap.md §5).
    EXPECT_EQ(ALDIS_VERSION_MAJOR, 0);
    EXPECT_EQ(ALDIS_VERSION_MINOR, 1);
    EXPECT_EQ(ALDIS_VERSION_PATCH, 0);
}

const int& borrowed(const int& value ALDIS_LIFETIMEBOUND) { return value; }

TEST(Config, LifetimeboundExpandsOnEveryCompiler) {
    // req: CPP-001
    const int value = 7;
    EXPECT_EQ(borrowed(value), 7);
}

} // namespace
