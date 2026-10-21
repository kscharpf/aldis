// Constant evaluation of the core vectors (CPP-034; docs/milestones/c0-foundations.md D5, WP-4).
//
// For every operation of test-vectors/core/, the first success case and the first error case of
// the generated arrays run through their check in a `static_assert`, so the compiler evaluates
// aldis_core on those inputs in constant evaluation, where undefined behavior is a compile error.
// Built as an object library under the library rules on every preset, bare metal included, so each
// pinned compiler evaluates them. The host runner checks every case at run time (CPP-053).

#include "aldis/vectors/case.hpp"
#include "aldis/vectors/generated/core.hpp"

#include <array>
#include <cstddef>

namespace {

using aldis::vectors::Named;

/// The first case of `cases` that expects success (`ok`) or an error (`!ok`), or nullptr.
template <typename Case, std::size_t N>
constexpr const Named<Case>* first(const std::array<Named<Case>, N>& cases, bool ok) {
    for (const Named<Case>& c : cases) {
        if ((c.data.expect.value() != nullptr) == ok) {
            return &c;
        }
    }
    return nullptr;
}

/// Whether the first success case and the first error case of `cases` pass; an absent one passes.
template <typename Case, std::size_t N>
constexpr bool subset_passes(const std::array<Named<Case>, N>& cases) {
    const Named<Case>* success = first(cases, true);
    const Named<Case>* failure = first(cases, false);
    return (success == nullptr || check(success->data).passed()) &&
           (failure == nullptr || check(failure->data).passed());
}

// req: CPP-034, CPP-053
static_assert(subset_passes(aldis::vectors::generated::bits_get));
static_assert(subset_passes(aldis::vectors::generated::bits_set));
static_assert(subset_passes(aldis::vectors::generated::counter_classify));
static_assert(subset_passes(aldis::vectors::generated::counter_increment));
static_assert(subset_passes(aldis::vectors::generated::crc16));
static_assert(subset_passes(aldis::vectors::generated::error_code));
static_assert(subset_passes(aldis::vectors::generated::frame_layout));
static_assert(subset_passes(aldis::vectors::generated::oid_fill));
static_assert(subset_passes(aldis::vectors::generated::oid_fill_sequence));

// The subset is not empty: every operation has a success case, and each operation with error
// vectors has an error case (counter_classify, crc16, error_code, and the OID fills have none).
static_assert(first(aldis::vectors::generated::bits_get, true) != nullptr &&
              first(aldis::vectors::generated::bits_get, false) != nullptr);
static_assert(first(aldis::vectors::generated::bits_set, true) != nullptr &&
              first(aldis::vectors::generated::bits_set, false) != nullptr);
static_assert(first(aldis::vectors::generated::counter_classify, true) != nullptr);
static_assert(first(aldis::vectors::generated::counter_increment, true) != nullptr &&
              first(aldis::vectors::generated::counter_increment, false) != nullptr);
static_assert(first(aldis::vectors::generated::crc16, true) != nullptr);
static_assert(first(aldis::vectors::generated::error_code, true) != nullptr);
static_assert(first(aldis::vectors::generated::frame_layout, true) != nullptr &&
              first(aldis::vectors::generated::frame_layout, false) != nullptr);
static_assert(first(aldis::vectors::generated::oid_fill, true) != nullptr);
static_assert(first(aldis::vectors::generated::oid_fill_sequence, true) != nullptr);

} // namespace
