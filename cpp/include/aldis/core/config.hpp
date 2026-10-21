// Aldis C++ configuration: language-standard check, version, and portability macros.
//
// Every Aldis build is exactly C++20, and these headers reject an older standard at compile
// time (CPP-001, OI-28). A consumer may compile them under a later standard; that use is not
// verified. Included first by every other Aldis header.

#ifndef ALDIS_CORE_CONFIG_HPP
#define ALDIS_CORE_CONFIG_HPP

// The check comes first, so that an older standard fails here rather than in later code.
// 202002L is the value of __cplusplus that C++20 defines ([cpp.predefined]).
// NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
static_assert(__cplusplus >= 202002L, "Aldis requires C++20 or later (CPP-001)");

// Version of the native C++ library. A native release carries the version of the Rust release
// it was verified against (docs/08-roadmap.md §5). The port follows aldis-core 0.2.0 (commit
// 0655f2a, release v0.2.0-m1); the version stays 0.1.0 because differential verification against
// a Rust release starts in C1 (VER-053), and only then does a native release carry its version.
// Consumers test the version in #if directives, so these stay macros.
// NOLINTBEGIN(cppcoreguidelines-macro-usage,cppcoreguidelines-macro-to-enum,modernize-macro-to-enum)
#define ALDIS_VERSION_MAJOR 0
#define ALDIS_VERSION_MINOR 1
#define ALDIS_VERSION_PATCH 0
// NOLINTEND(cppcoreguidelines-macro-usage,cppcoreguidelines-macro-to-enum,modernize-macro-to-enum)

// Marks a parameter whose referent a returned view borrows, so that Clang diagnoses a view
// bound to a temporary (CPP-022). GCC warns on unknown scoped attributes, so it expands to
// nothing there (docs/profiles/cpp.md §3).
#ifdef __clang__
#define ALDIS_LIFETIMEBOUND [[clang::lifetimebound]]
#else
#define ALDIS_LIFETIMEBOUND
#endif

#endif // ALDIS_CORE_CONFIG_HPP
