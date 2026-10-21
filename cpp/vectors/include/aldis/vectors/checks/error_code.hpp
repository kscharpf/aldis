// Check of the `error_code` operation of test-vectors/core/errors.json (GEN-040, CABI-020;
// CPP-021, CPP-053, TV-012). Counterpart of the `error_code` arm of
// crates/aldis-core/tests/vectors.rs: the C++ `ErrorKind` carries the catalogue codes (c0 plan D8).

#ifndef ALDIS_VECTORS_CHECKS_ERROR_CODE_HPP
#define ALDIS_VECTORS_CHECKS_ERROR_CODE_HPP

#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"

#include <cstdint>
#include <optional>
#include <span>

namespace aldis::vectors {

/// The stable code of an error kind, and the value the C ABI returns for it (its negation,
/// CABI-020).
struct ErrorCode {
    /// `expect.code`.
    std::uint16_t code{};
    /// `expect.c_abi`.
    std::int32_t c_abi{};

    friend constexpr bool operator==(ErrorCode, ErrorCode) noexcept = default;
};

/// `error_code`: the code of the error kind named `kind`.
struct ErrorCodeCase {
    /// The operation name in the vector files.
    static constexpr std::span<const char> operation = literal("error_code");

    /// `params.kind`: a catalogue name.
    Name kind;
    /// `expect.code` and `expect.c_abi`.
    Expect<ErrorCode> expect;
};

/// The code of the kind named `c.kind`, or nothing if the name is not in the catalogue.
[[nodiscard]] constexpr std::optional<ErrorCode> run(const ErrorCodeCase& c) noexcept {
    const std::optional<core::ErrorKind> kind = core::error::from_name(c.kind.view());
    if (!kind.has_value()) {
        return std::nullopt;
    }
    const std::uint16_t code = core::error::code(*kind);
    return ErrorCode{.code = code, .c_abi = -static_cast<std::int32_t>(code)};
}

/// The verdict on `c` (TV-012); `UnknownName` if the name is not in the catalogue.
[[nodiscard]] constexpr Verdict check(const ErrorCodeCase& c) noexcept {
    const std::optional<ErrorCode> actual = run(c);
    if (!actual.has_value()) {
        return Verdict::fail(Mismatch::UnknownName);
    }
    return compare(c.expect, core::Result<ErrorCode>{*actual});
}

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CHECKS_ERROR_CODE_HPP
