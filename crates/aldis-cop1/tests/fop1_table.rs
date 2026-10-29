//! Exhaustive unit tests for FOP-1 State Table 5-1.
//!
//! Verifies all 300 cells (S1–S6 × E1–E46/E101–E104) of Table 5-1 against CCSDS 232.1-B-2
//! and `docs/verification/reviews/cop1-tables.md` §3 (COP-001, COP-003, COP-004, RS-021).

use aldis_cop1::{
    AlertReason, BcCommand, DirectiveNotification, FOP1_EVENT_COUNT, FOP1_STATE_COUNT, FOP1_TABLE,
    Fop1Action, Fop1Cell, Fop1Event, Fop1State, FrameKind, TransferNotification,
};

#[expect(
    clippy::as_conversions,
    reason = "Fop1State and Fop1Event are repr(usize) with contiguous zero-based discriminants"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "state and event discriminants are bounded within [0..6] and [0..50]"
)]
const fn cell(state: Fop1State, event: Fop1Event) -> Fop1Cell {
    FOP1_TABLE[state as usize][event as usize]
}

const fn event_number(e: Fop1Event) -> u8 {
    match e {
        Fop1Event::E1 => 1,
        Fop1Event::E2 => 2,
        Fop1Event::E3 => 3,
        Fop1Event::E4 => 4,
        Fop1Event::E5 => 5,
        Fop1Event::E6 => 6,
        Fop1Event::E7 => 7,
        Fop1Event::E8 => 8,
        Fop1Event::E9 => 9,
        Fop1Event::E10 => 10,
        Fop1Event::E11 => 11,
        Fop1Event::E12 => 12,
        Fop1Event::E13 => 13,
        Fop1Event::E14 => 14,
        Fop1Event::E15 => 15,
        Fop1Event::E16 => 16,
        Fop1Event::E17 => 17,
        Fop1Event::E18 => 18,
        Fop1Event::E19 => 19,
        Fop1Event::E20 => 20,
        Fop1Event::E21 => 21,
        Fop1Event::E22 => 22,
        Fop1Event::E23 => 23,
        Fop1Event::E24 => 24,
        Fop1Event::E25 => 25,
        Fop1Event::E26 => 26,
        Fop1Event::E27 => 27,
        Fop1Event::E28 => 28,
        Fop1Event::E29 => 29,
        Fop1Event::E30 => 30,
        Fop1Event::E31 => 31,
        Fop1Event::E32 => 32,
        Fop1Event::E33 => 33,
        Fop1Event::E34 => 34,
        Fop1Event::E35 => 35,
        Fop1Event::E36 => 36,
        Fop1Event::E37 => 37,
        Fop1Event::E38 => 38,
        Fop1Event::E39 => 39,
        Fop1Event::E40 => 40,
        Fop1Event::E41 => 41,
        Fop1Event::E42 => 42,
        Fop1Event::E43 => 43,
        Fop1Event::E44 => 44,
        Fop1Event::E45 => 45,
        Fop1Event::E46 => 46,
        Fop1Event::E101 => 101,
        Fop1Event::E102 => 102,
        Fop1Event::E103 => 103,
        Fop1Event::E104 => 104,
    }
}

const fn from_event_number(n: u8) -> Option<Fop1Event> {
    match n {
        1 => Some(Fop1Event::E1),
        2 => Some(Fop1Event::E2),
        3 => Some(Fop1Event::E3),
        4 => Some(Fop1Event::E4),
        5 => Some(Fop1Event::E5),
        6 => Some(Fop1Event::E6),
        7 => Some(Fop1Event::E7),
        8 => Some(Fop1Event::E8),
        9 => Some(Fop1Event::E9),
        10 => Some(Fop1Event::E10),
        11 => Some(Fop1Event::E11),
        12 => Some(Fop1Event::E12),
        13 => Some(Fop1Event::E13),
        14 => Some(Fop1Event::E14),
        15 => Some(Fop1Event::E15),
        16 => Some(Fop1Event::E16),
        17 => Some(Fop1Event::E17),
        18 => Some(Fop1Event::E18),
        19 => Some(Fop1Event::E19),
        20 => Some(Fop1Event::E20),
        21 => Some(Fop1Event::E21),
        22 => Some(Fop1Event::E22),
        23 => Some(Fop1Event::E23),
        24 => Some(Fop1Event::E24),
        25 => Some(Fop1Event::E25),
        26 => Some(Fop1Event::E26),
        27 => Some(Fop1Event::E27),
        28 => Some(Fop1Event::E28),
        29 => Some(Fop1Event::E29),
        30 => Some(Fop1Event::E30),
        31 => Some(Fop1Event::E31),
        32 => Some(Fop1Event::E32),
        33 => Some(Fop1Event::E33),
        34 => Some(Fop1Event::E34),
        35 => Some(Fop1Event::E35),
        36 => Some(Fop1Event::E36),
        37 => Some(Fop1Event::E37),
        38 => Some(Fop1Event::E38),
        39 => Some(Fop1Event::E39),
        40 => Some(Fop1Event::E40),
        41 => Some(Fop1Event::E41),
        42 => Some(Fop1Event::E42),
        43 => Some(Fop1Event::E43),
        44 => Some(Fop1Event::E44),
        45 => Some(Fop1Event::E45),
        46 => Some(Fop1Event::E46),
        101 => Some(Fop1Event::E101),
        102 => Some(Fop1Event::E102),
        103 => Some(Fop1Event::E103),
        104 => Some(Fop1Event::E104),
        _ => None,
    }
}

#[test]
fn test_fop1_table_dimensions() {
    // req: COP-001, COP-003, RS-021
    assert_eq!(FOP1_STATE_COUNT, 6);
    assert_eq!(FOP1_EVENT_COUNT, 50);
    assert_eq!(FOP1_TABLE.len(), 6);
    for row in &FOP1_TABLE {
        assert_eq!(row.len(), 50);
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "verifies all 50 events bidirectionally"
)]
fn test_fop1_event_numbers() {
    // req: COP-004

    assert_eq!(event_number(Fop1Event::E1), 1);
    assert_eq!(from_event_number(1), Some(Fop1Event::E1));
    assert_eq!(event_number(Fop1Event::E2), 2);
    assert_eq!(from_event_number(2), Some(Fop1Event::E2));
    assert_eq!(event_number(Fop1Event::E3), 3);
    assert_eq!(from_event_number(3), Some(Fop1Event::E3));
    assert_eq!(event_number(Fop1Event::E4), 4);
    assert_eq!(from_event_number(4), Some(Fop1Event::E4));
    assert_eq!(event_number(Fop1Event::E5), 5);
    assert_eq!(from_event_number(5), Some(Fop1Event::E5));
    assert_eq!(event_number(Fop1Event::E6), 6);
    assert_eq!(from_event_number(6), Some(Fop1Event::E6));
    assert_eq!(event_number(Fop1Event::E7), 7);
    assert_eq!(from_event_number(7), Some(Fop1Event::E7));
    assert_eq!(event_number(Fop1Event::E8), 8);
    assert_eq!(from_event_number(8), Some(Fop1Event::E8));
    assert_eq!(event_number(Fop1Event::E9), 9);
    assert_eq!(from_event_number(9), Some(Fop1Event::E9));
    assert_eq!(event_number(Fop1Event::E10), 10);
    assert_eq!(from_event_number(10), Some(Fop1Event::E10));
    assert_eq!(event_number(Fop1Event::E11), 11);
    assert_eq!(from_event_number(11), Some(Fop1Event::E11));
    assert_eq!(event_number(Fop1Event::E12), 12);
    assert_eq!(from_event_number(12), Some(Fop1Event::E12));
    assert_eq!(event_number(Fop1Event::E13), 13);
    assert_eq!(from_event_number(13), Some(Fop1Event::E13));
    assert_eq!(event_number(Fop1Event::E14), 14);
    assert_eq!(from_event_number(14), Some(Fop1Event::E14));
    assert_eq!(event_number(Fop1Event::E15), 15);
    assert_eq!(from_event_number(15), Some(Fop1Event::E15));
    assert_eq!(event_number(Fop1Event::E16), 16);
    assert_eq!(from_event_number(16), Some(Fop1Event::E16));
    assert_eq!(event_number(Fop1Event::E17), 17);
    assert_eq!(from_event_number(17), Some(Fop1Event::E17));
    assert_eq!(event_number(Fop1Event::E18), 18);
    assert_eq!(from_event_number(18), Some(Fop1Event::E18));
    assert_eq!(event_number(Fop1Event::E19), 19);
    assert_eq!(from_event_number(19), Some(Fop1Event::E19));
    assert_eq!(event_number(Fop1Event::E20), 20);
    assert_eq!(from_event_number(20), Some(Fop1Event::E20));
    assert_eq!(event_number(Fop1Event::E21), 21);
    assert_eq!(from_event_number(21), Some(Fop1Event::E21));
    assert_eq!(event_number(Fop1Event::E22), 22);
    assert_eq!(from_event_number(22), Some(Fop1Event::E22));
    assert_eq!(event_number(Fop1Event::E23), 23);
    assert_eq!(from_event_number(23), Some(Fop1Event::E23));
    assert_eq!(event_number(Fop1Event::E24), 24);
    assert_eq!(from_event_number(24), Some(Fop1Event::E24));
    assert_eq!(event_number(Fop1Event::E25), 25);
    assert_eq!(from_event_number(25), Some(Fop1Event::E25));
    assert_eq!(event_number(Fop1Event::E26), 26);
    assert_eq!(from_event_number(26), Some(Fop1Event::E26));
    assert_eq!(event_number(Fop1Event::E27), 27);
    assert_eq!(from_event_number(27), Some(Fop1Event::E27));
    assert_eq!(event_number(Fop1Event::E28), 28);
    assert_eq!(from_event_number(28), Some(Fop1Event::E28));
    assert_eq!(event_number(Fop1Event::E29), 29);
    assert_eq!(from_event_number(29), Some(Fop1Event::E29));
    assert_eq!(event_number(Fop1Event::E30), 30);
    assert_eq!(from_event_number(30), Some(Fop1Event::E30));
    assert_eq!(event_number(Fop1Event::E31), 31);
    assert_eq!(from_event_number(31), Some(Fop1Event::E31));
    assert_eq!(event_number(Fop1Event::E32), 32);
    assert_eq!(from_event_number(32), Some(Fop1Event::E32));
    assert_eq!(event_number(Fop1Event::E33), 33);
    assert_eq!(from_event_number(33), Some(Fop1Event::E33));
    assert_eq!(event_number(Fop1Event::E34), 34);
    assert_eq!(from_event_number(34), Some(Fop1Event::E34));
    assert_eq!(event_number(Fop1Event::E35), 35);
    assert_eq!(from_event_number(35), Some(Fop1Event::E35));
    assert_eq!(event_number(Fop1Event::E36), 36);
    assert_eq!(from_event_number(36), Some(Fop1Event::E36));
    assert_eq!(event_number(Fop1Event::E37), 37);
    assert_eq!(from_event_number(37), Some(Fop1Event::E37));
    assert_eq!(event_number(Fop1Event::E38), 38);
    assert_eq!(from_event_number(38), Some(Fop1Event::E38));
    assert_eq!(event_number(Fop1Event::E39), 39);
    assert_eq!(from_event_number(39), Some(Fop1Event::E39));
    assert_eq!(event_number(Fop1Event::E40), 40);
    assert_eq!(from_event_number(40), Some(Fop1Event::E40));
    assert_eq!(event_number(Fop1Event::E41), 41);
    assert_eq!(from_event_number(41), Some(Fop1Event::E41));
    assert_eq!(event_number(Fop1Event::E42), 42);
    assert_eq!(from_event_number(42), Some(Fop1Event::E42));
    assert_eq!(event_number(Fop1Event::E43), 43);
    assert_eq!(from_event_number(43), Some(Fop1Event::E43));
    assert_eq!(event_number(Fop1Event::E44), 44);
    assert_eq!(from_event_number(44), Some(Fop1Event::E44));
    assert_eq!(event_number(Fop1Event::E45), 45);
    assert_eq!(from_event_number(45), Some(Fop1Event::E45));
    assert_eq!(event_number(Fop1Event::E46), 46);
    assert_eq!(from_event_number(46), Some(Fop1Event::E46));
    assert_eq!(event_number(Fop1Event::E101), 101);
    assert_eq!(from_event_number(101), Some(Fop1Event::E101));
    assert_eq!(event_number(Fop1Event::E102), 102);
    assert_eq!(from_event_number(102), Some(Fop1Event::E102));
    assert_eq!(event_number(Fop1Event::E103), 103);
    assert_eq!(from_event_number(103), Some(Fop1Event::E103));
    assert_eq!(event_number(Fop1Event::E104), 104);
    assert_eq!(from_event_number(104), Some(Fop1Event::E104));
    assert_eq!(from_event_number(0), None);
    assert_eq!(from_event_number(47), None);
    assert_eq!(from_event_number(100), None);
    assert_eq!(from_event_number(105), None);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive verification of 50 events in this state"
)]
fn test_fop1_active_cells() {
    // req: COP-001, COP-003, RS-021
    let s = Fop1State::Active;

    // E1: Ignore
    assert_eq!(
        cell(s, Fop1Event::E1),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Active),
        }
    );

    // E2: Remove acked, Cancel Timer, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E2),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::CancelTimer,
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E3: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E3),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E4: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E4),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E5: Ignore
    assert_eq!(
        cell(s, Fop1Event::E5),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Active),
        }
    );

    // E6: Remove acked, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E6),
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        }
    );

    // E7: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E7),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E8: Remove acked, Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E8),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::InitiateAdRetransmission,
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E9: Remove acked
    assert_eq!(
        cell(s, Fop1Event::E9),
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E10: Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E10),
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E11: Ignore
    assert_eq!(
        cell(s, Fop1Event::E11),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E12: Ignore
    assert_eq!(
        cell(s, Fop1Event::E12),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E13: Alert[NN(R)]
    assert_eq!(
        cell(s, Fop1Event::E13),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E14: Alert[Lockout]
    assert_eq!(
        cell(s, Fop1Event::E14),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E15: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E15),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E16: Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E16),
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        }
    );

    // E17: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E17),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E18: SS:=1, Suspend
    assert_eq!(
        cell(s, Fop1Event::E18),
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(1), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E19: Add to Wait_Queue, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E19),
        Fop1Cell {
            actions: &[Fop1Action::AddToWaitQueue, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        }
    );

    // E20: Reject
    assert_eq!(
        cell(s, Fop1Event::E20),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E21: Accept, Transmit BD frame
    assert_eq!(
        cell(s, Fop1Event::E21),
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E22: Reject
    assert_eq!(
        cell(s, Fop1Event::E22),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E23: Reject
    assert_eq!(
        cell(s, Fop1Event::E23),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E24: Reject
    assert_eq!(
        cell(s, Fop1Event::E24),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E25: Reject
    assert_eq!(
        cell(s, Fop1Event::E25),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E26: Reject
    assert_eq!(
        cell(s, Fop1Event::E26),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E27: Reject
    assert_eq!(
        cell(s, Fop1Event::E27),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E28: Reject
    assert_eq!(
        cell(s, Fop1Event::E28),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E29: Accept, Alert[term], Confirm
    assert_eq!(
        cell(s, Fop1Event::E29),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E30: Reject
    assert_eq!(
        cell(s, Fop1Event::E30),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E31: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E31),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E32: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E32),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E33: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E33),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E34: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E34),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E35: Reject
    assert_eq!(
        cell(s, Fop1Event::E35),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E36: Accept, Set K, Confirm
    assert_eq!(
        cell(s, Fop1Event::E36),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E37: Accept, Set T1_Initial, Confirm
    assert_eq!(
        cell(s, Fop1Event::E37),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E38: Accept, Set Transmission_Limit, Confirm
    assert_eq!(
        cell(s, Fop1Event::E38),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E39: Accept, Set TT, Confirm
    assert_eq!(
        cell(s, Fop1Event::E39),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E40: Reject
    assert_eq!(
        cell(s, Fop1Event::E40),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E41: AD_Out_Flag = Ready, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E41),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Ad),
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E42: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E42),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E43: BC_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E43),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::Active),
        }
    );

    // E44: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E44),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E45: BD_Out_Flag = Ready, Accept
    assert_eq!(
        cell(s, Fop1Event::E45),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E46: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E46),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E101: Remove acked, Alert[Limit]
    assert_eq!(
        cell(s, Fop1Event::E101),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::Alert(AlertReason::Limit)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E102: Alert[Limit]
    assert_eq!(
        cell(s, Fop1Event::E102),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Limit)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E103: Ignore
    assert_eq!(
        cell(s, Fop1Event::E103),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E104: Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E104),
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        }
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive verification of 50 events in this state"
)]
fn test_fop1_retransmit_without_wait_cells() {
    // req: COP-001, COP-003, RS-021
    let s = Fop1State::RetransmitWithoutWait;

    // E1: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E1),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E2: Remove acked, Cancel Timer, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E2),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::CancelTimer,
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E3: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E3),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E4: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E4),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E5: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E5),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E6: Remove acked, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E6),
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        }
    );

    // E7: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E7),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E8: Remove acked, Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E8),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::InitiateAdRetransmission,
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E9: Remove acked
    assert_eq!(
        cell(s, Fop1Event::E9),
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E10: Ignore
    assert_eq!(
        cell(s, Fop1Event::E10),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E11: Ignore
    assert_eq!(
        cell(s, Fop1Event::E11),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E12: Ignore
    assert_eq!(
        cell(s, Fop1Event::E12),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E13: Alert[NN(R)]
    assert_eq!(
        cell(s, Fop1Event::E13),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E14: Alert[Lockout]
    assert_eq!(
        cell(s, Fop1Event::E14),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E15: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E15),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E16: Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E16),
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E17: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E17),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E18: SS:=2, Suspend
    assert_eq!(
        cell(s, Fop1Event::E18),
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(2), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E19: Add to Wait_Queue, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E19),
        Fop1Cell {
            actions: &[Fop1Action::AddToWaitQueue, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E20: Reject
    assert_eq!(
        cell(s, Fop1Event::E20),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E21: Accept, Transmit BD frame
    assert_eq!(
        cell(s, Fop1Event::E21),
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E22: Reject
    assert_eq!(
        cell(s, Fop1Event::E22),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E23: Reject
    assert_eq!(
        cell(s, Fop1Event::E23),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E24: Reject
    assert_eq!(
        cell(s, Fop1Event::E24),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E25: Reject
    assert_eq!(
        cell(s, Fop1Event::E25),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E26: Reject
    assert_eq!(
        cell(s, Fop1Event::E26),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E27: Reject
    assert_eq!(
        cell(s, Fop1Event::E27),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E28: Reject
    assert_eq!(
        cell(s, Fop1Event::E28),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E29: Accept, Alert[term], Confirm
    assert_eq!(
        cell(s, Fop1Event::E29),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E30: Reject
    assert_eq!(
        cell(s, Fop1Event::E30),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E31: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E31),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E32: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E32),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E33: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E33),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E34: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E34),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E35: Reject
    assert_eq!(
        cell(s, Fop1Event::E35),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E36: Accept, Set K, Confirm
    assert_eq!(
        cell(s, Fop1Event::E36),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E37: Accept, Set T1_Initial, Confirm
    assert_eq!(
        cell(s, Fop1Event::E37),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E38: Accept, Set Transmission_Limit, Confirm
    assert_eq!(
        cell(s, Fop1Event::E38),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E39: Accept, Set TT, Confirm
    assert_eq!(
        cell(s, Fop1Event::E39),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E40: Reject
    assert_eq!(
        cell(s, Fop1Event::E40),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E41: AD_Out_Flag = Ready, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E41),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Ad),
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E42: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E42),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E43: BC_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E43),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E44: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E44),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E45: BD_Out_Flag = Ready, Accept
    assert_eq!(
        cell(s, Fop1Event::E45),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept)
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E46: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E46),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E101: Remove acked, Alert[Limit]
    assert_eq!(
        cell(s, Fop1Event::E101),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::Alert(AlertReason::Limit)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E102: Alert[Limit]
    assert_eq!(
        cell(s, Fop1Event::E102),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Limit)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E103: Ignore
    assert_eq!(
        cell(s, Fop1Event::E103),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E104: Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E104),
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive verification of 50 events in this state"
)]
fn test_fop1_retransmit_with_wait_cells() {
    // req: COP-001, COP-003, RS-021
    let s = Fop1State::RetransmitWithWait;

    // E1: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E1),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E2: Remove acked, Cancel Timer, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E2),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::CancelTimer,
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E3: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E3),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E4: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E4),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E5: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E5),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E6: Remove acked, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E6),
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::Active),
        }
    );

    // E7: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E7),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E8: Remove acked, Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E8),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::InitiateAdRetransmission,
                Fop1Action::LookForFdu
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E9: Remove acked
    assert_eq!(
        cell(s, Fop1Event::E9),
        Fop1Cell {
            actions: &[Fop1Action::RemoveAckedFrames],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E10: Initiate AD Retransmission, Look for FDU
    assert_eq!(
        cell(s, Fop1Event::E10),
        Fop1Cell {
            actions: &[Fop1Action::InitiateAdRetransmission, Fop1Action::LookForFdu],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E11: Ignore
    assert_eq!(
        cell(s, Fop1Event::E11),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E12: Ignore
    assert_eq!(
        cell(s, Fop1Event::E12),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E13: Alert[NN(R)]
    assert_eq!(
        cell(s, Fop1Event::E13),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E14: Alert[Lockout]
    assert_eq!(
        cell(s, Fop1Event::E14),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E15: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E15),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E16: Ignore
    assert_eq!(
        cell(s, Fop1Event::E16),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E17: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E17),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E18: SS:=3, Suspend
    assert_eq!(
        cell(s, Fop1Event::E18),
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(3), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E19: Add to Wait_Queue
    assert_eq!(
        cell(s, Fop1Event::E19),
        Fop1Cell {
            actions: &[Fop1Action::AddToWaitQueue],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E20: Reject
    assert_eq!(
        cell(s, Fop1Event::E20),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E21: Accept, Transmit BD frame
    assert_eq!(
        cell(s, Fop1Event::E21),
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E22: Reject
    assert_eq!(
        cell(s, Fop1Event::E22),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E23: Reject
    assert_eq!(
        cell(s, Fop1Event::E23),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E24: Reject
    assert_eq!(
        cell(s, Fop1Event::E24),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E25: Reject
    assert_eq!(
        cell(s, Fop1Event::E25),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E26: Reject
    assert_eq!(
        cell(s, Fop1Event::E26),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E27: Reject
    assert_eq!(
        cell(s, Fop1Event::E27),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E28: Reject
    assert_eq!(
        cell(s, Fop1Event::E28),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E29: Accept, Alert[term], Confirm
    assert_eq!(
        cell(s, Fop1Event::E29),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E30: Reject
    assert_eq!(
        cell(s, Fop1Event::E30),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E31: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E31),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E32: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E32),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E33: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E33),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E34: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E34),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E35: Reject
    assert_eq!(
        cell(s, Fop1Event::E35),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E36: Accept, Set K, Confirm
    assert_eq!(
        cell(s, Fop1Event::E36),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E37: Accept, Set T1_Initial, Confirm
    assert_eq!(
        cell(s, Fop1Event::E37),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E38: Accept, Set Transmission_Limit, Confirm
    assert_eq!(
        cell(s, Fop1Event::E38),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E39: Accept, Set TT, Confirm
    assert_eq!(
        cell(s, Fop1Event::E39),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E40: Reject
    assert_eq!(
        cell(s, Fop1Event::E40),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E41: AD_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E41),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E42: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E42),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E43: BC_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E43),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E44: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E44),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E45: BD_Out_Flag = Ready, Accept
    assert_eq!(
        cell(s, Fop1Event::E45),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept)
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E46: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E46),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E101: Remove acked, Alert[Limit]
    assert_eq!(
        cell(s, Fop1Event::E101),
        Fop1Cell {
            actions: &[
                Fop1Action::RemoveAckedFrames,
                Fop1Action::Alert(AlertReason::Limit)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E102: Alert[Limit]
    assert_eq!(
        cell(s, Fop1Event::E102),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Limit)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E103: Ignore
    assert_eq!(
        cell(s, Fop1Event::E103),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E104: Ignore
    assert_eq!(
        cell(s, Fop1Event::E104),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive verification of 50 events in this state"
)]
fn test_fop1_initializing_without_bc_frame_cells() {
    // req: COP-001, COP-003, RS-021
    let s = Fop1State::InitializingWithoutBcFrame;

    // E1: Confirm, Cancel Timer
    assert_eq!(
        cell(s, Fop1Event::E1),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
                Fop1Action::CancelTimer
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E2: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E2),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E3: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E3),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E4: Alert[synch]
    assert_eq!(
        cell(s, Fop1Event::E4),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Synch)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E5: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E5),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E6: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E6),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E7: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E7),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E8: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E8),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E9: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E9),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E10: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E10),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E11: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E11),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E12: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E12),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E13: Alert[NN(R)]
    assert_eq!(
        cell(s, Fop1Event::E13),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::NnR)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E14: Alert[Lockout]
    assert_eq!(
        cell(s, Fop1Event::E14),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Lockout)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E15: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E15),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E16: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E16),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E17: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E17),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E18: SS:=4, Suspend
    assert_eq!(
        cell(s, Fop1Event::E18),
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(4), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E19: Reject
    assert_eq!(
        cell(s, Fop1Event::E19),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E20: Reject
    assert_eq!(
        cell(s, Fop1Event::E20),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E21: Accept, Transmit BD frame
    assert_eq!(
        cell(s, Fop1Event::E21),
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E22: Reject
    assert_eq!(
        cell(s, Fop1Event::E22),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E23: Reject
    assert_eq!(
        cell(s, Fop1Event::E23),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E24: Reject
    assert_eq!(
        cell(s, Fop1Event::E24),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E25: Reject
    assert_eq!(
        cell(s, Fop1Event::E25),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E26: Reject
    assert_eq!(
        cell(s, Fop1Event::E26),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E27: Reject
    assert_eq!(
        cell(s, Fop1Event::E27),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E28: Reject
    assert_eq!(
        cell(s, Fop1Event::E28),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E29: Accept, Alert[term], Confirm
    assert_eq!(
        cell(s, Fop1Event::E29),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E30: Reject
    assert_eq!(
        cell(s, Fop1Event::E30),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E31: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E31),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E32: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E32),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E33: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E33),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E34: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E34),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E35: Reject
    assert_eq!(
        cell(s, Fop1Event::E35),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E36: Accept, Set K, Confirm
    assert_eq!(
        cell(s, Fop1Event::E36),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E37: Accept, Set T1_Initial, Confirm
    assert_eq!(
        cell(s, Fop1Event::E37),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E38: Accept, Set Transmission_Limit, Confirm
    assert_eq!(
        cell(s, Fop1Event::E38),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E39: Accept, Set TT, Confirm
    assert_eq!(
        cell(s, Fop1Event::E39),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E40: Reject
    assert_eq!(
        cell(s, Fop1Event::E40),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E41: AD_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E41),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E42: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E42),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E43: BC_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E43),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E44: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E44),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E45: BD_Out_Flag = Ready, Accept
    assert_eq!(
        cell(s, Fop1Event::E45),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept)
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E46: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E46),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E101: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E101),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E102: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E102),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E103: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E103),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E104: SS:=4, Suspend
    assert_eq!(
        cell(s, Fop1Event::E104),
        Fop1Cell {
            actions: &[Fop1Action::SetSuspendState(4), Fop1Action::Suspend],
            next_state: Some(Fop1State::Initial),
        }
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive verification of 50 events in this state"
)]
fn test_fop1_initializing_with_bc_frame_cells() {
    // req: COP-001, COP-003, RS-021
    let s = Fop1State::InitializingWithBcFrame;

    // E1: Confirm, Release BC copy, Cancel Timer
    assert_eq!(
        cell(s, Fop1Event::E1),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm),
                Fop1Action::ReleaseBcCopy,
                Fop1Action::CancelTimer
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E2: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E2),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E3: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E3),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E4: Ignore
    assert_eq!(
        cell(s, Fop1Event::E4),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E5: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E5),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E6: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E6),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E7: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E7),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E8: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E8),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E9: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E9),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E10: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E10),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E11: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E11),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E12: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E12),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E13: Ignore
    assert_eq!(
        cell(s, Fop1Event::E13),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E14: Ignore
    assert_eq!(
        cell(s, Fop1Event::E14),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E15: Alert[CLCW]
    assert_eq!(
        cell(s, Fop1Event::E15),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Clcw)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E16: Initiate BC Retransmission, Look for Directive
    assert_eq!(
        cell(s, Fop1Event::E16),
        Fop1Cell {
            actions: &[
                Fop1Action::InitiateBcRetransmission,
                Fop1Action::LookForDirective
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E17: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E17),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E18: Alert[T1]
    assert_eq!(
        cell(s, Fop1Event::E18),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::T1)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E19: Reject
    assert_eq!(
        cell(s, Fop1Event::E19),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E20: Reject
    assert_eq!(
        cell(s, Fop1Event::E20),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E21: Accept, Transmit BD frame
    assert_eq!(
        cell(s, Fop1Event::E21),
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E22: Reject
    assert_eq!(
        cell(s, Fop1Event::E22),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E23: Reject
    assert_eq!(
        cell(s, Fop1Event::E23),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E24: Reject
    assert_eq!(
        cell(s, Fop1Event::E24),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E25: Reject
    assert_eq!(
        cell(s, Fop1Event::E25),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E26: Reject
    assert_eq!(
        cell(s, Fop1Event::E26),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E27: Reject
    assert_eq!(
        cell(s, Fop1Event::E27),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E28: Reject
    assert_eq!(
        cell(s, Fop1Event::E28),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E29: Accept, Alert[term], Confirm
    assert_eq!(
        cell(s, Fop1Event::E29),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Alert(AlertReason::Term),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E30: Reject
    assert_eq!(
        cell(s, Fop1Event::E30),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E31: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E31),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E32: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E32),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E33: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E33),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E34: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E34),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E35: Reject
    assert_eq!(
        cell(s, Fop1Event::E35),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E36: Accept, Set K, Confirm
    assert_eq!(
        cell(s, Fop1Event::E36),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E37: Accept, Set T1_Initial, Confirm
    assert_eq!(
        cell(s, Fop1Event::E37),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E38: Accept, Set Transmission_Limit, Confirm
    assert_eq!(
        cell(s, Fop1Event::E38),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E39: Accept, Set TT, Confirm
    assert_eq!(
        cell(s, Fop1Event::E39),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E40: Reject
    assert_eq!(
        cell(s, Fop1Event::E40),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E41: AD_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E41),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E42: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E42),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E43: BC_Out_Flag = Ready, Look for Directive
    assert_eq!(
        cell(s, Fop1Event::E43),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bc),
                Fop1Action::LookForDirective
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E44: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E44),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E45: BD_Out_Flag = Ready, Accept
    assert_eq!(
        cell(s, Fop1Event::E45),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E46: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E46),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E101: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E101),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E102: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E102),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E103: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E103),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E104: Initiate BC Retransmission, Look for Directive
    assert_eq!(
        cell(s, Fop1Event::E104),
        Fop1Cell {
            actions: &[
                Fop1Action::InitiateBcRetransmission,
                Fop1Action::LookForDirective
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive verification of 50 events in this state"
)]
fn test_fop1_initial_cells() {
    // req: COP-001, COP-003, RS-021
    let s = Fop1State::Initial;

    // E1: Ignore
    assert_eq!(
        cell(s, Fop1Event::E1),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E2: Ignore
    assert_eq!(
        cell(s, Fop1Event::E2),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E3: Ignore
    assert_eq!(
        cell(s, Fop1Event::E3),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E4: Ignore
    assert_eq!(
        cell(s, Fop1Event::E4),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E5: Ignore
    assert_eq!(
        cell(s, Fop1Event::E5),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E6: Ignore
    assert_eq!(
        cell(s, Fop1Event::E6),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E7: Ignore
    assert_eq!(
        cell(s, Fop1Event::E7),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E8: Ignore
    assert_eq!(
        cell(s, Fop1Event::E8),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E9: Ignore
    assert_eq!(
        cell(s, Fop1Event::E9),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E10: Ignore
    assert_eq!(
        cell(s, Fop1Event::E10),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E11: Ignore
    assert_eq!(
        cell(s, Fop1Event::E11),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E12: Ignore
    assert_eq!(
        cell(s, Fop1Event::E12),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E13: Ignore
    assert_eq!(
        cell(s, Fop1Event::E13),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E14: Ignore
    assert_eq!(
        cell(s, Fop1Event::E14),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E15: Ignore
    assert_eq!(
        cell(s, Fop1Event::E15),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E16: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E16),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E17: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E17),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E18: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E18),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E19: Reject
    assert_eq!(
        cell(s, Fop1Event::E19),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E20: Reject
    assert_eq!(
        cell(s, Fop1Event::E20),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E21: Accept, Transmit BD frame
    assert_eq!(
        cell(s, Fop1Event::E21),
        Fop1Cell {
            actions: &[
                Fop1Action::TransferNotify(TransferNotification::Accept),
                Fop1Action::TransmitBd
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E22: Reject
    assert_eq!(
        cell(s, Fop1Event::E22),
        Fop1Cell {
            actions: &[Fop1Action::TransferNotify(TransferNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E23: Accept, Initialize, Confirm
    assert_eq!(
        cell(s, Fop1Event::E23),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E24: Accept, Initialize, Start Timer
    assert_eq!(
        cell(s, Fop1Event::E24),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::StartTimer
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E25: Accept, Initialize, Transmit Unlock BC
    assert_eq!(
        cell(s, Fop1Event::E25),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::TransmitBc(BcCommand::Unlock)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E26: Reject
    assert_eq!(
        cell(s, Fop1Event::E26),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E27: Accept, Initialize, V(S):=V*(R), NN(R):=V*(R), Transmit Set V(R) BC
    assert_eq!(
        cell(s, Fop1Event::E27),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Initialize,
                Fop1Action::SetVsFromVr,
                Fop1Action::TransmitBc(BcCommand::SetVr)
            ],
            next_state: Some(Fop1State::InitializingWithBcFrame),
        }
    );

    // E28: Reject
    assert_eq!(
        cell(s, Fop1Event::E28),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E29: Accept, Confirm
    assert_eq!(
        cell(s, Fop1Event::E29),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E30: Reject
    assert_eq!(
        cell(s, Fop1Event::E30),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E31: Accept, Resume, Confirm
    assert_eq!(
        cell(s, Fop1Event::E31),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Active),
        }
    );

    // E32: Accept, Resume, Confirm
    assert_eq!(
        cell(s, Fop1Event::E32),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithoutWait),
        }
    );

    // E33: Accept, Resume, Confirm
    assert_eq!(
        cell(s, Fop1Event::E33),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::RetransmitWithWait),
        }
    );

    // E34: Accept, Resume, Confirm
    assert_eq!(
        cell(s, Fop1Event::E34),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::Resume,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::InitializingWithoutBcFrame),
        }
    );

    // E35: If SS=0: Accept, V(S):=V*(S), NN(R):=V*(S), Confirm; Else Reject
    assert_eq!(
        cell(s, Fop1Event::E35),
        Fop1Cell {
            actions: &[Fop1Action::SetVs],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E36: Accept, Set K, Confirm
    assert_eq!(
        cell(s, Fop1Event::E36),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetSlidingWindowWidth,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E37: Accept, Set T1_Initial, Confirm
    assert_eq!(
        cell(s, Fop1Event::E37),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetT1Initial,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E38: Accept, Set Transmission_Limit, Confirm
    assert_eq!(
        cell(s, Fop1Event::E38),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTransmissionLimit,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E39: Accept, Set TT, Confirm
    assert_eq!(
        cell(s, Fop1Event::E39),
        Fop1Cell {
            actions: &[
                Fop1Action::DirectiveNotify(DirectiveNotification::Accept),
                Fop1Action::SetTimeoutType,
                Fop1Action::DirectiveNotify(DirectiveNotification::PositiveConfirm)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E40: Reject
    assert_eq!(
        cell(s, Fop1Event::E40),
        Fop1Cell {
            actions: &[Fop1Action::DirectiveNotify(DirectiveNotification::Reject)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E41: AD_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E41),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Ad)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E42: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E42),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E43: BC_Out_Flag = Ready
    assert_eq!(
        cell(s, Fop1Event::E43),
        Fop1Cell {
            actions: &[Fop1Action::SetOutFlagReady(FrameKind::Bc)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E44: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E44),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E45: BD_Out_Flag = Ready, Accept
    assert_eq!(
        cell(s, Fop1Event::E45),
        Fop1Cell {
            actions: &[
                Fop1Action::SetOutFlagReady(FrameKind::Bd),
                Fop1Action::TransferNotify(TransferNotification::Accept)
            ],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E46: Alert[LLIF]
    assert_eq!(
        cell(s, Fop1Event::E46),
        Fop1Cell {
            actions: &[Fop1Action::Alert(AlertReason::Llif)],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E101: Ignore
    assert_eq!(
        cell(s, Fop1Event::E101),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E102: Ignore
    assert_eq!(
        cell(s, Fop1Event::E102),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E103: Ignore
    assert_eq!(
        cell(s, Fop1Event::E103),
        Fop1Cell {
            actions: &[],
            next_state: Some(Fop1State::Initial),
        }
    );

    // E104: Not applicable
    assert_eq!(
        cell(s, Fop1Event::E104),
        Fop1Cell {
            actions: &[],
            next_state: None,
        }
    );
}

#[test]
fn test_exhaustive_300_cells() {
    // req: COP-001, COP-003, COP-005, RS-021
    let states = [
        Fop1State::Active,
        Fop1State::RetransmitWithoutWait,
        Fop1State::RetransmitWithWait,
        Fop1State::InitializingWithoutBcFrame,
        Fop1State::InitializingWithBcFrame,
        Fop1State::Initial,
    ];

    let events = [
        Fop1Event::E1,
        Fop1Event::E2,
        Fop1Event::E3,
        Fop1Event::E4,
        Fop1Event::E5,
        Fop1Event::E6,
        Fop1Event::E7,
        Fop1Event::E8,
        Fop1Event::E9,
        Fop1Event::E10,
        Fop1Event::E11,
        Fop1Event::E12,
        Fop1Event::E13,
        Fop1Event::E14,
        Fop1Event::E15,
        Fop1Event::E16,
        Fop1Event::E17,
        Fop1Event::E18,
        Fop1Event::E19,
        Fop1Event::E20,
        Fop1Event::E21,
        Fop1Event::E22,
        Fop1Event::E23,
        Fop1Event::E24,
        Fop1Event::E25,
        Fop1Event::E26,
        Fop1Event::E27,
        Fop1Event::E28,
        Fop1Event::E29,
        Fop1Event::E30,
        Fop1Event::E31,
        Fop1Event::E32,
        Fop1Event::E33,
        Fop1Event::E34,
        Fop1Event::E35,
        Fop1Event::E36,
        Fop1Event::E37,
        Fop1Event::E38,
        Fop1Event::E39,
        Fop1Event::E40,
        Fop1Event::E41,
        Fop1Event::E42,
        Fop1Event::E43,
        Fop1Event::E44,
        Fop1Event::E45,
        Fop1Event::E46,
        Fop1Event::E101,
        Fop1Event::E102,
        Fop1Event::E103,
        Fop1Event::E104,
    ];

    let mut total_cells = 0;
    let mut applicable_cells = 0;
    let mut not_applicable_cells = 0;

    for state in states {
        for event in events {
            total_cells += 1;
            let c = cell(state, event);
            if c.next_state.is_some() {
                applicable_cells += 1;
            } else {
                not_applicable_cells += 1;
            }
        }
    }

    assert_eq!(total_cells, 300);
    assert_eq!(applicable_cells, 252);
    assert_eq!(not_applicable_cells, 48);
}

#[test]
fn test_fop1_types() {
    // req: COP-001, RS-021
    assert_ne!(BcCommand::Unlock, BcCommand::SetVr);
    assert_ne!(FrameKind::Ad, FrameKind::Bc);
    assert_ne!(FrameKind::Bc, FrameKind::Bd);
    assert_ne!(FrameKind::Ad, FrameKind::Bd);
}
