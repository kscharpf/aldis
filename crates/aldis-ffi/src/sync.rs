//! C ABI of `aldis-sync`: the codec functions and the ASM search handle.
//!
//! Requirements: SYNC-001…SYNC-020, CABI-030…032, CABI-040, API-080, API-082 (CCSDS 131.0-B-6
//! §9, §10; M5 plan D3, WP-4).
//!
//! **Codec functions.** [`aldis_sync_attach`] writes the CADU of a frame, and
//! [`aldis_sync_randomize`] exclusive-ORs a buffer in place with the sequence the caller selects.
//! The caller randomizes the frame and then attaches the ASM, so the marker is never randomized
//! (SYNC-006). Neither keeps a pointer (API-060).
//!
//! **Handle.** The search engine is one opaque handle (`aldis_sync_search_t`) in the pattern of
//! the link engines ([`crate::link_cltu`]): the caller declares the typed storage, `_init` places
//! the engine in it, every input is one `_step` call that returns the units of the input in a
//! fixed-capacity actions struct, and the status is a read-only call (API-056). The engine has no
//! timer, so `_step` takes no time (API-053). A failed call leaves the engine as it was and
//! writes no actions (API-058).
//!
//! **Choices.** The frame protocol is an `ALDIS_LINK_PROTOCOL_*` constant, as in the link
//! engines: TM, AOS, and USLP (fixed length); TC and a value that is none are
//! `ALDIS_ERR_INVALID_CONFIG` on `sync_protocol`. The randomizer sequence is its length in bits,
//! `ALDIS_SYNC_RANDOMIZER_131071` or `ALDIS_SYNC_RANDOMIZER_255`; any other value is
//! `ALDIS_ERR_INVALID_CONFIG` on `randomizer_sequence` (SYNC-020; OI-83 d).
//!
//! **Capacity.** A chunk is at most `ALDIS_SYNC_MAX_CHUNK_OCTETS` octets and a frame at most
//! `ALDIS_SYNC_MAX_FRAME_OCTETS` (CABI-030); the engine is built in place
//! (`aldis::sync::AsmSearch`, #240), so its partial-unit storage does not pass through the stack.

use aldis::core::{Error, ErrorKind, Result, field};
use aldis::sync::{
    AsmErrors, AsmSearch, AsmSearchBuffers, AsmSearchHead, RandomizerSequence, SearchActions,
    SearchInput, SearchState, SyncConfig, SyncProtocol, attach_asm, max_actions, randomize,
};

use crate::abi::{self, Out};
use crate::capacities::{ALDIS_SYNC_MAX_CHUNK_OCTETS, ALDIS_SYNC_MAX_FRAME_OCTETS};
use crate::handle::{self, InPlace, Storage};
use crate::link::{ALDIS_LINK_PROTOCOL_AOS, ALDIS_LINK_PROTOCOL_TM, ALDIS_LINK_PROTOCOL_USLP};
use crate::link_common::range;
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The capacities of the engine: `ALDIS_SYNC_MAX_FRAME_OCTETS` and `ALDIS_SYNC_MAX_CHUNK_OCTETS`
/// (CABI-030).
const F: usize = ALDIS_SYNC_MAX_FRAME_OCTETS;
const C: usize = ALDIS_SYNC_MAX_CHUNK_OCTETS;

/// The most units one `aldis_sync_search_step` returns: ⌈`ALDIS_SYNC_MAX_CHUNK_OCTETS` / 5⌉, the
/// value for a frame of 1 octet (SYNC-013, API-057).
pub const ALDIS_SYNC_SEARCH_MAX_ACTIONS: usize = 52;

const _: () = assert!(ALDIS_SYNC_SEARCH_MAX_ACTIONS == max_actions(C));

/// The randomizer sequence of 131071 bits, h(x) = x^17 + x^14 + 1 (131.0 §10.4.1; SYNC-005): a
/// value of the `randomizer_sequence` argument of [`aldis_sync_randomize`].
pub const ALDIS_SYNC_RANDOMIZER_131071: u32 = 131_071;

/// The legacy randomizer sequence of 255 bits, h(x) = x^8 + x^7 + x^5 + x^3 + 1 (131.0 §10.4.2;
/// SYNC-020): a value of the `randomizer_sequence` argument of [`aldis_sync_randomize`].
pub const ALDIS_SYNC_RANDOMIZER_255: u32 = 255;

type Engine = AsmSearch<F, C, ALDIS_SYNC_SEARCH_MAX_ACTIONS>;
type Actions = SearchActions<ALDIS_SYNC_SEARCH_MAX_ACTIONS>;

/// 64-bit words of one `aldis_sync_search_t` (CABI-031): the tag and the engine at the build
/// capacity; the build fails when the engine outgrows it.
pub const ALDIS_SYNC_SEARCH_WORDS: usize = 8201;

const _: () = assert!(size_of::<handle::Slot<Engine>>() <= ALDIS_SYNC_SEARCH_WORDS * 8);

/// The kind of an `aldis_sync_search_input_t` (SYNC-015; API-057).
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
pub enum aldis_sync_search_input {
    /// The next octets of the stream (`data`, at most `ALDIS_SYNC_MAX_CHUNK_OCTETS`), read most
    /// significant bit first (SYNC-007, SYNC-010).
    ALDIS_SYNC_SEARCH_INPUT_CHUNK = 0,
    /// Discard the partial unit and the window and restart the skipped-bit count, keeping the
    /// counters and the configuration (SYNC-015).
    ALDIS_SYNC_SEARCH_INPUT_RESET = 1,
    /// As `RESET`, with the new configuration `config` (SYNC-015).
    ALDIS_SYNC_SEARCH_INPUT_RECONFIGURE = 2,
}
mirror!(pub aldis_sync_search_input: u32 {
    ALDIS_SYNC_SEARCH_INPUT_CHUNK,
    ALDIS_SYNC_SEARCH_INPUT_RESET,
    ALDIS_SYNC_SEARCH_INPUT_RECONFIGURE
});

/// The kind of an `aldis_sync_search_action_t` (API-057, API-080).
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
pub enum aldis_sync_search_action {
    /// A unit found after an ASM, in `out` (SYNC-011).
    ALDIS_SYNC_SEARCH_ACTION_UNIT = 0,
}
mirror!(pub aldis_sync_search_action: u32 {
    ALDIS_SYNC_SEARCH_ACTION_UNIT
});

/// The state of the search engine, `state` of an `aldis_sync_search_status_t` (SYNC-012).
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
pub enum aldis_sync_search_state {
    /// Looking for the ASM (SYNC-007).
    ALDIS_SYNC_SEARCH_STATE_SEARCHING = 0,
    /// Taking the unit that follows a matched ASM (SYNC-009).
    ALDIS_SYNC_SEARCH_STATE_COLLECTING = 1,
}
mirror!(pub aldis_sync_search_state: u32 {
    ALDIS_SYNC_SEARCH_STATE_SEARCHING,
    ALDIS_SYNC_SEARCH_STATE_COLLECTING
});

/// An ASM search engine in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (SYNC-007, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_sync_search_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_SYNC_SEARCH_WORDS],
}

impl Storage for aldis_sync_search_t {
    type State = Engine;
    const MAGIC: u32 = 0x414C_5353; // "ALSS"
}

/// The configuration of the SYNC functions: the frame protocol, the frame length, and the
/// tolerance of the ASM search.
///
/// Taken by the engine and by the attach function (131.0 §11.1, §11.3, §11.4; SYNC-002, SYNC-016).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_sync_search_config_t {
    /// The frame protocol: `ALDIS_LINK_PROTOCOL_TM`, `_AOS`, or `_USLP` (fixed-length frames).
    pub protocol: u32,
    /// The frame length in octets, without the marker: 1 up to 2048 for TM, 65536 for AOS and
    /// USLP (131.0 §11.3, §11.4).
    pub frame_len: usize,
    /// The tolerated bit errors of the ASM, 0 to 15 (SYNC-007, SYNC-016). A high value raises
    /// the false-lock rate: about 43 % of random windows match at 15 (HZ-SYNC-01).
    /// `aldis_sync_attach` does not read it.
    pub asm_errors: u32,
}

/// One input of the search engine (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_sync_search_input_t {
    /// `ALDIS_SYNC_SEARCH_INPUT_*`.
    pub kind: u32,
    /// For `CHUNK`: the octets of the stream, read once (API-060). `data` is checked on every
    /// kind and must not overlap the storage or `out`: `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`, at most `ALDIS_SYNC_MAX_CHUNK_OCTETS`.
    pub data_len: usize,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_sync_search_config_t,
}

/// One action of the search engine (API-057, API-061, SYNC-011). A field that the kind does not
/// use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_sync_search_action_t {
    /// `ALDIS_SYNC_SEARCH_ACTION_*`.
    pub kind: u32,
    /// For `UNIT`: the ASM bits that differed, 0 up to the tolerance.
    pub errors: u32,
    /// For `UNIT`: where the unit (the frame as received, possibly randomized, shifted to an
    /// octet boundary) is in `out`.
    pub unit: aldis_range_t,
    /// For `UNIT`: the stream bits between the end of the previous unit (or the start or reset
    /// of the stream) and the first bit of the ASM, saturating at `UINT64_MAX`.
    pub skipped_bits: u64,
}

/// The actions of one input of the search engine.
///
/// Requirements: SYNC-011, SYNC-013, CABI-040; docs/04-engines.md §4.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_sync_search_actions_t {
    /// The number of actions in `items`, at most `ALDIS_SYNC_SEARCH_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_sync_search_action_t; ALDIS_SYNC_SEARCH_MAX_ACTIONS],
}

/// A read-only snapshot of the search engine (SYNC-015, API-056).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_sync_search_status_t {
    /// Units returned.
    pub units_returned: u64,
    /// The sum of the `skipped_bits` of the units returned, saturating.
    pub skipped_bits_total: u64,
    /// `ALDIS_SYNC_SEARCH_STATE_*`.
    pub state: u32,
    /// The complete octets of the unit being collected; 0 while searching.
    pub partial_octets: usize,
}

/// The frame protocol of the constant `raw` (SYNC-002).
const fn protocol(raw: u32) -> Result<SyncProtocol> {
    match raw {
        ALDIS_LINK_PROTOCOL_TM => Ok(SyncProtocol::Tm),
        ALDIS_LINK_PROTOCOL_AOS => Ok(SyncProtocol::Aos),
        ALDIS_LINK_PROTOCOL_USLP => Ok(SyncProtocol::Uslp),
        _ => Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SYNC_PROTOCOL)),
    }
}

/// The configuration of `c`, checked in the order protocol, frame length, tolerance (OI-83 c).
fn config(c: &aldis_sync_search_config_t) -> Result<SyncConfig> {
    let p = protocol(c.protocol)?;
    SyncConfig::new(p, c.frame_len, AsmErrors::NONE)?;
    SyncConfig::new(p, c.frame_len, AsmErrors::new(c.asm_errors)?)
}

/// Writes the CADU of a frame: the ASM, then the frame as given (131.0 §9.2.1, §9.1.2 Table 9-1;
/// SYNC-001, SYNC-003, SYNC-006).
///
/// `frame` is the frame as it goes on the channel, randomized by the caller when the channel
/// randomizes (`aldis_sync_randomize`). `out_len` receives `4 + cfg->frame_len`, the length of
/// the CADU. Nothing is written on error.
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`sync_protocol`, `frame_len`, `asm_errors`) for a configuration
/// that is not valid, in that order; `ALDIS_ERR_LENGTH_MISMATCH` (`frame_len`) when `frame_len`
/// is not `cfg->frame_len`; `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`) when `cap` is below the
/// CADU; CABI-022 violations, including `out` overlapping `frame`.
///
/// # Safety
/// `cfg` is valid for a read; `frame` is valid for `frame_len` octets (or `NULL` with 0); `out`
/// is valid for `cap` octets of writes; `out_len` is valid for one write; `err` is `NULL` or
/// valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_sync_attach(
    cfg: *const aldis_sync_search_config_t,
    frame: *const u8,
    frame_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // that `out` overlaps no input.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::read(cfg)?)?;
            let input = abi::bytes(frame, frame_len)?;
            let area = abi::bytes_mut(out, cap, &[(frame, frame_len)])?;
            let out_len = abi::out(out_len)?;
            out_len.write(attach_asm(&config, input, area)?);
            Ok(())
        })
    }
}

/// Exclusive-ORs the `len` octets at `data` in place with a pseudo-random sequence (131.0 §10;
/// SYNC-004, SYNC-005, SYNC-020).
///
/// One function for both directions: applying it twice restores the buffer. The sequence starts
/// at the first bit of the buffer on every call. `sequence` is the length of the sequence in bits,
/// `ALDIS_SYNC_RANDOMIZER_131071` or `ALDIS_SYNC_RANDOMIZER_255`. A buffer of any length is
/// accepted, including 0.
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`randomizer_sequence`) for another `sequence`; CABI-022
/// violations (`data` `NULL` with `len` above 0, or a range that wraps). The buffer is not
/// changed on error.
///
/// # Safety
/// `data` is valid for reads and writes of `len` octets (or `NULL` with `len` 0); `err` is `NULL`
/// or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_sync_randomize(
    sequence: u32,
    data: *mut u8,
    len: usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and the range.
    unsafe {
        abi::call(err, || {
            let sequence = RandomizerSequence::from_period(sequence)?;
            randomize(sequence, abi::bytes_mut(data, len, &[])?);
            Ok(())
        })
    }
}

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are a plain octet array, valid when zeroed.
unsafe impl<const NF: usize, const NC: usize, const NA: usize> InPlace for AsmSearch<NF, NC, NA> {
    type Head = AsmSearchHead;
    type Buffers = AsmSearchBuffers<NF>;
    const HEAD_OFFSET: usize = <AsmSearch<NF, NC, NA>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <AsmSearch<NF, NC, NA>>::BUFFERS_OFFSET;
}

/// One step of `engine` (API-057).
fn run(
    engine: &mut Engine,
    input: &aldis_sync_search_input_t,
    data: &[u8],
    area: &mut [u8],
) -> Result<Actions> {
    let input = match parse::<aldis_sync_search_input>(input.kind)? {
        aldis_sync_search_input::ALDIS_SYNC_SEARCH_INPUT_CHUNK => SearchInput::Chunk(data),
        aldis_sync_search_input::ALDIS_SYNC_SEARCH_INPUT_RESET => {
            SearchInput::Reset { config: None }
        }
        aldis_sync_search_input::ALDIS_SYNC_SEARCH_INPUT_RECONFIGURE => SearchInput::Reset {
            config: Some(config(&input.config)?),
        },
    };
    engine.step(input, area)
}

fn action(a: &aldis::sync::SearchAction) -> aldis_sync_search_action_t {
    aldis_sync_search_action_t {
        kind: ALDIS_SYNC_SEARCH_ACTION_UNIT,
        errors: u32::from(a.errors().get()),
        unit: range(a.unit()),
        skipped_bits: a.skipped_bits(),
    }
}

/// Places an ASM search engine in `storage`, in `Searching` with an empty window (SYNC-007;
/// CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`;
/// `ALDIS_ERR_INVALID_CONFIG` (`sync_protocol`) for a protocol that is not TM, AOS, or USLP,
/// (`frame_len`) for a length of 0 or above the maximum of the protocol, (`asm_errors`) above 15,
/// in that order; `ALDIS_ERR_CAPACITY_EXCEEDED` for a frame above `ALDIS_SYNC_MAX_FRAME_OCTETS`
/// (no valid frame length is).
///
/// # Safety
/// `storage` is valid for writes of one `aldis_sync_search_t` and outlives its use; `cfg` is
/// valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_sync_search_init(
    storage: *mut aldis_sync_search_t,
    cfg: *const aldis_sync_search_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let head = Engine::new_head(config(&abi::read(cfg)?)?)?;
            handle::init_engine(storage, head)
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn engine(h: *mut aldis_sync_search_t) -> Result<handle::Handle<Engine>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

handle::step_io!(
    aldis_sync_search_t,
    aldis_sync_search_input_t,
    aldis_sync_search_actions_t
);

impl handle::Step for aldis_sync_search_t {
    fn step(
        state: &mut Engine,
        input: &aldis_sync_search_input_t,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_sync_search_actions_t>,
    ) -> Result<()> {
        let list = run(state, input, data, area)?;
        let mut result = aldis_sync_search_actions_t {
            count: 0,
            items: [aldis_sync_search_action_t::default(); ALDIS_SYNC_SEARCH_MAX_ACTIONS],
        };
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = action(output);
        }
        // `list` holds at most `ALDIS_SYNC_SEARCH_MAX_ACTIONS` actions, the length of `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
}

/// Runs one input of the search engine (API-057, CABI-040).
///
/// A `CHUNK` is searched bit by bit; each unit it completes is written to `out`, one after the
/// other from offset 0, and returned as an action. `out` must hold the units the chunk completes:
/// `ALDIS_SYNC_MAX_CHUNK_OCTETS + ALDIS_SYNC_MAX_FRAME_OCTETS` octets fit every chunk (SYNC-014).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a kind that
/// is none; for `CHUNK`: `ALDIS_ERR_OUT_OF_RANGE` (`chunk_len`) above
/// `ALDIS_SYNC_MAX_CHUNK_OCTETS`, `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`, the exact octets
/// of the units the chunk completes); for `RECONFIGURE`: as `aldis_sync_search_init`. The engine
/// is unchanged and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_sync_search_init` whose storage is still valid, or detectably
/// invalid storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is
/// valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is valid
/// for one write; `err` is `NULL` or valid for one write. No other thread uses the storage
/// (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_sync_search_step(
    h: *mut aldis_sync_search_t,
    input: *const aldis_sync_search_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_sync_search_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe { handle::step::<aldis_sync_search_t>(h, input, out, out_cap, actions, err) }
}

/// Reads the counters and the state of the search engine (SYNC-015, API-056). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_sync_search_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_sync_search_status(
    h: *mut aldis_sync_search_t,
    status: *mut aldis_sync_search_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(Engine::status);
            status.write(aldis_sync_search_status_t {
                units_returned: s.units_returned(),
                skipped_bits_total: s.skipped_bits_total(),
                state: match s.state() {
                    SearchState::Searching => ALDIS_SYNC_SEARCH_STATE_SEARCHING,
                    SearchState::Collecting => ALDIS_SYNC_SEARCH_STATE_COLLECTING,
                },
                partial_octets: s.partial_octets(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_sync_search_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_sync_search_deinit(h: *mut aldis_sync_search_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
