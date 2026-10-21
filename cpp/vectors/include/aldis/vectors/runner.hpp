// Host runner of the core vector cases (CPP-053, TV-012, TV-021; docs/milestones/c0-foundations.md
// D5, WP-4): the C++ counterpart of `aldis_testkit::run_codec` with the handler of
// crates/aldis-core/tests/vectors.rs. Test tooling, host only.

#ifndef ALDIS_VECTORS_RUNNER_HPP
#define ALDIS_VECTORS_RUNNER_HPP

#include "aldis/vectors/loader.hpp"

#include <cstddef>
#include <string>
#include <vector>

namespace aldis::vectors {

/// One failed case.
struct Failure {
    /// `id`.
    std::string case_id;
    /// What differed, or why the loader rejected the case.
    std::string message;
};

/// The result of running one file.
struct Report {
    /// Cases that passed.
    std::size_t passed{0};
    /// Superseded cases, which are not run (TV-021).
    std::size_t superseded{0};
    /// Cases that failed, rejected cases first.
    std::vector<Failure> failures;
};

/// Runs every live case of `file` through its check (TV-012); rejected cases are failures.
[[nodiscard]] Report run(const CodecFile& file);

/// Whether `report` passes: no failure, and at least one case ran (as `Report::assert_passed`
/// of `aldis-testkit`).
[[nodiscard]] bool passed(const Report& report);

/// A readable summary of `report` for `name`, one line per failure.
[[nodiscard]] std::string summary(const Report& report, const std::string& name);

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_RUNNER_HPP
