// The link-engine scenarios of test-vectors/link/ through the handles of aldis.hpp (CABI-072,
// CABI-030, CABI-031; LNK-003, LNK-010…LNK-015, LNK-020, LNK-021, LNK-030…LNK-036,
// LNK-040…LNK-043, LNK-050…LNK-054, LNK-060…LNK-064, LNK-070…LNK-073, TV-010…TV-012). The
// mappings are those of crates/aldis-ffi/tests/link_engine/mod.rs: a step builds the flat input
// struct, calls the handle, and compares the complete, ordered actions (or the error with its
// field) and the deadline.

#ifndef ALDIS_TESTS_LINK_HPP
#define ALDIS_TESTS_LINK_HPP

#include <array>
#include <cstdint>
#include <string>
#include <vector>

#include "harness.hpp"

namespace linkeng {

using namespace harness;

inline std::uint8_t byte(const Value& v) { return static_cast<std::uint8_t>(v.num()); }
inline std::size_t size(const Value& v) { return static_cast<std::size_t>(v.num()); }
inline std::uint8_t octet(const Value& c, const char* key) {
    return unhex(c.at(key).str()).at(0);
}

/// The catalogue name of a field code (03 §9.1), for the error of a scenario.
inline std::string field_name(std::uint16_t code) {
    // The names are the upper-case constants; the scenarios write them in lower case.
    struct Row {
        std::uint16_t code;
        const char* name;
    };
    static const Row rows[] = {
#define F(upper, lower) {ALDIS_FIELD_##upper, lower},
        F(PACKET_LEN, "packet_len") F(FRAME_LEN, "frame_len") F(PACKET_VERSION, "packet_version")
        F(SDU_LEN, "sdu_len") F(FIELD_LEN, "field_len")
        F(DATA_FIELD_CONTENT, "data_field_content") F(FHP_LVO_POINTER, "fhp_lvo_pointer")
        F(TFDZ_CONSTRUCTION_RULE, "tfdz_construction_rule") F(MAX_PACKET_LEN, "max_packet_len")
        F(MAX_SDU_LEN, "max_sdu_len") F(COP_IN_EFFECT, "cop_in_effect") F(QOS, "qos")
        F(RAW_FRAMES, "raw_frames") F(FIRST_HEADER_POINTER, "first_header_pointer")
        F(FSH_LEN, "fsh_len") F(MAP_ID, "map_id") F(MAX_FDU_LEN, "max_fdu_len")
        F(VCF_COUNT_LEN, "vcf_count_len") F(BITSTREAM_DATA_POINTER, "bitstream_data_pointer")
        F(OCF_FLAG, "ocf_flag") F(FSH_FLAG, "fsh_flag")
        F(PROTOCOL_CONTROL_COMMAND_FLAG, "protocol_control_command_flag")
        F(VCF_COUNT, "vcf_count") F(INSERT_ZONE_LEN, "insert_zone_len")
#undef F
    };
    for (const Row& r : rows) {
        if (r.code == code) {
            return r.name;
        }
    }
    return "field#" + std::to_string(code);
}

/// The error of a failed call as the scenarios write it: the kind and the field name.
inline Value error_value(aldis::Status s, const aldis::Detail& d) {
    if (static_cast<std::uint32_t>(s) != d.kind) {
        return Value::of_object().set("error", text("status and detail disagree"));
    }
    Value v = Value::of_object().set("error", text(error_name(s)));
    if (d.field != 0) {
        v.set("field", text(field_name(d.field)));
    }
    return v;
}

inline std::uint32_t protocol_of(const Value& c) {
    const std::string& p = c.at("protocol").str();
    return p == "tm"    ? ALDIS_LINK_PROTOCOL_TM
           : p == "tc"  ? ALDIS_LINK_PROTOCOL_TC
           : p == "aos" ? ALDIS_LINK_PROTOCOL_AOS
                        : ALDIS_LINK_PROTOCOL_USLP;
}

inline std::uint32_t qos_of(const Value* name) {
    return name != nullptr && name->is("expedited") ? ALDIS_LINK_QOS_EXPEDITED
                                                    : ALDIS_LINK_QOS_SEQUENCE_CONTROLLED;
}

inline aldis_link_channel_t channel_of(const Value& c) {
    aldis_link_channel_t ch{};
    ch.protocol = protocol_of(c);
    ch.vc_id = static_cast<std::uint8_t>(c.num_or("vc", 0));
    ch.aos_edition = c.has("aos_issue") && c.at("aos_issue").num() == 4 ? 1 : 0;
    ch.uslp_upid = static_cast<std::uint8_t>(c.num_or("upid", 0));
    ch.uslp_map = static_cast<std::uint8_t>(c.num_or("map_id", 0));
    ch.uslp_qos = qos_of(c.find("qos"));
    return ch;
}

inline void add_route(std::uint8_t map_id, std::uint32_t qos, Value& v) {
    v.set("map", number(map_id));
    v.set("qos", text(qos == ALDIS_LINK_QOS_EXPEDITED ? "expedited" : "sequence_controlled"));
}

inline Value discarded(std::size_t octets) {
    return Value::of_object().set(
        "notify", Value::of_object().set("kind", text("Discarded")).set("octets", number(octets)));
}

inline Value field_of(std::uint8_t vc, const std::uint8_t* area, const aldis_range_t& r) {
    return Value::of_object().set("vc", number(vc)).set("field_hex", text(hex(area + r.offset, r.len)));
}

inline bool expects_would_block(const Value& step) {
    for (const Value& e : step.at("expect").array) {
        const Value* err = e.find("error");
        if (err != nullptr && err->is("WouldBlock")) {
            return true;
        }
    }
    return false;
}

/// The generic scenario loop of a link engine. `E` supplies the handle type, the area size, the
/// scenario file's engine name, and: `config(Value)`, `input(key, v, cfg, in, data, counter)`,
/// `outputs(actions, area, got)`, `counter(handle, name)`, and `skips(step)`.
template <class E>
Tally run(const std::string& path) {
    Tally t;
    File file;
    if (!file.load(path, E::kEngine)) {
        t.failures.push_back(path + ": not loaded");
        return t;
    }
    for (const Scenario& sc : file.scenarios) {
        if (sc.superseded) {
            continue;
        }
        ++t.run;
        const auto cfg = E::config(*sc.config);
        Value current = *sc.config;  // the configuration in force; a reconfigure replaces it
        typename E::Handle engine;
        aldis::Detail detail{};
        if (!aldis::ok(engine.init(cfg, &detail))) {
            t.failures.push_back(sc.def->at("id").str() + ": init failed " +
                                 tj::dump(error_value(aldis::Status{detail.kind}, detail)));
            continue;
        }
        for (std::size_t i = 0; i < sc.steps.size(); ++i) {
            const Value& step = *sc.steps[i];
            if (E::skips(step)) {
                ++t.skipped;
                break;
            }
            const auto& [key, v] = step.at("input").object.front();
            typename E::Input in{};
            in.config = cfg;
            std::vector<std::uint8_t> data;
            std::string counter;
            E::input(key, v, current, in, data, counter);
            Value got = Value::of_array();
            if (!counter.empty()) {
                E::counter(engine, counter, got);
            } else {
                std::array<std::uint8_t, E::kArea> area{};
                typename E::Actions actions{};
                aldis::Status s;
                if constexpr (E::kTimed) {
                    s = engine.step(in, data, step.at("at_us").num(), area, actions, &detail);
                } else {
                    s = engine.step(in, data, area, actions, &detail);
                }
                if (aldis::ok(s)) {
                    E::outputs(actions, area.data(), got, current);
                } else {
                    got.push(error_value(s, detail));
                }
            }
            std::string problem = compare_outputs(got, step);
            if constexpr (E::kTimed) {
                aldis::Deadline deadline;
                if (!aldis::ok(engine.next_deadline(deadline))) {
                    problem += "; deadline failed";
                }
                const bool none = step.flag_or("expect_no_timeout", false);
                if (step.has("expect_timeout_us") || none) {
                    const bool match = none ? !deadline.armed
                                            : deadline.armed && deadline.at_us ==
                                                                    step.at("expect_timeout_us").num();
                    if (!match) {
                        problem += "; deadline " + std::to_string(deadline.at_us) +
                                   (deadline.armed ? " armed" : " not armed");
                    }
                }
            }
            if (!problem.empty()) {
                t.fail(sc, i, step.at("input"), problem);
                break;
            }
        }
    }
    return t;
}

inline bool no_skip(const Value&) { return false; }

// ---- PacketGenerator

struct PacketGen {
    using Handle = aldis::PacketGenerator<ALDIS_LINK_PACKET_QUEUE_OCTETS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_packet_generator_input_t;
    using Actions = aldis_packet_generator_actions_t;
    static constexpr const char* kEngine = "packet-generator";
    static constexpr std::size_t kArea = 64;
    static constexpr bool kTimed = true;

    static aldis_packet_generator_config_t config(const Value& c) {
        bool space = false;
        bool encap = false;
        for (const Value& p : c.at("valid_pvns").array) {
            space = space || p.num() == 0;
            encap = encap || p.num() == 7;
        }
        aldis_packet_generator_config_t cfg{};
        cfg.channel = channel_of(c);
        cfg.field_len = size(c.at("field_len"));
        cfg.space_packets = space ? 1 : 0;
        cfg.encapsulation_packets = encap ? 1 : 0;
        cfg.max_packet_len = size(c.at("max_packet_len"));
        cfg.idle_policy = c.at("idle_policy").is("oid") ? ALDIS_LINK_IDLE_POLICY_OID
                                                         : ALDIS_LINK_IDLE_POLICY_IDLE_FIELD;
        cfg.idle_packet_kind = c.find("idle_packet_kind") != nullptr &&
                                       c.at("idle_packet_kind").is("space_packet")
                                   ? ALDIS_LINK_IDLE_PACKET_SPACE_PACKET
                                   : ALDIS_LINK_IDLE_PACKET_ENCAPSULATION;
        cfg.idle_pattern = octet(c, "idle_pattern_hex");
        cfg.release_deadline_us = c.num_or("release_deadline_us", 0);
        return cfg;
    }
    // The refusal depends on the 48 or 64 octets of the scenario's queue, which the handle
    // fixes at the build constant (LNK-015): the steps before it ran, the Rust runner covers it.
    static bool skips(const Value& step) { return expects_would_block(step); }
    static void input(const std::string& key, const Value& v, Value&, Input& in, std::vector<std::uint8_t>& data, std::string&) {
        if (key == "submit_packet") {
            data = unhex(v.at("packet_hex").str());
            in.kind = ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET;
        } else if (key == "release") {
            in.kind = ALDIS_PACKET_GENERATOR_INPUT_RELEASE;
        } else if (key == "timeout") {
            in.kind = ALDIS_PACKET_GENERATOR_INPUT_TIMEOUT;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE;
            in.config = config(v.at("config"));
        }
    }
    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            if (x.kind == ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD) {
                Value v = field_of(x.vc_id, area, x.field);
                v.set("fhp", number(x.pointer));
                if (x.route_present == 1) {
                    add_route(x.map_id, x.qos, v);
                }
                got.push(Value::of_object().set("data_field", std::move(v)));
            } else {
                got.push(discarded(x.octets));
            }
        }
    }
    static void counter(Handle&, const std::string&, Value&) {}
};

// ---- AccessGenerator

struct AccessGen {
    using Handle = aldis::AccessGenerator<ALDIS_LINK_PACKET_QUEUE_OCTETS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_access_generator_input_t;
    using Actions = aldis_access_generator_actions_t;
    static constexpr const char* kEngine = "access-generator";
    static constexpr std::size_t kArea = 64;
    static constexpr bool kTimed = false;

    static aldis_access_generator_config_t config(const Value& c) {
        aldis_access_generator_config_t cfg{};
        cfg.channel = channel_of(c);
        cfg.field_len = size(c.at("field_len"));
        cfg.max_sdu_len = c.has("max_sdu_len") ? size(c.at("max_sdu_len")) : 0;
        cfg.idle_pattern = c.has("idle_pattern_hex") ? octet(c, "idle_pattern_hex") : 0;
        return cfg;
    }
    static constexpr auto skips = no_skip;
    static void input(const std::string& key, const Value& v, Value&, Input& in, std::vector<std::uint8_t>& data, std::string&) {
        if (key == "submit_sdu") {
            data = unhex(v.at("sdu_hex").str());
            in.kind = ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU;
        } else if (key == "release") {
            in.kind = ALDIS_ACCESS_GENERATOR_INPUT_RELEASE;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE;
            in.config = config(v.at("config"));
        }
    }
    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            if (x.kind == ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD) {
                Value v = field_of(x.vc_id, area, x.field);
                if (x.pointer_present == 1) {
                    v.set("lvo", number(x.pointer));
                }
                if (x.route_present == 1) {
                    add_route(x.map_id, x.qos, v);
                }
                got.push(Value::of_object().set("data_field", std::move(v)));
            } else {
                got.push(discarded(x.octets));
            }
        }
    }
    static void counter(Handle&, const std::string&, Value&) {}
};

// ---- BitstreamGenerator

struct BitstreamGen {
    using Handle =
        aldis::BitstreamGenerator<ALDIS_LINK_PACKET_QUEUE_OCTETS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_bitstream_generator_input_t;
    using Actions = aldis_bitstream_generator_actions_t;
    static constexpr const char* kEngine = "bitstream-generator";
    static constexpr std::size_t kArea = 64;
    static constexpr bool kTimed = false;

    static aldis_bitstream_generator_config_t config(const Value& c) {
        aldis_bitstream_generator_config_t cfg{};
        cfg.channel = channel_of(c);
        cfg.field_len = size(c.at("field_len"));
        cfg.idle_pattern = octet(c, "idle_pattern_hex");
        return cfg;
    }
    static constexpr auto skips = no_skip;
    static void input(const std::string& key, const Value& v, Value&, Input& in,
                      std::vector<std::uint8_t>& data, std::string&) {
        if (key == "submit_bits") {
            data = unhex(v.at("bits_hex").str());
            in.kind = ALDIS_BITSTREAM_GENERATOR_INPUT_SUBMIT_BITS;
            in.bit_len = size(v.at("bit_len"));
        } else if (key == "release") {
            in.kind = ALDIS_BITSTREAM_GENERATOR_INPUT_RELEASE;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_BITSTREAM_GENERATOR_INPUT_RECONFIGURE;
            in.config = config(v.at("config"));
        }
    }
    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            if (x.kind == ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD) {
                Value v = field_of(x.vc_id, area, x.field);
                v.set("bdp", number(x.pointer));
                got.push(Value::of_object().set("data_field", std::move(v)));
            } else {
                got.push(discarded(x.octets));
            }
        }
    }
    static void counter(Handle&, const std::string&, Value&) {}
};

// ---- PacketExtractor

struct Extractor {
    using Handle = aldis::PacketExtractor<ALDIS_LINK_PACKET_QUEUE_OCTETS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_packet_extractor_input_t;
    using Actions = aldis_packet_extractor_actions_t;
    static constexpr const char* kEngine = "packet-extractor";
    static constexpr std::size_t kArea = 512;
    static constexpr bool kTimed = false;

    static aldis_packet_extractor_config_t config(const Value& c) {
        bool space = false;
        bool encap = false;
        for (const Value& p : c.at("valid_pvns").array) {
            space = space || p.num() == 0;
            encap = encap || p.num() == 7;
        }
        aldis_packet_extractor_config_t cfg{};
        cfg.channel = channel_of(c);
        cfg.space_packets = space ? 1 : 0;
        cfg.encapsulation_packets = encap ? 1 : 0;
        cfg.max_packet_len = size(c.at("max_packet_len"));
        cfg.deliver_incomplete = c.at("deliver_incomplete").flag() ? 1 : 0;
        cfg.deliver_idle = c.at("deliver_idle").flag() ? 1 : 0;
        return cfg;
    }
    static constexpr auto skips = no_skip;
    static void input(const std::string& key, const Value& v, Value&, Input& in, std::vector<std::uint8_t>& data, std::string& counter) {
        if (key == "data_field") {
            data = unhex(v.at("data_hex").str());
            in.kind = ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD;
            if (v.has("fhp")) {
                in.header_pointer_present = 1;
                in.header_pointer = static_cast<std::uint16_t>(v.at("fhp").num());
            }
            in.gap = v.at("gap").flag() ? 1 : 0;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE;
        } else if (key == "query") {
            counter = v.at("counter").str();
        }
    }
    static Value packet(const std::uint8_t* octets, std::size_t len, bool incomplete) {
        return Value::of_object().set(
            "deliver", Value::of_object()
                           .set("kind", text("Packet"))
                           .set("pvn", number(len == 0 ? 9 : octets[0] >> 5))
                           .set("data_hex", text(hex(octets, len)))
                           .set("incomplete", Value::of_bool(incomplete)));
    }
    /// The length of the packet that begins `octets`, from its own length field (133.0 §4.1.3.5,
    /// 133.1 §4.1.2.8), through the length functions of the C header; 0 if it has none.
    static std::size_t packet_len(const std::uint8_t* octets, std::size_t avail) {
        std::size_t total = 0;
        const std::uint8_t pvn = octets[0] >> 5;
        aldis::Status s = pvn == 0 ? aldis::Status{aldis_spp_peek_length(octets, avail, &total, nullptr)}
                                   : aldis::Status{aldis_epp_peek_length(octets, avail, &total, nullptr)};
        return aldis::ok(s) && total <= avail ? total : 0;
    }
    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            const std::uint8_t* run = area + x.packets.offset;
            switch (x.kind) {
            case ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS: {
                std::size_t at = 0;
                std::size_t n = 0;
                // bound: each pass consumes one whole packet of the run, and stops at the end of
                // the run or at a header that names no length.
                while (at < x.packets.len) {
                    const std::size_t len = packet_len(run + at, x.packets.len - at);
                    if (len == 0) {
                        break;
                    }
                    got.push(packet(run + at, len, false));
                    at += len;
                    ++n;
                }
                if (n != x.count) {
                    got.push(text("packet count differs"));
                }
                break;
            }
            case ALDIS_PACKET_EXTRACTOR_ACTION_INCOMPLETE_PACKET:
                got.push(packet(run, x.packets.len, true));
                break;
            case ALDIS_PACKET_EXTRACTOR_ACTION_DISCARDED:
                got.push(discarded(x.octets));
                break;
            default: {
                const char* kind = x.error == ALDIS_PACKET_EXTRACTOR_ERROR_PACKET_TOO_LONG
                                       ? "PacketTooLong"
                                   : x.error == ALDIS_PACKET_EXTRACTOR_ERROR_INVALID_PVN
                                       ? "InvalidPvn"
                                       : "LengthInconsistent";
                got.push(Value::of_object().set(
                    "extraction_error", Value::of_object().set("kind", text(kind))));
            }
            }
        }
    }
    static void counter(Handle& h, const std::string& name, Value& got) {
        aldis_packet_extractor_status_t s{};
        if (!aldis::ok(h.query(s))) {
            got.push(text("status failed"));
            return;
        }
        const std::uint64_t value = name == "PacketTooLong"        ? s.packet_too_long
                                    : name == "InvalidPvn"         ? s.invalid_pvn
                                    : name == "LengthInconsistent" ? s.length_inconsistent
                                                                   : s.idle_packets;
        got.push(Value::of_object().set(
            "counter", Value::of_object().set("name", text(name)).set("value", number(value))));
    }
};

// ---- FrameReceiver

struct Receiver {
    using Handle = aldis::FrameReceiver<ALDIS_LINK_MAX_VIRTUAL_CHANNELS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_frame_receiver_input_t;
    using Actions = aldis_frame_receiver_actions_t;
    static constexpr const char* kEngine = "frame-receiver";
    static constexpr std::size_t kArea = 2048;
    static constexpr bool kTimed = false;

    static std::uint32_t service(const Value& v) {
        return v.is("vc") ? ALDIS_LINK_SERVICE_VC
               : v.is("mc") ? ALDIS_LINK_SERVICE_MC
                            : ALDIS_LINK_SERVICE_NONE;
    }

    static aldis_frame_receiver_config_t config(const Value& c) {
        aldis_frame_receiver_config_t cfg{};
        cfg.protocol = protocol_of(c);
        cfg.frame_len = size(c.at("frame_len"));
        cfg.fecf = c.at("fecf").flag() ? 1 : 0;
        cfg.raw_frames = c.flag_or("accept_rejected_frame_insert_zone", false) ? 1 : 0;
        cfg.insert_zone_len = c.has("insert_zone_len") ? size(c.at("insert_zone_len")) : 0;
        if (c.has("ocf_service")) {
            cfg.ocf_service = service(c.at("ocf_service"));
        }
        if (c.has("fsh_service")) {
            cfg.fsh_service = service(c.at("fsh_service"));
        }
        for (std::size_t i = 0; i < c.at("vcs").array.size() && i < ALDIS_LINK_MAX_VIRTUAL_CHANNELS; ++i) {
            const Value& vc = c.at("vcs").array[i];
            std::uint32_t content = ALDIS_LINK_CONTENT_PACKETS;
            if (vc.has("role")) {
                const std::string& role = vc.at("role").str();
                content = role == "vca"         ? ALDIS_LINK_CONTENT_VCA
                          : role == "bitstream" ? ALDIS_LINK_CONTENT_BITSTREAM
                          : role == "unserved"  ? ALDIS_LINK_CONTENT_UNSERVED
                                                : ALDIS_LINK_CONTENT_PACKETS;
            }
            std::uint16_t maps = 0;
            if (vc.has("maps")) {
                for (const Value& n : vc.at("maps").array) {
                    maps = static_cast<std::uint16_t>(maps | (1U << n.num()));
                }
            }
            cfg.vcs[i].vc_id = byte(vc.at("vcid"));
            cfg.vcs[i].content = content;
            cfg.vcs[i].ocf = vc.flag_or("ocf", false) ? 1 : 0;
            cfg.vcs[i].fsh_len = static_cast<std::uint8_t>(vc.num_or("fsh_len", 0));
            cfg.vcs[i].cop = vc.flag_or("cop", false) ? 1 : 0;
            cfg.vcs[i].maps = maps;
            ++cfg.vc_count;
        }
        return cfg;
    }
    static constexpr auto skips = no_skip;

    static void input(const std::string& key, const Value& v, Value&, Input& in, std::vector<std::uint8_t>& data, std::string& counter) {
        if (key == "frame" || key == "raw_frame") {
            data = unhex(v.at("frame_hex").str());
            in.kind = key == "frame" ? ALDIS_FRAME_RECEIVER_INPUT_FRAME
                                     : ALDIS_FRAME_RECEIVER_INPUT_RAW_FRAME;
        } else if (key == "caller_discard") {
            in.kind = ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD;
            const std::string& r = v.at("reason").str();
            in.reason = r == "CrcMismatch"           ? ALDIS_LINK_DISCARD_CRC_MISMATCH
                        : r == "HeaderUncorrectable" ? ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE
                                                     : ALDIS_LINK_DISCARD_SECURITY;
        } else if (key == "frame_loss") {
            in.kind = ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE;
        } else if (key == "query") {
            counter = v.at("counter").str();
        }
    }

    static Value kind(const char* k) { return Value::of_object().set("kind", text(k)); }
    static const char* scope_name(const aldis_frame_receiver_action_t& a) {
        return a.scope == ALDIS_LINK_SCOPE_MASTER_CHANNEL ? "mc" : "vc";
    }
    static bool vc_scope(const aldis_frame_receiver_action_t& a) {
        return a.scope == ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL;
    }
    static void replay(Value& m, const aldis_frame_receiver_action_t& a) {
        if (a.replay_present == 1) {
            m.set("replay", Value::of_bool(a.replay == 1));
        }
    }
    static std::string data_hex(const aldis_frame_receiver_action_t& a, const std::uint8_t* area) {
        return hex(area + a.data.offset, a.data.len);
    }

    static Value output(const aldis_frame_receiver_action_t& a, const std::uint8_t* area) {
        Value o = Value::of_object();
        switch (a.kind) {
        case ALDIS_FRAME_RECEIVER_ACTION_FRAME_GAP: {
            Value m = kind("FrameGap");
            if (vc_scope(a)) {
                m.set("vc", number(a.vc_id));
            }
            m.set("scope", text(scope_name(a)));
            m.set("missing", number(a.missing));
            return o.set("notify", std::move(m));
        }
        case ALDIS_FRAME_RECEIVER_ACTION_COUNT_DISCONTINUITY: {
            Value m = kind("CountDiscontinuity");
            if (vc_scope(a)) {
                m.set("vc", number(a.vc_id));
            }
            m.set("scope", text(scope_name(a)));
            return o.set("notify", std::move(m));
        }
        case ALDIS_FRAME_RECEIVER_ACTION_FRAME_DISCARDED: {
            const char* why = a.reason == ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL      ? "UnknownChannel"
                              : a.reason == ALDIS_LINK_DISCARD_NOT_SERVED         ? "NotServed"
                              : a.reason == ALDIS_LINK_DISCARD_CRC_MISMATCH       ? "CrcMismatch"
                              : a.reason == ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE ? "HeaderUncorrectable"
                                                                                  : "Security";
            Value m = Value::of_object().set("reason", text(why));
            if (a.vc_present == 1) {
                m.set("vc", number(a.vc_id));
            }
            if (a.map_present == 1) {
                m.set("map", number(a.map_id));
            }
            return o.set("frame_discarded", std::move(m));
        }
        case ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG:
            return o.set("notify", kind("LossFlag"));
        case ALDIS_FRAME_RECEIVER_ACTION_DATA_FIELD: {
            Value m = kind("DataField");
            m.set("vc", number(a.vc_id));
            m.set("data_hex", text(data_hex(a, area)));
            m.set("gap", Value::of_bool(a.gap == 1));
            if (a.pointer_present == 1) {
                m.set("fhp", number(a.pointer));
            }
            if (a.bit_count_present == 1) {
                m.set("bit_count", number(a.bit_count));
            }
            replay(m, a);
            return o.set("deliver", std::move(m));
        }
        case ALDIS_FRAME_RECEIVER_ACTION_OCF: {
            Value m = kind("Ocf");
            m.set("scope", text(scope_name(a)));
            // The report type of the vectors: a CLCW is Type-1 (bit 0 of the word is 0).
            m.set("report", text((a.ocf >> 31) == 0 ? "Type1" : "Type2"));
            const std::uint8_t w[4] = {static_cast<std::uint8_t>(a.ocf >> 24),
                                       static_cast<std::uint8_t>(a.ocf >> 16),
                                       static_cast<std::uint8_t>(a.ocf >> 8),
                                       static_cast<std::uint8_t>(a.ocf)};
            m.set("ocf_hex", text(hex(w, 4)));
            if (vc_scope(a)) {
                m.set("vc", number(a.vc_id));
            }
            replay(m, a);
            return o.set("deliver", std::move(m));
        }
        case ALDIS_FRAME_RECEIVER_ACTION_FSH_SDU: {
            Value m = kind("FshSdu");
            m.set("scope", text(scope_name(a)));
            m.set("data_hex", text(data_hex(a, area)));
            if (vc_scope(a)) {
                m.set("vc", number(a.vc_id));
            }
            return o.set("deliver", std::move(m));
        }
        case ALDIS_FRAME_RECEIVER_ACTION_INSERT_SDU: {
            Value m = kind("InsertSdu");
            m.set("data_hex", text(data_hex(a, area)));
            replay(m, a);
            return o.set("deliver", std::move(m));
        }
        default: {
            Value m = kind(a.kind == ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME ? "TcFrame"
                                                                           : "ProtocolControl");
            m.set("vc", number(a.vc_id));
            m.set("data_hex", text(data_hex(a, area)));
            return o.set("deliver", std::move(m));
        }
        }
    }
    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            got.push(output(a.items[i], area));
        }
    }
    static void counter(Handle& h, const std::string& name, Value& got) {
        aldis_frame_receiver_status_t s{};
        if (!aldis::ok(h.query(s))) {
            got.push(text("status failed"));
            return;
        }
        const std::uint64_t value = name == "UnknownChannel"          ? s.unknown_channel
                                    : name == "NotServed"             ? s.not_served
                                    : name == "CrcMismatch"           ? s.crc_mismatch
                                    : name == "HeaderUncorrectable"   ? s.header_uncorrectable
                                    : name == "Security"              ? s.security
                                    : name == "oid_frames"            ? s.oid_frames
                                    : name == "gaps"                  ? s.gaps
                                                                      : s.count_discontinuities;
        got.push(Value::of_object().set(
            "counter", Value::of_object().set("name", text(name)).set("value", number(value))));
    }
};

// ---- ChannelMux

struct Mux {
    using Handle = aldis::ChannelMux<ALDIS_LINK_MAX_VIRTUAL_CHANNELS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_channel_mux_input_t;
    using Actions = aldis_channel_mux_actions_t;
    static constexpr const char* kEngine = "channel-mux";
    static constexpr std::size_t kArea = 2048;
    static constexpr bool kTimed = false;

    static std::uint32_t ocf_word(const std::string& hex_octets) {
        const std::vector<std::uint8_t> o = unhex(hex_octets);
        return (std::uint32_t{o.at(0)} << 24) | (std::uint32_t{o.at(1)} << 16) |
               (std::uint32_t{o.at(2)} << 8) | o.at(3);
    }

    static aldis_channel_mux_vc_t vc(const Value& o) {
        aldis_channel_mux_vc_t out{};
        const std::vector<std::uint8_t> fsh =
            o.has("fsh_init_hex") ? unhex(o.at("fsh_init_hex").str()) : std::vector<std::uint8_t>{};
        for (std::size_t i = 0; i < fsh.size() && i < ALDIS_CHANNEL_MUX_MAX_FSH_DATA; ++i) {
            out.fsh_init[i] = fsh[i];
        }
        const std::string& role = o.at("role").str();
        out.vc_id = byte(o.at("vcid"));
        out.priority = byte(o.at("priority"));
        out.weight = static_cast<std::uint16_t>(o.at("weight").num());
        out.sec_header_len = o.has("sec_header_len") ? size(o.at("sec_header_len")) : 0;
        out.sec_trailer_len = o.has("sec_trailer_len") ? size(o.at("sec_trailer_len")) : 0;
        // The USLP roles are the generators' business; the mux does not read them.
        out.role = role == "packets"    ? ALDIS_LINK_CONTENT_PACKETS
                   : role == "bitstream" ? ALDIS_LINK_CONTENT_BITSTREAM
                                         : ALDIS_LINK_CONTENT_VCA;
        out.ocf = o.has("ocf_init_hex") ? 1 : 0;
        out.ocf_init = o.has("ocf_init_hex") ? ocf_word(o.at("ocf_init_hex").str()) : 0;
        out.fsh_len = static_cast<std::uint8_t>(fsh.size());
        out.uslp_map = static_cast<std::uint8_t>(o.num_or("map_id", 0));
        out.uslp_qos = qos_of(o.find("qos"));
        out.uslp_vcf_len_sc = static_cast<std::uint8_t>(o.num_or("vcf_len_sc", 0));
        out.uslp_vcf_len_exp = static_cast<std::uint8_t>(o.num_or("vcf_len_exp", 0));
        out.uslp_cop = o.flag_or("cop", false) ? 1 : 0;
        return out;
    }

    static aldis_channel_mux_config_t config(const Value& c) {
        aldis_channel_mux_config_t cfg{};
        const std::vector<std::uint8_t> fsh = c.has("mc_fsh_init_hex")
                                                  ? unhex(c.at("mc_fsh_init_hex").str())
                                                  : std::vector<std::uint8_t>{};
        for (std::size_t i = 0; i < fsh.size() && i < ALDIS_CHANNEL_MUX_MAX_FSH_DATA; ++i) {
            cfg.mc_fsh_init[i] = fsh[i];
        }
        const std::string& policy = c.at("policy").at("kind").str();
        cfg.protocol = protocol_of(c);
        cfg.scid = static_cast<std::uint16_t>(c.at("scid").num());
        cfg.frame_len = size(c.at("frame_len"));
        cfg.fecf = c.at("fecf").flag() ? 1 : 0;
        cfg.policy = policy == "fixed_priority" ? ALDIS_LINK_MUX_POLICY_FIXED_PRIORITY
                     : policy == "round_robin"  ? ALDIS_LINK_MUX_POLICY_ROUND_ROBIN
                                                : ALDIS_LINK_MUX_POLICY_WEIGHTED_ROUND_ROBIN;
        cfg.insert_zone_len = c.has("insert_zone_len") ? size(c.at("insert_zone_len")) : 0;
        cfg.insert_idle_pattern = c.has("insert_init_hex") ? octet(c, "insert_init_hex") : 0;
        cfg.tm_oid_vc = static_cast<std::uint8_t>(c.num_or("oid_vc", 0));
        cfg.mc_ocf = c.has("mc_ocf_init_hex") ? 1 : 0;
        cfg.mc_ocf_init = c.has("mc_ocf_init_hex") ? ocf_word(c.at("mc_ocf_init_hex").str()) : 0;
        cfg.mc_fsh_len = static_cast<std::uint8_t>(fsh.size());
        cfg.aos_edition = 0;
        cfg.aos_fhec = c.flag_or("fhec", false) ? 1 : 0;
        cfg.aos_cycle_in_use = c.flag_or("cycle_in_use", false) ? 1 : 0;
        cfg.uslp_destination = static_cast<std::uint8_t>(c.num_or("source_or_destination", 0));
        cfg.uslp_oid_vcf_len = c.has("oid") ? byte(c.at("oid").at("vcf_len")) : 0;
        const auto& listed = c.at("vcs").array;
        for (std::size_t i = 0; i < listed.size() && i < ALDIS_LINK_MAX_VIRTUAL_CHANNELS; ++i) {
            cfg.vcs[i] = vc(listed[i]);
        }
        cfg.vc_count = listed.size();
        return cfg;
    }
    static constexpr auto skips = no_skip;

    static Value range_of(const aldis_range_t& r) {
        return Value::of_array().push(number(r.offset)).push(number(r.offset + r.len));
    }
    static void scope(const Value& v, Input& in) {
        if (v.at("scope").is("mc")) {
            in.scope = ALDIS_LINK_SCOPE_MASTER_CHANNEL;
        } else {
            in.scope = ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL;
            in.vc_id = byte(v.at("vc"));
        }
    }

    static void input(const std::string& key, const Value& v, Value& current, Input& in,
                      std::vector<std::uint8_t>& data, std::string&) {
        if (key == "offer") {
            data = unhex(v.at("field_hex").str());
            in.kind = ALDIS_CHANNEL_MUX_INPUT_OFFER;
            in.vc_id = byte(v.at("vc"));
            if (v.has("fhp")) {
                in.pointer_present = 1;
                in.pointer = static_cast<std::uint16_t>(v.at("fhp").num());
            }
            if (v.has("map") || v.has("qos")) {
                // The default of a VC is its MAP and QoS (USLP); TM and AOS have none.
                const Value* entry = nullptr;
                for (const Value& e : current.at("vcs").array) {
                    if (byte(e.at("vcid")) == in.vc_id) {
                        entry = &e;
                    }
                }
                in.route_present = 1;
                in.map_id = v.has("map") ? byte(v.at("map"))
                                         : (entry != nullptr && entry->has("map_id")
                                                ? byte(entry->at("map_id"))
                                                : 0);
                const Value* qos = v.find("qos");
                if (qos == nullptr && entry != nullptr) {
                    qos = entry->find("qos");
                }
                in.qos = qos_of(qos);
            }
            if (v.has("cop")) {
                const Value& c = v.at("cop");
                in.cop_present = 1;
                in.cop_vcf_count = c.at("vcf_count").num();
                in.cop_bypass = byte(c.at("bypass"));
                in.cop_protocol_control = byte(c.at("protocol_control"));
            }
        } else if (key == "opportunity") {
            in.kind = ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY;
        } else if (key == "set_ocf") {
            in.kind = ALDIS_CHANNEL_MUX_INPUT_SET_OCF;
            scope(v, in);
            in.ocf = ocf_word(v.at("ocf_hex").str());
        } else if (key == "set_fsh") {
            data = unhex(v.at("data_hex").str());
            in.kind = ALDIS_CHANNEL_MUX_INPUT_SET_FSH;
            scope(v, in);
        } else if (key == "set_insert") {
            data = unhex(v.at("data_hex").str());
            in.kind = ALDIS_CHANNEL_MUX_INPUT_SET_INSERT;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE;
            current = v.at("config");
            in.config = config(current);
        }
    }

    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            if (x.kind == ALDIS_CHANNEL_MUX_ACTION_TRANSMIT) {
                got.push(Value::of_object().set(
                    "transmit",
                    Value::of_object()
                        .set("kind", text(x.class_ == ALDIS_LINK_FRAME_CLASS_DATA ? "data" : "oid"))
                        .set("vc", number(x.vc_id))
                        .set("frame_hex", text(hex(area + x.frame.offset, x.frame.len)))
                        .set("security_header", range_of(x.security_header))
                        .set("data", range_of(x.data))
                        .set("security_trailer", range_of(x.security_trailer))));
            } else if (x.kind == ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED) {
                got.push(Value::of_object().set(
                    "notify", Value::of_object().set("kind", text("InsertSduRepeated"))));
            } else {
                got.push(discarded(x.octets));
            }
        }
    }
    static void counter(Handle&, const std::string&, Value&) {}
};

// ---- MapSegmenter

struct Segmenter {
    using Handle = aldis::MapSegmenter<ALDIS_LINK_PACKET_QUEUE_OCTETS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_map_segmenter_input_t;
    using Actions = aldis_map_segmenter_actions_t;
    static constexpr const char* kEngine = "map-segmenter";
    // The longest `max_fdu_len` of the files, as in the Rust runner.
    static constexpr std::size_t kArea = 32;
    static constexpr bool kTimed = true;

    static aldis_map_segmenter_map_t map_entry(const Value& o, bool usl) {
        aldis_map_segmenter_map_t e{};
        e.map_id = byte(o.at("map_id"));
        e.service = ALDIS_MAP_SERVICE_ANY;
        e.segmentation = o.flag_or("segmentation", false) ? 1 : 0;
        e.max_sdu_len = o.has("max_sdu_len") ? size(o.at("max_sdu_len")) : 0;
        e.blocking = o.flag_or("blocking", false) ? 1 : 0;
        e.uslp_upid = static_cast<std::uint8_t>(o.num_or("upid", 0));
        e.release_deadline_us = o.num_or("release_deadline_us", 0);
        // A USLP MAP with a `max_sdu_len` segments.
        if (usl) {
            e.segmentation = o.has("max_sdu_len") ? 1 : 0;
            const std::string& s = o.at("service").str();
            e.service = s == "packets" ? ALDIS_MAP_SERVICE_PACKETS
                        : s == "mapa"  ? ALDIS_MAP_SERVICE_SDU
                                       : ALDIS_MAP_SERVICE_OCTET_STREAM;
        }
        return e;
    }

    static aldis_map_segmenter_config_t config(const Value& c) {
        const bool usl = protocol_of(c) == ALDIS_LINK_PROTOCOL_USLP;
        aldis_map_segmenter_config_t out{};
        out.channel.protocol = protocol_of(c);
        out.channel.vc_id = 1;
        out.max_fdu_len = size(usl ? c.at("max_tfdf_len") : c.at("max_fdu_len"));
        if (c.has("maps")) {
            const auto& listed = c.at("maps").array;
            for (std::size_t i = 0; i < listed.size() && i < ALDIS_LINK_MAX_MAP_CHANNELS; ++i) {
                out.maps[i] = map_entry(listed[i], usl);
            }
            out.map_count = listed.size();
        }
        if (usl) {
            out.uslp_vcf_len_sc = 2;
            out.uslp_vcf_len_exp = 2;
        } else {
            out.tc_segment_header = c.at("segment_header").flag() ? 1 : 0;
            out.tc_fecf = c.at("fecf").flag() ? 1 : 0;
            out.tc_blocking = c.flag_or("blocking", false) ? 1 : 0;
        }
        return out;
    }
    // The refusal depends on the 256 octets of the scenario's queue (LNK-015): the Rust runner
    // covers it.
    static bool skips(const Value& step) { return expects_would_block(step); }

    static void input(const std::string& key, const Value& v, Value&, Input& in,
                      std::vector<std::uint8_t>& data, std::string&) {
        in.map_id = static_cast<std::uint8_t>(v.num_or("map", 0));
        if (key == "submit_packet") {
            data = unhex(v.at("packet_hex").str());
            in.kind = ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_PACKET;
        } else if (key == "submit_sdu") {
            data = unhex(v.at("sdu_hex").str());
            in.kind = ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU;
        } else if (key == "submit_octets") {
            data = unhex(v.at("data_hex").str());
            in.kind = ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS;
        } else if (key == "next_fdu") {
            in.kind = ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU;
            in.handle = static_cast<std::uint32_t>(v.at("handle").num());
            in.qos = qos_of(&v.at("qos"));
        } else if (key == "timeout") {
            in.kind = ALDIS_MAP_SEGMENTER_INPUT_TIMEOUT;
        } else if (key == "reconfigure") {
            in.kind = ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE;
            in.config = config(v.at("config"));
        }
    }

    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value& c) {
        const bool usl = protocol_of(c) == ALDIS_LINK_PROTOCOL_USLP;
        // A TC VC without a Segment Header has no flags to report.
        const bool header = !c.has("segment_header") || c.at("segment_header").flag();
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            if (x.kind != ALDIS_MAP_SEGMENTER_ACTION_FDU) {
                got.push(discarded(x.octets));
                continue;
            }
            const std::uint8_t* octets = area + x.fdu.offset;
            Value v = Value::of_object();
            v.set("handle", number(x.handle));
            v.set("qos", text(x.qos == ALDIS_LINK_QOS_EXPEDITED ? "expedited" : "sequence_controlled"));
            v.set("fdu_hex", text(hex(octets, x.fdu.len)));
            if (x.map_present == 1) {
                v.set("map", number(x.map_id));
            }
            if (header) {
                if (usl) {
                    const char* rule = x.framing == ALDIS_MAP_FRAMING_WHOLE        ? "111"
                                       : x.framing == ALDIS_MAP_FRAMING_FIRST      ? "100"
                                       : x.framing == ALDIS_MAP_FRAMING_CONTINUING ? "101"
                                       : x.framing == ALDIS_MAP_FRAMING_LAST       ? "110"
                                                                                   : "011";
                    v.set("rule", text(rule));
                    v.set("upid", number(octets[0] & 0x1F));
                } else {
                    const char* flags = x.framing == ALDIS_MAP_FRAMING_WHOLE       ? "11"
                                        : x.framing == ALDIS_MAP_FRAMING_FIRST     ? "01"
                                        : x.framing == ALDIS_MAP_FRAMING_CONTINUING ? "00"
                                                                                   : "10";
                    v.set("flags", text(flags));
                }
            }
            got.push(Value::of_object().set("fdu", std::move(v)));
        }
    }
    static void counter(Handle&, const std::string&, Value&) {}
};

// ---- MapReassembler

struct Reassembler {
    using Handle = aldis::MapReassembler<ALDIS_LINK_MAX_MAP_CHANNELS, ALDIS_LINK_MAX_FRAME_OCTETS>;
    using Input = aldis_map_reassembler_input_t;
    using Actions = aldis_map_reassembler_actions_t;
    static constexpr const char* kEngine = "map-reassembler";
    // The longest SDU of the MAPs of the files that name none, and the longest FDU.
    static constexpr std::size_t kSdu = 64;
    static constexpr std::size_t kArea = kSdu + 64;
    static constexpr bool kTimed = false;

    static aldis_map_reassembler_config_t config(const Value& c) {
        const bool usl = protocol_of(c) == ALDIS_LINK_PROTOCOL_USLP;
        aldis_map_reassembler_config_t out{};
        const auto& listed = c.at("maps").array;
        for (std::size_t i = 0; i < listed.size() && i < ALDIS_LINK_MAX_MAP_CHANNELS; ++i) {
            const Value& m = listed[i];
            const std::string& s = m.at("service").str();
            out.maps[i].vc_id = usl ? byte(m.at("vc")) : byte(c.at("vc"));
            out.maps[i].map_id = byte(m.at("map"));
            out.maps[i].service = s == "packets" ? ALDIS_MAP_SERVICE_PACKETS
                                  : s == "sdu"   ? ALDIS_MAP_SERVICE_SDU
                                                 : ALDIS_MAP_SERVICE_OCTET_STREAM;
            out.maps[i].max_sdu_len = m.has("max_sdu_len") ? size(m.at("max_sdu_len")) : kSdu;
        }
        out.channel.protocol = protocol_of(c);
        out.tc_segment_header = usl ? 0 : 1;
        out.map_count = listed.size();
        return out;
    }
    static constexpr auto skips = no_skip;

    static void input(const std::string& key, const Value& v, Value&, Input& in,
                      std::vector<std::uint8_t>& data, std::string& counter) {
        if (key == "fdu") {
            data = unhex(v.at("data_hex").str());
            in.kind = ALDIS_MAP_REASSEMBLER_INPUT_FDU;
            in.vc_id = byte(v.at("vc"));
            in.map_id = static_cast<std::uint8_t>(v.num_or("map", 0));
            in.gap = v.flag_or("gap", false) ? 1 : 0;
        } else if (key == "reconfigure") {
            // The current configuration is applied again (LNK-003).
            in.kind = ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE;
        } else if (key == "query") {
            counter = v.at("counter").str();
        }
    }

    static const char* event_name(std::uint32_t e) {
        return e == ALDIS_MAP_REASSEMBLY_EVENT_GAP_DISCARD         ? "GapDiscard"
               : e == ALDIS_MAP_REASSEMBLY_EVENT_SEQUENCE_VIOLATION ? "SequenceViolation"
               : e == ALDIS_MAP_REASSEMBLY_EVENT_UNKNOWN_MAP        ? "UnknownMap"
                                                                    : "SduTooLong";
    }

    static void outputs(const Actions& a, const std::uint8_t* area, Value& got, const Value&) {
        for (std::size_t i = 0; i < a.count; ++i) {
            const auto& x = a.items[i];
            const std::uint8_t* data = area + x.data.offset;
            switch (x.kind) {
            case ALDIS_MAP_REASSEMBLER_ACTION_EVENT:
                got.push(Value::of_object().set(
                    "reassembly_event", Value::of_object()
                                            .set("kind", text(event_name(x.event)))
                                            .set("vc", number(x.vc_id))
                                            .set("map", number(x.map_id))));
                break;
            case ALDIS_MAP_REASSEMBLER_ACTION_SDU:
                got.push(Value::of_object().set(
                    "deliver", Value::of_object()
                                   .set("kind", text("Sdu"))
                                   .set("vc", number(x.vc_id))
                                   .set("map", number(x.map_id))
                                   .set("data_hex", text(hex(data, x.data.len)))));
                break;
            case ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA: {
                Value d = Value::of_object()
                              .set("kind", text("Sdu"))
                              .set("vc", number(x.vc_id))
                              .set("map", number(x.map_id))
                              .set("octet_stream", Value::of_bool(true))
                              .set("data_hex", text(hex(data, x.data.len)));
                if (x.after_gap == 1) {
                    d.set("after_gap", Value::of_bool(true));
                }
                got.push(Value::of_object().set("deliver", std::move(d)));
                break;
            }
            case ALDIS_MAP_REASSEMBLER_ACTION_PACKETS: {
                std::size_t at = 0;
                std::size_t n = 0;
                // bound: each pass consumes one whole packet of the run, and stops at the end of
                // the run or at a header that names no length.
                while (at < x.data.len) {
                    const std::size_t len = Extractor::packet_len(data + at, x.data.len - at);
                    if (len == 0) {
                        break;
                    }
                    got.push(Value::of_object().set(
                        "deliver", Value::of_object()
                                       .set("kind", text("Packet"))
                                       .set("vc", number(x.vc_id))
                                       .set("map", number(x.map_id))
                                       .set("pvn", number(data[at] >> 5))
                                       .set("data_hex", text(hex(data + at, len)))
                                       .set("incomplete", Value::of_bool(false))));
                    at += len;
                    ++n;
                }
                if (n != x.count) {
                    got.push(text("packet count differs"));
                }
                break;
            }
            default:
                got.push(discarded(x.octets));
            }
        }
    }
    static void counter(Handle& h, const std::string& name, Value& got) {
        aldis_map_reassembler_status_t s{};
        if (!aldis::ok(h.query(s))) {
            got.push(text("status failed"));
            return;
        }
        const std::uint64_t value = name == "GapDiscard"           ? s.gap_discards
                                    : name == "SequenceViolation"  ? s.sequence_violations
                                    : name == "UnknownMap"         ? s.unknown_maps
                                                                   : s.sdus_too_long;
        got.push(Value::of_object().set(
            "counter", Value::of_object().set("name", text(name)).set("value", number(value))));
    }
};

}  // namespace linkeng

#endif  // ALDIS_TESTS_LINK_HPP
