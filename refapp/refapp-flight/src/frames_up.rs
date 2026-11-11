//! The flight node's uplink at frame level (docs/reference-application.md §2.1 step 1, §4.2,
//! §4.4.1, §7.4; APP-040…APP-043, APP-134, APP-139, APP-131).
//!
//! A unit delivered to the node is a TC frame. It goes through the chain of 232.0 §4.4:
//! strict decoding with the FECF (TC-021, TC-023), the `FrameReceiver` (LNK-050), FARM-1 for the
//! Type-BD frame (COP-030…COP-036, APP-043), and the `MapReassembler` with its two MAPs
//! (LNK-070, LNK-071). Each packet it delivers goes to the A1 command handler unchanged
//! ([`crate::FlightNode`]); the frame-level events of §7.4 are raised in the order of APP-139.
//!
//! At the `cop1` level (A3 on, [`FrameUplink::new_cop1`]) FARM-1 also sees the Type-AD and
//! Type-BC frames and its notifications become application events (APP-146, APP-149): the
//! state of FARM-1 as `farm1_state`, an Unlock as `farm1_unlock` with the purge of the
//! reassembler and of the delivery buffer (D4 c of the A3 plan). The `frames` level stays as A2
//! left it.
//!
//! FARM-1 is owned here, and [`FrameUplink::clcw`] is the report the downlink sets into its OCF
//! slot (§4.4.1, APP-134).

use aldis::cop1::farm1::{Farm1, Farm1Config, Farm1Input, Farm1Notify, Farm1Output, Farm1Window};
use aldis::cop1::farm1_table::{Farm1Event, Farm1State};
use aldis::core::ids::FrameSeqNum;
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::time::Instant as CoreInstant;
use aldis::core::{DecodeMode, Error, ErrorKind, Result, SecuritySlices};
use aldis::link::protocol::Tc;
use aldis::link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, ReassemblyEvent, TcReassemblerParams,
};
use aldis::link::receiver::{
    CallerDiscardReason, DiscardReason, FrameReceiver, FrameReceiverConfig, ReceiverInput,
    ReceiverOutput, TcChannel, TcVc, VcEntry,
};
use aldis::spp::SpacePacket;
use aldis::tc::{
    Clcw, ClcwStatus, MapConfig, PhysicalChannelConfig, Reason, SegmentHeader, SegmentHeaderUse,
    TcMapId, TcSpacecraftId, TcVcId, VcConfig, VcTable, check_frame,
};
use refapp_scenario::generated::{SPACECRAFT_ID, cop1, tc};
use refapp_scenario::{Instant, Output};

use crate::event::APP_PACKET_REJECTED;

/// Code of the application event `frame_rejected` (§7.4): the argument is the error code of the
/// catalogue in docs/03-api-model.md §9.
pub const APP_FRAME_REJECTED: u16 = 102;
/// Code of the application event `frame_discarded` (§7.4): layer, reason.
pub const APP_FRAME_DISCARDED: u16 = 103;
/// Code of the application event `sequence_violation` (§7.4): MAP ID.
pub const APP_SEQUENCE_VIOLATION: u16 = 110;
/// Code of the application event `unknown_map` (§7.4): MAP ID.
pub const APP_UNKNOWN_MAP: u16 = 111;
/// Code of the application event `sdu_too_long` (§7.4): MAP ID.
pub const APP_SDU_TOO_LONG: u16 = 112;

/// Code of the application event `farm1_state` (§7.4): the new state, 1 Open, 2 Wait, 3 Lockout.
pub const APP_FARM1_STATE: u16 = 210;
/// Code of the application event `farm1_unlock` (§7.4): no argument.
pub const APP_FARM1_UNLOCK: u16 = 211;

/// Layer of `frame_discarded` for the `FrameReceiver` (§7.4).
const LAYER_RECEIVER: u64 = 1;
/// Layer of `frame_discarded` for FARM-1 (§7.4).
const LAYER_FARM1: u64 = 2;

/// `value` as a `usize`, for the sizes below: `From` is not `const`.
#[expect(
    clippy::as_conversions,
    reason = "u16 to usize is lossless, and `usize::from` is not a const fn"
)]
const fn widen(value: u16) -> usize {
    value as usize
}

/// Capacity of a MAP's assembly buffer: the Maximum Packet Length of the uplink, 512 octets
/// (§4.2; `generated::tc::MAX_PACKET_LENGTH`), so a change of the scenario changes the build.
const ASSEMBLY: usize = widen(tc::MAX_PACKET_LENGTH);
/// The output area of the receiver and of FARM-1: one frame of at most the maximum length
/// (API-062; `generated::tc::MAX_FRAME_LENGTH`).
const FRAME_AREA: usize = widen(tc::MAX_FRAME_LENGTH);
/// The output area of the `MapReassembler` (LNK-075): the SDU held, at most `ASSEMBLY - 1`
/// octets, plus the user data of one frame, which is shorter than `FRAME_AREA` (the bound of the
/// whole frame, used here as the bound of its data field).
const REASSEMBLY_AREA: usize = ASSEMBLY.saturating_add(FRAME_AREA);

/// The delivery buffer of FARM-1 (232.0 §4.1.2.6, COP-033; §4.2 of the scenario): `DEPTH` places
/// that the node owns, one taken for each FDU FARM-1 accepts and released when the reassembler
/// has the FDU, in the same step, so that no case reaches Wait (§4.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeliveryBuffer {
    held: u8,
}

impl DeliveryBuffer {
    /// Places of the buffer (`generated::cop1::FARM_BUFFER_DEPTH`, COP-032).
    const DEPTH: u8 = cop1::FARM_BUFFER_DEPTH;

    const fn empty() -> DeliveryBuffer {
        DeliveryBuffer { held: 0 }
    }

    /// The `buffer_available` input of FARM-1: a place is free.
    const fn has_room(self) -> bool {
        self.held < Self::DEPTH
    }

    /// An FDU is delivered into the buffer.
    const fn take(&mut self) {
        self.held = self.held.saturating_add(1);
    }

    /// The FDU has been given to the reassembler.
    const fn release(&mut self) {
        self.held = self.held.saturating_sub(1);
    }

    /// The reset of an Unlock (§4.5.3): no FDU is held.
    const fn reset(&mut self) {
        self.held = 0;
    }
}

/// The uplink receive chain of the flight node: one VC with MAPs 0 and 1 (§4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameUplink {
    /// The codec's view of the channel: SCID, VC 0, MAPs 0 and 1 (APP-026, APP-041).
    table: VcTable<1, 2>,
    vc: TcVcId,
    receiver: FrameReceiver<Tc, 1>,
    farm: Farm1,
    reassembler: MapReassembler<Tc, 2, ASSEMBLY>,
    /// The configuration of `reassembler`, to purge it on an Unlock (D4 c).
    reassembler_config: MapReassemblerConfig<Tc, 2>,
    delivery: DeliveryBuffer,
    /// At the `cop1` level the COP-1 events and the purge apply (APP-146, APP-149).
    cop1: bool,
}

impl FrameUplink {
    /// The chain configured from the scenario constants of `generated::tc` and
    /// `generated::cop1` (APP-136): FARM-1 with W = 8 and retransmission allowed (APP-044).
    ///
    /// # Errors
    /// `InvalidConfig` if a scenario value is not a valid parameter, which the generated
    /// constants rule out.
    pub fn new() -> Result<FrameUplink> {
        FrameUplink::build(false)
    }

    /// The chain at the `cop1` level (A3 on): as [`FrameUplink::new`], with the COP-1 events
    /// of §7.4 (210, 211) and the purge of an Unlock (APP-146).
    ///
    /// # Errors
    /// As [`FrameUplink::new`].
    pub fn new_cop1() -> Result<FrameUplink> {
        FrameUplink::build(true)
    }

    fn build(cop1: bool) -> Result<FrameUplink> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let scid = TcSpacecraftId::new(SPACECRAFT_ID).ok_or(bad)?;
        let vc = TcVcId::new(tc::VCID).ok_or(bad)?;
        let frame_len = usize::from(tc::MAX_FRAME_LENGTH);
        let map = |id: u8| {
            TcMapId::new(id)
                .map(|id| MapConfig::new(scid, vc, id, SecurityLayout::NONE))
                .ok_or(bad)
        };
        let header = if tc::SEGMENT_HEADER {
            SegmentHeaderUse::Present
        } else {
            SegmentHeaderUse::Absent(SecurityLayout::NONE)
        };
        let physical = PhysicalChannelConfig::new(frame_len, tc::FECF)?;
        let table = VcTable::new(
            physical,
            &[VcConfig::new(scid, vc, frame_len, header)?],
            &[map(tc::MAP_COMMANDS)?, map(tc::MAP_TABLE_UPLOAD)?],
        )?;
        let receiver = FrameReceiver::new(FrameReceiverConfig::new(
            TcChannel::new(physical),
            &[VcEntry::new(vc, TcVc::SERVED)],
        )?);
        let farm = Farm1::new(
            Farm1Config {
                vc_id: vc,
                status: ClcwStatus::new(cop1::FARM_STATUS_FIELD).ok_or(bad)?,
                window: Farm1Window::retransmission_allowed(u16::from(cop1::W))?,
            },
            FrameSeqNum::new(cop1::FARM_INITIAL_VR),
        );
        let max_packet = usize::from(tc::MAX_PACKET_LENGTH);
        let packets = |id: u8| {
            TcMapId::new(id)
                .ok_or(bad)
                .and_then(|id| ReassemblerMap::packets(vc, id, max_packet))
        };
        let reassembler_config = MapReassemblerConfig::new(
            TcReassemblerParams::with_segment_header(),
            &[packets(tc::MAP_COMMANDS)?, packets(tc::MAP_TABLE_UPLOAD)?],
        )?;
        let reassembler = MapReassembler::new(reassembler_config)?;
        Ok(FrameUplink {
            table,
            vc,
            receiver,
            farm,
            reassembler,
            reassembler_config,
            delivery: DeliveryBuffer::empty(),
            cop1,
        })
    }

    /// The CLCW FARM-1 reports now (232.1 §6.3.2.4; APP-134): the downlink converts it with
    /// `Clcw::to_ocf` and sets it into the OCF slot of the next frame (§4.4.1).
    #[must_use]
    pub const fn clcw(&self) -> Clcw {
        self.farm.clcw()
    }

    /// Handles one delivered TC frame: the events of APP-139 go to `out`, and each packet the
    /// reassembler delivers goes to `deliver`, in order (APP-131).
    pub fn accept(
        &mut self,
        now: Instant,
        octets: &[u8],
        out: &mut dyn Output,
        deliver: &mut dyn FnMut(&[u8], &mut dyn Output),
    ) {
        let now = CoreInstant::from_micros(now.as_micros());
        let validated = match check_frame(octets, &self.table, DecodeMode::Strict) {
            Ok(validated) => validated.into_view(),
            Err(rejection) => {
                // The FECF failure is the caller's discard (LNK-080). A rejected frame (232.0
                // §4.4.9.3.2) is FARM-1's event E9, which discards it and changes nothing in
                // any state (232.1 Table 6-1).
                let fecf = rejection.reason() == Reason::FecfMismatch;
                self.reject_frame(now, fecf, rejection.error().kind(), out);
                return;
            }
        };
        let plain = match validated.into_frame().into_plain(NoSecurity) {
            Ok(plain) => plain,
            Err(e) => return self.reject_frame(now, false, e.kind(), out),
        };
        let mut area = [0_u8; FRAME_AREA];
        match self.receiver.step(ReceiverInput::Frame(plain), &mut area) {
            Ok(actions) => {
                if !receiver_accepts(actions.iter(), out) {
                    return;
                }
            }
            Err(e) => return self.reject_frame(now, false, e.kind(), out),
        }
        let input = Farm1Input::from_tc(&validated, self.delivery.has_room());
        let (farm, actions) = match self.farm.step(input, now, &mut area) {
            Ok(stepped) => stepped,
            Err(e) => return self.reject_frame(now, false, e.kind(), out),
        };
        self.farm = farm;
        let mut fdu = None;
        let mut discarded = None;
        let mut unlocked = false;
        let mut new_state = None;
        for action in actions.iter() {
            match action {
                Farm1Output::Notify(Farm1Notify::FrameDiscarded { event, .. }) => {
                    discarded = Some(*event);
                }
                Farm1Output::Deliver { data, .. } => fdu = Some(*data),
                Farm1Output::Notify(Farm1Notify::StateChanged { to, .. }) => new_state = Some(*to),
                Farm1Output::Notify(Farm1Notify::Unlocked) => unlocked = true,
            }
        }
        // The order of APP-146: the discard, the Unlock, the reassembly, the state, which is
        // updated after the other outputs of the cell (232.1 §6.3.1).
        if let Some(event) = discarded {
            out.app_event(APP_FRAME_DISCARDED, &[LAYER_FARM1, event_number(event)]);
        }
        if unlocked && self.cop1 {
            out.app_event(APP_FARM1_UNLOCK, &[]);
            self.purge(out);
        }
        if let Some(range) = fdu {
            self.delivery.take();
            let header = validated.segment_header();
            self.reassemble(header, range.slice(&area), out, deliver);
            self.delivery.release();
        }
        if let (Some(state), true) = (new_state, self.cop1) {
            out.app_event(APP_FARM1_STATE, &[state_number(state)]);
        }
    }

    /// The purge of an Unlock (232.1 §6.3.2.2 note, D4 c): the delivery buffer is empty and no
    /// SDU is in progress, so a segment sent after the Unlock does not complete an earlier one.
    fn purge(&mut self, out: &mut dyn Output) {
        self.delivery.reset();
        let mut none = [0_u8; 0];
        let input = ReassemblerInput::Reconfigure(self.reassembler_config);
        if let Err(e) = self.reassembler.step(input, &mut none) {
            reject_packet(out, e.kind());
        }
    }

    /// A frame the strict decode or the validation refuses: `frame_rejected` with the error code
    /// (APP-139), the receiver told of a FECF failure (LNK-080), and FARM-1 given the invalid
    /// frame (E9). Neither discard is mapped to `frame_discarded`.
    ///
    /// One rejected frame raises one `frame_rejected`. The two engine steps run first; neither
    /// writes an octet or fails with these inputs, but if one does, its error is the one raised,
    /// because it says the engine is out of step with the node, which the frame's own error
    /// does not. Otherwise the event carries the error of the frame.
    fn reject_frame(
        &mut self,
        now: CoreInstant,
        fecf: bool,
        mut kind: ErrorKind,
        out: &mut dyn Output,
    ) {
        let mut none = [0_u8; 0];
        if fecf
            && let Err(e) = self.receiver.step(
                ReceiverInput::CallerDiscard(CallerDiscardReason::CrcMismatch),
                &mut none,
            )
        {
            kind = e.kind();
        }
        match self.farm.step(Farm1Input::InvalidFrame, now, &mut none) {
            Ok((farm, _)) => self.farm = farm,
            Err(e) => kind = e.kind(),
        }
        out.app_event(APP_FRAME_REJECTED, &[u64::from(kind.code())]);
    }

    /// The accepted FDU through the MAP reassembler: its events, then its packets (APP-139).
    ///
    /// FARM-1 delivers the user data of the frame; the reassembler reads the Frame Data Unit as
    /// the Segment Header followed by the user data (232.0 §4.1.3.2), so the header octet is
    /// put back in front.
    fn reassemble(
        &mut self,
        header: Option<SegmentHeader>,
        user_data: Option<&[u8]>,
        out: &mut dyn Output,
        deliver: &mut dyn FnMut(&[u8], &mut dyn Output),
    ) {
        let mut fdu = [0_u8; FRAME_AREA];
        let filled = header.zip(user_data).and_then(|(header, user)| {
            let dst = fdu.get_mut(..user.len().saturating_add(1))?;
            for (d, s) in dst
                .iter_mut()
                .zip(core::iter::once(&header.octet()).chain(user))
            {
                *d = *s;
            }
            Some(dst.len())
        });
        let Some(data) = filled.and_then(|len| fdu.get(..len)) else {
            return reject_packet(out, ErrorKind::InternalError);
        };
        let mut area = [0_u8; REASSEMBLY_AREA];
        let input = ReassemblerInput::Fdu(FduIn {
            vc: self.vc,
            meta: (),
            data,
        });
        let actions = match self.reassembler.step(input, &mut area) {
            Ok(actions) => actions,
            Err(e) => return reject_packet(out, e.kind()),
        };
        for action in actions.iter() {
            match action {
                ReassemblerOutput::Event { event, map, .. } => {
                    let code = match event {
                        ReassemblyEvent::SequenceViolation => APP_SEQUENCE_VIOLATION,
                        ReassemblyEvent::UnknownMap => APP_UNKNOWN_MAP,
                        ReassemblyEvent::SduTooLong => APP_SDU_TOO_LONG,
                        // USLP's event; a TC reassembler never raises it.
                        ReassemblyEvent::GapDiscard => continue,
                    };
                    out.app_event(code, &[u64::from(*map)]);
                }
                ReassemblerOutput::Packets { run, count, .. } => {
                    deliver_run(run.slice(&area), *count, out, deliver);
                }
                // Services the TC MAPs do not have, and the discard of a reconfiguration.
                ReassemblerOutput::Sdu { .. }
                | ReassemblerOutput::StreamData { .. }
                | ReassemblerOutput::Discarded { .. } => {}
            }
        }
    }
}

/// Splits the run of `count` packets the reassembler delivered and hands each to `deliver`
/// (LNK-071). A run that does not hold them is `packet_rejected`, which the engine rules out.
fn deliver_run(
    run: Option<&[u8]>,
    count: usize,
    out: &mut dyn Output,
    deliver: &mut dyn FnMut(&[u8], &mut dyn Output),
) {
    let mut rest = run.unwrap_or_default();
    // bound: each pass takes one packet off the run, and `count` is the number of passes.
    for _ in 0..count {
        let split = SpacePacket::peek_length(rest).and_then(|len| {
            rest.split_at_checked(len)
                .ok_or(Error::new(ErrorKind::InternalError))
        });
        match split {
            Ok((packet, tail)) => {
                deliver(packet, out);
                rest = tail;
            }
            Err(e) => return reject_packet(out, e.kind()),
        }
    }
}

/// Maps the receiver's actions for a valid frame: `true` if it gives the frame to FARM-1. A
/// frame it discards is `frame_discarded` of layer 1 (APP-139).
fn receiver_accepts<'a>(
    actions: impl Iterator<Item = &'a ReceiverOutput<Tc>>,
    out: &mut dyn Output,
) -> bool {
    let mut accepted = false;
    for action in actions {
        match action {
            ReceiverOutput::TcFrame { .. } => accepted = true,
            ReceiverOutput::FrameDiscarded { reason, .. } => {
                if let Some(reason) = receiver_reason(*reason) {
                    out.app_event(APP_FRAME_DISCARDED, &[LAYER_RECEIVER, reason]);
                }
                return false;
            }
            // Counts, OCF, FSH, and data fields: TM, AOS, and USLP's, not a TC frame's.
            ReceiverOutput::FrameGap { .. }
            | ReceiverOutput::CountDiscontinuity { .. }
            | ReceiverOutput::LossFlag
            | ReceiverOutput::DataField { .. }
            | ReceiverOutput::Ocf { .. }
            | ReceiverOutput::FshSdu { .. }
            | ReceiverOutput::InsertSdu { .. }
            | ReceiverOutput::ProtocolControl { .. } => {}
        }
    }
    accepted
}

/// The reason code of layer 1 of `frame_discarded` (§7.4); the discards the caller reports are
/// not mapped.
const fn receiver_reason(reason: DiscardReason) -> Option<u64> {
    match reason {
        DiscardReason::UnknownChannel => Some(1),
        DiscardReason::NotServed => Some(2),
        DiscardReason::CrcMismatch
        | DiscardReason::HeaderUncorrectable
        | DiscardReason::Security => None,
    }
}

/// The number of the FARM-1 event, E1 to E11 (232.1 Table 6-1), the reason of layer 2.
const fn event_number(event: Farm1Event) -> u64 {
    match event {
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

/// The number of the FARM-1 state of `farm1_state` (§7.4): 1 Open, 2 Wait, 3 Lockout.
const fn state_number(state: Farm1State) -> u64 {
    match state {
        Farm1State::Open => 1,
        Farm1State::Wait => 2,
        Farm1State::Lockout => 3,
    }
}

/// `packet_rejected` with the catalogue code of `kind` (§7.4, APP-131).
fn reject_packet(out: &mut dyn Output, kind: ErrorKind) {
    out.app_event(APP_PACKET_REJECTED, &[u64::from(kind.code())]);
}

#[cfg(test)]
pub(crate) mod tests;
