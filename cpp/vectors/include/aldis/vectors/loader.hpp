// Host-only loader of the shared vector files (CPP-053, TV-001…TV-003, TV-010, TV-012, TV-021;
// docs/milestones/c0-foundations.md D5, WP-4). Test tooling: it uses nlohmann/json, exceptions,
// and the heap, and is never part of a library build (CPP-003).
//
// The loader follows the model of `aldis-testkit` (crates/aldis-testkit/src/model.rs) and the
// published schemas (test-vectors/schema/): the file, case, `superseded`, and scenario step
// objects are closed (`additionalProperties: false`), so an unknown key fails the file; a value
// the schema types as nullable may be `null`, which means absent. `params`, `config`, and
// `expect` are open in the schema; for the core operations the loader then applies the exact
// match of TV-012 per operation, as the Rust runner does by comparing output maps:
//
// - a success expectation must give every output the operation produces for that outcome and no
//   other key, with the value's JSON type and canonical form (upper-case hex) that the Rust
//   handler would produce; otherwise the case is rejected as incomplete or over-specified;
// - an error expectation gives `error` (a catalogue name) and optionally `field`, the code of the
//   field the error names (1…65535, M2 plan D3). The code is kept in the case and not compared in
//   C0 (`ExpectedError`). Any other key is rejected: the runner could not check it;
// - a parameter the operation does not take, a non-empty `config`, `mode: lenient`, or an
//   `input_hex` for an operation without input is rejected, so that no part of a case is ignored;
// - an unknown operation is rejected (TV-012).
//
// Rejected cases are reported with their identifiers, as failures, and are not run. Superseded
// cases are checked against the schema but not parsed per operation, and never run (TV-021).

#ifndef ALDIS_VECTORS_LOADER_HPP
#define ALDIS_VECTORS_LOADER_HPP

#include "aldis/vectors/core.hpp"

#include <nlohmann/json.hpp>

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

namespace aldis::vectors {

/// A vector file that cannot be loaded: malformed JSON, or content outside the schema.
class LoadError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

/// Where a file's expected values come from (TV-020).
enum class Provenance : std::uint8_t { BlueBookExample, HandDerived, CrossChecked, FuzzRegression };

/// Marks a released case or scenario as replaced (TV-021).
struct Superseded {
    /// Why it was superseded.
    std::string reason;
    /// The replacing case, if any.
    std::optional<std::string> by;
};

/// A live case loaded at run time: identifier, requirements, and the literal case data.
template <typename Case> struct HostCase {
    /// `id`.
    std::string id;
    /// `requirements`.
    std::vector<std::string> requirements;
    /// The case.
    Case data;
};

/// The live cases of a file, by operation.
struct CoreCases {
    std::vector<HostCase<BitsGetCase>> bits_get;
    std::vector<HostCase<BitsSetCase>> bits_set;
    std::vector<HostCase<CounterClassifyCase>> counter_classify;
    std::vector<HostCase<CounterIncrementCase>> counter_increment;
    std::vector<HostCase<Crc16Case>> crc16;
    std::vector<HostCase<ErrorCodeCase>> error_code;
    std::vector<HostCase<FrameLayoutCase>> frame_layout;
    std::vector<HostCase<OidFillCase>> oid_fill;
    std::vector<HostCase<OidFillSequenceCase>> oid_fill_sequence;
};

/// Number of cases in `cases`.
[[nodiscard]] std::size_t size(const CoreCases& cases);

/// A live case the loader rejected, and why.
struct Rejected {
    /// `id`.
    std::string case_id;
    /// What is wrong with it.
    std::string message;
};

/// A codec vector file (TV-001…TV-003).
struct CodecFile {
    /// `standard`.
    std::string standard;
    /// `provenance`.
    Provenance provenance{Provenance::HandDerived};
    /// `provenance_note`.
    std::optional<std::string> provenance_note;
    /// The live cases that were accepted.
    CoreCases cases;
    /// The live cases that were rejected; each is a failure.
    std::vector<Rejected> rejected;
    /// Number of superseded cases, which are not run (TV-021).
    std::size_t superseded{0};
};

/// One step of an engine scenario (TV-010).
struct Step {
    /// `at_us`.
    std::uint64_t at_us{0};
    /// `input`: the single input of the step.
    nlohmann::json input;
    /// `expect`: the complete, ordered outputs.
    std::vector<nlohmann::json> expect;
    /// `expect_state`.
    std::optional<std::string> expect_state;
    /// `expect_timeout_us`.
    std::optional<std::uint64_t> expect_timeout_us;
};

/// An engine scenario file (TV-010…TV-012). No engine exists in C0; the model is loaded and
/// checked against the schema, so the runner of C3 starts from a tested loader.
struct ScenarioFile {
    /// `engine`.
    std::string engine;
    /// `id`.
    std::string id;
    /// `requirements`.
    std::vector<std::string> requirements;
    /// `description`.
    std::optional<std::string> description;
    /// `provenance`.
    Provenance provenance{Provenance::HandDerived};
    /// `provenance_note`.
    std::optional<std::string> provenance_note;
    /// `config`: an object.
    nlohmann::json config;
    /// `steps`.
    std::vector<Step> steps;
    /// `superseded`.
    std::optional<Superseded> superseded;
};

/// Any vector file.
using VectorFile = std::variant<CodecFile, ScenarioFile>;

/// Parses a vector file, dispatching on `kind`. Throws `LoadError`.
[[nodiscard]] VectorFile parse(std::string_view text);

/// Parses a codec vector file. Throws `LoadError`, also for another kind.
[[nodiscard]] CodecFile parse_codec(std::string_view text);

/// The contents of `path`. Throws `LoadError`.
[[nodiscard]] std::string read_file(const std::filesystem::path& path);

/// Octets as upper-case hex, the canonical form of the vector files.
[[nodiscard]] std::string hex(std::span<const std::byte> octets);

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_LOADER_HPP
