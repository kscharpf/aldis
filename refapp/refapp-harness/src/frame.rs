//! Strict decoding of a link unit by level (docs/reference-application.md §6.1, APP-086).
//!
//! At A2 on a link unit is a frame: a TM frame on the downlink, a TC frame on the uplink. The
//! trace tests decode every `unit_sent` with the strict codecs; this module holds the function
//! and the packets inside the frames, which the check over the golden traces uses (WP-6).

use aldis::core::ocf::Ocf;
use aldis::core::security::SecurityLayout;
use aldis::link::common::ValidPvns;
use aldis::link::extractor::{
    ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor, PacketExtractorConfig, PacketRun,
};
use aldis::link::protocol::Tm;
use aldis::link::protocol::TmParams;
use aldis::spp::{PacketType, SequenceFlags, SpacePacket};
use aldis::tc::{
    Clcw, ControlCommand, FrameType, MapConfig, PhysicalChannelConfig, SegmentHeaderUse, TcFrame,
    VcConfig, VcTable,
};
use aldis::tm::{TmFrame, TmPhysicalChannelConfig};
use refapp_flight::FrameUplink;
use refapp_flight::command::parse;
use refapp_flight::event::{APP_CMD_RECEIVED, APP_PACKET_REJECTED, APP_TABLE_RECEIVED};
use refapp_flight::frames_up::{
    APP_FRAME_REJECTED, APP_SDU_TOO_LONG, APP_SEQUENCE_VIOLATION, APP_UNKNOWN_MAP,
};
use refapp_scenario::generated::apid;
use refapp_scenario::{Apid, Instant, Output};

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
/// The OCF of a TM frame must be a CLCW that decodes strictly (APP-134, TC-030, API-030).
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
            // The OCF of every TM frame is a CLCW (APP-134), decoded strictly (D8 a, APP-086).
            if let Some(ocf) = frame.ocf() {
                let Ocf::Clcw(report) = ocf else {
                    return Err("the OCF of a TM frame is not a CLCW".to_owned());
                };
                Clcw::decode_report(report).map_err(|e| format!("not a strict CLCW: {e:?}"))?;
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

/// What the flight node's uplink chain said while it handled one frame.
#[derive(Default)]
struct Tap {
    events: Vec<(u16, Vec<u64>)>,
}

impl Output for Tap {
    fn unit(&mut self, _octets: &[u8]) {}
    fn packet_out(&mut self, _apid: Apid, _octets: &[u8]) {}
    fn packet_in(&mut self, _apid: Apid, _octets: &[u8]) {}
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.events.push((code, args.to_vec()));
    }
}

/// The packets inside the frames of a trace (APP-086; WP-6 of the A2 and A3 plans).
///
/// Downlink: one `PacketExtractor` for each of VC 0 and VC 1 (LNK-060), over the frames in
/// trace order as a clean link. Uplink: the receive chain of the flight node
/// ([`FrameUplink`]: FARM-1, then the `MapReassembler` with the two MAPs, LNK-070), over the
/// frames **as received**, since only the Type-BD frames and the Type-AD frames that FARM-1
/// accepts reach the reassembler (APP-146, §4.5.3); a retransmitted or duplicated Type-AD frame
/// is discarded by FARM-1 and gives no packet, and a reordered one gives its packet when FARM-1
/// later accepts it. Idle packets are dropped, as the
/// extractor does (LNK-063). A partial packet at the end of the trace is no fault.
pub struct FramedPackets {
    realtime: Extractor,
    payload: Extractor,
    extract_area: Vec<u8>,
    uplink: FrameUplink,
    /// An uplink unit was dropped or corrupted and no packet has come out since.
    up_loss: bool,
}

impl FramedPackets {
    /// The extractors and the uplink chain for the managed parameters of `scenario`.
    ///
    /// # Errors
    /// If the scenario does not form a configuration.
    pub fn new(scenario: &Scenario) -> Result<FramedPackets, String> {
        let bad = |e: aldis::core::Error| format!("packet engines of the scenario: {e:?}");
        let config = PacketExtractorConfig::<Tm>::new(
            TmParams,
            ValidPvns::from_versions(&[refapp_scenario::generated::tm::PACKET_VERSION])
                .map_err(bad)?,
            ASSEMBLY,
        )
        .map_err(bad)?;
        if !scenario.tc.segment_header {
            return Err(
                "a TC channel without a Segment Header has no MAPs to reassemble".to_owned(),
            );
        }
        Ok(FramedPackets {
            realtime: Extractor::new(config).map_err(bad)?,
            payload: Extractor::new(config).map_err(bad)?,
            extract_area: vec![0; EXTRACT_AREA],
            uplink: FrameUplink::new_cop1().map_err(bad)?,
            up_loss: false,
        })
    }

    /// Says that an uplink unit was dropped or corrupted (a `fault` record). Until the chain
    /// next delivers a packet, a segment may arrive with no First segment or after a missing
    /// one, and the reassembler's sequence violation and `packet_rejected` are then the
    /// scenario's own outcome (A2 `a2-uplink-faults`); a delivered packet shows the reassembler
    /// is back in step with the stream and ends the tolerance.
    pub const fn note_up_loss(&mut self) {
        self.up_loss = true;
    }

    /// Gives the next downlink frame to the extractors and decodes the packets they complete.
    /// Returns how many it decoded.
    ///
    /// # Errors
    /// If an engine refuses the frame or reports an error, a discard, or an incomplete packet
    /// (a clean link has none), or a packet is not a strict Space Packet.
    pub fn take_down(&mut self, frame: &DecodedFrame) -> Result<usize, String> {
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

    /// Gives a TC frame as the flight node received it, at `now_us` microseconds, to the
    /// uplink chain and decodes the packets the reassembler delivers. Returns them.
    ///
    /// A frame that FARM-1 discards, as a retransmitted or an out-of-order Type-AD frame, is no
    /// fault and gives no packet (APP-146). The check of the trace decodes the frame first
    /// ([`decode_frame`]).
    ///
    /// The chain reports no rejected frame, unknown MAP, or too long SDU. After a loss
    /// ([`note_up_loss`](Self::note_up_loss)) and until a packet comes out, a sequence violation
    /// and a rejected packet are tolerated; on a clean link, or once a packet has come out,
    /// they are errors too.
    ///
    /// # Errors
    /// If the chain reports an event it may not, or a packet is not a strict Space Packet.
    pub fn receive_up(&mut self, now_us: u64, octets: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut tap = Tap::default();
        let mut packets = Vec::new();
        self.uplink.accept(
            Instant::from_micros(now_us),
            octets,
            &mut tap,
            &mut |packet, _| packets.push(packet.to_vec()),
        );
        for (code, args) in &tap.events {
            let loss_explains = matches!(*code, APP_PACKET_REJECTED | APP_SEQUENCE_VIOLATION);
            let unexpected = matches!(
                *code,
                APP_FRAME_REJECTED
                    | APP_PACKET_REJECTED
                    | APP_SEQUENCE_VIOLATION
                    | APP_UNKNOWN_MAP
                    | APP_SDU_TOO_LONG
            );
            if unexpected && !(self.up_loss && loss_explains) {
                return Err(format!("uplink event {code} {args:?} on a received frame"));
            }
        }
        for packet in &packets {
            strict_packet(packet, "packet in a TC frame")?;
        }
        if !packets.is_empty() {
            self.up_loss = false;
        }
        Ok(packets)
    }
}

/// The flight node's own record of the packets it handled, set against the packets the strict
/// decode reassembled from the uplink (APP-129, APP-131, APP-146).
///
/// The flight node has no `packet_in` record (§7.3): a command packet is `cmd_received`, a table
/// upload `table_received` or `packet_rejected`, and any other packet `packet_rejected`.
pub struct FlightIntake;

impl FlightIntake {
    /// The event codes that record a handled packet: `cmd_received`, `table_received`,
    /// `packet_rejected`.
    pub const CODES: [u16; 3] = [APP_CMD_RECEIVED, APP_TABLE_RECEIVED, APP_PACKET_REJECTED];

    /// Checks the reassembled `packets` against the `intake` events of the flight node in the
    /// trace (code and arguments, in trace order): the `cmd_received` records are exactly those
    /// of the command packets, with function code and sequence count; every other packet is a
    /// `table_received` or a `packet_rejected`. `packet_rejected` may also stand for an FDU the
    /// reassembler refused, which gives no packet, so there may be more of them.
    ///
    /// # Errors
    /// The first disagreement.
    pub fn check(packets: &[Vec<u8>], intake: &[(u16, Vec<u64>)]) -> Result<(), String> {
        let mut commands = Vec::new();
        let mut tables = 0usize;
        let mut others = 0usize;
        for octets in packets {
            let packet = SpacePacket::decode(octets)
                .map_err(|e| format!("reassembled packet is not a Space Packet: {e:?}"))?;
            let plain = packet.packet_type() == PacketType::Telecommand
                && packet.sequence_flags() == SequenceFlags::Unsegmented
                && !packet.has_secondary_header();
            match packet.apid().get() {
                apid::COMMANDS if plain => {
                    let c = parse(packet.sequence_count(), packet.data_field());
                    commands.push(vec![u64::from(c.function), u64::from(c.seq)]);
                }
                apid::TABLE_UPLOAD if plain => tables += 1,
                _ => others += 1,
            }
        }
        let seen = |code: u16| intake.iter().filter(move |(c, _)| *c == code);
        let received: Vec<Vec<u64>> = seen(APP_CMD_RECEIVED).map(|(_, a)| a.clone()).collect();
        if received != commands {
            return Err(format!(
                "the flight node's cmd_received records {received:?} differ from the command \
                 packets reassembled from the uplink {commands:?}"
            ));
        }
        let stored = seen(APP_TABLE_RECEIVED).count();
        let rejected = seen(APP_PACKET_REJECTED).count();
        if stored > tables || rejected < others + (tables - stored) {
            return Err(format!(
                "the flight node's records ({stored} table_received, {rejected} packet_rejected) \
                 do not account for the {tables} table upload and {others} other packet(s) \
                 reassembled from the uplink"
            ));
        }
        Ok(())
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
