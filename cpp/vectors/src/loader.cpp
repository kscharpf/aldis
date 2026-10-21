// Host-only loader of the shared vector files (see aldis/vectors/loader.hpp for the rules).

#include "aldis/vectors/loader.hpp"

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

#include <nlohmann/json.hpp>
#include <nlohmann/json_fwd.hpp>

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <functional>
#include <ios>
#include <limits>
#include <map>
#include <optional>
#include <set>
#include <span>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

namespace aldis::vectors {

namespace {

using nlohmann::json;

/// A problem with one live case: it is rejected, and the rest of the file still loads.
class CaseError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

/// The members of one JSON object, each taken at most once; `finish` reports the rest. A member
/// whose value is `null` counts as absent where the schema types it as nullable (`take_opt`).
class Members {
public:
    Members(const json& value, std::string where) : where_{std::move(where)} {
        if (!value.is_object()) {
            throw LoadError(where_ + ": expected an object");
        }
        for (const auto& [key, member] : value.items()) {
            members_.emplace(key, &member);
        }
    }

    /// The member `key`, or nullptr if absent.
    const json* take(const std::string& key) {
        const auto it = members_.find(key);
        if (it == members_.end()) {
            return nullptr;
        }
        const json* member = it->second;
        members_.erase(it);
        return member;
    }

    /// The member `key`, or nullptr if absent or null.
    const json* take_opt(const std::string& key) {
        const json* member = take(key);
        return member != nullptr && member->is_null() ? nullptr : member;
    }

    /// Whether no member is left.
    [[nodiscard]] bool empty() const { return members_.empty(); }

    /// The names of the members left, comma-separated.
    [[nodiscard]] std::string left() const {
        std::string out;
        for (const auto& [key, member] : members_) {
            out += (out.empty() ? "" : ", ") + key;
        }
        return out;
    }

    [[nodiscard]] const std::string& where() const { return where_; }

private:
    std::string where_;
    std::map<std::string, const json*> members_;
};

// --- Schema level: file, case, superseded, scenario (closed objects; LoadError) ---

std::string schema_string(const json* v, const std::string& what) {
    if (v == nullptr || !v->is_string()) {
        throw LoadError(what + ": expected a string");
    }
    return v->get<std::string>();
}

std::optional<std::string> schema_opt_string(const json* v, const std::string& what) {
    if (v == nullptr) {
        return std::nullopt;
    }
    return schema_string(v, what);
}

std::vector<std::string> schema_strings(const json* v, const std::string& what) {
    if (v == nullptr || !v->is_array()) {
        throw LoadError(what + ": expected an array of strings");
    }
    std::vector<std::string> out;
    for (const json& item : *v) {
        out.push_back(schema_string(&item, what));
    }
    return out;
}

std::uint64_t schema_u64(const json* v, const std::string& what) {
    if (v == nullptr || !v->is_number_unsigned()) {
        throw LoadError(what + ": expected a non-negative integer");
    }
    return v->get<std::uint64_t>();
}

Provenance schema_provenance(const json* v, const std::string& what) {
    const std::string s = schema_string(v, what);
    static const std::map<std::string, Provenance, std::less<>> names{
        {"blue-book-example", Provenance::BlueBookExample},
        {"hand-derived", Provenance::HandDerived},
        {"cross-checked", Provenance::CrossChecked},
        {"fuzz-regression", Provenance::FuzzRegression},
    };
    const auto it = names.find(s);
    if (it == names.end()) {
        throw LoadError(what + ": unknown provenance \"" + s + "\"");
    }
    return it->second;
}

void schema_finish(const Members& m) {
    if (!m.empty()) {
        throw LoadError(m.where() + ": unknown field(s) " + m.left());
    }
}

std::optional<Superseded> schema_superseded(const json* v, const std::string& where) {
    if (v == nullptr) {
        return std::nullopt;
    }
    Members m(*v, where + ".superseded");
    Superseded s{
        .reason = schema_string(m.take("reason"), m.where() + ".reason"),
        .by = schema_opt_string(m.take_opt("by"), m.where() + ".by"),
    };
    schema_finish(m);
    return s;
}

/// A case at the level of the schema: every member checked for its type, nothing interpreted.
struct RawCase {
    std::string id;
    std::vector<std::string> requirements;
    std::string operation;
    std::optional<std::string> mode;
    const json* config{nullptr};
    const json* params{nullptr};
    std::optional<std::string> input_hex;
    const json* expect{nullptr};
    bool ok{false};
    bool superseded{false};
};

RawCase schema_case(const json& value, const std::string& where) {
    Members m(value, where);
    RawCase c;
    c.id = schema_string(m.take("id"), where + ".id");
    const std::string at = where + " (" + c.id + ")";
    c.requirements = schema_strings(m.take("requirements"), at + ".requirements");
    (void)schema_opt_string(m.take_opt("description"), at + ".description");
    c.operation = schema_string(m.take("operation"), at + ".operation");
    c.mode = schema_opt_string(m.take_opt("mode"), at + ".mode");
    if (c.mode.has_value() && *c.mode != "strict" && *c.mode != "lenient") {
        throw LoadError(at + R"(.mode: expected "strict" or "lenient")");
    }
    c.config = m.take_opt("config");
    if (c.config != nullptr && !c.config->is_object()) {
        throw LoadError(at + ".config: expected an object");
    }
    c.params = m.take_opt("params");
    if (c.params != nullptr && !c.params->is_object()) {
        throw LoadError(at + ".params: expected an object");
    }
    c.input_hex = schema_opt_string(m.take_opt("input_hex"), at + ".input_hex");
    c.expect = m.take("expect");
    if (c.expect == nullptr || !c.expect->is_object()) {
        throw LoadError(at + ".expect: expected an object");
    }
    const auto ok = c.expect->find("ok");
    if (ok == c.expect->end() || !ok->is_boolean()) {
        throw LoadError(at + ".expect.ok: expected a boolean");
    }
    c.ok = ok->get<bool>();
    const auto error = c.expect->find("error");
    if (error != c.expect->end() && !error->is_null() && !error->is_string()) {
        throw LoadError(at + ".expect.error: expected a string");
    }
    // A field code is a NonZeroU16 in the Rust model (M2 plan D3): 1…65535, or null.
    const auto field = c.expect->find("field");
    if (field != c.expect->end() && !field->is_null() &&
        (!field->is_number_unsigned() || field->get<std::uint64_t>() < 1 ||
         field->get<std::uint64_t>() > std::numeric_limits<std::uint16_t>::max())) {
        throw LoadError(at + ".expect.field: " + field->dump() + " is not a field code (1…65535)");
    }
    c.superseded = schema_superseded(m.take_opt("superseded"), at).has_value();
    schema_finish(m);
    return c;
}

// --- Operation level: parameters and expected outputs (CaseError) ---

std::uint64_t to_u64(const json* v, const std::string& what) {
    if (v == nullptr) {
        throw CaseError("missing " + what);
    }
    if (!v->is_number_unsigned()) {
        throw CaseError(what + ": expected a non-negative integer, got " + v->dump());
    }
    return v->get<std::uint64_t>();
}

template <typename T> T to_unsigned(const json* v, const std::string& what) {
    const std::uint64_t wide = to_u64(v, what);
    if (wide > std::numeric_limits<T>::max()) {
        throw CaseError(what + ": " + std::to_string(wide) + " does not fit the operation's " +
                        std::to_string(std::numeric_limits<T>::digits) + "-bit parameter");
    }
    return static_cast<T>(wide);
}

bool to_bool(const json* v, const std::string& what) {
    if (v == nullptr) {
        throw CaseError("missing " + what);
    }
    if (!v->is_boolean()) {
        throw CaseError(what + ": expected a boolean, got " + v->dump());
    }
    return v->get<bool>();
}

std::string to_string(const json* v, const std::string& what) {
    if (v == nullptr) {
        throw CaseError("missing " + what);
    }
    if (!v->is_string()) {
        throw CaseError(what + ": expected a string, got " + v->dump());
    }
    return v->get<std::string>();
}

unsigned hex_digit(char c, const std::string& what) {
    static constexpr std::string_view lower = "0123456789abcdef";
    static constexpr std::string_view upper = "0123456789ABCDEF";
    if (const std::size_t d = lower.find(c); d != std::string_view::npos) {
        return static_cast<unsigned>(d);
    }
    if (const std::size_t d = upper.find(c); d != std::string_view::npos) {
        return static_cast<unsigned>(d);
    }
    throw CaseError(what + ": invalid hex character '" + std::string(1, c) + "'");
}

/// Hex in either case without separators, as `aldis_testkit::hex::decode` accepts it.
std::vector<std::byte> decode_hex(const std::string& text, const std::string& what) {
    if (text.size() % 2 != 0) {
        throw CaseError(what + ": odd number of hex digits (" + std::to_string(text.size()) + ")");
    }
    std::vector<std::byte> out;
    for (std::size_t i = 0; i < text.size(); i += 2) {
        const unsigned hi = hex_digit(text.at(i), what);
        const unsigned lo = hex_digit(text.at(i + 1), what);
        out.push_back(std::byte{static_cast<std::uint8_t>((hi << 4U) | lo)});
    }
    return out;
}

Octets to_octets(std::span<const std::byte> data, const std::string& what) {
    const core::Result<Octets> octets = Octets::make(data);
    if (octets.ok() == nullptr) {
        throw CaseError(what + ": " + std::to_string(data.size()) + " octets, more than the " +
                        std::to_string(max_octets) + " a case can hold (max_octets)");
    }
    return *octets.ok();
}

/// An expected output in hex: upper case, as the Rust handler formats it and compares it as a
/// string (`aldis_testkit::hex::encode`).
Octets expected_hex(const json* v, const std::string& what) {
    const std::string text = to_string(v, what);
    const std::vector<std::byte> data = decode_hex(text, what);
    if (hex(data) != text) {
        throw CaseError(what + ": \"" + text +
                        "\" is not canonical upper-case hex, which the outcome is compared with");
    }
    return to_octets(data, what);
}

/// The parameters of a case without `params`. A reference to one object, so that `Members`,
/// which keeps pointers into the object it reads, never reads a temporary.
const json& no_params() {
    static const json empty = json::object();
    return empty;
}

/// What one live case gives an operation parser: its parameters, input, and expected outputs.
class CaseReader {
public:
    explicit CaseReader(const RawCase& raw)
        : raw_{&raw}, params_{raw.params != nullptr ? *raw.params : no_params(), "params"},
          values_{*raw.expect, "expect"} {
        (void)values_.take("ok");
        if (raw.config != nullptr && !raw.config->empty()) {
            throw CaseError("config is not used by " + raw.operation);
        }
        if (raw.mode.value_or("strict") != "strict") {
            throw CaseError("mode " + raw.mode.value_or("") + " is not used by " + raw.operation);
        }
    }

    /// The parameter `name`, or nullptr.
    const json* param(const std::string& name) { return params_.take(name); }

    /// The input octets; empty when `input_hex` is absent, as in the Rust handler.
    Octets input() {
        input_read_ = true;
        return to_octets(decode_hex(raw_->input_hex.value_or(""), "input_hex"), "input_hex");
    }

    /// The expected output `name`, or nullptr.
    const json* value(const std::string& name) { return values_.take(name); }

    /// The expectation: success with the value `parse` reads, or the expected error.
    template <typename Out, typename Parse> Expect<Out> expect(Parse parse) {
        const json* error = values_.take_opt("error");
        const json* field = values_.take_opt("field");
        if (raw_->ok) {
            if (error != nullptr) {
                throw CaseError("expect.ok is true but an error is named");
            }
            if (field != nullptr) {
                throw CaseError("expect.ok is true but a field code is given");
            }
            Expect<Out> e = Expect<Out>::success(parse(*this));
            if (!values_.empty()) {
                throw CaseError("over-specified expectation: " + values_.left() +
                                " not produced by " + raw_->operation + " (TV-012)");
            }
            return e;
        }
        if (error == nullptr) {
            throw CaseError("expect.ok is false but no error kind is named");
        }
        const std::string name = to_string(error, "expect.error");
        const std::optional<core::ErrorKind> kind =
            core::error::from_name(std::span<const char>{name.data(), name.size()});
        if (!kind.has_value()) {
            throw CaseError("expect.error: " + name +
                            " is not in the catalogue (03-api-model.md §9)");
        }
        // The schema level has checked the range (1…65535).
        std::optional<std::uint16_t> code;
        if (field != nullptr) {
            code = to_unsigned<std::uint16_t>(field, "expect.field");
        }
        if (!values_.empty()) {
            throw CaseError("error expectation with other keys: " + values_.left() +
                            " (only error and field are compared)");
        }
        return Expect<Out>::failure(ExpectedError{*kind, code});
    }

    /// Fails on parameters left over and on an input the operation does not read.
    void finish() {
        if (!params_.empty()) {
            throw CaseError("parameter(s) not used by " + raw_->operation + ": " + params_.left());
        }
        if (!input_read_ && raw_->input_hex.has_value()) {
            throw CaseError("input_hex is not used by " + raw_->operation);
        }
    }

private:
    const RawCase* raw_;
    Members params_;
    Members values_;
    bool input_read_{false};
};

std::uint64_t expected_u64(CaseReader& r, const std::string& name) {
    return to_u64(r.value(name), "expect." + name);
}

BitsGetCase parse_bits_get(CaseReader& r) {
    BitsGetCase c{
        .input = r.input(),
        .bit_offset = to_unsigned<std::size_t>(r.param("bit_offset"), "params.bit_offset"),
        .width = to_unsigned<std::uint8_t>(r.param("width"), "params.width"),
        .expect = r.expect<std::uint64_t>([](CaseReader& e) { return expected_u64(e, "value"); }),
    };
    return c;
}

BitsSetCase parse_bits_set(CaseReader& r) {
    return BitsSetCase{
        .input = r.input(),
        .bit_offset = to_unsigned<std::size_t>(r.param("bit_offset"), "params.bit_offset"),
        .width = to_unsigned<std::uint8_t>(r.param("width"), "params.width"),
        .value = to_u64(r.param("value"), "params.value"),
        .expect = r.expect<Octets>(
            [](CaseReader& e) { return expected_hex(e.value("output_hex"), "expect.output_hex"); }),
    };
}

CounterIncrementCase parse_counter_increment(CaseReader& r) {
    return CounterIncrementCase{
        .bits = to_unsigned<std::uint8_t>(r.param("bits"), "params.bits"),
        .value = to_u64(r.param("value"), "params.value"),
        .expect = r.expect<std::uint64_t>([](CaseReader& e) { return expected_u64(e, "value"); }),
    };
}

core::Continuity expected_continuity(CaseReader& r) {
    const std::string cls = to_string(r.value("class"), "expect.class");
    if (cls == "in-sequence") {
        return core::Continuity::in_sequence();
    }
    if (cls == "repeat") {
        return core::Continuity::repeat();
    }
    if (cls == "backwards") {
        return core::Continuity::backwards();
    }
    if (cls == "gap") {
        return core::Continuity::gap(expected_u64(r, "missing"));
    }
    throw CaseError("expect.class: unknown class \"" + cls + "\"");
}

CounterClassifyCase parse_counter_classify(CaseReader& r) {
    return CounterClassifyCase{
        .bits = to_unsigned<std::uint8_t>(r.param("bits"), "params.bits"),
        .prev = to_u64(r.param("prev"), "params.prev"),
        .next = to_u64(r.param("next"), "params.next"),
        .expect = r.expect<core::Continuity>(expected_continuity),
    };
}

Crc16Case parse_crc16(CaseReader& r) {
    return Crc16Case{
        .input = r.input(),
        .expect = r.expect<std::uint16_t>([](CaseReader& e) {
            const Octets value = expected_hex(e.value("value_hex"), "expect.value_hex");
            // The Rust handler formats the CRC as exactly four hex digits.
            const std::span<const std::byte> v = value.view();
            if (v.size() != 2) {
                throw CaseError("expect.value_hex: expected four hex digits");
            }
            return static_cast<std::uint16_t>((std::to_integer<unsigned>(v.front()) << 8U) |
                                              std::to_integer<unsigned>(v.back()));
        }),
    };
}

ErrorCodeCase parse_error_code(CaseReader& r) {
    const std::string kind = to_string(r.param("kind"), "params.kind");
    const core::Result<Name> name = Name::make(std::span<const char>{kind.data(), kind.size()});
    if (name.ok() == nullptr) {
        throw CaseError("params.kind: longer than " + std::to_string(max_name) + " characters");
    }
    return ErrorCodeCase{
        .kind = *name.ok(),
        .expect = r.expect<ErrorCode>([](CaseReader& e) {
            const auto code = to_unsigned<std::uint16_t>(e.value("code"), "expect.code");
            const json* c_abi = e.value("c_abi");
            if (c_abi == nullptr) {
                throw CaseError("missing expect.c_abi");
            }
            if (!c_abi->is_number_integer() ||
                (c_abi->is_number_unsigned() &&
                 c_abi->get<std::uint64_t>() >
                     static_cast<std::uint64_t>(std::numeric_limits<std::int32_t>::max()))) {
                throw CaseError("expect.c_abi: expected a 32-bit integer, got " + c_abi->dump());
            }
            const auto value = c_abi->get<std::int64_t>();
            if (value < std::numeric_limits<std::int32_t>::min()) {
                throw CaseError("expect.c_abi: expected a 32-bit integer, got " + c_abi->dump());
            }
            return ErrorCode{.code = code, .c_abi = static_cast<std::int32_t>(value)};
        }),
    };
}

LayoutRange expected_range(CaseReader& r, const std::string& name) {
    const json* v = r.value(name);
    const std::string what = "expect." + name;
    if (v == nullptr) {
        throw CaseError("missing " + what);
    }
    if (!v->is_array() || v->size() != 2) {
        throw CaseError(what + ": expected [start, end], got " + v->dump());
    }
    return LayoutRange{
        .start = to_unsigned<std::size_t>(&v->at(0), what + "[0]"),
        .end = to_unsigned<std::size_t>(&v->at(1), what + "[1]"),
    };
}

FrameLayoutCase parse_frame_layout(CaseReader& r) {
    const auto size = [&r](const std::string& name) {
        return to_unsigned<std::size_t>(r.param(name), "params." + name);
    };
    return FrameLayoutCase{
        .frame_len = size("frame_len"),
        .primary_header_len = size("primary_header_len"),
        .secondary_len = size("secondary_len"),
        .security_header_len = size("security_header_len"),
        .security_trailer_len = size("security_trailer_len"),
        .ocf = to_bool(r.param("ocf"), "params.ocf"),
        .fecf = to_bool(r.param("fecf"), "params.fecf"),
        .expect = r.expect<LayoutRanges>([](CaseReader& e) {
            return LayoutRanges{
                expected_range(e, "primary_header"),
                expected_range(e, "secondary"),
                expected_range(e, "security_header"),
                expected_range(e, "data"),
                expected_range(e, "security_trailer"),
                expected_range(e, "ocf"),
                expected_range(e, "fecf"),
            };
        }),
    };
}

OidFillCase parse_oid_fill(CaseReader& r) {
    return OidFillCase{
        .octets = to_unsigned<std::size_t>(r.param("octets"), "params.octets"),
        .expect = r.expect<Octets>(
            [](CaseReader& e) { return expected_hex(e.value("output_hex"), "expect.output_hex"); }),
    };
}

OidFillSequenceCase parse_oid_fill_sequence(CaseReader& r) {
    const json* fills = r.param("fills");
    if (fills == nullptr) {
        throw CaseError("missing params.fills");
    }
    if (!fills->is_array()) {
        throw CaseError("params.fills: expected an array, got " + fills->dump());
    }
    std::vector<std::size_t> lengths;
    for (const json& n : *fills) {
        lengths.push_back(to_unsigned<std::size_t>(&n, "params.fills[]"));
    }
    const core::Result<Fills> list = Fills::make(lengths);
    if (list.ok() == nullptr) {
        throw CaseError("params.fills: more than " + std::to_string(max_fills) + " fills");
    }
    return OidFillSequenceCase{
        .fills = *list.ok(),
        .expect = r.expect<OctetsList>([](CaseReader& e) {
            const json* outputs = e.value("outputs_hex");
            if (outputs == nullptr) {
                throw CaseError("missing expect.outputs_hex");
            }
            if (!outputs->is_array()) {
                throw CaseError("expect.outputs_hex: expected an array, got " + outputs->dump());
            }
            std::vector<Octets> items;
            for (const json& item : *outputs) {
                items.push_back(expected_hex(&item, "expect.outputs_hex[]"));
            }
            const core::Result<OctetsList> out = OctetsList::make(items);
            if (out.ok() == nullptr) {
                throw CaseError("expect.outputs_hex: more than " + std::to_string(max_fills) +
                                " outputs");
            }
            return *out.ok();
        }),
    };
}

/// Parses a live case with `parse` and appends it to `cases`.
template <typename Case>
void add(std::vector<HostCase<Case>>& cases, const RawCase& raw, Case (*parse)(CaseReader&)) {
    CaseReader reader(raw);
    Case data = parse(reader);
    reader.finish();
    cases.push_back(HostCase<Case>{.id = raw.id, .requirements = raw.requirements, .data = data});
}

/// Parses a live case by its operation (TV-012: an unknown operation is rejected).
void add_case(CoreCases& cases, const RawCase& raw) {
    const std::string& op = raw.operation;
    if (op == "bits_get") {
        add(cases.bits_get, raw, parse_bits_get);
    } else if (op == "bits_set") {
        add(cases.bits_set, raw, parse_bits_set);
    } else if (op == "counter_classify") {
        add(cases.counter_classify, raw, parse_counter_classify);
    } else if (op == "counter_increment") {
        add(cases.counter_increment, raw, parse_counter_increment);
    } else if (op == "crc16") {
        add(cases.crc16, raw, parse_crc16);
    } else if (op == "error_code") {
        add(cases.error_code, raw, parse_error_code);
    } else if (op == "frame_layout") {
        add(cases.frame_layout, raw, parse_frame_layout);
    } else if (op == "oid_fill") {
        add(cases.oid_fill, raw, parse_oid_fill);
    } else if (op == "oid_fill_sequence") {
        add(cases.oid_fill_sequence, raw, parse_oid_fill_sequence);
    } else {
        throw CaseError("operation \"" + op + "\" is not supported by the C++ runner (TV-012)");
    }
}

json parse_json(std::string_view text) {
    try {
        return json::parse(text);
    } catch (const json::parse_error& e) {
        throw LoadError(std::string("invalid JSON: ") + e.what());
    }
}

CodecFile codec_from(const json& root) {
    Members m(root, "file");
    (void)m.take("kind");
    CodecFile f;
    f.standard = schema_string(m.take("standard"), "standard");
    f.provenance = schema_provenance(m.take("provenance"), "provenance");
    f.provenance_note = schema_opt_string(m.take_opt("provenance_note"), "provenance_note");
    const json* cases = m.take("cases");
    if (cases == nullptr || !cases->is_array()) {
        throw LoadError("cases: expected an array");
    }
    schema_finish(m);
    std::set<std::string> ids;
    std::size_t index = 0;
    for (const json& value : *cases) {
        const RawCase raw = schema_case(value, "cases[" + std::to_string(index) + "]");
        ++index;
        if (!ids.insert(raw.id).second) {
            throw LoadError("case id " + raw.id + " is not unique");
        }
        if (raw.superseded) {
            ++f.superseded;
            continue;
        }
        try {
            add_case(f.cases, raw);
        } catch (const CaseError& e) {
            f.rejected.push_back(Rejected{.case_id = raw.id, .message = e.what()});
        }
    }
    return f;
}

ScenarioFile scenario_from(const json& root) {
    Members m(root, "file");
    (void)m.take("kind");
    ScenarioFile s;
    s.engine = schema_string(m.take("engine"), "engine");
    s.id = schema_string(m.take("id"), "id");
    s.requirements = schema_strings(m.take("requirements"), "requirements");
    s.description = schema_opt_string(m.take_opt("description"), "description");
    s.provenance = schema_provenance(m.take("provenance"), "provenance");
    s.provenance_note = schema_opt_string(m.take_opt("provenance_note"), "provenance_note");
    const json* config = m.take("config");
    if (config == nullptr || !config->is_object()) {
        throw LoadError("config: expected an object");
    }
    s.config = *config;
    const json* steps = m.take("steps");
    if (steps == nullptr || !steps->is_array()) {
        throw LoadError("steps: expected an array");
    }
    std::size_t index = 0;
    for (const json& value : *steps) {
        const std::string where = "steps[" + std::to_string(index) + "]";
        ++index;
        Members sm(value, where);
        Step step;
        step.at_us = schema_u64(sm.take("at_us"), where + ".at_us");
        const json* input = sm.take("input");
        if (input == nullptr || !input->is_object()) {
            throw LoadError(where + ".input: expected an object");
        }
        step.input = *input;
        const json* expect = sm.take("expect");
        if (expect == nullptr || !expect->is_array()) {
            throw LoadError(where + ".expect: expected an array of objects");
        }
        for (const json& output : *expect) {
            if (!output.is_object()) {
                throw LoadError(where + ".expect: expected an array of objects");
            }
            step.expect.push_back(output);
        }
        step.expect_state = schema_opt_string(sm.take_opt("expect_state"), where + ".expect_state");
        if (const json* t = sm.take_opt("expect_timeout_us"); t != nullptr) {
            step.expect_timeout_us = schema_u64(t, where + ".expect_timeout_us");
        }
        schema_finish(sm);
        s.steps.push_back(std::move(step));
    }
    s.superseded = schema_superseded(m.take_opt("superseded"), "file");
    schema_finish(m);
    return s;
}

} // namespace

std::size_t size(const CoreCases& cases) {
    return cases.bits_get.size() + cases.bits_set.size() + cases.counter_classify.size() +
           cases.counter_increment.size() + cases.crc16.size() + cases.error_code.size() +
           cases.frame_layout.size() + cases.oid_fill.size() + cases.oid_fill_sequence.size();
}

VectorFile parse(std::string_view text) {
    const json root = parse_json(text);
    if (!root.is_object()) {
        throw LoadError("file: expected an object");
    }
    const auto kind = root.find("kind");
    if (kind == root.end() || !kind->is_string()) {
        throw LoadError("missing string field \"kind\"");
    }
    const auto k = kind->get<std::string>();
    if (k == "codec") {
        return codec_from(root);
    }
    if (k == "engine-scenario") {
        return scenario_from(root);
    }
    throw LoadError("unknown kind \"" + k + "\"");
}

CodecFile parse_codec(std::string_view text) {
    VectorFile file = parse(text);
    if (auto* codec = std::get_if<CodecFile>(&file); codec != nullptr) {
        return std::move(*codec);
    }
    throw LoadError("expected a codec file, found an engine scenario");
}

std::string read_file(const std::filesystem::path& path) {
    const std::ifstream in(path, std::ios::binary);
    if (!in) {
        throw LoadError(path.string() + ": cannot be read");
    }
    std::ostringstream text;
    text << in.rdbuf();
    return text.str();
}

std::string hex(std::span<const std::byte> octets) {
    static constexpr std::string_view digits = "0123456789ABCDEF";
    std::string out;
    for (const std::byte b : octets) {
        const auto v = std::to_integer<unsigned>(b);
        out += digits.at(v >> 4U);
        out += digits.at(v & 0xFU);
    }
    return out;
}

} // namespace aldis::vectors
