// Host runner of the core vector cases (see aldis/vectors/runner.hpp).

#include "aldis/vectors/runner.hpp"

#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"
#include "aldis/vectors/checks/error_code.hpp"
#include "aldis/vectors/checks/frame_layout.hpp"
#include "aldis/vectors/loader.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <vector>

namespace aldis::vectors {

namespace {

std::string text(std::uint64_t v) { return std::to_string(v); }

std::string text(std::uint16_t v) {
    const std::array<std::byte, 2> octets{
        std::byte{static_cast<std::uint8_t>(v >> 8U)},
        std::byte{static_cast<std::uint8_t>(v & 0xFFU)},
    };
    return "\"" + hex(octets) + "\"";
}

std::string text(const Octets& v) { return "\"" + hex(v.view()) + "\""; }

std::string text(const OctetsList& v) {
    std::string out = "[";
    for (const Octets& o : v.view()) {
        out += (out.size() > 1 ? ", " : "") + text(o);
    }
    return out + "]";
}

std::string text(const core::Continuity& v) {
    switch (v.kind()) {
    case core::Continuity::Kind::InSequence:
        return "in-sequence";
    case core::Continuity::Kind::Repeat:
        return "repeat";
    case core::Continuity::Kind::Backwards:
        return "backwards";
    case core::Continuity::Kind::Gap:
        return "gap (missing " + std::to_string(v.missing().value_or(0)) + ")";
    }
    return "?";
}

std::string text(const ErrorCode& v) {
    return "code " + std::to_string(v.code) + ", c_abi " + std::to_string(v.c_abi);
}

std::string text(const LayoutRanges& v) {
    std::string out;
    for (const LayoutRange& r : v) {
        out += (out.empty() ? "" : " ") +
               ("[" + std::to_string(r.start) + ", " + std::to_string(r.end) + ")");
    }
    return out;
}

std::string kind_name(core::ErrorKind kind) {
    const std::span<const char> n = core::error::name(kind);
    return n.empty() ? "code " + std::to_string(core::error::code(kind))
                     : std::string(n.begin(), n.end());
}

template <typename Out> std::string text(const core::Result<Out>& actual) {
    if (const Out* v = actual.ok(); v != nullptr) {
        return "success " + text(*v);
    }
    const core::Error* e = actual.error();
    return e != nullptr ? "error " + kind_name(e->kind()) : "?";
}

std::string text(const std::optional<ErrorCode>& actual) {
    return actual.has_value() ? "success " + text(*actual) : "unknown error kind";
}

template <typename Out> std::string expected_text(const Expect<Out>& expect) {
    if (const Out* v = expect.value(); v != nullptr) {
        return "success " + text(*v);
    }
    const ExpectedError* e = expect.error();
    return e != nullptr ? "error " + kind_name(e->kind()) : "?";
}

/// What differs between the case and its outcome.
template <typename Case> std::string message(const Case& c, Verdict v) {
    const std::string expected = expected_text(c.expect);
    const std::string actual = text(run(c));
    switch (v.mismatch()) {
    case Mismatch::None:
        return "passed";
    case Mismatch::FormsDisagree:
        return "the two forms of the operation disagree (" + actual + ")";
    case Mismatch::UnknownName:
        return "the error kind is not in the catalogue";
    case Mismatch::ExpectedSuccess:
    case Mismatch::ExpectedError:
    case Mismatch::WrongError:
    case Mismatch::WrongValue:
        break;
    }
    return "expected " + expected + ", got " + actual;
}

template <typename Case> void run_all(const std::vector<HostCase<Case>>& cases, Report& report) {
    for (const HostCase<Case>& c : cases) {
        const Verdict v = check(c.data);
        if (v.passed()) {
            ++report.passed;
        } else {
            report.failures.push_back(Failure{.case_id = c.id, .message = message(c.data, v)});
        }
    }
}

} // namespace

Report run(const CodecFile& file) {
    Report report;
    report.superseded = file.superseded;
    for (const Rejected& r : file.rejected) {
        report.failures.push_back(Failure{.case_id = r.case_id, .message = r.message});
    }
    const CoreCases& c = file.cases;
    run_all(c.bits_get, report);
    run_all(c.bits_set, report);
    run_all(c.counter_classify, report);
    run_all(c.counter_increment, report);
    run_all(c.crc16, report);
    run_all(c.error_code, report);
    run_all(c.frame_layout, report);
    run_all(c.oid_fill, report);
    run_all(c.oid_fill_sequence, report);
    return report;
}

bool passed(const Report& report) { return report.failures.empty() && report.passed > 0; }

std::string summary(const Report& report, const std::string& name) {
    std::string out = name + ": " + std::to_string(report.passed) + " passed, " +
                      std::to_string(report.failures.size()) + " failed, " +
                      std::to_string(report.superseded) + " superseded (not run)\n";
    for (const Failure& f : report.failures) {
        out += "  " + f.case_id + ": " + f.message + "\n";
    }
    if (report.failures.empty() && report.passed == 0) {
        out += "  no runnable cases\n";
    }
    return out;
}

} // namespace aldis::vectors
