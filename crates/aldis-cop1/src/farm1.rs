//! FARM-1 engine (CCSDS 232.1-B-2 §6; COP-002, COP-006, COP-030…COP-036).
//!
//! [`Farm1`] is plain data: the State, the Retransmit Flag, V(R), the FARM-B Counter,
//! (the Lockout and Wait Flags are derived from the State, OI-64) and the physical-layer flags of the CLCW (232.1 §6.1). [`Farm1::step`] is
//! the pure function of API-057: from the state, one [`Farm1Input`], and `now` it returns the
//! next state and the [`Farm1Actions`] of the cell of Table 6-1 ([`FARM1_TABLE`]), in the order
//! the cell lists them. The transition reads the table with an exhaustive `match` and no
//! wildcard arm (COP-003, RS-021); an error leaves the state unchanged (API-058).
//!
//! # Events and inputs
//!
//! | Event | Input |
//! |---|---|
//! | E1, E2 | [`Farm1Input::Frame`] with a Type-AD frame, N(S) = V(R); `buffer_available` selects E1 or E2 (COP-033) |
//! | E3, E4, E5 | [`Farm1Input::Frame`] with a Type-AD frame, N(S) in the positive window, the negative window, or the lockout area (COP-030) |
//! | E6 | [`Farm1Input::Frame`] with a Type-BD frame |
//! | E7, E8 | [`Farm1Input::Frame`] with a Type-BC frame carrying Unlock or Set V(R) (COP-036) |
//! | E9 | [`Farm1Input::InvalidFrame`], and a Type-BC frame that carries neither command (COP-031) |
//! | E10 | [`Farm1Input::BufferReleased`] (COP-033) |
//! | E11 | the query [`Farm1::clcw`], not an input (COP-035) |
//!
//! [`Farm1Input::SetPhysicalFlags`] sets the No RF Available and No Bit Lock Flags of the CLCW,
//! which are not part of COP-1 (232.0 §4.2.1.8) and take no cell.
//!
//! # Delivery and the output area
//!
//! The engine holds no delivery buffer (COP-033, OI-56). An accepted FDU (E1, E6) is written
//! to the caller's output area from offset 0, and the [`Farm1Output::Deliver`] action holds its
//! [`OutRange`] (API-061). A [`Farm1::step`] that delivers needs an area as long as the data
//! field of the frame, at most the Maximum Transfer Frame Length of the channel (API-062);
//! every other input writes nothing.
//!
//! # Action bound
//!
//! One input produces at most [`FARM1_MAX_ACTIONS`] actions, derived from [`FARM1_TABLE`] at
//! compile time (COP-006): for each cell the number of its Discard and Accept tokens, which are
//! returned as actions, plus the `Unlocked` and `StateChanged` notifications any cell may add;
//! the longest cell sets the bound. Its value is 3, reached by a cell with one
//! such token (S1 with E1 or E2); no cell actually pushes more than 2.

use aldis_core::ids::FrameSeqNum;
use aldis_core::time::Instant;
use aldis_core::{Error, ErrorKind, Result, field};
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, FrameType, TcVcId,
    ValidatedTcFrame,
};

use crate::actions::{Actions, OutRange};
use crate::farm1_table::{
    EVENT_COUNT, FARM1_TABLE, Farm1Action, Farm1Cell, Farm1Event, Farm1State, STATE_COUNT,
};

/// The most actions one [`Farm1::step`] can produce, derived from the action lists of Table 6-1
/// (232.1 §6.3.3; COP-006, API-057).
pub const FARM1_MAX_ACTIONS: usize = longest_cell();

/// The actions of one FARM-1 input (API-057, RS-022).
pub type Farm1Actions = Actions<Farm1Output, FARM1_MAX_ACTIONS>;

/// The most actions any cell of Table 6-1 can push, computed at compile time (COP-006): the
/// Discard and Accept tokens of its action list, which `run` returns as actions, plus the
/// `Unlocked` and `StateChanged` notifications that any cell may add. The other tokens change
/// state and return nothing.
#[expect(
    clippy::indexing_slicing,
    reason = "evaluated at compile time only: both indices stay below the table dimensions, \
              and an out-of-range index would fail the build, not run"
)]
const fn longest_cell() -> usize {
    let mut longest = 0;
    let mut state = 0;
    // bound: STATE_COUNT passes, `state` grows by one per pass
    while state < STATE_COUNT {
        let mut event = 0;
        // bound: EVENT_COUNT passes, `event` grows by one per pass
        while event < EVENT_COUNT {
            let actions = FARM1_TABLE[state][event].actions;
            // Every cell can also push `Unlocked` and `StateChanged` (see `run`).
            let mut pushes: usize = 2;
            let mut i: usize = 0;
            // bound: `actions.len()` passes, `i` grows by one per pass
            while i < actions.len() {
                if matches!(actions[i], Farm1Action::Discard | Farm1Action::DeliverFdu) {
                    pushes = pushes.saturating_add(1);
                }
                i = i.saturating_add(1);
            }
            if pushes > longest {
                longest = pushes;
            }
            event = event.saturating_add(1);
        }
        state = state.saturating_add(1);
    }
    longest
}

/// The cell of Table 6-1 for `state` and `event` (232.1 §6.3.3; COP-002, COP-003, RS-021).
///
/// The irrefutable array patterns bind one name per state and per event, so that the lookup
/// is an exhaustive `match` with no wildcard arm and no indexing.
const fn cell(state: Farm1State, event: Farm1Event) -> &'static Farm1Cell {
    let [open, wait, lockout] = &FARM1_TABLE;
    let row = match state {
        Farm1State::Open => open,
        Farm1State::Wait => wait,
        Farm1State::Lockout => lockout,
    };
    let [e1, e2, e3, e4, e5, e6, e7, e8, e9, e10, e11] = row;
    match event {
        Farm1Event::E1 => e1,
        Farm1Event::E2 => e2,
        Farm1Event::E3 => e3,
        Farm1Event::E4 => e4,
        Farm1Event::E5 => e5,
        Farm1Event::E6 => e6,
        Farm1Event::E7 => e7,
        Farm1Event::E8 => e8,
        Farm1Event::E9 => e9,
        Farm1Event::E10 => e10,
        Farm1Event::E11 => e11,
    }
}

/// The widest NW Table 7-2 allows (232.1 §7.3, Table 7-2).
const MAX_NW: u16 = 127;

/// `InvalidConfig` naming the window `parameter` (API-040).
const fn bad_window(parameter: aldis_core::FieldId) -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(parameter)
}

/// The FARM Sliding Window: its positive part PW and negative part NW (232.1 §6.1.8, §7.3
/// Table 7-2; COP-030, API-040, RS-020).
///
/// The constructors check the relations of §6.1.8.3 once, so a window never holds a PW and NW
/// that overlap or exceed the sequence space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Farm1Window {
    pw: u16,
    nw: u16,
}

impl Farm1Window {
    /// The window of COP-1 operated with Type-AD frame retransmission allowed: W is even with
    /// 2 ≤ W ≤ 254, and PW = NW = W/2 (232.1 §6.1.8.2, §6.1.8.3.1; COP-030).
    ///
    /// # Errors
    /// `InvalidConfig` naming `farm_sliding_window_w` for a W outside 2..=254 or odd
    /// (API-040).
    pub fn retransmission_allowed(w: u16) -> Result<Farm1Window> {
        if !(2..=254).contains(&w) || w & 1 == 1 {
            return Err(bad_window(field::FARM_SLIDING_WINDOW_W));
        }
        let half = w / 2;
        Ok(Farm1Window { pw: half, nw: half })
    }

    /// The window of COP-1 operated with Type-AD frame retransmission not allowed
    /// (`Transmission_Limit` = 1): 1 ≤ PW ≤ W ≤ 256, NW = W − PW, and PW > NW is permitted
    /// (232.1 §6.1.8.2, §6.1.8.3.2, Table 7-2; COP-030).
    ///
    /// # Errors
    /// `InvalidConfig` naming `farm_sliding_window_w` for a W outside 1..=256,
    /// `farm_positive_window_pw` for a PW outside 1..=W, and `farm_negative_window_nw` when
    /// W − PW exceeds 127, the largest NW of Table 7-2 (API-040).
    pub fn retransmission_not_allowed(w: u16, pw: u16) -> Result<Farm1Window> {
        if !(1..=256).contains(&w) {
            return Err(bad_window(field::FARM_SLIDING_WINDOW_W));
        }
        if !(1..=w).contains(&pw) {
            return Err(bad_window(field::FARM_POSITIVE_WINDOW_PW));
        }
        let nw = w.saturating_sub(pw);
        if nw > MAX_NW {
            return Err(bad_window(field::FARM_NEGATIVE_WINDOW_NW));
        }
        Ok(Farm1Window { pw, nw })
    }

    /// The Sliding Window Width W = PW + NW (232.1 §6.1.8.2).
    #[must_use]
    pub const fn width(self) -> u16 {
        self.pw.saturating_add(self.nw)
    }

    /// The Positive Window Width PW (232.1 §6.1.8.3).
    #[must_use]
    pub const fn positive(self) -> u16 {
        self.pw
    }

    /// The Negative Window Width NW (232.1 §6.1.8.3).
    #[must_use]
    pub const fn negative(self) -> u16 {
        self.nw
    }
}

/// The managed parameters of one FARM-1 instance, one per Virtual Channel (232.1 §7.3; COP-035,
/// API-041).
///
/// The CLCW reporting period of Table 7-2 belongs to the caller, which decides when to ask for
/// the [`Farm1::clcw`] (E11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Farm1Config {
    /// The Virtual Channel Identification the CLCW reports (232.0 §4.2.1.6).
    pub vc_id: TcVcId,
    /// The Status Field the CLCW carries, passed through unchanged (232.0 §4.2.1.4).
    pub status: ClcwStatus,
    /// The FARM Sliding Window (232.1 §6.1.8).
    pub window: Farm1Window,
}

/// The kind of FDU a [`Farm1Output::Deliver`] action carries (232.1 §6.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FduKind {
    /// The FDU of an accepted Type-AD frame, E1 (Sequence-Controlled Service).
    Ad,
    /// The FDU of an accepted Type-BD frame, E6 (Expedited Service).
    Bd,
}

/// A frame the Lower Procedures passed with the Frame Validation Checks (232.1 §6.3.2.1;
/// TC-021, COP-031), reduced to what FARM-1 reads (COP-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farm1Frame<'a> {
    /// A valid Type-AD frame with Frame Sequence Number N(S) and its Frame Data Unit.
    Ad {
        /// N(S) (232.0 §4.1.2.8).
        sequence_number: FrameSeqNum,
        /// The data field, delivered when the frame is accepted.
        data: &'a [u8],
    },
    /// A valid Type-BD frame with its Frame Data Unit.
    Bd {
        /// The data field, delivered when the frame is accepted.
        data: &'a [u8],
    },
    /// A valid Type-BC frame: the Control Command, or `None` when the data field is neither
    /// Unlock nor Set V(R), which is event E9 (232.1 §6.3.2.1; COP-031).
    Bc {
        /// The Control Command of the data field (232.0 §4.1.3.3).
        command: Option<ControlCommand>,
    },
}

impl<'a> Farm1Frame<'a> {
    /// The frame FARM-1 reads from a validated TC frame (232.0 §4.1.2.3; COP-031, API-016).
    ///
    /// It takes a [`ValidatedTcFrame`], so that TC-023 (every frame is validated before any
    /// FARM-1 processing) cannot be skipped by passing a frame from `TcFrame::decode`.
    ///
    /// A Type-BC frame whose Control Command the decoder did not recognise, which only lenient
    /// decoding passes, has `command: None`.
    #[must_use]
    pub const fn from_tc(validated: &ValidatedTcFrame<'a>) -> Farm1Frame<'a> {
        let frame = validated.as_frame();
        match frame.frame_type() {
            FrameType::Ad => Farm1Frame::Ad {
                sequence_number: frame.sequence_number(),
                data: frame.data_field(),
            },
            FrameType::Bd => Farm1Frame::Bd {
                data: frame.data_field(),
            },
            FrameType::Bc => Farm1Frame::Bc {
                command: frame.control_command(),
            },
        }
    }
}

/// One input of FARM-1 (API-057; COP-033, COP-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farm1Input<'a> {
    /// A valid frame arrives: E1 to E8, or E9 for a Type-BC frame with an unknown command.
    Frame {
        /// The Virtual Channel Identifier of the frame; FARM-1 refuses a frame whose VCID is not
        /// that of its [`Farm1Config::vc_id`] (232.1 §3.3.1, §6.1.1; COP-037).
        vc_id: TcVcId,
        /// The frame.
        frame: Farm1Frame<'a>,
        /// Whether the caller has a buffer for the FDU of an accepted Type-AD frame; it
        /// selects E1 (`true`) or E2 (`false`) for N(S) = V(R) and is read for no other event
        /// (COP-033).
        buffer_available: bool,
    },
    /// An invalid frame arrives, E9 (232.1 Table 6-1).
    InvalidFrame,
    /// The Buffer Release Signal, E10: buffer space for at least one more maximum-size frame
    /// is available (232.1 §6.3.2.3; COP-033).
    BufferReleased,
    /// Sets the No RF Available and No Bit Lock Flags of the CLCW (232.0 §4.2.1.8.2,
    /// §4.2.1.8.3). Not a COP-1 event: no cell, no action.
    SetPhysicalFlags {
        /// The No RF Available Flag.
        no_rf_available: bool,
        /// The No Bit Lock Flag.
        no_bit_lock: bool,
    },
}

impl<'a> Farm1Input<'a> {
    /// The frame input of a validated TC frame: its Virtual Channel Identifier, the frame
    /// FARM-1 reads from it ([`Farm1Frame::from_tc`]), and `buffer_available` (232.0 §4.1.2.6;
    /// COP-037, COP-033, API-016).
    ///
    /// Taking the VCID from the validated frame keeps it from being supplied by hand.
    #[must_use]
    pub const fn from_tc(
        validated: &ValidatedTcFrame<'a>,
        buffer_available: bool,
    ) -> Farm1Input<'a> {
        Farm1Input::Frame {
            vc_id: validated.as_frame().vc_id(),
            frame: Farm1Frame::from_tc(validated),
            buffer_available,
        }
    }
}

/// A notification of FARM-1 to the caller (API-083, COP-004, COP-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Farm1Notify {
    /// A frame was discarded, by the Discard action of the cell (232.1 §6.2.3). The event
    /// number and the state before it say why (COP-004).
    FrameDiscarded {
        /// The event.
        event: Farm1Event,
        /// The state before the event.
        state: Farm1State,
    },
    /// The State variable changed (232.1 §6.3.1; COP-004).
    StateChanged {
        /// The event that caused the change.
        event: Farm1Event,
        /// The state before.
        from: Farm1State,
        /// The state after.
        to: Farm1State,
    },
    /// An Unlock Control Command was executed (E7): it reset FARM-1 only, so the Higher
    /// Procedures are to purge their buffers as they need (232.1 §6.3.2.2 note; COP-036).
    Unlocked,
}

/// One action of FARM-1 (API-057, API-061, API-083; cop1.md §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farm1Output {
    /// The Accept action: the FDU is in the output area, and the Higher Procedures are to be
    /// signalled (232.1 §6.2.2).
    Deliver {
        /// Type-AD or Type-BD.
        kind: FduKind,
        /// N(S) of the frame; 0 for a Type-BD frame (232.0 §4.1.2.8).
        sequence_number: FrameSeqNum,
        /// Where the FDU is in the output area (API-061).
        data: OutRange,
    },
    /// A notification (API-083).
    Notify(Farm1Notify),
}

/// What the cell of an event needs from the input: the FDU for Accept and V*(R) for Set V(R).
struct Context<'a> {
    kind: FduKind,
    sequence_number: FrameSeqNum,
    v_star: FrameSeqNum,
    fdu: &'a [u8],
}

/// The FARM-1 state of one Virtual Channel (232.1 §6.1; COP-002, API-060).
///
/// Plain data without a delivery buffer or an output queue (COP-033, API-059).
///
/// The Lockout and Wait Flags are not stored: 232.1 §6.1.3 and §6.1.4 define them as functions
/// of the State variable, so they are derived from it (OI-64; RS-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Farm1 {
    config: Farm1Config,
    state: Farm1State,
    retransmit: bool,
    v_r: FrameSeqNum,
    farm_b_counter: FarmBCounter,
    no_rf_available: bool,
    no_bit_lock: bool,
}

impl Farm1 {
    /// A FARM-1 in State S1 (Open) with the Retransmit Flag and the FARM-B Counter at 0, expecting
    /// `v_r` as the next N(S) (232.1 §6.1; OI-58).
    #[must_use]
    pub fn new(config: Farm1Config, v_r: FrameSeqNum) -> Farm1 {
        Farm1 {
            config,
            state: Farm1State::Open,
            retransmit: false,
            v_r,
            farm_b_counter: FarmBCounter::default(),
            no_rf_available: false,
            no_bit_lock: false,
        }
    }

    /// The managed parameters (232.1 §7.3).
    #[must_use]
    pub const fn config(&self) -> Farm1Config {
        self.config
    }

    /// The State variable (232.1 §6.1.2; API-056).
    #[must_use]
    pub const fn state(&self) -> Farm1State {
        self.state
    }

    /// V(R), the N(S) expected in the next Type-AD frame (232.1 §6.1.7; API-056).
    #[must_use]
    pub const fn v_r(&self) -> FrameSeqNum {
        self.v_r
    }

    /// The Lockout Flag: set exactly in State S3 (232.1 §6.1.3; OI-64; API-056).
    #[must_use]
    pub const fn lockout_flag(&self) -> bool {
        matches!(self.state, Farm1State::Lockout)
    }

    /// The Wait Flag: set exactly in State S2 (232.1 §6.1.4; OI-64; API-056).
    #[must_use]
    pub const fn wait_flag(&self) -> bool {
        matches!(self.state, Farm1State::Wait)
    }

    /// The Retransmit Flag (232.1 §6.1.5; API-056).
    #[must_use]
    pub const fn retransmit_flag(&self) -> bool {
        self.retransmit
    }

    /// The FARM-B Counter, modulo 4 (232.1 §6.1.6; COP-034, API-056).
    #[must_use]
    pub const fn farm_b_counter(&self) -> FarmBCounter {
        self.farm_b_counter
    }

    /// The CLCW of the state after the most recent input: Event E11, "Report" (232.1 §6.2.4,
    /// §6.3.2.4, Table 6-1; COP-035).
    ///
    /// A pure query: it changes no state and may be asked at any time. The CLCW carries COP in
    /// Effect `01`, the configured VCID and Status Field, the three flags (two derived from the State), the FARM-B Counter,
    /// V(R) as the Report Value, and the physical-layer flags last set.
    #[must_use]
    pub const fn clcw(&self) -> Clcw {
        Clcw {
            status: self.config.status,
            cop_in_effect: CopInEffect::COP_1,
            vc_id: self.config.vc_id,
            no_rf_available: self.no_rf_available,
            no_bit_lock: self.no_bit_lock,
            lockout: self.lockout_flag(),
            wait: self.wait_flag(),
            retransmit: self.retransmit,
            farm_b_counter: self.farm_b_counter,
            report_value: self.v_r,
        }
    }

    /// The event of a Type-AD frame with Frame Sequence Number `n_s` (232.1 §6.1.8.3, Table
    /// 6-1 E1 to E5; COP-030, COP-033).
    ///
    /// The distance `n_s − V(R)` modulo 256 is 0 for E1 and E2, in 1..PW for the positive
    /// window (E3), and 256 − distance in 1..=NW for the negative window (E4); every other
    /// number is in the lockout area (E5).
    fn ad_event(&self, n_s: FrameSeqNum, buffer_available: bool) -> Farm1Event {
        let ahead = n_s.get().wrapping_sub(self.v_r.get());
        if ahead == 0 {
            if buffer_available {
                Farm1Event::E1
            } else {
                Farm1Event::E2
            }
        } else if u16::from(ahead) < self.config.window.pw {
            Farm1Event::E3
        } else if u16::from(ahead.wrapping_neg()) <= self.config.window.nw {
            Farm1Event::E4
        } else {
            Farm1Event::E5
        }
    }

    /// The pure step function of API-057 and RS-022: applies `input` to this state and returns
    /// the next state and the actions of the cell of Table 6-1, in the order of the cell
    /// (COP-002, COP-006). `now` is unused: FARM-1 has no timer (232.1 §6).
    ///
    /// The octets of an accepted FDU go to `out` from offset 0 (API-061); nothing else is
    /// written.
    ///
    /// # Errors
    /// - `InvalidState` for the cell Table 6-1 marks "Not applicable": a Type-AD frame with
    ///   N(S) = V(R) and a buffer available while the state is S2 (Wait), because the caller
    ///   is to feed [`Farm1Input::BufferReleased`] before it offers a buffer again (COP-005,
    ///   COP-033).
    /// - `UnknownChannel` (`vc_id`) for a frame of another Virtual Channel than the configured
    ///   one: one FARM-1 serves one VC, so it neither accepts nor discards the frame, and no event
    ///   of Table 6-1 applies (232.1 §3.3.1, §6.1.1; COP-037).
    /// - `BufferTooSmall` (detail `Needed`) if the input delivers an FDU longer than `out`
    ///   (API-061).
    ///
    /// An error leaves the state unchanged and produces no action (API-058).
    pub fn step(
        &self,
        input: Farm1Input<'_>,
        _now: Instant,
        out: &mut [u8],
    ) -> Result<(Farm1, Farm1Actions)> {
        let idle = Context {
            kind: FduKind::Ad,
            sequence_number: self.v_r,
            v_star: self.v_r,
            fdu: &[],
        };
        match input {
            Farm1Input::Frame { vc_id, .. } if vc_id != self.config.vc_id => {
                Err(Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID))
            }
            Farm1Input::Frame {
                vc_id: _,
                frame,
                buffer_available,
            } => match frame {
                Farm1Frame::Ad {
                    sequence_number,
                    data,
                } => self.apply(
                    self.ad_event(sequence_number, buffer_available),
                    &Context {
                        kind: FduKind::Ad,
                        sequence_number,
                        fdu: data,
                        ..idle
                    },
                    out,
                ),
                Farm1Frame::Bd { data } => self.apply(
                    Farm1Event::E6,
                    &Context {
                        kind: FduKind::Bd,
                        sequence_number: FrameSeqNum::new(0),
                        fdu: data,
                        ..idle
                    },
                    out,
                ),
                Farm1Frame::Bc {
                    command: Some(ControlCommand::Unlock),
                } => self.apply(Farm1Event::E7, &idle, out),
                Farm1Frame::Bc {
                    command: Some(ControlCommand::SetVr(v_star)),
                } => self.apply(Farm1Event::E8, &Context { v_star, ..idle }, out),
                Farm1Frame::Bc { command: None } => self.apply(Farm1Event::E9, &idle, out),
            },
            Farm1Input::InvalidFrame => self.apply(Farm1Event::E9, &idle, out),
            Farm1Input::BufferReleased => self.apply(Farm1Event::E10, &idle, out),
            Farm1Input::SetPhysicalFlags {
                no_rf_available,
                no_bit_lock,
            } => Ok((
                Farm1 {
                    no_rf_available,
                    no_bit_lock,
                    ..*self
                },
                Actions::new(),
            )),
        }
    }

    /// The next state and the actions of `event` (API-057).
    fn apply(
        &self,
        event: Farm1Event,
        ctx: &Context<'_>,
        out: &mut [u8],
    ) -> Result<(Farm1, Farm1Actions)> {
        let mut actions = Actions::new();
        let next = self.run(event, ctx, out, &mut actions)?;
        Ok((next, actions))
    }

    /// Executes the cell of `event` in the current state, appending its actions to `actions`
    /// (232.1 §6.3.1: the actions, then the State variable; COP-002, COP-003, COP-004).
    ///
    /// `actions` is the caller's list, so that a test can hand over a list without room and
    /// reach the failure of every push; `apply` hands over an empty one.
    fn run(
        &self,
        event: Farm1Event,
        ctx: &Context<'_>,
        out: &mut [u8],
        actions: &mut Farm1Actions,
    ) -> Result<Farm1> {
        let cell = cell(self.state, event);
        let Some(next) = cell.next_state else {
            return Err(Error::new(ErrorKind::InvalidState));
        };
        if cell.actions.contains(&Farm1Action::DeliverFdu) && out.len() < ctx.fdu.len() {
            return Err(Error::buffer_too_small(ctx.fdu.len()));
        }
        let mut now = *self;
        for action in cell.actions {
            match *action {
                Farm1Action::Discard => {
                    actions.push(Farm1Output::Notify(Farm1Notify::FrameDiscarded {
                        event,
                        state: self.state,
                    }))?;
                }
                Farm1Action::DeliverFdu => {
                    for (octet, fdu) in out.iter_mut().zip(ctx.fdu) {
                        *octet = *fdu;
                    }
                    actions.push(Farm1Output::Deliver {
                        kind: ctx.kind,
                        sequence_number: ctx.sequence_number,
                        data: OutRange::new(0, ctx.fdu.len()),
                    })?;
                }
                Farm1Action::IncrementVr => now.v_r = now.v_r.next(),
                Farm1Action::SetVr => now.v_r = ctx.v_star,
                Farm1Action::SetRetransmitFlag(value) => now.retransmit = value,
                Farm1Action::IncrementFarmBCounter => {
                    now.farm_b_counter = now.farm_b_counter.next();
                }
                // Event E11 reports the state through `clcw`: nothing to change or return. The
                // Wait and Lockout Flags are functions of the State variable (232.1 §6.1.3,
                // §6.1.4; OI-64): a test checks the table's actions on them against the next
                // state, and they are not applied.
                Farm1Action::GenerateClcw
                | Farm1Action::SetWaitFlag(_)
                | Farm1Action::SetLockoutFlag(_) => {}
            }
        }
        if matches!(event, Farm1Event::E7) {
            actions.push(Farm1Output::Notify(Farm1Notify::Unlocked))?;
        }
        if next != self.state {
            actions.push(Farm1Output::Notify(Farm1Notify::StateChanged {
                event,
                from: self.state,
                to: next,
            }))?;
        }
        now.state = next;
        Ok(now)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        Context, FARM1_MAX_ACTIONS, Farm1, Farm1Action, Farm1Actions, Farm1Config, Farm1Notify,
        Farm1Output, Farm1Window, FduKind, cell, longest_cell,
    };
    use crate::farm1_table::{EVENT_COUNT, FARM1_TABLE, Farm1Event, Farm1State, STATE_COUNT};
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::{ErrorDetail, ErrorKind};
    use aldis_tc::{ClcwStatus, TcVcId};

    fn farm() -> Farm1 {
        let config = Farm1Config {
            vc_id: TcVcId::new(1).unwrap(),
            status: ClcwStatus::new(0).unwrap(),
            window: Farm1Window::retransmission_allowed(16).unwrap(),
        };
        Farm1::new(config, FrameSeqNum::new(5))
    }

    const EVENTS: [Farm1Event; EVENT_COUNT] = [
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
    const STATES: [Farm1State; STATE_COUNT] =
        [Farm1State::Open, Farm1State::Wait, Farm1State::Lockout];

    #[test]
    fn lookup_matches_the_table_and_the_bound_covers_every_cell() {
        // req: COP-002, COP-003, COP-006
        let mut longest = 0;
        for (s, state) in STATES.into_iter().enumerate() {
            for (e, event) in EVENTS.into_iter().enumerate() {
                assert_eq!(
                    *cell(state, event),
                    FARM1_TABLE[s][e],
                    "S{} E{}",
                    s + 1,
                    e + 1
                );
                let returned = FARM1_TABLE[s][e]
                    .actions
                    .iter()
                    .filter(|a| matches!(a, Farm1Action::Discard | Farm1Action::DeliverFdu))
                    .count();
                longest = longest.max(returned + 2);
            }
        }
        assert_eq!(FARM1_MAX_ACTIONS, longest);
        // The const evaluation of the bound, run once more at run time for the coverage gate.
        assert_eq!(longest_cell(), longest);
        assert_eq!(FARM1_MAX_ACTIONS, 3);
    }

    #[test]
    fn flag_actions_of_every_cell_agree_with_the_next_state() {
        // req: COP-002, COP-003, RS-021
        // The Lockout and Wait Flags are functions of the State (232.1 §6.1.3, §6.1.4; OI-64).
        // Starting from the flags the state implies, the flag actions of a cell must leave the
        // flags the next state implies. The one exception is (S2, E5), which OI-64 names.
        for (s, state) in STATES.into_iter().enumerate() {
            for (e, event) in EVENTS.into_iter().enumerate() {
                let cell = FARM1_TABLE[s][e];
                let Some(next) = cell.next_state else {
                    continue;
                };
                let mut lockout = state == Farm1State::Lockout;
                let mut wait = state == Farm1State::Wait;
                for action in cell.actions {
                    match *action {
                        Farm1Action::SetLockoutFlag(v) => lockout = v,
                        Farm1Action::SetWaitFlag(v) => wait = v,
                        _ => {}
                    }
                }
                assert_eq!(lockout, next == Farm1State::Lockout, "{state:?} {event:?}");
                let exception = state == Farm1State::Wait && event == Farm1Event::E5;
                assert_eq!(
                    wait,
                    next == Farm1State::Wait || exception,
                    "{state:?} {event:?}"
                );
            }
        }
    }

    fn ctx() -> Context<'static> {
        Context {
            kind: FduKind::Ad,
            sequence_number: FrameSeqNum::new(5),
            v_star: FrameSeqNum::new(9),
            fdu: &[1, 2],
        }
    }

    #[test]
    fn every_cell_runs_within_the_bound_and_only_the_na_cell_fails() {
        // req: COP-005, COP-006, API-057, API-083
        let mut out = [0u8; 4];
        for state in STATES {
            for event in EVENTS {
                let mut f = farm();
                f.state = state;
                match f.apply(event, &ctx(), &mut out) {
                    Ok((after, actions)) => {
                        assert!(actions.len() <= FARM1_MAX_ACTIONS);
                        assert_eq!(Some(after.state), cell(state, event).next_state);
                    }
                    Err(e) => {
                        assert_eq!(e.kind(), ErrorKind::InvalidState);
                        assert_eq!(cell(state, event).next_state, None);
                    }
                }
            }
        }
    }

    #[test]
    fn an_area_shorter_than_the_fdu_fails_before_anything_is_written() {
        // req: API-061, API-058
        let mut out = [0xEE; 1];
        let e = farm().apply(Farm1Event::E1, &ctx(), &mut out).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(e.detail(), ErrorDetail::Needed(2));
        assert_eq!(out, [0xEE]);
    }

    #[test]
    fn a_list_without_room_fails_at_every_push_and_never_drops_an_action() {
        // req: API-057, API-083, COP-006
        let unlocked = Farm1Output::Notify(Farm1Notify::Unlocked);
        for state in STATES {
            for event in EVENTS {
                let mut f = farm();
                f.state = state;
                let mut out = [0u8; 4];
                let Ok((_, produced)) = f.apply(event, &ctx(), &mut out) else {
                    continue;
                };
                for fill in 0..=FARM1_MAX_ACTIONS {
                    let mut list = Farm1Actions::new();
                    for _ in 0..fill {
                        list.push(unlocked).unwrap();
                    }
                    let result = f.run(event, &ctx(), &mut out, &mut list);
                    if fill + produced.len() <= FARM1_MAX_ACTIONS {
                        assert!(result.is_ok());
                        assert_eq!(list.len(), fill + produced.len());
                    } else {
                        let e = result.unwrap_err();
                        assert_eq!(e.kind(), ErrorKind::InternalError);
                    }
                }
            }
        }
    }

    #[test]
    fn report_event_changes_nothing() {
        // req: COP-035, COP-002
        for state in STATES {
            let mut f = farm();
            f.state = state;
            let (after, actions) = f.apply(Farm1Event::E11, &ctx(), &mut []).unwrap();
            assert_eq!((after, actions.len()), (f, 0));
            assert_eq!(after.clcw(), f.clcw());
        }
    }
}
