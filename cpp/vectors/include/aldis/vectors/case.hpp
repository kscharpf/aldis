// Case model of the shared conformance vectors for the native C++ runners (CPP-053, TV-001,
// TV-012; docs/milestones/c0-foundations.md D5, WP-4).
//
// Every type here is a literal type without heap storage: fixed-capacity octet arrays and
// trivially copyable values. The same case structs are filled at run time by the host loader
// (aldis/vectors/loader.hpp, nlohmann/json) and at build time by `aldis_vecgen`, which writes them
// as `constexpr` arrays for the freestanding exerciser (WP-5) and the `static_assert`s of CPP-034.
// So this header and the check functions obey the library rules (CPP-002, CPP-030, CPP-031): only
// allow-listed standard headers, no exceptions, and element access only through aldis/core/bytes.

#ifndef ALDIS_VECTORS_CASE_HPP
#define ALDIS_VECTORS_CASE_HPP

#include "aldis/core/bytes.hpp"
#include "aldis/core/config.hpp"
#include "aldis/core/error.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>

namespace aldis::vectors {

/// Capacity of one octet string of a case (input or expected output). The loader rejects a
/// longer one, so a case never runs truncated.
inline constexpr std::size_t max_octets = 64;

/// Capacity of the list of fills of an `oid_fill_sequence` case.
inline constexpr std::size_t max_fills = 8;

/// Capacity of an error-kind name of an `error_code` case, in characters.
inline constexpr std::size_t max_name = 64;

/// A view of the string literal `text`, without its terminating NUL (case identifiers in
/// generated code).
template <std::size_t N>
    requires(N >= 1)
// A string literal is a C array; the reference keeps its length.
// NOLINTNEXTLINE(cppcoreguidelines-avoid-c-arrays,hicpp-avoid-c-arrays,modernize-avoid-c-arrays)
consteval std::span<const char> literal(const char (&text)[N]) noexcept {
    return std::span<const char, N>{text}.template first<N - 1>();
}

/// Up to `Cap` elements of a trivially copyable `T`, stored inline (no heap).
template <typename T, std::size_t Cap> class FixedArray {
public:
    /// The empty array.
    constexpr FixedArray() noexcept = default;

    /// A copy of `items`, or `CapacityExceeded` if there are more than `Cap`.
    [[nodiscard]] static constexpr core::Result<FixedArray>
    make(std::span<const T> items) noexcept {
        FixedArray a;
        if (core::bytes::copy(std::span<T>{a.items_}, items).error() != nullptr) {
            return core::Error{core::ErrorKind::CapacityExceeded};
        }
        a.len_ = items.size();
        return a;
    }

    /// `len` value-initialized elements, or `CapacityExceeded` if `len > Cap`.
    [[nodiscard]] static constexpr core::Result<FixedArray> zeroed(std::size_t len) noexcept {
        if (len > Cap) {
            return core::Error{core::ErrorKind::CapacityExceeded};
        }
        FixedArray a;
        a.len_ = len;
        return a;
    }

    /// The elements.
    [[nodiscard]] constexpr std::span<const T> view() const noexcept {
        return core::bytes::subspan_or_empty(std::span<const T>{items_}, 0, len_);
    }

    /// The elements, writable.
    [[nodiscard]] constexpr std::span<T> view() noexcept {
        return core::bytes::subspan_or_empty(std::span<T>{items_}, 0, len_);
    }

    /// Number of elements.
    [[nodiscard]] constexpr std::size_t size() const noexcept { return len_; }

    /// Equal when the elements are equal; unused capacity is not compared.
    friend constexpr bool operator==(const FixedArray& a, const FixedArray& b) noexcept {
        return core::bytes::equal(a.view(), b.view());
    }

private:
    std::array<T, Cap> items_{};
    std::size_t len_{0};
};

/// An octet string of a case: input, or an expected or actual output.
using Octets = FixedArray<std::byte, max_octets>;

/// The octet string of `values` (generated code).
template <std::size_t N>
    requires(N <= max_octets)
[[nodiscard]] constexpr Octets octets(const std::array<std::uint8_t, N>& values) noexcept {
    std::array<std::byte, N> data{};
    std::size_t index = 0;
    for (const std::uint8_t v : values) {
        // index < N: one element per value.
        if (!core::bytes::put(std::span<std::byte>{data}, index, std::byte{v})) {
            return Octets{};
        }
        ++index;
    }
    // N <= max_octets, so this cannot fail.
    return Octets::make(data).value_or(Octets{});
}

/// The outputs of several fills, in order (`oid_fill_sequence`).
using OctetsList = FixedArray<Octets, max_fills>;

/// Lengths of the fills of an `oid_fill_sequence` case.
using Fills = FixedArray<std::size_t, max_fills>;

/// The list of `items` (generated code).
template <std::size_t N>
    requires(N <= max_fills)
[[nodiscard]] constexpr OctetsList octets_list(const std::array<Octets, N>& items) noexcept {
    // N <= max_fills, so this cannot fail.
    return OctetsList::make(items).value_or(OctetsList{});
}

/// The fill lengths `lengths` (generated code).
template <std::size_t N>
    requires(N <= max_fills)
[[nodiscard]] constexpr Fills fills(const std::array<std::size_t, N>& lengths) noexcept {
    // N <= max_fills, so this cannot fail.
    return Fills::make(lengths).value_or(Fills{});
}

/// An error-kind name of an `error_code` case.
using Name = FixedArray<char, max_name>;

/// The name `text` (generated code).
template <std::size_t N>
    requires(N >= 1 && N <= max_name + 1)
// A string literal is a C array; the reference keeps its length.
// NOLINTNEXTLINE(cppcoreguidelines-avoid-c-arrays,hicpp-avoid-c-arrays,modernize-avoid-c-arrays)
[[nodiscard]] constexpr Name name(const char (&text)[N]) noexcept {
    return Name::make(std::span<const char, N>{text}.template first<N - 1>()).value_or(Name{});
}

/// The error a case expects: its kind, and the code of the field it names when the vector gives
/// one (`expect.field`, M2 plan D3; 1…65535).
///
/// C0 keeps the code but does not compare it: `aldis::core::FieldId` gains its numeric code in C1
/// (c0 plan D8, M2 plan §6). Until then the runners compare the kind only, as the Rust runner does
/// for cases without `field`.
class ExpectedError {
public:
    /// An error of `kind`, naming the field with code `field` if given.
    constexpr explicit ExpectedError(core::ErrorKind kind,
                                     std::optional<std::uint16_t> field = std::nullopt) noexcept
        : kind_{kind}, field_{field} {}

    /// The error kind.
    [[nodiscard]] constexpr core::ErrorKind kind() const noexcept { return kind_; }

    /// The field code, if the vector gives one (kept for C1; not compared in C0).
    [[nodiscard]] constexpr std::optional<std::uint16_t> field() const noexcept { return field_; }

    friend constexpr bool operator==(ExpectedError, ExpectedError) noexcept = default;

private:
    core::ErrorKind kind_;
    std::optional<std::uint16_t> field_;
};

/// The expected outcome of a case: an output value `Out`, or an error (TV-001). Exactly one of
/// the two is present, by construction.
template <typename Out> class Expect {
public:
    /// Success with `value`.
    [[nodiscard]] static constexpr Expect success(Out value) noexcept {
        Expect e;
        e.value_ = value;
        return e;
    }

    /// Failure with `error`.
    [[nodiscard]] static constexpr Expect failure(ExpectedError error) noexcept {
        Expect e;
        e.error_ = error;
        return e;
    }

    /// The expected value, or `nullptr` if an error is expected.
    [[nodiscard]] constexpr const Out* value() const noexcept {
        return value_.has_value() ? &*value_ : nullptr;
    }

    /// The expected error, or `nullptr` if success is expected.
    [[nodiscard]] constexpr const ExpectedError* error() const noexcept {
        return error_.has_value() ? &*error_ : nullptr;
    }

private:
    constexpr Expect() noexcept = default;

    std::optional<Out> value_;
    std::optional<ExpectedError> error_;
};

/// How an outcome differs from its expectation (TV-012: any difference fails).
enum class Mismatch : std::uint8_t {
    /// No difference: the case passes.
    None,
    /// Success was expected, and the operation failed.
    ExpectedSuccess,
    /// An error was expected, and the operation succeeded.
    ExpectedError,
    /// Another error kind was expected.
    WrongError,
    /// The output differs from the expected output.
    WrongValue,
    /// Two forms of the same function disagree (bitwise and table CRC, Fibonacci and Galois OID).
    FormsDisagree,
    /// `error_code`: the name is not in the catalogue.
    UnknownName,
};

/// The verdict on one case.
class Verdict {
public:
    /// The case passes.
    [[nodiscard]] static constexpr Verdict pass() noexcept { return Verdict{}; }

    /// The case fails with `mismatch`, and the error the operation returned, if it returned one.
    [[nodiscard]] static constexpr Verdict fail(Mismatch mismatch,
                                                std::optional<core::ErrorKind> got = {}) noexcept {
        Verdict v;
        v.mismatch_ = mismatch;
        v.got_ = got;
        return v;
    }

    /// Whether the case passes.
    [[nodiscard]] constexpr bool passed() const noexcept { return mismatch_ == Mismatch::None; }

    /// How the outcome differs.
    [[nodiscard]] constexpr Mismatch mismatch() const noexcept { return mismatch_; }

    /// The error kind the operation returned, if it failed.
    [[nodiscard]] constexpr std::optional<core::ErrorKind> got() const noexcept { return got_; }

    friend constexpr bool operator==(Verdict, Verdict) noexcept = default;

private:
    constexpr Verdict() noexcept = default;

    Mismatch mismatch_{Mismatch::None};
    std::optional<core::ErrorKind> got_;
};

/// Compares the outcome `actual` with `expect` exactly (TV-012): the same value, or the same
/// error kind. The field code of an expected error is not compared in C0 (see `ExpectedError`).
template <typename Out>
[[nodiscard]] constexpr Verdict compare(const Expect<Out>& expect,
                                        const core::Result<Out>& actual) noexcept {
    const Out* got = actual.ok();
    const core::Error* got_error = actual.error();
    const std::optional<core::ErrorKind> got_kind =
        got_error != nullptr ? std::optional<core::ErrorKind>{got_error->kind()} : std::nullopt;
    if (const Out* want = expect.value(); want != nullptr) {
        if (got == nullptr) {
            return Verdict::fail(Mismatch::ExpectedSuccess, got_kind);
        }
        return *got == *want ? Verdict::pass() : Verdict::fail(Mismatch::WrongValue);
    }
    const ExpectedError* want_error = expect.error();
    if (want_error == nullptr || !got_kind.has_value()) {
        return Verdict::fail(Mismatch::ExpectedError);
    }
    return *got_kind == want_error->kind() ? Verdict::pass()
                                           : Verdict::fail(Mismatch::WrongError, got_kind);
}

/// A case of generated code: its identifier and data. The host loader keeps the identifier in a
/// `std::string` instead (aldis/vectors/loader.hpp).
template <typename Case> struct Named {
    /// The case identifier (`id`).
    std::span<const char> id{};
    /// The case.
    Case data;
};

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CASE_HPP
