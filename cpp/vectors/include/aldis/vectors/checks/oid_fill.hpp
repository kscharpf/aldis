// Check of the `oid_fill` operation of test-vectors/core/oid.json (CMN-020, CMN-022, CMN-023;
// CPP-053, TV-012). Counterpart of the `oid_fill` arm of crates/aldis-core/tests/vectors.rs, which
// also requires the Fibonacci and Galois forms to agree.

#ifndef ALDIS_VECTORS_CHECKS_OID_FILL_HPP
#define ALDIS_VECTORS_CHECKS_OID_FILL_HPP

#include "aldis/core/error.hpp"
#include "aldis/core/oid.hpp"
#include "aldis/vectors/case.hpp"

#include <cstddef>
#include <span>

namespace aldis::vectors {

/// `oid_fill`: the first `octets` octets of a generator in its start-up state (`output_hex`).
struct OidFillCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("oid_fill");

    /// `params.octets`.
    std::size_t octets{};
    /// `expect.output_hex`.
    Expect<Octets> expect;
};

namespace detail {

/// `len` octets from `gen`; `CapacityExceeded` if `len > max_octets`.
template <typename Generator>
constexpr core::Result<Octets> fill(Generator& gen, std::size_t len) noexcept {
    core::Result<Octets> out = Octets::zeroed(len);
    if (out.error() != nullptr) {
        return out;
    }
    Octets octets = *out.ok();
    gen.fill(octets.view());
    return octets;
}

} // namespace detail

/// The octets of the Fibonacci form, the reference (CMN-020).
[[nodiscard]] constexpr core::Result<Octets> run(const OidFillCase& c) noexcept {
    core::OidGenerator gen{};
    return detail::fill(gen, c.octets);
}

/// The verdict on `c` (TV-012); `FormsDisagree` if the Galois form differs (CMN-022).
[[nodiscard]] constexpr Verdict check(const OidFillCase& c) noexcept {
    const core::Result<Octets> fibonacci = run(c);
    core::OidGeneratorGalois gen{};
    const core::Result<Octets> galois = detail::fill(gen, c.octets);
    // The forms disagree if only one of them succeeds, or if both succeed with different octets.
    if ((fibonacci.ok() == nullptr) != (galois.ok() == nullptr) ||
        (fibonacci.ok() != nullptr && !(*fibonacci.ok() == *galois.ok()))) {
        return Verdict::fail(Mismatch::FormsDisagree);
    }
    return compare(c.expect, fibonacci);
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_OID_FILL_HPP
