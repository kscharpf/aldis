//! Strict decoding of a link unit by level (docs/reference-application.md §6.1, APP-086).
//!
//! At A2 on a link unit is a frame: a TM frame on the downlink, a TC frame on the uplink. The
//! trace tests decode every `unit_sent` with the strict codecs; this module holds the function
//! and the packets inside the frames, which the check over the golden traces uses (WP-6).

use aldis::core::security::SecurityLayout;
use aldis::link::common::ValidPvns;
use aldis::link::extractor::{
    ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor, PacketExtractorConfig, PacketRun,
};
use aldis::link::protocol::{Tc, Tm, TmParams};
use aldis::link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, TcReassemblerParams,
};
use aldis::spp::SpacePacket;
use aldis::tc::{
    ControlCommand, FrameType, MapConfig, PhysicalChannelConfig, SegmentHeaderUse, TcFrame,
    VcConfig, VcTable,
};
use aldis::tm::{TmFrame, TmPhysicalChannelConfig};

use crate::scenario::{Level, Scenario};
use crate::trace::Dir;

/// A strictly decoded frame: what the nodes and the trace checks read of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Virtual channel ID.
    pub vcid: u8,
    /// The data field (TM: the packet zone; TC: the Segment Header and the FDU).
    pub data: Vec<u8>,
    /// TM: the First Header Pointer of the primary header, as the extractor takes it.
    pub pointer: Option<u16>,
    /// TC: the Segment Header octet of a Type-D frame on a VC that has one.
    pub segment_header: Option<u8>,
    /// TC: the frame type, the Bypass and Control Command Flags (232.0 Table 4-1).
    pub frame_type: Option<FrameType>,
    /// TC: the Frame Sequence Number N(S) of the Primary Header (232.0 §4.1.2.8).
    pub sequence: Option<u8>,
    /// TC: the Control Command of a Type-BC frame (232.0 §4.1.3.3).
    pub control_command: Option<ControlCommand>,
}

impl DecodedFrame {
    /// The frame is a Type-BC frame: a control command, no packets.
    #[must_use]
    pub const fn is_control(&self) -> bool {
        matches!(self.frame_type, Some(FrameType::Bc))
    }
}

/// Decodes a link unit as a TM (`down`) or TC (`up`) frame, strictly (APP-086).
///
/// A unit of a [`Level::Packets`] case is a Space Packet ([`crate::trace::decode_strict`]).
///
/// The frame types a TC frame may have depend on the level (APP-043, APP-090): at
/// [`Level::Frames`] the ground node sends Type-BD frames only; at [`Level::Cop1`] it sends
/// Type-AD, Type-BD, and Type-BC frames (232.0 Table 4-1).
///
/// The frame must carry the Spacecraft ID of the scenario (APP-026). A TM frame must have
/// exactly the configured frame length; a TC frame is checked only against the maximum frame
/// length (its Frame Length field gives its length, TC-007).
///
/// # Errors
/// If `level` is [`Level::Packets`], the managed parameters of the scenario do not form a
/// configuration, or the codec rejects the frame (FECF, lengths, fields), the frame has
/// another Spacecraft ID, or a TC frame has a type its level does not use.
pub fn decode_frame(
    scenario: &Scenario,
    level: Level,
    dir: Dir,
    octets: &[u8],
) -> Result<DecodedFrame, String> {
    if level == Level::Packets {
        return Err("a unit of a packet-level case is a Space Packet, not a frame".to_owned());
    }
    let scid = scenario.spacecraft_id.get();
    match dir {
        Dir::Down => {
            let tm = &scenario.tm;
            let pc = TmPhysicalChannelConfig::new(usize::from(tm.frame_length.get()), tm.fecf)
                .map_err(|e| format!("TM channel of the scenario: {e:?}"))?;
            let frame = TmFrame::decode(octets, &pc)
                .map_err(|e| format!("not a strict TM frame: {e:?}"))?;
            if frame.scid().get() != scid {
                return Err(format!(
                    "TM frame has Spacecraft ID {}, expected {scid}",
                    frame.scid().get()
                ));
            }
            Ok(DecodedFrame {
                vcid: frame.vcid().get(),
                data: frame.data_field().to_vec(),
                pointer: Some(frame.header().first_header_pointer.get()),
                segment_header: None,
                frame_type: None,
                sequence: None,
                control_command: None,
            })
        }
        Dir::Up => {
            let tc = &scenario.tc;
            let max = usize::from(tc.max_frame_length.get());
            let id_err =
                |what: &str, e: &dyn std::fmt::Debug| format!("TC {what} of the scenario: {e:?}");
            let out_of_range = |what: &str| format!("TC {what} of the scenario is out of range");
            let scid_t =
                aldis::tc::TcSpacecraftId::new(scid).ok_or_else(|| out_of_range("SCID"))?;
            let vc_t = aldis::tc::TcVcId::new(tc.vcid.get()).ok_or_else(|| out_of_range("VCID"))?;
            let map = |id: u8| {
                aldis::tc::TcMapId::new(id)
                    .map(|m| MapConfig::new(scid_t, vc_t, m, SecurityLayout::NONE))
                    .ok_or_else(|| out_of_range("MAP ID"))
            };
            let physical =
                PhysicalChannelConfig::new(max, tc.fecf).map_err(|e| id_err("channel", &e))?;
            // MAPs exist only on a VC with a Segment Header (232.0 §4.1.3.2.1.4, Table 5-3). Blocking
            // and segmentation are parameters of the segmenter and reassembler, not of the
            // frame codec.
            let (header, maps) = if tc.segment_header {
                (
                    SegmentHeaderUse::Present,
                    vec![map(tc.map_commands.get())?, map(tc.map_table_upload.get())?],
                )
            } else {
                (SegmentHeaderUse::Absent(SecurityLayout::NONE), Vec::new())
            };
            let vc = VcConfig::new(scid_t, vc_t, max, header).map_err(|e| id_err("VC", &e))?;
            let table: VcTable<1, 2> =
                VcTable::new(physical, &[vc], &maps).map_err(|e| id_err("channels", &e))?;
            let frame = TcFrame::decode(octets, &table)
                .map_err(|e| format!("not a strict TC frame: {e:?}"))?;
            if frame.spacecraft_id() != scid_t {
                return Err(format!(
                    "TC frame has Spacecraft ID {}, expected {scid}",
                    frame.spacecraft_id().get()
                ));
            }
            let frame_type = frame.frame_type();
            if level == Level::Frames && frame_type != FrameType::Bd {
                return Err(format!(
                    "TC frame of type {frame_type:?} in a frame-level case, which has Type-BD only"
                ));
            }
            Ok(DecodedFrame {
                vcid: frame.vc_id().get(),
                data: frame.data_field().to_vec(),
                pointer: None,
                segment_header: frame.segment_header().map(aldis::tc::SegmentHeader::octet),
                frame_type: Some(frame_type),
                sequence: Some(frame.sequence_number().get()),
                control_command: frame.control_command(),
            })
        }
    }
}

/// The extractor of a downlink VC (the sizes of the ground node: §4.1).
type Extractor = PacketExtractor<Tm, ASSEMBLY, FIELD>;
/// The assembly buffer of an extractor: the Maximum Packet Length (§4.1).
const ASSEMBLY: usize = 512;
/// The longest data field an extractor takes (§4.1).
const FIELD: usize = 211;
/// The output area of an extractor (LNK-075).
const EXTRACT_AREA: usize = ASSEMBLY - 1 + FIELD;
/// The output area of the reassembler: the SDU held plus the user data of one frame (LNK-075).
const REASSEMBLY_AREA: usize = ASSEMBLY + 1024;

/// The packets inside the frames of a trace (APP-086; WP-6 of the A2 plan).
///
/// The engines of the nodes run over the frames of one direction in trace order as a clean link,
/// and every complete packet they hand out is decoded strictly.
///
/// Downlink: one `PacketExtractor` for each of VC 0 and VC 1 (LNK-060). Uplink: the
/// `MapReassembler` with the two MAPs of the scenario (LNK-070). Idle packets are dropped, as the
/// extractor does (LNK-063). A partial packet at the end of the trace is no fault.
pub struct FramedPackets {
    realtime: Extractor,
    payload: Extractor,
    extract_area: Vec<u8>,
    /// The output area of the reassembler, kept across frames like `extract_area` (#484).
    reassembly_area: Vec<u8>,
    reassembler: MapReassembler<Tc, 2, ASSEMBLY>,
    vc: aldis::tc::TcVcId,
}

impl FramedPackets {
    /// The extractors and the reassembler for the managed parameters of `scenario`.
    ///
    /// # Errors
    /// If the scenario does not form a configuration.
    pub fn new(scenario: &Scenario) -> Result<FramedPackets, String> {
        let bad = |e: aldis::core::Error| format!("packet engines of the scenario: {e:?}");
        let out_of_range = |what: &str| format!("TC {what} of the scenario is out of range");
        let config = PacketExtractorConfig::<Tm>::new(
            TmParams,
            ValidPvns::from_versions(&[refapp_scenario::generated::tm::PACKET_VERSION])
                .map_err(bad)?,
            ASSEMBLY,
        )
        .map_err(bad)?;
        let tc = &scenario.tc;
        if !tc.segment_header {
            return Err(
                "a TC channel without a Segment Header has no MAPs to reassemble".to_owned(),
            );
        }
        let vc = aldis::tc::TcVcId::new(tc.vcid.get()).ok_or_else(|| out_of_range("VCID"))?;
        let map = |id: u8| {
            let id = aldis::tc::TcMapId::new(id).ok_or_else(|| out_of_range("MAP ID"))?;
            ReassemblerMap::packets(vc, id, ASSEMBLY).map_err(bad)
        };
        let reassembler = MapReassembler::new(
            MapReassemblerConfig::new(
                TcReassemblerParams::with_segment_header(),
                &[map(tc.map_commands.get())?, map(tc.map_table_upload.get())?],
            )
            .map_err(bad)?,
        )
        .map_err(bad)?;
        Ok(FramedPackets {
            realtime: Extractor::new(config).map_err(bad)?,
            payload: Extractor::new(config).map_err(bad)?,
            extract_area: vec![0; EXTRACT_AREA],
            reassembly_area: vec![0; REASSEMBLY_AREA],
            reassembler,
            vc,
        })
    }

    /// Gives the next frame of `dir` to the engines and decodes the packets they complete.
    /// Returns how many it decoded.
    ///
    /// # Errors
    /// If an engine refuses the frame or reports an error, a discard, or an incomplete packet
    /// (a clean link has none), or a packet is not a strict Space Packet.
    pub fn take(&mut self, dir: Dir, frame: &DecodedFrame) -> Result<usize, String> {
        match dir {
            Dir::Down => self.take_down(frame),
            Dir::Up => self.take_up(frame),
        }
    }

    fn take_down(&mut self, frame: &DecodedFrame) -> Result<usize, String> {
        let extractor = match frame.vcid {
            0 => &mut self.realtime,
            1 => &mut self.payload,
            // The OID frames of VC 7 carry idle data only (§4.1).
            _ => return Ok(0),
        };
        let field = FieldIn {
            field: &frame.data,
            header_pointer: frame.pointer,
            gap: false,
        };
        let actions = extractor
            .step(ExtractorInput::DataField(field), &mut self.extract_area)
            .map_err(|e| format!("extractor of VC {}: {e:?}", frame.vcid))?;
        let mut packets = 0;
        for action in actions.iter() {
            match *action {
                ExtractorOutput::Packets { run, .. } => {
                    let run = PacketRun::new(run, &self.extract_area)
                        .ok_or("extractor run outside its area")?;
                    for packet in run {
                        strict_packet(packet, "packet in a TM frame")?;
                        packets += 1;
                    }
                }
                ExtractorOutput::Discarded { octets } => {
                    return Err(format!(
                        "VC {}: {octets} octet(s) of a partial packet discarded",
                        frame.vcid
                    ));
                }
                ExtractorOutput::Error(e) => {
                    return Err(format!("extraction error on VC {}: {e:?}", frame.vcid));
                }
                ExtractorOutput::IncompletePacket { .. } => {
                    return Err(format!("incomplete packet on VC {}", frame.vcid));
                }
            }
        }
        Ok(packets)
    }

    fn take_up(&mut self, frame: &DecodedFrame) -> Result<usize, String> {
        if frame.is_control() {
            return Ok(0);
        }
        let header = frame
            .segment_header
            .ok_or("a TC data frame without a Segment Header")?;
        let mut fdu = Vec::with_capacity(frame.data.len() + 1);
        fdu.push(header);
        fdu.extend_from_slice(&frame.data);
        let input = ReassemblerInput::Fdu(FduIn {
            vc: self.vc,
            meta: (),
            data: &fdu,
        });
        let actions = self
            .reassembler
            .step(input, &mut self.reassembly_area)
            .map_err(|e| format!("reassembler: {e:?}"))?;
        let mut packets = 0;
        for action in actions.iter() {
            match action {
                ReassemblerOutput::Packets { run, .. } => {
                    let run = PacketRun::new(*run, &self.reassembly_area)
                        .ok_or("reassembler run outside its area")?;
                    for packet in run {
                        strict_packet(packet, "packet in a TC frame")?;
                        packets += 1;
                    }
                }
                ReassemblerOutput::Event { event, map, .. } => {
                    return Err(format!("reassembly event {event:?} on MAP {map}"));
                }
                ReassemblerOutput::Sdu { .. }
                | ReassemblerOutput::StreamData { .. }
                | ReassemblerOutput::Discarded { .. } => {
                    return Err("reassembler output of a service the MAPs do not have".to_owned());
                }
            }
        }
        Ok(packets)
    }
}

/// `packet` is a strict Space Packet that fills its octets.
fn strict_packet(packet: &[u8], what: &str) -> Result<(), String> {
    let decoded = SpacePacket::decode(packet)
        .map_err(|e| format!("{what} is not a strict Space Packet: {e:?}"))?;
    if decoded.len() == packet.len() {
        Ok(())
    } else {
        Err(format!("{what} has octets after its packet"))
    }
}
