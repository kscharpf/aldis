// Aldis C++ configuration: language-standard check, version, and portability macros.
//
// Every Aldis build is exactly C++20, and these headers reject an older standard at compile
// time (CPP-001, OI-28). A consumer may compile them under a later standard; that use is not
// verified. Included first by every other Aldis header.

#ifndef ALDIS_CORE_CONFIG_HPP
#define ALDIS_CORE_CONFIG_HPP

// The check comes first, so that an older standard fails here rather than in later code.
static_assert(__cplusplus >= 202002L, "Aldis requires C++20 or later (CPP-001)");

// Version of the native C++ library. A native release carries the version of the Rust release
// it was verified against (docs/08-roadmap.md §5); the port follows aldis-core 0.1.0.
// NOLINTBEGIN(cppcoreguidelines-macro-usage): consumers test the version in #if directives.
#define ALDIS_VERSION_MAJOR 0
#define ALDIS_VERSION_MINOR 1
#define ALDIS_VERSION_PATCH 0
// NOLINTEND(cppcoreguidelines-macro-usage)

// Marks a parameter whose referent a returned view borrows, so that Clang diagnoses a view
// bound to a temporary (CPP-022). GCC warns on unknown scoped attributes, so it expands to
// nothing there (docs/profiles/cpp.md §3).
#if defined(__clang__)
#define ALDIS_LIFETIMEBOUND [[clang::lifetimebound]]
#else
#define ALDIS_LIFETIMEBOUND
#endif

#endif // ALDIS_CORE_CONFIG_HPP
