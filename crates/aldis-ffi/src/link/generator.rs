//! C ABI of the `PacketGenerator` (LNK-010…015, LNK-003, CABI-030…032, CABI-040; M4 plan D4, WP-7a).
//!
//! One opaque handle (`aldis_packet_generator_t`) in the pattern of [`crate::link`]: the caller declares the typed
//! storage, `_init` places the engine of the protocol named in the configuration
//! (`aldis_link_channel_t::protocol`) in it, and every input is one `_step` call that returns
//! the actions of the input in a fixed-capacity actions struct. The octets of a data field go
//! to the caller's `out` area and the action holds their `aldis_range_t` (API-061). The status
//! is a read-only call (API-056); there is no `poll_*` call and no callback. A failed call
//! leaves the engine as it was and writes no actions (API-058).
//! The queue holds `ALDIS_LINK_PACKET_QUEUE_OCTETS` octets and a data field
//! `ALDIS_LINK_MAX_FRAME_OCTETS` octets at most, fixed when the library is built (CABI-030).

use aldis::core::Result;
use aldis::core::time::{Duration, Instant};
use aldis::link::generator::{
    GeneratorActions, GeneratorInput, GeneratorOutput, GeneratorStatus,
    PACKET_GENERATOR_MAX_ACTIONS, PacketGenerator, PacketGeneratorBuffers, PacketGeneratorConfig,
    PacketGeneratorHead,
};
use aldis::link::protocol::{Aos, PacketProtocol, Tm, TmParams, Uslp};

use super::common::range;
use crate::abi::{self, Out};
use crate::capacities::{ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_PACKET_QUEUE_OCTETS};
use crate::handle::{self, InPlace, Storage};
use crate::link::{
    RouteFields, aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, aos_params,
    idle_packet, idle_policy, protocol, protocol_tag, unsupported_protocol, uslp_params,
    valid_pvns, vc,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The data field capacity of the engine: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const D: usize = ALDIS_LINK_MAX_FRAME_OCTETS;
/// The queue capacity of the engine: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const Q: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;

/// The most actions one `aldis_packet_generator_step` returns (LNK-074): a data field or the
/// octets a reconfiguration discarded.
pub const ALDIS_PACKET_GENERATOR_MAX_ACTIONS: usize = 1;
const _: () = assert!(ALDIS_PACKET_GENERATOR_MAX_ACTIONS == PACKET_GENERATOR_MAX_ACTIONS);

/// 64-bit words of one `aldis_packet_generator_t` (CABI-031): the tag and the largest of the
/// engines of TM, AOS, and USLP at the build capacities; the build fails when it outgrows it.
pub const ALDIS_PACKET_GENERATOR_WORDS: usize = 1308;
const _: () = assert!(
    size_of::<crate::handle::Slot<PacketGeneratorState>>() <= ALDIS_PACKET_GENERATOR_WORDS * 8
);

type Pg<P> = PacketGenerator<P, D, Q>;

handle::engine_enum! {
    /// The `PacketGenerator` of the protocol chosen at `aldis_packet_generator_init` (LNK-001).
    #[expect(
        dead_code,
        reason = "the variants are written in place by the init function, not constructed"
    )]
    pub(crate) enum PacketGeneratorState {
        /// TM.
        Tm(Pg<Tm>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM),
        /// AOS.
        Aos(Pg<Aos>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS),
        /// USLP.
        Uslp(Pg<Uslp>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP),
    }
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
///
/// Zero-initialize the struct before setting members: a member that the chosen `protocol` does not
/// read is ignored (OI-92).
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
///
/// Requirements: LNK-010, LNK-020, LNK-021, LNK-074, CABI-040; docs/04-engines.md §4.
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

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are plain octet arrays, valid when zeroed.
unsafe impl<P: PacketProtocol, const ND: usize, const NQ: usize> InPlace
    for PacketGenerator<P, ND, NQ>
{
    type Head = PacketGeneratorHead<P>;
    type Buffers = PacketGeneratorBuffers<ND, NQ>;
    const HEAD_OFFSET: usize = <PacketGenerator<P, ND, NQ>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <PacketGenerator<P, ND, NQ>>::BUFFERS_OFFSET;
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
            match protocol(&cfg.channel)? {
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => handle::init_variant::<_, Pg<Tm>>(
                    storage,
                    Pg::new_head(pg_config(&cfg, TmParams)?)?,
                ),
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => handle::init_variant::<_, Pg<Aos>>(
                    storage,
                    Pg::new_head(pg_config(&cfg, aos_params(&cfg.channel)?)?)?,
                ),
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
                    handle::init_variant::<_, Pg<Uslp>>(
                        storage,
                        Pg::new_head(pg_config(&cfg, uslp_params(&cfg.channel)?)?)?,
                    )
                }
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => Err(unsupported_protocol()),
            }
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

handle::step_io!(
    aldis_packet_generator_t,
    aldis_packet_generator_input_t,
    aldis_packet_generator_actions_t
);

impl handle::TimedStep for aldis_packet_generator_t {
    fn step(
        state: &mut PacketGeneratorState,
        input: &aldis_packet_generator_input_t,
        now: Instant,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_packet_generator_actions_t>,
    ) -> Result<()> {
        let list = state.step(input, data, now, area)?;
        let mut result = aldis_packet_generator_actions_t::default();
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = pg_action(output);
        }
        // `list` holds at most `PACKET_GENERATOR_MAX_ACTIONS` actions, the length of `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
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
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe {
        handle::step_timed::<aldis_packet_generator_t>(h, input, now_us, out, out_cap, actions, err)
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
