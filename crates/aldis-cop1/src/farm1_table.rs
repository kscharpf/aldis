//! FARM-1 State Table 6-1 represented as `const` data (COP-002, COP-003, RS-021).
//!
//! Defined in CCSDS 232.1-B-2 Section 6:
//! - Section 6.2 & Section 6.3.3: Protocol States S1 (`Open`), S2 (`Wait`), S3 (`Lockout`).
//! - Section 6.3.2: Events E1 through E11.
//! - Section 6.3.3, Table 6-1: State Table.
//! - Review record in `docs/verification/reviews/cop1-tables.md` (COP-003, VER-041).

/// The number of FARM-1 protocol states (S1..S3) defined in CCSDS 232.1-B-2 Table 6-1 (COP-002).
pub const STATE_COUNT: usize = 3;

/// The number of FARM-1 events (E1..E11) defined in CCSDS 232.1-B-2 Table 6-1 (COP-002).
pub const EVENT_COUNT: usize = 11;

/// FARM-1 Protocol States (CCSDS 232.1-B-2 §6.2, §6.3.3, Table 6-1, COP-002, RS-021).
///
/// FARM-1 operates in three closed protocol states:
/// - S1 ([`Farm1State::Open`]): normal frame acceptance.
/// - S2 ([`Farm1State::Wait`]): buffer full condition; frame reception suspended.
/// - S3 ([`Farm1State::Lockout`]): sequence error condition; unlocked only by Unlock command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum Farm1State {
    /// State S1: Open (normal acceptance of Type-AD and Type-BD frames).
    Open = 0,
    /// State S2: Wait (buffer full; Type-AD frames discarded until buffer release).
    Wait = 1,
    /// State S3: Lockout (sequence error; frames discarded until Unlock BC frame).
    Lockout = 2,
}

/// FARM-1 Events (CCSDS 232.1-B-2 §6.3.2, Table 6-1, COP-002, RS-021).
///
/// Events E1 through E11 classify incoming frames (AD, BD, BC, invalid),
/// buffer release signals, and CLCW report requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum Farm1Event {
    /// E1: Valid Type-AD frame arrives, N(S) = V(R), buffer available.
    E1 = 0,
    /// E2: Valid Type-AD frame arrives, N(S) = V(R), buffer unavailable.
    E2 = 1,
    /// E3: Valid Type-AD frame arrives, N(S) ≠ V(R) in positive window.
    E3 = 2,
    /// E4: Valid Type-AD frame arrives, in negative window.
    E4 = 3,
    /// E5: Valid Type-AD frame arrives, outside sliding window.
    E5 = 4,
    /// E6: Valid Type-BD frame arrives.
    E6 = 5,
    /// E7: Valid Unlock Type-BC frame arrives.
    E7 = 6,
    /// E8: Valid Set V(R) Type-BC frame arrives.
    E8 = 7,
    /// E9: Invalid frame arrives (or invalid BC command).
    E9 = 8,
    /// E10: Buffer release signal.
    E10 = 9,
    /// E11: CLCW report time.
    E11 = 10,
}

/// Primitive action tokens for FARM-1 state transitions (CCSDS 232.1-B-2 §6.3.3, Table 6-1, COP-002, COP-003).
///
/// Action tokens represent the discrete side effects executed by FARM-1 upon
/// handling an event as specified in the cell review (`docs/verification/reviews/cop1-tables.md` §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Farm1Action {
    /// Discard the incoming frame without delivery.
    Discard,
    /// Deliver the accepted FDU to the higher procedure or delivery queue.
    DeliverFdu,
    /// Increment V(R) modulo 256.
    IncrementVr,
    /// Set V(R) to the sequence number V*(R) from the Set V(R) command.
    SetVr,
    /// Set the CLCW Retransmit Flag to the given boolean value.
    SetRetransmitFlag(bool),
    /// Set the CLCW Wait Flag to the given boolean value.
    SetWaitFlag(bool),
    /// Set the CLCW Lockout Flag to the given boolean value.
    SetLockoutFlag(bool),
    /// Increment the FARM-B Counter modulo 4.
    IncrementFarmBCounter,
    /// Generate a CLCW report reflecting current V(R), flags, and FARM-B counter.
    GenerateClcw,
}

/// A cell in the FARM-1 state transition table (CCSDS 232.1-B-2 Table 6-1, COP-002, COP-003, COP-005, RS-021).
///
/// Each cell specifies the slice of primitive actions to execute and the next
/// protocol state, or [`None`] if the transition is Not Applicable (COP-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Farm1Cell {
    /// Primitive actions to execute upon event handling.
    pub actions: &'static [Farm1Action],
    /// Next protocol state, or `None` if Not Applicable (COP-005).
    pub next_state: Option<Farm1State>,
}

/// FARM-1 State Table 6-1 represented as `const` data (CCSDS 232.1-B-2 Table 6-1, COP-002, COP-003, RS-021).
///
/// Indexed as `FARM1_TABLE[state as usize][event as usize]`.
pub const FARM1_TABLE: [[Farm1Cell; EVENT_COUNT]; STATE_COUNT] = [
    // State S1: Open
    [
        // E1: Valid Type-AD, N(S) = V(R), buffer available -> Accept frame, V(R) := V(R) + 1, Retransmit_Flag := 0 -> S1
        Farm1Cell {
            actions: &[
                Farm1Action::DeliverFdu,
                Farm1Action::IncrementVr,
                Farm1Action::SetRetransmitFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        },
        // E2: Valid Type-AD, N(S) = V(R), buffer unavailable -> Discard, Retransmit_Flag := 1, Wait_Flag := 1 -> S2
        Farm1Cell {
            actions: &[
                Farm1Action::Discard,
                Farm1Action::SetRetransmitFlag(true),
                Farm1Action::SetWaitFlag(true),
            ],
            next_state: Some(Farm1State::Wait),
        },
        // E3: Valid Type-AD, N(S) != V(R) in pos window -> Discard, Retransmit_Flag := 1 -> S1
        Farm1Cell {
            actions: &[Farm1Action::Discard, Farm1Action::SetRetransmitFlag(true)],
            next_state: Some(Farm1State::Open),
        },
        // E4: Valid Type-AD, in neg window -> Discard -> S1
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Open),
        },
        // E5: Valid Type-AD, outside sliding window -> Discard, Lockout_Flag := 1 -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard, Farm1Action::SetLockoutFlag(true)],
            next_state: Some(Farm1State::Lockout),
        },
        // E6: Valid Type-BD frame arrives -> Accept, increment FARM-B_Counter -> S1
        Farm1Cell {
            actions: &[Farm1Action::DeliverFdu, Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Open),
        },
        // E7: Valid Unlock Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0 -> S1
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        },
        // E8: Valid Set V(R) Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, V(R) := V*(R) -> S1
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetVr,
            ],
            next_state: Some(Farm1State::Open),
        },
        // E9: Invalid frame arrives -> Discard -> S1
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Open),
        },
        // E10: Buffer release signal -> Ignore -> S1
        Farm1Cell {
            actions: &[],
            next_state: Some(Farm1State::Open),
        },
        // E11: CLCW report time -> Generate CLCW -> S1
        Farm1Cell {
            actions: &[Farm1Action::GenerateClcw],
            next_state: Some(Farm1State::Open),
        },
    ],
    // State S2: Wait
    [
        // E1: Not applicable (COP-005: invalid / discard / no transition)
        Farm1Cell {
            actions: &[],
            next_state: None,
        },
        // E2: Valid Type-AD, N(S) = V(R), buffer unavailable -> Discard -> S2
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        },
        // E3: Valid Type-AD, N(S) != V(R) in pos window -> Discard -> S2
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        },
        // E4: Valid Type-AD, in neg window -> Discard -> S2
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        },
        // E5: Valid Type-AD, outside sliding window -> Discard, Lockout_Flag := 1 -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard, Farm1Action::SetLockoutFlag(true)],
            next_state: Some(Farm1State::Lockout),
        },
        // E6: Valid Type-BD frame arrives -> Accept, increment FARM-B_Counter -> S2
        Farm1Cell {
            actions: &[Farm1Action::DeliverFdu, Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Wait),
        },
        // E7: Valid Unlock Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, Wait_Flag := 0 -> S1
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetWaitFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        },
        // E8: Valid Set V(R) Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, Wait_Flag := 0, V(R) := V*(R) -> S1
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetWaitFlag(false),
                Farm1Action::SetVr,
            ],
            next_state: Some(Farm1State::Open),
        },
        // E9: Invalid frame arrives -> Discard -> S2
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        },
        // E10: Buffer release signal -> Wait_Flag := 0 -> S1
        Farm1Cell {
            actions: &[Farm1Action::SetWaitFlag(false)],
            next_state: Some(Farm1State::Open),
        },
        // E11: CLCW report time -> Generate CLCW -> S2
        Farm1Cell {
            actions: &[Farm1Action::GenerateClcw],
            next_state: Some(Farm1State::Wait),
        },
    ],
    // State S3: Lockout
    [
        // E1: Valid Type-AD, N(S) = V(R), buffer available -> Discard -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        },
        // E2: Valid Type-AD, N(S) = V(R), buffer unavailable -> Discard -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        },
        // E3: Valid Type-AD, N(S) != V(R) in pos window -> Discard -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        },
        // E4: Valid Type-AD, in neg window -> Discard -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        },
        // E5: Valid Type-AD, outside sliding window -> Discard -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        },
        // E6: Valid Type-BD frame arrives -> Accept, increment FARM-B_Counter -> S3
        Farm1Cell {
            actions: &[Farm1Action::DeliverFdu, Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Lockout),
        },
        // E7: Valid Unlock Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, Wait_Flag := 0, Lockout_Flag := 0 -> S1
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetWaitFlag(false),
                Farm1Action::SetLockoutFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        },
        // E8: Valid Set V(R) Type-BC frame arrives -> Increment FARM-B_Counter -> S3
        Farm1Cell {
            actions: &[Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Lockout),
        },
        // E9: Invalid frame arrives -> Discard -> S3
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        },
        // E10: Buffer release signal -> Wait_Flag := 0 -> S3
        Farm1Cell {
            actions: &[Farm1Action::SetWaitFlag(false)],
            next_state: Some(Farm1State::Lockout),
        },
        // E11: CLCW report time -> Generate CLCW -> S3
        Farm1Cell {
            actions: &[Farm1Action::GenerateClcw],
            next_state: Some(Farm1State::Lockout),
        },
    ],
];
