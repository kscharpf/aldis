// The COP-1 scenarios of test-vectors/cop1/scenarios/ through aldis::Fop1 and aldis::Farm1
// (CABI-072, CABI-030, CABI-031; COP-001, COP-002, COP-005, COP-006, TV-010…TV-012). The input
// mapping is that of crates/aldis-ffi/tests/engine/mod.rs: a step builds the flat input struct,
// calls the handle, and compares the complete, ordered actions, the state, and the deadline.
// The scenarios whose trigger is a `table_event` feed an event number to the table, a
// conformance hook the C ABI does not expose (CABI-040): they are counted and skipped.

#ifndef ALDIS_TESTS_COP1_HPP
#define ALDIS_TESTS_COP1_HPP

#include <array>
#include <cstdint>
#include <string>
#include <vector>

#include "harness.hpp"

namespace cop1 {

using namespace harness;

using Fop1 = aldis::Fop1<ALDIS_FOP1_SENT_QUEUE_FRAMES, ALDIS_FOP1_MAX_FRAME_OCTETS>;
using aldis::Farm1;

constexpr std::size_t kFrame = 64;

inline std::uint8_t byte(const Value& v) { return static_cast<std::uint8_t>(v.num()); }

inline const char* notice(std::uint32_t s) {
    switch (s) {
    case ALDIS_FOP1_NOTICE_ACCEPT: return "accept";
    case ALDIS_FOP1_NOTICE_REJECT: return "reject";
    case ALDIS_FOP1_NOTICE_POSITIVE_CONFIRM: return "positive_confirm";
    case ALDIS_FOP1_NOTICE_NEGATIVE_CONFIRM: return "negative_confirm";
    default: return "?";
    }
}

inline const char* frame_name(std::uint32_t k) {
    switch (k) {
    case ALDIS_FOP1_FRAME_AD: return "AD";
    case ALDIS_FOP1_FRAME_BC: return "BC";
    case ALDIS_FOP1_FRAME_BD: return "BD";
    default: return "?";
    }
}

inline const char* alert_name(std::uint32_t r) {
    switch (r) {
    case ALDIS_FOP1_ALERT_LIMIT: return "limit";
    case ALDIS_FOP1_ALERT_T1: return "t1";
    case ALDIS_FOP1_ALERT_LOCKOUT: return "lockout";
    case ALDIS_FOP1_ALERT_SYNCH: return "synch";
    case ALDIS_FOP1_ALERT_NN_R: return "nn_r";
    case ALDIS_FOP1_ALERT_CLCW: return "clcw";
    case ALDIS_FOP1_ALERT_LLIF: return "llif";
    case ALDIS_FOP1_ALERT_TERM: return "term";
    default: return "?";
    }
}

inline Value error_output(aldis::Status s, const aldis::Detail& d) {
    // The status and the detail name the same error (CABI-020, CABI-021).
    if (static_cast<std::uint32_t>(s) != d.kind) {
        return Value::of_object().set("error", text("status and detail disagree"));
    }
    return Value::of_object().set("error", text(error_name(s)));
}

// ---- FOP-1

inline aldis_fop1_input_t fop1_blank() {
    aldis_fop1_input_t in{};
    in.kind = ALDIS_FOP1_INPUT_TIMEOUT;
    return in;
}

inline aldis_fop1_input_t fop1_directive(const Value& v) {
    aldis_fop1_input_t in = fop1_blank();
    in.handle = static_cast<std::uint32_t>(v.at("handle").num());
    const std::string& type = v.at("type").str();
    struct Row {
        const char* name;
        std::uint32_t code;
        bool valued;
    };
    const std::array<Row, 11> rows{{
        {"initiate_ad_without_clcw_check", ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK, false},
        {"initiate_ad_with_clcw_check", ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_CLCW_CHECK, false},
        {"initiate_ad_with_unlock", ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_UNLOCK, false},
        {"initiate_ad_with_set_vr", ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_SET_VR, true},
        {"terminate_ad", ALDIS_FOP1_DIRECTIVE_TERMINATE_AD, false},
        {"resume_ad", ALDIS_FOP1_DIRECTIVE_RESUME_AD, false},
        {"set_v_s", ALDIS_FOP1_DIRECTIVE_SET_VS, true},
        {"set_k", ALDIS_FOP1_DIRECTIVE_SET_K, true},
        {"set_t1_initial", ALDIS_FOP1_DIRECTIVE_SET_T1_INITIAL, true},
        {"set_transmission_limit", ALDIS_FOP1_DIRECTIVE_SET_TRANSMISSION_LIMIT, true},
        {"set_timeout_type", ALDIS_FOP1_DIRECTIVE_SET_TIMEOUT_TYPE, true},
    }};
    for (const Row& row : rows) {
        if (type == row.name) {
            if (row.valued && !v.has("value")) {
                break;
            }
            in.kind = ALDIS_FOP1_INPUT_DIRECTIVE;
            in.directive = row.code;
            in.value = v.num_or("value", 0);
            return in;
        }
    }
    // A directive the caller cannot parse is E40.
    in.kind = ALDIS_FOP1_INPUT_INVALID_DIRECTIVE;
    return in;
}

/// The C input of the step's input (one key); false for the `table_event` hook. `data` holds the
/// octets the input borrows.
inline bool fop1_input(const Value& input, std::uint8_t vcid, aldis_fop1_input_t& out,
                       std::vector<std::uint8_t>& data) {
    const auto& [key, v] = input.object.front();
    out = fop1_blank();
    if (key == "clcw") {
        const std::uint8_t b0 = static_cast<std::uint8_t>(
            v.num_or("cop_in_effect", 1) | (v.num_or("control_word_type", 0) << 7));
        const std::uint8_t b1 = static_cast<std::uint8_t>(v.num_or("vcid", vcid) << 2);
        const std::uint8_t b2 = static_cast<std::uint8_t>(
            (v.at("lockout").flag() << 5) | (v.at("wait").flag() << 4) |
            (v.at("retransmit").flag() << 3));
        const std::uint8_t b3 = byte(v.at("n_r"));
        out.kind = ALDIS_FOP1_INPUT_CLCW;
        out.clcw = (std::uint32_t{b0} << 24) | (std::uint32_t{b1} << 16) |
                   (std::uint32_t{b2} << 8) | b3;
    } else if (key == "timeout") {
        out.kind = ALDIS_FOP1_INPUT_TIMEOUT;
    } else if (key == "request_ad" || key == "request_bd") {
        data = unhex(v.at("data_hex").str());
        out.kind = key == "request_ad" ? ALDIS_FOP1_INPUT_REQUEST_AD : ALDIS_FOP1_INPUT_REQUEST_BD;
        out.handle = static_cast<std::uint32_t>(v.at("handle").num());
    } else if (key == "directive") {
        out = fop1_directive(v);
    } else if (key == "transmit_done") {
        out.kind = ALDIS_FOP1_INPUT_TRANSMIT_DONE;
        const std::string& kind = v.at("kind").str();
        out.frame_kind = kind == "AD"   ? ALDIS_FOP1_FRAME_AD
                         : kind == "BC" ? ALDIS_FOP1_FRAME_BC
                                        : ALDIS_FOP1_FRAME_BD;
        out.accepted = v.at("response").is("accept") ? 1 : 0;
    } else {
        return false;
    }
    return true;
}

inline Value fop1_output(const aldis_fop1_action_t& a, const std::uint8_t* area) {
    Value o = Value::of_object();
    switch (a.kind) {
    case ALDIS_FOP1_ACTION_TRANSMIT:
        return o.set("transmit",
                     Value::of_object()
                         .set("kind", text(frame_name(a.frame_kind)))
                         .set("ns", number(a.sequence_number))
                         .set("retransmission", Value::of_bool(a.retransmission == 1))
                         .set("frame_hex", text(hex(area + a.frame.offset, a.frame.len))));
    case ALDIS_FOP1_ACTION_ABORT:
        return o.set("event", text("abort"));
    case ALDIS_FOP1_ACTION_SUSPEND:
        return o.set("event", text("suspend"));
    case ALDIS_FOP1_ACTION_TRANSFER_NOTIFY:
        return o.set("event", text("transfer_notify"))
            .set("handle", number(a.handle))
            .set("type", text(notice(a.status)));
    case ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY:
        return o.set("event", text("directive_notify"))
            .set("handle", number(a.handle))
            .set("type", text(notice(a.status)));
    case ALDIS_FOP1_ACTION_ALERT:
        return o.set("event", text("alert"))
            .set("reason", text(alert_name(a.reason)))
            .set("event_number", number(a.event))
            .set("prior_state", text("S" + std::to_string(a.state)));
    default:
        return o.set("event", text("unknown action"));
    }
}

inline Tally run_fop1(const std::string& path) {
    Tally t;
    File file;
    if (!file.load(path, "fop1")) {
        t.failures.push_back(path + ": not loaded");
        return t;
    }
    for (const Scenario& sc : file.scenarios) {
        if (sc.superseded) {
            continue;  // kept for the record and not run (TV-021)
        }
        const std::uint8_t vcid = byte(sc.config->at("vcid"));
        bool hook = false;
        for (const Value* step : sc.steps) {
            aldis_fop1_input_t scratch{};
            std::vector<std::uint8_t> data;
            hook = hook || !fop1_input(step->at("input"), vcid, scratch, data);
        }
        if (hook) {
            ++t.skipped;
            continue;
        }
        ++t.run;
        const Value& c = *sc.config;
        aldis_fop1_config_t cfg{};
        cfg.scid = static_cast<std::uint16_t>(c.at("scid").num());
        cfg.vc_id = vcid;
        cfg.fecf = c.at("fecf").flag() ? 1 : 0;
        cfg.max_frame_len = kFrame;
        cfg.t1_initial_us = c.at("t1_initial_us").num();
        cfg.transmission_limit = byte(c.at("transmission_limit"));
        cfg.timeout_type = static_cast<std::uint32_t>(c.at("timeout_type").num());
        cfg.sliding_window_k = byte(c.at("k"));
        cfg.ad_repetitions = 1;
        cfg.bc_repetitions = 1;
        Fop1 engine;
        if (!aldis::ok(engine.init(cfg))) {
            t.failures.push_back(sc.def->at("id").str() + ": init failed");
            continue;
        }
        for (std::size_t i = 0; i < sc.steps.size(); ++i) {
            const Value& step = *sc.steps[i];
            aldis_fop1_input_t in{};
            std::vector<std::uint8_t> data;
            (void)fop1_input(step.at("input"), vcid, in, data);
            std::array<std::uint8_t, kFrame> area{};
            aldis_fop1_actions_t actions{};
            aldis::Detail detail{};
            const aldis::Status s =
                engine.step(in, data, step.at("at_us").num(), area, actions, &detail);
            Value got = Value::of_array();
            if (aldis::ok(s)) {
                for (std::size_t k = 0; k < actions.count; ++k) {
                    got.push(fop1_output(actions.items[k], area.data()));
                }
            } else {
                got.push(error_output(s, detail));
            }
            std::string problem = compare_outputs(got, step);
            aldis_fop1_status_t st{};
            if (!aldis::ok(engine.query(st))) {
                problem += "; status failed";
            }
            const std::string state = "S" + std::to_string(st.state);
            if (step.has("expect_state") && !step.at("expect_state").is(state)) {
                problem += "; state " + state + " want " + step.at("expect_state").str();
            }
            aldis::Deadline deadline;
            if (!aldis::ok(engine.next_deadline(deadline))) {
                problem += "; deadline failed";
            }
            const bool want_none = step.flag_or("expect_no_timeout", false);
            if (step.has("expect_timeout_us") || want_none) {
                const bool match = want_none
                                       ? !deadline.armed
                                       : deadline.armed &&
                                             deadline.at_us == step.at("expect_timeout_us").num();
                if (!match) {
                    problem += "; deadline " + std::to_string(deadline.at_us) +
                               (deadline.armed ? " armed" : " not armed");
                }
            }
            if (!problem.empty() && problem[0] == ';') {
                problem.erase(0, 2);
            }
            if (!problem.empty()) {
                t.fail(sc, i, step.at("input"), problem);
                break;
            }
        }
    }
    return t;
}

// ---- FARM-1

inline aldis_farm1_input_t farm1_blank() {
    aldis_farm1_input_t in{};
    in.kind = ALDIS_FARM1_INPUT_BUFFER_RELEASED;
    return in;
}

/// The input of a TC Transfer Frame that passed the Frame Validation Checks (232.0 §4.1.2,
/// §4.1.3.3, §4.1.4): the bypass and control command flags give the type, the data field follows
/// the five-octet header and precedes the FECF.
inline aldis_farm1_input_t farm1_frame(bool fecf, const std::vector<std::uint8_t>& octets,
                                       bool buffer_available, std::vector<std::uint8_t>& data) {
    const std::size_t trailer = fecf ? 2 : 0;
    data.assign(octets.begin() + 5, octets.end() - static_cast<std::ptrdiff_t>(trailer));
    const bool bypass = (octets[0] & 0x20) != 0;
    const bool control = (octets[0] & 0x10) != 0;
    aldis_farm1_input_t in = farm1_blank();
    in.vc_id = (octets[2] >> 2) & 0x3F;
    in.buffer_available = buffer_available ? 1 : 0;
    if (!bypass) {
        in.kind = ALDIS_FARM1_INPUT_FRAME_AD;
        in.sequence_number = octets[4];
    } else if (!control) {
        in.kind = ALDIS_FARM1_INPUT_FRAME_BD;
    } else if (data.size() == 1 && data[0] == 0x00) {
        in.kind = ALDIS_FARM1_INPUT_FRAME_BC_UNLOCK;
    } else if (data.size() == 3 && data[0] == 0x82 && data[1] == 0x00) {
        in.kind = ALDIS_FARM1_INPUT_FRAME_BC_SET_VR;
        in.v_star = data[2];
    } else {
        in.kind = ALDIS_FARM1_INPUT_FRAME_BC_OTHER;
    }
    return in;
}

inline std::string farm1_state(std::uint8_t n) { return "S" + std::to_string(n); }

inline Value farm1_output(const aldis_farm1_action_t& a, const std::uint8_t* area) {
    Value o = Value::of_object();
    switch (a.kind) {
    case ALDIS_FARM1_ACTION_DELIVER:
        return o.set("deliver",
                     Value::of_object()
                         .set("kind", text(a.fdu_kind == ALDIS_FARM1_FDU_AD ? "AD" : "BD"))
                         .set("ns", number(a.sequence_number))
                         .set("data_hex", text(hex(area + a.data.offset, a.data.len))));
    case ALDIS_FARM1_ACTION_FRAME_DISCARDED:
        return o.set("event", text("frame_discarded"))
            .set("event_number", number(a.event))
            .set("state", text(farm1_state(a.state)));
    case ALDIS_FARM1_ACTION_STATE_CHANGED:
        return o.set("event", text("state_changed"))
            .set("event_number", number(a.event))
            .set("from", text(farm1_state(a.state)))
            .set("to", text(farm1_state(a.to_state)));
    case ALDIS_FARM1_ACTION_UNLOCKED:
        return o.set("event", text("unlocked"));
    default:
        return o.set("event", text("unknown action"));
    }
}

/// The `query` output: the CLCW of the snapshot as the scenario writes it.
inline Value farm1_clcw(const aldis_farm1_status_t& s) {
    const std::uint8_t b0 = static_cast<std::uint8_t>(s.clcw >> 24);
    const std::uint8_t b1 = static_cast<std::uint8_t>(s.clcw >> 16);
    const std::uint8_t b2 = static_cast<std::uint8_t>(s.clcw >> 8);
    const std::uint8_t b3 = static_cast<std::uint8_t>(s.clcw);
    const std::uint8_t octets[4] = {b0, b1, b2, b3};
    return Value::of_object()
        .set("clcw", Value::of_object()
                         .set("lockout", Value::of_bool((b2 & 0x20) != 0))
                         .set("wait", Value::of_bool((b2 & 0x10) != 0))
                         .set("retransmit", Value::of_bool((b2 & 0x08) != 0))
                         .set("farm_b_counter", number((b2 >> 1) & 3))
                         .set("n_r", number(b3))
                         .set("cop_in_effect", number(b0 & 3))
                         .set("vcid", number(b1 >> 2))
                         .set("status_field", number((b0 >> 2) & 7))
                         .set("no_rf_available", Value::of_bool((b2 & 0x80) != 0))
                         .set("no_bit_lock", Value::of_bool((b2 & 0x40) != 0)))
        .set("clcw_hex", text(hex(octets, 4)));
}

inline Tally run_farm1(const std::string& path) {
    Tally t;
    File file;
    if (!file.load(path, "farm1")) {
        t.failures.push_back(path + ": not loaded");
        return t;
    }
    for (const Scenario& sc : file.scenarios) {
        if (sc.superseded) {
            continue;
        }
        ++t.run;
        const Value& c = *sc.config;
        const Value& w = c.at("window");
        const bool allowed = w.at("retransmission_allowed").flag();
        aldis_farm1_config_t cfg{};
        cfg.vc_id = byte(c.at("vcid"));
        cfg.status = byte(c.at("status_field"));
        cfg.retransmission_allowed = allowed ? 1 : 0;
        cfg.window = static_cast<std::uint16_t>(w.at("w").num());
        cfg.positive_window = allowed ? 0 : static_cast<std::uint16_t>(w.at("pw").num());
        cfg.v_r = byte(c.at("v_r"));
        Farm1 engine;
        if (!aldis::ok(engine.init(cfg))) {
            t.failures.push_back(sc.def->at("id").str() + ": init failed");
            continue;
        }
        for (std::size_t i = 0; i < sc.steps.size(); ++i) {
            const Value& step = *sc.steps[i];
            const auto& [key, v] = step.at("input").object.front();
            std::array<std::uint8_t, 2048> area{};
            std::vector<std::uint8_t> data;
            Value got = Value::of_array();
            if (key == "query") {
                aldis_farm1_status_t st{};
                if (!aldis::ok(engine.query(st))) {
                    got.push(text("status failed"));
                } else {
                    got.push(farm1_clcw(st));
                }
            } else {
                aldis_farm1_input_t in = farm1_blank();
                if (key == "frame") {
                    in = farm1_frame(c.at("fecf").flag(), unhex(v.at("frame_hex").str()),
                                     v.at("buffer_available").flag(), data);
                } else if (key == "invalid_frame") {
                    in.kind = ALDIS_FARM1_INPUT_INVALID_FRAME;
                } else if (key == "set_physical_flags") {
                    in.kind = ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS;
                    in.no_rf_available = v.at("no_rf_available").flag() ? 1 : 0;
                    in.no_bit_lock = v.at("no_bit_lock").flag() ? 1 : 0;
                }
                aldis_farm1_actions_t actions{};
                aldis::Detail detail{};
                const aldis::Status s =
                    engine.step(in, data, step.at("at_us").num(), area, actions, &detail);
                if (aldis::ok(s)) {
                    for (std::size_t k = 0; k < actions.count; ++k) {
                        got.push(farm1_output(actions.items[k], area.data()));
                    }
                } else {
                    got.push(error_output(s, detail));
                }
            }
            std::string problem = compare_outputs(got, step);
            aldis_farm1_status_t st{};
            if (!aldis::ok(engine.query(st))) {
                problem += "; status failed";
            }
            if (step.has("expect_state") && !step.at("expect_state").is(farm1_state(st.state))) {
                problem += "; state " + farm1_state(st.state) + " want " +
                           step.at("expect_state").str();
            }
            if (!problem.empty()) {
                t.fail(sc, i, step.at("input"), problem);
                break;
            }
        }
    }
    return t;
}

}  // namespace cop1

#endif  // ALDIS_TESTS_COP1_HPP
