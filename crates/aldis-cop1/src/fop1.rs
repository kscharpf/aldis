//! FOP-1 engine (CCSDS 232.1-B-2 §5; COP-001, COP-004, COP-006, COP-010…COP-017).
//!
//! [`Fop1`] is plain data: the State, V(S), NN(R), the `Sent_Queue` entries, the `Wait_Queue`
//! entry, the three Out flags, the timer T1, the `Transmission_Count`, `Suspend_State`, and the
//! managed parameters (232.1 §5.1, §7.2), in its [`Fop1Head`], and the octets of the master
//! copies of the Type-AD and Type-BC frames and of the queued FDU, in the [`Fop1Store`] it owns.
//! The step of API-057 is the pair [`Fop1::decide`] and [`Fop1Decision::commit`] (API-058,
//! RS-022), because copying the master copies for every input is too costly (OI-45): `decide`
//! reads the engine (the head and the store), one [`Fop1Input`], and `now`, and returns a
//! [`Fop1Decision`] with the [`Fop1Actions`] of the cell of Table 5-1 ([`FOP1_TABLE`]), in the
//! order the cell lists them, and the change to the head. The decision holds the exclusive
//! borrow of the engine until it is committed or dropped, so the engine takes no other input in
//! between, and the decision can be committed to no other engine; `commit` applies it in place
//! and cannot fail. The engine owns its store, so no `decide` or `commit` takes a store of
//! another engine (RS-021). The transition reads the table with an exhaustive `match` and no
//! wildcard arm (COP-003, RS-021); an error of `decide` leaves the state unchanged, and so does
//! a decision that is dropped (API-058).
//!
//! # Events and inputs
//!
//! | Event | Input |
//! |---|---|
//! | E1–E14, E101–E103 | [`Fop1Input::Clcw`], classified after the invalid-pattern check (E15) by the Lockout Flag, then N(R) against NN(R) and V(S) modulo 256, then the Retransmit and Wait Flags (232.1 §5.3.2.1; COP-012) |
//! | E15 | [`Fop1Input::Clcw`] with a COP in Effect other than `01`, and [`Fop1Input::InvalidClcw`] (232.1 §5.3.2.1 a; OI-62, which ranks it first) |
//! | E16–E18, E104 | [`Fop1Input::Timeout`] once `now` reaches [`Fop1::next_deadline`] (API-053) |
//! | E19–E22 | [`Fop1Input::RequestAd`], [`Fop1Input::RequestBd`] |
//! | E23–E39 | [`Fop1Input::Directive`] with a valid value |
//! | E40 | [`Fop1Input::Directive`] with a value outside its range (COP-015), and [`Fop1Input::InvalidDirective`] |
//! | E41–E46 | [`Fop1Input::TransmitDone`] |
//!
//! A CLCW for another Virtual Channel is refused with `UnknownChannel` and changes nothing: it is
//! a routing error, not a COP-1 event (cop1.md §2.2). [`Fop1::decide_event`] decides the cell of a
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
//!   where it lists Initiate AD (or BC) Retransmission (§5.2.7 c). The one exception is S3,
//!   where the cells of E16 and E104 say "Ignore": T1 is re-armed for another `T1_Initial`
//!   without a change of the `Transmission_Count`, so that a lost frame is still discovered
//!   once the Wait Flag clears (§5.1.9.1; COP-013; OI-67). The table data keeps the literal cell.
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

use aldis_core::format::FrameFormat;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::{Duration, Instant};
use aldis_core::{Error, ErrorDetail, ErrorKind, Result, field};
use aldis_tc::{
    Clcw, ControlCommand, FrameType, PhysicalChannelConfig, SegmentHeaderUse, TcFormat,
    TcSpacecraftId, TcVcId, VcConfig, VcTable,
};
#[cfg(feature = "uslp")]
use aldis_uslp::{
    MapConfig, PhysicalChannelConfig as UslpPhysical, SecurityScope, TfdfHeader, TfdzRule,
    TransferFrameType, Upid, UslpFormat, VcConfig as UslpVcConfig, VcTable as UslpVcTable,
};
use core::marker::PhantomData;
use core::num::NonZeroU8;

use crate::fop1_table::{
    EVENT_COUNT, FOP1_TABLE, Fop1Action, Fop1Cell, Fop1Event, Fop1State, FrameKind, STATE_COUNT,
    SuspendState,
};
use crate::notify::{Alert, Directive, DirectiveNotify, TimeoutType, TransferNotify};
use aldis_core::engine::{Actions, OutRange};

mod act;
mod classify;
mod commit;
pub mod protocol;

#[cfg(feature = "uslp")]
pub use protocol::UslpChannel;
use protocol::max_fdu_len;
pub use protocol::{Fop1Format, TcChannel};

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

/// The most actions one [`Fop1::decide`] can produce with a `Sent_Queue` of `sent_queue` frames,
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
/// (732.1 §4.1). `P` is the codec's [`Fop1Format`]; its frame format gives every length and its
/// writer plan builds every frame (OI-93).
///
/// FOP-1 puts the FDU in the data field as it is: the Frame Data Field of a TC frame, the TFDZ of
/// a USLP frame. Built only by [`tc`](Fop1Target::tc) and [`uslp`](Fop1Target::uslp), which check
/// the channel once (RS-020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fop1Target<P: Fop1Format = TcFormat> {
    channel: P::Channel,
    clcw_vc_id: u8,
    max_frame_len: usize,
    max_fdu_len: usize,
    protocol: PhantomData<P>,
}

/// A frame of a target to build, by its type and what it carries: the FDU of a Type-AD or a
/// Type-BD frame, the Control Command of a Type-BC frame (232.0 §4.1.2.3.3 Table 4-1,
/// §4.1.3.3). A pair of a frame type and a payload that do not belong together has no value
/// (RS-020).
#[derive(Debug, Clone, Copy)]
enum Payload<'a> {
    Ad(&'a [u8]),
    Bd(&'a [u8]),
    Bc(ControlCommand),
}

impl Payload<'_> {
    /// The type of the frame that carries the payload.
    const fn frame_type(&self) -> FrameType {
        match self {
            Payload::Ad(_) => FrameType::Ad,
            Payload::Bd(_) => FrameType::Bd,
            Payload::Bc(_) => FrameType::Bc,
        }
    }
}

impl Fop1Target<TcFormat> {
    /// The TC Virtual Channel (`spacecraft_id`, `vc_id`) of the physical channel `physical`,
    /// whose managed parameters give the Maximum Transfer Frame Length and the FECF (232.0
    /// §4.1.4, Table 5-1, Table 5-3; COP-010; M6 plan WP-2c).
    ///
    /// A `const fn`: a target of constant parameters is built and checked when the program is
    /// built, for a [`Fop1Config`] in a `const` (see [`Fop1::new`]).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_frame_len`) if a frame of the length of `physical` has no data
    /// field octet (232.0 Table 5-3, CMN-041): `PhysicalChannelConfig::new` has checked the
    /// length, 6 to 1 024 octets and 8 or more with a FECF, so this does not fail in practice;
    /// the VC and table constructors it calls are the ones that would report it.
    pub const fn tc(
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        physical: PhysicalChannelConfig,
    ) -> Result<Self> {
        let max_frame_len = physical.max_frame_len();
        let vc = match VcConfig::new(
            spacecraft_id,
            vc_id,
            max_frame_len,
            SegmentHeaderUse::Absent(SecurityLayout::NONE),
        ) {
            Ok(vc) => vc,
            Err(e) => return Err(e),
        };
        let table = match VcTable::new(physical, &[vc], &[]) {
            Ok(table) => table,
            Err(e) => return Err(e),
        };
        Ok(Self::new(
            TcChannel {
                table,
                spacecraft_id,
                vc_id,
            },
            vc_id.get(),
            max_frame_len,
            max_fdu_len(
                max_frame_len,
                physical.fecf(),
                TcFormat::PRIMARY_HEADER_LEN,
                TcFormat::SDU_HEADER_LEN,
            ),
        ))
    }
}

#[cfg(feature = "uslp")]
impl Fop1Target<UslpFormat> {
    /// The USLP Virtual Channel `vc` and MAP `map` of the variable-length Physical Channel
    /// `physical`, whose user-data frames carry the TFDF Header `tfdf` and an FDU as the TFDZ
    /// (732.1 §4.1, §4.2.7.3; COP-010, USLP-013).
    ///
    /// FOP-1 supplies the Bypass/Sequence Control and Protocol Control Command Flags and the
    /// Frame Sequence Number (732.1 §4.2.7.4): Type-AD frames are sequence-controlled user data
    /// with N(S) in the VCF Count, Type-BD frames expedited user data, and Type-BC frames
    /// Protocol Control Commands with the UPID "COP-1 Control Commands", the rule `111` (no
    /// segmentation, which holds "one or more Control Commands", §4.1.4.2.2.2.8 d), and the TFDZ
    /// of the Unlock or Set V(R) command. A frame has no Insert Zone, OCF, or SDLS fields, since
    /// FOP-1 has none to give. FOP-1 is the sending end of a ground station: the
    /// Source-or-Destination Identifier of every frame is Destination (`1`, the Spacecraft ID
    /// names the destination, §4.1.2.3.3); a spacecraft-side sender is not built.
    ///
    /// Two limits are this crate's own readings and not the Blue Book's (OI-66): the VCF Count
    /// Length of the sequence-controlled service is one octet, so that N(S) keeps the 8-bit
    /// range of COP-1, and the expedited VCF Count is absent, because FOP-1 keeps no expedited
    /// counter. A VC that needs another length cannot use this target.
    ///
    /// # Errors
    /// `InvalidConfig`, checked in this order:
    /// - (`transfer_frame_type`) for a fixed-length Physical Channel or Virtual Channel: Control
    ///   Commands need a variable-length TFDZ (rule `111`, §4.1.4.2.2.2.8, Table 4-3), and
    ///   FOP-1 has no Insert Zone to fill;
    /// - (`truncated_frame_len`) for a VC with a truncated frame length, which carries only
    ///   truncated frames (OI-48);
    /// - (`vcf_count_len`) unless the VCF Count Length is one octet for the sequence-controlled
    ///   service and none for the expedited service (OI-66);
    /// - (`security_header_len`) for SDLS fields on the VC or the MAP;
    /// - (`tfdz_construction_rule`) for a rule of a fixed-length TFDZ (`000` to `010`, Table
    ///   4-3);
    /// - (`upid`) for a UPID that cannot label user data on this target: "Only Idle Data" (an
    ///   OID TFDF carries no user data, §4.1.4.1.9, §4.1.4.1.11), and the UPIDs of control
    ///   commands and SPDUs (COP-1, COP-P, Proximity-1), which FOP-1 uses for its own Type-BC
    ///   frames;
    /// - (`max_frame_len`) for a frame too short for a Set V(R) command;
    /// - and those of [`UslpVcTable::new`](aldis_uslp::VcTable::new).
    pub fn uslp(
        physical: UslpPhysical,
        vc: UslpVcConfig,
        map: MapConfig,
        tfdf: TfdfHeader,
    ) -> Result<Self> {
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
        if matches!(
            tfdf.upid,
            Upid::OnlyIdleData
                | Upid::Cop1ControlCommands
                | Upid::CopPControlCommands
                | Upid::Proximity1Spdus
        ) {
            return Err(bad_config(field::UPID));
        }
        let max_frame_len = physical.frame_len();
        // The Primary Header with the VCF Count of a Type-AD frame (one octet, because the VCF
        // Count Length check above refuses any other length; OI-66), the TFDF Header (one octet:
        // the rules with a pointer are refused above), the TFDZ, and the FECF.
        let max_fdu_len = max_fdu_len(
            max_frame_len,
            physical.fecf(),
            UslpFormat::primary_header_len(&vc.vcf_count_len(false)),
            tfdf.len(),
        );
        if max_fdu_len < SET_VR_LEN {
            return Err(bad_config(field::MAX_FRAME_LEN));
        }
        let table = UslpVcTable::new(physical, &[vc], &[map])?;
        Ok(Self::new(
            UslpChannel {
                table,
                spacecraft_id: vc.spacecraft_id(),
                vc_id: vc.vc_id(),
                map_id: map.map_id(),
                tfdf,
            },
            vc.vc_id().get(),
            max_frame_len,
            max_fdu_len,
        ))
    }
}

impl<P: Fop1Format> Fop1Target<P> {
    /// A target of the checked `channel`, with the lengths its constructor computed.
    const fn new(
        channel: P::Channel,
        clcw_vc_id: u8,
        max_frame_len: usize,
        max_fdu_len: usize,
    ) -> Self {
        Self {
            channel,
            clcw_vc_id,
            max_frame_len,
            max_fdu_len,
            protocol: PhantomData,
        }
    }

    /// The channel the frames are built for: its identifiers (COP-010).
    #[must_use]
    pub const fn channel(&self) -> &P::Channel {
        &self.channel
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

    /// Encodes a frame as [`encode`](Self::encode) into `out` after the octets it holds, and
    /// returns its range (API-061).
    ///
    /// # Errors
    /// Those of [`encode`](Self::encode); `BufferTooSmall` counts the octets `out` holds in `Needed`.
    fn encode_at(
        &self,
        sequence_number: FrameSeqNum,
        payload: Payload<'_>,
        out: &mut OutArea<'_>,
    ) -> Result<OutRange> {
        let start = out.used;
        let area = out
            .area
            .get_mut(start..)
            .ok_or(Error::buffer_too_small(start))?;
        let len = self.encode(sequence_number, payload, area).map_err(|e| {
            match (e.kind(), e.detail()) {
                (ErrorKind::BufferTooSmall, ErrorDetail::Needed(n)) => {
                    Error::buffer_too_small(start.saturating_add(n))
                }
                _ => e,
            }
        })?;
        out.used = start.saturating_add(len);
        Ok(OutRange::new(start, len))
    }

    /// Encodes a frame of `frame_type` with N(S) = `sequence_number` and `payload` into `out`;
    /// returns its length (232.0 §4.1, 732.1 §4.1; COP-010, TC-005, TC-008, USLP-013).
    ///
    /// The codec's plan starts the frame, and its writer fills the data field, fixes its length
    /// (no SDLS), and computes the FECF (RS-023, API-023).
    fn encode(
        &self,
        sequence_number: FrameSeqNum,
        payload: Payload<'_>,
        out: &mut [u8],
    ) -> Result<usize> {
        let command;
        let octets = match payload {
            Payload::Ad(user_data) | Payload::Bd(user_data) => user_data,
            Payload::Bc(c) => {
                command = c.to_octets();
                command.as_slice()
            }
        };
        let mut writer = P::begin(
            &self.channel,
            payload.frame_type(),
            sequence_number,
            octets.len(),
            out,
        )?;
        // The copy is exact: `P::begin` reserved `octets.len()` octets of data field, which
        // `data_mut` returns whole, and `unsecured` below fixes that same length (RS-023).
        for (d, s) in writer.data_mut().iter_mut().zip(octets) {
            *d = *s;
        }
        writer.unsecured(octets.len()).map(|w| w.finalize().len())
    }
}

/// The octets of the longest Control Command, Set V(R) (232.0 §4.1.3.3).
#[cfg(feature = "uslp")]
const SET_VR_LEN: usize = 3;

/// The managed parameters of one FOP-1 instance, one per Virtual Channel, at start-up
/// (232.1 §7.2 Table 7-1; COP-015, COP-016, API-041).
///
/// The directives E36 to E39 change K, `T1_Initial`, `Transmission_Limit`, and `Timeout_Type`
/// later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fop1Config<P: Fop1Format = TcFormat> {
    /// The Virtual Channel the frames are built for (COP-010).
    pub target: Fop1Target<P>,
    /// `T1_Initial`, at least 1 µs (232.1 §5.1.9).
    pub t1_initial: Duration,
    /// `Transmission_Limit` (232.1 §5.1.10.2).
    pub transmission_limit: NonZeroU8,
    /// `Timeout_Type` (232.1 §5.1.10.3).
    pub timeout_type: TimeoutType,
    /// The FOP Sliding Window Width K: 1..=min(255, `Q`) (232.1 §5.1.12, §7.2 Table 7-1; COP-015).
    /// 0 is unrepresentable; [`Fop1::new`] checks the upper bound, which depends on `Q`.
    pub sliding_window_k: NonZeroU8,
    /// The Repetitions value of Type-AD frames, at least 1 (232.0 §2.4.2, Table 5-3,
    /// 231.0 Annex A3), passed through on their Transmit actions and never acted on
    /// (232.1 §5.1.9.3 note 1; COP-016). 1 requests no systematic retransmission.
    pub ad_repetitions: NonZeroU8,
    /// The Repetitions value of Type-BC frames, at least 1 (232.0 §2.4.2, Table 5-3,
    /// 231.0 Annex A3; COP-016).
    pub bc_repetitions: NonZeroU8,
}

/// `InvalidConfig` naming `parameter` (API-040).
const fn bad_config(parameter: aldis_core::FieldId) -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(parameter)
}

/// The kind of a frame FOP-1 retains on the `Sent_Queue` for an outstanding Transmit request:
/// Type-AD or Type-BC. A Type-BD frame is not retained (232.1 §5.1.6, §5.1.7; COP-011, API-056).
///
/// [`Fop1::held_frame`] takes it, so a request for the held frame of a Type-BD request does not
/// compile (RS-020, RS-021). The caller answers the request with the [`FrameKind`] of
/// [`Fop1Output::Transmit`]: `FrameKind::from(held)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeldKind {
    /// A Type-AD frame (232.1 §5.2.4).
    Ad,
    /// The Type-BC frame of an Initiate AD directive (232.1 §5.2.5).
    Bc,
}

impl HeldKind {
    /// The retained kind of `kind`: `None` for Type-BD, which is not retained (232.1 §5.1.7;
    /// COP-011, API-056).
    #[must_use]
    pub const fn from_frame_kind(kind: FrameKind) -> Option<Self> {
        match kind {
            FrameKind::Ad => Some(Self::Ad),
            FrameKind::Bc => Some(Self::Bc),
            FrameKind::Bd => None,
        }
    }
}

impl From<HeldKind> for FrameKind {
    /// The frame kind of a retained kind, for [`Fop1Input::TransmitDone`] (COP-011, API-056).
    fn from(kind: HeldKind) -> Self {
        match kind {
            HeldKind::Ad => Self::Ad,
            HeldKind::Bc => Self::Bc,
        }
    }
}

/// What a frame on the `Sent_Queue` is (232.1 §5.1.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SentKind {
    /// A Type-AD frame and the handle of its transfer request.
    Ad(Handle),
    /// The Type-BC frame of an Initiate AD directive.
    Bc,
}

/// Where the FDU of a Type-AD frame that a decision builds is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FduSource {
    /// The FDU of the input.
    Input,
    /// The first octets of the engine's `Wait_Queue` entry.
    Wait(usize),
}

/// Where the octets of a master copy are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Origin {
    /// Position `i` of the engine's `frames`.
    Base(usize),
    /// A Type-AD frame that a decision builds, and that [`Fop1Decision::commit`] encodes into its
    /// place.
    FreshAd(FduSource),
    /// A Type-BC frame that a decision builds.
    FreshBc(ControlCommand),
}

/// An entry of the `Sent_Queue` with its `To_Be_Retransmitted_Flag`, without the octets of the
/// master copy (232.1 §5.1.5, §5.1.7; COP-011). The octets are in [`Fop1::frames`], in the
/// order of the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Sent {
    origin: Origin,
    len: usize,
    sequence_number: FrameSeqNum,
    kind: SentKind,
    to_be_retransmitted: bool,
}

impl Sent {
    /// The handle of a Type-AD frame's request; `None` for the Type-BC frame.
    const fn handle(&self) -> Option<Handle> {
        match self.kind {
            SentKind::Ad(handle) => Some(handle),
            SentKind::Bc => None,
        }
    }
}

/// The FDU on the `Wait_Queue` and the handle of its request (232.1 §5.1.4). The octets are in
/// [`Fop1::wait_fdu`], or, for a `fresh` entry that a decision adds, in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Waiting {
    handle: Handle,
    len: usize,
    fresh: bool,
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
    ///
    /// At most one Transmit request per kind is outstanding: the Out flag of the kind is
    /// `Not_Ready` from the request until a [`Fop1Input::TransmitDone`] that accepts it
    /// (232.1 §5.1.6); a Reject leaves it `Not_Ready` (E42, E44, E46). For a
    /// Type-AD or Type-BC request, [`Fop1::held_frame`] returns the frame the engine holds, read
    /// with a [`HeldKind`], so a caller that waits for a link slot need not copy it; a Type-BD
    /// frame is not retained.
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

/// The FOP-1 state without the octets of the queues: everything that is small (232.1 §5.1), for
/// a `Sent_Queue` of `Q` entries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Core<const Q: usize> {
    t1_initial: Duration,
    transmission_limit: NonZeroU8,
    timeout_type: TimeoutType,
    k: NonZeroU8,
    ad_repetitions: NonZeroU8,
    bc_repetitions: NonZeroU8,
    state: Fop1State,
    v_s: FrameSeqNum,
    nn_r: FrameSeqNum,
    sent: [Option<Sent>; Q],
    wait: Option<Waiting>,
    /// `AD_Out_Flag` (232.1 §5.1.6): `None` is Ready, `Some(n)` is `Not_Ready` with the Type-AD
    /// frame N(S) = `n` in flight. At most one is, and no Type-AD frame is added while one is.
    ad_out: Option<FrameSeqNum>,
    bc_out_ready: bool,
    bd_out: Option<Handle>,
    pending_initiate: Option<Handle>,
    deadline: Option<Instant>,
    transmission_count: u8,
    suspend_state: SuspendState,
}

/// The FOP-1 state of one Virtual Channel, with a `Sent_Queue` of `Q` entries for frames of at
/// most `F` octets and an action list of `N` entries (232.1 §5.1; COP-001, API-060).
///
/// Plain data without an output queue (API-059). The const parameters are capacities:
/// - `Q`, the number of entries of the `Sent_Queue` (master copies of Type-AD and Type-BC
///   frames), at least the sliding window width K (COP-015);
/// - `F`, the most octets of a frame or an FDU the engine keeps, at least the Maximum Transfer
///   Frame Length of the target;
/// - `N`, the most actions one input returns, at least [`fop1_max_actions`]`(Q)` (COP-006).
///
/// The engine owns its octets: the [`Fop1Head`], everything small, and the [`Fop1Store`], the
/// master copies and the `Wait_Queue` FDU, the only large part of the state. An input is decided
/// without copying them ([`decide`](Self::decide)) and then applied in place
/// ([`Fop1Decision::commit`]). No `decide` or `commit` takes a store, so no input is decided
/// or committed with the store of another engine (RS-021, RS-022).
///
/// The layout is `repr(C)`, the head and then the store, and [`HEAD_OFFSET`](Self::HEAD_OFFSET)
/// and [`BUFFERS_OFFSET`](Self::BUFFERS_OFFSET) are the offsets of the two: a caller that places an
/// engine in its own memory without building the store by value (a C caller, CABI-040) writes
/// a [`Fop1Head`] at the first and leaves the zeroed store at the second. This crate does the
/// placement with no `unsafe` (GEN-071); the caller's `unsafe` is its own.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct Fop1<const Q: usize, const F: usize, const N: usize, P: Fop1Format = TcFormat> {
    head: Fop1Head<Q, F, N, P>,
    store: Fop1Store<Q, F>,
}

/// The small part of a [`Fop1`]: the target, the managed parameters, and every variable of
/// 232.1 §5.1 except the octets of the queues (COP-001, API-040).
///
/// A head is built from the configuration ([`Fop1Head::new`]) and has no method that acts on an
/// input: only a [`Fop1`], which pairs it with its [`Fop1Store`], decides one. It is public so
/// that a caller that places an engine in its own memory can build the head apart from the store
/// ([`Fop1::HEAD_OFFSET`]). The capacities `Q`, `F`, and `N` are those of the [`Fop1`] it is for;
/// the head of one capacity is not the head of another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fop1Head<const Q: usize, const F: usize, const N: usize, P: Fop1Format = TcFormat> {
    target: Fop1Target<P>,
    core: Core<Q>,
}

/// The octets of the engine's queues: the master copies of the `Sent_Queue` and the FDU of the
/// `Wait_Queue` (232.1 §5.1.4, §5.1.7; COP-011, API-060).
///
/// The one large part of a FOP-1 (`Q × F` octets and one more `F`), for the capacities `Q`
/// (entries of the `Sent_Queue`) and `F` (octets of a frame or an FDU) of [`Fop1`]. It is a
/// plain array type that is valid when zeroed (GEN-011), so that a caller places a [`Fop1`] in
/// its own storage (memory that a C caller zeroes) without building it by value
/// ([`Fop1::BUFFERS_OFFSET`]); a Rust caller builds the [`Fop1`] in a `static` through the
/// `const fn` [`Fop1::new`]. A [`Fop1`] owns its store from [`Fop1::new`] on, and the store
/// starts zeroed; no function that decides or commits an input takes one (RS-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fop1Store<const Q: usize, const F: usize> {
    /// The master copies of the `Sent_Queue`, by position in the queue; a position without an
    /// entry is zero.
    frames: [[u8; F]; Q],
    /// The FDU of the `Wait_Queue`; zero while it is empty.
    wait_fdu: [u8; F],
}

impl<const Q: usize, const F: usize> Fop1Store<Q, F> {
    /// An empty store: every octet zero (232.1 §5.1; COP-001, GEN-011).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frames: [[0; F]; Q],
            wait_fdu: [0; F],
        }
    }
}

impl<const Q: usize, const F: usize> Default for Fop1Store<Q, F> {
    fn default() -> Self {
        Self::new()
    }
}

/// What a decision reads of the engine besides its small state: the target and the store.
struct Base<'b, const Q: usize, const F: usize, P: Fop1Format> {
    target: &'b Fop1Target<P>,
    store: &'b Fop1Store<Q, F>,
}

/// The actions one input of [`Fop1`] returns, at most `N` (COP-006, API-060).
pub type Fop1Actions<const N: usize> = Actions<Fop1Output, N>;

/// What [`Fop1::decide`] found for one input: the actions of the cell and the change it makes
/// to the state, to be applied by [`commit`](Self::commit) (API-058, RS-022).
///
/// It holds the exclusive borrow of the [`Fop1`] that `decide` was called on, so the engine
/// takes no other input while the decision is live, and the decision can be committed to
/// nothing else: committing it to another engine, or after the engine has taken another input,
/// does not compile (RS-021, RS-022, API-058; the examples below). The engine owns its store,
/// so the decision cannot be made or committed with the store of another engine either.
///
/// It holds the small part of the next state and the entries of the `Sent_Queue` by reference
/// to the engine's master copies, not the octets of the master copies. It borrows the FDU of
/// the input, which [`commit`](Self::commit) copies to the `Wait_Queue` or into a new master
/// copy. `Q`, `F`, and `N` are those of the [`Fop1`] that made it (`N`: the most actions, the
/// capacity of [`Fop1Actions`]). There are two lifetimes: `'e` of the borrow of the engine,
/// which the decision cannot outlive, and `'a` of the FDU of the input, so that an input that
/// ends before the decision is committed is still no reason to shorten the borrow of the
/// engine.
///
/// The decision is built apart from the engine, which `decide` only reads, so an input that
/// fails drops the decision and leaves the engine as it was, and so does a decision that is
/// dropped without a commit (API-058). A decision is not `Clone`, and [`commit`](Self::commit)
/// takes it by value, so it is applied at most once.
///
/// Another engine: `commit` takes no engine, so a decision is not committed to one other than
/// the one it borrows (RS-021, RS-022, API-058).
///
/// ```compile_fail,E0061
/// use aldis_cop1::{Fop1, Fop1Config, Fop1Input, fop1_max_actions};
/// use aldis_core::time::Instant;
///
/// const Q: usize = 4;
/// const F: usize = 64;
/// const N: usize = fop1_max_actions(Q);
/// fn other(ca: Fop1Config, cb: Fop1Config, input: Fop1Input<'_>) {
///     let mut a = Fop1::<Q, F, N>::new(ca).unwrap();
///     let mut b = Fop1::<Q, F, N>::new(cb).unwrap();
///     let mut out = [0u8; F];
///     let decision = a.decide(input, Instant::from_micros(0), &mut out).unwrap();
///     let _ = decision.commit(&mut b);
/// }
/// ```
///
/// After the engine's state changed: the decision borrows the engine until its commit, so a
/// second input cannot reach the engine in between (RS-021, RS-022, API-058).
///
/// ```compile_fail,E0499
/// use aldis_cop1::{Fop1, Fop1Config, Fop1Input, fop1_max_actions};
/// use aldis_core::time::Instant;
///
/// const Q: usize = 4;
/// const F: usize = 64;
/// const N: usize = fop1_max_actions(Q);
/// fn stale(config: Fop1Config, a: Fop1Input<'_>, b: Fop1Input<'_>) {
///     let mut fop = Fop1::<Q, F, N>::new(config).unwrap();
///     let mut out = [0u8; F];
///     let old = fop.decide(a, Instant::from_micros(0), &mut out).unwrap();
///     let _ = fop.decide(b, Instant::from_micros(1), &mut out).unwrap().commit();
///     let _ = old.commit();
/// }
/// ```
///
/// A store that is not the engine's: `decide` takes no store, so there is none to give it
/// (RS-021, RS-022, API-058).
///
/// ```compile_fail,E0061
/// use aldis_cop1::{Fop1, Fop1Config, Fop1Input, Fop1Store, fop1_max_actions};
/// use aldis_core::time::Instant;
///
/// const Q: usize = 4;
/// const F: usize = 64;
/// const N: usize = fop1_max_actions(Q);
/// fn foreign(config: Fop1Config, input: Fop1Input<'_>) {
///     let mut fop = Fop1::<Q, F, N>::new(config).unwrap();
///     let mut store = Fop1Store::<Q, F>::new();
///     let mut out = [0u8; F];
///     let _ = fop.decide(&mut store, input, Instant::from_micros(0), &mut out);
/// }
/// ```
///
/// The decision does not outlive the engine it borrows (RS-021, RS-022, API-058).
///
/// ```compile_fail,E0505
/// use aldis_cop1::{Fop1, Fop1Config, Fop1Input, fop1_max_actions};
/// use aldis_core::time::Instant;
///
/// const Q: usize = 4;
/// const F: usize = 64;
/// const N: usize = fop1_max_actions(Q);
/// fn outlives(config: Fop1Config, input: Fop1Input<'_>) {
///     let mut fop = Fop1::<Q, F, N>::new(config).unwrap();
///     let mut out = [0u8; F];
///     let decision = fop.decide(input, Instant::from_micros(0), &mut out).unwrap();
///     drop(fop);
///     let _ = decision.commit();
/// }
/// ```
#[must_use]
#[derive(Debug)]
pub struct Fop1Decision<
    'e,
    'a,
    const Q: usize,
    const F: usize,
    const N: usize,
    P: Fop1Format = TcFormat,
> {
    engine: &'e mut Fop1<Q, F, N, P>,
    draft: Draft<'a, Q, F, N, P>,
}

/// The result of the cell, apart from the engine: the small state it leads to, its actions, and
/// the FDU of the input. [`Fop1Decision`] adds the borrow that makes it applicable.
#[derive(Debug)]
struct Draft<'a, const Q: usize, const F: usize, const N: usize, P: Fop1Format> {
    core: Core<Q>,
    actions: Fop1Actions<N>,
    fdu: &'a [u8],
    protocol: PhantomData<P>,
}

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Fop1Head<Q, F, N, P> {
    /// Fails the build of a head whose `N` is below the bound of Table 5-1 (COP-006).
    const BOUND: () = assert!(N >= fop1_max_actions(Q), "N is below fop1_max_actions(Q)");

    /// The head of a FOP-1 in State S6 (Initial) with V(S) = NN(R) = 0, every Out flag Ready, no
    /// timer, and `Suspend_State` 0 (232.1 §5.1; OI-59). [`Fop1::new`] pairs it with an empty
    /// [`Fop1Store`]. A `const fn`, like [`Fop1::new`].
    ///
    /// # Errors
    /// `InvalidConfig` naming `sliding_window_k` for a K outside 1..=min(255, `Q`) (COP-015),
    /// `t1_initial` for a `T1_Initial` of 0, and `max_frame_len` for a target whose frames do
    /// not fit `F` octets (API-040).
    pub const fn new(config: Fop1Config<P>) -> Result<Self> {
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
        let core = Core {
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
            ad_out: None,
            bc_out_ready: true,
            bd_out: None,
            pending_initiate: None,
            deadline: None,
            transmission_count: 1,
            suspend_state: SuspendState::NotSuspended,
        };
        Ok(Self {
            target: config.target,
            core,
        })
    }

    /// Whether `k` is a valid FOP Sliding Window Width: 1..=min(255, `Q`) (232.1 §5.1.12;
    /// COP-015). The type holds the bounds 1 and 255; this checks the one that depends on `Q`.
    #[expect(
        clippy::as_conversions,
        reason = "u8 to usize is lossless, and `usize::from` is not a const fn"
    )]
    const fn k_fits(k: NonZeroU8) -> bool {
        (k.get() as usize) <= Q
    }
}

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Fop1<Q, F, N, P> {
    /// The offset of the [`Fop1Head`] in a `Fop1`, for the in-place build of the C ABI
    /// (CABI-040). Not part of the API.
    #[doc(hidden)]
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the [`Fop1Store`] in a `Fop1`, for the in-place build of the C ABI
    /// (CABI-040): a zeroed engine with a head written at [`HEAD_OFFSET`](Self::HEAD_OFFSET) is a
    /// valid engine, because the store is valid when zeroed (GEN-011). Not part of the API.
    #[doc(hidden)]
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, store);

    /// A FOP-1 in State S6 (Initial) with V(S) = NN(R) = 0, every Out flag Ready, no timer,
    /// and `Suspend_State` 0, with an empty store (232.1 §5.1; OI-59, OI-45).
    ///
    /// The engine holds its store, `Q × F` octets and one more `F`, and is built by value, with
    /// no `unsafe`. A Rust caller whose stack cannot hold it (at the `[fop1]` sizes of
    /// `capacities.toml`, 16 × 1024 octets, it cannot: OI-45, VER-011) places it in a `static`
    /// from a `const` configuration: this is a `const fn`, as are [`Fop1Target::tc`],
    /// [`Fop1Head::new`], and the constructors under them, so the compiler builds the engine and
    /// its zeroed store and checks the configuration (GEN-011). The C ABI places the engine in
    /// caller storage with [`Fop1Head::new`] and the hidden offsets ([`HEAD_OFFSET`](Self::HEAD_OFFSET),
    /// [`BUFFERS_OFFSET`](Self::BUFFERS_OFFSET); CABI-040). The target of a USLP channel
    /// ([`Fop1Target::uslp`]) is not a `const fn`, so a `static` of the USLP format has no
    /// `const` configuration.
    ///
    /// [`decide`](Self::decide) takes `&mut self`, so an engine that is to be driven sits in a
    /// cell with a `const` constructor, which gives the `&mut` with no `unsafe` in the caller:
    /// `critical_section::Mutex<RefCell<_>>` or `static_cell::ConstStaticCell` in `no_std`,
    /// `std::sync::Mutex` in the example below. This crate depends on none of them.
    ///
    /// ```
    /// use aldis_cop1::{
    ///     Directive, Fop1, Fop1Config, Fop1Input, Fop1State, Fop1Target, TimeoutType,
    ///     fop1_max_actions,
    /// };
    /// use aldis_core::ids::FrameSeqNum;
    /// use aldis_core::time::{Duration, Instant};
    /// use aldis_tc::{PhysicalChannelConfig, TcSpacecraftId, TcVcId};
    /// use core::num::NonZeroU8;
    /// use std::sync::Mutex;
    ///
    /// // The `[fop1]` sizes of `capacities.toml`; the store is 17 KiB.
    /// const Q: usize = 16;
    /// const F: usize = 1024;
    /// const N: usize = fop1_max_actions(Q);
    ///
    /// const CONFIG: Fop1Config = {
    ///     let Ok(physical) = PhysicalChannelConfig::new(F, true) else { panic!() };
    ///     let Ok(target) = Fop1Target::tc(
    ///         TcSpacecraftId::from_const::<0x2A5>(),
    ///         TcVcId::from_const::<5>(),
    ///         physical,
    ///     ) else { panic!() };
    ///     let Some(transmission_limit) = NonZeroU8::new(3) else { panic!() };
    ///     let Some(sliding_window_k) = NonZeroU8::new(8) else { panic!() };
    ///     Fop1Config {
    ///         target,
    ///         t1_initial: Duration::from_micros(1_000_000),
    ///         transmission_limit,
    ///         timeout_type: TimeoutType::Tt0,
    ///         sliding_window_k,
    ///         ad_repetitions: NonZeroU8::MIN,
    ///         bc_repetitions: NonZeroU8::MIN,
    ///     }
    /// };
    ///
    /// // Built when the program is: the store is data of the program, not stack. The `Mutex`
    /// // gives the `&mut` that `decide` needs.
    /// static ENGINE: Mutex<Fop1<Q, F, N>> = Mutex::new(match Fop1::new(CONFIG) {
    ///     Ok(engine) => engine,
    ///     Err(_) => panic!("the configuration is valid"),
    /// });
    ///
    /// let mut engine = ENGINE.lock().unwrap();
    /// assert_eq!(engine.state(), Fop1State::Initial);
    /// assert_eq!(engine.v_s(), FrameSeqNum::new(0));
    /// let input = Fop1Input::Directive {
    ///     handle: 1,
    ///     directive: Directive::InitiateAdWithoutClcwCheck,
    /// };
    /// let mut out = [0u8; F];
    /// engine.decide(input, Instant::from_micros(0), &mut out)?.commit();
    /// assert_eq!(engine.state(), Fop1State::Active);
    /// # Ok::<(), aldis_core::Error>(())
    /// ```
    ///
    /// # Errors
    /// As [`Fop1Head::new`].
    pub const fn new(config: Fop1Config<P>) -> Result<Self> {
        match Fop1Head::new(config) {
            Ok(head) => Ok(Self {
                head,
                store: Fop1Store::new(),
            }),
            Err(e) => Err(e),
        }
    }

    /// The target Virtual Channel (COP-010).
    #[must_use]
    pub const fn target(&self) -> &Fop1Target<P> {
        &self.head.target
    }

    /// The State variable (232.1 §5.1.2; API-056).
    #[must_use]
    pub const fn state(&self) -> Fop1State {
        self.head.core.state
    }

    /// V(S), the N(S) of the next new Type-AD frame (232.1 §5.1.3; API-056).
    #[must_use]
    pub const fn v_s(&self) -> FrameSeqNum {
        self.head.core.v_s
    }

    /// NN(R), the N(S) of the oldest unacknowledged Type-AD frame (232.1 §5.1.8; API-056).
    #[must_use]
    pub const fn nn_r(&self) -> FrameSeqNum {
        self.head.core.nn_r
    }

    /// The FOP Sliding Window Width K (232.1 §5.1.12; API-056).
    #[must_use]
    pub const fn sliding_window_k(&self) -> NonZeroU8 {
        self.head.core.k
    }

    /// `T1_Initial` (232.1 §5.1.9; API-056).
    #[must_use]
    pub const fn t1_initial(&self) -> Duration {
        self.head.core.t1_initial
    }

    /// `Transmission_Limit` (232.1 §5.1.10.2; API-056).
    #[must_use]
    pub const fn transmission_limit(&self) -> NonZeroU8 {
        self.head.core.transmission_limit
    }

    /// `Timeout_Type` (232.1 §5.1.10.3; API-056).
    #[must_use]
    pub const fn timeout_type(&self) -> TimeoutType {
        self.head.core.timeout_type
    }

    /// `Transmission_Count` (232.1 §5.1.10.4; API-056).
    #[must_use]
    pub const fn transmission_count(&self) -> u8 {
        self.head.core.transmission_count
    }

    /// `Suspend_State` (232.1 §5.1.11; COP-014, API-056).
    #[must_use]
    pub const fn suspend_state(&self) -> SuspendState {
        self.head.core.suspend_state
    }

    /// The number of frames on the `Sent_Queue` (232.1 §5.1.7; API-056).
    #[must_use]
    pub fn sent_queue_len(&self) -> usize {
        self.head.core.sent.iter().flatten().count()
    }

    /// Whether an FDU waits on the `Wait_Queue` (232.1 §5.1.4; API-056).
    #[must_use]
    pub const fn wait_queue_occupied(&self) -> bool {
        self.head.core.wait.is_some()
    }

    /// Whether the Out flag of `kind` is Ready (232.1 §5.1.6; API-056).
    #[must_use]
    pub const fn out_flag_ready(&self, kind: FrameKind) -> bool {
        match kind {
            FrameKind::Ad => self.head.core.ad_out.is_none(),
            FrameKind::Bc => self.head.core.bc_out_ready,
            FrameKind::Bd => self.head.core.bd_out.is_none(),
        }
    }

    /// The frame the engine holds for the outstanding Transmit request of `kind`: the master
    /// copy on the `Sent_Queue`, trimmed to the frame's length (232.1 §5.1.6, §5.1.7; COP-011,
    /// API-056). A [`HeldKind`] has only Type-AD and Type-BC: a Type-BD frame is not retained and
    /// cannot be asked for (RS-020).
    ///
    /// A caller that holds a [`Fop1Output::Transmit`] request until a link slot reads the frame
    /// here instead of copying it from the output area. There is a frame while the Out flag of
    /// `kind` is `Not_Ready` ([`out_flag_ready`](Self::out_flag_ready) is `false`) and the master
    /// copy is still on the queue: the frame of the request, or the retransmitted master copy.
    /// The frame is byte-identical to the one in the output area.
    ///
    /// It is `None`:
    /// - while the Out flag of `kind` is Ready, that is, no request of the kind is outstanding;
    /// - for a request whose master copy has since left the `Sent_Queue`, acknowledged by a CLCW
    ///   or removed by a purge (an Alert, or Initialize; 232.1 §5.1.7): the engine retains no
    ///   copy, and the caller still answers the request with [`Fop1Input::TransmitDone`].
    ///
    /// An input changes the answer, as it changes the `Sent_Queue`: read the frame before the
    /// next input, which the borrow of this reader enforces (RS-021).
    #[must_use]
    pub fn held_frame(&self, kind: HeldKind) -> Option<&[u8]> {
        let core = &self.head.core;
        let in_flight = |sent: &Sent| match (kind, sent.kind) {
            (HeldKind::Ad, SentKind::Ad(_)) => core.ad_out == Some(sent.sequence_number),
            (HeldKind::Bc, SentKind::Bc) => !core.bc_out_ready,
            (HeldKind::Ad, SentKind::Bc) | (HeldKind::Bc, SentKind::Ad(_)) => false,
        };
        let (position, sent) = core
            .sent
            .iter()
            .enumerate()
            .find_map(|(p, s)| s.as_ref().filter(|s| in_flight(s)).map(|s| (p, s)))?;
        self.store
            .frames
            .get(position)
            .and_then(|frame| frame.get(..sent.len))
    }

    /// The instant at which [`Fop1Input::Timeout`] is to be fed, or `None` while the timer
    /// is not running (232.1 §5.1.9; COP-013, API-053).
    #[must_use]
    pub const fn next_deadline(&self) -> Option<Instant> {
        self.head.core.deadline
    }

    /// Decides `input` (API-058, RS-022): classifies it into its event, applies the cell of
    /// Table 5-1 to a copy of the small part of the state, and returns the actions of the cell,
    /// in the order of the cell, with the change to make (COP-001, COP-006). The state is not
    /// touched, and the master copies are not copied: [`Fop1Decision::commit`] applies the
    /// decision in place. The decision holds the exclusive borrow of the engine until it is
    /// committed or dropped, so no other input reaches the engine in between; dropping it
    /// changes nothing (RS-021). The store is the engine's own: there is none to pass.
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
    pub fn decide<'e, 'a>(
        &'e mut self,
        input: Fop1Input<'a>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<Fop1Decision<'e, 'a, Q, F, N, P>> {
        let classified = self.classify(input, now)?;
        let fdu = classified.as_ref().map_or(&[][..], |(_, ctx)| ctx.fdu);
        let mut decision = Fop1Decision::start(self, fdu);
        if let Some((event, ctx)) = &classified {
            decision.run(*event, ctx, out)?;
        }
        Ok(decision)
    }

    /// Decides the cell of `event` in the current state with the parameters of `input` (the
    /// handle, the FDU, the directive, N(R)), without classifying it (COP-005, API-058).
    ///
    /// This is the table function the conformance vectors and the cell checks address for the
    /// cells that no input reaches; [`decide`](Self::decide) is the same function after the
    /// classification.
    ///
    /// # Errors
    /// As [`decide`](Self::decide), and `InvalidState` when `input` lacks what the cell of
    /// `event` needs: its directive, its handle, or a Set V(R) directive for E27.
    pub fn decide_event<'e, 'a>(
        &'e mut self,
        event: Fop1Event,
        input: Fop1Input<'a>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<Fop1Decision<'e, 'a, Q, F, N, P>> {
        let mut ctx = Context {
            event,
            now,
            handle: None,
            fdu: &[],
            directive: None,
            n_r: self.head.core.nn_r,
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
            Fop1Input::TransmitDone { .. } => ctx.handle = self.head.core.bd_out,
            Fop1Input::InvalidClcw | Fop1Input::Timeout => {}
        }
        let mut decision = Fop1Decision::start(self, ctx.fdu);
        decision.run(event, &ctx, out)?;
        Ok(decision)
    }
}

impl<'e, 'a, const Q: usize, const F: usize, const N: usize, P: Fop1Format>
    Fop1Decision<'e, 'a, Q, F, N, P>
{
    /// The decision that changes nothing: the engine's small state, no action. `decide` builds
    /// the one decision it returns here and runs the cell on it where it is, with no draft built
    /// elsewhere and moved in, which would take another frame of a decision's size (VER-011).
    fn start(engine: &'e mut Fop1<Q, F, N, P>, fdu: &'a [u8]) -> Self {
        Fop1Decision {
            draft: Draft::start(&engine.head.core, fdu),
            engine,
        }
    }

    /// Runs the cell of `event` on the draft, reading the engine's target and store.
    fn run(&mut self, event: Fop1Event, ctx: &Context<'_>, out: &mut [u8]) -> Result<()> {
        let base = Base {
            target: &self.engine.head.target,
            store: &self.engine.store,
        };
        self.draft.run(&base, event, ctx, out)
    }
}

impl<'a, const Q: usize, const F: usize, const N: usize, P: Fop1Format> Draft<'a, Q, F, N, P> {
    /// The draft that changes nothing: the small state `core`, no action.
    fn start(core: &Core<Q>, fdu: &'a [u8]) -> Self {
        Draft {
            core: core.clone(),
            actions: Actions::new(),
            fdu,
            protocol: PhantomData,
        }
    }

    /// Executes the cell of `event` on this draft, which starts as the state before it,
    /// appending its actions (232.1 §5.3.1: the actions, then the State variable; COP-001,
    /// COP-003, COP-004).
    fn run(
        &mut self,
        base: &Base<'_, Q, F, P>,
        event: Fop1Event,
        ctx: &Context<'_>,
        out: &mut [u8],
    ) -> Result<()> {
        let prior = self.core.state;
        let cell = cell(prior, event);
        let Some(next) = cell.next_state else {
            return Err(Error::new(ErrorKind::InvalidState));
        };
        if matches!(
            event,
            Fop1Event::E16 | Fop1Event::E17 | Fop1Event::E18 | Fop1Event::E104
        ) {
            // The timer has expired (§5.1.9.1).
            self.core.deadline = None;
            if prior == Fop1State::RetransmitWithWait
                && matches!(event, Fop1Event::E16 | Fop1Event::E104)
            {
                // The one departure from Table 5-1 (OI-67): the cell says "Ignore", but a
                // disarmed timer would leave the frames of the Sent_Queue unrecovered when a
                // later CLCW takes FOP-1 to S1 with none running, and §5.1.9.1 has the Timer so
                // that a lost frame is always discovered. T1 runs for another T1_Initial and
                // the Transmission_Count stays (COP-013).
                self.start_timer(ctx.now);
            }
        }
        let mut out = OutArea::new(out);
        for action in cell.actions {
            self.act(base, *action, prior, ctx, &mut out)?;
        }
        if matches!(event, Fop1Event::E24 | Fop1Event::E25 | Fop1Event::E27) {
            // The directive completes on a later CLCW (S4, S5 with E1) or by Alert.
            self.core.pending_initiate = ctx.handle;
        }
        self.core.state = next;
        Ok(())
    }
}

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format>
    Fop1Decision<'_, '_, Q, F, N, P>
{
    /// The actions of the input, in the order of the cell (API-057).
    #[must_use]
    pub const fn actions(&self) -> &Fop1Actions<N> {
        &self.draft.actions
    }
}

/// The caller's output area and the octets of it that the actions of this input have taken, so
/// that the ranges of one input do not overlap (API-061).
struct OutArea<'o> {
    area: &'o mut [u8],
    used: usize,
}

impl<'o> OutArea<'o> {
    /// An output area with nothing written.
    const fn new(area: &'o mut [u8]) -> Self {
        OutArea { area, used: 0 }
    }

    /// Copies `frame` after the octets already used and returns their range (API-061).
    ///
    /// # Errors
    /// `BufferTooSmall` (detail `Needed`) if the area is too short.
    fn emit(&mut self, frame: &[u8]) -> Result<OutRange> {
        let start = self.used;
        let len = frame.len();
        let end = start.saturating_add(len);
        let area = self
            .area
            .get_mut(start..end)
            .ok_or(Error::buffer_too_small(end))?;
        for (d, s) in area.iter_mut().zip(frame) {
            *d = *s;
        }
        self.used = end;
        Ok(OutRange::new(start, len))
    }
}

// Kani proof harnesses, compiled only by `cargo kani` (docs/verification/formal-verification.md).
#[cfg(kani)]
mod proofs;

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
        Base, Context, Draft, FduSource, Fop1, Fop1Actions, Fop1Config, Fop1Format, Fop1Head,
        Fop1Input, Fop1Output, Fop1Store, Fop1Target, Origin, OutArea, Sent, SentKind, cell,
        fop1_max_actions,
    };
    use crate::fop1_table::{EVENT_COUNT, FOP1_TABLE, Fop1Action, STATE_COUNT};
    use crate::fop1_table::{Fop1Event, Fop1State, FrameKind, SuspendState};
    use crate::notify::{
        Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, TimeoutType,
        TransferNotification, TransferNotify,
    };
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::time::{Duration, Instant};
    use aldis_core::{ErrorDetail, ErrorKind, field};
    use aldis_tc::{Clcw, ClcwStatus, CopInEffect, FarmBCounter, TcFormat, TcSpacecraftId, TcVcId};
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
            aldis_tc::PhysicalChannelConfig::new(F, true).unwrap(),
        )
        .unwrap()
    }

    fn nz(n: u8) -> NonZeroU8 {
        NonZeroU8::new(n).unwrap()
    }

    fn config(tt: TimeoutType, limit: u8) -> Fop1Config {
        Fop1Config {
            target: target(),
            t1_initial: Duration::from_micros(100),
            transmission_limit: NonZeroU8::new(limit).unwrap(),
            timeout_type: tt,
            sliding_window_k: nz(2),
            ad_repetitions: nz(3),
            bc_repetitions: nz(4),
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

    fn go<P: Fop1Format>(
        f: &Fop1<Q, F, N, P>,
        input: Fop1Input<'_>,
        at: u64,
    ) -> (Fop1<Q, F, N, P>, Vec<Fop1Output>) {
        let mut out = [0u8; F];
        let mut next = f.clone();
        let actions = next
            .decide(input, Instant::from_micros(at), &mut out)
            .unwrap()
            .commit();
        (next, actions.iter().copied().collect())
    }

    fn view(r: &Engine) -> Base<'_, Q, F, TcFormat> {
        Base {
            target: &r.head.target,
            store: &r.store,
        }
    }

    fn inject(f: &Engine, event: Fop1Event, input: Fop1Input<'_>) -> Result<Engine, ErrorKind> {
        let mut out = [0u8; F];
        let mut next = f.clone();
        let _ = next
            .decide_event(event, input, Instant::from_micros(0), &mut out)
            .map_err(|e| e.kind())?
            .commit();
        Ok(next)
    }

    fn active() -> Engine {
        go(
            &fresh(),
            directive(1, Directive::InitiateAdWithoutClcwCheck),
            0,
        )
        .0
    }

    /// The frames an input transmits, as (N(S), retransmission, octets).
    fn frames_of(f: &mut Engine, input: Fop1Input<'_>, at: u64) -> Vec<(u8, bool, Vec<u8>)> {
        let mut out = [0u8; F];
        let decision = f.decide(input, Instant::from_micros(at), &mut out).unwrap();
        let listed: Vec<Fop1Output> = decision.actions().iter().copied().collect();
        let committed: Vec<Fop1Output> = decision.commit().iter().copied().collect();
        assert_eq!(listed, committed);
        committed
            .iter()
            .filter_map(|o| {
                if let Fop1Output::Transmit {
                    sequence_number,
                    retransmission,
                    frame,
                    ..
                } = o
                {
                    Some((
                        sequence_number.get(),
                        *retransmission,
                        frame.slice(&out).unwrap().to_vec(),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn decide_changes_nothing_and_commit_keeps_the_master_copies_in_queue_order() {
        // req: COP-011, API-058
        let mut f = active();
        let fdus: [[u8; 2]; 3] = [[0xA0, 1], [0xA0, 2], [0xA0, 3]];
        let done = Fop1Input::TransmitDone {
            kind: FrameKind::Ad,
            accepted: true,
        };
        let request = |i: usize| Fop1Input::RequestAd {
            handle: 10,
            fdu: &fdus[i],
        };
        let before = f.clone();
        let mut out = [0u8; F];
        let _ = f
            .decide(request(0), Instant::from_micros(1), &mut out)
            .unwrap();
        assert_eq!(f, before);
        let first = frames_of(&mut f, request(0), 1);
        frames_of(&mut f, done, 2);
        let second = frames_of(&mut f, request(1), 3);
        frames_of(&mut f, done, 4);
        // K = 2: the third FDU waits.
        assert_eq!(frames_of(&mut f, request(2), 5).len(), 0);
        assert!(f.wait_queue_occupied());
        // N(R) = 1 acknowledges the first frame, moves the second to the front of the queue,
        // and the FDU from the Wait_Queue becomes the third master copy.
        let third = frames_of(&mut f, clcw(false, false, false, 1), 6);
        assert_eq!((f.sent_queue_len(), f.wait_queue_occupied()), (2, false));
        assert_eq!(third.len(), 1);
        frames_of(&mut f, done, 7);
        // A retransmission reads the master copies from their new places.
        let again = frames_of(&mut f, clcw(false, false, true, 1), 8);
        assert_eq!(again, vec![(1, true, second[0].2.clone())]);
        let again = frames_of(&mut f, done, 9);
        assert_eq!(again, vec![(2, true, third[0].2.clone())]);
        assert_ne!(first[0].2, second[0].2);
        // The engine equals one that reached the same queue without the history: the unused
        // positions are zero.
        let mut g = f.clone();
        let _ = frames_of(&mut g, clcw(false, false, false, 3), 10);
        assert_eq!(g.sent_queue_len(), 0);
        assert!(
            g.store
                .frames
                .iter()
                .all(|frame| frame.iter().all(|b| *b == 0))
        );
    }

    #[test]
    fn an_input_that_fails_leaves_the_engine_and_the_store_unchanged() {
        // req: API-058, API-061, COP-011
        let mut f = active();
        let fdu = [0xA0u8, 9];
        let request = Fop1Input::RequestAd {
            handle: 1,
            fdu: &fdu,
        };
        frames_of(&mut f, request, 1);
        frames_of(
            &mut f,
            Fop1Input::TransmitDone {
                kind: FrameKind::Ad,
                accepted: true,
            },
            2,
        );
        let before = f.clone();
        // The output area is too short for the retransmission that the cell would make, after
        // the cell has already changed its copy of the state.
        let mut tiny = [0u8; 2];
        let e = f
            .decide(
                clcw(false, false, true, 0),
                Instant::from_micros(2),
                &mut tiny,
            )
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(f, before);
        let e = f
            .decide_event(
                Fop1Event::E31,
                Fop1Input::Timeout,
                Instant::from_micros(2),
                &mut tiny,
            )
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidState);
        assert_eq!(f, before);
    }

    #[test]
    fn a_timeout_counts_the_transmission_of_the_retransmitted_frames() {
        // req: COP-011, COP-013, API-056
        let mut f = active();
        let fdu = [0xA0u8, 9];
        let request = Fop1Input::RequestAd {
            handle: 1,
            fdu: &fdu,
        };
        frames_of(&mut f, request, 1);
        // The retransmission waits for the lower layer to have taken the first transmission.
        frames_of(
            &mut f,
            Fop1Input::TransmitDone {
                kind: FrameKind::Ad,
                accepted: true,
            },
            2,
        );
        assert_eq!(f.transmission_count(), 1);
        let late = f.next_deadline().unwrap().as_micros();
        let sent = frames_of(&mut f, Fop1Input::Timeout, late);
        assert_eq!(sent.len(), 1);
        assert!(sent.iter().all(|(_, retransmission, _)| *retransmission));
        assert_eq!(f.transmission_count(), 2);
    }

    #[test]
    fn a_new_store_is_zero() {
        // req: COP-011, API-058
        let store = Fop1Store::<Q, F>::default();
        assert_eq!(store, Fop1Store::new());
        assert!(store.frames.iter().flatten().all(|b| *b == 0));
        assert!(store.wait_fdu.iter().all(|b| *b == 0));
    }

    #[test]
    fn the_engine_is_its_head_and_its_zero_store_at_the_published_offsets() {
        // req: COP-001, API-040, GEN-011
        let f = fresh();
        let head = Fop1Head::<Q, F, N>::new(config(TimeoutType::Tt0, 2)).unwrap();
        assert_eq!(f.head, head);
        assert_eq!(f.store, Fop1Store::new());
        // The head comes first, the store after it, and both are inside the engine.
        assert_eq!(Engine::HEAD_OFFSET, 0);
        assert!(Engine::HEAD_OFFSET + size_of::<Fop1Head<Q, F, N>>() <= Engine::BUFFERS_OFFSET);
        assert!(Engine::BUFFERS_OFFSET + size_of::<Fop1Store<Q, F>>() <= size_of::<Engine>());
    }

    #[test]
    fn a_frame_a_decision_added_is_emitted_again_by_encoding_it() {
        // req: COP-011, API-061
        let f = active();
        let d: Draft<'_, Q, F, N, TcFormat> = Draft::start(&f.head.core, &FDU);
        let mut area = [0u8; 2 * F];
        let mut out = OutArea::new(&mut area);
        let entry = |origin| Sent {
            origin,
            len: 0,
            sequence_number: FrameSeqNum::new(0),
            kind: SentKind::Bc,
            to_be_retransmitted: false,
        };
        let ad = entry(Origin::FreshAd(FduSource::Input));
        let bc = entry(Origin::FreshBc(aldis_tc::ControlCommand::Unlock));
        let first = d.emit_sent(&view(&f), &ad, &mut out).unwrap();
        let second = d.emit_sent(&view(&f), &bc, &mut out).unwrap();
        assert_eq!((first.start(), second.start()), (0, first.len()));
        assert_eq!(out.used, first.len() + second.len());
        // The area is too short for the next frame: the octets already used count in `Needed`.
        let mut short = [0u8; 10];
        let mut short = OutArea::new(&mut short);
        short.used = 4;
        let e = d.emit_sent(&view(&f), &ad, &mut short).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(e.detail(), ErrorDetail::Needed(4 + first.len()));
        let wait = entry(Origin::FreshAd(FduSource::Wait(F + 1)));
        let e = d.emit_sent(&view(&f), &wait, &mut out).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InternalError);
    }

    /// The USLP target, as the integration tests of `tests/fop1_uslp.rs` use it; the unit tests
    /// run the same instance so that coverage counts the lines in both builds (VER-024).
    #[cfg(feature = "uslp")]
    #[test]
    fn the_uslp_target_builds_its_frames_and_refuses_what_it_cannot_fill() {
        // req: COP-010, USLP-013, API-040
        use aldis_core::security::SecurityLayout;
        use aldis_uslp::{
            FirstHeaderPointer, LastValidOctet, MapConfig, PhysicalChannelConfig, SecurityScope,
            TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen, Upid, UslpFormat,
            UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcfCountLen,
        };
        let scid = UslpSpacecraftId::new(0x155).unwrap();
        let vcid = UslpVcId::new(1).unwrap();
        let physical = |len, fecf| {
            PhysicalChannelConfig::new(TransferFrameType::Variable, len, 0, fecf).unwrap()
        };
        let params = VcParams {
            vcf_count_len_sequence: VcfCountLen::new(1).unwrap(),
            ..VcParams::new(TransferFrameType::Variable)
        };
        let vc = |p: VcParams| VcConfig::new(scid, vcid, p).unwrap();
        let map = MapConfig::new(scid, vcid, UslpMapId::new(2).unwrap(), SecurityLayout::NONE);
        let tfdf = TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid: Upid::OctetStream,
        };
        let why = |physical, vc, map, tfdf| {
            Fop1Target::uslp(physical, vc, map, tfdf)
                .map(|_| ())
                .map_err(|e| e.field())
        };
        let sdls = SecurityLayout::new(6, 0).unwrap();
        let fixed = PhysicalChannelConfig::new(TransferFrameType::Fixed, F, 0, true).unwrap();
        let fixed_vc = vc(VcParams::new(TransferFrameType::Fixed));
        let truncated = vc(VcParams {
            truncated_frame_len: Some(TruncatedFrameLen::new(12).unwrap()),
            ..params
        });
        let longer = |p: fn(&mut VcParams)| {
            let mut q = params;
            p(&mut q);
            vc(q)
        };
        let per_map = vc(VcParams {
            security: SecurityScope::PerMap,
            ..params
        });
        let rule = |rule| TfdfHeader { rule, ..tfdf };
        let (p64, v) = (physical(F, true), vc(params));
        let bad = |f| Err(Some(f));
        assert_eq!(why(fixed, v, map, tfdf), bad(field::TRANSFER_FRAME_TYPE));
        assert_eq!(
            why(p64, fixed_vc, map, tfdf),
            bad(field::TRANSFER_FRAME_TYPE)
        );
        assert_eq!(
            why(p64, truncated, map, tfdf),
            bad(field::TRUNCATED_FRAME_LEN)
        );
        let two = longer(|p| p.vcf_count_len_sequence = VcfCountLen::new(2).unwrap());
        let counted = longer(|p| p.vcf_count_len_expedited = VcfCountLen::new(1).unwrap());
        assert_eq!(why(p64, two, map, tfdf), bad(field::VCF_COUNT_LEN));
        assert_eq!(why(p64, counted, map, tfdf), bad(field::VCF_COUNT_LEN));
        let own = longer(|p| p.security = SecurityScope::Vc(SecurityLayout::new(6, 0).unwrap()));
        assert_eq!(why(p64, own, map, tfdf), bad(field::SECURITY_HEADER_LEN));
        let map_sdls = MapConfig::new(scid, vcid, map.map_id(), sdls);
        assert_eq!(
            why(p64, per_map, map_sdls, tfdf),
            bad(field::SECURITY_HEADER_LEN)
        );
        assert_eq!(why(p64, per_map, map, tfdf), Ok(()));
        for fixed_rule in [
            TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(0)),
            TfdzRule::SduStart(LastValidOctet::from_raw(0)),
            TfdzRule::SduContinuation(LastValidOctet::from_raw(0)),
        ] {
            assert_eq!(
                why(p64, v, map, rule(fixed_rule)),
                bad(field::TFDZ_CONSTRUCTION_RULE)
            );
        }
        let idle = TfdfHeader {
            upid: Upid::OnlyIdleData,
            ..tfdf
        };
        assert_eq!(why(p64, v, map, idle), bad(field::UPID));
        assert_eq!(
            why(physical(13, true), v, map, tfdf),
            bad(field::MAX_FRAME_LEN)
        );
        assert_eq!(why(physical(12, false), v, map, tfdf), Ok(()));
        let other = MapConfig::new(
            scid,
            UslpVcId::new(6).unwrap(),
            map.map_id(),
            SecurityLayout::NONE,
        );
        assert_eq!(why(p64, v, other, tfdf), bad(field::MAP_ID));

        // The frames: a Type-BC with Unlock, a Type-AD, and a Type-BD.
        let target = Fop1Target::uslp(p64, v, map, tfdf).unwrap();
        assert_eq!(target.max_fdu_len(), F - 11);
        assert_eq!(target.channel().map_id(), map.map_id());
        let uslp_config = Fop1Config {
            target,
            t1_initial: Duration::from_micros(100),
            transmission_limit: NonZeroU8::new(2).unwrap(),
            timeout_type: TimeoutType::Tt0,
            sliding_window_k: nz(2),
            ad_repetitions: nz(3),
            bc_repetitions: nz(4),
        };
        let f = Fop1::<Q, F, N, UslpFormat>::new(uslp_config).unwrap();
        let (f, out) = go(&f, directive(1, Directive::InitiateAdWithUnlock), 0);
        assert!(matches!(
            out.last(),
            Some(Fop1Output::Transmit {
                kind: FrameKind::Bc,
                ..
            })
        ));
        let (mut f, _) = go(
            &f,
            Fop1Input::TransmitDone {
                kind: FrameKind::Bc,
                accepted: true,
            },
            1,
        );
        let mut area = [0u8; F];
        let actions = f
            .decide(
                clcw(false, false, false, 0),
                Instant::from_micros(2),
                &mut area,
            )
            .unwrap()
            .commit();
        assert_eq!((f.state(), actions.len()), (Fop1State::Active, 1));
        let (f, out) = go(
            &f,
            Fop1Input::RequestAd {
                handle: 2,
                fdu: &FDU,
            },
            3,
        );
        assert!(matches!(
            out.last(),
            Some(Fop1Output::Transmit {
                kind: FrameKind::Ad,
                ..
            })
        ));
        let (_, out) = go(
            &f,
            Fop1Input::RequestBd {
                handle: 3,
                fdu: &FDU,
            },
            4,
        );
        assert!(matches!(
            out.last(),
            Some(Fop1Output::Transmit {
                kind: FrameKind::Bd,
                ..
            })
        ));
    }

    #[test]
    fn queries_and_construction() {
        // req: API-056, COP-006, COP-015, API-040
        assert_eq!(fop1_max_actions(black_box(Q)), N);
        let t = target();
        let plain = Fop1Target::tc(
            TcSpacecraftId::new(42).unwrap(),
            TcVcId::new(1).unwrap(),
            aldis_tc::PhysicalChannelConfig::new(F, false).unwrap(),
        )
        .unwrap();
        assert_eq!(plain.max_fdu_len(), F - 5);
        assert_eq!(
            t.channel().spacecraft_id(),
            TcSpacecraftId::new(42).unwrap()
        );
        assert_eq!(t.channel().vc_id(), TcVcId::new(1).unwrap());
        assert_eq!((t.max_frame_len(), t.max_fdu_len()), (F, F - 7));
        let f = fresh();
        assert_eq!(f.target(), &t);
        assert_eq!(
            (f.state(), f.v_s().get(), f.nn_r().get()),
            (Fop1State::Initial, 0, 0)
        );
        assert_eq!(f.sliding_window_k(), nz(2));
        assert_eq!(f.t1_initial(), Duration::from_micros(100));
        assert_eq!(f.transmission_limit().get(), 2);
        assert_eq!(f.timeout_type(), TimeoutType::Tt0);
        assert_eq!(f.transmission_count(), 1);
        assert_eq!(f.suspend_state(), SuspendState::NotSuspended);
        assert_eq!((f.sent_queue_len(), f.wait_queue_occupied()), (0, false));
        assert!(f.out_flag_ready(FrameKind::Ad) && f.out_flag_ready(FrameKind::Bc));
        assert!(f.out_flag_ready(FrameKind::Bd) && f.next_deadline().is_none());
        let bad = |c: Fop1Config| Engine::new(c).unwrap_err().field();
        // K = 0 and a Repetitions value of 0 do not compile (`NonZeroU8`); the bound that depends
        // on Q is checked here.
        let k = |k| Fop1Config {
            sliding_window_k: nz(k),
            ..config(TimeoutType::Tt0, 1)
        };
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
                aldis_tc::PhysicalChannelConfig::new(100, false).unwrap(),
            )
            .unwrap(),
            ..config(TimeoutType::Tt0, 1)
        };
        assert_eq!(bad(big), Some(field::MAX_FRAME_LEN));
        // The physical channel's managed parameters refuse the length (232.0 Table 5-1).
        assert!(aldis_tc::PhysicalChannelConfig::new(2000, true).is_err());
    }

    #[test]
    fn fdu_and_area_checks() {
        // req: API-058, API-061, CMN-041, TC-007
        let mut f = active();
        let mut out = [0u8; F];
        let now = Instant::from_micros(1);
        let empty = Fop1Input::RequestAd {
            handle: 2,
            fdu: &[],
        };
        assert_eq!(
            f.decide(empty, now, &mut out).unwrap_err().kind(),
            ErrorKind::InvalidField
        );
        let long = [0u8; F];
        let long = Fop1Input::RequestBd {
            handle: 2,
            fdu: &long,
        };
        assert_eq!(
            f.decide(long, now, &mut out).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            f.decide_event(Fop1Event::E21, long, now, &mut out)
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
            f.decide(request, now, &mut tiny).unwrap_err().kind(),
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
            directive(9, Directive::SetSlidingWindowWidth { width: nz(1) }),
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
            (nz(1), limit, TimeoutType::Tt1)
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
            Directive::SetSlidingWindowWidth { width: nz(5) },
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
        let mut a = active();
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
            a.decide(Fop1Input::Clcw(other), Instant::from_micros(1), &mut out)
                .unwrap_err()
                .kind(),
            ErrorKind::UnknownChannel
        );
    }

    #[test]
    fn a_clcw_that_matches_several_events_gets_the_highest_ranked() {
        // req: COP-001, COP-012
        // 232.1 §5.3.2.1 with OI-62 (a): the invalid pattern (E15), then Lockout (E14),
        // then an invalid N(R) (E13). V(S) = NN(R) = 0 here, so N(R) = 5 is invalid.
        let a = active();
        let event_of = |input: Fop1Input<'static>| -> Option<Fop1Event> {
            let (_, out) = go(&a, input, 1);
            out.iter().find_map(|o| {
                if let Fop1Output::Alert(alert) = o {
                    Some(alert.event)
                } else {
                    None
                }
            })
        };
        let word = |lockout: bool, n_r: u8, pattern_ok: bool| -> Fop1Input<'static> {
            match clcw(lockout, false, false, n_r) {
                Fop1Input::Clcw(c) if !pattern_ok => Fop1Input::Clcw(Clcw {
                    cop_in_effect: CopInEffect::new(2).unwrap(),
                    ..c
                }),
                other @ (Fop1Input::Clcw(_)
                | Fop1Input::InvalidClcw
                | Fop1Input::Timeout
                | Fop1Input::RequestAd { .. }
                | Fop1Input::RequestBd { .. }
                | Fop1Input::Directive { .. }
                | Fop1Input::InvalidDirective { .. }
                | Fop1Input::TransmitDone { .. }) => other,
            }
        };
        assert_eq!(event_of(word(true, 5, false)), Some(Fop1Event::E15));
        assert_eq!(event_of(word(false, 5, false)), Some(Fop1Event::E15));
        assert_eq!(event_of(word(true, 5, true)), Some(Fop1Event::E14));
        assert_eq!(event_of(word(false, 5, true)), Some(Fop1Event::E13));
    }

    #[test]
    fn transmit_actions_carry_the_configured_repetitions() {
        // req: COP-016
        let reps = |out: &[Fop1Output]| -> Vec<(FrameKind, u8)> {
            out.iter()
                .filter_map(|o| {
                    if let Fop1Output::Transmit {
                        kind, repetitions, ..
                    } = *o
                    {
                        Some((kind, repetitions))
                    } else {
                        None
                    }
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

    /// OI-67: T1 expires in S3 (E16, E104), where Table 5-1 says "Ignore". The timer runs for
    /// another `T1_Initial` and nothing else changes, so that the frames on the `Sent_Queue` are
    /// recovered when the Wait Flag clears (232.1 §5.1.9.1).
    #[test]
    fn t1_is_rearmed_when_it_expires_in_s3() {
        // req: COP-013
        for tt in [TimeoutType::Tt0, TimeoutType::Tt1] {
            let f = Engine::new(config(tt, 3)).unwrap();
            let (a, _) = go(&f, directive(1, Directive::InitiateAdWithoutClcwCheck), 0);
            let req = |handle| Fop1Input::RequestAd { handle, fdu: &FDU };
            let done = Fop1Input::TransmitDone {
                kind: FrameKind::Ad,
                accepted: true,
            };
            let (a, _) = go(&a, req(2), 1);
            let (a, _) = go(&a, done, 2);
            let (a, _) = go(&a, req(3), 3);
            let (s3, _) = go(&a, clcw(false, true, true, 1), 6);
            assert_eq!(s3.state(), Fop1State::RetransmitWithWait);
            let count = s3.transmission_count();
            let due = s3.next_deadline().unwrap();
            let (after, out) = go(&s3, Fop1Input::Timeout, due.as_micros());
            assert_eq!(out, vec![]);
            assert_eq!(after.state(), Fop1State::RetransmitWithWait);
            assert_eq!(after.transmission_count(), count);
            assert_eq!(
                after.next_deadline(),
                Some(due.saturating_add(Duration::from_micros(100)))
            );
        }
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
        let ad = a2.head.core.sent.iter().flatten().next().copied().unwrap();
        let mut slots = mixed.head.core.sent.iter_mut().filter(|s| s.is_none());
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
        let (mut s5r, _) = go(&s5r, Fop1Input::Timeout, 100);
        s5r.head.core.bc_out_ready = true;
        // A Sent_Queue full beyond what K admits: a broken invariant (InternalError).
        let mut jammed = a2.clone();
        jammed.head.core.sent.fill(Some(ad));
        jammed.head.core.ad_out = None;
        jammed.head.core.nn_r = jammed.head.core.v_s;
        let mut suspended = fresh();
        suspended.head.core.suspend_state = SuspendState::RetransmitWithWait;
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
            .filter_map(|o| {
                if let Fop1Output::Alert(a) = *o {
                    Some(a.reason)
                } else {
                    None
                }
            })
            .collect()
    }

    /// The Alert reasons the cell of `state` and `event` lists, in order.
    fn listed_alerts(state: Fop1State, event: Fop1Event) -> Vec<AlertReason> {
        cell(state, event)
            .actions
            .iter()
            .filter_map(|a| {
                if let Fop1Action::Alert(r) = *a {
                    Some(r)
                } else {
                    None
                }
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
    #[expect(
        clippy::excessive_nesting,
        reason = "an exhaustive sweep over independent parameters: each loop is one dimension"
    )]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "only the inputs that carry a directive or an N(R) set the context; the rest leave it"
    )]
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
                start.head.core.state = state;
                for event in events {
                    for input in inputs() {
                        let mut ctx = Context {
                            event,
                            now: Instant::from_micros(10),
                            handle: Some(20),
                            fdu: &FDU,
                            directive: None,
                            n_r: start.head.core.nn_r,
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
                            let mut a: Draft<'_, Q, F, N, TcFormat> =
                                Draft::start(&start.head.core, &FDU);
                            a.actions = list;
                            // A push without room fails the cell; none is dropped (API-083).
                            if a.run(&view(&start), event, &ctx, &mut big).is_ok() {
                                assert!(a.actions.len() <= N);
                                assert_eq!(alerts(&a.actions), listed_alerts(state, event));
                            }
                        }
                        // No output area: every transmit fails.
                        let mut b: Draft<'_, Q, F, N, TcFormat> =
                            Draft::start(&start.head.core, &FDU);
                        if b.run(&view(&start), event, &ctx, &mut []).is_ok() {
                            assert!(
                                b.actions
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
