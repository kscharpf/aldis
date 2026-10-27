//! Exhaustive unit tests for FARM-1 State Table 6-1 and common notification types.
//!
//! Verifies all 33 cells (S1–S3 × E1–E11) of Table 6-1 against CCSDS 232.1-B-2
//! and `docs/verification/reviews/cop1-tables.md` (COP-002, COP-003, COP-004, RS-021).

#![cfg_attr(coverage_nightly, coverage(off))]

use core::time::Duration;

use aldis_cop1::{
    Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, FARM1_EVENT_COUNT,
    FARM1_STATE_COUNT, FARM1_TABLE, Farm1Action, Farm1Cell, Farm1Event, Farm1State, TimeoutType,
    TransferNotification, TransferNotify,
};

const fn state_idx(s: Farm1State) -> usize {
    match s {
        Farm1State::Open => 0,
        Farm1State::Wait => 1,
        Farm1State::Lockout => 2,
    }
}

const fn event_idx(e: Farm1Event) -> usize {
    match e {
        Farm1Event::E1 => 0,
        Farm1Event::E2 => 1,
        Farm1Event::E3 => 2,
        Farm1Event::E4 => 3,
        Farm1Event::E5 => 4,
        Farm1Event::E6 => 5,
        Farm1Event::E7 => 6,
        Farm1Event::E8 => 7,
        Farm1Event::E9 => 8,
        Farm1Event::E10 => 9,
        Farm1Event::E11 => 10,
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "state_idx and event_idx are bounded within [0..3] and [0..11] by exhaustive enum mapping"
)]
const fn cell(state: Farm1State, event: Farm1Event) -> Farm1Cell {
    FARM1_TABLE[state_idx(state)][event_idx(event)]
}

#[test]
fn test_farm1_table_dimensions() {
    // req: COP-002, COP-003, RS-021
    assert_eq!(FARM1_STATE_COUNT, 3);
    assert_eq!(FARM1_EVENT_COUNT, 11);
    assert_eq!(FARM1_TABLE.len(), 3);
    for row in &FARM1_TABLE {
        assert_eq!(row.len(), 11);
    }
}

#[test]
fn test_farm1_s1_open_cells() {
    // req: COP-002, COP-003, RS-021
    let s = Farm1State::Open;

    // E1: Valid Type-AD, N(S) = V(R), buffer available -> Accept frame, V(R) := V(R) + 1, Retransmit_Flag := 0 -> S1
    assert_eq!(
        cell(s, Farm1Event::E1),
        Farm1Cell {
            actions: &[
                Farm1Action::DeliverFdu,
                Farm1Action::IncrementVr,
                Farm1Action::SetRetransmitFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        }
    );

    // E2: Valid Type-AD, N(S) = V(R), buffer unavailable -> Discard, Retransmit_Flag := 1, Wait_Flag := 1 -> S2
    assert_eq!(
        cell(s, Farm1Event::E2),
        Farm1Cell {
            actions: &[
                Farm1Action::Discard,
                Farm1Action::SetRetransmitFlag(true),
                Farm1Action::SetWaitFlag(true),
            ],
            next_state: Some(Farm1State::Wait),
        }
    );

    // E3: Valid Type-AD, N(S) != V(R) in pos window -> Discard, Retransmit_Flag := 1 -> S1
    assert_eq!(
        cell(s, Farm1Event::E3),
        Farm1Cell {
            actions: &[Farm1Action::Discard, Farm1Action::SetRetransmitFlag(true)],
            next_state: Some(Farm1State::Open),
        }
    );

    // E4: Valid Type-AD, in neg window -> Discard -> S1
    assert_eq!(
        cell(s, Farm1Event::E4),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Open),
        }
    );

    // E5: Valid Type-AD, outside sliding window -> Discard, Lockout_Flag := 1 -> S3
    assert_eq!(
        cell(s, Farm1Event::E5),
        Farm1Cell {
            actions: &[Farm1Action::Discard, Farm1Action::SetLockoutFlag(true)],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E6: Valid Type-BD frame arrives -> Accept, increment FARM-B_Counter -> S1
    assert_eq!(
        cell(s, Farm1Event::E6),
        Farm1Cell {
            actions: &[Farm1Action::DeliverFdu, Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Open),
        }
    );

    // E7: Valid Unlock Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0 -> S1
    assert_eq!(
        cell(s, Farm1Event::E7),
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        }
    );

    // E8: Valid Set V(R) Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, V(R) := V*(R) -> S1
    assert_eq!(
        cell(s, Farm1Event::E8),
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetVr,
            ],
            next_state: Some(Farm1State::Open),
        }
    );

    // E9: Invalid frame arrives -> Discard -> S1
    assert_eq!(
        cell(s, Farm1Event::E9),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Open),
        }
    );

    // E10: Buffer release signal -> Ignore -> S1
    assert_eq!(
        cell(s, Farm1Event::E10),
        Farm1Cell {
            actions: &[],
            next_state: Some(Farm1State::Open),
        }
    );

    // E11: CLCW report time -> Generate CLCW -> S1
    assert_eq!(
        cell(s, Farm1Event::E11),
        Farm1Cell {
            actions: &[Farm1Action::GenerateClcw],
            next_state: Some(Farm1State::Open),
        }
    );
}

#[test]
fn test_farm1_s2_wait_cells() {
    // req: COP-002, COP-003, RS-021
    let s = Farm1State::Wait;

    // E1: Not applicable (COP-005)
    assert_eq!(
        cell(s, Farm1Event::E1),
        Farm1Cell {
            actions: &[],
            next_state: None,
        }
    );

    // E2: Valid Type-AD, N(S) = V(R), buffer unavailable -> Discard -> S2
    assert_eq!(
        cell(s, Farm1Event::E2),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        }
    );

    // E3: Valid Type-AD, N(S) != V(R) in pos window -> Discard -> S2
    assert_eq!(
        cell(s, Farm1Event::E3),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        }
    );

    // E4: Valid Type-AD, in neg window -> Discard -> S2
    assert_eq!(
        cell(s, Farm1Event::E4),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        }
    );

    // E5: Valid Type-AD, outside sliding window -> Discard, Lockout_Flag := 1 -> S3
    assert_eq!(
        cell(s, Farm1Event::E5),
        Farm1Cell {
            actions: &[Farm1Action::Discard, Farm1Action::SetLockoutFlag(true)],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E6: Valid Type-BD frame arrives -> Accept, increment FARM-B_Counter -> S2
    assert_eq!(
        cell(s, Farm1Event::E6),
        Farm1Cell {
            actions: &[Farm1Action::DeliverFdu, Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Wait),
        }
    );

    // E7: Valid Unlock Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, Wait_Flag := 0 -> S1
    assert_eq!(
        cell(s, Farm1Event::E7),
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetWaitFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        }
    );

    // E8: Valid Set V(R) Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, Wait_Flag := 0, V(R) := V*(R) -> S1
    assert_eq!(
        cell(s, Farm1Event::E8),
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetWaitFlag(false),
                Farm1Action::SetVr,
            ],
            next_state: Some(Farm1State::Open),
        }
    );

    // E9: Invalid frame arrives -> Discard -> S2
    assert_eq!(
        cell(s, Farm1Event::E9),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Wait),
        }
    );

    // E10: Buffer release signal -> Wait_Flag := 0 -> S1
    assert_eq!(
        cell(s, Farm1Event::E10),
        Farm1Cell {
            actions: &[Farm1Action::SetWaitFlag(false)],
            next_state: Some(Farm1State::Open),
        }
    );

    // E11: CLCW report time -> Generate CLCW -> S2
    assert_eq!(
        cell(s, Farm1Event::E11),
        Farm1Cell {
            actions: &[Farm1Action::GenerateClcw],
            next_state: Some(Farm1State::Wait),
        }
    );
}

#[test]
fn test_farm1_s3_lockout_cells() {
    // req: COP-002, COP-003, RS-021
    let s = Farm1State::Lockout;

    // E1: Valid Type-AD, N(S) = V(R), buffer available -> Discard -> S3
    assert_eq!(
        cell(s, Farm1Event::E1),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E2: Valid Type-AD, N(S) = V(R), buffer unavailable -> Discard -> S3
    assert_eq!(
        cell(s, Farm1Event::E2),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E3: Valid Type-AD, N(S) != V(R) in pos window -> Discard -> S3
    assert_eq!(
        cell(s, Farm1Event::E3),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E4: Valid Type-AD, in neg window -> Discard -> S3
    assert_eq!(
        cell(s, Farm1Event::E4),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E5: Valid Type-AD, outside sliding window -> Discard -> S3
    assert_eq!(
        cell(s, Farm1Event::E5),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E6: Valid Type-BD frame arrives -> Accept, increment FARM-B_Counter -> S3
    assert_eq!(
        cell(s, Farm1Event::E6),
        Farm1Cell {
            actions: &[Farm1Action::DeliverFdu, Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E7: Valid Unlock Type-BC frame arrives -> Increment FARM-B_Counter, Retransmit_Flag := 0, Wait_Flag := 0, Lockout_Flag := 0 -> S1
    assert_eq!(
        cell(s, Farm1Event::E7),
        Farm1Cell {
            actions: &[
                Farm1Action::IncrementFarmBCounter,
                Farm1Action::SetRetransmitFlag(false),
                Farm1Action::SetWaitFlag(false),
                Farm1Action::SetLockoutFlag(false),
            ],
            next_state: Some(Farm1State::Open),
        }
    );

    // E8: Valid Set V(R) Type-BC frame arrives -> Increment FARM-B_Counter -> S3
    assert_eq!(
        cell(s, Farm1Event::E8),
        Farm1Cell {
            actions: &[Farm1Action::IncrementFarmBCounter],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E9: Invalid frame arrives -> Discard -> S3
    assert_eq!(
        cell(s, Farm1Event::E9),
        Farm1Cell {
            actions: &[Farm1Action::Discard],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E10: Buffer release signal -> Wait_Flag := 0 -> S3
    assert_eq!(
        cell(s, Farm1Event::E10),
        Farm1Cell {
            actions: &[Farm1Action::SetWaitFlag(false)],
            next_state: Some(Farm1State::Lockout),
        }
    );

    // E11: CLCW report time -> Generate CLCW -> S3
    assert_eq!(
        cell(s, Farm1Event::E11),
        Farm1Cell {
            actions: &[Farm1Action::GenerateClcw],
            next_state: Some(Farm1State::Lockout),
        }
    );
}

#[test]
fn test_exhaustive_33_cells() {
    // req: COP-002, COP-003, RS-021
    let states = [Farm1State::Open, Farm1State::Wait, Farm1State::Lockout];
    let events = [
        Farm1Event::E1,
        Farm1Event::E2,
        Farm1Event::E3,
        Farm1Event::E4,
        Farm1Event::E5,
        Farm1Event::E6,
        Farm1Event::E7,
        Farm1Event::E8,
        Farm1Event::E9,
        Farm1Event::E10,
        Farm1Event::E11,
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
                // Only (S2, E1) is Not Applicable in Table 6-1
                assert_eq!(state, Farm1State::Wait);
                assert_eq!(event, Farm1Event::E1);
            }
        }
    }

    assert_eq!(total_cells, 33);
    assert_eq!(applicable_cells, 32);
    assert_eq!(not_applicable_cells, 1);
}

#[test]
fn test_notify_types() {
    // req: COP-004, RS-021
    // Directive variants
    let d1 = Directive::InitiateAdWithoutClcwCheck;
    let d2 = Directive::InitiateAdWithClcwCheck;
    let d3 = Directive::InitiateAdWithUnlock;
    let d4 = Directive::InitiateAdWithSetVr { v_r: 42 };
    let d5 = Directive::TerminateAd;
    let d6 = Directive::ResumeAd;
    let d7 = Directive::SetVs { v_s: 100 };
    let d8 = Directive::SetSlidingWindowWidth { width: 10 };
    let d9 = Directive::SetT1Initial {
        duration: Duration::from_millis(500),
    };
    let d10 = Directive::SetTransmissionLimit { limit: 5 };
    let d11 = Directive::SetTimeoutType {
        timeout_type: TimeoutType::Tt0,
    };
    let d12 = Directive::SetTimeoutType {
        timeout_type: TimeoutType::Tt1,
    };

    assert_ne!(d1, d2);
    assert_ne!(d3, d5);
    assert_eq!(d4, Directive::InitiateAdWithSetVr { v_r: 42 });
    assert_eq!(d6, Directive::ResumeAd);
    assert_eq!(d7, Directive::SetVs { v_s: 100 });
    assert_eq!(d8, Directive::SetSlidingWindowWidth { width: 10 });
    assert_eq!(
        d9,
        Directive::SetT1Initial {
            duration: Duration::from_millis(500)
        }
    );
    assert_eq!(d10, Directive::SetTransmissionLimit { limit: 5 });
    assert_ne!(d11, d12);

    // TimeoutType enum values
    assert_eq!(TimeoutType::Tt0, TimeoutType::Tt0);
    assert_ne!(TimeoutType::Tt0, TimeoutType::Tt1);

    // AlertReason variants
    let reasons = [
        AlertReason::Limit,
        AlertReason::T1,
        AlertReason::Lockout,
        AlertReason::Synch,
        AlertReason::NnR,
        AlertReason::Clcw,
        AlertReason::Llif,
        AlertReason::Term,
    ];
    assert_eq!(reasons.len(), 8);

    // Alert struct with event number and prior state (COP-004)
    let alert = Alert {
        reason: AlertReason::Synch,
        event_number: 1,
        prior_state: Farm1State::Wait,
    };
    assert_eq!(alert.reason, AlertReason::Synch);
    assert_eq!(alert.event_number, 1);
    assert_eq!(alert.prior_state, Farm1State::Wait);

    // Notifications
    let t_notif = TransferNotification::PositiveConfirm;
    assert_eq!(t_notif, TransferNotification::PositiveConfirm);
    assert_ne!(t_notif, TransferNotification::NegativeConfirm);

    let t_notify = TransferNotify {
        handle: 123u32,
        status: TransferNotification::Accept,
    };
    assert_eq!(t_notify.handle, 123);
    assert_eq!(t_notify.status, TransferNotification::Accept);

    let d_notif = DirectiveNotification::PositiveConfirm;
    assert_eq!(d_notif, DirectiveNotification::PositiveConfirm);
    assert_ne!(d_notif, DirectiveNotification::Reject);

    let d_notify = DirectiveNotify {
        handle: 456u32,
        status: DirectiveNotification::PositiveConfirm,
    };
    assert_eq!(d_notify.handle, 456);
    assert_eq!(d_notify.status, DirectiveNotification::PositiveConfirm);
}
