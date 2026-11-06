// aldis.hpp: header-only C++20 wrapper of the engine handles of the C ABI (CABI-072, OI-19, OI-20).
//
// Hand-written; `aldis.h` is generated and is never edited. This header adds no behaviour to the
// engines: each call forwards to the function of `aldis.h` and returns its status.
//
//  * RAII handle types (CABI-031, CABI-032): the storage is a member of the typed storage struct
//    of the C ABI (`aldis_fop1_t`, ...), so its size and alignment come from the C type and no
//    length crosses the ABI. A handle is built empty; `init` places the engine in the storage and
//    the destructor calls `deinit`. A handle cannot be copied or moved: the tag of the storage
//    holds its own address, so a copy is `ALDIS_ERR_INVALID_HANDLE` (CABI-031).
//  * Capacities are template parameters, checked with `static_assert` against the build-time
//    `ALDIS_*_MAX_*` constants of the library (CABI-030, OI-20): a handle type whose capacity
//    differs from the library's does not compile.
//  * A call returns `aldis::Status`, a `[[nodiscard]] enum class` that mirrors the status codes
//    of `aldis.h` (CABI-020); the optional `aldis_error_detail_t` names the field and the values.
//    There are no exceptions and no `expected`-like type.
//  * Buffers are `std::span`: the octets an input carries and the output area of a call.
//  * The codec functions are not wrapped: use those of `aldis.h` (CABI-072).
//
// Threads: an engine is used by one thread at a time (CABI-050).

#ifndef ALDIS_HPP
#define ALDIS_HPP

#include <cstddef>
#include <cstdint>
#include <span>

#include "aldis/aldis.h"

namespace aldis {

/// The status of a call: `Ok`, or the catalogue code of the error (03 §9; CABI-020). The values
/// are those of `ALDIS_OK` and `ALDIS_ERR_*`; the assertions below fail the build when the
/// header and this wrapper disagree.
enum class [[nodiscard]] Status : std::uint32_t {
    Ok = 0,
    BufferTooSmall = 1,
    Incomplete = 2,
    LengthMismatch = 3,
    UnsupportedVersion = 4,
    OutOfRange = 5,
    InvalidField = 6,
    ReservedValue = 7,
    CrcMismatch = 8,
    HeaderUncorrectable = 9,
    UnknownChannel = 10,
    InvalidConfig = 11,
    WouldBlock = 12,
    InvalidState = 13,
    Security = 14,
    CapacityExceeded = 15,
    InvalidHandle = 16,
    Malformed = 17,
    InternalError = 18,
};

static_assert(static_cast<std::uint32_t>(Status::Ok) == ALDIS_OK);
static_assert(static_cast<std::uint32_t>(Status::BufferTooSmall) == ALDIS_ERR_BUFFER_TOO_SMALL);
static_assert(static_cast<std::uint32_t>(Status::Incomplete) == ALDIS_ERR_INCOMPLETE);
static_assert(static_cast<std::uint32_t>(Status::LengthMismatch) == ALDIS_ERR_LENGTH_MISMATCH);
static_assert(static_cast<std::uint32_t>(Status::UnsupportedVersion) ==
              ALDIS_ERR_UNSUPPORTED_VERSION);
static_assert(static_cast<std::uint32_t>(Status::OutOfRange) == ALDIS_ERR_OUT_OF_RANGE);
static_assert(static_cast<std::uint32_t>(Status::InvalidField) == ALDIS_ERR_INVALID_FIELD);
static_assert(static_cast<std::uint32_t>(Status::ReservedValue) == ALDIS_ERR_RESERVED_VALUE);
static_assert(static_cast<std::uint32_t>(Status::CrcMismatch) == ALDIS_ERR_CRC_MISMATCH);
static_assert(static_cast<std::uint32_t>(Status::HeaderUncorrectable) ==
              ALDIS_ERR_HEADER_UNCORRECTABLE);
static_assert(static_cast<std::uint32_t>(Status::UnknownChannel) == ALDIS_ERR_UNKNOWN_CHANNEL);
static_assert(static_cast<std::uint32_t>(Status::InvalidConfig) == ALDIS_ERR_INVALID_CONFIG);
static_assert(static_cast<std::uint32_t>(Status::WouldBlock) == ALDIS_ERR_WOULD_BLOCK);
static_assert(static_cast<std::uint32_t>(Status::InvalidState) == ALDIS_ERR_INVALID_STATE);
static_assert(static_cast<std::uint32_t>(Status::Security) == ALDIS_ERR_SECURITY);
static_assert(static_cast<std::uint32_t>(Status::CapacityExceeded) ==
              ALDIS_ERR_CAPACITY_EXCEEDED);
static_assert(static_cast<std::uint32_t>(Status::InvalidHandle) == ALDIS_ERR_INVALID_HANDLE);
static_assert(static_cast<std::uint32_t>(Status::Malformed) == ALDIS_ERR_MALFORMED);
static_assert(static_cast<std::uint32_t>(Status::InternalError) == ALDIS_ERR_INTERNAL_ERROR);

/// True for `Status::Ok`.
[[nodiscard]] constexpr bool ok(Status s) noexcept { return s == Status::Ok; }

/// The error detail of the C ABI (CABI-021): the kind, the field code, and the values.
using Detail = aldis_error_detail_t;

/// The deadline of a timer (API-053): `armed` is false and `at_us` 0 when no timer runs.
struct Deadline {
    bool armed = false;
    std::uint64_t at_us = 0;
};

namespace detail {

/// The C functions of one engine, bound by `ALDIS_HPP_BINDING`.
template <class Api>
class Handle {
public:
    using Config = typename Api::Config;
    using Input = typename Api::Input;
    using Actions = typename Api::Actions;
    using Snapshot = typename Api::Snapshot;

    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    Handle(Handle&&) = delete;
    Handle& operator=(Handle&&) = delete;

    /// Places the engine in the storage of the handle (CABI-031, CABI-032). It replaces whatever
    /// the storage holds, live or not.
    [[nodiscard]] Status init(const Config& config, Detail* err = nullptr) noexcept {
        return Status{Api::init(&storage_, &config, err)};
    }

    /// Releases the engine; later calls answer `Status::InvalidHandle`. The destructor does it.
    void deinit() noexcept { Api::deinit(&storage_); }

    /// Applies one input and writes the actions of its cell (CABI-040). `data` is the octets the
    /// input carries (it replaces the `data` and `data_len` of `input`), `out` the area the
    /// actions' ranges refer to (API-061). On an error `actions` is not written (API-058).
    /// For an engine without a timer.
    [[nodiscard]] Status step(Input input, std::span<const std::uint8_t> data,
                              std::span<std::uint8_t> out, Actions& actions,
                              Detail* err = nullptr) noexcept
        requires(!Api::kTimed)
    {
        input.data = data.data();
        input.data_len = data.size();
        return Status{Api::step(&storage_, input, 0, out.data(), out.size(), actions, err)};
    }

    /// As above, for an engine with a timer: `now_us` is the caller's monotonic clock in
    /// microseconds (API-003).
    [[nodiscard]] Status step(Input input, std::span<const std::uint8_t> data,
                              std::uint64_t now_us, std::span<std::uint8_t> out,
                              Actions& actions, Detail* err = nullptr) noexcept
        requires(Api::kTimed)
    {
        input.data = data.data();
        input.data_len = data.size();
        return Status{Api::step(&storage_, input, now_us, out.data(), out.size(), actions, err)};
    }

    /// Reads the state of the engine; it changes nothing (API-056).
    [[nodiscard]] Status query(Snapshot& snapshot, Detail* err = nullptr) noexcept {
        return Status{Api::status(&storage_, &snapshot, err)};
    }

    /// Reads the deadline of the timer of the engine (API-053).
    [[nodiscard]] Status next_deadline(Deadline& deadline, Detail* err = nullptr) noexcept
        requires(Api::kDeadline)
    {
        std::uint8_t armed = 0;
        std::uint64_t at = 0;
        const Status s{Api::deadline(&storage_, &armed, &at, err)};
        if (ok(s)) {
            deadline.armed = armed == 1;
            deadline.at_us = at;
        }
        return s;
    }

protected:
    Handle() noexcept = default;
    ~Handle() { deinit(); }

private:
    // The words of the C storage struct; the zero state is not a live engine (the tag check).
    typename Api::Storage storage_{};
};

}  // namespace detail

}  // namespace aldis

// One binding per engine: the C types and functions of `aldis_<name>_*`. `kTimed` engines take
// `now_us`; `kDeadline` engines have `aldis_<name>_next_deadline`. `STEP` is the argument list of
// the step call, written with the parameter names of `step` below.
#define ALDIS_HPP_BINDING(Name, Cname, Timed, Deadline_, STEP)                                    \
    struct Name##Api {                                                                           \
        using Storage = aldis_##Cname##_t;                                                       \
        using Config = aldis_##Cname##_config_t;                                                 \
        using Input = aldis_##Cname##_input_t;                                                   \
        using Actions = aldis_##Cname##_actions_t;                                               \
        using Snapshot = aldis_##Cname##_status_t;                                               \
        static constexpr bool kTimed = Timed;                                                    \
        static constexpr bool kDeadline = Deadline_;                                             \
        static std::uint32_t init(Storage* s, const Config* c, Detail* e) noexcept {            \
            return aldis_##Cname##_init(s, c, e);                                                \
        }                                                                                        \
        static std::uint32_t step(Storage* s, const Input& in, std::uint64_t now,                \
                                  std::uint8_t* out, std::size_t cap, Actions& a,                \
                                  Detail* e) noexcept {                                          \
            (void)now;                                                                           \
            return aldis_##Cname##_step STEP;                                                    \
        }                                                                                        \
        static std::uint32_t status(Storage* s, Snapshot* st, Detail* e) noexcept {              \
            return aldis_##Cname##_status(s, st, e);                                             \
        }                                                                                        \
        static void deinit(Storage* s) noexcept { aldis_##Cname##_deinit(s); }                  \
        static std::uint32_t deadline(Storage* s, std::uint8_t* armed, std::uint64_t* at,        \
                                      Detail* e) noexcept {                                      \
            return ALDIS_HPP_DEADLINE_##Deadline_(Cname, s, armed, at, e);                       \
        }                                                                                        \
    }

#define ALDIS_HPP_DEADLINE_true(Cname, s, armed, at, e) aldis_##Cname##_next_deadline(s, armed, at, e)
#define ALDIS_HPP_DEADLINE_false(Cname, s, armed, at, e) \
    ((void)(s), (void)(armed), (void)(at), (void)(e), std::uint32_t{0})

namespace aldis::detail {

ALDIS_HPP_BINDING(Fop1, fop1, true, true, (s, &in, now, out, cap, &a, e));
ALDIS_HPP_BINDING(Farm1, farm1, true, false, (s, &in, now, out, cap, &a, e));
ALDIS_HPP_BINDING(CltuPacker, cltu_packer, false, false, (s, &in, out, cap, &a, e));
ALDIS_HPP_BINDING(PacketGenerator, packet_generator, true, true, (s, &in, now, out, cap, &a, e));
ALDIS_HPP_BINDING(AccessGenerator, access_generator, false, false, (s, &in, out, cap, &a, e));
ALDIS_HPP_BINDING(BitstreamGenerator, bitstream_generator, false, false,
                  (s, &in, out, cap, &a, e));
ALDIS_HPP_BINDING(MapSegmenter, map_segmenter, true, true, (s, &in, now, out, cap, &a, e));
ALDIS_HPP_BINDING(MapReassembler, map_reassembler, false, false, (s, &in, out, cap, &a, e));
ALDIS_HPP_BINDING(ChannelMux, channel_mux, false, false, (s, &in, out, cap, &a, e));
ALDIS_HPP_BINDING(PacketExtractor, packet_extractor, false, false, (s, &in, out, cap, &a, e));
ALDIS_HPP_BINDING(FrameReceiver, frame_receiver, false, false, (s, &in, out, cap, &a, e));

}  // namespace aldis::detail

#undef ALDIS_HPP_BINDING
#undef ALDIS_HPP_DEADLINE_true
#undef ALDIS_HPP_DEADLINE_false

namespace aldis {

/// FOP-1 (232.1 §5; COP-001). `SentQueueFrames` and `MaxFrameOctets` are the library's
/// `ALDIS_FOP1_SENT_QUEUE_FRAMES` and `ALDIS_FOP1_MAX_FRAME_OCTETS` (COP-015, CABI-030).
template <std::size_t SentQueueFrames, std::size_t MaxFrameOctets>
class Fop1 final : public detail::Handle<detail::Fop1Api> {
    static_assert(SentQueueFrames == ALDIS_FOP1_SENT_QUEUE_FRAMES,
                  "Sent_Queue capacity differs from ALDIS_FOP1_SENT_QUEUE_FRAMES (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_FOP1_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_FOP1_MAX_FRAME_OCTETS (CABI-030)");
};

/// FARM-1 (232.1 §6; COP-002). It has no capacity: it holds no queue.
class Farm1 final : public detail::Handle<detail::Farm1Api> {};

/// The CLTU packer (LNK-044). `MaxFrameOctets` is `ALDIS_LINK_MAX_FRAME_OCTETS`.
template <std::size_t MaxFrameOctets>
class CltuPacker final : public detail::Handle<detail::CltuPackerApi> {
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The packet generator (LNK-010…015). The capacities are `ALDIS_LINK_PACKET_QUEUE_OCTETS` and
/// `ALDIS_LINK_MAX_FRAME_OCTETS`.
template <std::size_t PacketQueueOctets, std::size_t MaxFrameOctets>
class PacketGenerator final : public detail::Handle<detail::PacketGeneratorApi> {
    static_assert(PacketQueueOctets == ALDIS_LINK_PACKET_QUEUE_OCTETS,
                  "queue capacity differs from ALDIS_LINK_PACKET_QUEUE_OCTETS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The access (SDU) generator (LNK-020, LNK-021); capacities as the packet generator.
template <std::size_t PacketQueueOctets, std::size_t MaxFrameOctets>
class AccessGenerator final : public detail::Handle<detail::AccessGeneratorApi> {
    static_assert(PacketQueueOctets == ALDIS_LINK_PACKET_QUEUE_OCTETS,
                  "queue capacity differs from ALDIS_LINK_PACKET_QUEUE_OCTETS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The bitstream generator (LNK-020, LNK-021); capacities as the packet generator.
template <std::size_t PacketQueueOctets, std::size_t MaxFrameOctets>
class BitstreamGenerator final : public detail::Handle<detail::BitstreamGeneratorApi> {
    static_assert(PacketQueueOctets == ALDIS_LINK_PACKET_QUEUE_OCTETS,
                  "queue capacity differs from ALDIS_LINK_PACKET_QUEUE_OCTETS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The MAP segmenter (LNK-060…064); capacities as the packet generator.
template <std::size_t PacketQueueOctets, std::size_t MaxFrameOctets>
class MapSegmenter final : public detail::Handle<detail::MapSegmenterApi> {
    static_assert(PacketQueueOctets == ALDIS_LINK_PACKET_QUEUE_OCTETS,
                  "queue capacity differs from ALDIS_LINK_PACKET_QUEUE_OCTETS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The MAP reassembler (LNK-070…073). The capacities are `ALDIS_LINK_MAX_MAP_CHANNELS` and
/// `ALDIS_LINK_MAX_FRAME_OCTETS`.
template <std::size_t MaxMapChannels, std::size_t MaxFrameOctets>
class MapReassembler final : public detail::Handle<detail::MapReassemblerApi> {
    static_assert(MaxMapChannels == ALDIS_LINK_MAX_MAP_CHANNELS,
                  "MAP channel capacity differs from ALDIS_LINK_MAX_MAP_CHANNELS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The channel multiplexer (LNK-030…036). The capacities are `ALDIS_LINK_MAX_VIRTUAL_CHANNELS`
/// and `ALDIS_LINK_MAX_FRAME_OCTETS`.
template <std::size_t MaxVirtualChannels, std::size_t MaxFrameOctets>
class ChannelMux final : public detail::Handle<detail::ChannelMuxApi> {
    static_assert(MaxVirtualChannels == ALDIS_LINK_MAX_VIRTUAL_CHANNELS,
                  "VC capacity differs from ALDIS_LINK_MAX_VIRTUAL_CHANNELS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The packet extractor (LNK-040…043); capacities as the packet generator.
template <std::size_t PacketQueueOctets, std::size_t MaxFrameOctets>
class PacketExtractor final : public detail::Handle<detail::PacketExtractorApi> {
    static_assert(PacketQueueOctets == ALDIS_LINK_PACKET_QUEUE_OCTETS,
                  "queue capacity differs from ALDIS_LINK_PACKET_QUEUE_OCTETS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

/// The frame receiver (LNK-050…054); capacities as the channel multiplexer.
template <std::size_t MaxVirtualChannels, std::size_t MaxFrameOctets>
class FrameReceiver final : public detail::Handle<detail::FrameReceiverApi> {
    static_assert(MaxVirtualChannels == ALDIS_LINK_MAX_VIRTUAL_CHANNELS,
                  "VC capacity differs from ALDIS_LINK_MAX_VIRTUAL_CHANNELS (CABI-030)");
    static_assert(MaxFrameOctets == ALDIS_LINK_MAX_FRAME_OCTETS,
                  "frame capacity differs from ALDIS_LINK_MAX_FRAME_OCTETS (CABI-030)");
};

}  // namespace aldis

#endif  // ALDIS_HPP
