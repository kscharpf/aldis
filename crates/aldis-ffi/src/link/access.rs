//! C ABI of the `AccessGenerator` (LNK-010…015, LNK-003, CABI-030…032, CABI-040; M4 plan D4, WP-7a).
//!
//! One opaque handle (`aldis_access_generator_t`) in the pattern of [`crate::link`]: the caller declares the typed
//! storage, `_init` places the engine of the protocol named in the configuration
//! (`aldis_link_channel_t::protocol`) in it, and every input is one `_step` call that returns
//! the actions of the input in a fixed-capacity actions struct. The octets of a data field go
//! to the caller's `out` area and the action holds their `aldis_range_t` (API-061). The status
//! is a read-only call (API-056); there is no `poll_*` call and no callback. A failed call
//! leaves the engine as it was and writes no actions (API-058).

use aldis::core::Result;
use aldis::link::access::{
    ACCESS_GENERATOR_MAX_ACTIONS, AccessActions, AccessGenerator, AccessGeneratorBuffers,
    AccessGeneratorConfig, AccessGeneratorHead, AccessInput, AccessOutput, AccessProtocol,
};
use aldis::link::protocol::{Aos, Tm, Uslp};

use super::common::range;
use crate::abi::{self, Out};
use crate::capacities::ALDIS_LINK_PACKET_QUEUE_OCTETS;
use crate::handle::{self, InPlace, Storage};
use crate::link::{
    RouteFields, aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, protocol,
    protocol_tag, unsupported_protocol, uslp_params, vc,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The queue capacity of the engine: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const Q: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;

/// The most actions one `aldis_access_generator_step` returns (LNK-074).
pub const ALDIS_ACCESS_GENERATOR_MAX_ACTIONS: usize = 1;
const _: () = assert!(ALDIS_ACCESS_GENERATOR_MAX_ACTIONS == ACCESS_GENERATOR_MAX_ACTIONS);

/// 64-bit words of one `aldis_access_generator_t` (CABI-031).
pub const ALDIS_ACCESS_GENERATOR_WORDS: usize = 1040;
const _: () = assert!(
    size_of::<crate::handle::Slot<AccessGeneratorState>>() <= ALDIS_ACCESS_GENERATOR_WORDS * 8
);

type Ag<P> = AccessGenerator<P, Q>;

handle::engine_enum! {
    /// The `AccessGenerator` of the protocol chosen at `aldis_access_generator_init` (LNK-001).
    #[expect(
        dead_code,
        reason = "the variants are written in place by the init function, not constructed"
    )]
    pub(crate) enum AccessGeneratorState {
        /// TM.
        Tm(Ag<Tm>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM),
        /// AOS.
        Aos(Ag<Aos>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS),
        /// USLP.
        Uslp(Ag<Uslp>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP),
    }
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
///
/// Zero-initialize the struct before setting members: a member that the chosen `protocol` does not
/// read is ignored (OI-92).
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
///
/// Requirements: LNK-010, LNK-020, LNK-021, LNK-074, CABI-040; docs/04-engines.md §4.
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

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are plain octet arrays, valid when zeroed.
unsafe impl<P: AccessProtocol, const NQ: usize> InPlace for AccessGenerator<P, NQ> {
    type Head = AccessGeneratorHead<P>;
    type Buffers = AccessGeneratorBuffers<NQ>;
    const HEAD_OFFSET: usize = <AccessGenerator<P, NQ>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <AccessGenerator<P, NQ>>::BUFFERS_OFFSET;
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
            match protocol(&cfg.channel)? {
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => {
                    handle::init_variant::<_, Ag<Tm>>(storage, Ag::new_head(ag_config(&cfg, ())?)?)
                }
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => {
                    handle::init_variant::<_, Ag<Aos>>(storage, Ag::new_head(ag_config(&cfg, ())?)?)
                }
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
                    handle::init_variant::<_, Ag<Uslp>>(
                        storage,
                        Ag::new_head(ag_config(&cfg, uslp_params(&cfg.channel)?)?)?,
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
unsafe fn ag_engine(
    h: *mut aldis_access_generator_t,
) -> Result<handle::Handle<AccessGeneratorState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

handle::step_io!(
    aldis_access_generator_t,
    aldis_access_generator_input_t,
    aldis_access_generator_actions_t
);

impl handle::Step for aldis_access_generator_t {
    fn step(
        state: &mut AccessGeneratorState,
        input: &aldis_access_generator_input_t,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_access_generator_actions_t>,
    ) -> Result<()> {
        let list = state.step(input, data, area)?;
        let mut result = aldis_access_generator_actions_t::default();
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = ag_action(output);
        }
        // `list` holds at most `ACCESS_GENERATOR_MAX_ACTIONS` actions, the length of `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
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
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe { handle::step::<aldis_access_generator_t>(h, input, out, out_cap, actions, err) }
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
