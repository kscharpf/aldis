//! Downlink frames: strict TM, AOS, or USLP decoding, the `FrameReceiver`, one `PacketExtractor` per data
//! VC, the CLCW, and the frame-level events.
//!
//! Specification: `docs/reference-application.md` §2.2, §4.1, §4.3.1, §4.3.2, §4.4.1, §4.4.3, §7.4;
//! APP-011, APP-036, APP-133, APP-136, APP-138, APP-169…APP-180. The downlink is generic over the
//! protocol type ([`DownlinkProtocol`]: `Tm`, `Aos`, `Uslp`; D3 of the A4 plan), with one copy of the
//! receiver, extractor, and event logic. The packets the extractors hand out go to the telemetry
//! path of A1 unchanged ([`Telemetry`]). The OCF is decoded as a CLCW and reported when it
//! changes; at the `cop1` level (A3) it also goes to FOP-1 (D3 a).

use aldis::core::ocf::Ocf;
use aldis::core::{Error, ErrorKind};
use aldis::link::extractor::{
    ExtractionError, ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor, PacketRun,
};
use aldis::link::protocol::{Aos, Tm, Uslp};
use aldis::link::receiver::{
    CallerDiscardReason, ChannelScope, DiscardReason, FrameReceiver, ReceiverInput, ReceiverOutput,
};
use aldis::tc::Clcw;
use refapp_scenario::{Instant, Output, Variant};

use crate::GroundError;
use crate::frames_up::Uplink;
use crate::telemetry::Telemetry;

mod protocol;

pub(crate) use protocol::DownlinkProtocol;
use protocol::VC_CAPACITY;

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

/// The assembly buffer of an extractor: the Maximum Packet Length (§4.1, APP-172).
const ASSEMBLY: usize = 512;
/// The longest data field an extractor takes: the 211 octets of a TM frame (§4.1); the `M_PDU`
/// of an AOS frame is 209 (§4.3.1), the TFDF of a USLP frame 209 (§4.3.2).
const FIELD: usize = 211;
/// The output area of an extractor: `max_packet_len - 1` plus the zone (LNK-075).
const EXTRACT_AREA: usize = ASSEMBLY - 1 + FIELD;

/// The extractor of one VC.
type Extractor<P> = PacketExtractor<P, ASSEMBLY, FIELD>;

/// The frame-level downlink of the ground node for the variant `P` (APP-036, APP-174).
#[derive(Debug, Clone)]
pub(crate) struct FrameDownlink<P: DownlinkProtocol> {
    /// The managed parameters of the channel (§4.1, §4.3.1), for the strict decode.
    channel: P::Managed,
    receiver: FrameReceiver<P, VC_CAPACITY>,
    /// The receiver's output area.
    area: Vec<u8>,
    /// The extractors of the data VCs and their output area.
    extraction: Extraction<P>,
    /// The OCF word of the last frame decoded (§7.4 `clcw_changed`).
    ocf_word: u32,
    /// That word as a CLCW, `None` if it is another report or not a valid CLCW.
    clcw: Option<Clcw>,
}

/// The downlink of the variant a node was made for: the closed set of the protocol types.
#[derive(Debug, Clone)]
pub(crate) enum Downlink {
    /// The TM downlink (§4.1).
    Tm(Box<FrameDownlink<Tm>>),
    /// The AOS downlink (§4.3.1).
    Aos(Box<FrameDownlink<Aos>>),
    /// The USLP downlink (§4.3.2).
    Uslp(Box<FrameDownlink<Uslp>>),
}

impl Downlink {
    /// The downlink of `variant`.
    ///
    /// # Errors
    /// [`GroundError::Setup`] if the constants do not form a configuration.
    pub(crate) fn new(variant: Variant) -> Result<Self, GroundError> {
        match variant {
            Variant::TmTc => FrameDownlink::new().map(|down| Self::Tm(Box::new(down))),
            Variant::Aos => FrameDownlink::new().map(|down| Self::Aos(Box::new(down))),
            Variant::Uslp => FrameDownlink::new().map(|down| Self::Uslp(Box::new(down))),
        }
    }

    /// The CLCW of the last frame decoded (see [`FrameDownlink::clcw`]).
    pub(crate) const fn clcw(&self) -> Option<Clcw> {
        match self {
            Self::Tm(down) => down.clcw(),
            Self::Aos(down) => down.clcw(),
            Self::Uslp(down) => down.clcw(),
        }
    }

    /// Handles one delivered frame (see [`FrameDownlink::receive`]).
    pub(crate) fn receive(
        &mut self,
        unit: &[u8],
        telemetry: &mut Telemetry,
        uplink: Option<&mut Uplink>,
        now: Instant,
        out: &mut dyn Output,
    ) {
        match self {
            Self::Tm(down) => down.receive(unit, telemetry, uplink, now, out),
            Self::Aos(down) => down.receive(unit, telemetry, uplink, now, out),
            Self::Uslp(down) => down.receive(unit, telemetry, uplink, now, out),
        }
    }
}

/// One `PacketExtractor` for each data VC (LNK-060…LNK-064): VC 0 and VC 1, and in a variant
/// whose idle VC has an extractor (AOS, APP-138) that VC.
#[derive(Debug, Clone)]
struct Extraction<P: DownlinkProtocol> {
    realtime: Extractor<P>,
    payload: Extractor<P>,
    /// The idle VC's extractor: `Some` if and only if `P::IDLE_VC_EXTRACTED`.
    idle: Option<Extractor<P>>,
    area: Vec<u8>,
}

impl<P: DownlinkProtocol> FrameDownlink<P> {
    /// A downlink that has received no frame, with the parameters of §4.1 or §4.3.1 taken from the
    /// generated scenario constants (APP-136, APP-189).
    ///
    /// # Errors
    /// [`GroundError::Setup`] if the constants do not form a configuration.
    pub(crate) fn new() -> Result<Self, GroundError> {
        Self::build().map_err(GroundError::Setup)
    }

    fn build() -> Result<Self, Error> {
        let receiver = FrameReceiver::new(P::receiver_config()?);
        let ocf_word = P::OCF_INITIAL;
        Ok(Self {
            area: vec![0; receiver.config().area_len()],
            channel: P::managed()?,
            receiver,
            extraction: Extraction::new()?,
            ocf_word,
            clcw: Clcw::decode(ocf_word.to_be_bytes()).ok(),
        })
    }

    /// The CLCW of the last frame decoded, initially the CLCW of the initial OCF word; `None` if
    /// the OCF of the last frame is not a valid CLCW.
    pub(crate) const fn clcw(&self) -> Option<Clcw> {
        self.clcw
    }

    /// Handles one delivered frame, outputting in the order of APP-138 (and APP-144 at `cop1`).
    ///
    /// At the `cop1` level `uplink` gets the OCF of the frame as FOP-1's `Clcw` right after
    /// `clcw_changed` (D3 a, APP-144): of every frame, whatever its VC (APP-173).
    pub(crate) fn receive(
        &mut self,
        unit: &[u8],
        telemetry: &mut Telemetry,
        mut uplink: Option<&mut Uplink>,
        now: Instant,
        out: &mut dyn Output,
    ) {
        let frame = match P::decode(&self.channel, unit) {
            Ok(frame) => frame,
            Err(e) => {
                // A frame that failed the FECF or the FHEC is counted by the receiver (LNK-080);
                // the discard it returns is not an event.
                if let Some(reason) = caller_discard(e.kind())
                    && let Err(refused) = self.discard_by_caller(reason)
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
                ReceiverOutput::Ocf { ocf, .. } => {
                    let word = self.observe(ocf, out);
                    if let Some(uplink) = uplink.as_deref_mut() {
                        uplink.clcw(word, now, out);
                    }
                }
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
                        vc.into(),
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

    /// Tells the receiver that the caller discarded a frame (LNK-080). The `FrameDiscarded`
    /// action it returns has no event (APP-138).
    ///
    /// # Errors
    /// The receiver's refusal of the input.
    fn discard_by_caller(&mut self, reason: CallerDiscardReason) -> Result<(), Error> {
        self.receiver
            .step(ReceiverInput::CallerDiscard(reason), &mut self.area)
            .map(|_| ())
    }

    /// Reports a change of the OCF word and keeps it as a CLCW (§4.4.1); returns the word.
    fn observe(&mut self, ocf: Ocf, out: &mut dyn Output) -> u32 {
        let word = u32::from_be_bytes(ocf.to_octets());
        self.clcw = match ocf {
            Ocf::Clcw(report) => Clcw::decode_report(report).ok(),
            Ocf::ProjectSpecific(_) | Ocf::SdlsFsr(_) => None,
        };
        if word != self.ocf_word {
            self.ocf_word = word;
            out.app_event(CLCW_CHANGED, &[u64::from(word)]);
        }
        word
    }
}

impl<P: DownlinkProtocol> Extraction<P> {
    fn new() -> Result<Self, Error> {
        let config = P::extractor_config()?;
        let idle = if P::IDLE_VC_EXTRACTED {
            Some(Extractor::new(config)?)
        } else {
            None
        };
        Ok(Self {
            realtime: Extractor::new(config)?,
            payload: Extractor::new(config)?,
            idle,
            area: vec![0; EXTRACT_AREA],
        })
    }

    /// Gives a data field to the extractor of its VC and hands the packets on, with the
    /// extractor's events before them in the engine's order (APP-138).
    ///
    /// A data frame on the idle VC is not served where that VC has no extractor (TM: the VC
    /// carries OID frames only, §4.1); where it has one (AOS), its idle `M_PDU`s give no packet
    /// and no event (APP-138, APP-174).
    ///
    /// # Errors
    /// The extractor's refusal of the field.
    fn run(
        &mut self,
        vcid: u8,
        field: FieldIn<'_, P>,
        telemetry: &mut Telemetry,
        out: &mut dyn Output,
    ) -> Result<(), Error> {
        let extractor = if vcid == P::VCID_REALTIME {
            &mut self.realtime
        } else if vcid == P::VCID_PAYLOAD {
            &mut self.payload
        } else if let Some(idle) = self.idle.as_mut().filter(|_| vcid == P::VCID_IDLE) {
            idle
        } else {
            // A data frame on the OID VC: the VC carries OID frames only (§4.1), so the
            // receiver delivers the field and the node discards it (APP-138).
            out.app_event(FRAME_DISCARDED, &[LAYER_RECEIVER, REASON_NOT_SERVED]);
            return Ok(());
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

/// What the receiver is told of a frame the strict decode refused (LNK-080): a FECF or an FHEC
/// failure; no other refusal is a discard the receiver counts.
fn caller_discard(kind: ErrorKind) -> Option<CallerDiscardReason> {
    if kind == ErrorKind::CrcMismatch {
        Some(CallerDiscardReason::CrcMismatch)
    } else if kind == ErrorKind::HeaderUncorrectable {
        Some(CallerDiscardReason::HeaderUncorrectable)
    } else {
        None
    }
}

/// The scope and VCID arguments of `frame_gap` and `frame_discontinuity`. A scope of the master
/// channel comes from TM only: AOS and USLP have no master-channel count (APP-138, D10).
fn scope_args<V: Into<u8>>(scope: ChannelScope<V>) -> (u64, u64) {
    match scope {
        ChannelScope::MasterChannel => (SCOPE_MC, 0),
        ChannelScope::VirtualChannel(vc) => (SCOPE_VC, u64::from(vc.into())),
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
#[cfg(test)]
mod tests_aos;
#[cfg(test)]
mod tests_cop1;
#[cfg(test)]
mod tests_uslp;
