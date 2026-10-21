// aldis_vectors: the host vector runner (CPP-053, TV-012; docs/milestones/c0-foundations.md D5,
// WP-4). Registered with CTest once per file of test-vectors/core/.
//
//   aldis_vectors <file.json>...
//
// Runs every live case of each file against aldis_core with exact comparison, prints a summary
// with the identifier of every failing case, and exits with 1 if any case failed, a file could not
// be loaded, or a file had no runnable case.

#include "aldis/vectors/loader.hpp"
#include "aldis/vectors/runner.hpp"

#include <cstddef>
#include <cstdlib>
#include <exception>
#include <iostream>
#include <span>
#include <string>

int main(int argc, char** argv) {
    // argv has argc entries.
    const std::span<char*> args{argv, static_cast<std::size_t>(argc)};
    if (args.size() < 2) {
        std::cerr << "usage: aldis_vectors <file.json>...\n";
        return EXIT_FAILURE;
    }
    bool ok = true;
    for (const char* arg : args.subspan(1)) {
        const std::string path{arg};
        try {
            const aldis::vectors::Report report =
                aldis::vectors::run(aldis::vectors::parse_codec(aldis::vectors::read_file(path)));
            std::cout << aldis::vectors::summary(report, path);
            ok = ok && aldis::vectors::passed(report);
        } catch (const std::exception& e) {
            std::cout << path << ": " << e.what() << "\n";
            ok = false;
        }
    }
    return ok ? EXIT_SUCCESS : EXIT_FAILURE;
}
