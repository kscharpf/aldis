// Self-tests of the vector loader (docs/milestones/c0-foundations.md D5, WP-4): the schema rules,
// the exact match of TV-012 per operation, the optional field code of M2 plan D3, superseded
// cases, and engine scenarios. A corrupted, an incomplete, and an over-specified vector each fail.

#include "aldis/core/error.hpp"
#include "aldis/vectors/loader.hpp"
#include "aldis/vectors/runner.hpp"

#include <gtest/gtest.h>

#include <cstdint>
#include <optional>
#include <string>
#include <variant>

namespace {

using aldis::core::ErrorKind;
using aldis::vectors::CodecFile;
using aldis::vectors::LoadError;
using aldis::vectors::parse;
using aldis::vectors::parse_codec;

/// A codec file holding `cases` (a JSON array body).
std::string file(const std::string& cases) {
    return R"({ "kind": "codec", "standard": "test", "provenance": "hand-derived",
                "provenance_note": "unit test", "cases": [)" +
           cases + "] }";
}

/// One case of `operation` with the given extra members (JSON object members, comma-first).
std::string one(const std::string& operation, const std::string& members) {
    return R"({ "id": "c1", "requirements": ["TV-012"], "operation": ")" + operation + "\"" +
           members + " }";
}

/// The message of the single rejected case of `text`.
std::string rejection(const std::string& text) {
    const CodecFile f = parse_codec(text);
    if (f.rejected.size() != 1) {
        return "not rejected";
    }
    return f.rejected.front().message;
}

const std::string bits_get_params =
    R"(, "input_hex": "0001", "params": { "bit_offset": 5, "width": 11 })";

TEST(VectorLoader, AcceptsAValidCase) {
    // req: TV-001, CPP-053
    const CodecFile f = parse_codec(
        file(one("bits_get", bits_get_params + R"(, "expect": { "ok": true, "value": 1 })")));
    ASSERT_EQ(f.cases.bits_get.size(), 1U);
    EXPECT_TRUE(f.rejected.empty());
    EXPECT_EQ(f.cases.bits_get.front().id, "c1");
    EXPECT_EQ(f.cases.bits_get.front().data.width, 11U);
    EXPECT_EQ(aldis::vectors::size(f.cases), 1U);
    EXPECT_TRUE(aldis::vectors::passed(aldis::vectors::run(f)));
}

TEST(VectorLoader, SchemaViolationsFailTheFile) {
    // req: TV-001
    EXPECT_THROW((void)parse("not json"), LoadError);
    EXPECT_THROW((void)parse("[]"), LoadError);
    EXPECT_THROW((void)parse(R"({ "kind": "bogus" })"), LoadError);
    EXPECT_THROW((void)parse(R"({ "standard": "x" })"), LoadError);
    // Closed objects: file, case, superseded.
    EXPECT_THROW((void)parse(R"({ "kind": "codec", "standard": "t", "provenance": "hand-derived",
                                  "cases": [], "typo_field": 1 })"),
                 LoadError);
    EXPECT_THROW((void)parse(file(one("crc16", R"(, "expect": { "ok": true }, "inputhex": "00")"))),
                 LoadError);
    EXPECT_THROW((void)parse(file(one("crc16", R"(, "expect": { "ok": true },
                                      "superseded": { "reason": "r", "extra": 1 })"))),
                 LoadError);
    // Types and enumerations.
    EXPECT_THROW((void)parse(R"({ "kind": "codec", "standard": "t", "provenance": "guessed",
                                  "cases": [] })"),
                 LoadError);
    EXPECT_THROW((void)parse(file(one("crc16", R"(, "mode": "sloppy", "expect": { "ok": true })"))),
                 LoadError);
    EXPECT_THROW((void)parse(file(one("crc16", R"(, "expect": { "ok": "yes" })"))), LoadError);
    EXPECT_THROW((void)parse(file(one("crc16", R"(, "expect": [])"))), LoadError);
    EXPECT_THROW((void)parse(file(one("crc16", R"(, "params": [], "expect": { "ok": true })"))),
                 LoadError);
    EXPECT_THROW(
        (void)parse(R"({ "kind": "codec", "standard": "t", "provenance": "hand-derived" })"),
        LoadError);
    // Identifiers are unique.
    const std::string c = one("crc16", R"(, "expect": { "ok": true, "value_hex": "FFFF" })");
    EXPECT_THROW((void)parse(file(c + ", " + c)), LoadError);
}

TEST(VectorLoader, NullMeansAbsentWhereTheSchemaAllowsIt) {
    // req: TV-001
    const CodecFile f = parse_codec(
        file(one("crc16", R"(, "description": null, "mode": null, "config": null, "params": null,
                     "input_hex": null, "superseded": null,
                     "expect": { "ok": true, "error": null, "value_hex": "FFFF" })")));
    EXPECT_EQ(f.cases.crc16.size(), 1U);
    EXPECT_TRUE(f.rejected.empty());
    EXPECT_TRUE(aldis::vectors::passed(aldis::vectors::run(f)));
}

TEST(VectorLoader, IncompleteExpectationIsRejected) {
    // req: TV-002, TV-012
    EXPECT_NE(rejection(file(one("bits_get", bits_get_params + R"(, "expect": { "ok": true })")))
                  .find("missing expect.value"),
              std::string::npos);
    // A gap names the counts missing.
    EXPECT_NE(rejection(file(one("counter_classify",
                                 R"(, "params": { "bits": 8, "prev": 1, "next": 4 },
                                    "expect": { "ok": true, "class": "gap" })")))
                  .find("missing expect.missing"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": false })"))).find("no error kind"),
              std::string::npos);
}

TEST(VectorLoader, OverSpecifiedExpectationIsRejected) {
    // req: TV-002, TV-012
    EXPECT_NE(rejection(file(one("bits_get",
                                 bits_get_params +
                                     R"(, "expect": { "ok": true, "value": 1, "extra": 0 })")))
                  .find("over-specified expectation: extra"),
              std::string::npos);
    // Only a gap produces `missing`.
    EXPECT_NE(rejection(file(one("counter_classify",
                                 R"(, "params": { "bits": 8, "prev": 1, "next": 2 },
                                    "expect": { "ok": true, "class": "in-sequence", "missing": 0 })")))
                  .find("over-specified"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": true, "value_hex": "FFFF",
                                                            "error": "Malformed" })")))
                  .find("an error is named"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": false, "error": "Malformed",
                                                            "needed": 2 })")))
                  .find("only error and field"),
              std::string::npos);
}

TEST(VectorLoader, UnusedPartsOfACaseAreRejected) {
    // req: TV-001, TV-012
    EXPECT_NE(
        rejection(file(one("frobnicate", R"(, "expect": { "ok": true })"))).find("not supported"),
        std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "params": { "x": 1 }, "expect": { "ok": true,
                                                            "value_hex": "FFFF" })")))
                  .find("not used by crc16: x"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "config": { "x": 1 }, "expect": { "ok": true,
                                                            "value_hex": "FFFF" })")))
                  .find("config"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "mode": "lenient", "expect": { "ok": true,
                                                            "value_hex": "FFFF" })")))
                  .find("mode lenient"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("counter_increment", R"(, "input_hex": "00",
                                 "params": { "bits": 8, "value": 1 },
                                 "expect": { "ok": true, "value": 2 })")))
                  .find("input_hex is not used"),
              std::string::npos);
    // An empty config and an explicit strict mode are what the core operations assume.
    const CodecFile f = parse_codec(file(one("crc16", R"(, "config": {}, "mode": "strict",
                                                          "expect": { "ok": true, "value_hex": "FFFF" })")));
    EXPECT_TRUE(f.rejected.empty());
}

TEST(VectorLoader, ValuesNeedTheirTypeAndCanonicalForm) {
    // req: TV-001, TV-012
    // Expected hex is compared as the upper-case string the Rust handler produces.
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": true, "value_hex": "ffff" })")))
                  .find("canonical"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": true, "value_hex": "FFFFFF" })")))
                  .find("four hex digits"),
              std::string::npos);
    // Input hex may use either case.
    EXPECT_TRUE(parse_codec(file(one("crc16", R"(, "input_hex": "ab", "expect": { "ok": true,
                                                                      "value_hex": "E571" })")))
                    .rejected.empty());
    EXPECT_NE(rejection(file(one("crc16", R"(, "input_hex": "abc", "expect": { "ok": true,
                                                                      "value_hex": "FFFF" })")))
                  .find("odd number"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "input_hex": "zz", "expect": { "ok": true,
                                                                     "value_hex": "FFFF" })")))
                  .find("invalid hex"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("bits_get", R"(, "params": { "bit_offset": 0, "width": 256 },
                                 "expect": { "ok": true, "value": 0 })")))
                  .find("does not fit"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("bits_get", R"(, "params": { "bit_offset": -1, "width": 1 },
                                 "expect": { "ok": true, "value": 0 })")))
                  .find("non-negative integer"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("bits_get", R"(, "params": { "bit_offset": 1.0, "width": 1 },
                                 "expect": { "ok": true, "value": 0 })")))
                  .find("non-negative integer"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("frame_layout", R"(, "params": { "frame_len": 8,
                                 "primary_header_len": 6, "secondary_len": 0,
                                 "security_header_len": 0, "security_trailer_len": 0,
                                 "ocf": 0, "fecf": false }, "expect": { "ok": false,
                                 "error": "InvalidConfig" })")))
                  .find("expected a boolean"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("error_code", R"(, "params": { "kind": "Malformed" },
                                 "expect": { "ok": true, "code": 17, "c_abi": 4294967296 })")))
                  .find("32-bit"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("counter_classify", R"(, "params": { "bits": 8, "prev": 1,
                                 "next": 2 }, "expect": { "ok": true, "class": "sideways" })")))
                  .find("unknown class"),
              std::string::npos);
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": false, "error": "Oops" })")))
                  .find("not in the catalogue"),
              std::string::npos);
}

TEST(VectorLoader, CapacitiesAreChecked) {
    // req: TV-001
    const std::string long_hex(2 * (aldis::vectors::max_octets + 1), 'A');
    EXPECT_NE(
        rejection(file(one("crc16", R"(, "input_hex": ")" + long_hex +
                                        R"(", "expect": { "ok": true, "value_hex": "FFFF" })")))
            .find("max_octets"),
        std::string::npos);
    EXPECT_NE(rejection(file(one("oid_fill_sequence",
                                 R"(, "params": { "fills": [1, 1, 1, 1, 1, 1, 1, 1, 1] },
                                    "expect": { "ok": true, "outputs_hex": [] })")))
                  .find("fills"),
              std::string::npos);
}

TEST(VectorLoader, FieldCodeIsKeptAndNotCompared) {
    // req: TV-001, TV-012
    // M2 plan D3: an error expectation may give the code of the field it names (1…65535). C0
    // keeps the code for C1, when FieldId gains its codes, and compares the kind only.
    const CodecFile f = parse_codec(
        file(one("bits_get", R"(, "input_hex": "0000", "params": { "bit_offset": 9, "width": 8 },
                       "expect": { "ok": false, "error": "OutOfRange", "field": 4 })")));
    ASSERT_EQ(f.cases.bits_get.size(), 1U);
    const aldis::vectors::ExpectedError* e = f.cases.bits_get.front().data.expect.error();
    ASSERT_NE(e, nullptr);
    EXPECT_EQ(e->kind(), ErrorKind::OutOfRange);
    EXPECT_EQ(e->field(), std::optional<std::uint16_t>{4});
    EXPECT_TRUE(aldis::vectors::passed(aldis::vectors::run(f)));
    // Absent or null: no code.
    const CodecFile g = parse_codec(
        file(one("bits_get", R"(, "input_hex": "0000", "params": { "bit_offset": 9, "width": 8 },
                       "expect": { "ok": false, "error": "OutOfRange", "field": null })")));
    ASSERT_EQ(g.cases.bits_get.size(), 1U);
    const aldis::vectors::ExpectedError* ge = g.cases.bits_get.front().data.expect.error();
    ASSERT_NE(ge, nullptr);
    EXPECT_EQ(ge->field(), std::nullopt);
    // Outside 1…65535, or not an integer: outside the schema (NonZeroU16).
    for (const char* bad : {"0", "65536", "-1", "1.5", "\"4\""}) {
        EXPECT_THROW((void)parse(file(one("crc16", std::string(R"(, "expect": { "ok": false,
                                          "error": "Malformed", "field": )") +
                                                       bad + " })"))),
                     LoadError)
            << bad;
    }
    // Only an error names a field.
    EXPECT_NE(rejection(file(one("crc16", R"(, "expect": { "ok": true, "value_hex": "FFFF",
                                                            "field": 4 })")))
                  .find("field code"),
              std::string::npos);
}

TEST(VectorLoader, SupersededCasesAreCountedAndNotRun) {
    // req: TV-021
    const CodecFile f = parse_codec(file(
        R"({ "id": "old", "requirements": ["TV-021"], "operation": "no_longer_exists",
             "expect": { "ok": true }, "superseded": { "reason": "wrong", "by": "new" } },
           { "id": "new", "requirements": ["TV-021"], "operation": "crc16",
             "expect": { "ok": true, "value_hex": "FFFF" } })"));
    EXPECT_EQ(f.superseded, 1U);
    EXPECT_EQ(aldis::vectors::size(f.cases), 1U);
    const aldis::vectors::Report r = aldis::vectors::run(f);
    EXPECT_EQ(r.passed, 1U);
    EXPECT_EQ(r.superseded, 1U);
    EXPECT_TRUE(r.failures.empty());
    // A superseded case is still checked against the schema.
    EXPECT_THROW((void)parse(file(R"({ "id": "old", "requirements": [], "operation": "x",
                                       "expect": { "ok": true }, "superseded": {} })")),
                 LoadError);
}

TEST(VectorLoader, EngineScenarios) {
    // req: TV-010, TV-012
    const std::string scenario = R"({ "kind": "engine-scenario", "engine": "fop1", "id": "s1",
        "requirements": ["TV-010"], "provenance": "hand-derived", "provenance_note": "test",
        "config": { "k": 8 },
        "steps": [ { "at_us": 0, "input": { "directive": "initiate" },
                     "expect": [ { "event": "accept" } ], "expect_state": "S1" },
                   { "at_us": 10, "input": { "request_ad": "AA" }, "expect": [],
                     "expect_timeout_us": 2000010 } ] })";
    const aldis::vectors::VectorFile v = parse(scenario);
    const auto* s = std::get_if<aldis::vectors::ScenarioFile>(&v);
    ASSERT_NE(s, nullptr);
    EXPECT_EQ(s->engine, "fop1");
    ASSERT_EQ(s->steps.size(), 2U);
    EXPECT_EQ(s->steps.front().expect.size(), 1U);
    EXPECT_EQ(s->steps.front().expect_state, std::optional<std::string>{"S1"});
    EXPECT_EQ(s->steps.back().at_us, 10U);
    EXPECT_EQ(s->steps.back().expect_timeout_us, std::optional<std::uint64_t>{2000010});
    EXPECT_FALSE(s->superseded.has_value());
    // Not a codec file.
    EXPECT_THROW((void)parse_codec(scenario), LoadError);
    // Closed objects and required members.
    EXPECT_THROW((void)parse(R"({ "kind": "engine-scenario", "engine": "e", "id": "s",
        "requirements": [], "provenance": "hand-derived", "config": {}, "steps": [],
        "extra": 1 })"),
                 LoadError);
    EXPECT_THROW((void)parse(R"({ "kind": "engine-scenario", "engine": "e", "id": "s",
        "requirements": [], "provenance": "hand-derived", "steps": [] })"),
                 LoadError);
    EXPECT_THROW((void)parse(R"({ "kind": "engine-scenario", "engine": "e", "id": "s",
        "requirements": [], "provenance": "hand-derived", "config": {},
        "steps": [ { "at_us": 0, "input": {}, "expect": [], "expect_events": [] } ] })"),
                 LoadError);
    EXPECT_THROW((void)parse(R"({ "kind": "engine-scenario", "engine": "e", "id": "s",
        "requirements": [], "provenance": "hand-derived", "config": {},
        "steps": [ { "at_us": -5, "input": {}, "expect": [] } ] })"),
                 LoadError);
    EXPECT_THROW((void)parse(R"({ "kind": "engine-scenario", "engine": "e", "id": "s",
        "requirements": [], "provenance": "hand-derived", "config": {},
        "steps": [ { "at_us": 0, "input": {}, "expect": [ 1 ] } ] })"),
                 LoadError);
}

} // namespace
