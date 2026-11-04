//! C ABI of the `CltuPacker` (LNK-044, LNK-003, LNK-074, LNK-075, CABI-030…032, CABI-040; M4 plan
//! D4, WP-7d).
//!
//! The engine is one opaque handle (`aldis_cltu_packer_t`) in the pattern of the engines of
//! [`crate::link_gen`], [`crate::link_rx`], and [`crate::link_map`]: the caller declares the
//! typed storage, `_init` places the engine of the protocol named in the configuration in it,
//! and every input is one `_step` call that returns the actions of the input in a fixed-capacity
//! actions struct. The frames of a data unit go to the caller's `out` area, one after the other,
//! and the action holds their `aldis_range_t` (API-061). The status is a read-only call
//! (API-056); the engine has no timer, so `_step` takes no time (API-053). A failed call leaves
//! the engine as it was and writes no actions (API-058).
//!
//! **Protocol.** The first member of the configuration is an `aldis_link_channel_t` whose
//! `protocol` names the protocol: TC or USLP (TM and AOS are `ALDIS_ERR_INVALID_CONFIG` on
//! `link_protocol`); the packer uses no other member of the channel, but each must be valid. A reconfiguration cannot
//! change the protocol (LNK-003).
//!
//! **Capacity.** The unit in progress is at most `ALDIS_LINK_MAX_FRAME_OCTETS` octets (CABI-030);
//! the engine is built in place (`aldis::link::inplace`, #240).

use aldis::core::Result;
use aldis::link::cltu::{
    CLTU_PACKER_MAX_ACTIONS, CltuActions, CltuInput, CltuOutput, CltuPacker, CltuPackerBuffers,
    CltuPackerConfig, CltuPackerHead, CltuProtocol, CltuStatus, FrameIn, Repetitions,
};
use aldis::link::protocol::{Tc, Uslp};

use crate::abi::{self, flag, invalid_argument};
use crate::capacities::ALDIS_LINK_MAX_FRAME_OCTETS;
use crate::handle::{self, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, protocol, qos,
    unsupported_protocol,
};
use crate::link_gen::{engine_in_place, range, variant_payload};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The unit capacity of the engine: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const U: usize = ALDIS_LINK_MAX_FRAME_OCTETS;

/// The most actions one `aldis_cltu_packer_step` returns (LNK-074): the unit the frame did not
/// fit and the new unit if the frame fills it.
pub const ALDIS_CLTU_PACKER_MAX_ACTIONS: usize = 2;

const _: () = assert!(ALDIS_CLTU_PACKER_MAX_ACTIONS == CLTU_PACKER_MAX_ACTIONS);

/// 64-bit words of one `aldis_cltu_packer_t` (CABI-031): the tag and the larger of the engines of
/// TC and USLP at the build capacity; the build fails when it outgrows it.
pub const ALDIS_CLTU_PACKER_WORDS: usize = 270;

const _: () =
    assert!(size_of::<crate::handle::Slot<CltuPackerState>>() <= ALDIS_CLTU_PACKER_WORDS * 8);

type Cp<P> = CltuPacker<P, U>;

/// The `CltuPacker` of the protocol chosen at `aldis_cltu_packer_init` (LNK-001).
///
/// `repr(u8)`: `aldis_cltu_packer_init` writes a variant in place (`variant_payload`).
#[repr(u8)]
#[expect(
    dead_code,
    reason = "the variants are written in place by the init function, not constructed"
)]
pub(crate) enum CltuPackerState {
    /// TC.
    Tc(Cp<Tc>) = 0,
    /// USLP.
    Uslp(Cp<Uslp>) = 1,
}

/// The kind of an `aldis_cltu_packer_input_t` (LNK-044, LNK-003; API-057).
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
pub enum aldis_cltu_packer_input {
    /// The next frame (`data`, `qos`, `repetitions`, `truncated`), in the order of transmission
    /// (LNK-044).
    ALDIS_CLTU_PACKER_INPUT_ADD_FRAME = 0,
    /// Return the unit in progress, if any (LNK-044).
    ALDIS_CLTU_PACKER_INPUT_FLUSH = 1,
    /// Replace the configuration (`config`) between data units, discarding the frames of the
    /// unit in progress (LNK-003). The protocol cannot change.
    ALDIS_CLTU_PACKER_INPUT_RECONFIGURE = 2,
}
mirror!(pub aldis_cltu_packer_input: u32 {
    ALDIS_CLTU_PACKER_INPUT_ADD_FRAME,
    ALDIS_CLTU_PACKER_INPUT_FLUSH,
    ALDIS_CLTU_PACKER_INPUT_RECONFIGURE
});

/// The kind of an `aldis_cltu_packer_action_t` (API-057).
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
pub enum aldis_cltu_packer_action {
    /// A data unit for the coding sublayer, in `out` (LNK-044).
    ALDIS_CLTU_PACKER_ACTION_DATA_UNIT = 0,
    /// A reconfiguration discarded `octets` of the unit in progress (LNK-003).
    ALDIS_CLTU_PACKER_ACTION_DISCARDED = 1,
}
mirror!(pub aldis_cltu_packer_action: u32 {
    ALDIS_CLTU_PACKER_ACTION_DATA_UNIT,
    ALDIS_CLTU_PACKER_ACTION_DISCARDED
});

/// A `CltuPacker` in caller storage: the caller declares an object of this type, and the pointer
/// to it is the handle (LNK-044, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_cltu_packer_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_CLTU_PACKER_WORDS],
}

impl Storage for aldis_cltu_packer_t {
    type State = CltuPackerState;
    const MAGIC: u32 = 0x414C_4350; // "ALCP"
}

/// The managed parameters of a `CltuPacker` (232.0 §5.1 Table 5-1; 732.1 §5.1 Table 5-1;
/// LNK-044, LNK-078).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_cltu_packer_config_t {
    /// The protocol (TC or USLP); the other members of the channel are not used but must be valid.
    pub channel: aldis_link_channel_t,
    /// The Maximum Number of Transfer Frames Given to the Coding Sublayer as a Single Data
    /// Unit, at least 1.
    pub max_frames: usize,
    /// The Maximum Length of Data Unit in octets (TC; for USLP the project's bound on a unit),
    /// at least 1 and at most `ALDIS_LINK_MAX_FRAME_OCTETS`.
    pub max_unit_len: usize,
}

/// One input of a `CltuPacker` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_cltu_packer_input_t {
    /// `ALDIS_CLTU_PACKER_INPUT_*`.
    pub kind: u32,
    /// For `ADD_FRAME`: the whole frame, FECF included, read once and copied (API-060). `data`
    /// is checked on every kind and must not overlap the storage or `out`: `NULL` only with
    /// `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `ADD_FRAME`: `ALDIS_LINK_QOS_*`; the expedited service is transferred once whatever
    /// `repetitions` says (LNK-044).
    pub qos: u32,
    /// For `ADD_FRAME`: the Repetitions value of the VC for this frame, at least 1 (232.0
    /// Table 5-3; 732.1 Table 5-3).
    pub repetitions: u16,
    /// For `ADD_FRAME`: 1 for a truncated USLP frame (732.1 Annex D), else 0.
    pub truncated: u8,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_cltu_packer_config_t,
}

/// One action of a `CltuPacker` (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_cltu_packer_action_t {
    /// `ALDIS_CLTU_PACKER_ACTION_*`.
    pub kind: u32,
    /// For `DATA_UNIT`: where the frames are in `out`, one after the other.
    pub unit: aldis_range_t,
    /// For `DATA_UNIT`: how many frames the unit holds.
    pub frames: usize,
    /// For `DATA_UNIT`: the Repetitions parameter to request, 1 for the expedited service.
    pub repetitions: u16,
    /// For `DATA_UNIT`: 1 when the frames are truncated USLP frames.
    pub truncated: u8,
    /// For `DISCARDED`: the octets of the frames held.
    pub octets: usize,
}

/// The actions of one input of a `CltuPacker`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_cltu_packer_actions_t {
    /// The number of actions in `items`, at most `ALDIS_CLTU_PACKER_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_cltu_packer_action_t; ALDIS_CLTU_PACKER_MAX_ACTIONS],
}

/// A read-only snapshot of a `CltuPacker` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_cltu_packer_status_t {
    /// Frames taken.
    pub frames_in: u64,
    /// Data units returned.
    pub units_out: u64,
    /// Frames refused as invalid.
    pub refused: u64,
    /// Octets discarded by `RECONFIGURE` (LNK-003).
    pub discarded_octets: u64,
    /// The unit in progress, in octets.
    pub pending: aldis_link_occupancy_t,
}

/// The packer configuration of `c` for the protocol `P`.
const fn cp_config<P: CltuProtocol>(c: &aldis_cltu_packer_config_t) -> Result<CltuPackerConfig<P>> {
    CltuPackerConfig::<P>::new(c.max_frames, c.max_unit_len)
}

/// The head of the engine of `c`; the protocol of the engine is fixed here (LNK-001).
enum CpHead {
    Tc(CltuPackerHead<Tc>),
    Uslp(CltuPackerHead<Uslp>),
}

fn cp_head(c: &aldis_cltu_packer_config_t) -> Result<CpHead> {
    match protocol(&c.channel)? {
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => {
            Ok(CpHead::Tc(Cp::new_head(cp_config::<Tc>(c)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
            Ok(CpHead::Uslp(Cp::new_head(cp_config::<Uslp>(c)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM
        | aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => Err(unsupported_protocol()),
    }
}

/// Builds the engine of `head` at `state`, in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `CltuPackerState`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
unsafe fn cp_place(state: *mut CltuPackerState, head: CpHead) {
    type B = CltuPackerBuffers<U>;
    match head {
        CpHead::Tc(h) => {
            // SAFETY: as the contract says; the tag 0 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Cp<Tc>>(state, 0) };
            engine_in_place!(Cp<Tc>, CltuPackerHead<Tc>, B, p, h);
        }
        CpHead::Uslp(h) => {
            // SAFETY: as the contract says; the tag 1 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Cp<Uslp>>(state, 1) };
            engine_in_place!(Cp<Uslp>, CltuPackerHead<Uslp>, B, p, h);
        }
    }
}

/// One step of `engine`, in the protocol `P` (API-057).
fn cp_run<P: CltuProtocol>(
    engine: &mut Cp<P>,
    input: &aldis_cltu_packer_input_t,
    data: &[u8],
    area: &mut [u8],
) -> Result<CltuActions> {
    let input = match parse::<aldis_cltu_packer_input>(input.kind)? {
        aldis_cltu_packer_input::ALDIS_CLTU_PACKER_INPUT_ADD_FRAME => {
            CltuInput::AddFrame(FrameIn {
                frame: data,
                qos: qos(input.qos)?,
                repetitions: Repetitions::new(input.repetitions).ok_or_else(invalid_argument)?,
                truncated: flag(input.truncated)?,
            })
        }
        aldis_cltu_packer_input::ALDIS_CLTU_PACKER_INPUT_FLUSH => CltuInput::Flush,
        aldis_cltu_packer_input::ALDIS_CLTU_PACKER_INPUT_RECONFIGURE => {
            CltuInput::Reconfigure(cp_config::<P>(&input.config)?)
        }
    };
    engine.step(input, area)
}

impl CltuPackerState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_cltu_packer_input_t,
        data: &[u8],
        area: &mut [u8],
    ) -> Result<CltuActions> {
        let reconfigure = parse::<aldis_cltu_packer_input>(input.kind)?
            == aldis_cltu_packer_input::ALDIS_CLTU_PACKER_INPUT_RECONFIGURE;
        let ch = &input.config.channel;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want| protocol(ch).map(|p| p == want);
        match self {
            CltuPackerState::Tc(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC)? {
                    return Err(unsupported_protocol());
                }
                cp_run(e, input, data, area)
            }
            CltuPackerState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                cp_run(e, input, data, area)
            }
        }
    }

    const fn status(&self) -> CltuStatus {
        match self {
            CltuPackerState::Tc(e) => *e.status(),
            CltuPackerState::Uslp(e) => *e.status(),
        }
    }
}

fn cp_action(output: &CltuOutput) -> aldis_cltu_packer_action_t {
    match output {
        CltuOutput::DataUnit(u) => aldis_cltu_packer_action_t {
            kind: ALDIS_CLTU_PACKER_ACTION_DATA_UNIT,
            unit: range(u.unit),
            frames: u.frames,
            repetitions: u.repetitions.get(),
            truncated: u8::from(u.truncated),
            ..aldis_cltu_packer_action_t::default()
        },
        CltuOutput::Discarded { octets } => aldis_cltu_packer_action_t {
            kind: ALDIS_CLTU_PACKER_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_cltu_packer_action_t::default()
        },
    }
}

/// Places a `CltuPacker` with no frame held in `storage` (LNK-044; CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg` or a constant that is
/// none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TM and AOS, (`max_frames_per_unit`) for
/// 0 frames, (`max_unit_len`) for 0 octets; `ALDIS_ERR_CAPACITY_EXCEEDED` (`max_unit_len`) above
/// `ALDIS_LINK_MAX_FRAME_OCTETS`.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_cltu_packer_t` and outlives its use; `cfg` is
/// valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_cltu_packer_init(
    storage: *mut aldis_cltu_packer_t,
    cfg: *const aldis_cltu_packer_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let head = cp_head(&cfg)?;
            // `init_with` passes a pointer to the state in the slot, valid for writes.
            handle::init_with(storage, |state| cp_place(state, head))
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn cp_engine(h: *mut aldis_cltu_packer_t) -> Result<handle::Handle<CltuPackerState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `CltuPacker` (API-057, CABI-040).
///
/// The frames of a unit go to `out`, which must hold at least twice `max_unit_len` octets for
/// `ADD_FRAME` and `max_unit_len` for `FLUSH` (LNK-075).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation, a flag that
/// is not 0 or 1, a constant that is none, or 0 `repetitions`; for `ADD_FRAME`:
/// `ALDIS_ERR_LENGTH_MISMATCH` (`frame_len`), `ALDIS_ERR_OUT_OF_RANGE` (`max_unit_len`),
/// `ALDIS_ERR_INVALID_FIELD` (`truncated_frame_len`), `ALDIS_ERR_BUFFER_TOO_SMALL`; for
/// `FLUSH`: `ALDIS_ERR_BUFFER_TOO_SMALL`; for `RECONFIGURE`: as `aldis_cltu_packer_init`, and
/// `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another protocol. The engine is unchanged
/// and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_cltu_packer_init` whose storage is still valid, or detectably
/// invalid storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is
/// valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is
/// valid for one write; `err` is `NULL` or valid for one write. No other thread uses the
/// storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_cltu_packer_step(
    h: *mut aldis_cltu_packer_t,
    input: *const aldis_cltu_packer_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_cltu_packer_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = cp_engine(h)?;
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
            let mut result = aldis_cltu_packer_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = cp_action(output);
            }
            // `list` holds at most `CLTU_PACKER_MAX_ACTIONS` actions, the length of `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the counters and the occupancy of the `CltuPacker` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_cltu_packer_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_cltu_packer_status(
    h: *mut aldis_cltu_packer_t,
    status: *mut aldis_cltu_packer_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = cp_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(CltuPackerState::status);
            status.write(aldis_cltu_packer_status_t {
                frames_in: s.frames_in.get(),
                units_out: s.units_out.get(),
                refused: s.refused.get(),
                discarded_octets: s.discarded_octets.get(),
                pending: s.pending.into(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_cltu_packer_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_cltu_packer_deinit(h: *mut aldis_cltu_packer_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
