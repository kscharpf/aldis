//! C ABI of the FARM-1 engine in caller-owned storage (232.1 §6; COP-002, COP-030…036,
//! CABI-030…032, CABI-040).
//!
//! The caller declares an `aldis_farm1_t`, `aldis_farm1_init` places an engine in it (the
//! pointer to it is the handle), and every input of Table 6-1 is one `aldis_farm1_step` call that
//! returns the actions of its cell in a fixed-capacity `aldis_farm1_actions_t`. The engine has no
//! delivery buffer and no output queue (D6, API-059): the FDU of an accepted frame goes to the
//! caller's `out` area, and the action holds its `aldis_range_t`. `aldis_farm1_status` reads the
//! state and the CLCW (E11) without changing anything. There is no `poll_*` call and no callback
//! (CABI-040), and the engine has no timer, so there is no `next_deadline`.

use aldis::cop1::{
    FARM1_MAX_ACTIONS, Farm1, Farm1Config, Farm1Event, Farm1Frame, Farm1Input, Farm1Notify,
    Farm1Output, Farm1State, Farm1Window, FduKind,
};
use aldis::core::ids::FrameSeqNum;
use aldis::core::time::Instant;
use aldis::core::{Error, Result};
use aldis::tc::{ClcwStatus, ControlCommand, TcVcId};

use crate::abi::{self, flag};
use crate::handle::{self, Storage};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The most actions one `aldis_farm1_step` returns (COP-006): `FARM1_MAX_ACTIONS` of the engine,
/// computed from Table 6-1.
///
/// The header publishes the literal because the array length of
/// `aldis_farm1_actions_t::items` must be one constant; the assertion below fails the build when
/// it differs from the engine's.
pub const ALDIS_FARM1_MAX_ACTIONS: usize = 3;

const _: () = assert!(ALDIS_FARM1_MAX_ACTIONS == FARM1_MAX_ACTIONS);

/// 64-bit words of one `aldis_farm1_t` (CABI-031).
///
/// The tag and the engine state; the build fails when the state outgrows it.
pub const ALDIS_FARM1_WORDS: usize = 8;

/// The kind of an `aldis_farm1_input_t` (232.1 §6.3.2, Table 6-1; COP-033, COP-035).
///
/// A `uint32_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_farm1_input {
    /// A valid Type-AD frame: E1 to E5 (`sequence_number`, `data`, `buffer_available`).
    ALDIS_FARM1_INPUT_FRAME_AD = 0,
    /// A valid Type-BD frame: E6 (`data`).
    ALDIS_FARM1_INPUT_FRAME_BD = 1,
    /// A valid Type-BC frame carrying the Unlock command: E7.
    ALDIS_FARM1_INPUT_FRAME_BC_UNLOCK = 2,
    /// A valid Type-BC frame carrying the Set V(R) command: E8 (`v_star`).
    ALDIS_FARM1_INPUT_FRAME_BC_SET_VR = 3,
    /// A valid Type-BC frame whose data field is neither Unlock nor Set V(R): E9.
    ALDIS_FARM1_INPUT_FRAME_BC_OTHER = 4,
    /// A frame that failed the Frame Validation Checks: E9.
    ALDIS_FARM1_INPUT_INVALID_FRAME = 5,
    /// The Buffer Release Signal: E10.
    ALDIS_FARM1_INPUT_BUFFER_RELEASED = 6,
    /// Sets the No RF Available and No Bit Lock Flags of the CLCW (`no_rf_available`,
    /// `no_bit_lock`); not a COP-1 event (232.0 §4.2.1.8).
    ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS = 7,
}
mirror!(pub aldis_farm1_input: u32 {
    ALDIS_FARM1_INPUT_FRAME_AD,
    ALDIS_FARM1_INPUT_FRAME_BD,
    ALDIS_FARM1_INPUT_FRAME_BC_UNLOCK,
    ALDIS_FARM1_INPUT_FRAME_BC_SET_VR,
    ALDIS_FARM1_INPUT_FRAME_BC_OTHER,
    ALDIS_FARM1_INPUT_INVALID_FRAME,
    ALDIS_FARM1_INPUT_BUFFER_RELEASED,
    ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS
});

/// The kind of an `aldis_farm1_action_t` (232.1 §6.2, §6.3; API-057, API-083).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of the
/// header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_farm1_action {
    /// The Accept action: the FDU is at `data` in `out` (`fdu_kind`, `sequence_number`).
    ALDIS_FARM1_ACTION_DELIVER = 0,
    /// The Discard action: a frame was discarded (`event` and `state`, the state before).
    ALDIS_FARM1_ACTION_FRAME_DISCARDED = 1,
    /// The State variable changed (`event`, `state` before, `to_state` after).
    ALDIS_FARM1_ACTION_STATE_CHANGED = 2,
    /// An Unlock command was executed (E7): the Higher Procedures purge their buffers as they
    /// need (232.1 §6.3.2.2).
    ALDIS_FARM1_ACTION_UNLOCKED = 3,
}
mirror!(pub aldis_farm1_action: u32 {
    ALDIS_FARM1_ACTION_DELIVER,
    ALDIS_FARM1_ACTION_FRAME_DISCARDED,
    ALDIS_FARM1_ACTION_STATE_CHANGED,
    ALDIS_FARM1_ACTION_UNLOCKED
});

/// The kind of FDU a `DELIVER` action carries (232.1 §6.2.2).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of the
/// header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_farm1_fdu {
    /// The FDU of an accepted Type-AD frame (Sequence-Controlled Service).
    ALDIS_FARM1_FDU_AD = 0,
    /// The FDU of an accepted Type-BD frame (Expedited Service).
    ALDIS_FARM1_FDU_BD = 1,
}
mirror!(pub aldis_farm1_fdu: u32 { ALDIS_FARM1_FDU_AD, ALDIS_FARM1_FDU_BD });

/// FARM-1 in caller storage: the caller declares an object of this type, and the pointer to it is
/// the handle (COP-002, CABI-031, CABI-032).
///
/// The words are the library's: a C caller never reads or writes them.
#[repr(C)]
pub struct aldis_farm1_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_FARM1_WORDS],
}

impl Storage for aldis_farm1_t {
    type State = Farm1;
    const MAGIC: u32 = 0x414C_4631; // "ALF1"
}

/// The managed parameters of one FARM-1 and its start (232.1 §6.1, §7.3 Table 7-2; COP-030,
/// COP-035).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_farm1_config_t {
    /// The Virtual Channel Identifier the CLCW reports, 0 to 63 (232.0 §4.2.1.6).
    pub vc_id: u8,
    /// The Status Field of the CLCW, 0 to 7, passed through (232.0 §4.2.1.4).
    pub status: u8,
    /// 1 when Type-AD retransmission is allowed (PW = NW = `window` / 2, `window` even from 2 to
    /// 254), 0 when it is not (`window` is W from 1 to 256, `positive_window` is PW) (232.1
    /// §6.1.8.3).
    pub retransmission_allowed: u8,
    /// The FARM Sliding Window Width W (232.1 §6.1.8.2).
    pub window: u16,
    /// The Positive Window Width PW, read only when `retransmission_allowed` is 0.
    pub positive_window: u16,
    /// The initial V(R), 0 to 255 (232.1 §6.1.7).
    pub v_r: u8,
}

/// One input of FARM-1 (API-057). A field that the kind does not use is ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_farm1_input_t {
    /// `ALDIS_FARM1_INPUT_*`.
    pub kind: u32,
    /// N(S) of the Type-AD frame (232.0 §4.1.2.8).
    pub sequence_number: u8,
    /// V*(R) of the Set V(R) command (232.0 §4.1.3.3).
    pub v_star: u8,
    /// The Virtual Channel Identifier of a frame input (`FRAME_*`), 0 to 63; a value other than
    /// the `vc_id` of the configuration is refused with `ALDIS_ERR_UNKNOWN_CHANNEL` (232.1
    /// §3.3.1; COP-037). Not read by the other kinds.
    pub vc_id: u8,
    /// 1 when the caller has a buffer for the FDU of a Type-AD frame, 0 when not; it selects E1
    /// or E2 for N(S) = V(R) (232.1 §6.3.2.3; COP-033).
    pub buffer_available: u8,
    /// The No RF Available Flag, 0 or 1.
    pub no_rf_available: u8,
    /// The No Bit Lock Flag, 0 or 1.
    pub no_bit_lock: u8,
    /// The data field of a Type-AD or Type-BD frame, read once and copied to `out` when accepted
    /// (API-060); `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
}

/// One action of FARM-1 (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_farm1_action_t {
    /// `ALDIS_FARM1_ACTION_*`.
    pub kind: u32,
    /// For `DELIVER`: `ALDIS_FARM1_FDU_*`.
    pub fdu_kind: u32,
    /// For `DELIVER`: N(S) of the frame, 0 for a Type-BD frame.
    pub sequence_number: u8,
    /// For `DELIVER`: where the FDU is in `out`.
    pub data: aldis_range_t,
    /// For `FRAME_DISCARDED` and `STATE_CHANGED`: the number of the event, 1 to 11 (Table 6-1).
    pub event: u8,
    /// For `FRAME_DISCARDED`: the state before the event; for `STATE_CHANGED`: the state before.
    /// 1 to 3 for S1 to S3.
    pub state: u8,
    /// For `STATE_CHANGED`: the state after, 1 to 3.
    pub to_state: u8,
}

/// The actions of one input, in the order of the cell (API-057, COP-006).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_farm1_actions_t {
    /// The number of actions in `items`, at most `ALDIS_FARM1_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_farm1_action_t; ALDIS_FARM1_MAX_ACTIONS],
}

/// A read-only snapshot of FARM-1 (232.1 §6.1, §6.3.2.4; API-056, COP-035).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_farm1_status_t {
    /// The State variable, 1 to 3 for S1 to S3.
    pub state: u8,
    /// V(R).
    pub v_r: u8,
    /// The Lockout Flag, 0 or 1.
    pub lockout: u8,
    /// The Wait Flag, 0 or 1.
    pub wait: u8,
    /// The Retransmit Flag, 0 or 1.
    pub retransmit: u8,
    /// The FARM-B Counter, 0 to 3.
    pub farm_b_counter: u8,
    /// The CLCW of event E11, "Report", as the 32-bit Operational Control Field word.
    pub clcw: u32,
}

/// The number of a FARM-1 state: 1 to 3 for S1 to S3 (Table 6-1).
const fn state_number(s: Farm1State) -> u8 {
    match s {
        Farm1State::Open => 1,
        Farm1State::Wait => 2,
        Farm1State::Lockout => 3,
    }
}

/// The number of a FARM-1 event: 1 to 11 (Table 6-1).
const fn event_number(e: Farm1Event) -> u8 {
    match e {
        Farm1Event::E1 => 1,
        Farm1Event::E2 => 2,
        Farm1Event::E3 => 3,
        Farm1Event::E4 => 4,
        Farm1Event::E5 => 5,
        Farm1Event::E6 => 6,
        Farm1Event::E7 => 7,
        Farm1Event::E8 => 8,
        Farm1Event::E9 => 9,
        Farm1Event::E10 => 10,
        Farm1Event::E11 => 11,
    }
}

/// The C action of an engine output.
const fn action(output: &Farm1Output) -> aldis_farm1_action_t {
    let none = aldis_farm1_action_t {
        kind: 0,
        fdu_kind: 0,
        sequence_number: 0,
        data: aldis_range_t { offset: 0, len: 0 },
        event: 0,
        state: 0,
        to_state: 0,
    };
    match *output {
        Farm1Output::Deliver {
            kind,
            sequence_number,
            data,
        } => aldis_farm1_action_t {
            kind: ALDIS_FARM1_ACTION_DELIVER,
            fdu_kind: match kind {
                FduKind::Ad => ALDIS_FARM1_FDU_AD,
                FduKind::Bd => ALDIS_FARM1_FDU_BD,
            },
            sequence_number: sequence_number.get(),
            data: aldis_range_t {
                offset: data.start(),
                len: data.len(),
            },
            ..none
        },
        Farm1Output::Notify(Farm1Notify::FrameDiscarded { event, state }) => aldis_farm1_action_t {
            kind: ALDIS_FARM1_ACTION_FRAME_DISCARDED,
            event: event_number(event),
            state: state_number(state),
            ..none
        },
        Farm1Output::Notify(Farm1Notify::StateChanged { event, from, to }) => {
            aldis_farm1_action_t {
                kind: ALDIS_FARM1_ACTION_STATE_CHANGED,
                event: event_number(event),
                state: state_number(from),
                to_state: state_number(to),
                ..none
            }
        }
        Farm1Output::Notify(Farm1Notify::Unlocked) => aldis_farm1_action_t {
            kind: ALDIS_FARM1_ACTION_UNLOCKED,
            ..none
        },
    }
}

/// The engine configuration of `cfg` (232.1 §6.1.8, §7.3; COP-030).
fn config(cfg: &aldis_farm1_config_t) -> Result<Farm1Config> {
    let window = if flag(cfg.retransmission_allowed)? {
        Farm1Window::retransmission_allowed(cfg.window)?
    } else {
        Farm1Window::retransmission_not_allowed(cfg.window, cfg.positive_window)?
    };
    Ok(Farm1Config {
        vc_id: TcVcId::try_from(cfg.vc_id)?,
        status: ClcwStatus::try_from(cfg.status)?,
        window,
    })
}

/// The engine input of `input`, with the FDU it borrows (CABI-040: parsed once).
fn engine_input<'a>(input: &aldis_farm1_input_t, data: &'a [u8]) -> Result<Farm1Input<'a>> {
    let frame = |frame| {
        Ok(Farm1Input::Frame {
            vc_id: TcVcId::try_from(input.vc_id)?,
            frame,
            buffer_available: flag(input.buffer_available)?,
        })
    };
    match parse::<aldis_farm1_input>(input.kind)? {
        aldis_farm1_input::ALDIS_FARM1_INPUT_FRAME_AD => frame(Farm1Frame::Ad {
            sequence_number: FrameSeqNum::new(input.sequence_number),
            data,
        }),
        aldis_farm1_input::ALDIS_FARM1_INPUT_FRAME_BD => frame(Farm1Frame::Bd { data }),
        aldis_farm1_input::ALDIS_FARM1_INPUT_FRAME_BC_UNLOCK => frame(Farm1Frame::Bc {
            command: Some(ControlCommand::Unlock),
        }),
        aldis_farm1_input::ALDIS_FARM1_INPUT_FRAME_BC_SET_VR => frame(Farm1Frame::Bc {
            command: Some(ControlCommand::SetVr(FrameSeqNum::new(input.v_star))),
        }),
        aldis_farm1_input::ALDIS_FARM1_INPUT_FRAME_BC_OTHER => {
            frame(Farm1Frame::Bc { command: None })
        }
        aldis_farm1_input::ALDIS_FARM1_INPUT_INVALID_FRAME => Ok(Farm1Input::InvalidFrame),
        aldis_farm1_input::ALDIS_FARM1_INPUT_BUFFER_RELEASED => Ok(Farm1Input::BufferReleased),
        aldis_farm1_input::ALDIS_FARM1_INPUT_SET_PHYSICAL_FLAGS => {
            Ok(Farm1Input::SetPhysicalFlags {
                no_rf_available: flag(input.no_rf_available)?,
                no_bit_lock: flag(input.no_bit_lock)?,
            })
        }
    }
}

/// Places a FARM-1 in State S1 (Open) in `storage` (232.1 §6.1; COP-002; CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, or a flag that is not 0
/// or 1; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `clcw_status`); `ALDIS_ERR_INVALID_CONFIG`
/// (`farm_sliding_window_w`, `farm_positive_window_pw`, `farm_negative_window_nw`) for a window
/// that 232.1 §6.1.8.3 and Table 7-2 do not allow.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_farm1_t` and outlives its use; `cfg` is valid for
/// a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_farm1_init(
    storage: *mut aldis_farm1_t,
    cfg: *const aldis_farm1_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let engine = Farm1::new(config(&cfg)?, FrameSeqNum::new(cfg.v_r));
            handle::init(storage, engine)
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn engine(h: *mut aldis_farm1_t) -> Result<handle::Handle<Farm1>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Applies one input to FARM-1 and returns the actions of its cell (232.1 §6.3, Table 6-1; COP-002,
/// COP-006, COP-033, COP-035, CABI-040).
///
/// The FDU of an accepted frame is written to `out` from offset 0 and named by the `DELIVER`
/// action (API-061). `now_us` is the caller's clock in microseconds; FARM-1 has no timer and
/// ignores it.
///
/// # Errors
/// `ALDIS_ERR_INVALID_STATE` for the one cell Table 6-1 marks "Not applicable" (a Type-AD frame
/// with N(S) = V(R) and a buffer available in S2: feed `BUFFER_RELEASED` first, COP-033);
/// `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`) if the FDU does not fit `out_cap`;
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`vc_id`) for a frame input of another VC than the configured one
/// (COP-037); `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`) for a frame input with a `vc_id` above 63;
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for an input `kind` with no constant, a
/// flag that is not 0 or 1, or a CABI-022 violation (`NULL`, misalignment, a wrapping range, or
/// `out` or `actions` overlapping the storage, `input->data`, or each other). On an error the state
/// is unchanged and `actions` is not written (API-058).
///
/// # Safety
/// `h` is a handle from `aldis_farm1_init` whose storage is still valid, or detectably invalid
/// storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is valid for
/// `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is valid for one
/// write; `err` is `NULL` or valid for one write. No other thread uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_farm1_step(
    h: *mut aldis_farm1_t,
    input: *const aldis_farm1_input_t,
    now_us: u64,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_farm1_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = engine(h)?;
            // The input struct is copied; the buffers of the call are checked against each other
            // and against the storage that `update` lends as `&mut` (CABI-022, c-abi.md §12.2).
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
            let input = engine_input(&value, data)?;
            let list = h.update(|engine| {
                let (next, list) = engine.step(input, Instant::from_micros(now_us), area)?;
                *engine = next;
                Ok::<_, Error>(list)
            })?;
            let mut result = aldis_farm1_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = action(output);
            }
            // `list` holds at most `FARM1_MAX_ACTIONS` actions, the length of `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the state of FARM-1 and its CLCW (232.1 §6.1, §6.3.2.4, E11; API-056, COP-035,
/// CABI-040). It changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_farm1_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_farm1_status(
    h: *mut aldis_farm1_t,
    status: *mut aldis_farm1_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let snapshot = h.read(|engine| aldis_farm1_status_t {
                state: state_number(engine.state()),
                v_r: engine.v_r().get(),
                lockout: u8::from(engine.lockout_flag()),
                wait: u8::from(engine.wait_flag()),
                retransmit: u8::from(engine.retransmit_flag()),
                farm_b_counter: engine.farm_b_counter().get(),
                clcw: engine.clcw().to_word(),
            });
            status.write(snapshot);
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_farm1_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_farm1_deinit(h: *mut aldis_farm1_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
