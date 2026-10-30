//! FOP-1 engine (CCSDS 232.1-B-2 §5; COP-001, COP-004, COP-006, COP-010…COP-017).
//!
//! [`Fop1`] is plain data: the State, V(S), NN(R), the `Sent_Queue` with the master copies of
//! the Type-AD and Type-BC frames, the `Wait_Queue`, the three Out flags, the timer T1, the
//! `Transmission_Count`, `Suspend_State`, and the managed parameters (232.1 §5.1, §7.2).
//! [`Fop1::step`] is the pure function of API-057: from the state, one [`Fop1Input`], and `now`
//! it returns the next state and the [`Fop1Actions`] of the cell of Table 5-1 ([`FOP1_TABLE`]),
//! in the order the cell lists them. The transition reads the table with an exhaustive `match`
//! and no wildcard arm (COP-003, RS-021); an error leaves the state unchanged (API-058).
//!
//! # Events and inputs
//!
//! | Event | Input |
//! |---|---|
//! | E1–E14, E101–E103 | [`Fop1Input::Clcw`], classified by the Lockout Flag, then N(R) against NN(R) and V(S) modulo 256, then the Retransmit and Wait Flags (232.1 §5.3.2.1; COP-012) |
//! | E15 | [`Fop1Input::Clcw`] with a COP in Effect other than `01`, and [`Fop1Input::InvalidClcw`] (232.1 §5.3.2.1 a; OI-62) |
//! | E16–E18, E104 | [`Fop1Input::Timeout`] once `now` reaches [`Fop1::next_deadline`] (API-053) |
//! | E19–E22 | [`Fop1Input::RequestAd`], [`Fop1Input::RequestBd`] |
//! | E23–E39 | [`Fop1Input::Directive`] with a valid value |
//! | E40 | [`Fop1Input::Directive`] with a value outside its range (COP-015), and [`Fop1Input::InvalidDirective`] |
//! | E41–E46 | [`Fop1Input::TransmitDone`] |
//!
//! A CLCW for another Virtual Channel is refused with `UnknownChannel` and changes nothing: it is
//! a routing error, not a COP-1 event (cop1.md §2.2). [`Fop1::step_event`] runs the cell of a
//! given event without classifying an input; it is the table function the conformance vectors
//! address for the cells no input reaches (COP-005, API-058).
//!
//! # Readings
//!
//! - The engine starts in S6 with V(S) = NN(R) = 0, every Out flag Ready, no timer, and
//!   `Suspend_State` 0 (OI-59).
//! - A purge of the `Sent_Queue` sets NN(R) := V(S) (OI-60).
//! - A Type-BC frame on the `Sent_Queue` is purged without a notification; an Initiate AD
//!   directive that awaits its confirmation gets its Negative Confirm from Alert (§5.2.15 d).
//! - The timer is disarmed when it expires: the cell of E16 to E18 and E104 restarts it only
//!   where it lists Initiate AD (or BC) Retransmission (§5.2.7 c).
//! - No Alert follows an Alert (COP-017) because of the table, not a filter: every cell that
//!   lists Alert goes to S6, and no cell of S6 lists an Alert other than `Alert[LLIF]`. Every
//!   Alert a cell lists is returned (API-083).
//!
//! # The output area
//!
//! One input transmits at most one frame (every cell holds at most one Transmit or Look
//! action that sends). Its octets are written to the caller's output area from offset 0, and
//! the [`Fop1Output::Transmit`] action holds its [`OutRange`] (API-061). An area of the
//! channel's Maximum Transfer Frame Length is enough for every input (API-062).
//!
//! # Action bound
//!
//! One input produces at most [`fop1_max_actions`]`(Q)` actions, derived from [`FOP1_TABLE`] at
//! compile time (COP-006): for each cell, the actions each of its tokens can return, where a
//! purge or a removal of acknowledged frames returns one per `Sent_Queue` entry (`Q`), and the
//! longest cell sets the bound. The `N` of a [`Fop1`] must be at least that value, which its
//! constructor checks at compile time.

use aldis_core::ids::FrameSeqNum;
use aldis_core::layout::FECF_LEN;
use aldis_core::security::SecurityLayout;
use aldis_core::time::{Duration, Instant};
use aldis_core::{Error, ErrorKind, Result, field};
use aldis_tc::{
    Clcw, ControlCommand, CopInEffect, FrameData, FrameFields, FrameType, PRIMARY_HEADER_LEN,
    PhysicalChannelConfig, SegmentHeaderUse, TcSpacecraftId, TcVcId, VcConfig, VcTable, encode,
};
use core::num::NonZeroU8;
#[cfg(feature = "uslp")]
use {
    aldis_uslp::{
        FrameContent as UslpContent, FrameFields as UslpFields, FrameKind as UslpKind, MapConfig,
        SecurityScope, SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, Upid,
        UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig as UslpVcConfig, VcTable as UslpVcTable,
        encode as uslp_encode,
    },
    aldis_uslp::{PRIMARY_HEADER_LEN as USLP_HEADER_LEN, PhysicalChannelConfig as UslpPhysical},
};

use crate::actions::{Actions, OutRange};
use crate::fop1_table::{
    BcCommand, EVENT_COUNT, FOP1_TABLE, Fop1Action, Fop1Cell, Fop1Event, Fop1State, FrameKind,
    STATE_COUNT, SuspendState,
};
use crate::notify::{
    Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, TimeoutType,
    TransferNotification, TransferNotify,
};

/// The handle of a transfer request or a directive, chosen by the caller and returned in its
/// notifications (232.1 §4.2, §4.4; API-054).
pub type Handle = u32;

/// The most actions one token of `action` can return with a `Sent_Queue` of `q` frames (COP-006).
const fn weight(action: Fop1Action, q: usize) -> usize {
    match action {
        // One Positive (or Negative) Confirm per queued frame (§5.2.2, §5.2.8 a).
        Fop1Action::RemoveAckedFrames => q,
        // The purges of §5.2.14: a Negative Confirm per frame and a Reject.
        Fop1Action::Initialize => q.saturating_add(1),
        // The purges, the Negative Confirm of a pending directive, and the Alert (§5.2.15).
        Fop1Action::Alert(_) => q.saturating_add(3),
        // Accept and Transmit (§5.2.10 c); Accept and Confirm, or Reject.
        Fop1Action::LookForFdu | Fop1Action::SetVs => 2,
        Fop1Action::LookForDirective
        | Fop1Action::InitiateAdRetransmission
        | Fop1Action::InitiateBcRetransmission
        | Fop1Action::TransmitBd
        | Fop1Action::TransmitBc(_)
        | Fop1Action::Suspend
        | Fop1Action::TransferNotify(_)
        | Fop1Action::DirectiveNotify(_) => 1,
        Fop1Action::CancelTimer
        | Fop1Action::StartTimer
        | Fop1Action::ReleaseBcCopy
        | Fop1Action::AddToWaitQueue
        | Fop1Action::SetVsFromVr
        | Fop1Action::SetSlidingWindowWidth
        | Fop1Action::SetT1Initial
        | Fop1Action::SetTransmissionLimit
        | Fop1Action::SetTimeoutType
        | Fop1Action::SetSuspendState(_)
        | Fop1Action::Resume
        | Fop1Action::SetOutFlagReady(_) => 0,
    }
}

/// The most actions one [`Fop1::step`] can produce with a `Sent_Queue` of `sent_queue` frames,
/// derived from the action lists of Table 5-1 (232.1 §5.3; COP-006, API-057).
#[expect(
    clippy::indexing_slicing,
    reason = "every index is bounded by its loop: `state < STATE_COUNT`, `event < EVENT_COUNT`, \
              and `i < actions.len()` of the row and cell it indexes"
)]
#[must_use]
pub const fn fop1_max_actions(sent_queue: usize) -> usize {
    let mut longest = 0;
    let mut state = 0;
    // bound: STATE_COUNT passes, `state` grows by one per pass
    while state < STATE_COUNT {
        let mut event = 0;
        // bound: EVENT_COUNT passes, `event` grows by one per pass
        while event < EVENT_COUNT {
            let actions = FOP1_TABLE[state][event].actions;
            let mut pushes: usize = 0;
            let mut i: usize = 0;
            // bound: `actions.len()` passes, `i` grows by one per pass
            while i < actions.len() {
                pushes = pushes.saturating_add(weight(actions[i], sent_queue));
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

/// The cell of Table 5-1 for `state` and `event` (232.1 §5.3; COP-001, COP-003, RS-021).
///
/// The irrefutable array patterns bind one name per state and per event, so that the lookup
/// is an exhaustive `match` with no wildcard arm and no indexing.
#[expect(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "one arm per state and per event of Table 5-1, named after the event"
)]
const fn cell(state: Fop1State, event: Fop1Event) -> &'static Fop1Cell {
    let [s1, s2, s3, s4, s5, s6] = &FOP1_TABLE;
    let row = match state {
        Fop1State::Active => s1,
        Fop1State::RetransmitWithoutWait => s2,
        Fop1State::RetransmitWithWait => s3,
        Fop1State::InitializingWithoutBcFrame => s4,
        Fop1State::InitializingWithBcFrame => s5,
        Fop1State::Initial => s6,
    };
    let [
        e1,
        e2,
        e3,
        e4,
        e5,
        e6,
        e7,
        e8,
        e9,
        e10,
        e11,
        e12,
        e13,
        e14,
        e15,
        e16,
        e17,
        e18,
        e19,
        e20,
        e21,
        e22,
        e23,
        e24,
        e25,
        e26,
        e27,
        e28,
        e29,
        e30,
        e31,
        e32,
        e33,
        e34,
        e35,
        e36,
        e37,
        e38,
        e39,
        e40,
        e41,
        e42,
        e43,
        e44,
        e45,
        e46,
        e101,
        e102,
        e103,
        e104,
    ] = row;
    match event {
        Fop1Event::E1 => e1,
        Fop1Event::E2 => e2,
        Fop1Event::E3 => e3,
        Fop1Event::E4 => e4,
        Fop1Event::E5 => e5,
        Fop1Event::E6 => e6,
        Fop1Event::E7 => e7,
        Fop1Event::E8 => e8,
        Fop1Event::E9 => e9,
        Fop1Event::E10 => e10,
        Fop1Event::E11 => e11,
        Fop1Event::E12 => e12,
        Fop1Event::E13 => e13,
        Fop1Event::E14 => e14,
        Fop1Event::E15 => e15,
        Fop1Event::E16 => e16,
        Fop1Event::E17 => e17,
        Fop1Event::E18 => e18,
        Fop1Event::E19 => e19,
        Fop1Event::E20 => e20,
        Fop1Event::E21 => e21,
        Fop1Event::E22 => e22,
        Fop1Event::E23 => e23,
        Fop1Event::E24 => e24,
        Fop1Event::E25 => e25,
        Fop1Event::E26 => e26,
        Fop1Event::E27 => e27,
        Fop1Event::E28 => e28,
        Fop1Event::E29 => e29,
        Fop1Event::E30 => e30,
        Fop1Event::E31 => e31,
        Fop1Event::E32 => e32,
        Fop1Event::E33 => e33,
        Fop1Event::E34 => e34,
        Fop1Event::E35 => e35,
        Fop1Event::E36 => e36,
        Fop1Event::E37 => e37,
        Fop1Event::E38 => e38,
        Fop1Event::E39 => e39,
        Fop1Event::E40 => e40,
        Fop1Event::E41 => e41,
        Fop1Event::E42 => e42,
        Fop1Event::E43 => e43,
        Fop1Event::E44 => e44,
        Fop1Event::E45 => e45,
        Fop1Event::E46 => e46,
        Fop1Event::E101 => e101,
        Fop1Event::E102 => e102,
        Fop1Event::E103 => e103,
        Fop1Event::E104 => e104,
    }
}

/// The Virtual Channel FOP-1 builds frames for (COP-010, USLP-013).
///
/// A TC Virtual Channel without Segment Headers and without SDLS (232.0 §4.1, §5.3), or, with
/// the feature `uslp`, a USLP Virtual Channel and MAP of a variable-length Physical Channel
/// (732.1 §4.1).
///
/// FOP-1 puts the FDU in the data field as it is: the Frame Data Field of a TC frame, the TFDZ of
/// a USLP frame. Built only by [`tc`](Self::tc) and [`uslp`](Self::uslp), which check the channel
/// once (RS-020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fop1Target {
    channels: Channels,
    clcw_vc_id: u8,
    max_frame_len: usize,
    max_fdu_len: usize,
}

/// The channel tables of a target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Channels {
    Tc {
        table: VcTable<1, 0>,
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
    },
    #[cfg(feature = "uslp")]
    Uslp {
        table: UslpVcTable<1, 1>,
        spacecraft_id: UslpSpacecraftId,
        vc_id: UslpVcId,
        map_id: UslpMapId,
        tfdf: TfdfHeader,
    },
}

/// The identifiers of the channel a [`Fop1Target`] builds frames for (232.0 §4.1.2.5, §4.1.2.6;
/// 732.1 §4.1.2.2–5; COP-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fop1Channel {
    /// A TC Virtual Channel.
    Tc {
        /// The Spacecraft Identifier.
        spacecraft_id: TcSpacecraftId,
        /// The Virtual Channel Identifier.
        vc_id: TcVcId,
    },
    /// A USLP Virtual Channel and MAP.
    #[cfg(feature = "uslp")]
    Uslp {
        /// The Spacecraft Identifier.
        spacecraft_id: UslpSpacecraftId,
        /// The Virtual Channel Identifier.
        vc_id: UslpVcId,
        /// The MAP Identifier.
        map_id: UslpMapId,
    },
}

/// What a frame of a target carries: an FDU, or a Control Command.
#[derive(Debug, Clone, Copy)]
enum Payload<'a> {
    Fdu(&'a [u8]),
    Control(ControlCommand),
}

impl Fop1Target {
    /// The TC Virtual Channel (`spacecraft_id`, `vc_id`) with frames of at most `max_frame_len`
    /// octets, with the FECF when `fecf` (232.0 §4.1.4, Table 5-1, Table 5-3; COP-010).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_frame_len`) outside 6..=1024 octets, or too short for one data
    /// octet (232.0 Table 5-3, CMN-041).
    pub fn tc(
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        max_frame_len: usize,
        fecf: bool,
    ) -> Result<Fop1Target> {
        let physical = PhysicalChannelConfig::new(max_frame_len, fecf)?;
        let vc = VcConfig::new(
            spacecraft_id,
            vc_id,
            max_frame_len,
            SegmentHeaderUse::Absent(SecurityLayout::NONE),
        )?;
        let table = VcTable::new(physical, &[vc], &[])?;
        let trailer = if fecf { FECF_LEN } else { 0 };
        Ok(Fop1Target {
            channels: Channels::Tc {
                table,
                spacecraft_id,
                vc_id,
            },
            clcw_vc_id: vc_id.get(),
            max_frame_len,
            max_fdu_len: max_frame_len
                .saturating_sub(PRIMARY_HEADER_LEN)
                .saturating_sub(trailer),
        })
    }

    /// The USLP Virtual Channel `vc` and MAP `map` of the variable-length Physical Channel
    /// `physical`, whose frames carry the TFDF Header `tfdf` and an FDU as the TFDZ (732.1
    /// §4.1, §4.2.7.3; COP-010, USLP-013).
    ///
    /// FOP-1 supplies the Bypass/Sequence Control and Protocol Control Command Flags and the
    /// Frame Sequence Number (732.1 §4.2.7.4): Type-AD frames are sequence-controlled user data
    /// with N(S) in the VCF Count, Type-BD frames expedited user data, and Type-BC frames
    /// Protocol Control Commands with the UPID "COP-1 Control Commands" and the TFDZ of the
    /// Unlock or Set V(R) command. A frame has no Insert Zone, OCF, or SDLS fields, since
    /// FOP-1 has none to give.
    ///
    /// # Errors
    /// `InvalidConfig`, checked in this order:
    /// - (`transfer_frame_type`) for a fixed-length Physical Channel or Virtual Channel, whose
    ///   TFDZ cannot hold a one-octet command (and whose Insert Zone FOP-1 could not fill);
    /// - (`truncated_frame_len`) for a VC with a truncated frame length, which carries only
    ///   truncated frames (OI-48);
    /// - (`vcf_count_len`) unless the VCF Count Length is one octet for the sequence-controlled
    ///   service, which holds N(S) modulo 256 (232.1 §5.1.3), and none for the expedited
    ///   service, whose counter FOP-1 does not keep;
    /// - (`security_header_len`) for SDLS fields on the VC or the MAP;
    /// - (`tfdz_construction_rule`) for a rule of a fixed-length TFDZ (`000` to `010`);
    /// - (`max_frame_len`) for a frame too short for a Set V(R) command;
    /// - and those of [`UslpVcTable::new`](aldis_uslp::VcTable::new) (`map_id` when the MAP is
    ///   not of the VC).
    #[cfg(feature = "uslp")]
    pub fn uslp(
        physical: UslpPhysical,
        vc: UslpVcConfig,
        map: MapConfig,
        tfdf: TfdfHeader,
    ) -> Result<Fop1Target> {
        let variable = |t| matches!(t, TransferFrameType::Variable);
        if !variable(physical.frame_type()) || !variable(vc.frame_type()) {
            return Err(bad_config(field::TRANSFER_FRAME_TYPE));
        }
        if vc.truncated_frame_len().is_some() {
            return Err(bad_config(field::TRUNCATED_FRAME_LEN));
        }
        if vc.vcf_count_len(false).get() != 1 || vc.vcf_count_len(true).get() != 0 {
            return Err(bad_config(field::VCF_COUNT_LEN));
        }
        let no_sdls = match vc.security() {
            SecurityScope::Vc(layout) => layout.is_none(),
            SecurityScope::PerMap => map.security().is_none(),
        };
        if !no_sdls {
            return Err(bad_config(field::SECURITY_HEADER_LEN));
        }
        let fixed_rule = matches!(
            tfdf.rule,
            TfdzRule::PacketsSpanningFrames(_)
                | TfdzRule::SduStart(_)
                | TfdzRule::SduContinuation(_)
        );
        if fixed_rule {
            return Err(bad_config(field::TFDZ_CONSTRUCTION_RULE));
        }
        let max_frame_len = physical.frame_len();
        let trailer = if physical.fecf() { FECF_LEN } else { 0 };
        // The Primary Header, the one-octet VCF Count of a Type-AD frame, the one-octet TFDF
        // Header (the shortest, for a rule without a pointer), the TFDZ, and the FECF.
        let max_fdu_len = max_frame_len
            .saturating_sub(USLP_HEADER_LEN)
            .saturating_sub(1)
            .saturating_sub(1)
            .saturating_sub(trailer);
        if max_fdu_len < SET_VR_LEN {
            return Err(bad_config(field::MAX_FRAME_LEN));
        }
        let table = UslpVcTable::new(physical, &[vc], &[map])?;
        Ok(Fop1Target {
            channels: Channels::Uslp {
                table,
                spacecraft_id: vc.spacecraft_id(),
                vc_id: vc.vc_id(),
                map_id: map.map_id(),
                tfdf,
            },
            clcw_vc_id: vc.vc_id().get(),
            max_frame_len,
            max_fdu_len,
        })
    }

    /// The identifiers of the channel the frames are built for (COP-010).
    #[must_use]
    pub const fn channel(&self) -> Fop1Channel {
        match &self.channels {
            Channels::Tc {
                spacecraft_id,
                vc_id,
                ..
            } => Fop1Channel::Tc {
                spacecraft_id: *spacecraft_id,
                vc_id: *vc_id,
            },
            #[cfg(feature = "uslp")]
            Channels::Uslp {
                spacecraft_id,
                vc_id,
                map_id,
                ..
            } => Fop1Channel::Uslp {
                spacecraft_id: *spacecraft_id,
                vc_id: *vc_id,
                map_id: *map_id,
            },
        }
    }

    /// The Maximum Transfer Frame Length, the most octets one input writes to the output area
    /// (232.0 Table 5-3, 732.1 Table 5-1; API-062).
    #[must_use]
    pub const fn max_frame_len(&self) -> usize {
        self.max_frame_len
    }

    /// The longest FDU a frame of the channel carries (232.0 §4.1.3; 732.1 §4.1.4.2): for USLP,
    /// the TFDZ of a Type-AD frame, which has the VCF Count (COP-010).
    #[must_use]
    pub const fn max_fdu_len(&self) -> usize {
        self.max_fdu_len
    }

    /// Encodes a frame of `frame_type` with N(S) = `sequence_number` and `payload` into `out`;
    /// returns its length (232.0 §4.1, 732.1 §4.1; COP-010, TC-005, TC-008, USLP-013).
    fn encode(
        &self,
        frame_type: FrameType,
        sequence_number: FrameSeqNum,
        payload: Payload<'_>,
        out: &mut [u8],
    ) -> Result<usize> {
        match &self.channels {
            Channels::Tc {
                table,
                spacecraft_id,
                vc_id,
            } => {
                let fields = FrameFields {
                    frame_type,
                    spacecraft_id: *spacecraft_id,
                    vc_id: *vc_id,
                    sequence_number,
                };
                let data = match payload {
                    Payload::Fdu(user_data) => FrameData::Fdu {
                        segment_header: None,
                        user_data,
                    },
                    Payload::Control(command) => FrameData::Control(command),
                };
                encode(table, fields, data, out)
            }
            #[cfg(feature = "uslp")]
            Channels::Uslp {
                table,
                spacecraft_id,
                vc_id,
                map_id,
                tfdf,
            } => {
                let kind = match frame_type {
                    FrameType::Ad => UslpKind::SequenceControlled,
                    FrameType::Bd => UslpKind::Expedited,
                    FrameType::Bc => UslpKind::ProtocolControl,
                };
                // N(S) is the VCF Count of a sequence-controlled frame (732.1 §4.2.7.4 note 3);
                // the expedited counter is absent (the constructor checked it).
                let fields = UslpFields {
                    spacecraft_id: *spacecraft_id,
                    source_or_destination: SourceOrDestination::Destination,
                    vc_id: *vc_id,
                    map_id: *map_id,
                    kind,
                    vcf_count: u64::from(sequence_number.get()),
                };
                let (tfdf, octets) = match payload {
                    Payload::Fdu(user_data) => (*tfdf, user_data),
                    Payload::Control(command) => {
                        return uslp_control(table, fields, command, out);
                    }
                };
                let content = UslpContent {
                    tfdf,
                    insert_zone: &[],
                    ocf: None,
                };
                uslp_encode(table, fields, content, octets, out)
            }
        }
    }
}

/// The octets of the longest Control Command, Set V(R) (232.0 §4.1.3.3).
#[cfg(feature = "uslp")]
const SET_VR_LEN: usize = 3;

/// Encodes a Type-BC USLP frame carrying `command` as its TFDZ with the UPID "COP-1 Control
/// Commands" (732.1 Table 4-1; USLP-013).
#[cfg(feature = "uslp")]
fn uslp_control(
    table: &UslpVcTable<1, 1>,
    fields: UslpFields,
    command: ControlCommand,
    out: &mut [u8],
) -> Result<usize> {
    let content = UslpContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid: Upid::Cop1ControlCommands,
        },
        insert_zone: &[],
        ocf: None,
    };
    let octets = command.to_octets();
    uslp_encode(table, fields, content, octets.as_slice(), out)
}

/// The managed parameters of one FOP-1 instance, one per Virtual Channel, at start-up
/// (232.1 §7.2 Table 7-1; COP-015, COP-016, API-041).
///
/// The directives E36 to E39 change K, `T1_Initial`, `Transmission_Limit`, and `Timeout_Type`
/// later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fop1Config {
    /// The Virtual Channel the frames are built for (COP-010).
    pub target: Fop1Target,
    /// `T1_Initial`, at least 1 µs (232.1 §5.1.9).
    pub t1_initial: Duration,
    /// `Transmission_Limit` (232.1 §5.1.10.2).
    pub transmission_limit: NonZeroU8,
    /// `Timeout_Type` (232.1 §5.1.10.3).
    pub timeout_type: TimeoutType,
    /// The FOP Sliding Window Width K: 1..=min(255, `Q`) (232.1 §5.1.12; COP-015).
    pub sliding_window_k: u8,
    /// The systematic retransmission value of Type-AD frames, passed through on their
    /// Transmit actions and never acted on (232.1 §5.1.9.3 note 1; COP-016).
    pub ad_repetitions: u8,
    /// The systematic retransmission value of Type-BC frames (COP-016).
    pub bc_repetitions: u8,
}

/// `InvalidConfig` naming `parameter` (API-040).
const fn bad_config(parameter: aldis_core::FieldId) -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(parameter)
}

/// What a frame on the `Sent_Queue` is (232.1 §5.1.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SentKind {
    /// A Type-AD frame and the handle of its transfer request.
    Ad(Handle),
    /// The Type-BC frame of an Initiate AD directive.
    Bc,
}

/// The master copy of a frame on the `Sent_Queue` with its `To_Be_Retransmitted_Flag` (232.1
/// §5.1.5, §5.1.7; COP-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Sent<const F: usize> {
    frame: [u8; F],
    len: usize,
    sequence_number: FrameSeqNum,
    kind: SentKind,
    to_be_retransmitted: bool,
}

impl<const F: usize> Sent<F> {
    /// The handle of a Type-AD frame's request; `None` for the Type-BC frame.
    const fn handle(&self) -> Option<Handle> {
        match self.kind {
            SentKind::Ad(handle) => Some(handle),
            SentKind::Bc => None,
        }
    }
}

/// The FDU on the `Wait_Queue` and the handle of its request (232.1 §5.1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Waiting<const F: usize> {
    handle: Handle,
    fdu: [u8; F],
    len: usize,
}

/// One input of FOP-1 (232.1 §5.3.2; API-057, API-055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fop1Input<'a> {
    /// A CLCW arrives: E1 to E15, E101 to E103 (232.1 §5.3.2.1).
    Clcw(Clcw),
    /// A word that is not a CLCW under COP-1 arrives, such as one whose Control Word Type is
    /// `1`, which `Clcw::decode` refuses: E15 (232.1 §5.3.2.1).
    InvalidClcw,
    /// The timer T1 may have expired: E16 to E18 and E104 once `now` reaches the deadline,
    /// otherwise nothing (API-053).
    Timeout,
    /// A Request to Transfer FDU of the Sequence-Controlled Service: E19, E20 (232.1 §4.4).
    RequestAd {
        /// The handle of the request (API-054).
        handle: Handle,
        /// The FDU, 1..=[`Fop1Target::max_fdu_len`] octets; copied (API-060).
        fdu: &'a [u8],
    },
    /// A Request to Transfer FDU of the Expedited Service: E21, E22 (232.1 §4.4).
    RequestBd {
        /// The handle of the request (API-054).
        handle: Handle,
        /// The FDU, 1..=[`Fop1Target::max_fdu_len`] octets.
        fdu: &'a [u8],
    },
    /// A directive: E23 to E40 (232.1 §4.2, Table 4-1).
    Directive {
        /// The handle of the directive (API-054).
        handle: Handle,
        /// The directive and its value.
        directive: Directive,
    },
    /// A directive the caller could not parse (an unknown type, or a value that does not fit
    /// its field): E40 (232.1 Table 5-1).
    InvalidDirective {
        /// The handle of the directive.
        handle: Handle,
    },
    /// The response of the Lower Procedures to the outstanding Transmit Request of `kind`:
    /// E41 to E46. At most one request per kind is outstanding, so the kind is the transmit
    /// handle (232.1 §5.1.6; API-055).
    TransmitDone {
        /// The kind of the frame the response answers.
        kind: FrameKind,
        /// Accept (`true`) or Reject (`false`) Response.
        accepted: bool,
    },
}

/// One action of FOP-1 (232.1 §4, §5.2; API-057, API-061, API-083; cop1.md §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fop1Output {
    /// A Transmit Request for Frame to the Lower Procedures: the frame is in the output area
    /// (232.1 §5.2.4 to §5.2.6; COP-010, COP-011, COP-016).
    Transmit {
        /// Type-AD, Type-BC, or Type-BD.
        kind: FrameKind,
        /// N(S): V(S) for a Type-AD frame, 0 for the others (232.0 §4.1.2.8).
        sequence_number: FrameSeqNum,
        /// Whether this is a retransmission of a master copy from the `Sent_Queue`.
        retransmission: bool,
        /// The configured systematic retransmission value; 0 for a Type-BD frame (COP-016).
        repetitions: u8,
        /// Where the frame is in the output area (API-061).
        frame: OutRange,
    },
    /// An Abort Request to the Lower Procedures, the first action of Initiate AD (or BC)
    /// Retransmission (232.1 §5.2.7 a).
    Abort,
    /// A response to a Request to Transfer FDU (232.1 §4.4).
    Transfer(TransferNotify<Handle>),
    /// A response to a directive (232.1 §4.2).
    Directive(DirectiveNotify<Handle>),
    /// An Alert notification (232.1 §4.3, §5.2.15 e; COP-004).
    Alert(Alert),
    /// The Suspend notification (232.1 §4.3, §5.2.16).
    Suspend,
}

/// What a cell needs from its input: the handle, the FDU, and the directive.
#[derive(Clone, Copy)]
struct Context<'a> {
    event: Fop1Event,
    now: Instant,
    handle: Option<Handle>,
    fdu: &'a [u8],
    directive: Option<Directive>,
    n_r: FrameSeqNum,
}

/// The FOP-1 state of one Virtual Channel, with a `Sent_Queue` of `Q` frames of at most `F`
/// octets and an action list of `N` entries (232.1 §5.1; COP-001, API-060).
///
/// Plain data without an output queue (API-059). `N` must be at least
/// [`fop1_max_actions`]`(Q)`, and `F` at least the Maximum Transfer Frame Length of the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fop1<const Q: usize, const F: usize, const N: usize> {
    target: Fop1Target,
    t1_initial: Duration,
    transmission_limit: NonZeroU8,
    timeout_type: TimeoutType,
    k: u8,
    ad_repetitions: u8,
    bc_repetitions: u8,
    state: Fop1State,
    v_s: FrameSeqNum,
    nn_r: FrameSeqNum,
    sent: [Option<Sent<F>>; Q],
    wait: Option<Waiting<F>>,
    ad_out_ready: bool,
    bc_out_ready: bool,
    bd_out: Option<Handle>,
    pending_initiate: Option<Handle>,
    deadline: Option<Instant>,
    transmission_count: u8,
    suspend_state: SuspendState,
}

/// The actions of one FOP-1 input (API-057, RS-022).
pub type Fop1Actions<const N: usize> = Actions<Fop1Output, N>;

impl<const Q: usize, const F: usize, const N: usize> Fop1<Q, F, N> {
    /// Fails the build of a `Fop1` whose `N` is below the bound of Table 5-1 (COP-006).
    const BOUND: () = assert!(N >= fop1_max_actions(Q), "N is below fop1_max_actions(Q)");

    /// A FOP-1 in State S6 (Initial) with V(S) = NN(R) = 0, every Out flag Ready, no timer,
    /// and `Suspend_State` 0 (232.1 §5.1; OI-59).
    ///
    /// # Errors
    /// `InvalidConfig` naming `sliding_window_k` for a K outside 1..=min(255, `Q`) (COP-015),
    /// `t1_initial` for a `T1_Initial` of 0, and `max_frame_len` for a target whose frames do
    /// not fit `F` octets (API-040).
    pub fn new(config: Fop1Config) -> Result<Fop1<Q, F, N>> {
        let () = Self::BOUND;
        if !Self::k_fits(config.sliding_window_k) {
            return Err(bad_config(field::SLIDING_WINDOW_K));
        }
        if config.t1_initial.as_micros() == 0 {
            return Err(bad_config(field::T1_INITIAL));
        }
        if config.target.max_frame_len > F {
            return Err(bad_config(field::MAX_FRAME_LEN));
        }
        Ok(Fop1 {
            target: config.target,
            t1_initial: config.t1_initial,
            transmission_limit: config.transmission_limit,
            timeout_type: config.timeout_type,
            k: config.sliding_window_k,
            ad_repetitions: config.ad_repetitions,
            bc_repetitions: config.bc_repetitions,
            state: Fop1State::Initial,
            v_s: FrameSeqNum::new(0),
            nn_r: FrameSeqNum::new(0),
            sent: [None; Q],
            wait: None,
            ad_out_ready: true,
            bc_out_ready: true,
            bd_out: None,
            pending_initiate: None,
            deadline: None,
            transmission_count: 1,
            suspend_state: SuspendState::NotSuspended,
        })
    }

    /// Whether `k` is a valid FOP Sliding Window Width: 1..=min(255, `Q`) (232.1 §5.1.12;
    /// COP-015).
    fn k_fits(k: u8) -> bool {
        k >= 1 && usize::from(k) <= Q
    }

    /// The target Virtual Channel (COP-010).
    #[must_use]
    pub const fn target(&self) -> &Fop1Target {
        &self.target
    }

    /// The State variable (232.1 §5.1.2; API-056).
    #[must_use]
    pub const fn state(&self) -> Fop1State {
        self.state
    }

    /// V(S), the N(S) of the next new Type-AD frame (232.1 §5.1.3; API-056).
    #[must_use]
    pub const fn v_s(&self) -> FrameSeqNum {
        self.v_s
    }

    /// NN(R), the N(S) of the oldest unacknowledged Type-AD frame (232.1 §5.1.8; API-056).
    #[must_use]
    pub const fn nn_r(&self) -> FrameSeqNum {
        self.nn_r
    }

    /// The FOP Sliding Window Width K (232.1 §5.1.12; API-056).
    #[must_use]
    pub const fn sliding_window_k(&self) -> u8 {
        self.k
    }

    /// `T1_Initial` (232.1 §5.1.9; API-056).
    #[must_use]
    pub const fn t1_initial(&self) -> Duration {
        self.t1_initial
    }

    /// `Transmission_Limit` (232.1 §5.1.10.2; API-056).
    #[must_use]
    pub const fn transmission_limit(&self) -> NonZeroU8 {
        self.transmission_limit
    }

    /// `Timeout_Type` (232.1 §5.1.10.3; API-056).
    #[must_use]
    pub const fn timeout_type(&self) -> TimeoutType {
        self.timeout_type
    }

    /// `Transmission_Count` (232.1 §5.1.10.4; API-056).
    #[must_use]
    pub const fn transmission_count(&self) -> u8 {
        self.transmission_count
    }

    /// `Suspend_State` (232.1 §5.1.11; COP-014, API-056).
    #[must_use]
    pub const fn suspend_state(&self) -> SuspendState {
        self.suspend_state
    }

    /// The number of frames on the `Sent_Queue` (232.1 §5.1.7; API-056).
    #[must_use]
    pub fn sent_queue_len(&self) -> usize {
        self.sent.iter().flatten().count()
    }

    /// Whether an FDU waits on the `Wait_Queue` (232.1 §5.1.4; API-056).
    #[must_use]
    pub const fn wait_queue_occupied(&self) -> bool {
        self.wait.is_some()
    }

    /// Whether the Out flag of `kind` is Ready (232.1 §5.1.6; API-056).
    #[must_use]
    pub const fn out_flag_ready(&self, kind: FrameKind) -> bool {
        match kind {
            FrameKind::Ad => self.ad_out_ready,
            FrameKind::Bc => self.bc_out_ready,
            FrameKind::Bd => self.bd_out.is_none(),
        }
    }

    /// The instant at which [`Fop1Input::Timeout`] is to be fed, or `None` while the timer
    /// is not running (232.1 §5.1.9; COP-013, API-053).
    #[must_use]
    pub const fn next_deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// The event of a CLCW (232.1 §5.3.2.1, Table 5-1; COP-012).
    ///
    /// # Errors
    /// `UnknownChannel` (`vc_id`) for a CLCW of another Virtual Channel (cop1.md §2.2).
    fn clcw_event(&self, clcw: &Clcw) -> Result<Fop1Event> {
        if clcw.vc_id.get() != self.target.clcw_vc_id {
            return Err(Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID));
        }
        if clcw.cop_in_effect != CopInEffect::COP_1 {
            return Ok(Fop1Event::E15);
        }
        if clcw.lockout {
            return Ok(Fop1Event::E14);
        }
        let n_r = clcw.report_value;
        let outstanding = self.v_s.get().wrapping_sub(self.nn_r.get());
        let acked = n_r.get().wrapping_sub(self.nn_r.get());
        let first = n_r == self.nn_r;
        let (retransmit, wait) = (clcw.retransmit, clcw.wait);
        let event = if acked > outstanding {
            Fop1Event::E13
        } else if acked == outstanding {
            match (retransmit, wait, first) {
                (true, _, _) => Fop1Event::E4,
                (false, true, _) => Fop1Event::E3,
                (false, false, true) => Fop1Event::E1,
                (false, false, false) => Fop1Event::E2,
            }
        } else if !retransmit {
            match (wait, first) {
                (true, _) => Fop1Event::E7,
                (false, true) => Fop1Event::E5,
                (false, false) => Fop1Event::E6,
            }
        } else if self.transmission_limit.get() == 1 {
            if first {
                Fop1Event::E102
            } else {
                Fop1Event::E101
            }
        } else if !first {
            if wait { Fop1Event::E9 } else { Fop1Event::E8 }
        } else if self.transmission_count < self.transmission_limit.get() {
            if wait { Fop1Event::E11 } else { Fop1Event::E10 }
        } else if wait {
            Fop1Event::E103
        } else {
            Fop1Event::E12
        };
        Ok(event)
    }

    /// The event of a directive (232.1 Table 4-1, Table 5-1; COP-014, COP-015).
    fn directive_event(&self, directive: Directive) -> Fop1Event {
        match directive {
            Directive::InitiateAdWithoutClcwCheck => Fop1Event::E23,
            Directive::InitiateAdWithClcwCheck => Fop1Event::E24,
            Directive::InitiateAdWithUnlock => {
                if self.bc_out_ready {
                    Fop1Event::E25
                } else {
                    Fop1Event::E26
                }
            }
            Directive::InitiateAdWithSetVr { .. } => {
                if self.bc_out_ready {
                    Fop1Event::E27
                } else {
                    Fop1Event::E28
                }
            }
            Directive::TerminateAd => Fop1Event::E29,
            Directive::ResumeAd => match self.suspend_state {
                SuspendState::NotSuspended => Fop1Event::E30,
                SuspendState::Active => Fop1Event::E31,
                SuspendState::RetransmitWithoutWait => Fop1Event::E32,
                SuspendState::RetransmitWithWait => Fop1Event::E33,
                SuspendState::InitializingWithoutBcFrame => Fop1Event::E34,
            },
            Directive::SetVs { .. } => Fop1Event::E35,
            Directive::SetSlidingWindowWidth { width } => {
                if Self::k_fits(width) {
                    Fop1Event::E36
                } else {
                    Fop1Event::E40
                }
            }
            Directive::SetT1Initial { duration } => {
                if duration.as_micros() == 0 {
                    Fop1Event::E40
                } else {
                    Fop1Event::E37
                }
            }
            Directive::SetTransmissionLimit { .. } => Fop1Event::E38,
            Directive::SetTimeoutType { .. } => Fop1Event::E39,
        }
    }

    /// The event of `input` and the context its cell reads; `None` for a Timeout before the
    /// deadline, which changes nothing (API-053).
    fn classify<'a>(
        &self,
        input: Fop1Input<'a>,
        now: Instant,
    ) -> Result<Option<(Fop1Event, Context<'a>)>> {
        let mut ctx = Context {
            event: Fop1Event::E1,
            now,
            handle: None,
            fdu: &[],
            directive: None,
            n_r: self.nn_r,
        };
        let event = match input {
            Fop1Input::Clcw(clcw) => {
                ctx.n_r = clcw.report_value;
                self.clcw_event(&clcw)?
            }
            Fop1Input::InvalidClcw => Fop1Event::E15,
            Fop1Input::Timeout => {
                let Some(deadline) = self.deadline else {
                    return Ok(None);
                };
                if now < deadline {
                    return Ok(None);
                }
                let below = self.transmission_count < self.transmission_limit.get();
                match (below, self.timeout_type) {
                    (true, TimeoutType::Tt0) => Fop1Event::E16,
                    (true, TimeoutType::Tt1) => Fop1Event::E104,
                    (false, TimeoutType::Tt0) => Fop1Event::E17,
                    (false, TimeoutType::Tt1) => Fop1Event::E18,
                }
            }
            Fop1Input::RequestAd { handle, fdu } => {
                self.check_fdu(fdu)?;
                ctx.handle = Some(handle);
                ctx.fdu = fdu;
                if self.wait.is_none() {
                    Fop1Event::E19
                } else {
                    Fop1Event::E20
                }
            }
            Fop1Input::RequestBd { handle, fdu } => {
                self.check_fdu(fdu)?;
                ctx.handle = Some(handle);
                ctx.fdu = fdu;
                if self.bd_out.is_none() {
                    Fop1Event::E21
                } else {
                    Fop1Event::E22
                }
            }
            Fop1Input::Directive { handle, directive } => {
                ctx.handle = Some(handle);
                ctx.directive = Some(directive);
                self.directive_event(directive)
            }
            Fop1Input::InvalidDirective { handle } => {
                ctx.handle = Some(handle);
                Fop1Event::E40
            }
            Fop1Input::TransmitDone { kind, accepted } => match (kind, accepted) {
                (FrameKind::Ad, true) => Fop1Event::E41,
                (FrameKind::Ad, false) => Fop1Event::E42,
                (FrameKind::Bc, true) => Fop1Event::E43,
                (FrameKind::Bc, false) => Fop1Event::E44,
                (FrameKind::Bd, true) => {
                    ctx.handle = self.bd_out;
                    Fop1Event::E45
                }
                (FrameKind::Bd, false) => Fop1Event::E46,
            },
        };
        ctx.event = event;
        Ok(Some((event, ctx)))
    }

    /// `InvalidField` (`frame_data_field`) for an empty FDU, `OutOfRange` (`frame_len`) for
    /// one longer than a frame of the target carries (232.0 §4.1.3; CMN-041, TC-007).
    const fn check_fdu(&self, fdu: &[u8]) -> Result<()> {
        if fdu.is_empty() {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::FRAME_DATA_FIELD));
        }
        if fdu.len() > self.target.max_fdu_len {
            return Err(Error::new(ErrorKind::OutOfRange).with_field(field::FRAME_LEN));
        }
        Ok(())
    }

    /// The pure step function of API-057 and RS-022: classifies `input` into its event,
    /// applies the cell of Table 5-1 to this state, and returns the next state and the
    /// actions of the cell, in the order of the cell (COP-001, COP-006).
    ///
    /// A frame to transmit goes to `out` from offset 0 (API-061); nothing else is written.
    ///
    /// # Errors
    /// - `UnknownChannel` (`vc_id`) for a CLCW of another Virtual Channel.
    /// - `InvalidField` (`frame_data_field`) for an empty FDU, `OutOfRange` (`frame_len`) for
    ///   one longer than [`Fop1Target::max_fdu_len`].
    /// - `InvalidState` for an input whose cell Table 5-1 marks "Not applicable" (COP-005).
    /// - `BufferTooSmall` (detail `Needed`) if `out` is shorter than the frame to transmit
    ///   (API-061).
    ///
    /// An error leaves the state unchanged and produces no action (API-058).
    pub fn step(
        &self,
        input: Fop1Input<'_>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<(Fop1<Q, F, N>, Fop1Actions<N>)> {
        let Some((event, ctx)) = self.classify(input, now)? else {
            return Ok((self.clone(), Actions::new()));
        };
        self.apply(event, &ctx, out)
    }

    /// Runs the cell of `event` in the current state with the parameters of `input` (the
    /// handle, the FDU, the directive, N(R)), without classifying it (COP-005, API-058).
    ///
    /// This is the table function the conformance vectors and the cell checks address for the
    /// cells that no input reaches; [`step`](Self::step) is the same function after the
    /// classification.
    ///
    /// # Errors
    /// As [`step`](Self::step), and `InvalidState` when `input` lacks what the cell of `event`
    /// needs: its directive, its handle, or a Set V(R) directive for E27.
    pub fn step_event(
        &self,
        event: Fop1Event,
        input: Fop1Input<'_>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<(Fop1<Q, F, N>, Fop1Actions<N>)> {
        let mut ctx = Context {
            event,
            now,
            handle: None,
            fdu: &[],
            directive: None,
            n_r: self.nn_r,
        };
        match input {
            Fop1Input::Clcw(clcw) => ctx.n_r = clcw.report_value,
            Fop1Input::RequestAd { handle, fdu } | Fop1Input::RequestBd { handle, fdu } => {
                self.check_fdu(fdu)?;
                ctx.handle = Some(handle);
                ctx.fdu = fdu;
            }
            Fop1Input::Directive { handle, directive } => {
                ctx.handle = Some(handle);
                ctx.directive = Some(directive);
            }
            Fop1Input::InvalidDirective { handle } => ctx.handle = Some(handle),
            Fop1Input::TransmitDone { .. } => ctx.handle = self.bd_out,
            Fop1Input::InvalidClcw | Fop1Input::Timeout => {}
        }
        self.apply(event, &ctx, out)
    }

    /// The next state and the actions of `event` (API-057, API-058).
    fn apply(
        &self,
        event: Fop1Event,
        ctx: &Context<'_>,
        out: &mut [u8],
    ) -> Result<(Fop1<Q, F, N>, Fop1Actions<N>)> {
        let mut next = self.clone();
        let mut actions = Actions::new();
        next.run(event, ctx, out, &mut actions)?;
        Ok((next, actions))
    }

    /// Executes the cell of `event` on `self`, which is a copy of the state before it,
    /// appending its actions to `actions` (232.1 §5.3.1: the actions, then the State
    /// variable; COP-001, COP-003, COP-004).
    fn run(
        &mut self,
        event: Fop1Event,
        ctx: &Context<'_>,
        out: &mut [u8],
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        let prior = self.state;
        let cell = cell(prior, event);
        let Some(next) = cell.next_state else {
            return Err(Error::new(ErrorKind::InvalidState));
        };
        if matches!(
            event,
            Fop1Event::E16 | Fop1Event::E17 | Fop1Event::E18 | Fop1Event::E104
        ) {
            // The timer has expired (§5.1.9.1).
            self.deadline = None;
        }
        let mut used = 0;
        for action in cell.actions {
            self.act(*action, prior, ctx, out, &mut used, actions)?;
        }
        if matches!(event, Fop1Event::E24 | Fop1Event::E25 | Fop1Event::E27) {
            // The directive completes on a later CLCW (S4, S5 with E1) or by Alert.
            self.pending_initiate = ctx.handle;
        }
        self.state = next;
        Ok(())
    }

    /// Executes one action token of a cell (232.1 §5.2).
    #[expect(clippy::too_many_lines, reason = "one arm per action token of §5.2")]
    fn act(
        &mut self,
        action: Fop1Action,
        prior: Fop1State,
        ctx: &Context<'_>,
        out: &mut [u8],
        used: &mut usize,
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        match action {
            Fop1Action::CancelTimer => self.deadline = None,
            Fop1Action::StartTimer => self.start_timer(ctx.now),
            Fop1Action::ReleaseBcCopy => self.remove_sent(|s| matches!(s.kind, SentKind::Bc)),
            Fop1Action::RemoveAckedFrames => self.remove_acked(ctx.n_r, actions)?,
            Fop1Action::LookForFdu => self.look_for_fdu(ctx.now, out, used, actions)?,
            Fop1Action::LookForDirective => self.look_for_directive(out, used, actions)?,
            Fop1Action::InitiateAdRetransmission | Fop1Action::InitiateBcRetransmission => {
                // §5.2.7: Abort, count, timer, then the marks.
                actions.push(Fop1Output::Abort)?;
                self.transmission_count = self.transmission_count.saturating_add(1);
                self.start_timer(ctx.now);
                let bc = matches!(action, Fop1Action::InitiateBcRetransmission);
                for sent in self.sent.iter_mut().flatten() {
                    if matches!(sent.kind, SentKind::Bc) == bc {
                        sent.to_be_retransmitted = true;
                    }
                }
            }
            Fop1Action::AddToWaitQueue => {
                if self.wait.is_some() {
                    return Err(Error::new(ErrorKind::InternalError));
                }
                let mut fdu = [0; F];
                for (d, s) in fdu.iter_mut().zip(ctx.fdu) {
                    *d = *s;
                }
                self.wait = Some(Waiting {
                    handle: ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?,
                    fdu,
                    len: ctx.fdu.len(),
                });
            }
            Fop1Action::TransmitBd => {
                // §5.2.6: `BD_Out_Flag`, then the request; no master copy, no timer.
                let handle = ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?;
                let mut frame = [0; F];
                let len = self.target.encode(
                    FrameType::Bd,
                    FrameSeqNum::new(0),
                    Payload::Fdu(ctx.fdu),
                    &mut frame,
                )?;
                self.bd_out = Some(handle);
                let range = emit(out, used, &frame, len)?;
                actions.push(Fop1Output::Transmit {
                    kind: FrameKind::Bd,
                    sequence_number: FrameSeqNum::new(0),
                    retransmission: false,
                    repetitions: 0,
                    frame: range,
                })?;
            }
            Fop1Action::TransmitBc(command) => {
                self.transmit_bc(command, ctx, out, used, actions)?;
            }
            Fop1Action::Initialize => {
                // §5.2.14.
                self.purge_sent(actions)?;
                self.purge_wait(actions)?;
                self.transmission_count = 1;
                self.suspend_state = SuspendState::NotSuspended;
            }
            Fop1Action::SetVsFromVr => {
                let Some(Directive::InitiateAdWithSetVr { v_r }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.v_s = v_r;
                self.nn_r = v_r;
            }
            Fop1Action::SetVs => {
                // Table 5-1 E35 in S6: only while the AD Service is not suspended.
                let handle = ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?;
                let Some(Directive::SetVs { v_s }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                if matches!(self.suspend_state, SuspendState::NotSuspended) {
                    actions.push(directive(handle, DirectiveNotification::Accept))?;
                    self.v_s = v_s;
                    self.nn_r = v_s;
                    actions.push(directive(handle, DirectiveNotification::PositiveConfirm))?;
                } else {
                    actions.push(directive(handle, DirectiveNotification::Reject))?;
                }
            }
            Fop1Action::SetSlidingWindowWidth => {
                let Some(Directive::SetSlidingWindowWidth { width }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.k = width;
            }
            Fop1Action::SetT1Initial => {
                let Some(Directive::SetT1Initial { duration }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.t1_initial = duration;
            }
            Fop1Action::SetTransmissionLimit => {
                let Some(Directive::SetTransmissionLimit { limit }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.transmission_limit = limit;
            }
            Fop1Action::SetTimeoutType => {
                let Some(Directive::SetTimeoutType { timeout_type }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.timeout_type = timeout_type;
            }
            Fop1Action::SetSuspendState(ss) => self.suspend_state = ss,
            Fop1Action::Suspend => actions.push(Fop1Output::Suspend)?,
            Fop1Action::Resume => {
                // §5.2.17.
                self.start_timer(ctx.now);
                self.suspend_state = SuspendState::NotSuspended;
            }
            Fop1Action::SetOutFlagReady(kind) => match kind {
                FrameKind::Ad => self.ad_out_ready = true,
                FrameKind::Bc => self.bc_out_ready = true,
                FrameKind::Bd => self.bd_out = None,
            },
            Fop1Action::Alert(reason) => self.alert(reason, prior, ctx.event, actions)?,
            Fop1Action::TransferNotify(status) => {
                if let Some(handle) = ctx.handle {
                    actions.push(Fop1Output::Transfer(TransferNotify { handle, status }))?;
                }
            }
            Fop1Action::DirectiveNotify(status) => {
                if is_directive(ctx.event) {
                    let handle = ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?;
                    actions.push(directive(handle, status))?;
                } else if let Some(handle) = self.pending_initiate.take() {
                    // The Confirm of S4 and S5 with E1 completes the Initiate AD directive.
                    actions.push(directive(handle, status))?;
                }
            }
        }
        Ok(())
    }

    /// Starts or restarts the timer with `T1_Initial` (232.1 §5.1.9.1; COP-013).
    const fn start_timer(&mut self, now: Instant) {
        self.deadline = Some(now.saturating_add(self.t1_initial));
    }

    /// Removes the frames for which `remove` holds, keeping the order of the others.
    fn remove_sent(&mut self, remove: impl Fn(&Sent<F>) -> bool) {
        let mut kept = [None; Q];
        for (slot, sent) in kept
            .iter_mut()
            .zip(self.sent.iter().flatten().filter(|s| !remove(s)))
        {
            *slot = Some(*sent);
        }
        self.sent = kept;
    }

    /// Remove Acknowledged Frames from `Sent_Queue` (232.1 §5.2.8): a Positive Confirm for each
    /// Type-AD frame N(R) acknowledges, in queue order, NN(R) := N(R), and
    /// `Transmission_Count` := 1.
    fn remove_acked(&mut self, n_r: FrameSeqNum, actions: &mut Fop1Actions<N>) -> Result<()> {
        let count = usize::from(n_r.get().wrapping_sub(self.nn_r.get()));
        for handle in self
            .sent
            .iter()
            .flatten()
            .take(count)
            .filter_map(Sent::handle)
        {
            actions.push(transfer(handle, TransferNotification::PositiveConfirm))?;
        }
        let mut kept = [None; Q];
        for (slot, sent) in kept.iter_mut().zip(self.sent.iter().flatten().skip(count)) {
            *slot = Some(*sent);
        }
        self.sent = kept;
        self.nn_r = n_r;
        self.transmission_count = 1;
        Ok(())
    }

    /// Purging the `Sent_Queue` (232.1 §5.2.2): a Negative Confirm for each Type-AD frame, in
    /// queue order; the Type-BC frame goes without one. NN(R) := V(S) (OI-60).
    fn purge_sent(&mut self, actions: &mut Fop1Actions<N>) -> Result<()> {
        for handle in self.sent.iter().flatten().filter_map(Sent::handle) {
            actions.push(transfer(handle, TransferNotification::NegativeConfirm))?;
        }
        self.sent = [None; Q];
        self.nn_r = self.v_s;
        Ok(())
    }

    /// Purging the `Wait_Queue` (232.1 §5.2.3): a Reject for the queued FDU.
    fn purge_wait(&mut self, actions: &mut Fop1Actions<N>) -> Result<()> {
        if let Some(waiting) = self.wait.take() {
            actions.push(transfer(waiting.handle, TransferNotification::Reject))?;
        }
        Ok(())
    }

    /// Alert (232.1 §5.2.15): cancel the timer, purge both queues, complete a pending
    /// Initiate AD directive with a Negative Confirm, and notify (API-083).
    fn alert(
        &mut self,
        reason: AlertReason,
        prior: Fop1State,
        event: Fop1Event,
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        self.deadline = None;
        self.purge_sent(actions)?;
        self.purge_wait(actions)?;
        if let Some(handle) = self.pending_initiate.take() {
            actions.push(directive(handle, DirectiveNotification::NegativeConfirm))?;
        }
        actions.push(Fop1Output::Alert(Alert {
            reason,
            event,
            prior_state: prior,
        }))?;
        Ok(())
    }

    /// Look for FDU (232.1 §5.2.10): with `AD_Out_Flag` Ready, the first Type-AD frame marked
    /// `To_Be_Retransmitted`, else the FDU of the `Wait_Queue` if V(S) < NN(R) + K.
    fn look_for_fdu(
        &mut self,
        now: Instant,
        out: &mut [u8],
        used: &mut usize,
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        if !self.ad_out_ready {
            return Ok(());
        }
        let repetitions = self.ad_repetitions;
        if let Some(sent) = self
            .sent
            .iter_mut()
            .flatten()
            .find(|s| matches!(s.kind, SentKind::Ad(_)) && s.to_be_retransmitted)
        {
            sent.to_be_retransmitted = false;
            let range = emit(out, used, &sent.frame, sent.len)?;
            let sequence_number = sent.sequence_number;
            self.ad_out_ready = false;
            actions.push(Fop1Output::Transmit {
                kind: FrameKind::Ad,
                sequence_number,
                retransmission: true,
                repetitions,
                frame: range,
            })?;
            return Ok(());
        }
        let outstanding = self.v_s.get().wrapping_sub(self.nn_r.get());
        if outstanding >= self.k {
            return Ok(());
        }
        let Some(waiting) = self.wait.take() else {
            return Ok(());
        };
        actions.push(transfer(waiting.handle, TransferNotification::Accept))?;
        self.transmit_ad(&waiting, now, out, used, actions)
    }

    /// Transmit Type-AD Frame (232.1 §5.2.4): N(S) := V(S), V(S) + 1, the master copy on the
    /// `Sent_Queue`, `Transmission_Count` := 1 if the queue was empty, the timer, `AD_Out_Flag`,
    /// and the request.
    fn transmit_ad(
        &mut self,
        waiting: &Waiting<F>,
        now: Instant,
        out: &mut [u8],
        used: &mut usize,
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        let sequence_number = self.v_s;
        let fdu = waiting
            .fdu
            .get(..waiting.len)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let mut frame = [0; F];
        let len = self.target.encode(
            FrameType::Ad,
            sequence_number,
            Payload::Fdu(fdu),
            &mut frame,
        )?;
        let sent = Sent {
            frame,
            len,
            sequence_number,
            kind: SentKind::Ad(waiting.handle),
            to_be_retransmitted: false,
        };
        self.push_sent(sent)?;
        self.v_s = self.v_s.next();
        self.start_timer(now);
        self.ad_out_ready = false;
        let range = emit(out, used, &sent.frame, sent.len)?;
        actions.push(Fop1Output::Transmit {
            kind: FrameKind::Ad,
            sequence_number,
            retransmission: false,
            repetitions: self.ad_repetitions,
            frame: range,
        })
    }

    /// Transmit Type-BC Frame (232.1 §5.2.5): the master copy on the `Sent_Queue`,
    /// `Transmission_Count` := 1, the timer, `BC_Out_Flag`, and the request. The frame carries
    /// Unlock, or Set V(R) with the V*(R) of the directive, which E27 has just put in V(S)
    /// (232.0 §4.1.3.3).
    fn transmit_bc(
        &mut self,
        command: BcCommand,
        ctx: &Context<'_>,
        out: &mut [u8],
        used: &mut usize,
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        let command = match command {
            BcCommand::Unlock => ControlCommand::Unlock,
            // The one cell that lists it (E27) sets V(S) := V*(R) first.
            BcCommand::SetVr => ControlCommand::SetVr(self.v_s),
        };
        let sequence_number = FrameSeqNum::new(0);
        let mut frame = [0; F];
        let len = self.target.encode(
            FrameType::Bc,
            sequence_number,
            Payload::Control(command),
            &mut frame,
        )?;
        let sent = Sent {
            frame,
            len,
            sequence_number,
            kind: SentKind::Bc,
            to_be_retransmitted: false,
        };
        self.push_sent(sent)?;
        self.transmission_count = 1;
        self.start_timer(ctx.now);
        self.bc_out_ready = false;
        let range = emit(out, used, &sent.frame, sent.len)?;
        actions.push(Fop1Output::Transmit {
            kind: FrameKind::Bc,
            sequence_number,
            retransmission: false,
            repetitions: self.bc_repetitions,
            frame: range,
        })
    }

    /// Look for Directive (232.1 §5.2.9): with `BC_Out_Flag` Ready, the Type-BC frame if it is
    /// marked `To_Be_Retransmitted`.
    fn look_for_directive(
        &mut self,
        out: &mut [u8],
        used: &mut usize,
        actions: &mut Fop1Actions<N>,
    ) -> Result<()> {
        if !self.bc_out_ready {
            return Ok(());
        }
        let repetitions = self.bc_repetitions;
        if let Some(sent) = self
            .sent
            .iter_mut()
            .flatten()
            .find(|s| matches!(s.kind, SentKind::Bc) && s.to_be_retransmitted)
        {
            sent.to_be_retransmitted = false;
            let range = emit(out, used, &sent.frame, sent.len)?;
            let sequence_number = sent.sequence_number;
            self.bc_out_ready = false;
            actions.push(Fop1Output::Transmit {
                kind: FrameKind::Bc,
                sequence_number,
                retransmission: true,
                repetitions,
                frame: range,
            })?;
        }
        Ok(())
    }

    /// Appends `sent` to the `Sent_Queue`.
    ///
    /// # Errors
    /// `InternalError` if the queue is full: K ≤ `Q` keeps a free entry for every frame the
    /// window admits (COP-015), so this does not happen.
    fn push_sent(&mut self, sent: Sent<F>) -> Result<()> {
        let slot = self
            .sent
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        *slot = Some(sent);
        Ok(())
    }
}

/// Whether `event` is a directive event, E23 to E40 (232.1 Table 5-1).
const fn is_directive(event: Fop1Event) -> bool {
    matches!(
        event,
        Fop1Event::E23
            | Fop1Event::E24
            | Fop1Event::E25
            | Fop1Event::E26
            | Fop1Event::E27
            | Fop1Event::E28
            | Fop1Event::E29
            | Fop1Event::E30
            | Fop1Event::E31
            | Fop1Event::E32
            | Fop1Event::E33
            | Fop1Event::E34
            | Fop1Event::E35
            | Fop1Event::E36
            | Fop1Event::E37
            | Fop1Event::E38
            | Fop1Event::E39
            | Fop1Event::E40
    )
}

/// The transfer notification `status` for `handle`.
const fn transfer(handle: Handle, status: TransferNotification) -> Fop1Output {
    Fop1Output::Transfer(TransferNotify { handle, status })
}

/// The directive notification `status` for `handle`.
const fn directive(handle: Handle, status: DirectiveNotification) -> Fop1Output {
    Fop1Output::Directive(DirectiveNotify { handle, status })
}

/// Copies the first `len` octets of `frame` into `out` after the `used` octets and returns
/// their range (API-061).
///
/// # Errors
/// `BufferTooSmall` (detail `Needed`) if `out` is too short.
fn emit(out: &mut [u8], used: &mut usize, frame: &[u8], len: usize) -> Result<OutRange> {
    let start = *used;
    let end = start.saturating_add(len);
    let area = out
        .get_mut(start..end)
        .ok_or(Error::buffer_too_small(end))?;
    for (d, s) in area.iter_mut().zip(frame) {
        *d = *s;
    }
    *used = end;
    Ok(OutRange::new(start, len))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "tests name the engines of a scenario by their state, and the sweep is one loop nest"
)]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use super::{
        Context, Fop1, Fop1Actions, Fop1Channel, Fop1Config, Fop1Input, Fop1Output, Fop1Target,
        Sent, SentKind, cell, fop1_max_actions,
    };
    use crate::fop1_table::{EVENT_COUNT, FOP1_TABLE, Fop1Action, STATE_COUNT};
    use crate::fop1_table::{Fop1Event, Fop1State, FrameKind, SuspendState};
    use crate::notify::{
        Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, TimeoutType,
        TransferNotification, TransferNotify,
    };
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::time::{Duration, Instant};
    use aldis_core::{ErrorKind, field};
    use aldis_tc::{Clcw, ClcwStatus, CopInEffect, FarmBCounter, TcSpacecraftId, TcVcId};
    use core::hint::black_box;
    use core::num::NonZeroU8;

    const Q: usize = 4;
    const F: usize = 64;
    const N: usize = fop1_max_actions(Q);
    type Engine = Fop1<Q, F, N>;
    const FDU: [u8; 2] = [0xA0, 0x01];

    fn target() -> Fop1Target {
        Fop1Target::tc(
            TcSpacecraftId::new(42).unwrap(),
            TcVcId::new(1).unwrap(),
            F,
            true,
        )
        .unwrap()
    }

    fn config(tt: TimeoutType, limit: u8) -> Fop1Config {
        Fop1Config {
            target: target(),
            t1_initial: Duration::from_micros(100),
            transmission_limit: NonZeroU8::new(limit).unwrap(),
            timeout_type: tt,
            sliding_window_k: 2,
            ad_repetitions: 3,
            bc_repetitions: 4,
        }
    }

    fn fresh() -> Engine {
        Engine::new(config(TimeoutType::Tt0, 2)).unwrap()
    }

    fn clcw(lockout: bool, wait: bool, retransmit: bool, n_r: u8) -> Fop1Input<'static> {
        Fop1Input::Clcw(Clcw {
            status: ClcwStatus::new(0).unwrap(),
            cop_in_effect: CopInEffect::COP_1,
            vc_id: TcVcId::new(1).unwrap(),
            no_rf_available: false,
            no_bit_lock: false,
            lockout,
            wait,
            retransmit,
            farm_b_counter: FarmBCounter::default(),
            report_value: FrameSeqNum::new(n_r),
        })
    }

    fn directive(handle: u32, directive: Directive) -> Fop1Input<'static> {
        Fop1Input::Directive { handle, directive }
    }

    fn go(f: &Engine, input: Fop1Input<'_>, at: u64) -> (Engine, Vec<Fop1Output>) {
        let mut out = [0u8; F];
        let (next, actions) = f.step(input, Instant::from_micros(at), &mut out).unwrap();
        (next, actions.iter().copied().collect())
    }

    fn inject(f: &Engine, event: Fop1Event, input: Fop1Input<'_>) -> Result<Engine, ErrorKind> {
        let mut out = [0u8; F];
        f.step_event(event, input, Instant::from_micros(0), &mut out)
            .map(|(next, _)| next)
            .map_err(|e| e.kind())
    }

    fn active() -> Engine {
        go(
            &fresh(),
            directive(1, Directive::InitiateAdWithoutClcwCheck),
            0,
        )
        .0
    }

    #[test]
    fn queries_and_construction() {
        // req: API-056, COP-006, COP-015, API-040
        assert_eq!(fop1_max_actions(black_box(Q)), N);
        let t = target();
        let plain = Fop1Target::tc(
            TcSpacecraftId::new(42).unwrap(),
            TcVcId::new(1).unwrap(),
            F,
            false,
        )
        .unwrap();
        assert_eq!(plain.max_fdu_len(), F - 5);
        assert_eq!(
            t.channel(),
            Fop1Channel::Tc {
                spacecraft_id: TcSpacecraftId::new(42).unwrap(),
                vc_id: TcVcId::new(1).unwrap(),
            }
        );
        assert_eq!((t.max_frame_len(), t.max_fdu_len()), (F, F - 7));
        let f = fresh();
        assert_eq!(f.target(), &t);
        assert_eq!(
            (f.state(), f.v_s().get(), f.nn_r().get()),
            (Fop1State::Initial, 0, 0)
        );
        assert_eq!(f.sliding_window_k(), 2);
        assert_eq!(f.t1_initial(), Duration::from_micros(100));
        assert_eq!(f.transmission_limit().get(), 2);
        assert_eq!(f.timeout_type(), TimeoutType::Tt0);
        assert_eq!(f.transmission_count(), 1);
        assert_eq!(f.suspend_state(), SuspendState::NotSuspended);
        assert_eq!((f.sent_queue_len(), f.wait_queue_occupied()), (0, false));
        assert!(f.out_flag_ready(FrameKind::Ad) && f.out_flag_ready(FrameKind::Bc));
        assert!(f.out_flag_ready(FrameKind::Bd) && f.next_deadline().is_none());
        let bad = |c: Fop1Config| Engine::new(c).unwrap_err().field();
        let k = |k| Fop1Config {
            sliding_window_k: k,
            ..config(TimeoutType::Tt0, 1)
        };
        assert_eq!(bad(k(0)), Some(field::SLIDING_WINDOW_K));
        assert_eq!(bad(k(5)), Some(field::SLIDING_WINDOW_K));
        let t1 = Fop1Config {
            t1_initial: Duration::from_micros(0),
            ..config(TimeoutType::Tt0, 1)
        };
        assert_eq!(bad(t1), Some(field::T1_INITIAL));
        let big = Fop1Config {
            target: Fop1Target::tc(
                TcSpacecraftId::new(42).unwrap(),
                TcVcId::new(1).unwrap(),
                100,
                false,
            )
            .unwrap(),
            ..config(TimeoutType::Tt0, 1)
        };
        assert_eq!(bad(big), Some(field::MAX_FRAME_LEN));
        assert!(
            Fop1Target::tc(
                TcSpacecraftId::new(1).unwrap(),
                TcVcId::new(1).unwrap(),
                2000,
                true
            )
            .is_err()
        );
    }

    #[test]
    fn fdu_and_area_checks() {
        // req: API-058, API-061, CMN-041, TC-007
        let f = active();
        let mut out = [0u8; F];
        let now = Instant::from_micros(1);
        let empty = Fop1Input::RequestAd {
            handle: 2,
            fdu: &[],
        };
        assert_eq!(
            f.step(empty, now, &mut out).unwrap_err().kind(),
            ErrorKind::InvalidField
        );
        let long = [0u8; F];
        let long = Fop1Input::RequestBd {
            handle: 2,
            fdu: &long,
        };
        assert_eq!(
            f.step(long, now, &mut out).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            f.step_event(Fop1Event::E21, long, now, &mut out)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        let request = Fop1Input::RequestAd {
            handle: 2,
            fdu: &FDU,
        };
        let mut tiny = [0u8; 4];
        assert_eq!(
            f.step(request, now, &mut tiny).unwrap_err().kind(),
            ErrorKind::BufferTooSmall
        );
        // A Timeout with no timer, or before the deadline, changes nothing (API-053).
        assert_eq!(go(&f, Fop1Input::Timeout, 5), (f.clone(), vec![]));
        let (g, _) = go(&f, request, 1);
        assert_eq!(go(&g, Fop1Input::Timeout, 50), (g.clone(), vec![]));
        assert!(!g.out_flag_ready(FrameKind::Ad));
        assert!(!g.wait_queue_occupied() && g.sent_queue_len() == 1);
    }

    #[test]
    fn injected_cells_take_their_parameters_from_the_input() {
        // req: COP-005, API-058
        let f = fresh();
        let a = active();
        // Not applicable.
        assert_eq!(
            inject(&a, Fop1Event::E31, Fop1Input::Timeout),
            Err(ErrorKind::InvalidState)
        );
        // Inputs without what the cell needs: an event/input mismatch is InvalidState.
        assert_eq!(
            inject(&a, Fop1Event::E19, Fop1Input::Timeout),
            Err(ErrorKind::InvalidState)
        );
        assert_eq!(
            inject(&a, Fop1Event::E21, Fop1Input::Timeout),
            Err(ErrorKind::InvalidState)
        );
        assert_eq!(
            inject(&f, Fop1Event::E35, Fop1Input::Timeout),
            Err(ErrorKind::InvalidState)
        );
        assert_eq!(
            inject(&f, Fop1Event::E27, Fop1Input::InvalidClcw),
            Err(ErrorKind::InvalidState)
        );
        // A second FDU for a full Wait_Queue.
        let w = Fop1Input::RequestAd {
            handle: 2,
            fdu: &FDU,
        };
        let full = inject(&a, Fop1Event::E19, w).unwrap();
        let full = inject(&full, Fop1Event::E19, w).unwrap();
        assert_eq!(
            inject(&full, Fop1Event::E19, w),
            Err(ErrorKind::InternalError)
        );
        // A directive cell injected with another directive, or with no directive, fails
        // rather than confirm a directive it did not apply.
        let other = directive(9, Directive::TerminateAd);
        for e in [
            Fop1Event::E35,
            Fop1Event::E36,
            Fop1Event::E37,
            Fop1Event::E38,
            Fop1Event::E39,
        ] {
            assert_eq!(inject(&f, e, other), Err(ErrorKind::InvalidState));
        }
        // The same cells with their own directive apply it.
        let k = inject(
            &f,
            Fop1Event::E36,
            directive(9, Directive::SetSlidingWindowWidth { width: 1 }),
        )
        .unwrap();
        let limit = NonZeroU8::new(3).unwrap();
        let l = inject(
            &k,
            Fop1Event::E38,
            directive(9, Directive::SetTransmissionLimit { limit }),
        )
        .unwrap();
        let t = inject(
            &l,
            Fop1Event::E39,
            directive(
                9,
                Directive::SetTimeoutType {
                    timeout_type: TimeoutType::Tt1,
                },
            ),
        )
        .unwrap();
        assert_eq!(
            (
                t.sliding_window_k(),
                t.transmission_limit(),
                t.timeout_type()
            ),
            (1, limit, TimeoutType::Tt1)
        );
        let unlock = directive(9, Directive::InitiateAdWithUnlock);
        assert_eq!(
            inject(&f, Fop1Event::E27, unlock),
            Err(ErrorKind::InvalidState)
        );
        assert_eq!(
            inject(&f, Fop1Event::E29, Fop1Input::InvalidClcw),
            Err(ErrorKind::InvalidState)
        );
        // E27 without Set V(R) still initializes, with V(S) unchanged.
        assert_eq!(
            inject(&a, Fop1Event::E2, clcw(false, false, false, 0))
                .unwrap()
                .state(),
            Fop1State::Active
        );
        let i = inject(
            &f,
            Fop1Event::E40,
            Fop1Input::InvalidDirective { handle: 3 },
        )
        .unwrap();
        assert_eq!(i, f);
        let d = inject(
            &a,
            Fop1Event::E45,
            Fop1Input::TransmitDone {
                kind: FrameKind::Bd,
                accepted: true,
            },
        )
        .unwrap();
        assert!(d.out_flag_ready(FrameKind::Bd));
        // A directive cell without a directive to answer fails.
        assert_eq!(
            inject(&f, Fop1Event::E24, Fop1Input::Timeout),
            Err(ErrorKind::InvalidState)
        );
        let set = directive(
            4,
            Directive::SetT1Initial {
                duration: Duration::from_micros(5),
            },
        );
        assert_eq!(
            inject(&f, Fop1Event::E37, set)
                .unwrap()
                .t1_initial()
                .as_micros(),
            5
        );
    }

    #[test]
    fn classification_of_directives_and_responses() {
        // req: COP-014, COP-015, API-055
        let f = fresh();
        let reject = |h| {
            vec![Fop1Output::Directive(DirectiveNotify {
                handle: h,
                status: DirectiveNotification::Reject,
            })]
        };
        for d in [
            Directive::SetSlidingWindowWidth { width: 0 },
            Directive::SetSlidingWindowWidth { width: 5 },
            Directive::SetT1Initial {
                duration: Duration::from_micros(0),
            },
        ] {
            assert_eq!(go(&f, directive(7, d), 0).1, reject(7));
        }
        let (s5, _) = go(&f, directive(1, Directive::InitiateAdWithUnlock), 0);
        assert_eq!(s5.state(), Fop1State::InitializingWithBcFrame);
        // E26, E28: BC_Out_Flag Not_Ready.
        let (t, _) = go(&s5, directive(2, Directive::TerminateAd), 1);
        assert_eq!(
            go(&t, directive(3, Directive::InitiateAdWithUnlock), 2).1,
            reject(3)
        );
        let setvr = directive(
            4,
            Directive::InitiateAdWithSetVr {
                v_r: FrameSeqNum::new(9),
            },
        );
        assert_eq!(go(&t, setvr, 2).1, reject(4));
        let done = |k, a| Fop1Input::TransmitDone {
            kind: k,
            accepted: a,
        };
        let (t, _) = go(&t, done(FrameKind::Bc, true), 3);
        let (s5, _) = go(&t, setvr, 4);
        assert_eq!(s5.v_s().get(), 9);
        // E44: BC_Reject.
        let (g, out) = go(&s5, done(FrameKind::Bc, false), 5);
        assert_eq!(g.state(), Fop1State::Initial);
        assert!(matches!(out.last(), Some(Fop1Output::Alert(a)) if a.reason == AlertReason::Llif));
        // E46: BD_Reject; the CLCW of a word that is not a CLCW (E15) in S1.
        let a = active();
        let (b, _) = go(
            &a,
            Fop1Input::RequestBd {
                handle: 5,
                fdu: &FDU,
            },
            1,
        );
        let (_, out) = go(&b, done(FrameKind::Bd, false), 2);
        assert!(matches!(out.last(), Some(Fop1Output::Alert(a)) if a.reason == AlertReason::Llif));
        let (g, _) = go(&a, Fop1Input::InvalidClcw, 1);
        assert_eq!(g.state(), Fop1State::Initial);
        let mut other = Clcw::decode([0, 0, 0, 0]).unwrap();
        other.cop_in_effect = CopInEffect::COP_1;
        let mut out = [0u8; F];
        assert_eq!(
            a.step(Fop1Input::Clcw(other), Instant::from_micros(1), &mut out)
                .unwrap_err()
                .kind(),
            ErrorKind::UnknownChannel
        );
    }

    #[test]
    fn transmit_actions_carry_the_configured_repetitions() {
        // req: COP-016
        let reps = |out: &[Fop1Output]| -> Vec<(FrameKind, u8)> {
            out.iter()
                .filter_map(|o| match *o {
                    Fop1Output::Transmit {
                        kind, repetitions, ..
                    } => Some((kind, repetitions)),
                    _ => None,
                })
                .collect()
        };
        let a = active();
        let ad = Fop1Input::RequestAd {
            handle: 2,
            fdu: &FDU,
        };
        // One Transmit action per frame: the engine annotates, never duplicates.
        assert_eq!(reps(&go(&a, ad, 1).1), vec![(FrameKind::Ad, 3)]);
        let bd = Fop1Input::RequestBd {
            handle: 3,
            fdu: &FDU,
        };
        assert_eq!(reps(&go(&a, bd, 1).1), vec![(FrameKind::Bd, 0)]);
        let unlock = directive(4, Directive::InitiateAdWithUnlock);
        assert_eq!(reps(&go(&fresh(), unlock, 1).1), vec![(FrameKind::Bc, 4)]);
    }

    #[test]
    fn suspension_set_vs_and_alerts() {
        // req: COP-013, COP-014, API-083
        let f = Engine::new(config(TimeoutType::Tt1, 1)).unwrap();
        let (a, _) = go(&f, directive(1, Directive::InitiateAdWithoutClcwCheck), 0);
        let (a, _) = go(
            &a,
            Fop1Input::RequestAd {
                handle: 2,
                fdu: &FDU,
            },
            1,
        );
        let (s, out) = go(&a, Fop1Input::Timeout, 101);
        assert_eq!(
            (s.state(), out),
            (Fop1State::Initial, vec![Fop1Output::Suspend])
        );
        assert_eq!(s.suspend_state(), SuspendState::Active);
        // Set V(S) while suspended is rejected.
        let setvs = directive(
            3,
            Directive::SetVs {
                v_s: FrameSeqNum::new(7),
            },
        );
        assert_eq!(
            go(&s, setvs, 102).1,
            vec![Fop1Output::Directive(DirectiveNotify {
                handle: 3,
                status: DirectiveNotification::Reject
            })]
        );
        // An LLIF Alert in S6, Resume, then a Lockout CLCW: its Alert is returned.
        let (l, out) = go(
            &s,
            Fop1Input::TransmitDone {
                kind: FrameKind::Ad,
                accepted: false,
            },
            103,
        );
        assert_eq!(
            out.first(),
            Some(&Fop1Output::Transfer(TransferNotify {
                handle: 2,
                status: TransferNotification::NegativeConfirm
            }))
        );
        let (r, _) = go(&l, directive(4, Directive::ResumeAd), 104);
        assert_eq!(
            (r.state(), r.next_deadline()),
            (Fop1State::Active, Some(Instant::from_micros(204)))
        );
        let (g, out) = go(&r, clcw(true, false, false, 1), 105);
        assert_eq!(
            (g.state(), out),
            (
                Fop1State::Initial,
                vec![Fop1Output::Alert(Alert {
                    reason: AlertReason::Lockout,
                    event: Fop1Event::E14,
                    prior_state: Fop1State::Active,
                })]
            )
        );
    }

    /// Every state the sweep starts from: S6, S1 with frames outstanding and an FDU
    /// waiting, S2, S3, S4, S5, and a S5 that also holds a Type-AD frame.
    fn bases() -> Vec<Engine> {
        let a = active();
        let (a1, _) = go(
            &a,
            Fop1Input::RequestAd {
                handle: 2,
                fdu: &FDU,
            },
            1,
        );
        let done = Fop1Input::TransmitDone {
            kind: FrameKind::Ad,
            accepted: true,
        };
        let (a1, _) = go(&a1, done, 2);
        let (a2, _) = go(
            &a1,
            Fop1Input::RequestAd {
                handle: 3,
                fdu: &FDU,
            },
            3,
        );
        let (a2, _) = go(
            &a2,
            Fop1Input::RequestAd {
                handle: 4,
                fdu: &FDU,
            },
            4,
        );
        let (a2, _) = go(
            &a2,
            Fop1Input::RequestBd {
                handle: 5,
                fdu: &FDU,
            },
            5,
        );
        let (s2, _) = go(&a2, clcw(false, false, true, 0), 6);
        let (s3, _) = go(&a2, clcw(false, true, true, 1), 6);
        let f = fresh();
        let (s4, _) = go(&f, directive(6, Directive::InitiateAdWithClcwCheck), 0);
        let (s5, _) = go(&f, directive(7, Directive::InitiateAdWithUnlock), 0);
        let mut mixed = s5.clone();
        let ad = a2.sent.iter().flatten().next().copied().unwrap();
        let mut slots = mixed.sent.iter_mut().filter(|s| s.is_none());
        *slots.next().unwrap() = Some(Sent {
            kind: SentKind::Ad(9),
            ..ad
        });
        // S5 with the Type-BC frame marked for retransmission and BC_Out_Flag Ready.
        let (s5r, _) = go(
            &s5,
            Fop1Input::TransmitDone {
                kind: FrameKind::Bc,
                accepted: true,
            },
            1,
        );
        let (s5r, _) = go(&s5r, Fop1Input::Timeout, 100);
        let mut s5r = s5r;
        s5r.bc_out_ready = true;
        // A Sent_Queue full beyond what K admits: a broken invariant (InternalError).
        let mut jammed = a2.clone();
        jammed.sent.fill(Some(ad));
        jammed.ad_out_ready = true;
        jammed.nn_r = jammed.v_s;
        let mut suspended = fresh();
        suspended.suspend_state = SuspendState::RetransmitWithWait;
        vec![f, a, a1, a2, s2, s3, s4, s5, mixed, s5r, jammed, suspended]
    }

    fn inputs() -> Vec<Fop1Input<'static>> {
        vec![
            Fop1Input::Timeout,
            clcw(false, false, false, 1),
            Fop1Input::RequestAd {
                handle: 11,
                fdu: &FDU,
            },
            directive(
                12,
                Directive::InitiateAdWithSetVr {
                    v_r: FrameSeqNum::new(3),
                },
            ),
            directive(
                13,
                Directive::SetVs {
                    v_s: FrameSeqNum::new(3),
                },
            ),
            Fop1Input::TransmitDone {
                kind: FrameKind::Bd,
                accepted: true,
            },
        ]
    }

    /// The Alert reasons in `list`, in order.
    fn alerts(list: &Fop1Actions<N>) -> Vec<AlertReason> {
        list.iter()
            .filter_map(|o| match *o {
                Fop1Output::Alert(a) => Some(a.reason),
                _ => None,
            })
            .collect()
    }

    /// The Alert reasons the cell of `state` and `event` lists, in order.
    fn listed_alerts(state: Fop1State, event: Fop1Event) -> Vec<AlertReason> {
        cell(state, event)
            .actions
            .iter()
            .filter_map(|a| match *a {
                Fop1Action::Alert(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn no_alert_follows_an_alert_before_an_initiate_ad() {
        // req: COP-017
        // Every cell that lists Alert goes to S6, and no cell of S6 lists an Alert other than
        // Alert[LLIF], so after an Alert only an Initiate AD (E23 to E28) leaves S6.
        for row in &FOP1_TABLE {
            for c in row {
                if c.actions.iter().any(|a| matches!(a, Fop1Action::Alert(_))) {
                    assert_eq!(c.next_state, Some(Fop1State::Initial));
                }
            }
        }
        let [.., s6] = &FOP1_TABLE;
        for c in s6 {
            for a in c.actions {
                if let Fop1Action::Alert(r) = a {
                    assert_eq!(*r, AlertReason::Llif);
                }
            }
            if let Some(next) = c.next_state
                && next != Fop1State::Initial
            {
                assert!(
                    c.actions
                        .iter()
                        .any(|a| matches!(a, Fop1Action::Initialize | Fop1Action::Resume))
                );
            }
        }
    }

    #[test]
    fn every_failure_of_a_cell_leaves_the_state_unchanged() {
        // req: API-058, API-061, API-083, COP-006, COP-017
        let events = {
            use Fop1Event as E;
            [
                E::E1,
                E::E2,
                E::E3,
                E::E4,
                E::E5,
                E::E6,
                E::E7,
                E::E8,
                E::E9,
                E::E10,
                E::E11,
                E::E12,
                E::E13,
                E::E14,
                E::E15,
                E::E16,
                E::E17,
                E::E18,
                E::E19,
                E::E20,
                E::E21,
                E::E22,
                E::E23,
                E::E24,
                E::E25,
                E::E26,
                E::E27,
                E::E28,
                E::E29,
                E::E30,
                E::E31,
                E::E32,
                E::E33,
                E::E34,
                E::E35,
                E::E36,
                E::E37,
                E::E38,
                E::E39,
                E::E40,
                E::E41,
                E::E42,
                E::E43,
                E::E44,
                E::E45,
                E::E46,
                E::E101,
                E::E102,
                E::E103,
                E::E104,
            ]
        };
        assert_eq!((events.len(), STATE_COUNT), (EVENT_COUNT, 6));
        for base in bases() {
            for state in [
                Fop1State::Active,
                Fop1State::RetransmitWithoutWait,
                Fop1State::RetransmitWithWait,
                Fop1State::InitializingWithoutBcFrame,
                Fop1State::InitializingWithBcFrame,
                Fop1State::Initial,
            ] {
                let mut start = base.clone();
                start.state = state;
                for event in events {
                    for input in inputs() {
                        let mut ctx = Context {
                            event,
                            now: Instant::from_micros(10),
                            handle: Some(20),
                            fdu: &FDU,
                            directive: None,
                            n_r: start.nn_r,
                        };
                        match input {
                            Fop1Input::Directive { directive, .. } => {
                                ctx.directive = Some(directive);
                            }
                            Fop1Input::Clcw(c) => ctx.n_r = c.report_value,
                            _ => {}
                        }
                        // Lists with every amount of room: each push fails somewhere.
                        for room in 0..=N {
                            let mut list: Fop1Actions<N> = Fop1Actions::new();
                            for _ in room..N {
                                list.push(Fop1Output::Abort).unwrap();
                            }
                            let mut big = [0u8; F];
                            let mut a = start.clone();
                            // A push without room fails the cell; none is dropped (API-083).
                            if a.run(event, &ctx, &mut big, &mut list).is_ok() {
                                assert!(list.len() <= N);
                                assert_eq!(alerts(&list), listed_alerts(state, event));
                            }
                        }
                        // No output area: every transmit fails.
                        let mut b = start.clone();
                        let mut empty = Fop1Actions::new();
                        if b.run(event, &ctx, &mut [], &mut empty).is_ok() {
                            assert!(
                                empty
                                    .iter()
                                    .all(|o| !matches!(o, Fop1Output::Transmit { .. }))
                            );
                        }
                    }
                }
            }
        }
    }
}
