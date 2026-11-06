// The engine-scenario vectors of test-vectors/cop1/ and test-vectors/link/ through `aldis.hpp`
// (CABI-072), linked against the Rust-backed C ABI library. Usage: `engines <test-vectors-dir>`.
// Built with `-std=c++20 -fno-exceptions -Wall -Wextra -Wpedantic -Werror` (VER-050).

#include <cstdio>
#include <string>
#include <type_traits>

#include "cop1.hpp"
#include "link.hpp"

// The handle lifecycle (CABI-031, CABI-032) and the status mirror, on the CLTU packer, which has no
// scenario vectors: storage not yet initialised, a refused configuration, a live engine, and a
// released one.
static int lifecycle() {
    using Packer = aldis::CltuPacker<ALDIS_LINK_MAX_FRAME_OCTETS>;
    static_assert(!std::is_copy_constructible_v<Packer> && !std::is_move_constructible_v<Packer>);
    static_assert(!std::is_copy_assignable_v<Packer> && !std::is_move_assignable_v<Packer>);
    static_assert(sizeof(Packer) >= sizeof(aldis_cltu_packer_t));
    int failed = 0;
    const auto expect = [&failed](const char* what, aldis::Status got, aldis::Status want) {
        if (got != want) {
            std::fprintf(stderr, "FAIL lifecycle %s: %s, want %s\n", what, harness::error_name(got),
                         harness::error_name(want));
            failed = 1;
        }
    };
    Packer packer;
    aldis_cltu_packer_status_t snapshot{};
    expect("query before init", packer.query(snapshot), aldis::Status::InvalidHandle);
    aldis_cltu_packer_config_t cfg{};
    cfg.channel.protocol = ALDIS_LINK_PROTOCOL_TM;
    cfg.max_frames = 1;
    cfg.max_unit_len = 64;
    aldis::Detail detail{};
    expect("TM is refused", packer.init(cfg, &detail), aldis::Status::InvalidConfig);
    if (detail.kind != static_cast<std::uint32_t>(aldis::Status::InvalidConfig)) {
        std::fprintf(stderr, "FAIL lifecycle: the detail does not name the error\n");
        failed = 1;
    }
    cfg.channel.protocol = ALDIS_LINK_PROTOCOL_TC;
    expect("init", packer.init(cfg), aldis::Status::Ok);
    expect("query", packer.query(snapshot), aldis::Status::Ok);
    packer.deinit();
    expect("query after deinit", packer.query(snapshot), aldis::Status::InvalidHandle);
    std::printf("lifecycle: %s\n", failed == 0 ? "ok" : "failed");
    return failed;
}

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: engines <test-vectors-dir>\n");
        return 2;
    }
    const std::string dir = argv[1];
    int failed = lifecycle();
    for (const char* name : {"fop1_table", "fop1_ad_service", "fop1_window_timer",
                             "fop1_wrap_directives", "fop1_clcw"}) {
        failed |= harness::report(name, cop1::run_fop1(dir + "/cop1/scenarios/" + name + ".json"));
    }
    for (const char* name : {"farm1_table", "farm1_window", "farm1_flow_clcw"}) {
        failed |= harness::report(name, cop1::run_farm1(dir + "/cop1/scenarios/" + name + ".json"));
    }
    const auto vectors = [&](const char* dir_name, const char* name, auto run) {
        return harness::report(name, run(dir + "/link/" + dir_name + "/" + name + ".json"));
    };
    for (const char* name : {"packet_generator_tm", "packet_generator_aos",
                             "packet_generator_aos_issue4", "packet_generator_uslp"}) {
        failed |= vectors("send", name, linkeng::run<linkeng::PacketGen>);
    }
    for (const char* name : {"access_generator_tm", "access_generator_aos", "access_generator_uslp"}) {
        failed |= vectors("send", name, linkeng::run<linkeng::AccessGen>);
    }
    failed |= vectors("send", "bitstream_generator_aos", linkeng::run<linkeng::BitstreamGen>);
    for (const char* name : {"packet_extractor_tm", "packet_extractor_aos", "packet_extractor_uslp"}) {
        failed |= vectors("recv", name, linkeng::run<linkeng::Extractor>);
    }
    for (const char* name : {"frame_receiver_tm", "frame_receiver_tc", "frame_receiver_aos",
                             "frame_receiver_uslp"}) {
        failed |= vectors("recv", name, linkeng::run<linkeng::Receiver>);
    }
    for (const char* name : {"channel_mux_tm", "channel_mux_aos", "channel_mux_uslp"}) {
        failed |= vectors("send", name, linkeng::run<linkeng::Mux>);
    }
    for (const char* name : {"map_segmenter_tc", "map_segmenter_uslp"}) {
        failed |= vectors("send", name, linkeng::run<linkeng::Segmenter>);
    }
    for (const char* name : {"map_reassembler_tc", "map_reassembler_uslp"}) {
        failed |= vectors("recv", name, linkeng::run<linkeng::Reassembler>);
    }
    return failed;
}
