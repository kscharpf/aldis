// aldis_vecgen: writes the core vector cases as constexpr arrays into a header (CPP-053,
// CPP-034; docs/milestones/c0-foundations.md D5, WP-4).
//
//   aldis_vecgen <output.hpp> <vectors-dir> <file.json>...
//
// Loads each file (relative to <vectors-dir>) with the host loader and writes the header of
// `aldis::vectors::generate_header`. Fails, writing nothing, if a file cannot be loaded or a live
// case is rejected. The header is rewritten only when its content changes, so an unchanged vector
// set does not rebuild its users. Cross builds take this tool from a host build (ALDIS_VECGEN).

#include "aldis/vectors/codegen.hpp"
#include "aldis/vectors/loader.hpp"

#include <cstddef>
#include <cstdlib>
#include <exception>
#include <filesystem>
#include <fstream>
#include <ios>
#include <iostream>
#include <span>
#include <string>
#include <utility>
#include <vector>

int main(int argc, char** argv) {
    // argv has argc entries.
    const std::span<char*> args{argv, static_cast<std::size_t>(argc)};
    if (args.size() < 4) {
        std::cerr << "usage: aldis_vecgen <output.hpp> <vectors-dir> <file.json>...\n";
        return EXIT_FAILURE;
    }
    const std::filesystem::path output{args.subspan(1).front()};
    const std::filesystem::path root{args.subspan(2).front()};
    try {
        std::vector<std::pair<std::string, aldis::vectors::CodecFile>> files;
        for (const char* arg : args.subspan(3)) {
            const std::string name{arg};
            try {
                files.emplace_back(
                    "test-vectors/" + name,
                    aldis::vectors::parse_codec(aldis::vectors::read_file(root / name)));
            } catch (const aldis::vectors::LoadError& e) {
                throw aldis::vectors::LoadError(name + ": " + e.what());
            }
        }
        const std::string header = aldis::vectors::generate_header(files);
        if (std::filesystem::exists(output) && aldis::vectors::read_file(output) == header) {
            return EXIT_SUCCESS;
        }
        std::filesystem::create_directories(output.parent_path());
        std::ofstream out(output, std::ios::binary | std::ios::trunc);
        out << header;
        out.close();
        if (!out) {
            std::cerr << "aldis_vecgen: cannot write " << output.string() << "\n";
            return EXIT_FAILURE;
        }
    } catch (const std::exception& e) {
        std::cerr << "aldis_vecgen: " << e.what() << "\n";
        return EXIT_FAILURE;
    }
    return EXIT_SUCCESS;
}
