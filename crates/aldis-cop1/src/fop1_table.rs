//! FOP-1 State Table 5-1 represented as `const` data (COP-001, COP-003, RS-021).
//!
//! Defined in CCSDS 232.1-B-2 Section 5:
//! - Section 5.1.2 & Section 5.3: Protocol States S1 (`Active`), S2 (`RetransmitWithoutWait`),
//!   S3 (`RetransmitWithWait`), S4 (`InitializingWithoutBcFrame`), S5 (`InitializingWithBcFrame`),
//!   and S6 (`Initial`).
//! - Section 5.3.2 & Section 5.3.3: Events E1 through E46, and E101 through E104 (Rev. B / Cor. 1).
//! - Section 5.3, Table 5-1 (Parts 1–7): State Table.
//! - Section 5.2: FOP-1 Action primitives.
//! - Review record in `docs/verification/reviews/cop1-tables.md` §3 (COP-003, VER-041).

use crate::notify::{AlertReason, DirectiveNotification, TransferNotification};

/// The number of FOP-1 protocol states (S1..S6) defined in CCSDS 232.1-B-2 Table 5-1 (COP-001).
pub const STATE_COUNT: usize = 6;

/// The number of FOP-1 events (E1..E46, E101..E104) defined in CCSDS 232.1-B-2 Table 5-1 (COP-001).
pub const EVENT_COUNT: usize = 50;

/// FOP-1 Protocol States (CCSDS 232.1-B-2 §5.1.2, Table 5-1, COP-001, RS-021).
///
/// FOP-1 operates in six closed protocol states:
/// - S1 ([`Fop1State::Active`]): normal frame transmission and acknowledgement.
/// - S2 ([`Fop1State::RetransmitWithoutWait`]): Retransmit Flag set, Wait Flag reset; retransmission pending.
/// - S3 ([`Fop1State::RetransmitWithWait`]): Wait Flag set; transmissions held until buffer freed.
/// - S4 ([`Fop1State::InitializingWithoutBcFrame`]): waiting for clean CLCW after Initiate AD directive.
/// - S5 ([`Fop1State::InitializingWithBcFrame`]): waiting for clean CLCW after BC frame transmission.
/// - S6 ([`Fop1State::Initial`]): inactive, unconfigured, or suspended; awaits Initiate AD directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum Fop1State {
    /// State S1: Active (normal link operation).
    Active = 0,
    /// State S2: Retransmit without Wait (Retransmit Flag set, Wait Flag reset).
    RetransmitWithoutWait = 1,
    /// State S3: Retransmit with Wait (Wait Flag set in received CLCW).
    RetransmitWithWait = 2,
    /// State S4: Initializing without BC Frame (waiting for clean CLCW after Initiate AD).
    InitializingWithoutBcFrame = 3,
    /// State S5: Initializing with BC Frame (waiting for clean CLCW after BC frame).
    InitializingWithBcFrame = 4,
    /// State S6: Initial (inactive, unconfigured, or suspended; awaits Initiate AD directive).
    Initial = 5,
}

/// FOP-1 Events (CCSDS 232.1-B-2 §5.3, Table 5-1, COP-001, COP-004, RS-021).
///
/// Events E1 through E46 and E101 through E104 (50 variants total) classify:
/// - CLCW arrivals: E1–E15, E101–E103 (Table 5-1 Parts 1–3).
/// - Timer T1 expiration: E16–E18, E104 (Table 5-1 Part 3).
/// - Higher-procedure transfer requests: E19–E22 (Table 5-1 Part 4).
/// - Management directives: E23–E40 (Table 5-1 Parts 4–6).
/// - Lower-procedure responses: E41–E46 (Table 5-1 Part 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum Fop1Event {
    /// E1: Lockout=0, N(R)=V(S), Retransmit=0, Wait=0, N(R)=NN(R).
    E1 = 0,
    /// E2: Lockout=0, N(R)=V(S), Retransmit=0, Wait=0, N(R)≠NN(R).
    E2 = 1,
    /// E3: Lockout=0, N(R)=V(S), Retransmit=0, Wait=1.
    E3 = 2,
    /// E4: Lockout=0, N(R)=V(S), Retransmit=1.
    E4 = 3,
    /// E5: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=0, Wait=0, N(R)=NN(R).
    E5 = 4,
    /// E6: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=0, Wait=0, N(R)≠NN(R).
    E6 = 5,
    /// E7: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=0, Wait=1.
    E7 = 6,
    /// E8: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`>1, N(R)≠NN(R), Wait=0.
    E8 = 7,
    /// E9: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`>1, N(R)≠NN(R), Wait=1.
    E9 = 8,
    /// E10: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`>1, N(R)=NN(R), Count<Limit, Wait=0.
    E10 = 9,
    /// E11: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`>1, N(R)=NN(R), Count<Limit, Wait=1.
    E11 = 10,
    /// E12: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`>1, N(R)=NN(R), Count≥Limit, Wait=0.
    E12 = 11,
    /// E13: Invalid N(R) (N(R)<NN(R) or N(R)>V(S) mod 256).
    E13 = 12,
    /// E14: Lockout Flag = 1.
    E14 = 13,
    /// E15: CLCW bit pattern invalid under COP-1.
    E15 = 14,
    /// E16: Timer expires, Count<Limit, `Timeout_Type`=0.
    E16 = 15,
    /// E17: Timer expires, Count≥Limit, `Timeout_Type`=0.
    E17 = 16,
    /// E18: Timer expires, Count≥Limit, `Timeout_Type`=1.
    E18 = 17,
    /// E19: Request AD, `Wait_Queue` empty.
    E19 = 18,
    /// E20: Request AD, `Wait_Queue` not empty.
    E20 = 19,
    /// E21: Request BD, `BD_Out_Flag` = Ready.
    E21 = 20,
    /// E22: Request BD, `BD_Out_Flag` = `Not_Ready`.
    E22 = 21,
    /// E23: Initiate AD (without CLCW check) directive.
    E23 = 22,
    /// E24: Initiate AD (with CLCW check) directive.
    E24 = 23,
    /// E25: Initiate AD (with Unlock) directive, `BC_Ready`.
    E25 = 24,
    /// E26: Initiate AD (with Unlock) directive, `BC_Not_Ready`.
    E26 = 25,
    /// E27: Initiate AD (with Set V(R)) directive, `BC_Ready`.
    E27 = 26,
    /// E28: Initiate AD (with Set V(R)) directive, `BC_Not_Ready`.
    E28 = 27,
    /// E29: Terminate AD Service directive.
    E29 = 28,
    /// E30: Resume AD Service directive, SS = 0.
    E30 = 29,
    /// E31: Resume AD Service directive, SS = 1.
    E31 = 30,
    /// E32: Resume AD Service directive, SS = 2.
    E32 = 31,
    /// E33: Resume AD Service directive, SS = 3.
    E33 = 32,
    /// E34: Resume AD Service directive, SS = 4.
    E34 = 33,
    /// E35: Set V(S) to V*(S) directive.
    E35 = 34,
    /// E36: Set Sliding Window Width K directive.
    E36 = 35,
    /// E37: Set `T1_Initial` directive.
    E37 = 36,
    /// E38: Set `Transmission_Limit` directive.
    E38 = 37,
    /// E39: Set `Timeout_Type` directive.
    E39 = 38,
    /// E40: Invalid directive.
    E40 = 39,
    /// E41: `AD_Accept` response from lower layer.
    E41 = 40,
    /// E42: `AD_Reject` response from lower layer.
    E42 = 41,
    /// E43: `BC_Accept` response from lower layer.
    E43 = 42,
    /// E44: `BC_Reject` response from lower layer.
    E44 = 43,
    /// E45: `BD_Accept` response from lower layer.
    E45 = 44,
    /// E46: `BD_Reject` response from lower layer.
    E46 = 45,
    /// E101: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`=1, N(R)≠NN(R).
    E101 = 46,
    /// E102: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`=1, N(R)=NN(R).
    E102 = 47,
    /// E103: Lockout=0, NN(R)≤N(R)<V(S), Retransmit=1, `Transmission_Limit`>1, N(R)=NN(R), Count≥Limit, Wait=1.
    E103 = 48,
    /// E104: Timer expires, Count<Limit, `Timeout_Type`=1.
    E104 = 49,
}

/// Target frame kind for lower-procedure ready flags (CCSDS 232.1-B-2 Table 5-1, Events E41, E43, E45).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameKind {
    /// Type-AD Sequence-Controlled frame (Event E41).
    Ad,
    /// Type-BC Protocol Control frame (Event E43).
    Bc,
    /// Type-BD Expedited frame (Event E45).
    Bd,
}

/// Type-BC frame command payload (CCSDS 232.1-B-2 §5.2.5, Table 5-1, Events E25, E27).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BcCommand {
    /// Unlock command (CCSDS 232.0-B-4 §4.1.3.3.2, Event E25).
    Unlock,
    /// Set V(R) command (CCSDS 232.0-B-4 §4.1.3.3.3, Event E27).
    SetVr,
}

/// Primitive action tokens for FOP-1 state transitions (CCSDS 232.1-B-2 §5.2, Table 5-1, COP-001, COP-003).
///
/// Action tokens represent the discrete side effects executed by FOP-1 upon
/// handling an event as specified in the cell review (`docs/verification/reviews/cop1-tables.md` §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fop1Action {
    /// Cancel the T1 retransmission timer (CCSDS 232.1-B-2 §5.2.15, Table 5-1).
    CancelTimer,
    /// Start the T1 retransmission timer (CCSDS 232.1-B-2 §5.2.4, §5.2.5, Table 5-1).
    StartTimer,
    /// Release the master copy of the Type-BC frame on `Sent_Queue` (Table 5-1, Event E1 in S5).
    ReleaseBcCopy,
    /// Remove acknowledged Type-AD frames from `Sent_Queue` (CCSDS 232.1-B-2 §5.2.8).
    RemoveAckedFrames,
    /// Look for FDU to transmit if window and queue allow (CCSDS 232.1-B-2 §5.2.10).
    LookForFdu,
    /// Look for directive waiting for BC transmission (CCSDS 232.1-B-2 §5.2.9).
    LookForDirective,
    /// Initiate Type-AD retransmission on `Sent_Queue` (CCSDS 232.1-B-2 §5.2.7).
    InitiateAdRetransmission,
    /// Initiate Type-BC retransmission on `Sent_Queue` (CCSDS 232.1-B-2 §5.2.7).
    InitiateBcRetransmission,
    /// Add incoming Type-AD transfer request to `Wait_Queue` (Table 5-1, Event E19).
    AddToWaitQueue,
    /// Transmit Type-BD user data frame (CCSDS 232.1-B-2 §5.2.6, Table 5-1, Event E21).
    TransmitBd,
    /// Transmit Type-BC frame carrying Unlock or Set V(R) (CCSDS 232.1-B-2 §5.2.5, Table 5-1, Events E25, E27).
    TransmitBc(BcCommand),
    /// Initialize FOP-1 state variables and queues (CCSDS 232.1-B-2 §5.2.14).
    Initialize,
    /// Set V(S) and NN(R) to V*(R) from Set V(R) directive (Table 5-1, Event E27).
    SetVsFromVr,
    /// Set V(S) and NN(R) to V*(S) from directive if SS = 0, else reject (Table 5-1, Event E35).
    SetVs,
    /// Set sliding window width K from directive (Table 5-1, Event E36).
    SetSlidingWindowWidth,
    /// Set initial T1 timer duration from directive (Table 5-1, Event E37).
    SetT1Initial,
    /// Set transmission limit from directive (Table 5-1, Event E38).
    SetTransmissionLimit,
    /// Set timeout type TT from directive (Table 5-1, Event E39).
    SetTimeoutType,
    /// Record protocol state in `Suspend_State` ($SS := 1..4$) (CCSDS 232.1-B-2 §5.1.11, Table 5-1, Events E18, E104).
    SetSuspendState(u8),
    /// Suspend AD service and emit Suspend notification (CCSDS 232.1-B-2 §5.2.16).
    Suspend,
    /// Resume AD service from suspended state (CCSDS 232.1-B-2 §5.2.17).
    Resume,
    /// Set lower-procedure output flag to Ready (Table 5-1, Events E41, E43, E45).
    SetOutFlagReady(FrameKind),
    /// Generate an Alert notification and transition to S6 (CCSDS 232.1-B-2 §5.2.15, Table 4-4, Table 5-1).
    Alert(AlertReason),
    /// Transfer service notification (CCSDS 232.1-B-2 §4.4, Table 4-5).
    TransferNotify(TransferNotification),
    /// Directive service notification (CCSDS 232.1-B-2 §4.2, Table 4-2).
    DirectiveNotify(DirectiveNotification),
}

/// A cell in the FOP-1 state transition table (CCSDS 232.1-B-2 Table 5-1, COP-001, COP-003, COP-005, RS-021).
///
/// Each cell specifies the slice of primitive actions to execute and the next
/// protocol state, or [`None`] if the transition is Not Applicable (COP-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fop1Cell {
    /// Primitive actions to execute upon event handling.
    pub actions: &'static [Fop1Action],
    /// Next protocol state, or `None` if Not Applicable (COP-005).
    pub next_state: Option<Fop1State>,
}

/// FOP-1 State Table 5-1 represented as `const` data (CCSDS 232.1-B-2 Table 5-1, COP-001, COP-003, RS-021).
///
/// Indexed as `FOP1_TABLE[state as usize][event as usize]`.
pub const FOP1_TABLE: [[Fop1Cell; EVENT_COUNT]; STATE_COUNT] = [
    // State: Active (Open / normal link operation (S1))
    [
        // E1: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Active),
        },
        // E2: Remove acked, Cancel Timer, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::CancelTimer,
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E3: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E4: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E5: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Active),
        },
        // E6: Remove acked, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        },
        // E7: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E8: Remove acked, Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::InitiateAdRetransmission,
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E9: Remove acked
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E10: Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E11: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E12: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E13: Alert[NN(R)]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        },
        // E14: Alert[Lockout]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        },
        // E15: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E16: Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        },
        // E17: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E18: SS:=1, Suspend
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(1), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        },
        // E19: Add to Wait_Queue, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::AddToWaitQueue, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        },
        // E20: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E21: Accept, Transmit BD frame
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E22: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E23: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E24: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E25: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E26: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E27: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E28: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E29: Accept, Alert[term], Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E30: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E31: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E32: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E33: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E34: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E35: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E36: Accept, Set K, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E37: Accept, Set T1_Initial, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E38: Accept, Set Transmission_Limit, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E39: Accept, Set TT, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E40: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        },
        // E41: AD_Out_Flag = Ready, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Ad),
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E42: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E43: BC_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::Active),
        },
        // E44: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E45: BD_Out_Flag = Ready, Accept
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E46: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E101: Remove acked, Alert[Limit]
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::Alert(AlertReason::Limit),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E102: Alert[Limit]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Limit)],
            next_state: Some(Fop1State::Initial),
        },
        // E103: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E104: Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        },
    ],
    // State: RetransmitWithoutWait (Retransmit without wait (S2))
    [
        // E1: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E2: Remove acked, Cancel Timer, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::CancelTimer,
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E3: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E4: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E5: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E6: Remove acked, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        },
        // E7: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E8: Remove acked, Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::InitiateAdRetransmission,
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E9: Remove acked
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E10: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E11: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E12: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E13: Alert[NN(R)]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        },
        // E14: Alert[Lockout]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        },
        // E15: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E16: Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E17: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E18: SS:=2, Suspend
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(2), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        },
        // E19: Add to Wait_Queue, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::AddToWaitQueue, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E20: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E21: Accept, Transmit BD frame
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd,
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E22: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E23: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E24: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E25: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E26: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E27: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E28: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E29: Accept, Alert[term], Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E30: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E31: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E32: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E33: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E34: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E35: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E36: Accept, Set K, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E37: Accept, Set T1_Initial, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E38: Accept, Set Transmission_Limit, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E39: Accept, Set TT, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E40: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E41: AD_Out_Flag = Ready, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Ad),
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E42: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E43: BC_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E44: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E45: BD_Out_Flag = Ready, Accept
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept),
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E46: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E101: Remove acked, Alert[Limit]
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::Alert(AlertReason::Limit),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E102: Alert[Limit]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Limit)],
            next_state: Some(Fop1State::Initial),
        },
        // E103: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E104: Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
    ],
    // State: RetransmitWithWait (Retransmit with wait (S3))
    [
        // E1: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E2: Remove acked, Cancel Timer, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::CancelTimer,
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E3: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E4: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E5: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E6: Remove acked, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        },
        // E7: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E8: Remove acked, Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::InitiateAdRetransmission,
                Fop1Action::LookForFdu,
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E9: Remove acked
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E10: Initiate AD Retransmission, Look for FDU
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E11: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E12: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E13: Alert[NN(R)]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        },
        // E14: Alert[Lockout]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        },
        // E15: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E16: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E17: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E18: SS:=3, Suspend
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(3), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        },
        // E19: Add to Wait_Queue
        Fop1Cell {
            actions: &[Fop1Action::AddToWaitQueue],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E20: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E21: Accept, Transmit BD frame
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd,
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E22: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E23: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E24: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E25: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E26: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E27: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E28: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E29: Accept, Alert[term], Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E30: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E31: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E32: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E33: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E34: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E35: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E36: Accept, Set K, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E37: Accept, Set T1_Initial, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E38: Accept, Set Transmission_Limit, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E39: Accept, Set TT, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E40: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E41: AD_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E42: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E43: BC_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E44: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E45: BD_Out_Flag = Ready, Accept
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept),
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E46: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E101: Remove acked, Alert[Limit]
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::Alert(AlertReason::Limit),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E102: Alert[Limit]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Limit)],
            next_state: Some(Fop1State::Initial),
        },
        // E103: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E104: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
    ],
    // State: InitializingWithoutBcFrame (Initializing without BC frame (S4))
    [
        // E1: Confirm, Cancel Timer
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
                Fop1Action::CancelTimer,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E2: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E3: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E4: Alert[synch]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        },
        // E5: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E6: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E7: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E8: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E9: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E10: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E11: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E12: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E13: Alert[NN(R)]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        },
        // E14: Alert[Lockout]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        },
        // E15: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E16: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E17: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E18: SS:=4, Suspend
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(4), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        },
        // E19: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E20: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E21: Accept, Transmit BD frame
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd,
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E22: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E23: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E24: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E25: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E26: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E27: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E28: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E29: Accept, Alert[term], Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E30: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E31: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E32: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E33: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E34: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E35: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E36: Accept, Set K, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E37: Accept, Set T1_Initial, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E38: Accept, Set Transmission_Limit, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E39: Accept, Set TT, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E40: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E41: AD_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E42: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E43: BC_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E44: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E45: BD_Out_Flag = Ready, Accept
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept),
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E46: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E101: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E102: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E103: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E104: SS:=4, Suspend
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(4), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        },
    ],
    // State: InitializingWithBcFrame (Initializing with BC frame (S5))
    [
        // E1: Confirm, Release BC copy, Cancel Timer
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
                Fop1Action::ReleaseBcCopy,
                Fop1Action::CancelTimer,
            ],
            next_state: Some(Fop1State::Active),
        },
        // E2: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E3: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E4: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E5: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E6: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E7: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E8: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E9: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E10: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E11: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E12: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E13: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E14: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E15: Alert[CLCW]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        },
        // E16: Initiate BC Retransmission, Look for Directive
        Fop1Cell {
            actions: &[
                Fop1Action::InitiateBcRetransmission,
                Fop1Action::LookForDirective,
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E17: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E18: Alert[T1]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        },
        // E19: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E20: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E21: Accept, Transmit BD frame
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd,
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E22: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E23: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E24: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E25: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E26: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E27: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E28: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E29: Accept, Alert[term], Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E30: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E31: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E32: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E33: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E34: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E35: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E36: Accept, Set K, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E37: Accept, Set T1_Initial, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E38: Accept, Set Transmission_Limit, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E39: Accept, Set TT, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E40: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E41: AD_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E42: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E43: BC_Out_Flag = Ready, Look for Directive
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bc),
                Fop1Action::LookForDirective,
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E44: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E45: BD_Out_Flag = Ready, Accept
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E46: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E101: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E102: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E103: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E104: Initiate BC Retransmission, Look for Directive
        Fop1Cell {
            actions: &[
                Fop1Action::InitiateBcRetransmission,
                Fop1Action::LookForDirective,
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
    ],
    // State: Initial (Initial / unconfigured / suspended (S6))
    [
        // E1: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E2: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E3: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E4: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E5: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E6: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E7: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E8: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E9: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E10: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E11: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E12: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E13: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E14: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E15: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E16: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E17: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E18: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
        // E19: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E20: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E21: Accept, Transmit BD frame
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd,
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E22: Reject
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E23: Accept, Initialize, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E24: Accept, Initialize, Start Timer
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::StartTimer,
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E25: Accept, Initialize, Transmit Unlock BC
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::TransmitBc(BcCommand::Unlock),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E26: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E27: Accept, Initialize, V(S):=V*(R), NN(R):=V*(R), Transmit Set V(R) BC
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::SetVsFromVr,
                Fop1Action::TransmitBc(BcCommand::SetVr),
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        },
        // E28: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E29: Accept, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E30: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E31: Accept, Resume, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Active),
        },
        // E32: Accept, Resume, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        },
        // E33: Accept, Resume, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        },
        // E34: Accept, Resume, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        },
        // E35: If SS=0: Accept, V(S):=V*(S), NN(R):=V*(S), Confirm; Else Reject
        Fop1Cell {
            actions: &[Fop1Action::SetVs],
            next_state: Some(Fop1State::Initial),
        },
        // E36: Accept, Set K, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E37: Accept, Set T1_Initial, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E38: Accept, Set Transmission_Limit, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E39: Accept, Set TT, Confirm
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E40: Reject
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        },
        // E41: AD_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::Initial),
        },
        // E42: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E43: BC_Out_Flag = Ready
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::Initial),
        },
        // E44: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E45: BD_Out_Flag = Ready, Accept
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept),
            ],
            next_state: Some(Fop1State::Initial),
        },
        // E46: Alert[LLIF]
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        },
        // E101: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E102: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E103: Ignore
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        },
        // E104: Not applicable
        Fop1Cell {
            actions: &[],
            next_state: None,
        },
    ],
];
