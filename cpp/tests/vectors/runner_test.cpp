// Self-tests of the host runner, the check functions, and the code generator
// (docs/milestones/c0-foundations.md D5, WP-4): every live core case passes, any difference fails
// with the case identifier (TV-012), and the generated constexpr cases are the loaded ones.

#include "aldis/core/counter.hpp"
#include "aldis/core/error.hpp"
#include "aldis/vectors/case.hpp"
#include "aldis/vectors/checks/bits_get.hpp"
#include "aldis/vectors/checks/counter_classify.hpp"
#include "aldis/vectors/checks/crc16.hpp"
#include "aldis/vectors/checks/error_code.hpp"
#include "aldis/vectors/codegen.hpp"
#include "aldis/vectors/generated/core.hpp"
#include "aldis/vectors/loader.hpp"
#include "aldis/vectors/runner.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>
#include <utility>
#include <vector>

#ifndef ALDIS_VECTORS_DIR
#error "ALDIS_VECTORS_DIR must name the test-vectors directory"
#endif

namespace {

namespace gen = aldis::vectors::generated;
using aldis::core::Continuity;
using aldis::core::ErrorKind;
using aldis::vectors::CodecFile;
using aldis::vectors::Expect;
using aldis::vectors::ExpectedError;
using aldis::vectors::Mismatch;

const std::vector<std::string> core_files{"core/bits.json",   "core/counters.json",
                                          "core/crc16.json",  "core/errors.json",
                                          "core/layout.json", "core/oid.json"};

CodecFile load(const std::string& name) {
    return aldis::vectors::parse_codec(
        aldis::vectors::read_file(std::string(ALDIS_VECTORS_DIR) + "/" + name));
}

TEST(VectorRunner, EveryLiveCoreCasePasses) {
    // req: CPP-053, VER-021, TV-012, TV-021
    std::size_t passed = 0;
    std::size_t superseded = 0;
    for (const std::string& name : core_files) {
        const aldis::vectors::Report r = aldis::vectors::run(load(name));
        EXPECT_TRUE(aldis::vectors::passed(r)) << aldis::vectors::summary(r, name);
        passed += r.passed;
        superseded += r.superseded;
    }
    // test-vectors/README.md and the c0 plan: 77 live cases, 6 superseded.
    EXPECT_EQ(passed, 77U);
    EXPECT_EQ(superseded, 6U);
}

TEST(VectorRunner, CorruptedVectorFailsNamingTheCase) {
    // req: TV-012, CPP-053
    std::string text =
        aldis::vectors::read_file(std::string(ALDIS_VECTORS_DIR) + "/core/crc16.json");
    const std::string from = R"("value_hex": "29B1" }
    },
    {
      "id": "crc16-empty-002")";
    const std::size_t at = text.find(from);
    ASSERT_NE(at, std::string::npos);
    const std::string value = R"("value_hex": "29B1")";
    text.replace(at, value.size(), R"("value_hex": "0000")");
    const aldis::vectors::Report r = aldis::vectors::run(aldis::vectors::parse_codec(text));
    ASSERT_EQ(r.failures.size(), 1U);
    EXPECT_EQ(r.failures.front().case_id, "crc16-check-002");
    EXPECT_EQ(r.failures.front().message, R"(expected success "0000", got success "29B1")");
    EXPECT_FALSE(aldis::vectors::passed(r));
    EXPECT_NE(aldis::vectors::summary(r, "crc16").find("1 failed"), std::string::npos);
}

TEST(VectorRunner, RejectedCasesAndEmptyFilesFail) {
    // req: TV-012
    const CodecFile f = aldis::vectors::parse_codec(
        R"({ "kind": "codec", "standard": "t", "provenance": "hand-derived", "cases": [
             { "id": "u", "requirements": [], "operation": "unknown", "expect": { "ok": true } } ] })");
    const aldis::vectors::Report r = aldis::vectors::run(f);
    ASSERT_EQ(r.failures.size(), 1U);
    EXPECT_EQ(r.failures.front().case_id, "u");
    EXPECT_FALSE(aldis::vectors::passed(r));
    const aldis::vectors::Report empty = aldis::vectors::run(aldis::vectors::parse_codec(
        R"({ "kind": "codec", "standard": "t", "provenance": "hand-derived", "cases": [] })"));
    EXPECT_FALSE(aldis::vectors::passed(empty));
    EXPECT_NE(aldis::vectors::summary(empty, "e").find("no runnable cases"), std::string::npos);
}

TEST(VectorChecks, EveryDifferenceFails) {
    // req: TV-012
    const auto input = aldis::vectors::octets(std::array<std::uint8_t, 2>{0x00U, 0x01U});
    using aldis::vectors::BitsGetCase;
    const auto verdict = [&](std::size_t offset, std::uint8_t width, Expect<std::uint64_t> e) {
        return check(
            BitsGetCase{.input = input, .bit_offset = offset, .width = width, .expect = e});
    };
    EXPECT_TRUE(verdict(5, 11, Expect<std::uint64_t>::success(1)).passed());
    EXPECT_EQ(verdict(5, 11, Expect<std::uint64_t>::success(2)).mismatch(), Mismatch::WrongValue);
    const auto expected_success = verdict(9, 8, Expect<std::uint64_t>::success(0));
    EXPECT_EQ(expected_success.mismatch(), Mismatch::ExpectedSuccess);
    EXPECT_EQ(expected_success.got(), ErrorKind::OutOfRange);
    EXPECT_EQ(verdict(5, 11, Expect<std::uint64_t>::failure(ExpectedError{ErrorKind::OutOfRange}))
                  .mismatch(),
              Mismatch::ExpectedError);
    const auto wrong =
        verdict(9, 8, Expect<std::uint64_t>::failure(ExpectedError{ErrorKind::Malformed}));
    EXPECT_EQ(wrong.mismatch(), Mismatch::WrongError);
    EXPECT_EQ(wrong.got(), ErrorKind::OutOfRange);
    // The field code is not compared in C0.
    EXPECT_TRUE(verdict(9, 8,
                        Expect<std::uint64_t>::failure(
                            ExpectedError{ErrorKind::OutOfRange, std::uint16_t{999}}))
                    .passed());
    // A gap compares its count.
    using aldis::vectors::CounterClassifyCase;
    EXPECT_TRUE(
        check(CounterClassifyCase{.bits = 8,
                                  .prev = 1,
                                  .next = 4,
                                  .expect = Expect<Continuity>::success(Continuity::gap(2))})
            .passed());
    EXPECT_EQ(check(CounterClassifyCase{.bits = 8,
                                        .prev = 1,
                                        .next = 4,
                                        .expect = Expect<Continuity>::success(Continuity::gap(3))})
                  .mismatch(),
              Mismatch::WrongValue);
    // An unknown name is not an error kind.
    using aldis::vectors::ErrorCode;
    using aldis::vectors::ErrorCodeCase;
    EXPECT_EQ(check(ErrorCodeCase{
                        .kind = aldis::vectors::name("NoSuchKind"),
                        .expect = Expect<ErrorCode>::success(ErrorCode{.code = 1, .c_abi = -1})})
                  .mismatch(),
              Mismatch::UnknownName);
    // CRC forms agree on the check string.
    using aldis::vectors::Crc16Case;
    EXPECT_TRUE(check(Crc16Case{.input = aldis::vectors::octets(std::array<std::uint8_t, 1>{0xABU}),
                                .expect = Expect<std::uint16_t>::success(0xE571U)})
                    .passed());
}

TEST(VectorCodegen, GeneratedCasesAreTheLoadedCases) {
    // req: CPP-053, CPP-034
    std::vector<std::string> loaded_ids;
    std::size_t loaded = 0;
    for (const std::string& name : core_files) {
        const CodecFile f = load(name);
        loaded += aldis::vectors::size(f.cases);
        for (const auto& c : f.cases.bits_get) {
            loaded_ids.push_back(c.id);
        }
    }
    EXPECT_EQ(gen::live_cases, loaded);
    EXPECT_EQ(gen::superseded_cases, 6U);
    std::vector<std::string> generated_ids;
    for (const auto& c : gen::bits_get) {
        generated_ids.emplace_back(c.id.begin(), c.id.end());
    }
    EXPECT_EQ(generated_ids, loaded_ids);
    const std::size_t total =
        gen::bits_get.size() + gen::bits_set.size() + gen::counter_classify.size() +
        gen::counter_increment.size() + gen::crc16.size() + gen::error_code.size() +
        gen::frame_layout.size() + gen::oid_fill.size() + gen::oid_fill_sequence.size();
    EXPECT_EQ(total, gen::live_cases);
}

template <typename Case, std::size_t N>
std::vector<std::string> failing(const std::array<aldis::vectors::Named<Case>, N>& cases) {
    std::vector<std::string> out;
    for (const auto& c : cases) {
        if (!check(c.data).passed()) {
            out.emplace_back(c.id.begin(), c.id.end());
        }
    }
    return out;
}

TEST(VectorCodegen, EveryGeneratedCasePassesAtRunTime) {
    // req: CPP-053
    EXPECT_EQ(failing(gen::bits_get), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::bits_set), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::counter_classify), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::counter_increment), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::crc16), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::error_code), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::frame_layout), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::oid_fill), std::vector<std::string>{});
    EXPECT_EQ(failing(gen::oid_fill_sequence), std::vector<std::string>{});
}

TEST(VectorCodegen, HeaderTextAndRejection) {
    // req: CPP-053
    const CodecFile f = aldis::vectors::parse_codec(
        R"({ "kind": "codec", "standard": "t", "provenance": "hand-derived", "cases": [
             { "id": "q\"\\\u0001", "requirements": [], "operation": "bits_get",
               "input_hex": "0001", "params": { "bit_offset": 9, "width": 8 },
               "expect": { "ok": false, "error": "OutOfRange", "field": 7 } } ] })");
    const std::string header = aldis::vectors::generate_header({{"t.json", f}});
    EXPECT_NE(header.find(R"(literal("q\"\\\001"))"), std::string::npos) << header;
    EXPECT_NE(header.find("ExpectedError{core::ErrorKind::OutOfRange, std::uint16_t{7U}}"),
              std::string::npos)
        << header;
    EXPECT_NE(header.find("inline constexpr std::size_t live_cases = 1U;"), std::string::npos);
    EXPECT_NE(header.find("std::array<Named<Crc16Case>, 0> crc16{};"), std::string::npos);
    const CodecFile bad = aldis::vectors::parse_codec(
        R"({ "kind": "codec", "standard": "t", "provenance": "hand-derived", "cases": [
             { "id": "u", "requirements": [], "operation": "unknown", "expect": { "ok": true } } ] })");
    EXPECT_THROW((void)aldis::vectors::generate_header({{"bad.json", bad}}),
                 aldis::vectors::LoadError);
}

} // namespace
