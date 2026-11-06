// The ASM search scenarios of test-vectors/sync/scenarios/ through the handle of aldis.hpp
// (CABI-072, CABI-030, CABI-031; SYNC-007…SYNC-015, TV-010…TV-012). The mappings are those of
// crates/aldis-ffi/tests/sync_engine/mod.rs: a step builds the flat input struct, calls the
// handle, and compares the complete, ordered units (or the error with its field and `needed`)
// and the state. The handle has the capacities of capacities.toml, so the scenarios that state
// another chunk capacity are counted and skipped (the Rust runner of aldis-sync covers them).

#ifndef ALDIS_TESTS_SYNC_HPP
#define ALDIS_TESTS_SYNC_HPP

#include <cstdint>
#include <string>
#include <vector>

#include "harness.hpp"

namespace synceng {

using namespace harness;
using Handle = aldis::SyncSearch<ALDIS_SYNC_MAX_FRAME_OCTETS, ALDIS_SYNC_MAX_CHUNK_OCTETS>;

/// The output area of a step that gives none: the scenarios have frames of at most 64 octets and
/// chunks of at most 128.
constexpr std::size_t kArea = 192;

inline std::uint32_t protocol_of(const std::string& name) {
    return name == "tm"    ? ALDIS_LINK_PROTOCOL_TM
           : name == "aos" ? ALDIS_LINK_PROTOCOL_AOS
                           : ALDIS_LINK_PROTOCOL_USLP;
}

inline aldis_sync_search_config_t config_of(const Value& c) {
    aldis_sync_search_config_t cfg{};
    cfg.protocol = protocol_of(c.at("protocol").str());
    cfg.frame_len = static_cast<std::size_t>(c.at("frame_len").num());
    cfg.asm_errors = static_cast<std::uint32_t>(c.num_or("asm_errors", 0));
    return cfg;
}

inline Value error_value(aldis::Status s, const aldis::Detail& d) {
    if (static_cast<std::uint32_t>(s) != d.kind) {
        return Value::of_object().set("error", text("status and detail disagree"));
    }
    Value v = Value::of_object().set("error", text(error_name(s)));
    if (d.field == ALDIS_FIELD_CHUNK_LEN) {
        v.set("field", text("chunk_len"));
    } else if (d.field != 0) {
        v.set("field", text("field#" + std::to_string(d.field)));
    }
    if (d.detail == ALDIS_DETAIL_NEEDED) {
        v.set("needed", number(d.needed));
    }
    return v;
}

inline const char* state_name(std::uint32_t state) {
    return state == ALDIS_SYNC_SEARCH_STATE_SEARCHING    ? "Searching"
           : state == ALDIS_SYNC_SEARCH_STATE_COLLECTING ? "Collecting"
                                                          : "?";
}

/// Runs one scenario file of the engine `asm-search`.
inline Tally run(const std::string& path) {
    Tally t;
    File file;
    if (!file.load(path, "asm-search")) {
        t.failures.push_back(path + ": not loaded");
        return t;
    }
    for (const Scenario& sc : file.scenarios) {
        if (sc.superseded) {
            continue;
        }
        // Only a scenario that states a chunk capacity other than the handle's is skipped.
        if (sc.config->num_or("chunk_capacity", ALDIS_SYNC_MAX_CHUNK_OCTETS) !=
            ALDIS_SYNC_MAX_CHUNK_OCTETS) {
            ++t.skipped;
            continue;
        }
        ++t.run;
        Handle engine;
        aldis::Detail detail{};
        if (!aldis::ok(engine.init(config_of(*sc.config), &detail))) {
            t.failures.push_back(sc.def->at("id").str() + ": init failed " +
                                 tj::dump(error_value(aldis::Status{detail.kind}, detail)));
            continue;
        }
        for (std::size_t i = 0; i < sc.steps.size(); ++i) {
            const Value& step = *sc.steps[i];
            const auto& [key, v] = step.at("input").object.front();
            Value got = Value::of_array();
            if (key == "query") {
                aldis_sync_search_status_t status{};
                if (!aldis::ok(engine.query(status))) {
                    t.fail(sc, i, step.at("input"), "status failed");
                    break;
                }
                const std::string& name = v.at("counter").str();
                const std::uint64_t value = name == "units_returned" ? status.units_returned
                                            : name == "partial_octets"
                                                ? status.partial_octets
                                                : status.skipped_bits_total;
                got.push(Value::of_object().set(
                    "counter", Value::of_object().set("name", text(name)).set("value", number(value))));
            } else {
                aldis_sync_search_input_t in{};
                std::vector<std::uint8_t> data;
                std::size_t area_len = kArea;
                if (key == "chunk") {
                    in.kind = ALDIS_SYNC_SEARCH_INPUT_CHUNK;
                    data = unhex(v.at("data_hex").str());
                    area_len = static_cast<std::size_t>(v.num_or("out_len", kArea));
                } else {
                    in.kind = ALDIS_SYNC_SEARCH_INPUT_RESET;
                    area_len = 0;
                }
                std::vector<std::uint8_t> area(area_len, 0xEE);
                aldis_sync_search_actions_t actions{};
                const aldis::Status s = engine.step(in, data, area, actions, &detail);
                if (aldis::ok(s)) {
                    for (std::size_t n = 0; n < actions.count; ++n) {
                        const auto& a = actions.items[n];
                        got.push(Value::of_object().set(
                            "unit", Value::of_object()
                                        .set("data_hex", text(hex(area.data() + a.unit.offset, a.unit.len)))
                                        .set("errors", number(a.errors))
                                        .set("skipped_bits", number(a.skipped_bits))));
                    }
                } else {
                    got.push(error_value(s, detail));
                }
            }
            std::string problem = compare_outputs(got, step);
            if (step.has("expect_state")) {
                aldis_sync_search_status_t status{};
                if (!aldis::ok(engine.query(status)) ||
                    !step.at("expect_state").is(state_name(status.state))) {
                    problem += "; state " + std::string(state_name(status.state));
                }
            }
            if (!problem.empty()) {
                t.fail(sc, i, step.at("input"), problem);
                break;
            }
        }
    }
    // Only search_capacity.json skips (its one scenario states a chunk capacity of 16); a file
    // that skips otherwise, or that skips nothing there, fails, so nothing passes unrun.
    const bool capacity_file = path.find("search_capacity") != std::string::npos;
    if ((t.skipped != 0) != capacity_file) {
        t.failures.push_back(path + ": unexpected number of skipped scenarios: " +
                             std::to_string(t.skipped));
    }
    return t;
}

}  // namespace synceng

#endif  // ALDIS_TESTS_SYNC_HPP
