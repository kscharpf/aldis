//! Downlink frames: strict TM decoding, the `FrameReceiver`, one `PacketExtractor` per data VC,
//! the CLCW, and the frame-level events.
//!
//! Specification: `docs/reference-application.md` §2.2, §4.1, §4.4.1, §4.4.3, §7.4; APP-011,
//! APP-036, APP-133, APP-136, APP-138. The packets the extractors hand out go to the telemetry
//! path of A1 unchanged ([`Telemetry`]). The OCF is decoded as a CLCW and reported when it
//! changes; it is not given to FOP-1 in A2 (D2 of `docs/milestones/a2-frames.md`).

use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::ocf::Ocf;
use aldis::core::security::NoSecurity;
use aldis::core::{Check, Error, ErrorKind};
use aldis::link::common::ValidPvns;
use aldis::link::extractor::{
    ExtractionError, ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor,
    PacketExtractorConfig, PacketRun,
};
use aldis::link::protocol::{Tm, TmParams};
use aldis::link::receiver::{
    CallerDiscardReason, ChannelScope, DeliveryService, DiscardReason, FrameReceiver,
    FrameReceiverConfig, ReceiverInput, ReceiverOutput, TmChannel, TmContent, TmVc, VcEntry,
};
use aldis::tc::Clcw;
use aldis::tm::{
    DataFieldContent, TmFrame, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig,
};
use refapp_scenario::Output;
use refapp_scenario::generated::{SPACECRAFT_ID, tm};

use crate::GroundError;
use crate::telemetry::Telemetry;

/// Event code `frame_gap` (§7.4).
pub const FRAME_GAP: u16 = 100;
/// Event code `frame_discontinuity` (§7.4).
pub const FRAME_DISCONTINUITY: u16 = 101;
/// Event code `frame_rejected` (§7.4).
pub const FRAME_REJECTED: u16 = 102;
/// Event code `frame_discarded` (§7.4).
pub const FRAME_DISCARDED: u16 = 103;
/// Event code `clcw_changed` (§7.4).
pub const CLCW_CHANGED: u16 = 104;
/// Event code `partial_discarded` (§7.4).
pub const PARTIAL_DISCARDED: u16 = 105;
/// Event code `extraction_error` (§7.4).
pub const EXTRACTION_ERROR: u16 = 106;

/// Scope argument of `frame_gap` and `frame_discontinuity`: the master channel.
const SCOPE_MC: u64 = 0;
/// Scope argument of `frame_gap` and `frame_discontinuity`: a virtual channel.
const SCOPE_VC: u64 = 1;
/// Layer argument of `frame_discarded`: the frame receiver.
const LAYER_RECEIVER: u64 = 1;
/// Reason argument of `frame_discarded`, layer 1: `UnknownChannel`.
const REASON_UNKNOWN_CHANNEL: u64 = 1;
/// Reason argument of `frame_discarded`, layer 1: `NotServed`.
const REASON_NOT_SERVED: u64 = 2;

/// Capacity of the receiver's VC table: VC 0, 1, and 7.
const VC_CAPACITY: usize = 3;
/// The assembly buffer of an extractor: the Maximum Packet Length (§4.1).
const ASSEMBLY: usize = 512;
/// The longest data field an extractor takes: the 211 octets of §4.1.
const FIELD: usize = 211;
/// The output area of an extractor: `max_packet_len - 1` plus the zone (LNK-075).
const EXTRACT_AREA: usize = ASSEMBLY - 1 + FIELD;

/// The extractor of one VC.
type Extractor = PacketExtractor<Tm, ASSEMBLY, FIELD>;

/// The frame-level downlink of the ground node (APP-036).
#[derive(Debug, Clone)]
pub(crate) struct FrameDownlink {
    /// The managed parameters of the master channel (§4.1), for the strict decode.
    channel: TmMasterChannelConfig,
    receiver: FrameReceiver<Tm, VC_CAPACITY>,
    /// The receiver's output area.
    area: Vec<u8>,
    /// The extractors of VC 0 and VC 1 and their output area.
    extraction: Extraction,
    /// The OCF word of the last frame decoded (§7.4 `clcw_changed`).
    ocf_word: u32,
    /// That word as a CLCW, `None` if it is another report or not a valid CLCW.
    clcw: Option<Clcw>,
}

/// One `PacketExtractor` for each of VC 0 and VC 1 (LNK-060…LNK-064).
#[derive(Debug, Clone)]
struct Extraction {
    realtime: Extractor,
    payload: Extractor,
    area: Vec<u8>,
}

impl FrameDownlink {
    /// A downlink that has received no frame, with the parameters of §4.1 taken from the
    /// generated scenario constants (APP-136).
    ///
    /// # Errors
    /// [`GroundError::Setup`] if the constants do not form a configuration.
    pub(crate) fn new() -> Result<FrameDownlink, GroundError> {
        FrameDownlink::build().map_err(GroundError::Setup)
    }

    fn build() -> Result<FrameDownlink, Error> {
        let frame_len = usize::from(tm::FRAME_LENGTH);
        let invalid = || Error::new(ErrorKind::InvalidConfig);
        let scid = SpacecraftId::new(SPACECRAFT_ID).ok_or_else(invalid)?;
        let vc = |id: u8| VcId::<3>::new(id).ok_or_else(invalid);
        let (realtime, payload, idle) = (
            vc(tm::VCID_REALTIME)?,
            vc(tm::VCID_PAYLOAD)?,
            vc(tm::VCID_IDLE)?,
        );
        // The MC_OCF is present, so no VC has an OCF of its own (§4.1).
        let mut channel = TmMasterChannelConfig::new(
            TmPhysicalChannelConfig::new(frame_len, tm::FECF)?,
            scid,
            None,
            true,
        );
        for id in [realtime, payload, idle] {
            channel =
                channel.with_vc(TmVirtualChannelConfig::new(id, DataFieldContent::Packets))?;
        }
        let entry = |id: VcId<3>| -> Result<VcEntry<Tm>, Error> {
            Ok(VcEntry::new(id, TmVc::new(TmContent::Packets, true, None)?))
        };
        let receiver = FrameReceiver::new(FrameReceiverConfig::<Tm, VC_CAPACITY>::new(
            frame_len,
            tm::FECF,
            TmChannel::new(DeliveryService::Mc, DeliveryService::None),
            &[entry(realtime)?, entry(payload)?, entry(idle)?],
        )?);
        let ocf_word = tm::OCF_INITIAL;
        Ok(FrameDownlink {
            area: vec![0; receiver.config().area_len()],
            channel,
            receiver,
            extraction: Extraction::new()?,
            ocf_word,
            clcw: Clcw::decode(ocf_word.to_be_bytes()).ok(),
        })
    }

    /// The CLCW of the last frame decoded, initially the CLCW of `OCF_INITIAL`; `None` if the OCF
    /// of the last frame is not a valid CLCW.
    pub(crate) const fn clcw(&self) -> Option<Clcw> {
        self.clcw
    }

    /// Handles one delivered TM frame, outputting in the order of APP-138.
    pub(crate) fn receive(&mut self, unit: &[u8], telemetry: &mut Telemetry, out: &mut dyn Output) {
        let frame = match self.decode(unit) {
            Ok(frame) => frame,
            Err(e) => {
                // A frame that failed the FECF is counted by the receiver (LNK-080); the
                // discard it returns is not an event.
                if e.kind() == ErrorKind::CrcMismatch
                    && let Err(refused) = self.discard_for_crc()
                {
                    return reject(out, refused.kind());
                }
                return reject(out, e.kind());
            }
        };
        let actions = match self
            .receiver
            .step(ReceiverInput::Frame(frame), &mut self.area)
        {
            Ok(actions) => actions,
            Err(e) => return reject(out, e.kind()),
        };
        for action in actions.iter() {
            match *action {
                ReceiverOutput::FrameGap { scope, missing } => {
                    let (scope, vcid) = scope_args(scope);
                    out.app_event(FRAME_GAP, &[scope, vcid, missing]);
                }
                ReceiverOutput::CountDiscontinuity { scope } => {
                    let (scope, vcid) = scope_args(scope);
                    out.app_event(FRAME_DISCONTINUITY, &[scope, vcid]);
                }
                ReceiverOutput::Ocf { ocf, .. } => self.observe(ocf, out),
                ReceiverOutput::DataField {
                    vc,
                    data,
                    pointer,
                    gap,
                    ..
                } => {
                    let Some(field) = data.slice(&self.area) else {
                        return reject(out, ErrorKind::InternalError);
                    };
                    let handled = self.extraction.run(
                        vc.get(),
                        FieldIn {
                            field,
                            header_pointer: pointer,
                            gap,
                        },
                        telemetry,
                        out,
                    );
                    if let Err(e) = handled {
                        return reject(out, e.kind());
                    }
                }
                ReceiverOutput::FrameDiscarded { reason, .. } => discarded(reason, out),
                // The other actions are for services this channel does not configure (no FSH,
                // no Insert Zone, no frame loss signal) or for other protocols.
                ReceiverOutput::LossFlag
                | ReceiverOutput::FshSdu { .. }
                | ReceiverOutput::InsertSdu { .. }
                | ReceiverOutput::TcFrame { .. }
                | ReceiverOutput::ProtocolControl { .. } => {}
            }
        }
    }

    /// The strict decode and the check against the managed parameters (TM-001…TM-011).
    fn decode<'a>(
        &self,
        unit: &'a [u8],
    ) -> Result<<Tm as aldis::link::receiver::ReceiveProtocol>::Plain<'a>, Error> {
        TmFrame::decode_with(unit, self.channel.pc(), &self.channel)?
            .check(&self.channel)?
            .into_frame()
            .into_plain(NoSecurity)
    }

    /// Tells the receiver that the caller discarded a frame for its FECF (LNK-080). The
    /// `FrameDiscarded` action it returns has no event (APP-138).
    ///
    /// # Errors
    /// The receiver's refusal of the input.
    fn discard_for_crc(&mut self) -> Result<(), Error> {
        self.receiver
            .step(
                ReceiverInput::CallerDiscard(CallerDiscardReason::CrcMismatch),
                &mut self.area,
            )
            .map(|_| ())
    }

    /// Reports a change of the OCF word and keeps it as a CLCW (§4.4.1; D2 of the plan: not given to FOP-1).
    fn observe(&mut self, ocf: Ocf, out: &mut dyn Output) {
        let word = u32::from_be_bytes(ocf.to_octets());
        self.clcw = match ocf {
            Ocf::Clcw(report) => Clcw::decode_report(report).ok(),
            Ocf::ProjectSpecific(_) | Ocf::SdlsFsr(_) => None,
        };
        if word != self.ocf_word {
            self.ocf_word = word;
            out.app_event(CLCW_CHANGED, &[u64::from(word)]);
        }
    }
}

impl Extraction {
    fn new() -> Result<Extraction, Error> {
        let config = PacketExtractorConfig::<Tm>::new(
            TmParams,
            ValidPvns::from_versions(&[tm::PACKET_VERSION])?,
            usize::from(tm::MAX_PACKET_LENGTH),
        )?
        .with_delivery(tm::DELIVER_INCOMPLETE_PACKETS, tm::DELIVER_IDLE_PACKETS);
        Ok(Extraction {
            realtime: Extractor::new(config)?,
            payload: Extractor::new(config)?,
            area: vec![0; EXTRACT_AREA],
        })
    }

    /// Gives a data field to the extractor of its VC and hands the packets on, with the
    /// extractor's events before them in the engine's order (APP-138).
    ///
    /// A data frame on another VC (the VC of the OID frames) is not served.
    ///
    /// # Errors
    /// The extractor's refusal of the field.
    fn run(
        &mut self,
        vcid: u8,
        field: FieldIn<'_>,
        telemetry: &mut Telemetry,
        out: &mut dyn Output,
    ) -> Result<(), Error> {
        let extractor = match vcid {
            tm::VCID_REALTIME => &mut self.realtime,
            tm::VCID_PAYLOAD => &mut self.payload,
            _ => {
                // A data frame on the OID VC: the VC carries OID frames only (§4.1), so the
                // receiver delivers the field and the node discards it (APP-138).
                out.app_event(FRAME_DISCARDED, &[LAYER_RECEIVER, REASON_NOT_SERVED]);
                return Ok(());
            }
        };
        let actions = extractor.step(ExtractorInput::DataField(field), &mut self.area)?;
        for action in actions.iter() {
            match *action {
                ExtractorOutput::Packets { run, .. } => {
                    let packets = PacketRun::new(run, &self.area)
                        .ok_or_else(|| Error::new(ErrorKind::InternalError))?;
                    for packet in packets {
                        telemetry.receive(packet, out);
                    }
                }
                ExtractorOutput::Discarded { octets } => {
                    let lost =
                        u64::try_from(octets).map_err(|_| Error::new(ErrorKind::OutOfRange))?;
                    out.app_event(PARTIAL_DISCARDED, &[u64::from(vcid), lost]);
                }
                ExtractorOutput::Error(e) => {
                    let kind = match e {
                        ExtractionError::PacketTooLong => 1,
                        ExtractionError::InvalidPvn => 2,
                        ExtractionError::LengthInconsistent => 3,
                    };
                    out.app_event(EXTRACTION_ERROR, &[u64::from(vcid), kind]);
                }
                // The scenario does not ask for incomplete packets (APP-136).
                ExtractorOutput::IncompletePacket { .. } => {
                    return Err(Error::new(ErrorKind::InternalError));
                }
            }
        }
        Ok(())
    }
}

/// `frame_rejected` with the catalogue code of `kind` (§7.4).
fn reject(out: &mut dyn Output, kind: ErrorKind) {
    out.app_event(FRAME_REJECTED, &[u64::from(kind.code())]);
}

/// The scope and VCID arguments of `frame_gap` and `frame_discontinuity`.
fn scope_args(scope: ChannelScope<VcId<3>>) -> (u64, u64) {
    match scope {
        ChannelScope::MasterChannel => (SCOPE_MC, 0),
        ChannelScope::VirtualChannel(vc) => (SCOPE_VC, u64::from(vc.get())),
    }
}

/// `frame_discarded` for a frame the receiver discards. The reasons the caller supplies (FECF,
/// FHEC, SDLS) are not outputs of a frame input and have no event (APP-138).
fn discarded(reason: DiscardReason, out: &mut dyn Output) {
    let reason = match reason {
        DiscardReason::UnknownChannel => REASON_UNKNOWN_CHANNEL,
        DiscardReason::NotServed => REASON_NOT_SERVED,
        DiscardReason::CrcMismatch
        | DiscardReason::HeaderUncorrectable
        | DiscardReason::Security => return,
    };
    out.app_event(FRAME_DISCARDED, &[LAYER_RECEIVER, reason]);
}

#[cfg(test)]
mod tests;
