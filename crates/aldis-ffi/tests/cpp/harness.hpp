// The shared part of the C++ engine tests of aldis.hpp (CABI-072): scenario files, hex, and the
// comparison of a step with its expected outputs (TV-010, TV-012).

#ifndef ALDIS_TESTS_HARNESS_HPP
#define ALDIS_TESTS_HARNESS_HPP

#include <cstdint>
#include <cstdio>
#include <string>
#include <string_view>
#include <vector>

#include "aldis/aldis.hpp"
#include "json.hpp"

namespace harness {

using tj::Value;

inline int hexval(char c) {
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1;
}

inline std::vector<std::uint8_t> unhex(const std::string& text) {
    std::vector<std::uint8_t> out;
    for (std::size_t i = 0; i + 1 < text.size(); i += 2) {
        out.push_back(static_cast<std::uint8_t>(hexval(text[i]) * 16 + hexval(text[i + 1])));
    }
    return out;
}

inline std::string hex(const std::uint8_t* octets, std::size_t len) {
    static const char digits[] = "0123456789ABCDEF";
    std::string out;
    for (std::size_t i = 0; i < len; ++i) {
        out += digits[octets[i] >> 4];
        out += digits[octets[i] & 15];
    }
    return out;
}

inline Value number(std::uint64_t n) { return Value::of_number(n); }
inline Value text(std::string s) { return Value::of_string(std::move(s)); }

/// The name of a status as the scenarios write an error (the `ErrorKind` variant).
inline const char* error_name(aldis::Status s) {
    using aldis::Status;
    switch (s) {
    case Status::Ok: return "Ok";
    case Status::BufferTooSmall: return "BufferTooSmall";
    case Status::Incomplete: return "Incomplete";
    case Status::LengthMismatch: return "LengthMismatch";
    case Status::UnsupportedVersion: return "UnsupportedVersion";
    case Status::OutOfRange: return "OutOfRange";
    case Status::InvalidField: return "InvalidField";
    case Status::ReservedValue: return "ReservedValue";
    case Status::CrcMismatch: return "CrcMismatch";
    case Status::HeaderUncorrectable: return "HeaderUncorrectable";
    case Status::UnknownChannel: return "UnknownChannel";
    case Status::InvalidConfig: return "InvalidConfig";
    case Status::WouldBlock: return "WouldBlock";
    case Status::InvalidState: return "InvalidState";
    case Status::Security: return "Security";
    case Status::CapacityExceeded: return "CapacityExceeded";
    case Status::InvalidHandle: return "InvalidHandle";
    case Status::Malformed: return "Malformed";
    case Status::InternalError: return "InternalError";
    }
    return "?";
}

/// One scenario with its setup chain expanded (the setups of `extends` first).
struct Scenario {
    const Value* def = nullptr;
    const Value* config = nullptr;
    std::vector<const Value*> steps;
    bool superseded = false;
};

/// An engine-scenario file. The scenarios point into `root`: the file stays where it is.
struct File {
    File() = default;
    File(const File&) = delete;
    File& operator=(const File&) = delete;

    Value root;
    std::vector<Scenario> scenarios;

    bool load(const std::string& path, std::string_view engine) {
        if (!tj::parse_file(path, root)) {
            std::fprintf(stderr, "%s: cannot read or parse\n", path.c_str());
            return false;
        }
        if (!root.at("engine").is(engine)) {
            std::fprintf(stderr, "%s: engine is not %.*s\n", path.c_str(),
                         static_cast<int>(engine.size()), engine.data());
            return false;
        }
        const Value* setups = root.find("setups");
        for (const Value& s : root.at("scenarios").array) {
            Scenario out;
            out.def = &s;
            out.config = s.has("config") ? &s.at("config") : &root.at("config");
            out.superseded = s.has("superseded");
            std::vector<const Value*> chain;
            const Value* name = s.find("setup");
            // bound: one setup of the file per pass, and a chain longer than the file's setups
            // is a loop, which the check below ends.
            while (name != nullptr && setups != nullptr) {
                const Value* found = nullptr;
                for (const Value& candidate : setups->array) {
                    if (candidate.at("name").str() == name->str()) {
                        found = &candidate;
                    }
                }
                if (found == nullptr || chain.size() > setups->array.size()) {
                    std::fprintf(stderr, "%s: bad setup chain\n", path.c_str());
                    return false;
                }
                chain.push_back(found);
                name = found->find("extends");
            }
            for (std::size_t i = chain.size(); i-- > 0;) {
                for (const Value& step : chain[i]->at("steps").array) {
                    out.steps.push_back(&step);
                }
            }
            for (const Value& step : s.at("steps").array) {
                out.steps.push_back(&step);
            }
            scenarios.push_back(std::move(out));
        }
        return true;
    }
};

/// The outcome of one file: scenarios run, skipped, and the failures.
struct Tally {
    int run = 0;
    int skipped = 0;
    std::vector<std::string> failures;

    void fail(const Scenario& sc, std::size_t step, const Value& input, const std::string& why) {
        failures.push_back(sc.def->at("id").str() + " step " + std::to_string(step) + " " +
                           tj::dump(input) + ": " + why);
    }
};

/// Compares the outputs of a step with the expectation; returns the problem or an empty string.
inline std::string compare_outputs(const Value& got, const Value& step) {
    if (tj::equal(got, step.at("expect"))) {
        return "";
    }
    return "outputs\n      got  " + tj::dump(got) + "\n      want " + tj::dump(step.at("expect"));
}

inline int report(const char* name, const Tally& t) {
    for (const std::string& f : t.failures) {
        std::fprintf(stderr, "FAIL %s\n", f.c_str());
    }
    std::printf("%s: %d scenarios run, %d skipped, %zu failed\n", name, t.run, t.skipped,
                t.failures.size());
    // A file that runs nothing is a failure, unless a runner skipped its scenarios on purpose.
    return t.failures.empty() && t.run + t.skipped > 0 ? 0 : 1;
}

}  // namespace harness

#endif  // ALDIS_TESTS_HARNESS_HPP
