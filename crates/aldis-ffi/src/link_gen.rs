//! C ABI of the `PacketGenerator`, the `AccessGenerator`, and the `BitstreamGenerator`
//! (LNK-010…015, LNK-020, LNK-021, LNK-003, CABI-030…032, CABI-040; M4 plan D4, WP-7a).
//!
//! Each engine is one opaque handle (`aldis_packet_generator_t`, `aldis_access_generator_t`,
//! `aldis_bitstream_generator_t`): the caller declares the typed storage, `_init` places the
//! engine of the protocol named in the configuration in it (`aldis_link_channel_t::protocol`),
//! and every input is one `_step` call that returns the actions of the input in a
//! fixed-capacity actions struct. The octets of a data field go to the caller's `out` area and
//! the action holds their `aldis_range_t` (API-061). The status and the packet generator's
//! deadline are read-only calls (API-053, API-056); there is no `poll_*` call and no callback.
//! A queue or buffer of an engine holds `ALDIS_LINK_PACKET_QUEUE_OCTETS` octets and a data
//! field `ALDIS_LINK_MAX_FRAME_OCTETS` octets at most, fixed when the library is built
//! (CABI-030). A failed call leaves the engine as it was and writes no actions (API-058).

use aldis::core::Result;
use aldis::core::engine::OutRange;
use aldis::core::time::{Duration, Instant};
use aldis::link::access::{
    ACCESS_GENERATOR_MAX_ACTIONS, AccessActions, AccessGenerator, AccessGeneratorBuffers,
    AccessGeneratorConfig, AccessGeneratorHead, AccessInput, AccessOutput, AccessProtocol,
};
use aldis::link::bitstream::{
    BITSTREAM_GENERATOR_MAX_ACTIONS, BitstreamActions, BitstreamGenerator,
    BitstreamGeneratorBuffers, BitstreamGeneratorConfig, BitstreamGeneratorHead, BitstreamInput,
    BitstreamOutput, BitstreamStatus,
};
use aldis::link::generator::{
    GeneratorActions, GeneratorInput, GeneratorOutput, GeneratorStatus,
    PACKET_GENERATOR_MAX_ACTIONS, PacketGenerator, PacketGeneratorBuffers, PacketGeneratorConfig,
    PacketGeneratorHead,
};
use aldis::link::protocol::{Aos, PacketProtocol, Tm, TmParams, Uslp};

use crate::abi;
use crate::capacities::{ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_PACKET_QUEUE_OCTETS};
use crate::handle::{self, Storage};
use crate::link::{
    RouteFields, aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, aos_params,
    idle_packet, idle_policy, protocol, unsupported_protocol, uslp_params, valid_pvns, vc,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The data field capacity of the engines: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const D: usize = ALDIS_LINK_MAX_FRAME_OCTETS;
/// The queue capacity of the engines: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const Q: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;

/// The most actions one `aldis_packet_generator_step` returns (LNK-074): a data field or the
/// octets a reconfiguration discarded.
pub const ALDIS_PACKET_GENERATOR_MAX_ACTIONS: usize = 1;
/// The most actions one `aldis_access_generator_step` returns (LNK-074).
pub const ALDIS_ACCESS_GENERATOR_MAX_ACTIONS: usize = 1;
/// The most actions one `aldis_bitstream_generator_step` returns (LNK-074).
pub const ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS: usize = 1;

const _: () = assert!(ALDIS_PACKET_GENERATOR_MAX_ACTIONS == PACKET_GENERATOR_MAX_ACTIONS);
const _: () = assert!(ALDIS_ACCESS_GENERATOR_MAX_ACTIONS == ACCESS_GENERATOR_MAX_ACTIONS);
const _: () = assert!(ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS == BITSTREAM_GENERATOR_MAX_ACTIONS);

/// 64-bit words of one `aldis_packet_generator_t` (CABI-031): the tag and the largest of the
/// engines of TM, AOS, and USLP at the build capacities; the build fails when it outgrows it.
pub const ALDIS_PACKET_GENERATOR_WORDS: usize = 1308;
/// 64-bit words of one `aldis_access_generator_t` (CABI-031).
pub const ALDIS_ACCESS_GENERATOR_WORDS: usize = 1040;
/// 64-bit words of one `aldis_bitstream_generator_t` (CABI-031).
pub const ALDIS_BITSTREAM_GENERATOR_WORDS: usize = 1037;

const _: () = assert!(
    size_of::<crate::handle::Slot<PacketGeneratorState>>() <= ALDIS_PACKET_GENERATOR_WORDS * 8
);
const _: () = assert!(
    size_of::<crate::handle::Slot<AccessGeneratorState>>() <= ALDIS_ACCESS_GENERATOR_WORDS * 8
);
const _: () = assert!(
    size_of::<crate::handle::Slot<BitstreamGeneratorState>>()
        <= ALDIS_BITSTREAM_GENERATOR_WORDS * 8
);

// ---- Building in place ----

/// Builds an engine at `engine` from its head, without an engine on the stack (CABI-040, #240;
/// the pattern of `aldis::link::inplace`): the head is written at the offset of its field and the
/// octet buffers are zeroed at theirs, which is their empty state. Every field of the engine is
/// then initialized. The arguments are the engine type `$e`, its head type `$h` and buffers type
/// `$b`, the pointer, and the head.
macro_rules! engine_in_place {
    ($e:ty, $h:ty, $b:ty, $ptr:expr, $head:expr) => {{
        const {
            assert!(
                <$e>::HEAD_OFFSET.saturating_add(size_of::<$h>()) <= <$e>::BUFFERS_OFFSET
                    && <$e>::BUFFERS_OFFSET.saturating_add(size_of::<$b>()) <= size_of::<$e>()
            );
        };
        let engine: *mut $e = $ptr;
        // SAFETY: `$ptr` is valid for writes of one `$e` (the macro's caller). `$e` is
        // `repr(C)` with the fields head (`$h`) and buffers (`$b`) in that order, at the
        // offsets `HEAD_OFFSET` and `BUFFERS_OFFSET` (`offset_of!`), and the assertion above
        // keeps both in the engine and apart. The buffers are plain arrays of octets, valid
        // when zeroed. The writes go through raw pointers, so no reference to uninitialized
        // memory is made.
        unsafe {
            engine.byte_add(<$e>::HEAD_OFFSET).cast::<$h>().write($head);
            engine
                .byte_add(<$e>::BUFFERS_OFFSET)
                .cast::<u8>()
                .write_bytes(0, size_of::<$b>());
        }
    }};
}

pub(crate) use engine_in_place;

/// The `repr(u8)` layout of an enum variant with one field (RFC 2195): the tag, then the field.
#[repr(C)]
struct Tagged<T> {
    tag: u8,
    payload: T,
}

/// Writes the tag `tag` of the variant of the `repr(u8)` enum `S` whose field is a `T`, and
/// returns a pointer to the field, which the caller then initializes in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `S`; `S` is a `repr(u8)` enum, `tag` the discriminant of
/// the variant, and `T` the type of that variant's only field.
#[expect(unsafe_code, reason = "initializes an enum variant in place (GEN-071)")]
pub(crate) unsafe fn variant_payload<S, T>(state: *mut S, tag: u8) -> *mut T {
    const { assert!(size_of::<Tagged<T>>() <= size_of::<S>()) };
    let tagged = state.cast::<Tagged<T>>();
    // SAFETY: by the contract, `S` has the layout of a union of `repr(C)` structs of tag and
    // field, so `Tagged<T>` is the layout of this variant and in bounds. The writes are through
    // raw field pointers.
    unsafe {
        (&raw mut (*tagged).tag).write(tag);
        &raw mut (*tagged).payload
    }
}

// ---- PacketGenerator ----

type Pg<P> = PacketGenerator<P, D, Q>;

/// The `PacketGenerator` of the protocol chosen at `aldis_packet_generator_init` (LNK-001).
///
/// `repr(u8)`: `aldis_packet_generator_init` writes a variant in place (`variant_payload`).
#[repr(u8)]
#[expect(
    dead_code,
    reason = "the variants are written in place by the init function, not constructed"
)]
pub(crate) enum PacketGeneratorState {
    /// TM.
    Tm(Pg<Tm>) = 0,
    /// AOS.
    Aos(Pg<Aos>) = 1,
    /// USLP.
    Uslp(Pg<Uslp>) = 2,
}

/// The kind of an `aldis_packet_generator_input_t` (LNK-010…LNK-015, LNK-003; API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_packet_generator_input {
    /// A whole packet (`data`): LNK-010, LNK-015.
    ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET = 0,
    /// Give out the next releasable field, or the idle field of the idle policy (LNK-012,
    /// LNK-014).
    ALDIS_PACKET_GENERATOR_INPUT_RELEASE = 1,
    /// The release deadline of the started field may have passed (API-053, LNK-012).
    ALDIS_PACKET_GENERATOR_INPUT_TIMEOUT = 2,
    /// Replace the configuration (`config`) between data units, discarding the field and the
    /// queue (LNK-003). The protocol cannot change.
    ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE = 3,
}
mirror!(pub aldis_packet_generator_input: u32 {
    ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET,
    ALDIS_PACKET_GENERATOR_INPUT_RELEASE,
    ALDIS_PACKET_GENERATOR_INPUT_TIMEOUT,
    ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE
});

/// The kind of an `aldis_packet_generator_action_t` (API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_packet_generator_action {
    /// A data field, in `out` (LNK-011).
    ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD = 0,
    /// A reconfiguration discarded `octets` (LNK-003).
    ALDIS_PACKET_GENERATOR_ACTION_DISCARDED = 1,
}
mirror!(pub aldis_packet_generator_action: u32 {
    ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD,
    ALDIS_PACKET_GENERATOR_ACTION_DISCARDED
});

/// A `PacketGenerator` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-010, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_packet_generator_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_PACKET_GENERATOR_WORDS],
}

impl Storage for aldis_packet_generator_t {
    type State = PacketGeneratorState;
    const MAGIC: u32 = 0x414C_5047; // "ALPG"
}

/// The managed parameters of a `PacketGenerator` (132.0 §5.4 Table 5-4; 732.0 §5.5 Table 5-4;
/// 732.1 §5.6 Table 5-5; LNK-010, LNK-012…LNK-014, LNK-078).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_generator_config_t {
    /// The protocol and the channel (TM, AOS, or USLP).
    pub channel: aldis_link_channel_t,
    /// The octets of the data field, header included (TM data field, AOS `M_PDU`, USLP TFDF),
    /// at most `ALDIS_LINK_MAX_FRAME_OCTETS` and what the pointer of the protocol addresses.
    pub field_len: usize,
    /// 1 when Space Packets (PVN 0) are valid, else 0.
    pub space_packets: u8,
    /// 1 when Encapsulation Packets (PVN 7) are valid, else 0. At least one of the two is 1.
    pub encapsulation_packets: u8,
    /// The longest packet in octets, at least 1.
    pub max_packet_len: usize,
    /// `ALDIS_LINK_IDLE_POLICY_*` (LNK-014).
    pub idle_policy: u32,
    /// `ALDIS_LINK_IDLE_PACKET_*`; ignored for USLP, which fixes the Encapsulation Idle Packet
    /// (LNK-013).
    pub idle_packet_kind: u32,
    /// The idle data octet.
    pub idle_pattern: u8,
    /// The release deadline in microseconds, 0 for none (732.1 §5.4 Table 5-3; LNK-012).
    pub release_deadline_us: u64,
}

/// One input of a `PacketGenerator` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_packet_generator_input_t {
    /// `ALDIS_PACKET_GENERATOR_INPUT_*`.
    pub kind: u32,
    /// For `SUBMIT_PACKET`: the packet, read once and copied (API-060). `data` is checked on
    /// every kind and must not overlap the storage or `out`: `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_packet_generator_config_t,
}

/// One action of a `PacketGenerator` (API-057, API-061). A field that the kind does not use is
/// 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_generator_action_t {
    /// `ALDIS_PACKET_GENERATOR_ACTION_*`.
    pub kind: u32,
    /// For `DATA_FIELD`: the VCID.
    pub vc_id: u8,
    /// For `DATA_FIELD`: where the field is in `out`.
    pub field: aldis_range_t,
    /// For `DATA_FIELD`: the raw First Header Pointer of the field.
    pub pointer: u16,
    /// For `DATA_FIELD`: 1 when the field has a MAP and `QoS` (USLP), else 0.
    pub route_present: u8,
    /// For `DATA_FIELD` with a route: the MAP ID.
    pub map_id: u8,
    /// For `DATA_FIELD` with a route: `ALDIS_LINK_QOS_*`.
    pub qos: u32,
    /// For `DISCARDED`: the octets discarded.
    pub octets: usize,
}

/// The actions of one input of a `PacketGenerator`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_generator_actions_t {
    /// The number of actions in `items`, at most `ALDIS_PACKET_GENERATOR_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_packet_generator_action_t; ALDIS_PACKET_GENERATOR_MAX_ACTIONS],
}

/// A read-only snapshot of a `PacketGenerator` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_generator_status_t {
    /// Packets taken.
    pub packets_in: u64,
    /// Data fields given out, idle fields included.
    pub fields_out: u64,
    /// Idle packets generated to complete a field (LNK-013).
    pub idle_packets: u64,
    /// Idle fields given out under the idle-field policy (LNK-014).
    pub idle_fields: u64,
    /// Packets refused as invalid (LNK-010).
    pub refused: u64,
    /// Packets refused with `ALDIS_ERR_WOULD_BLOCK` (LNK-015).
    pub would_block: u64,
    /// Octets discarded by a reconfiguration (LNK-003).
    pub discarded_octets: u64,
    /// The packet queue.
    pub queue: aldis_link_occupancy_t,
    /// The field being assembled.
    pub field: aldis_link_occupancy_t,
}

/// The `PacketGenerator` configuration of `c` for the protocol `P` with the parameters
/// `params`.
fn pg_config<P: PacketProtocol>(
    c: &aldis_packet_generator_config_t,
    params: P::Params,
) -> Result<PacketGeneratorConfig<P>> {
    let mut config = PacketGeneratorConfig::<P>::new(
        vc::<P>(&c.channel)?,
        params,
        c.field_len,
        valid_pvns(c.space_packets, c.encapsulation_packets)?,
        c.max_packet_len,
    )?
    .with_idle(
        idle_policy(c.idle_policy)?,
        idle_packet(c.idle_packet_kind)?,
        c.idle_pattern,
    )?;
    if c.release_deadline_us != 0 {
        config = config.with_release_deadline(Duration::from_micros(c.release_deadline_us));
    }
    Ok(config)
}

/// The head of the engine of `c`; the protocol of the engine is fixed here (LNK-001).
enum PgHead {
    Tm(PacketGeneratorHead<Tm>),
    Aos(PacketGeneratorHead<Aos>),
    Uslp(PacketGeneratorHead<Uslp>),
}

fn pg_head(c: &aldis_packet_generator_config_t) -> Result<PgHead> {
    match protocol(&c.channel)? {
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => {
            Ok(PgHead::Tm(Pg::new_head(pg_config(c, TmParams)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => Ok(PgHead::Aos(Pg::new_head(pg_config(
            c,
            aos_params(&c.channel)?,
        )?)?)),
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => Ok(PgHead::Uslp(Pg::new_head(
            pg_config(c, uslp_params(&c.channel)?)?,
        )?)),
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => Err(unsupported_protocol()),
    }
}

/// Builds the engine of `head` at `state`, in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `PacketGeneratorState`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
unsafe fn pg_place(state: *mut PacketGeneratorState, head: PgHead) {
    type B = PacketGeneratorBuffers<D, Q>;
    match head {
        PgHead::Tm(h) => {
            // SAFETY: as the contract says; the tag 0 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Pg<Tm>>(state, 0) };
            engine_in_place!(Pg<Tm>, PacketGeneratorHead<Tm>, B, p, h);
        }
        PgHead::Aos(h) => {
            // SAFETY: as the contract says; the tag 1 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Pg<Aos>>(state, 1) };
            engine_in_place!(Pg<Aos>, PacketGeneratorHead<Aos>, B, p, h);
        }
        PgHead::Uslp(h) => {
            // SAFETY: as the contract says; the tag 2 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Pg<Uslp>>(state, 2) };
            engine_in_place!(Pg<Uslp>, PacketGeneratorHead<Uslp>, B, p, h);
        }
    }
}

/// One step of `engine`, in the protocol `P` (API-057).
fn pg_run<P: PacketProtocol>(
    engine: &mut Pg<P>,
    input: &aldis_packet_generator_input_t,
    data: &[u8],
    params: Result<P::Params>,
    now: Instant,
    area: &mut [u8],
) -> Result<GeneratorActions> {
    let input = match parse::<aldis_packet_generator_input>(input.kind)? {
        aldis_packet_generator_input::ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET => {
            GeneratorInput::SubmitPacket(data)
        }
        aldis_packet_generator_input::ALDIS_PACKET_GENERATOR_INPUT_RELEASE => {
            GeneratorInput::Release
        }
        aldis_packet_generator_input::ALDIS_PACKET_GENERATOR_INPUT_TIMEOUT => {
            GeneratorInput::Timeout
        }
        aldis_packet_generator_input::ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE => {
            GeneratorInput::Reconfigure(pg_config(&input.config, params?)?)
        }
    };
    engine.step(input, now, area)
}

impl PacketGeneratorState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_packet_generator_input_t,
        data: &[u8],
        now: Instant,
        area: &mut [u8],
    ) -> Result<GeneratorActions> {
        let reconfigure = parse::<aldis_packet_generator_input>(input.kind)?
            == aldis_packet_generator_input::ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE;
        let ch = &input.config.channel;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want| protocol(ch).map(|p| p == want);
        match self {
            PacketGeneratorState::Tm(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM)? {
                    return Err(unsupported_protocol());
                }
                pg_run(e, input, data, Ok(TmParams), now, area)
            }
            PacketGeneratorState::Aos(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS)? {
                    return Err(unsupported_protocol());
                }
                pg_run(e, input, data, aos_params(ch), now, area)
            }
            PacketGeneratorState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                pg_run(e, input, data, uslp_params(ch), now, area)
            }
        }
    }

    const fn status(&self) -> GeneratorStatus {
        match self {
            PacketGeneratorState::Tm(e) => *e.status(),
            PacketGeneratorState::Aos(e) => *e.status(),
            PacketGeneratorState::Uslp(e) => *e.status(),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        match self {
            PacketGeneratorState::Tm(e) => e.next_deadline(),
            PacketGeneratorState::Aos(e) => e.next_deadline(),
            PacketGeneratorState::Uslp(e) => e.next_deadline(),
        }
    }
}

/// The C form of a range of `out`.
pub(crate) const fn range(r: OutRange) -> aldis_range_t {
    aldis_range_t {
        offset: r.start(),
        len: r.len(),
    }
}

fn pg_action(output: &GeneratorOutput) -> aldis_packet_generator_action_t {
    match output {
        GeneratorOutput::DataField(f) => {
            let route = RouteFields::from(f.route);
            aldis_packet_generator_action_t {
                kind: ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD,
                vc_id: f.vc,
                field: range(f.field),
                pointer: f.pointer,
                route_present: route.present,
                map_id: route.map_id,
                qos: route.qos,
                octets: 0,
            }
        }
        GeneratorOutput::Discarded { octets } => aldis_packet_generator_action_t {
            kind: ALDIS_PACKET_GENERATOR_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_packet_generator_action_t::default()
        },
    }
}

/// Places a `PacketGenerator` with an empty field and queue in `storage` (LNK-010; CABI-031,
/// CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0
/// or 1, or a constant that is none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TC,
/// (`valid_pvns`, `max_packet_len`, `field_len`, `idle_packet_kind`) for a value the
/// configuration relations of LNK-078 refuse; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `upid`,
/// `map_id`); `ALDIS_ERR_CAPACITY_EXCEEDED` for a field or packet length above the build
/// capacities (LNK-015).
///
/// # Safety
/// `storage` is valid for writes of one `aldis_packet_generator_t` and outlives its use; `cfg`
/// is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_generator_init(
    storage: *mut aldis_packet_generator_t,
    cfg: *const aldis_packet_generator_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let head = pg_head(&cfg)?;
            // `init_with` passes a pointer to the state in the slot, valid for writes.
            handle::init_with(storage, |state| pg_place(state, head))
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn pg_engine(
    h: *mut aldis_packet_generator_t,
) -> Result<handle::Handle<PacketGeneratorState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `PacketGenerator` (API-057, CABI-040).
///
/// `now_us` is the time in microseconds (API-003). The octets of a data field go to `out`.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a
/// constant that is none; for `SUBMIT_PACKET`: `ALDIS_ERR_INVALID_FIELD` (`packet_version`),
/// `ALDIS_ERR_LENGTH_MISMATCH` (`packet_len`), `ALDIS_ERR_OUT_OF_RANGE` (`max_packet_len`),
/// `ALDIS_ERR_WOULD_BLOCK`; for `RELEASE`: `ALDIS_ERR_BUFFER_TOO_SMALL`; for `RECONFIGURE`: as
/// `aldis_packet_generator_init`, and `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another
/// protocol. The engine is unchanged and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_packet_generator_init` whose storage is still valid, or
/// detectably invalid storage; `input` is valid for a read, and its `data` for `data_len`
/// octets; `out` is valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0);
/// `actions` is valid for one write; `err` is `NULL` or valid for one write. No other thread
/// uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_generator_step(
    h: *mut aldis_packet_generator_t,
    input: *const aldis_packet_generator_input_t,
    now_us: u64,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_packet_generator_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = pg_engine(h)?;
            let value = abi::read(input)?;
            let data = abi::bytes_disjoint(value.data, value.data_len, &[h.range()])?;
            let area = abi::bytes_mut(out, out_cap, &[(value.data, value.data_len), h.range()])?;
            let actions = abi::out_disjoint(
                actions,
                &[
                    h.range(),
                    (value.data, value.data_len),
                    (out.cast_const(), out_cap),
                ],
            )?;
            let list =
                h.update(|state| state.step(&value, data, Instant::from_micros(now_us), area))?;
            let mut result = aldis_packet_generator_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = pg_action(output);
            }
            // `list` holds at most `PACKET_GENERATOR_MAX_ACTIONS` actions, the length of `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the release deadline of the started field (LNK-012, API-053, CABI-040).
///
/// While a deadline is pending, `armed` is 1 and `deadline_us` the instant in microseconds at
/// which the caller is to feed the `TIMEOUT` input; otherwise both are 0. It changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_packet_generator_step`]; `armed` and `deadline_us` are valid for one write
/// each.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_generator_next_deadline(
    h: *mut aldis_packet_generator_t,
    armed: *mut u8,
    deadline_us: *mut u64,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = pg_engine(h)?;
            let armed = abi::out_disjoint(armed, &[h.range()])?;
            let deadline_us = abi::out_disjoint(deadline_us, &[h.range()])?;
            let deadline = h.read(PacketGeneratorState::next_deadline);
            armed.write(u8::from(deadline.is_some()));
            deadline_us.write(deadline.map_or(0, Instant::as_micros));
            Ok(())
        })
    }
}

/// Reads the counters and the occupancy of the `PacketGenerator` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_packet_generator_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_generator_status(
    h: *mut aldis_packet_generator_t,
    status: *mut aldis_packet_generator_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = pg_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(PacketGeneratorState::status);
            status.write(aldis_packet_generator_status_t {
                packets_in: s.packets_in.get(),
                fields_out: s.fields_out.get(),
                idle_packets: s.idle_packets.get(),
                idle_fields: s.idle_fields.get(),
                refused: s.refused.get(),
                would_block: s.would_block.get(),
                discarded_octets: s.discarded_octets.get(),
                queue: s.queue.into(),
                field: s.field.into(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_packet_generator_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_generator_deinit(h: *mut aldis_packet_generator_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}

// ---- AccessGenerator ----

type Ag<P> = AccessGenerator<P, Q>;

/// The `AccessGenerator` of the protocol chosen at `aldis_access_generator_init` (LNK-001).
///
/// `repr(u8)`: `aldis_access_generator_init` writes a variant in place (`variant_payload`).
#[repr(u8)]
#[expect(
    dead_code,
    reason = "the variants are written in place by the init function, not constructed"
)]
pub(crate) enum AccessGeneratorState {
    /// TM.
    Tm(Ag<Tm>) = 0,
    /// AOS.
    Aos(Ag<Aos>) = 1,
    /// USLP.
    Uslp(Ag<Uslp>) = 2,
}

/// The kind of an `aldis_access_generator_input_t` (LNK-020, LNK-003; API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_access_generator_input {
    /// A whole SDU (`data`): LNK-020.
    ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU = 0,
    /// Give out the next field (LNK-020).
    ALDIS_ACCESS_GENERATOR_INPUT_RELEASE = 1,
    /// Replace the configuration (`config`) between data units, discarding the queue
    /// (LNK-003). The protocol cannot change.
    ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE = 2,
}
mirror!(pub aldis_access_generator_input: u32 {
    ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU,
    ALDIS_ACCESS_GENERATOR_INPUT_RELEASE,
    ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE
});

/// The kind of an `aldis_access_generator_action_t` (API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_access_generator_action {
    /// A data field, in `out` (LNK-020).
    ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD = 0,
    /// A reconfiguration discarded `octets` (LNK-003).
    ALDIS_ACCESS_GENERATOR_ACTION_DISCARDED = 1,
}
mirror!(pub aldis_access_generator_action: u32 {
    ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD,
    ALDIS_ACCESS_GENERATOR_ACTION_DISCARDED
});

/// An `AccessGenerator` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-020, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_access_generator_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_ACCESS_GENERATOR_WORDS],
}

impl Storage for aldis_access_generator_t {
    type State = AccessGeneratorState;
    const MAGIC: u32 = 0x414C_4147; // "ALAG"
}

/// The managed parameters of an `AccessGenerator` (LNK-020, LNK-078).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_access_generator_config_t {
    /// The protocol and the channel (TM, AOS, or USLP).
    pub channel: aldis_link_channel_t,
    /// The octets of the data field, header included (TM data field, AOS `VCA_SDU`, USLP
    /// TFDF), whose zone holds 1 to 65 535 octets, and which fits `ALDIS_LINK_PACKET_QUEUE_OCTETS`
    /// with the length of an SDU.
    pub field_len: usize,
    /// The longest SDU: 0 for the zone of the field; TM and AOS take exactly the zone (LNK-020).
    pub max_sdu_len: usize,
    /// The octet that fills the zone after the end of an SDU.
    pub idle_pattern: u8,
}

/// One input of an `AccessGenerator` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_access_generator_input_t {
    /// `ALDIS_ACCESS_GENERATOR_INPUT_*`.
    pub kind: u32,
    /// For `SUBMIT_SDU`: the SDU, read once and copied (API-060). `data` is checked on every kind and must not overlap
    /// the storage or `out`: `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_access_generator_config_t,
}

/// One action of an `AccessGenerator` (API-057, API-061). A field that the kind does not use is
/// 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_access_generator_action_t {
    /// `ALDIS_ACCESS_GENERATOR_ACTION_*`.
    pub kind: u32,
    /// For `DATA_FIELD`: the VCID.
    pub vc_id: u8,
    /// For `DATA_FIELD`: where the field is in `out`.
    pub field: aldis_range_t,
    /// For `DATA_FIELD`: 1 when the field has a Last Valid Octet Pointer (USLP), else 0.
    pub pointer_present: u8,
    /// For `DATA_FIELD` with a pointer: the raw Last Valid Octet Pointer.
    pub pointer: u16,
    /// For `DATA_FIELD`: 1 when the field has a MAP and `QoS` (USLP), else 0.
    pub route_present: u8,
    /// For `DATA_FIELD` with a route: the MAP ID.
    pub map_id: u8,
    /// For `DATA_FIELD` with a route: `ALDIS_LINK_QOS_*`.
    pub qos: u32,
    /// For `DISCARDED`: the octets discarded.
    pub octets: usize,
}

/// The actions of one input of an `AccessGenerator`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_access_generator_actions_t {
    /// The number of actions in `items`, at most `ALDIS_ACCESS_GENERATOR_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_access_generator_action_t; ALDIS_ACCESS_GENERATOR_MAX_ACTIONS],
}

/// A read-only snapshot of an `AccessGenerator` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_access_generator_status_t {
    /// SDUs taken.
    pub sdus_in: u64,
    /// Data fields given out.
    pub fields_out: u64,
    /// SDUs refused as invalid (LNK-020).
    pub refused: u64,
    /// SDUs refused with `ALDIS_ERR_WOULD_BLOCK`.
    pub would_block: u64,
    /// Octets discarded by a reconfiguration (LNK-003).
    pub discarded_octets: u64,
    /// The SDU queue.
    pub queue: aldis_link_occupancy_t,
}

/// The `AccessGenerator` configuration of `c` for the protocol `P` with the parameters `params`.
fn ag_config<P: AccessProtocol>(
    c: &aldis_access_generator_config_t,
    params: P::Params,
) -> Result<AccessGeneratorConfig<P>> {
    let mut config = AccessGeneratorConfig::<P>::new(vc::<P>(&c.channel)?, params, c.field_len)?;
    if c.max_sdu_len != 0 {
        config = config.with_max_sdu_len(c.max_sdu_len)?;
    }
    Ok(config.with_idle_pattern(c.idle_pattern))
}

/// The head of the engine of `c`; the protocol of the engine is fixed here (LNK-001).
enum AgHead {
    Tm(AccessGeneratorHead<Tm>),
    Aos(AccessGeneratorHead<Aos>),
    Uslp(AccessGeneratorHead<Uslp>),
}

fn ag_head(c: &aldis_access_generator_config_t) -> Result<AgHead> {
    match protocol(&c.channel)? {
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => {
            Ok(AgHead::Tm(Ag::new_head(ag_config(c, ())?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => {
            Ok(AgHead::Aos(Ag::new_head(ag_config(c, ())?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => Ok(AgHead::Uslp(Ag::new_head(
            ag_config(c, uslp_params(&c.channel)?)?,
        )?)),
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => Err(unsupported_protocol()),
    }
}

/// Builds the engine of `head` at `state`, in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `AccessGeneratorState`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
unsafe fn ag_place(state: *mut AccessGeneratorState, head: AgHead) {
    type B = AccessGeneratorBuffers<Q>;
    match head {
        AgHead::Tm(h) => {
            // SAFETY: as the contract says; the tag 0 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Ag<Tm>>(state, 0) };
            engine_in_place!(Ag<Tm>, AccessGeneratorHead<Tm>, B, p, h);
        }
        AgHead::Aos(h) => {
            // SAFETY: as the contract says; the tag 1 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Ag<Aos>>(state, 1) };
            engine_in_place!(Ag<Aos>, AccessGeneratorHead<Aos>, B, p, h);
        }
        AgHead::Uslp(h) => {
            // SAFETY: as the contract says; the tag 2 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Ag<Uslp>>(state, 2) };
            engine_in_place!(Ag<Uslp>, AccessGeneratorHead<Uslp>, B, p, h);
        }
    }
}

/// One step of `engine`, in the protocol `P` (API-057).
fn ag_run<P: AccessProtocol>(
    engine: &mut Ag<P>,
    input: &aldis_access_generator_input_t,
    data: &[u8],
    params: Result<P::Params>,
    area: &mut [u8],
) -> Result<AccessActions> {
    let input = match parse::<aldis_access_generator_input>(input.kind)? {
        aldis_access_generator_input::ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU => {
            AccessInput::SubmitSdu(data)
        }
        aldis_access_generator_input::ALDIS_ACCESS_GENERATOR_INPUT_RELEASE => AccessInput::Release,
        aldis_access_generator_input::ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE => {
            AccessInput::Reconfigure(ag_config(&input.config, params?)?)
        }
    };
    engine.step(input, area)
}

/// Fails unless a reconfiguration names the protocol of the engine (LNK-003).
fn keeps_protocol(input: &aldis_access_generator_input_t, want: aldis_link_protocol) -> Result<()> {
    let reconfigure = parse::<aldis_access_generator_input>(input.kind)?
        == aldis_access_generator_input::ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE;
    if reconfigure && protocol(&input.config.channel)? != want {
        return Err(unsupported_protocol());
    }
    Ok(())
}

impl AccessGeneratorState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_access_generator_input_t,
        data: &[u8],
        area: &mut [u8],
    ) -> Result<AccessActions> {
        let ch = &input.config.channel;
        match self {
            AccessGeneratorState::Tm(e) => {
                keeps_protocol(input, aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM)?;
                ag_run(e, input, data, Ok(()), area)
            }
            AccessGeneratorState::Aos(e) => {
                keeps_protocol(input, aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS)?;
                ag_run(e, input, data, Ok(()), area)
            }
            AccessGeneratorState::Uslp(e) => {
                keeps_protocol(input, aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)?;
                ag_run(e, input, data, uslp_params(ch), area)
            }
        }
    }

    fn status(&self) -> aldis_access_generator_status_t {
        let s = match self {
            AccessGeneratorState::Tm(e) => *e.status(),
            AccessGeneratorState::Aos(e) => *e.status(),
            AccessGeneratorState::Uslp(e) => *e.status(),
        };
        aldis_access_generator_status_t {
            sdus_in: s.sdus_in.get(),
            fields_out: s.fields_out.get(),
            refused: s.refused.get(),
            would_block: s.would_block.get(),
            discarded_octets: s.discarded_octets.get(),
            queue: s.queue.into(),
        }
    }
}

fn ag_action(output: &AccessOutput) -> aldis_access_generator_action_t {
    match output {
        AccessOutput::DataField(f) => {
            let route = RouteFields::from(f.route);
            aldis_access_generator_action_t {
                kind: ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD,
                vc_id: f.vc,
                field: range(f.field),
                pointer_present: u8::from(f.pointer.is_some()),
                pointer: f.pointer.unwrap_or_default(),
                route_present: route.present,
                map_id: route.map_id,
                qos: route.qos,
                octets: 0,
            }
        }
        AccessOutput::Discarded { octets } => aldis_access_generator_action_t {
            kind: ALDIS_ACCESS_GENERATOR_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_access_generator_action_t::default()
        },
    }
}

/// Places an `AccessGenerator` with an empty queue in `storage` (LNK-020; CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, or a constant that is
/// none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TC, (`field_len`, `max_sdu_len`) for a
/// value LNK-078 refuses; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `upid`, `map_id`);
/// `ALDIS_ERR_CAPACITY_EXCEEDED` (`max_sdu_len`) when the longest SDU and its length do not fit
/// `ALDIS_LINK_PACKET_QUEUE_OCTETS`.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_access_generator_t` and outlives its use; `cfg`
/// is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_access_generator_init(
    storage: *mut aldis_access_generator_t,
    cfg: *const aldis_access_generator_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let head = ag_head(&cfg)?;
            // `init_with` passes a pointer to the state in the slot, valid for writes.
            handle::init_with(storage, |state| ag_place(state, head))
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn ag_engine(
    h: *mut aldis_access_generator_t,
) -> Result<handle::Handle<AccessGeneratorState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `AccessGenerator` (API-057, CABI-040). The octets of a data field go
/// to `out`; the generator has no clock.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a constant
/// that is none; for `SUBMIT_SDU` and `RELEASE`: the errors of `AccessGenerator::step`
/// (`ALDIS_ERR_LENGTH_MISMATCH`, `ALDIS_ERR_OUT_OF_RANGE`, `ALDIS_ERR_WOULD_BLOCK`,
/// `ALDIS_ERR_BUFFER_TOO_SMALL`); for `RECONFIGURE`: as `aldis_access_generator_init`, and
/// `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another protocol. The engine is unchanged
/// and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_access_generator_init` whose storage is still valid, or
/// detectably invalid storage; `input` is valid for a read, and its `data` for `data_len`
/// octets; `out` is valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0);
/// `actions` is valid for one write; `err` is `NULL` or valid for one write. No other thread
/// uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_access_generator_step(
    h: *mut aldis_access_generator_t,
    input: *const aldis_access_generator_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_access_generator_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = ag_engine(h)?;
            let value = abi::read(input)?;
            let data = abi::bytes_disjoint(value.data, value.data_len, &[h.range()])?;
            let area = abi::bytes_mut(out, out_cap, &[(value.data, value.data_len), h.range()])?;
            let actions = abi::out_disjoint(
                actions,
                &[
                    h.range(),
                    (value.data, value.data_len),
                    (out.cast_const(), out_cap),
                ],
            )?;
            let list = h.update(|state| state.step(&value, data, area))?;
            let mut result = aldis_access_generator_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = ag_action(output);
            }
            // `list` holds at most `ACCESS_GENERATOR_MAX_ACTIONS` actions, the length of `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the counters and the occupancy of the `AccessGenerator` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_access_generator_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_access_generator_status(
    h: *mut aldis_access_generator_t,
    status: *mut aldis_access_generator_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = ag_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            status.write(h.read(AccessGeneratorState::status));
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_access_generator_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_access_generator_deinit(h: *mut aldis_access_generator_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}

// ---- BitstreamGenerator ----

/// The `BitstreamGenerator` of AOS (LNK-021; the only protocol with a Bitstream Service).
pub(crate) type BitstreamGeneratorState = BitstreamGenerator<Aos, Q>;

/// The kind of an `aldis_bitstream_generator_input_t` (LNK-021, LNK-003; API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_bitstream_generator_input {
    /// Bits for the stream (`data`, `bit_len`): LNK-021.
    ALDIS_BITSTREAM_GENERATOR_INPUT_SUBMIT_BITS = 0,
    /// Give out the next `B_PDU` (LNK-021).
    ALDIS_BITSTREAM_GENERATOR_INPUT_RELEASE = 1,
    /// Replace the configuration (`config`) between data units, discarding the queued bits
    /// (LNK-003).
    ALDIS_BITSTREAM_GENERATOR_INPUT_RECONFIGURE = 2,
}
mirror!(pub aldis_bitstream_generator_input: u32 {
    ALDIS_BITSTREAM_GENERATOR_INPUT_SUBMIT_BITS,
    ALDIS_BITSTREAM_GENERATOR_INPUT_RELEASE,
    ALDIS_BITSTREAM_GENERATOR_INPUT_RECONFIGURE
});

/// The kind of an `aldis_bitstream_generator_action_t` (API-057).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_bitstream_generator_action {
    /// A `B_PDU`, in `out` (LNK-021).
    ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD = 0,
    /// A reconfiguration discarded `octets` (LNK-003).
    ALDIS_BITSTREAM_GENERATOR_ACTION_DISCARDED = 1,
}
mirror!(pub aldis_bitstream_generator_action: u32 {
    ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD,
    ALDIS_BITSTREAM_GENERATOR_ACTION_DISCARDED
});

/// A `BitstreamGenerator` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-021, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_bitstream_generator_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_BITSTREAM_GENERATOR_WORDS],
}

impl Storage for aldis_bitstream_generator_t {
    type State = BitstreamGeneratorState;
    const MAGIC: u32 = 0x414C_4247; // "ALBG"
}

/// The managed parameters of a `BitstreamGenerator` (732.0 §4.1.4.3, §5.5; LNK-021, LNK-078).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_bitstream_generator_config_t {
    /// The protocol (AOS) and the channel.
    pub channel: aldis_link_channel_t,
    /// The octets of the `B_PDU`, header included; its zone holds 1 to 2 047 octets.
    pub field_len: usize,
    /// The octet whose bits complete a zone that is not full.
    pub idle_pattern: u8,
}

/// One input of a `BitstreamGenerator` (API-057, CABI-040). A field that the kind does not use
/// is ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_bitstream_generator_input_t {
    /// `ALDIS_BITSTREAM_GENERATOR_INPUT_*`.
    pub kind: u32,
    /// For `SUBMIT_BITS`: the bits, most significant first and left-aligned, read once and
    /// copied (API-060). `data` is checked on every kind and must not overlap the storage or
    /// `out`: `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `SUBMIT_BITS`: how many bits of `data` to take, at least 1.
    pub bit_len: usize,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_bitstream_generator_config_t,
}

/// One action of a `BitstreamGenerator` (API-057, API-061). A field that the kind does not use
/// is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_bitstream_generator_action_t {
    /// `ALDIS_BITSTREAM_GENERATOR_ACTION_*`.
    pub kind: u32,
    /// For `DATA_FIELD`: the VCID.
    pub vc_id: u8,
    /// For `DATA_FIELD`: where the `B_PDU` is in `out`.
    pub field: aldis_range_t,
    /// For `DATA_FIELD`: the raw Bitstream Data Pointer.
    pub pointer: u16,
    /// For `DISCARDED`: the octets discarded.
    pub octets: usize,
}

/// The actions of one input of a `BitstreamGenerator`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_bitstream_generator_actions_t {
    /// The number of actions in `items`, at most `ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_bitstream_generator_action_t; ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS],
}

/// A read-only snapshot of a `BitstreamGenerator` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_bitstream_generator_status_t {
    /// Submissions taken.
    pub submissions: u64,
    /// `B_PDUs` given out, idle ones included.
    pub fields_out: u64,
    /// Idle `B_PDUs` given out: no bit was queued.
    pub idle_fields: u64,
    /// Submissions refused as invalid.
    pub refused: u64,
    /// Submissions refused with `ALDIS_ERR_WOULD_BLOCK`.
    pub would_block: u64,
    /// Octets discarded by a reconfiguration (LNK-003).
    pub discarded_octets: u64,
    /// The bit queue, in octets.
    pub queue: aldis_link_occupancy_t,
}

/// The `BitstreamGenerator` configuration of `c`: AOS only.
fn bg_config(c: &aldis_bitstream_generator_config_t) -> Result<BitstreamGeneratorConfig<Aos>> {
    if protocol(&c.channel)? != aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS {
        return Err(unsupported_protocol());
    }
    BitstreamGeneratorConfig::new(vc::<Aos>(&c.channel)?, c.field_len, c.idle_pattern)
}

fn bg_action(output: &BitstreamOutput) -> aldis_bitstream_generator_action_t {
    match output {
        BitstreamOutput::DataField(f) => aldis_bitstream_generator_action_t {
            kind: ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD,
            vc_id: f.vc,
            field: range(f.field),
            pointer: f.pointer,
            octets: 0,
        },
        BitstreamOutput::Discarded { octets } => aldis_bitstream_generator_action_t {
            kind: ALDIS_BITSTREAM_GENERATOR_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_bitstream_generator_action_t::default()
        },
    }
}

/// One step of `engine` (API-057).
fn bg_run(
    engine: &mut BitstreamGeneratorState,
    input: &aldis_bitstream_generator_input_t,
    data: &[u8],
    area: &mut [u8],
) -> Result<BitstreamActions> {
    let input = match parse::<aldis_bitstream_generator_input>(input.kind)? {
        aldis_bitstream_generator_input::ALDIS_BITSTREAM_GENERATOR_INPUT_SUBMIT_BITS => {
            BitstreamInput::SubmitBits {
                bits: data,
                bit_len: input.bit_len,
            }
        }
        aldis_bitstream_generator_input::ALDIS_BITSTREAM_GENERATOR_INPUT_RELEASE => {
            BitstreamInput::Release
        }
        aldis_bitstream_generator_input::ALDIS_BITSTREAM_GENERATOR_INPUT_RECONFIGURE => {
            BitstreamInput::Reconfigure(bg_config(&input.config)?)
        }
    };
    engine.step(input, area)
}

fn bg_status(engine: &BitstreamGeneratorState) -> aldis_bitstream_generator_status_t {
    let s: &BitstreamStatus = engine.status();
    aldis_bitstream_generator_status_t {
        submissions: s.submissions.get(),
        fields_out: s.fields_out.get(),
        idle_fields: s.idle_fields.get(),
        refused: s.refused.get(),
        would_block: s.would_block.get(),
        discarded_octets: s.discarded_octets.get(),
        queue: s.queue.into(),
    }
}

/// Builds the engine of `head` at `state`, in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `BitstreamGeneratorState`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
const unsafe fn bg_place(state: *mut BitstreamGeneratorState, head: BitstreamGeneratorHead<Aos>) {
    engine_in_place!(
        BitstreamGeneratorState,
        BitstreamGeneratorHead<Aos>,
        BitstreamGeneratorBuffers<Q>,
        state,
        head
    );
}

/// Places a `BitstreamGenerator` with an empty queue in `storage` (LNK-021; CABI-031,
/// CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`; `ALDIS_ERR_INVALID_CONFIG`
/// (`link_protocol`) for a protocol other than AOS, (`field_len`) for a zone outside 1 to 2 047
/// octets; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`).
///
/// # Safety
/// `storage` is valid for writes of one `aldis_bitstream_generator_t` and outlives its use;
/// `cfg` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_bitstream_generator_init(
    storage: *mut aldis_bitstream_generator_t,
    cfg: *const aldis_bitstream_generator_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let head = BitstreamGenerator::<Aos, Q>::new_head(bg_config(&cfg)?);
            handle::init_with(storage, |state| bg_place(state, head))
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn bg_engine(
    h: *mut aldis_bitstream_generator_t,
) -> Result<handle::Handle<BitstreamGeneratorState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `BitstreamGenerator` (API-057, CABI-040). The octets of a `B_PDU` go
/// to `out`; the generator has no clock.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a constant
/// that is none; for `SUBMIT_BITS` `ALDIS_ERR_LENGTH_MISMATCH` (`sdu_len`) and
/// `ALDIS_ERR_WOULD_BLOCK`; for `RELEASE` `ALDIS_ERR_BUFFER_TOO_SMALL`; for `RECONFIGURE` as
/// `aldis_bitstream_generator_init`. The engine is unchanged and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_bitstream_generator_init` whose storage is still valid, or
/// detectably invalid storage; `input` is valid for a read, and its `data` for `data_len`
/// octets; `out` is valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0);
/// `actions` is valid for one write; `err` is `NULL` or valid for one write. No other thread
/// uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_bitstream_generator_step(
    h: *mut aldis_bitstream_generator_t,
    input: *const aldis_bitstream_generator_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_bitstream_generator_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = bg_engine(h)?;
            let value = abi::read(input)?;
            let data = abi::bytes_disjoint(value.data, value.data_len, &[h.range()])?;
            let area = abi::bytes_mut(out, out_cap, &[(value.data, value.data_len), h.range()])?;
            let actions = abi::out_disjoint(
                actions,
                &[
                    h.range(),
                    (value.data, value.data_len),
                    (out.cast_const(), out_cap),
                ],
            )?;
            let list = h.update(|engine| bg_run(engine, &value, data, area))?;
            let mut result = aldis_bitstream_generator_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = bg_action(output);
            }
            // `list` holds at most `BITSTREAM_GENERATOR_MAX_ACTIONS` actions, the length of
            // `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the counters and the occupancy of the `BitstreamGenerator` (API-056, LNK-002). It
/// changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_bitstream_generator_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_bitstream_generator_status(
    h: *mut aldis_bitstream_generator_t,
    status: *mut aldis_bitstream_generator_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = bg_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            status.write(h.read(bg_status));
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_bitstream_generator_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_bitstream_generator_deinit(h: *mut aldis_bitstream_generator_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
