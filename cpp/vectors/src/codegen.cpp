// Code generation of the core vector cases (see aldis/vectors/codegen.hpp).

#include "aldis/vectors/codegen.hpp"

#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"
#include "aldis/vectors/checks/bits_get.hpp"
#include "aldis/vectors/checks/bits_set.hpp"
#include "aldis/vectors/checks/counter_classify.hpp"
#include "aldis/vectors/checks/counter_increment.hpp"
#include "aldis/vectors/checks/crc16.hpp"
#include "aldis/vectors/checks/error_code.hpp"
#include "aldis/vectors/checks/frame_layout.hpp"
#include "aldis/vectors/checks/oid_fill.hpp"
#include "aldis/vectors/checks/oid_fill_sequence.hpp"
#include "aldis/vectors/loader.hpp"

#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace aldis::vectors {

namespace {

/// A C++ string literal with the content `s`.
std::string literal_of(std::string_view s) {
    std::string out = "\"";
    for (const char c : s) {
        const unsigned u = static_cast<unsigned char>(c);
        if (c == '"' || c == '\\') {
            out += '\\';
            out += c;
        } else if (u < 0x20U || u >= 0x7FU) {
            // Octal escapes end after three digits, so the next character cannot extend them.
            out += '\\';
            out += static_cast<char>('0' + ((u >> 6U) & 7U));
            out += static_cast<char>('0' + ((u >> 3U) & 7U));
            out += static_cast<char>('0' + (u & 7U));
        } else {
            out += c;
        }
    }
    return out + "\"";
}

std::string lit(std::uint64_t v) { return std::to_string(v) + "U"; }

std::string_view lit_bool(bool v) { return v ? "true" : "false"; }

std::string value(std::uint64_t v) { return "std::uint64_t{" + lit(v) + "}"; }

std::string value(std::uint16_t v) { return "std::uint16_t{" + lit(v) + "}"; }

std::string value(const Octets& v) {
    const std::span<const std::byte> octets = v.view();
    std::string out = "octets(std::array<std::uint8_t, " + std::to_string(octets.size()) + ">{";
    bool first = true;
    for (const std::byte b : octets) {
        out += (first ? "0x" : ", 0x") + hex(std::span<const std::byte>{&b, 1}) + "U";
        first = false;
    }
    return out + "})";
}

std::string value(const OctetsList& v) {
    std::string out = "octets_list(std::array<Octets, " + std::to_string(v.size()) + ">{";
    bool first = true;
    for (const Octets& o : v.view()) {
        out += (first ? "" : ", ") + value(o);
        first = false;
    }
    return out + "})";
}

std::string value(const Fills& v) {
    std::string out = "fills(std::array<std::size_t, " + std::to_string(v.size()) + ">{";
    bool first = true;
    for (const std::size_t n : v.view()) {
        out += (first ? "" : ", ") + lit(n);
        first = false;
    }
    return out + "})";
}

std::string value(const core::Continuity& v) {
    switch (v.kind()) {
    case core::Continuity::Kind::InSequence:
        return "core::Continuity::in_sequence()";
    case core::Continuity::Kind::Repeat:
        return "core::Continuity::repeat()";
    case core::Continuity::Kind::Backwards:
        return "core::Continuity::backwards()";
    case core::Continuity::Kind::Gap:
        return "core::Continuity::gap(" + lit(v.missing().value_or(0)) + ")";
    }
    return "";
}

std::string value(const ErrorCode& v) {
    return "ErrorCode{.code = " + lit(v.code) + ", .c_abi = " + std::to_string(v.c_abi) + "}";
}

std::string value(const LayoutRanges& v) {
    std::string out = "LayoutRanges{{";
    bool first = true;
    for (const LayoutRange& r : v) {
        out += std::string(first ? "" : ", ") + "{.start = " + lit(r.start) +
               ", .end = " + lit(r.end) + "}";
        first = false;
    }
    return out + "}}";
}

std::string value(const Name& v) {
    const std::span<const char> chars = v.view();
    return "name(" + literal_of(std::string_view{chars.data(), chars.size()}) + ")";
}

template <typename Out> std::string type_name();
template <> std::string type_name<std::uint64_t>() { return "std::uint64_t"; }
template <> std::string type_name<std::uint16_t>() { return "std::uint16_t"; }
template <> std::string type_name<Octets>() { return "Octets"; }
template <> std::string type_name<OctetsList>() { return "OctetsList"; }
template <> std::string type_name<core::Continuity>() { return "core::Continuity"; }
template <> std::string type_name<ErrorCode>() { return "ErrorCode"; }
template <> std::string type_name<LayoutRanges>() { return "LayoutRanges"; }

template <typename Out> std::string value(const Expect<Out>& e) {
    const std::string type = "Expect<" + type_name<Out>() + ">";
    if (const Out* v = e.value(); v != nullptr) {
        return type + "::success(" + value(*v) + ")";
    }
    const ExpectedError* err = e.error();
    if (err == nullptr) {
        throw LoadError("an expectation with neither a value nor an error");
    }
    const std::span<const char> kind = core::error::name(err->kind());
    std::string out =
        type + "::failure(ExpectedError{core::ErrorKind::" + std::string(kind.begin(), kind.end());
    if (err->field().has_value()) {
        out += ", std::uint16_t{" + lit(*err->field()) + "}";
    }
    return out + "})";
}

std::string fields(const BitsGetCase& c) {
    return ".input = " + value(c.input) + ", .bit_offset = " + lit(c.bit_offset) +
           ", .width = " + lit(c.width) + ", .expect = " + value(c.expect);
}

std::string fields(const BitsSetCase& c) {
    return ".input = " + value(c.input) + ", .bit_offset = " + lit(c.bit_offset) +
           ", .width = " + lit(c.width) + ", .value = " + lit(c.value) +
           ", .expect = " + value(c.expect);
}

std::string fields(const CounterClassifyCase& c) {
    return ".bits = " + lit(c.bits) + ", .prev = " + lit(c.prev) + ", .next = " + lit(c.next) +
           ", .expect = " + value(c.expect);
}

std::string fields(const CounterIncrementCase& c) {
    return ".bits = " + lit(c.bits) + ", .value = " + lit(c.value) +
           ", .expect = " + value(c.expect);
}

std::string fields(const Crc16Case& c) {
    return ".input = " + value(c.input) + ", .expect = " + value(c.expect);
}

std::string fields(const ErrorCodeCase& c) {
    return ".kind = " + value(c.kind) + ", .expect = " + value(c.expect);
}

std::string fields(const FrameLayoutCase& c) {
    return ".frame_len = " + lit(c.frame_len) +
           ", .primary_header_len = " + lit(c.primary_header_len) +
           ", .secondary_len = " + lit(c.secondary_len) +
           ", .security_header_len = " + lit(c.security_header_len) +
           ", .security_trailer_len = " + lit(c.security_trailer_len) +
           ", .ocf = " + std::string(lit_bool(c.ocf)) +
           ", .fecf = " + std::string(lit_bool(c.fecf)) + ", .expect = " + value(c.expect);
}

std::string fields(const OidFillCase& c) {
    return ".octets = " + lit(c.octets) + ", .expect = " + value(c.expect);
}

std::string fields(const OidFillSequenceCase& c) {
    return ".fills = " + value(c.fills) + ", .expect = " + value(c.expect);
}

/// One array of the generated header: every case of one operation, from every file.
template <typename Case>
std::string array(const std::string& name, const std::string& type,
                  const std::vector<std::pair<std::string, CodecFile>>& files,
                  std::vector<HostCase<Case>> CoreCases::* member) {
    std::vector<const HostCase<Case>*> cases;
    for (const auto& [file_name, file] : files) {
        for (const HostCase<Case>& c : file.cases.*member) {
            cases.push_back(&c);
        }
    }
    std::string out = "/// `" + name + "` cases (" + std::to_string(cases.size()) + ").\n" +
                      "inline constexpr std::array<Named<" + type + ">, " +
                      std::to_string(cases.size()) + "> " + name + "{";
    if (cases.empty()) {
        return out + "};\n\n";
    }
    out += "{\n";
    for (const HostCase<Case>* c : cases) {
        out += "    {.id = literal(" + literal_of(c->id) + "),\n     .data = {" + fields(c->data) +
               "}},\n";
    }
    return out + "}};\n\n";
}

} // namespace

std::string generate_header(const std::vector<std::pair<std::string, CodecFile>>& files) {
    std::size_t live = 0;
    std::size_t superseded = 0;
    std::string sources;
    for (const auto& [name, file] : files) {
        if (!file.rejected.empty()) {
            const Rejected& r = file.rejected.front();
            throw LoadError(name + ": case " + r.case_id + " rejected: " + r.message);
        }
        live += size(file.cases);
        superseded += file.superseded;
        sources += "//   " + name + "\n";
    }
    std::string out =
        "// Generated by aldis_vecgen (docs/milestones/c0-foundations.md D5, WP-4). Do not edit.\n"
        "//\n"
        "// The live cases of:\n" +
        sources +
        "// as constexpr arrays of the case structs of aldis/vectors/core.hpp (CPP-053, CPP-034).\n"
        "\n"
        "#ifndef ALDIS_VECTORS_GENERATED_CORE_HPP\n"
        "#define ALDIS_VECTORS_GENERATED_CORE_HPP\n"
        "\n"
        "#include \"aldis/core/counter.hpp\"\n"
        "#include \"aldis/core/error.hpp\"\n"
        "#include \"aldis/vectors/core.hpp\"\n"
        "\n"
        "#include <array>\n"
        "#include <cstddef>\n"
        "#include <cstdint>\n"
        "\n"
        "namespace aldis::vectors::generated {\n"
        "\n"
        "/// Number of live cases, which the arrays below hold.\n"
        "inline constexpr std::size_t live_cases = " +
        lit(live) +
        ";\n"
        "\n"
        "/// Number of superseded cases, which are not run (TV-021).\n"
        "inline constexpr std::size_t superseded_cases = " +
        lit(superseded) + ";\n\n";
    out += array<BitsGetCase>("bits_get", "BitsGetCase", files, &CoreCases::bits_get);
    out += array<BitsSetCase>("bits_set", "BitsSetCase", files, &CoreCases::bits_set);
    out += array<CounterClassifyCase>("counter_classify", "CounterClassifyCase", files,
                                      &CoreCases::counter_classify);
    out += array<CounterIncrementCase>("counter_increment", "CounterIncrementCase", files,
                                       &CoreCases::counter_increment);
    out += array<Crc16Case>("crc16", "Crc16Case", files, &CoreCases::crc16);
    out += array<ErrorCodeCase>("error_code", "ErrorCodeCase", files, &CoreCases::error_code);
    out +=
        array<FrameLayoutCase>("frame_layout", "FrameLayoutCase", files, &CoreCases::frame_layout);
    out += array<OidFillCase>("oid_fill", "OidFillCase", files, &CoreCases::oid_fill);
    out += array<OidFillSequenceCase>("oid_fill_sequence", "OidFillSequenceCase", files,
                                      &CoreCases::oid_fill_sequence);
    out +=
        "} // namespace aldis::vectors::generated\n\n#endif // ALDIS_VECTORS_GENERATED_CORE_HPP\n";
    return out;
}

} // namespace aldis::vectors
