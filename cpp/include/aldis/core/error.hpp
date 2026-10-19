// Error kinds and details shared by all modules: aldis-core `error.rs`.
//
// Implements the stable error catalogue of docs/03-api-model.md §9 (GEN-040), errors with field
// and value detail without allocation (GEN-041), and `Result<T>`, whose accessors cannot be
// misused into undefined behavior (CPP-021, docs/milestones/c0-foundations.md D8).

#ifndef ALDIS_CORE_ERROR_HPP
#define ALDIS_CORE_ERROR_HPP

#include "aldis/core/config.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <type_traits>

namespace aldis::core {

/// Error kind with a stable numeric code shared by all profiles (03-api-model.md §9; GEN-040,
/// CPP-021). The C ABI returns the negated code (CABI-020). Codes are never renumbered, and
/// consumers handle values they do not know (GEN-081).
// The codes are 16-bit in every profile, so the type is fixed (GEN-040, CABI-020).
// NOLINTNEXTLINE(performance-enum-size)
enum class ErrorKind : std::uint16_t {
    /// Output buffer too small (detail: `needed`).
    BufferTooSmall = 1,
    /// Input shorter than required (detail: `needed`).
    Incomplete = 2,
    /// Length field, configured length, and buffer length disagree.
    LengthMismatch = 3,
    /// Version number field is not the one this codec handles.
    UnsupportedVersion = 4,
    /// A value exceeds its field range or a configured limit.
    OutOfRange = 5,
    /// A field holds a value the Blue Book forbids.
    InvalidField = 6,
    /// A field holds a value CCSDS reserves (strict mode).
    ReservedValue = 7,
    /// FECF check failed.
    CrcMismatch = 8,
    /// AOS Frame Header Error Control detected an uncorrectable error.
    HeaderUncorrectable = 9,
    /// SCID, VCID, MAP ID, or APID not in the configuration.
    UnknownChannel = 10,
    /// Configuration violates a Blue Book constraint.
    InvalidConfig = 11,
    /// Engine queue full; input rejected without state change.
    WouldBlock = 12,
    /// Operation not permitted in the engine's current state.
    InvalidState = 13,
    /// Security hook reported failure.
    Security = 14,
    /// Configuration exceeds a compile-time capacity.
    CapacityExceeded = 15,
    /// Unknown or stale transmit or request handle.
    InvalidHandle = 16,
    /// Structurally impossible input.
    Malformed = 17,
};

namespace detail {

/// Longest name of a field or error kind, in characters; bounds every name comparison.
inline constexpr std::size_t max_name_len = 32;

/// A view of the string literal `name`, without its terminating NUL.
template <std::size_t N>
    requires(N >= 1 && N <= max_name_len + 1)
// A string literal is a C array; the reference keeps its length.
// NOLINTNEXTLINE(cppcoreguidelines-avoid-c-arrays,hicpp-avoid-c-arrays,modernize-avoid-c-arrays)
constexpr std::span<const char> literal(const char (&name)[N]) noexcept {
    return std::span<const char, N>{name}.template first<N - 1>();
}

/// Whether two names are equal, compared by content and bounded by `max_name_len`. The sizes
/// are compared first, so the iterator over `b` moves in step with the range over `a` and never
/// passes its end.
constexpr bool names_equal(std::span<const char> a, std::span<const char> b) noexcept {
    if (a.size() != b.size() || a.size() > max_name_len) {
        return false;
    }
    auto other = b.begin();
    for (const char c : a) {
        if (c != *other) {
            return false;
        }
        ++other;
    }
    return true;
}

} // namespace detail

/// Identifies the field an error refers to, e.g. `FieldId{"apid"}` (GEN-041, CPP-021).
///
/// Holds a view of a string literal, as aldis-core holds a `&'static str`, and compares by
/// content. The numeric field code of API-082 joins when the catalogue is created (OI-27).
class FieldId {
public:
    /// A field identifier naming the string literal `name` (at most 32 characters).
    template <std::size_t N>
        requires(N >= 1 && N <= detail::max_name_len + 1)
    // Implicit from a string literal (a C array), and only at compile time (consteval).
    // NOLINTNEXTLINE(misc-explicit-constructor,cppcoreguidelines-explicit-constructor,hicpp-explicit-conversions,cppcoreguidelines-avoid-c-arrays,hicpp-avoid-c-arrays,modernize-avoid-c-arrays)
    consteval FieldId(const char (&name)[N]) noexcept : name_{detail::literal(name)} {}

    /// The field name, without a terminating NUL.
    [[nodiscard]] constexpr std::span<const char> name() const noexcept { return name_; }

    /// Equal when the names are equal.
    friend constexpr bool operator==(FieldId a, FieldId b) noexcept {
        return detail::names_equal(a.name_, b.name_);
    }

private:
    std::span<const char> name_;
};

/// The value found, and the value or limit expected (GEN-041).
struct Expected {
    /// What the standard or configuration requires.
    std::uint64_t expected;
    /// What was found.
    std::uint64_t found;

    friend constexpr bool operator==(Expected, Expected) noexcept = default;
};

/// Additional detail carried by an error, without allocation (GEN-041, CPP-021).
///
/// One of: none, octets `needed`, or a value `found` against what was `expected`.
class ErrorDetail {
public:
    /// Which detail is present.
    enum class Kind : std::uint8_t {
        /// No further detail.
        None,
        /// Octets (or bits, where documented) required.
        Needed,
        /// The value found, and the value or limit expected.
        Expected,
    };

    /// No further detail.
    constexpr ErrorDetail() noexcept = default;

    /// No further detail.
    [[nodiscard]] static constexpr ErrorDetail none() noexcept { return ErrorDetail{}; }

    /// `needed` octets (or bits, where documented) required.
    [[nodiscard]] static constexpr ErrorDetail needed(std::size_t needed) noexcept {
        ErrorDetail d;
        d.kind_ = Kind::Needed;
        d.needed_ = needed;
        return d;
    }

    /// `found` where `expected` is required.
    [[nodiscard]] static constexpr ErrorDetail expected(std::uint64_t expected,
                                                        std::uint64_t found) noexcept {
        ErrorDetail d;
        d.kind_ = Kind::Expected;
        d.expected_ = Expected{.expected = expected, .found = found};
        return d;
    }

    /// Which detail is present.
    [[nodiscard]] constexpr Kind kind() const noexcept { return kind_; }

    /// The octets needed, if this is a `Needed` detail.
    [[nodiscard]] constexpr std::optional<std::size_t> needed() const noexcept {
        if (kind_ == Kind::Needed) {
            return needed_;
        }
        return std::nullopt;
    }

    /// Expected and found values, if this is an `Expected` detail.
    [[nodiscard]] constexpr std::optional<Expected> expected() const noexcept {
        if (kind_ == Kind::Expected) {
            return expected_;
        }
        return std::nullopt;
    }

    friend constexpr bool operator==(ErrorDetail, ErrorDetail) noexcept = default;

private:
    Kind kind_{Kind::None};
    std::size_t needed_{0};
    Expected expected_{.expected = 0, .found = 0};
};

/// An error: kind, optional field, and detail (RS-011, CPP-021). Trivially copyable.
class Error {
public:
    /// An error of `kind` with no field or detail.
    constexpr explicit Error(ErrorKind kind) noexcept : kind_{kind} {}

    /// `BufferTooSmall` with the number of octets needed (API-022, API-061).
    [[nodiscard]] static constexpr Error buffer_too_small(std::size_t needed) noexcept {
        return Error{ErrorKind::BufferTooSmall}.with_detail(ErrorDetail::needed(needed));
    }

    /// `Incomplete` with the number of octets needed (API-013).
    [[nodiscard]] static constexpr Error incomplete(std::size_t needed) noexcept {
        return Error{ErrorKind::Incomplete}.with_detail(ErrorDetail::needed(needed));
    }

    /// `OutOfRange` for `field`, with the limit and the value found.
    [[nodiscard]] static constexpr Error out_of_range(FieldId field, std::uint64_t limit,
                                                      std::uint64_t found) noexcept {
        return Error{ErrorKind::OutOfRange}.with_field(field).with_detail(
            ErrorDetail::expected(limit, found));
    }

    /// The same error, naming `field`.
    [[nodiscard]] constexpr Error with_field(FieldId field) const noexcept {
        Error e = *this;
        e.field_ = field;
        return e;
    }

    /// The same error, with `detail`.
    [[nodiscard]] constexpr Error with_detail(ErrorDetail detail) const noexcept {
        Error e = *this;
        e.detail_ = detail;
        return e;
    }

    /// The error kind.
    [[nodiscard]] constexpr ErrorKind kind() const noexcept { return kind_; }

    /// The field, if known.
    [[nodiscard]] constexpr std::optional<FieldId> field() const noexcept { return field_; }

    /// The detail.
    [[nodiscard]] constexpr ErrorDetail detail() const noexcept { return detail_; }

    friend constexpr bool operator==(const Error&, const Error&) noexcept = default;

private:
    ErrorKind kind_;
    std::optional<FieldId> field_;
    ErrorDetail detail_;
};

/// Trivially copyable and destructible, so that a `Result` holding it stays a literal, trivially
/// copyable type (D8).
template <typename T>
concept TrivialValue = std::is_trivially_copyable_v<T> && std::is_trivially_destructible_v<T>;

/// A type `Result<T>` holds as its value: trivial, and neither a reference nor `Error` (D8).
template <typename T>
concept ResultValue = TrivialValue<T> && !std::is_reference_v<T> && !std::is_same_v<T, Error>;

/// The value of a fallible operation, or its `Error` (CPP-021, GEN-040).
///
/// `ok()` and `error()` return a pointer that is null in the wrong state, so no accessor can
/// throw, abort, or have undefined behavior (D8). Converts implicitly from `T` and from
/// `Error`, so `return error;` propagates an error.
template <typename T> class [[nodiscard]] Result {
    // Checked here rather than as a constraint, so that a type may return Result<itself> from a
    // member declared inside its own (still incomplete) definition.
    static_assert(ResultValue<T>, "Result<T> needs a trivially copyable and destructible T (D8)");

public:
    /// A success holding `value`. Implicit by design (D8).
    // NOLINTNEXTLINE(misc-explicit-constructor,cppcoreguidelines-explicit-constructor,hicpp-explicit-conversions)
    constexpr Result(T value) noexcept : is_ok_{true}, storage_{.value = value} {}

    /// A failure holding `error`. Implicit by design (D8).
    // NOLINTNEXTLINE(misc-explicit-constructor,cppcoreguidelines-explicit-constructor,hicpp-explicit-conversions)
    constexpr Result(Error error) noexcept : is_ok_{false}, storage_{.error = error} {}

    /// The value, or `nullptr` if this is an error.
    [[nodiscard]] constexpr const T* ok() const noexcept {
        // The tag says which member is active.
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-union-access)
        return is_ok_ ? &storage_.value : nullptr;
    }

    /// The error, or `nullptr` if this is a success.
    [[nodiscard]] constexpr const Error* error() const noexcept {
        // The tag says which member is active.
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-union-access)
        return is_ok_ ? nullptr : &storage_.error;
    }

    /// Whether this holds a value.
    [[nodiscard]] constexpr bool is_ok() const noexcept { return is_ok_; }

    /// The value, or `fallback` if this is an error (Rust `unwrap_or`).
    [[nodiscard]] constexpr T value_or(T fallback) const noexcept {
        const T* value = ok();
        return value != nullptr ? *value : fallback;
    }

private:
    union Storage {
        T value;
        Error error;
    };

    bool is_ok_;
    Storage storage_;
};

/// The success or failure of an operation without a value (CPP-021).
template <> class [[nodiscard]] Result<void> {
public:
    /// Success.
    constexpr Result() noexcept = default;

    /// A failure holding `error`. Implicit by design (D8).
    // NOLINTNEXTLINE(misc-explicit-constructor,cppcoreguidelines-explicit-constructor,hicpp-explicit-conversions)
    constexpr Result(Error error) noexcept : error_{error} {}

    /// The error, or `nullptr` on success.
    [[nodiscard]] constexpr const Error* error() const noexcept {
        return error_.has_value() ? &*error_ : nullptr;
    }

    /// Whether the operation succeeded.
    [[nodiscard]] constexpr bool is_ok() const noexcept { return !error_.has_value(); }

private:
    std::optional<Error> error_;
};

namespace error {

/// Every kind, in code order (GEN-040).
inline constexpr std::array<ErrorKind, 17> all_kinds{
    ErrorKind::BufferTooSmall,     ErrorKind::Incomplete,    ErrorKind::LengthMismatch,
    ErrorKind::UnsupportedVersion, ErrorKind::OutOfRange,    ErrorKind::InvalidField,
    ErrorKind::ReservedValue,      ErrorKind::CrcMismatch,   ErrorKind::HeaderUncorrectable,
    ErrorKind::UnknownChannel,     ErrorKind::InvalidConfig, ErrorKind::WouldBlock,
    ErrorKind::InvalidState,       ErrorKind::Security,      ErrorKind::CapacityExceeded,
    ErrorKind::InvalidHandle,      ErrorKind::Malformed,
};

/// The stable numeric code of `kind` (GEN-040).
[[nodiscard]] constexpr std::uint16_t code(ErrorKind kind) noexcept {
    return static_cast<std::uint16_t>(kind);
}

/// The kind with numeric code `code`, if defined (GEN-040).
[[nodiscard]] constexpr std::optional<ErrorKind> from_code(std::uint16_t code) noexcept {
    for (const ErrorKind kind : all_kinds) {
        if (error::code(kind) == code) {
            return kind;
        }
    }
    return std::nullopt;
}

/// The catalogue name of `kind`, e.g. `CrcMismatch`; empty for a value outside the catalogue
/// (GEN-040, GEN-081).
[[nodiscard]] constexpr std::span<const char> name(ErrorKind kind) noexcept {
    switch (kind) {
    case ErrorKind::BufferTooSmall:
        return detail::literal("BufferTooSmall");
    case ErrorKind::Incomplete:
        return detail::literal("Incomplete");
    case ErrorKind::LengthMismatch:
        return detail::literal("LengthMismatch");
    case ErrorKind::UnsupportedVersion:
        return detail::literal("UnsupportedVersion");
    case ErrorKind::OutOfRange:
        return detail::literal("OutOfRange");
    case ErrorKind::InvalidField:
        return detail::literal("InvalidField");
    case ErrorKind::ReservedValue:
        return detail::literal("ReservedValue");
    case ErrorKind::CrcMismatch:
        return detail::literal("CrcMismatch");
    case ErrorKind::HeaderUncorrectable:
        return detail::literal("HeaderUncorrectable");
    case ErrorKind::UnknownChannel:
        return detail::literal("UnknownChannel");
    case ErrorKind::InvalidConfig:
        return detail::literal("InvalidConfig");
    case ErrorKind::WouldBlock:
        return detail::literal("WouldBlock");
    case ErrorKind::InvalidState:
        return detail::literal("InvalidState");
    case ErrorKind::Security:
        return detail::literal("Security");
    case ErrorKind::CapacityExceeded:
        return detail::literal("CapacityExceeded");
    case ErrorKind::InvalidHandle:
        return detail::literal("InvalidHandle");
    case ErrorKind::Malformed:
        return detail::literal("Malformed");
    }
    return {};
}

/// The kind whose catalogue name is `name`, if defined (GEN-040).
[[nodiscard]] constexpr std::optional<ErrorKind> from_name(std::span<const char> name) noexcept {
    for (const ErrorKind kind : all_kinds) {
        if (detail::names_equal(error::name(kind), name)) {
            return kind;
        }
    }
    return std::nullopt;
}

} // namespace error

} // namespace aldis::core

#endif // ALDIS_CORE_ERROR_HPP
