//! Strict decoding of a link unit by level and variant (docs/reference-application.md §6.1,
//! §4.3, APP-086).
//!
//! At A2 on a link unit is a frame: at the variant `tm_tc` a TM frame on the downlink and a TC
//! frame on the uplink; at `aos` an AOS frame down and a TC frame up; at `uslp` a fixed-length
//! USLP frame down and a variable-length USLP frame up. The trace tests decode every `unit_sent`
//! with the strict codecs; this module holds the function and the packets inside the frames,
//! which the check over the golden traces uses.

use aldis::core::DecodeMode;
use aldis::core::ids::MapId;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::ocf::Ocf;
use aldis::core::security::SecurityLayout;
use aldis::link::common::{Qos, ValidPvns};
use aldis::link::extractor::{
    ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor, PacketExtractorConfig, PacketRun,
};
use aldis::link::protocol::{Aos, AosParams, PacketProtocol, Tc, Tm, TmParams, Uslp, UslpParams};
use aldis::spp::{PacketType, SequenceFlags, SpacePacket};
use aldis::tc::{
    Clcw, ControlCommand, FrameType, MapConfig, PhysicalChannelConfig, SegmentHeaderUse, TcFrame,
    VcConfig, VcTable,
};
use aldis::tm::{FirstHeaderPointer, TmFrame, TmPhysicalChannelConfig};
use aldis::{aos, uslp};
use refapp_flight::FrameUplink;
use refapp_flight::command::parse;
use refapp_flight::event::{APP_CMD_RECEIVED, APP_PACKET_REJECTED, APP_TABLE_RECEIVED};
use refapp_flight::frames_up::{
    APP_FRAME_REJECTED, APP_GAP_DISCARD, APP_SDU_TOO_LONG, APP_SEQUENCE_VIOLATION, APP_UNKNOWN_MAP,
};
use refapp_scenario::generated::apid;
use refapp_scenario::{Apid, Instant, Output};

use crate::scenario::{Level, Scenario, Variant};
use crate::trace::Dir;

/// A strictly decoded frame: what the nodes and the trace checks read of it.
///
/// A field that belongs to one frame protocol is `None` in the others. The Type-AD, -BD, and
/// -BC frames of COP-1 are the same three kinds in a TC frame and in a USLP frame (232.1 §4.2.7.4),
/// so a USLP uplink frame fills `frame_type`, `sequence`, and `control_command` as a TC frame does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecodedFrame {
    /// Virtual channel ID.
    pub vcid: u8,
    /// The data field (TM: the packet zone; TC: the Segment Header and the FDU; AOS: the packet
    /// zone of the `M_PDU`; USLP: the TFDZ).
    pub data: Vec<u8>,
    /// TM: the First Header Pointer of the primary header, as the extractor takes it.
    pub pointer: Option<FirstHeaderPointer>,
    /// TC: the Segment Header octet of a Type-D frame on a VC that has one.
    pub segment_header: Option<u8>,
    /// TC and USLP uplink: the frame type, the Bypass and Control Command Flags (232.0 Table
    /// 4-1; 732.1 Table 4-1).
    pub frame_type: Option<FrameType>,
    /// TC: the Frame Sequence Number N(S) of the Primary Header (232.0 §4.1.2.8). USLP uplink: the
    /// N(S) of a Type-AD frame, its VCF Count (232.1 §4.2.7.4); a Type-BD or -BC frame has none.
    pub sequence: Option<u8>,
    /// TC and USLP uplink: the Control Command of a Type-BC frame (232.0 §4.1.3.3).
    pub control_command: Option<ControlCommand>,
    /// TM, AOS, and USLP downlink: the CLCW of the OCF, when the frame has one (232.0 §4.2.1).
    pub clcw: Option<Clcw>,
    /// AOS: the First Header Pointer of the `M_PDU` Header (732.0 §4.1.4.2.2).
    pub packet_pointer: Option<aos::PacketPointer>,
    /// USLP: the TFDF Header, the construction rule with its pointer and the UPID (732.1
    /// §4.1.4.2).
    pub tfdf: Option<uslp::TfdfHeader>,
    /// USLP: the MAP ID (732.1 §4.1.2.5).
    pub map_id: Option<u8>,
    /// USLP: what the Bypass and Protocol Control Command Flags say, the quality of service and
    /// whether the TFDZ is user data or a control command (732.1 Table 4-1).
    pub kind: Option<uslp::FrameKind>,
    /// AOS: the VC Frame Count (732.0 §4.1.2.4). USLP: the VCF Count of a frame that has one
    /// (732.1 §4.1.2.12). `None` for a USLP frame whose count length is 0.
    pub count: Option<u64>,
    /// USLP: an OID frame, on VCID 63 (732.1 §4.1.4.1.5).
    pub oid: bool,
    /// AOS and USLP downlink: the data field as the `PacketExtractor` takes it, the `M_PDU` (its
    /// header and the packet zone) or the TFDF (its header and the TFDZ) (docs/04-engines.md
    /// §3.1).
    pub field: Vec<u8>,
}

impl DecodedFrame {
    /// The frame is a Type-BC frame: a control command, no packets.
    #[must_use]
    pub const fn is_control(&self) -> bool {
        matches!(self.frame_type, Some(FrameType::Bc))
    }
}

/// Decodes a link unit as a frame of the variant, strictly (APP-086): TM, AOS, or USLP down, TC
/// or USLP up.
///
/// A unit of a [`Level::Packets`] case is a Space Packet ([`crate::trace::decode_strict`]).
///
/// The frame types a TC frame may have depend on the level (APP-043, APP-090): at
/// [`Level::Frames`] the ground node sends Type-BD frames only; at [`Level::Cop1`] it sends
/// Type-AD, Type-BD, and Type-BC frames (232.0 Table 4-1).
///
/// The OCF of a TM, AOS, or USLP downlink frame must be a CLCW that decodes strictly (APP-134,
/// APP-173, APP-179, TC-030, API-030).
///
/// The frame must carry the Spacecraft ID of the scenario (APP-026). A TM, AOS, or USLP downlink
/// frame must have exactly the configured frame length; a TC or USLP uplink frame is checked only
/// against the maximum frame length (its Frame Length field gives its length, TC-007, USLP-003),
/// and a USLP uplink frame must fill the octets given, which a bit error in its Frame Length
/// field would not (USLP-014).
///
/// An AOS frame has its FHEC corrected and its FECF checked (APP-169, APP-174). A USLP uplink
/// frame has VC 0 and MAP 0, and is a Type-AD frame, a Type-BD frame, or a Type-BC frame by its
/// two flags; a Type-BC frame holds exactly one Control Command (APP-183, OI-100). An AOS frame is
/// no OID frame (APP-170). A USLP downlink frame is expedited, and a data frame has the rule
/// `000` (APP-176, APP-178).
///
/// # Errors
/// If `level` is [`Level::Packets`] or the variant does not apply at `level` (APP-187), the
/// managed parameters of the scenario do not form a configuration, or the codec rejects the
/// frame (FHEC, FECF, lengths, fields), the frame has another Spacecraft ID, a TC frame has a
/// type its level does not use, or a USLP frame breaks a rule of APP-176, APP-178, or APP-183.
pub fn decode_frame(
    scenario: &Scenario,
    level: Level,
    variant: Variant,
    dir: Dir,
    octets: &[u8],
) -> Result<DecodedFrame, String> {
    if level == Level::Packets {
        return Err("a unit of a packet-level case is a Space Packet, not a frame".to_owned());
    }
    if !variant.applies_at(level) {
        return Err(format!(
            "the variant `{variant}` does not apply at the level `{level:?}` (APP-187)"
        ));
    }
    match (variant, dir) {
        (Variant::TmTc, Dir::Down) => decode_tm(scenario, octets),
        (Variant::TmTc | Variant::Aos, Dir::Up) => decode_tc(scenario, level, octets),
        (Variant::Aos, Dir::Down) => decode_aos(scenario, octets),
        (Variant::Uslp, Dir::Down) => decode_uslp_down(scenario, octets),
        (Variant::Uslp, Dir::Up) => decode_uslp_up(scenario, octets),
    }
}

/// The OCF of a downlink frame as a CLCW, decoded strictly (APP-134, D8 a, APP-086).
fn clcw_of(ocf: Option<Ocf>, frame: &str) -> Result<Option<Clcw>, String> {
    match ocf {
        Some(Ocf::Clcw(report)) => Ok(Some(
            Clcw::decode_report(report).map_err(|e| format!("not a strict CLCW: {e:?}"))?,
        )),
        Some(Ocf::ProjectSpecific(_) | Ocf::SdlsFsr(_)) => {
            Err(format!("the OCF of {frame} is not a CLCW"))
        }
        None => Ok(None),
    }
}

/// A TM frame of the downlink (§4.1).
fn decode_tm(scenario: &Scenario, octets: &[u8]) -> Result<DecodedFrame, String> {
    let scid = scenario.spacecraft_id.get();
    let tm = &scenario.tm;
    let pc = TmPhysicalChannelConfig::new(usize::from(tm.frame_length.get()), tm.fecf)
        .map_err(|e| format!("TM channel of the scenario: {e:?}"))?;
    let frame =
        TmFrame::decode(octets, &pc).map_err(|e| format!("not a strict TM frame: {e:?}"))?;
    if frame.scid().get() != scid {
        return Err(format!(
            "TM frame has Spacecraft ID {}, expected {scid}",
            frame.scid().get()
        ));
    }
    // The OCF of every TM frame is a CLCW (APP-134), decoded strictly (D8 a, APP-086).
    let clcw = clcw_of(frame.ocf(), "a TM frame")?;
    Ok(DecodedFrame {
        vcid: frame.vcid().get(),
        data: frame.data_field().to_vec(),
        pointer: Some(frame.header().first_header_pointer),
        clcw,
        ..DecodedFrame::default()
    })
}

/// A TC frame of the uplink (§4.2).
fn decode_tc(scenario: &Scenario, level: Level, octets: &[u8]) -> Result<DecodedFrame, String> {
    let scid = scenario.spacecraft_id.get();
    let tc = &scenario.tc;
    let max = usize::from(tc.max_frame_length.get());
    let id_err = |what: &str, e: &dyn std::fmt::Debug| format!("TC {what} of the scenario: {e:?}");
    let out_of_range = |what: &str| format!("TC {what} of the scenario is out of range");
    let scid_t = aldis::tc::TcSpacecraftId::new(scid).ok_or_else(|| out_of_range("SCID"))?;
    let vc_t = aldis::tc::TcVcId::new(tc.vcid.get()).ok_or_else(|| out_of_range("VCID"))?;
    let map = |id: u8| {
        aldis::tc::TcMapId::new(id)
            .map(|m| MapConfig::new(scid_t, vc_t, m, SecurityLayout::NONE))
            .ok_or_else(|| out_of_range("MAP ID"))
    };
    let physical = PhysicalChannelConfig::new(max, tc.fecf).map_err(|e| id_err("channel", &e))?;
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
    let frame =
        TcFrame::decode(octets, &table).map_err(|e| format!("not a strict TC frame: {e:?}"))?;
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
        segment_header: frame.segment_header().map(aldis::tc::SegmentHeader::octet),
        frame_type: Some(frame_type),
        sequence: Some(frame.sequence_number().get()),
        control_command: frame.control_command(),
        ..DecodedFrame::default()
    })
}

/// An AOS frame of the downlink (§4.3.1): FHEC corrected, FECF checked, the channel one of the
/// configured VCs and no OID frame, the `VC_OCF` a CLCW (APP-169, APP-170, APP-173, APP-174).
fn decode_aos(scenario: &Scenario, octets: &[u8]) -> Result<DecodedFrame, String> {
    use aldis::core::Check as _;
    let scid = scenario.spacecraft_id.get();
    let a = &scenario.aos;
    let id_err = |what: &str, e: &dyn std::fmt::Debug| format!("AOS {what} of the scenario: {e:?}");
    let out_of_range = |what: &str| format!("AOS {what} of the scenario is out of range");
    let pc = aos::AosPhysicalChannelConfig::new(
        usize::from(a.frame_length.get()),
        a.fhec,
        0,
        a.fecf,
        aos::AosEdition::Issue5,
    )
    .map_err(|e| id_err("channel", &e))?;
    let scid_t = SpacecraftId::<10>::new(scid).ok_or_else(|| out_of_range("SCID"))?;
    let vc = |id: u8| {
        let vcid = VcId::<6>::new(id).ok_or_else(|| out_of_range("VCID"))?;
        let vc = aos::AosVirtualChannelConfig::new(scid_t, vcid, aos::DataFieldContent::Mpdu)
            .map_err(|e| id_err("VC", &e))?;
        Ok::<_, String>(if a.vc_ocf { vc.with_ocf() } else { vc })
    };
    let table: aos::VcTable<3> = aos::VcTable::new(
        pc,
        &[
            vc(a.vcid_realtime.get())?,
            vc(a.vcid_payload.get())?,
            vc(a.vcid_idle.get())?,
        ],
    )
    .map_err(|e| id_err("channels", &e))?;
    let frame = aos::AosFrame::decode(octets, &pc, &table)
        .map_err(|e| format!("not a strict AOS frame: {e:?}"))?;
    // A frame of a channel the table does not hold decodes as opaque octets; the check refuses it
    // (732.0 §4.3.5.4).
    let frame = frame
        .check(table.slots())
        .map_err(|r| format!("AOS frame of no configured channel: {:?}", r.error()))?;
    if frame.scid().get() != scid {
        return Err(format!(
            "AOS frame has Spacecraft ID {}, expected {scid}",
            frame.scid().get()
        ));
    }
    // The flight node sends no OID frame (APP-170, OI-43).
    if frame.is_oid() {
        return Err("an AOS frame must not be an OID frame (APP-170, OI-43)".to_owned());
    }
    let (data, packet_pointer) = match frame.data_unit() {
        aos::DataUnit::Mpdu(mpdu) => (mpdu.packet_zone().to_vec(), Some(mpdu.pointer())),
        aos::DataUnit::IdleData(_) | aos::DataUnit::Bpdu(_) | aos::DataUnit::VcaSdu(_) => {
            return Err("the data field of an AOS frame is not an M_PDU".to_owned());
        }
    };
    Ok(DecodedFrame {
        vcid: frame.vcid().get(),
        data,
        clcw: clcw_of(frame.ocf(), "an AOS frame")?,
        packet_pointer,
        field: frame.data_field().to_vec(),
        count: Some(u64::from(frame.vc_frame_count().get())),
        ..DecodedFrame::default()
    })
}

/// The USLP Spacecraft ID of the scenario.
fn uslp_scid(scenario: &Scenario) -> Result<uslp::UslpSpacecraftId, String> {
    uslp::UslpSpacecraftId::new(scenario.spacecraft_id.get())
        .ok_or_else(|| "USLP SCID of the scenario is out of range".to_owned())
}

/// A USLP identifier of the scenario that the type refuses.
fn uslp_vc(id: u8) -> Result<uslp::UslpVcId, String> {
    uslp::UslpVcId::new(id).ok_or_else(|| format!("USLP VCID {id} of the scenario is out of range"))
}

/// The MAP ID `id` of the scenario.
fn uslp_map(id: u8) -> Result<uslp::UslpMapId, String> {
    uslp::UslpMapId::new(id)
        .ok_or_else(|| format!("USLP MAP ID {id} of the scenario is out of range"))
}

/// A VC count length of the scenario.
fn uslp_count_len(octets: u8) -> Result<uslp::VcfCountLen, String> {
    uslp::VcfCountLen::new(octets)
        .map_err(|e| format!("USLP VCF Count Length of the scenario: {e:?}"))
}

/// A USLP frame that passed the strict decode and the USLP Frame Validation Checks against
/// `table` (732.1 §4.3.11.3.4, USLP-014), as a non-truncated frame.
fn uslp_checked<'a, const V: usize, const M: usize>(
    octets: &'a [u8],
    table: &uslp::VcTable<V, M>,
) -> Result<uslp::UslpFrame<'a>, String> {
    let checked = uslp::check_frame(octets, table, DecodeMode::Strict).map_err(|r| {
        format!(
            "not a strict USLP frame: {:?} ({:?})",
            r.reason(),
            r.error()
        )
    })?;
    Ok(checked.into_view().into_frame())
}

/// The non-truncated frame in `frame`: the scenario has no truncated frames (732.1 Annex D).
fn uslp_full<'a, 'f>(
    frame: &'f uslp::UslpFrame<'a>,
) -> Result<&'f uslp::UslpFullFrame<'a>, String> {
    frame
        .full()
        .ok_or_else(|| "a truncated USLP frame in a case without truncated frames".to_owned())
}

/// The TFDF as the extractor takes it: the header, then the TFDZ (docs/04-engines.md §3.1).
fn tfdf_field(header: uslp::TfdfHeader, zone: &[u8]) -> Result<Vec<u8>, String> {
    let mut field = header
        .octets()
        .get(..header.len())
        .ok_or("the TFDF Header is longer than its array")?
        .to_vec();
    field.extend_from_slice(zone);
    Ok(field)
}

/// A fixed-length USLP frame of the downlink (§4.3.2): FECF checked, the SCID that of the
/// scenario, VC 0, VC 1, or an OID frame on VCID 63, and the MC OCF a CLCW on every frame (OID
/// frames included) when the master channel requires one (APP-175…APP-179).
fn decode_uslp_down(scenario: &Scenario, octets: &[u8]) -> Result<DecodedFrame, String> {
    let d = &scenario.uslp_down;
    let id_err =
        |what: &str, e: &dyn std::fmt::Debug| format!("USLP {what} of the scenario: {e:?}");
    let scid = uslp_scid(scenario)?;
    let physical = uslp::PhysicalChannelConfig::new(
        uslp::TransferFrameType::Fixed,
        usize::from(d.frame_length.get()),
        0,
        d.fecf,
    )
    .map_err(|e| id_err("channel", &e))?;
    let ocf = if d.ocf_required {
        uslp::OcfUse::Required
    } else {
        uslp::OcfUse::Absent
    };
    let data_vc = uslp::VcParams {
        vcf_count_len_sequence: uslp_count_len(d.vcf_count_length_sequence.get())?,
        vcf_count_len_expedited: uslp_count_len(d.vcf_count_length_expedited.get())?,
        ocf,
        ..uslp::VcParams::new(uslp::TransferFrameType::Fixed)
    };
    // VCID 63 is valid without an entry, with no OCF; an entry gives the OID frames the OCF of
    // the master channel and no VCF Count (732.1 Table 5-2 note 2, §4.1.4.1.10 note 4).
    let oid_vc = uslp::VcParams {
        ocf,
        ..uslp::VcParams::new(uslp::TransferFrameType::Fixed)
    };
    let vc = |id: u8, params: uslp::VcParams| {
        uslp::VcConfig::new(scid, uslp_vc(id)?, params).map_err(|e| id_err("VC", &e))
    };
    let map = |id: u8| {
        Ok::<_, String>(uslp::MapConfig::new(
            scid,
            uslp_vc(id)?,
            uslp_map(d.map_id.get())?,
            SecurityLayout::NONE,
        ))
    };
    let table: uslp::VcTable<3, 2> = uslp::VcTable::new(
        physical,
        &[
            vc(d.vcid_realtime.get(), data_vc)?,
            vc(d.vcid_payload.get(), data_vc)?,
            vc(uslp::OID_VCID, oid_vc)?,
        ],
        &[map(d.vcid_realtime.get())?, map(d.vcid_payload.get())?],
    )
    .map_err(|e| id_err("channels", &e))?;
    let frame = uslp_checked(octets, &table)?;
    let full = uslp_full(&frame)?;
    let header = full.header();
    if header.spacecraft_id() != scid {
        return Err(format!(
            "USLP frame has Spacecraft ID {}, expected {}",
            header.spacecraft_id().get(),
            scid.get()
        ));
    }
    // The spacecraft is the source of every downlink frame (APP-176, 732.1 §4.1.2.3.3 a).
    if header.source_or_destination() != uslp::SourceOrDestination::Source {
        return Err(
            "a USLP downlink frame must have Source-or-Destination Identifier 0".to_owned(),
        );
    }
    // Every downlink frame is expedited: Bypass/Sequence Control Flag 1, Protocol Control Command
    // Flag 0 (APP-176, APP-178, OI-99).
    if full.kind() != uslp::FrameKind::Expedited {
        return Err(format!(
            "a USLP downlink frame must be expedited (flags `10`), not {:?} (APP-176, OI-99)",
            full.kind()
        ));
    }
    let vcid = header.vc_id().get();
    let oid = vcid == uslp::OID_VCID;
    let tfdf = full.tfdf_header();
    if oid && tfdf.upid != uslp::Upid::OnlyIdleData {
        return Err("an OID frame must have the UPID of only idle data".to_owned());
    }
    if !oid && tfdf.upid != uslp::Upid::Packets {
        return Err(format!(
            "a data frame must have the UPID of Space Packets or Encapsulation packets, not {:?}",
            tfdf.upid
        ));
    }
    // A data frame has the rule `000`, packets spanning frames (APP-176).
    if !oid && !matches!(tfdf.rule, uslp::TfdzRule::PacketsSpanningFrames(_)) {
        return Err(format!(
            "a USLP downlink data frame must have the rule `000`, not {:?} (APP-176)",
            tfdf.rule.rule()
        ));
    }
    let count = header.vcf_count();
    let field = tfdf_field(tfdf, full.tfdz())?;
    Ok(DecodedFrame {
        vcid,
        data: full.tfdz().to_vec(),
        field,
        clcw: clcw_of(full.ocf(), "a USLP frame")?,
        tfdf: Some(tfdf),
        map_id: Some(header.map_id().get()),
        kind: Some(full.kind()),
        count: (count.len().get() > 0).then(|| count.value()),
        oid,
        ..DecodedFrame::default()
    })
}

/// A variable-length USLP frame of the uplink (§4.3.3): the SCID of the scenario, VC 0 and MAP 0,
/// Type-AD, -BD, or -BC by the two flags, N(S) the VCF Count of a Type-AD frame, and in a Type-BC
/// frame exactly one Control Command (APP-181…APP-183, OI-100).
fn decode_uslp_up(scenario: &Scenario, octets: &[u8]) -> Result<DecodedFrame, String> {
    let u = &scenario.uslp_up;
    let id_err =
        |what: &str, e: &dyn std::fmt::Debug| format!("USLP {what} of the scenario: {e:?}");
    let scid = uslp_scid(scenario)?;
    let physical = uslp::PhysicalChannelConfig::new(
        uslp::TransferFrameType::Variable,
        usize::from(u.max_frame_length.get()),
        0,
        u.fecf,
    )
    .map_err(|e| id_err("channel", &e))?;
    let params = uslp::VcParams {
        vcf_count_len_sequence: uslp_count_len(u.vcf_count_length_sequence.get())?,
        vcf_count_len_expedited: uslp_count_len(u.vcf_count_length_expedited.get())?,
        ocf: if u.ocf_allowed {
            uslp::OcfUse::Allowed
        } else {
            uslp::OcfUse::Absent
        },
        ..uslp::VcParams::new(uslp::TransferFrameType::Variable)
    };
    let vc =
        uslp::VcConfig::new(scid, uslp_vc(u.vcid.get())?, params).map_err(|e| id_err("VC", &e))?;
    let map = uslp::MapConfig::new(
        scid,
        uslp_vc(u.vcid.get())?,
        uslp_map(u.map_id.get())?,
        SecurityLayout::NONE,
    );
    let table: uslp::VcTable<1, 1> =
        uslp::VcTable::new(physical, &[vc], &[map]).map_err(|e| id_err("channels", &e))?;
    let frame = uslp_checked(octets, &table)?;
    let full = uslp_full(&frame)?;
    let header = full.header();
    if header.spacecraft_id() != scid {
        return Err(format!(
            "USLP frame has Spacecraft ID {}, expected {}",
            header.spacecraft_id().get(),
            scid.get()
        ));
    }
    // The SCID refers to the destination, the spacecraft (APP-181, 732.1 §4.1.2.3.3 b).
    if header.source_or_destination() != uslp::SourceOrDestination::Destination {
        return Err("a USLP uplink frame must have Source-or-Destination Identifier 1".to_owned());
    }
    let kind = full.kind();
    let tfdf = full.tfdf_header();
    if tfdf.rule != uslp::TfdzRule::NoSegmentation {
        return Err(format!(
            "a USLP uplink frame must have the rule `111`, not {:?}",
            tfdf.rule.rule()
        ));
    }
    let frame_type = match kind {
        uslp::FrameKind::SequenceControlled => FrameType::Ad,
        uslp::FrameKind::Expedited => FrameType::Bd,
        uslp::FrameKind::ProtocolControl => FrameType::Bc,
    };
    let (want, control_command) = match kind {
        uslp::FrameKind::SequenceControlled | uslp::FrameKind::Expedited => {
            (uslp::Upid::Packets, None)
        }
        uslp::FrameKind::ProtocolControl => (
            uslp::Upid::Cop1ControlCommands,
            // A TFDZ that is not exactly one Control Command makes the frame invalid (OI-100).
            Some(ControlCommand::parse(full.tfdz()).ok_or_else(|| {
                "the TFDZ of a Type-BC frame is not exactly one Control Command".to_owned()
            })?),
        ),
    };
    if tfdf.upid != want {
        return Err(format!(
            "a {frame_type:?} frame must have the UPID {want:?}, not {:?}",
            tfdf.upid
        ));
    }
    let count = full.vcf_count();
    let sequence = match kind {
        uslp::FrameKind::SequenceControlled => Some(
            u8::try_from(count.value())
                .map_err(|_| "the VCF Count of a Type-AD frame is not an N(S)".to_owned())?,
        ),
        uslp::FrameKind::Expedited | uslp::FrameKind::ProtocolControl => None,
    };
    Ok(DecodedFrame {
        vcid: header.vc_id().get(),
        data: full.tfdz().to_vec(),
        frame_type: Some(frame_type),
        sequence,
        control_command,
        clcw: clcw_of(full.ocf(), "a USLP frame")?,
        tfdf: Some(tfdf),
        map_id: Some(header.map_id().get()),
        kind: Some(kind),
        count: (count.len().get() > 0).then(|| count.value()),
        ..DecodedFrame::default()
    })
}

/// The extractor of a downlink VC of the protocol `P` (the sizes of the ground node: §4.1).
type Extractor<P> = PacketExtractor<P, ASSEMBLY, FIELD>;
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

/// The extractors of the downlink VCs of a variant (§4.1, §4.3.1, §4.3.2).
#[expect(
    clippy::large_enum_variant,
    reason = "one value per trace check; boxing the extractors would only add indirection"
)]
enum Down {
    /// TM: VC 0 and VC 1; the OID frames of VC 7 carry idle data only.
    Tm {
        realtime: Extractor<Tm>,
        payload: Extractor<Tm>,
    },
    /// AOS: VC 0, VC 1, and the idle VC 7 (APP-170, APP-174).
    Aos {
        realtime: Extractor<Aos>,
        payload: Extractor<Aos>,
        idle: Extractor<Aos>,
    },
    /// USLP: VC 0 and VC 1; the OID frames of VCID 63 carry idle data only (APP-175, APP-180).
    Uslp {
        realtime: Extractor<Uslp>,
        payload: Extractor<Uslp>,
    },
}

/// The uplink receive chain of a variant: the TC chain at `tm_tc` and `aos` (APP-187), the USLP
/// chain at `uslp` (APP-181…APP-185).
enum Up {
    Tc(FrameUplink<Tc>),
    Uslp(FrameUplink<Uslp>),
}

/// The packets inside the frames of a trace (APP-086; WP-6 of the A2, A3, and A4 plans).
///
/// Downlink: one `PacketExtractor` for each data VC (LNK-060), over the frames in trace order as
/// a clean link: VC 0 and VC 1 at `tm_tc`; VC 0, VC 1, and 7 at `aos`; VC 0 and VC 1 at `uslp`.
/// Uplink: the receive chain of the flight node ([`FrameUplink`]: FARM-1, then the
/// `MapReassembler`, LNK-070), over the frames **as received**, since only the Type-BD frames
/// and the Type-AD frames that FARM-1 accepts reach the reassembler (APP-146, §4.5.3); a
/// retransmitted or duplicated Type-AD frame is discarded by FARM-1 and gives no packet, and a
/// reordered one gives its packet when FARM-1 later accepts it. The chain is that of TC at
/// `tm_tc` and `aos` (the two MAPs), and that of USLP at `uslp` (MAP 0, blocked). Idle packets
/// are dropped, as the extractor does (LNK-063). A partial packet at the end of the trace is no
/// fault.
pub struct FramedPackets {
    down: Down,
    extract_area: Vec<u8>,
    uplink: Up,
    /// An uplink unit was dropped or corrupted and no packet has come out since.
    up_loss: bool,
}

impl FramedPackets {
    /// The extractors and the uplink chain of the variant `tm_tc` for the managed parameters of
    /// `scenario`.
    ///
    /// # Errors
    /// If the scenario does not form a configuration.
    pub fn new(scenario: &Scenario) -> Result<Self, String> {
        Self::for_variant(scenario, Variant::TmTc)
    }

    /// The extractors and the uplink chain of `variant` for the managed parameters of `scenario`.
    ///
    /// # Errors
    /// If the scenario does not form a configuration.
    pub fn for_variant(scenario: &Scenario, variant: Variant) -> Result<Self, String> {
        let bad = |e: aldis::core::Error| format!("packet engines of the scenario: {e:?}");
        let tc_up = || -> Result<Up, String> {
            if !scenario.tc.segment_header {
                return Err(
                    "a TC channel without a Segment Header has no MAPs to reassemble".to_owned(),
                );
            }
            Ok(Up::Tc(FrameUplink::new_cop1().map_err(bad)?))
        };
        let (down, uplink) = match variant {
            Variant::TmTc => {
                let config = PacketExtractorConfig::<Tm>::new(
                    TmParams,
                    ValidPvns::from_versions(&[refapp_scenario::generated::tm::PACKET_VERSION])
                        .map_err(bad)?,
                    ASSEMBLY,
                )
                .map_err(bad)?;
                let down = Down::Tm {
                    realtime: Extractor::new(config).map_err(bad)?,
                    payload: Extractor::new(config).map_err(bad)?,
                };
                (down, tc_up()?)
            }
            Variant::Aos => {
                use refapp_scenario::generated::aos as a;
                // Idle Packets and the idle `M_PDU`s of VC 7 go to no one (APP-174).
                let config = PacketExtractorConfig::<Aos>::new(
                    AosParams {
                        edition: aos::AosEdition::Issue5,
                    },
                    ValidPvns::from_versions(&[a::PACKET_VERSION]).map_err(bad)?,
                    usize::from(a::MAX_PACKET_LENGTH),
                )
                .map_err(bad)?
                .with_delivery(a::DELIVER_INCOMPLETE_PACKETS, false);
                let down = Down::Aos {
                    realtime: Extractor::new(config).map_err(bad)?,
                    payload: Extractor::new(config).map_err(bad)?,
                    idle: Extractor::new(config).map_err(bad)?,
                };
                (down, tc_up()?)
            }
            Variant::Uslp => {
                use refapp_scenario::generated::uslp_down as u;
                // The SPP and Encapsulation PVNs are valid; Encapsulation Idle Packets go to no
                // one (APP-180).
                let map = MapId::<4>::new(u::MAP_ID)
                    .ok_or_else(|| "USLP MAP ID of the scenario is out of range".to_owned())?;
                let config = PacketExtractorConfig::<Uslp>::new(
                    UslpParams::new(aldis::uslp::Upid::Packets, map, Qos::Expedited)
                        .map_err(bad)?,
                    ValidPvns::from_versions(&[u::PACKET_VERSION, u::ENCAPSULATION_PACKET_VERSION])
                        .map_err(bad)?,
                    usize::from(u::MAX_PACKET_LENGTH),
                )
                .map_err(bad)?
                .with_delivery(u::DELIVER_INCOMPLETE_PACKETS, false);
                let down = Down::Uslp {
                    realtime: Extractor::new(config).map_err(bad)?,
                    payload: Extractor::new(config).map_err(bad)?,
                };
                (down, Up::Uslp(FrameUplink::new_cop1().map_err(bad)?))
            }
        };
        Ok(Self {
            down,
            extract_area: vec![0; EXTRACT_AREA],
            uplink,
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
        let area = &mut self.extract_area;
        let vcid = frame.vcid;
        match &mut self.down {
            Down::Tm { realtime, payload } => {
                let extractor = match vcid {
                    0 => realtime,
                    1 => payload,
                    // The OID frames of VC 7 carry idle data only (§4.1).
                    _ => return Ok(0),
                };
                let field = FieldIn {
                    field: &frame.data,
                    header_pointer: frame.pointer,
                    gap: false,
                };
                extract(extractor, field, area, vcid, "packet in a TM frame")
            }
            Down::Aos {
                realtime,
                payload,
                idle,
            } => {
                let extractor = match vcid {
                    0 => realtime,
                    1 => payload,
                    // The idle `M_PDU`s of VC 7 give no packet (APP-170, APP-174).
                    7 => idle,
                    _ => return Err(format!("AOS frame on VC {vcid}, which has no extractor")),
                };
                let field = FieldIn {
                    field: &frame.field,
                    header_pointer: None,
                    gap: false,
                };
                extract(extractor, field, area, vcid, "packet in an AOS frame")
            }
            Down::Uslp { realtime, payload } => {
                let extractor = match vcid {
                    0 => realtime,
                    1 => payload,
                    // The OID frames carry only idle data (APP-175).
                    _ if frame.oid => return Ok(0),
                    _ => return Err(format!("USLP frame on VC {vcid}, which has no extractor")),
                };
                let field = FieldIn {
                    field: &frame.field,
                    header_pointer: None,
                    gap: false,
                };
                extract(extractor, field, area, vcid, "packet in a USLP frame")
            }
        }
    }

    /// Gives an uplink frame as the flight node received it, at `now_us` microseconds, to the
    /// uplink chain and decodes the packets the reassembler delivers. Returns them.
    ///
    /// A frame that FARM-1 discards, as a retransmitted or an out-of-order Type-AD frame, is no
    /// fault and gives no packet (APP-146). The check of the trace decodes the frame first
    /// ([`decode_frame`]).
    ///
    /// The chain reports no rejected frame, unknown MAP, too long SDU, or gap discard. After a
    /// loss ([`note_up_loss`](Self::note_up_loss)) and until a packet comes out, a sequence
    /// violation and a rejected packet are tolerated; on a clean link, or once a packet has
    /// come out, they are errors too.
    ///
    /// # Errors
    /// If the chain reports an event it may not, or a packet is not a strict Space Packet.
    pub fn receive_up(&mut self, now_us: u64, octets: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut tap = Tap::default();
        let mut packets = Vec::new();
        let now = Instant::from_micros(now_us);
        match &mut self.uplink {
            Up::Tc(up) => up.accept(now, octets, &mut tap, &mut |packet, _| {
                packets.push(packet.to_vec());
            }),
            Up::Uslp(up) => up.accept(now, octets, &mut tap, &mut |packet, _| {
                packets.push(packet.to_vec());
            }),
        }
        for (code, args) in &tap.events {
            let loss_explains = matches!(*code, APP_PACKET_REJECTED | APP_SEQUENCE_VIOLATION);
            let unexpected = matches!(
                *code,
                APP_FRAME_REJECTED
                    | APP_PACKET_REJECTED
                    | APP_SEQUENCE_VIOLATION
                    | APP_UNKNOWN_MAP
                    | APP_SDU_TOO_LONG
                    | APP_GAP_DISCARD
            );
            if unexpected && !(self.up_loss && loss_explains) {
                return Err(format!("uplink event {code} {args:?} on a received frame"));
            }
        }
        for packet in &packets {
            strict_packet(packet, "packet in an uplink frame")?;
        }
        if !packets.is_empty() {
            self.up_loss = false;
        }
        Ok(packets)
    }
}

/// Gives `field` to `extractor` and checks every packet it completes with [`strict_packet`];
/// returns how many. A discard, an error, or an incomplete packet is a fault on a clean link.
fn extract<P: PacketProtocol>(
    extractor: &mut Extractor<P>,
    field: FieldIn<'_, P>,
    area: &mut [u8],
    vcid: u8,
    what: &str,
) -> Result<usize, String> {
    let actions = extractor
        .step(ExtractorInput::DataField(field), area)
        .map_err(|e| format!("extractor of VC {vcid}: {e:?}"))?;
    let mut packets = 0;
    for action in actions.iter() {
        match *action {
            ExtractorOutput::Packets { run, .. } => {
                let run = PacketRun::new(run, area).ok_or("extractor run outside its area")?;
                for packet in run {
                    strict_packet(packet, what)?;
                    packets += 1;
                }
            }
            ExtractorOutput::Discarded { octets } => {
                return Err(format!(
                    "VC {vcid}: {octets} octet(s) of a partial packet discarded"
                ));
            }
            ExtractorOutput::Error(e) => {
                return Err(format!("extraction error on VC {vcid}: {e:?}"));
            }
            ExtractorOutput::IncompletePacket { .. } => {
                return Err(format!("incomplete packet on VC {vcid}"));
            }
        }
    }
    Ok(packets)
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
