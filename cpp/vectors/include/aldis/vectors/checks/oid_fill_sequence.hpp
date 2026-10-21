// Check of the `oid_fill_sequence` operation of test-vectors/core/oid.json (CMN-021, CMN-022,
// CMN-023; CPP-053, TV-012). Counterpart of the `oid_fill_sequence` arm of
// crates/aldis-core/tests/vectors.rs, which also requires the Fibonacci and Galois forms to agree.

#ifndef ALDIS_VECTORS_CHECKS_OID_FILL_SEQUENCE_HPP
#define ALDIS_VECTORS_CHECKS_OID_FILL_SEQUENCE_HPP

#include "aldis/core/bytes.hpp"
#include "aldis/core/error.hpp"
#include "aldis/core/oid.hpp"
#include "aldis/vectors/case.hpp"
#include "aldis/vectors/checks/oid_fill.hpp"

#include <cstddef>
#include <span>

namespace aldis::vectors {

/// `oid_fill_sequence`: consecutive fills of one generator, which continue the sequence
/// (`outputs_hex`, CMN-021).
struct OidFillSequenceCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("oid_fill_sequence");

    /// `params.fills`: the length of each fill.
    Fills fills;
    /// `expect.outputs_hex`.
    Expect<OctetsList> expect;
};

namespace detail {

/// The fills of `c` from one generator of type `Generator`.
template <typename Generator>
constexpr core::Result<OctetsList> fill_sequence(const OidFillSequenceCase& c) noexcept {
    core::Result<OctetsList> list = OctetsList::zeroed(c.fills.size());
    if (list.error() != nullptr) {
        return list;
    }
    OctetsList outputs = *list.ok();
    Generator gen{};
    std::size_t index = 0;
    for (Octets& output : outputs.view()) {
        const core::Result<Octets> octets =
            fill(gen, core::bytes::get_or(c.fills.view(), index, std::size_t{0}));
        if (octets.error() != nullptr) {
            return *octets.error();
        }
        output = *octets.ok();
        ++index;
    }
    return outputs;
}

} // namespace detail

/// The fills of the Fibonacci form, the reference (CMN-020, CMN-021).
[[nodiscard]] constexpr core::Result<OctetsList> run(const OidFillSequenceCase& c) noexcept {
    return detail::fill_sequence<core::OidGenerator>(c);
}

/// The verdict on `c` (TV-012); `FormsDisagree` if the Galois form differs (CMN-022).
[[nodiscard]] constexpr Verdict check(const OidFillSequenceCase& c) noexcept {
    const core::Result<OctetsList> fibonacci = run(c);
    const core::Result<OctetsList> galois = detail::fill_sequence<core::OidGeneratorGalois>(c);
    if (fibonacci.ok() != nullptr && galois.ok() != nullptr && !(*fibonacci.ok() == *galois.ok())) {
        return Verdict::fail(Mismatch::FormsDisagree);
    }
    return compare(c.expect, fibonacci);
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_OID_FILL_SEQUENCE_HPP
