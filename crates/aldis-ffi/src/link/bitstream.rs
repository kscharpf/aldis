//! C ABI of the `BitstreamGenerator` (LNK-020, LNK-021, LNK-003, CABI-030…032, CABI-040; M4 plan D4, WP-7a).
//!
//! One opaque handle (`aldis_bitstream_generator_t`) in the pattern of [`crate::link`]: the caller declares the typed
//! storage, `_init` places the engine of the protocol named in the configuration
//! (`aldis_link_channel_t::protocol`) in it, and every input is one `_step` call that returns
//! the actions of the input in a fixed-capacity actions struct. The octets of a data field go
//! to the caller's `out` area and the action holds their `aldis_range_t` (API-061). The status
//! is a read-only call (API-056); there is no `poll_*` call and no callback. A failed call
//! leaves the engine as it was and writes no actions (API-058).

use aldis::core::Result;
use aldis::link::bitstream::{
    BITSTREAM_GENERATOR_MAX_ACTIONS, BitstreamActions, BitstreamGenerator,
    BitstreamGeneratorBuffers, BitstreamGeneratorConfig, BitstreamGeneratorHead, BitstreamInput,
    BitstreamOutput, BitstreamProtocol, BitstreamStatus,
};
use aldis::link::protocol::Aos;

use super::common::range;
use crate::abi::{self, Out};
use crate::capacities::ALDIS_LINK_PACKET_QUEUE_OCTETS;
use crate::handle::{self, InPlace, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, protocol,
    unsupported_protocol, vc,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The queue capacity of the engine: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const Q: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;

/// The most actions one `aldis_bitstream_generator_step` returns (LNK-074).
pub const ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS: usize = 1;
const _: () = assert!(ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS == BITSTREAM_GENERATOR_MAX_ACTIONS);

/// 64-bit words of one `aldis_bitstream_generator_t` (CABI-031).
pub const ALDIS_BITSTREAM_GENERATOR_WORDS: usize = 1037;
const _: () = assert!(
    size_of::<crate::handle::Slot<BitstreamGeneratorState>>()
        <= ALDIS_BITSTREAM_GENERATOR_WORDS * 8
);

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
///
/// Zero-initialize the struct before setting members: a member that the chosen `protocol` does not
/// read is ignored (OI-92).
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
///
/// Requirements: LNK-010, LNK-020, LNK-021, LNK-074, CABI-040; docs/04-engines.md §4.
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

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are plain octet arrays, valid when zeroed.
unsafe impl<P: BitstreamProtocol, const NQ: usize> InPlace for BitstreamGenerator<P, NQ> {
    type Head = BitstreamGeneratorHead<P>;
    type Buffers = BitstreamGeneratorBuffers<NQ>;
    const HEAD_OFFSET: usize = <BitstreamGenerator<P, NQ>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <BitstreamGenerator<P, NQ>>::BUFFERS_OFFSET;
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
            handle::init_engine(storage, head)
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

handle::step_io!(
    aldis_bitstream_generator_t,
    aldis_bitstream_generator_input_t,
    aldis_bitstream_generator_actions_t
);

impl handle::Step for aldis_bitstream_generator_t {
    fn step(
        engine: &mut BitstreamGeneratorState,
        input: &aldis_bitstream_generator_input_t,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_bitstream_generator_actions_t>,
    ) -> Result<()> {
        let list = bg_run(engine, input, data, area)?;
        let mut result = aldis_bitstream_generator_actions_t::default();
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = bg_action(output);
        }
        // `list` holds at most `BITSTREAM_GENERATOR_MAX_ACTIONS` actions, the length of
        // `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
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
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe { handle::step::<aldis_bitstream_generator_t>(h, input, out, out_cap, actions, err) }
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
