//! C ABI of the FOP-1 engine in caller-owned storage (232.1 §5; COP-001, COP-010…017,
//! CABI-030…032, CABI-040).
//!
//! The caller declares an `aldis_fop1_t`, `aldis_fop1_init` places an engine in it (the pointer to
//! it is the handle), and every input of Table 5-1 is one `aldis_fop1_step` call that returns the
//! actions of its cell in a fixed-capacity `aldis_fop1_actions_t`. The frames to transmit go to
//! the caller's `out` area and the actions hold their `aldis_range_t` (API-061). The deadline of
//! timer T1 and a snapshot of the state are read-only calls (`aldis_fop1_next_deadline`,
//! `aldis_fop1_status`); the caller feeds the `TIMEOUT` input when the deadline has passed (API-053).
//! There is no `poll_*` call and no callback (CABI-040).
//!
//! The engine builds TC Transfer Frames of one Virtual Channel without Segment Header and SDLS
//! (232.0 §4.1). The USLP target of `Fop1Target::uslp` (COP-010, USLP-013) has no C form yet.
//! The `Sent_Queue` and the frame size are fixed when the library is built:
//! `ALDIS_FOP1_SENT_QUEUE_FRAMES` and `ALDIS_FOP1_MAX_FRAME_OCTETS` (CABI-030).

use core::num::NonZeroU8;

use aldis::cop1::{
    Alert, AlertReason, Directive, DirectiveNotification, Fop1, Fop1Config, Fop1Event, Fop1Input,
    Fop1Output, Fop1State, Fop1Target, FrameKind, SuspendState, TimeoutType, TransferNotification,
    fop1_max_actions,
};
use aldis::core::ids::FrameSeqNum;
use aldis::core::time::{Duration, Instant};
use aldis::core::{Error, ErrorKind, Result, field};
use aldis::tc::{Clcw, TcSpacecraftId, TcVcId};

use crate::abi::{self, flag};
use crate::capacities::{ALDIS_FOP1_MAX_FRAME_OCTETS, ALDIS_FOP1_SENT_QUEUE_FRAMES};
use crate::handle::{self, Storage};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The most actions one `aldis_fop1_step` returns (COP-006): `fop1_max_actions` of the
/// `Sent_Queue` capacity, computed from Table 5-1.
///
/// The header publishes the literal because the array length of
/// `aldis_fop1_actions_t::items` must be one constant; the assertion below fails the build when
/// it differs from the engine's bound.
pub const ALDIS_FOP1_MAX_ACTIONS: usize = 35;

const _: () = assert!(ALDIS_FOP1_MAX_ACTIONS == fop1_max_actions(ALDIS_FOP1_SENT_QUEUE_FRAMES));

/// 64-bit words of one `aldis_fop1_t` (CABI-031).
///
/// The tag and the engine state at `ALDIS_FOP1_SENT_QUEUE_FRAMES` frames of
/// `ALDIS_FOP1_MAX_FRAME_OCTETS` octets; the build fails when the state outgrows it.
pub const ALDIS_FOP1_WORDS: usize = 2252;

/// The engine at the build capacities (CABI-030).
type Engine = Fop1<
    { ALDIS_FOP1_SENT_QUEUE_FRAMES },
    { ALDIS_FOP1_MAX_FRAME_OCTETS },
    { ALDIS_FOP1_MAX_ACTIONS },
>;

/// The kind of an `aldis_fop1_input_t` (232.1 §5.3.2, Table 5-1; API-057).
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
pub enum aldis_fop1_input {
    /// A CLCW arrives (`clcw`): E1 to E15, E101 to E103. A word whose Control Word Type is 1 is
    /// E15 (232.1 §5.3.2.1).
    ALDIS_FOP1_INPUT_CLCW = 0,
    /// Timer T1 may have expired: E16 to E18 and E104 once `now_us` reaches the deadline,
    /// otherwise nothing (API-053).
    ALDIS_FOP1_INPUT_TIMEOUT = 1,
    /// A Request to Transfer FDU of the Sequence-Controlled Service (`handle`, `data`): E19, E20.
    ALDIS_FOP1_INPUT_REQUEST_AD = 2,
    /// A Request to Transfer FDU of the Expedited Service (`handle`, `data`): E21, E22.
    ALDIS_FOP1_INPUT_REQUEST_BD = 3,
    /// A directive (`handle`, `directive`, `value`): E23 to E40.
    ALDIS_FOP1_INPUT_DIRECTIVE = 4,
    /// A directive the caller could not parse (`handle`): E40.
    ALDIS_FOP1_INPUT_INVALID_DIRECTIVE = 5,
    /// The response of the Lower Procedures to the outstanding Transmit Request (`frame_kind`,
    /// `accepted`): E41 to E46.
    ALDIS_FOP1_INPUT_TRANSMIT_DONE = 6,
}
mirror!(pub aldis_fop1_input: u32 {
    ALDIS_FOP1_INPUT_CLCW,
    ALDIS_FOP1_INPUT_TIMEOUT,
    ALDIS_FOP1_INPUT_REQUEST_AD,
    ALDIS_FOP1_INPUT_REQUEST_BD,
    ALDIS_FOP1_INPUT_DIRECTIVE,
    ALDIS_FOP1_INPUT_INVALID_DIRECTIVE,
    ALDIS_FOP1_INPUT_TRANSMIT_DONE
});

/// The directive of an `ALDIS_FOP1_INPUT_DIRECTIVE` (232.1 §4.2, Table 4-1; COP-015).
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
pub enum aldis_fop1_directive {
    /// Initiate AD Service without CLCW Check: E23.
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK = 0,
    /// Initiate AD Service with CLCW Check: E24.
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_CLCW_CHECK = 1,
    /// Initiate AD Service with Unlock: E25, E26.
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_UNLOCK = 2,
    /// Initiate AD Service with Set V(R) (`value` is V*(R), 0 to 255): E27, E28.
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_SET_VR = 3,
    /// Terminate AD Service: E29.
    ALDIS_FOP1_DIRECTIVE_TERMINATE_AD = 4,
    /// Resume AD Service: E30 to E34.
    ALDIS_FOP1_DIRECTIVE_RESUME_AD = 5,
    /// Set V(S) (`value` is V*(S), 0 to 255): E35.
    ALDIS_FOP1_DIRECTIVE_SET_VS = 6,
    /// Set the FOP Sliding Window Width K (`value`, 1 to the `Sent_Queue` capacity): E36.
    ALDIS_FOP1_DIRECTIVE_SET_K = 7,
    /// Set `T1_Initial` (`value` in microseconds, at least 1): E37.
    ALDIS_FOP1_DIRECTIVE_SET_T1_INITIAL = 8,
    /// Set `Transmission_Limit` (`value`, 1 to 255): E38.
    ALDIS_FOP1_DIRECTIVE_SET_TRANSMISSION_LIMIT = 9,
    /// Set `Timeout_Type` (`value` is 0 for TT0 or 1 for TT1): E39.
    ALDIS_FOP1_DIRECTIVE_SET_TIMEOUT_TYPE = 10,
}
mirror!(pub aldis_fop1_directive: u32 {
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK,
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_CLCW_CHECK,
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_UNLOCK,
    ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_SET_VR,
    ALDIS_FOP1_DIRECTIVE_TERMINATE_AD,
    ALDIS_FOP1_DIRECTIVE_RESUME_AD,
    ALDIS_FOP1_DIRECTIVE_SET_VS,
    ALDIS_FOP1_DIRECTIVE_SET_K,
    ALDIS_FOP1_DIRECTIVE_SET_T1_INITIAL,
    ALDIS_FOP1_DIRECTIVE_SET_TRANSMISSION_LIMIT,
    ALDIS_FOP1_DIRECTIVE_SET_TIMEOUT_TYPE
});

/// The kind of frame a transmit request or response names (232.1 §5.1.6).
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
pub enum aldis_fop1_frame {
    /// A Type-AD frame (Sequence-Controlled Service).
    ALDIS_FOP1_FRAME_AD = 0,
    /// A Type-BC frame (Control Command).
    ALDIS_FOP1_FRAME_BC = 1,
    /// A Type-BD frame (Expedited Service).
    ALDIS_FOP1_FRAME_BD = 2,
}
mirror!(pub aldis_fop1_frame: u32 {
    ALDIS_FOP1_FRAME_AD,
    ALDIS_FOP1_FRAME_BC,
    ALDIS_FOP1_FRAME_BD
});

/// The `Timeout_Type` parameter (232.1 §5.1.10.3, Table 7-1).
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
pub enum aldis_fop1_timeout {
    /// TT0: an Alert when the transmission limit is reached.
    ALDIS_FOP1_TIMEOUT_TT0 = 0,
    /// TT1: the AD Service is suspended when the transmission limit is reached.
    ALDIS_FOP1_TIMEOUT_TT1 = 1,
}
mirror!(pub aldis_fop1_timeout: u32 { ALDIS_FOP1_TIMEOUT_TT0, ALDIS_FOP1_TIMEOUT_TT1 });

/// The kind of an `aldis_fop1_action_t` (232.1 §4, §5.2; API-057, API-083).
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
pub enum aldis_fop1_action {
    /// A Transmit Request for Frame: the frame is at `frame` in `out` (`frame_kind`,
    /// `sequence_number`, `retransmission`, `repetitions`).
    ALDIS_FOP1_ACTION_TRANSMIT = 0,
    /// An Abort Request to the Lower Procedures (232.1 §5.2.7 a).
    ALDIS_FOP1_ACTION_ABORT = 1,
    /// A notification of a transfer request (`handle`, `status`).
    ALDIS_FOP1_ACTION_TRANSFER_NOTIFY = 2,
    /// A notification of a directive (`handle`, `status`).
    ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY = 3,
    /// An Alert (`reason`, `event`, `state`, the state before the event).
    ALDIS_FOP1_ACTION_ALERT = 4,
    /// The Suspend notification (232.1 §4.3.3).
    ALDIS_FOP1_ACTION_SUSPEND = 5,
}
mirror!(pub aldis_fop1_action: u32 {
    ALDIS_FOP1_ACTION_TRANSMIT,
    ALDIS_FOP1_ACTION_ABORT,
    ALDIS_FOP1_ACTION_TRANSFER_NOTIFY,
    ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY,
    ALDIS_FOP1_ACTION_ALERT,
    ALDIS_FOP1_ACTION_SUSPEND
});

/// The status of a transfer or directive notification (232.1 §4.2 Table 4-2, §4.4 Table 4-5).
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
pub enum aldis_fop1_notice {
    /// The request or directive is accepted.
    ALDIS_FOP1_NOTICE_ACCEPT = 0,
    /// The request or directive is rejected.
    ALDIS_FOP1_NOTICE_REJECT = 1,
    /// Positive confirmation.
    ALDIS_FOP1_NOTICE_POSITIVE_CONFIRM = 2,
    /// Negative confirmation.
    ALDIS_FOP1_NOTICE_NEGATIVE_CONFIRM = 3,
}
mirror!(pub aldis_fop1_notice: u32 {
    ALDIS_FOP1_NOTICE_ACCEPT,
    ALDIS_FOP1_NOTICE_REJECT,
    ALDIS_FOP1_NOTICE_POSITIVE_CONFIRM,
    ALDIS_FOP1_NOTICE_NEGATIVE_CONFIRM
});

/// The reason of an Alert (232.1 §4.3, Table 4-4; COP-004).
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
pub enum aldis_fop1_alert {
    /// The transmission limit is reached for a Type-AD or Type-BC frame.
    ALDIS_FOP1_ALERT_LIMIT = 0,
    /// Timer T1 expired with the transmission limit reached, or in S4 or S5.
    ALDIS_FOP1_ALERT_T1 = 1,
    /// The Lockout Flag is set in the CLCW.
    ALDIS_FOP1_ALERT_LOCKOUT = 2,
    /// Synchronization lost between transmitter and receiver.
    ALDIS_FOP1_ALERT_SYNCH = 3,
    /// An invalid N(R) in the CLCW.
    ALDIS_FOP1_ALERT_NN_R = 4,
    /// An invalid CLCW.
    ALDIS_FOP1_ALERT_CLCW = 5,
    /// A Lower-Layer Interface failure.
    ALDIS_FOP1_ALERT_LLIF = 6,
    /// The AD Service was terminated by the Terminate AD directive.
    ALDIS_FOP1_ALERT_TERM = 7,
}
mirror!(pub aldis_fop1_alert: u32 {
    ALDIS_FOP1_ALERT_LIMIT,
    ALDIS_FOP1_ALERT_T1,
    ALDIS_FOP1_ALERT_LOCKOUT,
    ALDIS_FOP1_ALERT_SYNCH,
    ALDIS_FOP1_ALERT_NN_R,
    ALDIS_FOP1_ALERT_CLCW,
    ALDIS_FOP1_ALERT_LLIF,
    ALDIS_FOP1_ALERT_TERM
});

/// FOP-1 in caller storage: the caller declares an object of this type, and the pointer to it is
/// the handle (COP-001, CABI-031, CABI-032).
///
/// The words are the library's: a C caller never reads or writes them.
#[repr(C)]
pub struct aldis_fop1_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_FOP1_WORDS],
}

impl Storage for aldis_fop1_t {
    type State = Engine;
    const MAGIC: u32 = 0x414C_4650; // "ALFP"
}

/// The managed parameters of one FOP-1 at start-up (232.1 §7.2 Table 7-1; COP-010, COP-015,
/// COP-016). FOP-1 starts in S6 (Initial).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_fop1_config_t {
    /// The Spacecraft Identifier, 0 to 1023 (232.0 §4.1.2.5).
    pub scid: u16,
    /// The Virtual Channel Identifier, 0 to 63 (232.0 §4.1.2.6).
    pub vc_id: u8,
    /// 1 when the frames carry the Frame Error Control Field, 0 when not (232.0 §4.1.4).
    pub fecf: u8,
    /// The Maximum Transfer Frame Length, 6 to `ALDIS_FOP1_MAX_FRAME_OCTETS` octets, the most
    /// that one input writes to `out`.
    pub max_frame_len: usize,
    /// `T1_Initial` in microseconds, at least 1 (232.1 §5.1.9).
    pub t1_initial_us: u64,
    /// `Transmission_Limit`, 1 to 255 (232.1 §5.1.10.2).
    pub transmission_limit: u8,
    /// `ALDIS_FOP1_TIMEOUT_*` (232.1 §5.1.10.3).
    pub timeout_type: u32,
    /// The FOP Sliding Window Width K, 1 to `ALDIS_FOP1_SENT_QUEUE_FRAMES` (232.1 §5.1.12).
    pub sliding_window_k: u8,
    /// The systematic retransmission value of Type-AD frames, passed through on `TRANSMIT`
    /// actions (232.1 §5.1.9.3 note 1; COP-016).
    pub ad_repetitions: u8,
    /// The systematic retransmission value of Type-BC frames (COP-016).
    pub bc_repetitions: u8,
}

/// One input of FOP-1 (API-057). A field that the kind does not use is ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_fop1_input_t {
    /// `ALDIS_FOP1_INPUT_*`.
    pub kind: u32,
    /// The handle of a request or directive, chosen by the caller and returned in its
    /// notifications (API-054).
    pub handle: u32,
    /// For `CLCW`: the 32-bit CLCW as received (232.0 §4.2), the most significant octet first.
    pub clcw: u32,
    /// For `DIRECTIVE`: `ALDIS_FOP1_DIRECTIVE_*`.
    pub directive: u32,
    /// For `DIRECTIVE`: the qualifier of the directive. A value outside the range of the
    /// directive makes it E40 (232.1 Table 5-1).
    pub value: u64,
    /// For `TRANSMIT_DONE`: `ALDIS_FOP1_FRAME_*`, the kind of frame the response answers.
    pub frame_kind: u32,
    /// For `TRANSMIT_DONE`: 1 for an Accept Response, 0 for a Reject Response.
    pub accepted: u8,
    /// For `REQUEST_AD` and `REQUEST_BD`: the FDU, 1 to the Maximum Transfer Frame Length less
    /// the frame overhead octets, read once and copied (API-060); `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
}

/// One action of FOP-1 (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_fop1_action_t {
    /// `ALDIS_FOP1_ACTION_*`.
    pub kind: u32,
    /// For `TRANSMIT`: `ALDIS_FOP1_FRAME_*`.
    pub frame_kind: u32,
    /// For `TRANSMIT`: N(S), V(S) for a Type-AD frame and 0 for the others.
    pub sequence_number: u8,
    /// For `TRANSMIT`: 1 for a retransmission of a master copy, else 0.
    pub retransmission: u8,
    /// For `TRANSMIT`: the configured systematic retransmission value, 0 for a Type-BD frame.
    pub repetitions: u8,
    /// For `TRANSMIT`: where the frame is in `out`.
    pub frame: aldis_range_t,
    /// For `TRANSFER_NOTIFY` and `DIRECTIVE_NOTIFY`: the handle of the request or directive.
    pub handle: u32,
    /// For `TRANSFER_NOTIFY` and `DIRECTIVE_NOTIFY`: `ALDIS_FOP1_NOTICE_*`.
    pub status: u32,
    /// For `ALERT`: `ALDIS_FOP1_ALERT_*`.
    pub reason: u32,
    /// For `ALERT`: the number of the event that caused it, 1 to 46 or 101 to 104.
    pub event: u16,
    /// For `ALERT`: the state before the event, 1 to 6 for S1 to S6.
    pub state: u8,
}

/// The actions of one input, in the order of the cell (API-057, COP-006).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_fop1_actions_t {
    /// The number of actions in `items`, at most `ALDIS_FOP1_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_fop1_action_t; ALDIS_FOP1_MAX_ACTIONS],
}

impl Default for aldis_fop1_actions_t {
    fn default() -> aldis_fop1_actions_t {
        aldis_fop1_actions_t {
            count: 0,
            items: [aldis_fop1_action_t::default(); ALDIS_FOP1_MAX_ACTIONS],
        }
    }
}

/// A read-only snapshot of FOP-1 (232.1 §5.1; API-056).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_fop1_status_t {
    /// The State variable, 1 to 6 for S1 to S6.
    pub state: u8,
    /// V(S).
    pub v_s: u8,
    /// NN(R).
    pub nn_r: u8,
    /// The FOP Sliding Window Width K.
    pub sliding_window_k: u8,
    /// `Transmission_Limit`.
    pub transmission_limit: u8,
    /// `Transmission_Count`.
    pub transmission_count: u8,
    /// `Suspend_State` SS, 0 to 4.
    pub suspend_state: u8,
    /// 1 when the Type-AD Out flag is Ready, else 0.
    pub ad_out_ready: u8,
    /// 1 when the Type-BC Out flag is Ready, else 0.
    pub bc_out_ready: u8,
    /// 1 when the Type-BD Out flag is Ready, else 0.
    pub bd_out_ready: u8,
    /// 1 when the `Wait_Queue` holds a request, else 0.
    pub wait_queue_occupied: u8,
    /// `ALDIS_FOP1_TIMEOUT_*`.
    pub timeout_type: u32,
    /// The number of frames on the `Sent_Queue`.
    pub sent_queue_len: usize,
    /// `T1_Initial` in microseconds.
    pub t1_initial_us: u64,
}

/// The number of a FOP-1 state: 1 to 6 for S1 to S6 (Table 5-1).
const fn state_number(s: Fop1State) -> u8 {
    match s {
        Fop1State::Active => 1,
        Fop1State::RetransmitWithoutWait => 2,
        Fop1State::RetransmitWithWait => 3,
        Fop1State::InitializingWithoutBcFrame => 4,
        Fop1State::InitializingWithBcFrame => 5,
        Fop1State::Initial => 6,
    }
}

/// The number of a FOP-1 event: 1 to 46 or 101 to 104 (Table 5-1). The events are declared in
/// the order of the table, so the number follows from the position.
#[expect(
    clippy::as_conversions,
    reason = "the discriminant of a fieldless `repr(usize)` enum, its position in Table 5-1"
)]
fn event_number(e: Fop1Event) -> u16 {
    let position = u16::try_from(e as usize).unwrap_or(u16::MAX);
    // E1 to E46 are positions 0 to 45; E101 to E104 are positions 46 to 49.
    if position < 46 {
        position.saturating_add(1)
    } else {
        position.saturating_add(55)
    }
}

/// The C constant of a frame kind.
const fn frame_kind(k: FrameKind) -> u32 {
    match k {
        FrameKind::Ad => ALDIS_FOP1_FRAME_AD,
        FrameKind::Bc => ALDIS_FOP1_FRAME_BC,
        FrameKind::Bd => ALDIS_FOP1_FRAME_BD,
    }
}

/// The engine frame kind of a C constant.
const fn engine_frame_kind(k: aldis_fop1_frame) -> FrameKind {
    match k {
        aldis_fop1_frame::ALDIS_FOP1_FRAME_AD => FrameKind::Ad,
        aldis_fop1_frame::ALDIS_FOP1_FRAME_BC => FrameKind::Bc,
        aldis_fop1_frame::ALDIS_FOP1_FRAME_BD => FrameKind::Bd,
    }
}

/// The C constant of a transfer notification status.
const fn transfer_status(s: TransferNotification) -> u32 {
    match s {
        TransferNotification::Accept => ALDIS_FOP1_NOTICE_ACCEPT,
        TransferNotification::Reject => ALDIS_FOP1_NOTICE_REJECT,
        TransferNotification::PositiveConfirm => ALDIS_FOP1_NOTICE_POSITIVE_CONFIRM,
        TransferNotification::NegativeConfirm => ALDIS_FOP1_NOTICE_NEGATIVE_CONFIRM,
    }
}

/// The C constant of a directive notification status.
const fn directive_status(s: DirectiveNotification) -> u32 {
    match s {
        DirectiveNotification::Accept => ALDIS_FOP1_NOTICE_ACCEPT,
        DirectiveNotification::Reject => ALDIS_FOP1_NOTICE_REJECT,
        DirectiveNotification::PositiveConfirm => ALDIS_FOP1_NOTICE_POSITIVE_CONFIRM,
        DirectiveNotification::NegativeConfirm => ALDIS_FOP1_NOTICE_NEGATIVE_CONFIRM,
    }
}

/// The C constant of an alert reason.
const fn alert_reason(r: AlertReason) -> u32 {
    match r {
        AlertReason::Limit => ALDIS_FOP1_ALERT_LIMIT,
        AlertReason::T1 => ALDIS_FOP1_ALERT_T1,
        AlertReason::Lockout => ALDIS_FOP1_ALERT_LOCKOUT,
        AlertReason::Synch => ALDIS_FOP1_ALERT_SYNCH,
        AlertReason::NnR => ALDIS_FOP1_ALERT_NN_R,
        AlertReason::Clcw => ALDIS_FOP1_ALERT_CLCW,
        AlertReason::Llif => ALDIS_FOP1_ALERT_LLIF,
        AlertReason::Term => ALDIS_FOP1_ALERT_TERM,
    }
}

/// The C action of an engine output.
fn action(output: &Fop1Output) -> aldis_fop1_action_t {
    let none = aldis_fop1_action_t::default();
    match *output {
        Fop1Output::Transmit {
            kind,
            sequence_number,
            retransmission,
            repetitions,
            frame,
        } => aldis_fop1_action_t {
            kind: ALDIS_FOP1_ACTION_TRANSMIT,
            frame_kind: frame_kind(kind),
            sequence_number: sequence_number.get(),
            retransmission: u8::from(retransmission),
            repetitions,
            frame: aldis_range_t {
                offset: frame.start(),
                len: frame.len(),
            },
            ..none
        },
        Fop1Output::Abort => aldis_fop1_action_t {
            kind: ALDIS_FOP1_ACTION_ABORT,
            ..none
        },
        Fop1Output::Transfer(n) => aldis_fop1_action_t {
            kind: ALDIS_FOP1_ACTION_TRANSFER_NOTIFY,
            handle: n.handle,
            status: transfer_status(n.status),
            ..none
        },
        Fop1Output::Directive(n) => aldis_fop1_action_t {
            kind: ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY,
            handle: n.handle,
            status: directive_status(n.status),
            ..none
        },
        Fop1Output::Alert(Alert {
            reason,
            event,
            prior_state,
        }) => aldis_fop1_action_t {
            kind: ALDIS_FOP1_ACTION_ALERT,
            reason: alert_reason(reason),
            event: event_number(event),
            state: state_number(prior_state),
            ..none
        },
        Fop1Output::Suspend => aldis_fop1_action_t {
            kind: ALDIS_FOP1_ACTION_SUSPEND,
            ..none
        },
    }
}

/// The engine configuration of `cfg` (232.1 §7.2; COP-010, COP-015).
fn config(cfg: &aldis_fop1_config_t) -> Result<Fop1Config> {
    let target = Fop1Target::tc(
        TcSpacecraftId::try_from(cfg.scid)?,
        TcVcId::try_from(cfg.vc_id)?,
        cfg.max_frame_len,
        flag(cfg.fecf)?,
    )?;
    let timeout_type = match parse::<aldis_fop1_timeout>(cfg.timeout_type)? {
        aldis_fop1_timeout::ALDIS_FOP1_TIMEOUT_TT0 => TimeoutType::Tt0,
        aldis_fop1_timeout::ALDIS_FOP1_TIMEOUT_TT1 => TimeoutType::Tt1,
    };
    Ok(Fop1Config {
        target,
        t1_initial: Duration::from_micros(cfg.t1_initial_us),
        transmission_limit: NonZeroU8::new(cfg.transmission_limit).ok_or_else(|| {
            Error::new(ErrorKind::InvalidConfig).with_field(field::TRANSMISSION_LIMIT)
        })?,
        timeout_type,
        sliding_window_k: cfg.sliding_window_k,
        ad_repetitions: cfg.ad_repetitions,
        bc_repetitions: cfg.bc_repetitions,
    })
}

/// The directive of `kind` with the qualifier `value`, or `None` when the value does not fit its
/// field, which is E40 (232.1 Table 5-1; COP-015).
fn directive(kind: aldis_fop1_directive, value: u64) -> Option<Directive> {
    let octet = u8::try_from(value).ok();
    Some(match kind {
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITHOUT_CLCW_CHECK => {
            Directive::InitiateAdWithoutClcwCheck
        }
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_CLCW_CHECK => {
            Directive::InitiateAdWithClcwCheck
        }
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_UNLOCK => {
            Directive::InitiateAdWithUnlock
        }
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_INITIATE_AD_WITH_SET_VR => {
            Directive::InitiateAdWithSetVr {
                v_r: FrameSeqNum::new(octet?),
            }
        }
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_TERMINATE_AD => Directive::TerminateAd,
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_RESUME_AD => Directive::ResumeAd,
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_SET_VS => Directive::SetVs {
            v_s: FrameSeqNum::new(octet?),
        },
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_SET_K => {
            Directive::SetSlidingWindowWidth { width: octet? }
        }
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_SET_T1_INITIAL => Directive::SetT1Initial {
            duration: Duration::from_micros(value),
        },
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_SET_TRANSMISSION_LIMIT => {
            Directive::SetTransmissionLimit {
                limit: NonZeroU8::new(octet?)?,
            }
        }
        aldis_fop1_directive::ALDIS_FOP1_DIRECTIVE_SET_TIMEOUT_TYPE => Directive::SetTimeoutType {
            timeout_type: match value {
                0 => TimeoutType::Tt0,
                1 => TimeoutType::Tt1,
                _ => return None,
            },
        },
    })
}

/// The engine input of `input`, with the FDU it borrows (CABI-040: parsed once).
fn engine_input<'a>(input: &aldis_fop1_input_t, data: &'a [u8]) -> Result<Fop1Input<'a>> {
    Ok(match parse::<aldis_fop1_input>(input.kind)? {
        aldis_fop1_input::ALDIS_FOP1_INPUT_CLCW => match Clcw::decode(input.clcw.to_be_bytes()) {
            Ok(clcw) => Fop1Input::Clcw(clcw),
            // A word that is not a CLCW (Control Word Type 1) is E15 (232.1 §5.3.2.1).
            Err(e) if e.field() == Some(aldis::tc::field::CONTROL_WORD_TYPE) => {
                Fop1Input::InvalidClcw
            }
            Err(e) => return Err(e),
        },
        aldis_fop1_input::ALDIS_FOP1_INPUT_TIMEOUT => Fop1Input::Timeout,
        aldis_fop1_input::ALDIS_FOP1_INPUT_REQUEST_AD => Fop1Input::RequestAd {
            handle: input.handle,
            fdu: data,
        },
        aldis_fop1_input::ALDIS_FOP1_INPUT_REQUEST_BD => Fop1Input::RequestBd {
            handle: input.handle,
            fdu: data,
        },
        aldis_fop1_input::ALDIS_FOP1_INPUT_DIRECTIVE => {
            let kind = parse::<aldis_fop1_directive>(input.directive)?;
            match directive(kind, input.value) {
                Some(directive) => Fop1Input::Directive {
                    handle: input.handle,
                    directive,
                },
                None => Fop1Input::InvalidDirective {
                    handle: input.handle,
                },
            }
        }
        aldis_fop1_input::ALDIS_FOP1_INPUT_INVALID_DIRECTIVE => Fop1Input::InvalidDirective {
            handle: input.handle,
        },
        aldis_fop1_input::ALDIS_FOP1_INPUT_TRANSMIT_DONE => Fop1Input::TransmitDone {
            kind: engine_frame_kind(parse::<aldis_fop1_frame>(input.frame_kind)?),
            accepted: flag(input.accepted)?,
        },
    })
}

/// The number of the suspend state: SS 0 to 4.
const fn suspend_number(s: SuspendState) -> u8 {
    match s {
        SuspendState::NotSuspended => 0,
        SuspendState::Active => 1,
        SuspendState::RetransmitWithoutWait => 2,
        SuspendState::RetransmitWithWait => 3,
        SuspendState::InitializingWithoutBcFrame => 4,
    }
}

/// Places a FOP-1 in State S6 (Initial) in `storage`, with V(S) = NN(R) = 0, every Out flag
/// Ready, and no timer (232.1 §5.1; COP-001, OI-59; CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0 or
/// 1, or a `timeout_type` with no constant; `ALDIS_ERR_OUT_OF_RANGE` (`spacecraft_id`, `vc_id`);
/// `ALDIS_ERR_INVALID_CONFIG` (`max_frame_len`, `sliding_window_k`, `t1_initial`,
/// `transmission_limit`) for a value outside its range or a frame length above
/// `ALDIS_FOP1_MAX_FRAME_OCTETS`.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_fop1_t` and outlives its use; `cfg` is valid for a
/// read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_fop1_init(
    storage: *mut aldis_fop1_t,
    cfg: *const aldis_fop1_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let engine = Engine::new(config(&cfg)?)?;
            handle::init(storage, engine)
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn engine(h: *mut aldis_fop1_t) -> Result<handle::Handle<Engine>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Applies one input to FOP-1 and returns the actions of its cell (232.1 §5.3, Table 5-1;
/// COP-001, COP-005, COP-006, COP-010…017, CABI-040).
///
/// A frame to transmit is written to `out` from offset 0 and named by a `TRANSMIT` action
/// (API-061); `out_cap` should be the Maximum Transfer Frame Length of the configuration at
/// least. `now_us` is the caller's monotonic clock in microseconds (API-003).
///
/// # Errors
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`vc_id`) for a CLCW of another Virtual Channel;
/// `ALDIS_ERR_INVALID_FIELD` (`frame_data_field`) for an empty FDU and `ALDIS_ERR_OUT_OF_RANGE`
/// (`frame_len`) for one longer than the frame allows; `ALDIS_ERR_INVALID_STATE` for an input
/// whose cell Table 5-1 marks "Not applicable" (COP-005); `ALDIS_ERR_BUFFER_TOO_SMALL` (with
/// `needed`) if `out_cap` is shorter than the frame to transmit; `ALDIS_ERR_INVALID_HANDLE`;
/// `ALDIS_ERR_INVALID_FIELD` for an input `kind`, directive, or frame kind with no constant, a
/// flag that is not 0 or 1, a CLCW that does not decode, or a CABI-022 violation (`NULL`,
/// misalignment, a wrapping range, or `out` or `actions` overlapping the storage, `input->data`,
/// or each other). On an error the state is unchanged and `actions` is not written (API-058).
///
/// # Safety
/// `h` is a handle from `aldis_fop1_init` whose storage is still valid, or detectably invalid
/// storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is valid for
/// `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is valid for one
/// write; `err` is `NULL` or valid for one write. No other thread uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_fop1_step(
    h: *mut aldis_fop1_t,
    input: *const aldis_fop1_input_t,
    now_us: u64,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_fop1_actions_t,
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
            let mut result = aldis_fop1_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = action(output);
            }
            // `list` holds at most `ALDIS_FOP1_MAX_ACTIONS` actions, the length of `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the deadline of timer T1 (232.1 §5.1.9; COP-013, API-053, CABI-040).
///
/// While the timer runs, `armed` is 1 and `deadline_us` the instant in microseconds at which the
/// caller is to feed the `TIMEOUT` input; otherwise both are 0. It changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_fop1_step`]; `armed` and `deadline_us` are valid for one write each.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_fop1_next_deadline(
    h: *mut aldis_fop1_t,
    armed: *mut u8,
    deadline_us: *mut u64,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = engine(h)?;
            let range = h.range();
            let armed = abi::out_disjoint(armed, &[range])?;
            let deadline_us = abi::out_disjoint(deadline_us, &[range])?;
            let deadline = h.read(Engine::next_deadline);
            armed.write(u8::from(deadline.is_some()));
            deadline_us.write(deadline.map_or(0, Instant::as_micros));
            Ok(())
        })
    }
}

/// Reads the state of FOP-1 (232.1 §5.1; API-056, CABI-040). It changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_fop1_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_fop1_status(
    h: *mut aldis_fop1_t,
    status: *mut aldis_fop1_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let snapshot = h.read(|engine| aldis_fop1_status_t {
                state: state_number(engine.state()),
                v_s: engine.v_s().get(),
                nn_r: engine.nn_r().get(),
                sliding_window_k: engine.sliding_window_k(),
                transmission_limit: engine.transmission_limit().get(),
                transmission_count: engine.transmission_count(),
                suspend_state: suspend_number(engine.suspend_state()),
                ad_out_ready: u8::from(engine.out_flag_ready(FrameKind::Ad)),
                bc_out_ready: u8::from(engine.out_flag_ready(FrameKind::Bc)),
                bd_out_ready: u8::from(engine.out_flag_ready(FrameKind::Bd)),
                wait_queue_occupied: u8::from(engine.wait_queue_occupied()),
                timeout_type: match engine.timeout_type() {
                    TimeoutType::Tt0 => ALDIS_FOP1_TIMEOUT_TT0,
                    TimeoutType::Tt1 => ALDIS_FOP1_TIMEOUT_TT1,
                },
                sent_queue_len: engine.sent_queue_len(),
                t1_initial_us: engine.t1_initial().as_micros(),
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
/// As for [`aldis_fop1_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_fop1_deinit(h: *mut aldis_fop1_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
