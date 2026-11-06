// The engine-scenario vectors of test-vectors/cop1/ and test-vectors/link/ through `aldis.hpp`
// (CABI-072), linked against the Rust-backed C ABI library. Usage: `engines <test-vectors-dir>`.
// Built with `-std=c++20 -fno-exceptions -Wall -Wextra -Wpedantic -Werror` (VER-050).

#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <system_error>
#include <vector>
#include <string>
#include <type_traits>

#include "cop1.hpp"
#include "link.hpp"
#include "sync.hpp"

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

// The sync handle and the codec functions through the C header (CABI-031, CABI-032, SYNC-003,
// SYNC-004): the codecs are not wrapped, so a C++ caller calls `aldis_sync_attach` and
// `aldis_sync_randomize` directly.
static int sync_lifecycle() {
    using Search = aldis::SyncSearch<ALDIS_SYNC_MAX_FRAME_OCTETS, ALDIS_SYNC_MAX_CHUNK_OCTETS>;
    static_assert(!std::is_copy_constructible_v<Search> && !std::is_move_constructible_v<Search>);
    static_assert(sizeof(Search) >= sizeof(aldis_sync_search_t));
    int failed = 0;
    const auto expect = [&failed](const char* what, aldis::Status got, aldis::Status want) {
        if (got != want) {
            std::fprintf(stderr, "FAIL sync lifecycle %s: %s, want %s\n", what,
                         harness::error_name(got), harness::error_name(want));
            failed = 1;
        }
    };
    Search search;
    aldis_sync_search_status_t snapshot{};
    expect("query before init", search.query(snapshot), aldis::Status::InvalidHandle);
    aldis_sync_search_config_t cfg{};
    cfg.protocol = ALDIS_LINK_PROTOCOL_TC;
    cfg.frame_len = 3;
    expect("TC is refused", search.init(cfg), aldis::Status::InvalidConfig);
    cfg.protocol = ALDIS_LINK_PROTOCOL_TM;
    expect("init", search.init(cfg), aldis::Status::Ok);
    // The CADU of a randomized frame, found again and derandomized.
    std::uint8_t frame[3] = {1, 2, 3};
    std::uint8_t cadu[7] = {};
    std::size_t cadu_len = 0;
    expect("randomize", aldis::Status{aldis_sync_randomize(ALDIS_SYNC_RANDOMIZER_131071, frame, 3, nullptr)},
           aldis::Status::Ok);
    expect("attach", aldis::Status{aldis_sync_attach(&cfg, frame, 3, cadu, 7, &cadu_len, nullptr)},
           aldis::Status::Ok);
    aldis_sync_search_input_t in{};
    in.kind = ALDIS_SYNC_SEARCH_INPUT_CHUNK;
    aldis_sync_search_actions_t actions{};
    std::vector<std::uint8_t> area(ALDIS_SYNC_MAX_CHUNK_OCTETS + ALDIS_SYNC_MAX_FRAME_OCTETS);
    expect("step", search.step(in, std::span<const std::uint8_t>(cadu, cadu_len), area, actions),
           aldis::Status::Ok);
    expect("randomize back",
           aldis::Status{aldis_sync_randomize(ALDIS_SYNC_RANDOMIZER_131071, area.data(), 3, nullptr)},
           aldis::Status::Ok);
    if (cadu_len != 7 || actions.count != 1 || actions.items[0].unit.len != 3 || area[0] != 1 ||
        area[1] != 2 || area[2] != 3) {
        std::fprintf(stderr, "FAIL sync lifecycle: the frame did not come back\n");
        failed = 1;
    }
    search.deinit();
    expect("query after deinit", search.query(snapshot), aldis::Status::InvalidHandle);
    std::printf("sync lifecycle: %s\n", failed == 0 ? "ok" : "failed");
    return failed;
}

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: engines <test-vectors-dir>\n");
        return 2;
    }
    const std::string dir = argv[1];
    int failed = lifecycle() | sync_lifecycle();
    // Every scenario file of the four directories, sorted; the engine is named by the file's
    // prefix. A file no runner handles fails the run, so a new vector file cannot pass unrun.
    namespace fs = std::filesystem;
    std::vector<fs::path> files;
    for (const char* sub : {"cop1/scenarios", "link/send", "link/recv", "sync/scenarios"}) {
        std::error_code ec;
        for (fs::directory_iterator it(fs::path(dir) / sub, ec), end; !ec && it != end;
             it.increment(ec)) {
            if (it->path().extension() == ".json") {
                files.push_back(it->path());
            }
        }
        if (ec) {
            std::fprintf(stderr, "FAIL cannot list %s/%s\n", dir.c_str(), sub);
            failed = 1;
        }
    }
    std::sort(files.begin(), files.end());
    struct Runner {
        const char* prefix;
        harness::Tally (*run)(const std::string&);
    };
    static const Runner runners[] = {
        {"fop1_", cop1::run_fop1},
        {"farm1_", cop1::run_farm1},
        {"packet_generator_", linkeng::run<linkeng::PacketGen>},
        {"access_generator_", linkeng::run<linkeng::AccessGen>},
        {"bitstream_generator_", linkeng::run<linkeng::BitstreamGen>},
        {"packet_extractor_", linkeng::run<linkeng::Extractor>},
        {"frame_receiver_", linkeng::run<linkeng::Receiver>},
        {"channel_mux_", linkeng::run<linkeng::Mux>},
        {"map_segmenter_", linkeng::run<linkeng::Segmenter>},
        {"map_reassembler_", linkeng::run<linkeng::Reassembler>},
        {"search_", synceng::run},
    };
    for (const fs::path& file : files) {
        const std::string name = file.stem().string();
        bool handled = false;
        for (const Runner& r : runners) {
            if (name.rfind(r.prefix, 0) == 0) {
                failed |= harness::report(name.c_str(), r.run(file.string()));
                handled = true;
                break;
            }
        }
        if (!handled) {
            std::fprintf(stderr, "FAIL %s: no runner handles this vector file\n", file.c_str());
            failed = 1;
        }
    }
    if (files.empty()) {
        std::fprintf(stderr, "FAIL no vector files under %s\n", dir.c_str());
        failed = 1;
    }
    return failed;
}
