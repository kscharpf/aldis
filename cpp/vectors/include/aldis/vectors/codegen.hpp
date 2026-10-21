// Code generation of the core vector cases (CPP-053, CPP-034; docs/milestones/c0-foundations.md
// D5, WP-4): the cases the host loader reads, written as `constexpr` arrays of the same literal
// case structs, so the freestanding exerciser (WP-5) and the `static_assert`s of CPP-034 run them
// without JSON. The counterpart of exerciser/build.rs. Test tooling, host only.

#ifndef ALDIS_VECTORS_CODEGEN_HPP
#define ALDIS_VECTORS_CODEGEN_HPP

#include "aldis/vectors/loader.hpp"

#include <string>
#include <utility>
#include <vector>

namespace aldis::vectors {

/// The generated header for `files` (each a name for the comment, and the loaded file), with
/// the arrays of namespace `aldis::vectors::generated`. Throws `LoadError` if any live case was
/// rejected, so the generated data never silently lacks a case.
[[nodiscard]] std::string
generate_header(const std::vector<std::pair<std::string, CodecFile>>& files);

} // namespace aldis::vectors

#endif // ALDIS_VECTORS_CODEGEN_HPP
