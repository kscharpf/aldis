//! The scenario file `refapp/scenario/scenario.json` (docs/reference-application.md, APP-107).
//!
//! These types are the single source of truth for the file format: [`Scenario::load`]
//! deserializes the file with them (unknown fields are errors), [`schema`] publishes the JSON
//! Schema generated from them, and [`rust_constants`] generates `refapp-scenario`'s constants,
//! so the validator, the schema, and the generated code cannot drift apart.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::micros::Micros;
use crate::ranged::{EvenRanged, Ranged};
use refapp_scenario::{Crc16, Duration, TableId, TableLength};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The scenario file's text, compiled into the harness.
pub const SCENARIO_JSON: &str = include_str!("../../scenario/scenario.json");

/// The whole scenario (docs/reference-application.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Scenario identifier, always `aldis-refapp` (§7.2).
    pub scenario: String,
    /// Scenario version (§7.2, APP-002).
    pub scenario_version: u16,
    /// Spacecraft ID of every frame (§3.4, APP-026); fictional (OI-26).
    pub spacecraft_id: Ranged<u16, 0, 1023>,
    /// Time model (§5).
    pub time: Time,
    /// Link model (§6.1).
    pub link: Link,
    /// APIDs (§3.1).
    pub apid: Apids,
    /// TM downlink (§4.1).
    pub tm: Tm,
    /// TC uplink (§4.2).
    pub tc: Tc,
    /// COP-1 parameters (§4.2).
    pub cop1: Cop1,
    /// AOS downlink of the variant `aos` (§4.3.1).
    pub aos: Aos,
    /// USLP downlink of the variant `uslp` (§4.3.2).
    pub uslp_down: UslpDown,
    /// USLP uplink of the variant `uslp` (§4.3.3).
    pub uslp_up: UslpUp,
    /// Cases (§6.4).
    pub cases: Vec<Case>,
}

/// Time model (§5, APP-060).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Time {
    /// CUC coarse time of the run's start, seconds since 1958-01-01 TAI.
    pub t0_coarse_s: u32,
    /// CUC coarse octets.
    pub cuc_coarse_octets: Ranged<u8, 1, 4>,
    /// CUC fractional octets.
    pub cuc_fine_octets: Ranged<u8, 0, 3>,
    /// Explicit P-field octet (301.0 §3.2.2).
    pub cuc_explicit_pfield: u8,
}

/// Link model (§6.1, APP-071, APP-072).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// One-way delay, both directions, µs.
    pub one_way_delay_us: Micros,
    /// Downlink frame slot period, µs (A2 on).
    pub frame_slot_us: Ranged<Micros, 1, 18_446_744_073_709_551_615>,
    /// Uplink slot period, µs (A2 on).
    pub uplink_slot_us: Ranged<Micros, 1, 18_446_744_073_709_551_615>,
}

/// APIDs (§3.1, APP-020).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Apids {
    /// Housekeeping.
    pub housekeeping: Ranged<u16, 0, 2046>,
    /// Event reports.
    pub events: Ranged<u16, 0, 2046>,
    /// Payload science.
    pub payload: Ranged<u16, 0, 2046>,
    /// Memory dump.
    pub dump: Ranged<u16, 0, 2046>,
    /// Commands.
    pub commands: Ranged<u16, 0, 2046>,
    /// Table upload.
    pub table_upload: Ranged<u16, 0, 2046>,
}

/// TM downlink (§4.1, APP-030…APP-034).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tm {
    /// Transfer frame length, octets.
    pub frame_length: Ranged<u16, 0, 2048>,
    /// FECF present.
    pub fecf: bool,
    /// VC of housekeeping and events.
    pub vcid_realtime: Ranged<u8, 0, 7>,
    /// VC of payload and dump.
    pub vcid_payload: Ranged<u8, 0, 7>,
    /// VC of OID frames.
    pub vcid_idle: Ranged<u8, 0, 7>,
    /// Release deadline of the real-time VC, µs.
    pub release_deadline_realtime_us: Micros,
    /// Release deadline of the payload VC, µs.
    pub release_deadline_payload_us: Micros,
    /// Maximum packet length, octets.
    pub max_packet_length: u16,
    /// Idle Packet data octet.
    pub idle_pattern: u8,
    /// Valid Packet Version Number of the packets on every VC (§4.4.3).
    pub packet_version: Ranged<u8, 0, 7>,
    /// Multiplexing priority of the real-time VC; 1 is served first (§4.4.3, LNK-031).
    pub priority_realtime: Ranged<u8, 1, 255>,
    /// Multiplexing priority of the payload VC (§4.4.3, LNK-031).
    pub priority_payload: Ranged<u8, 1, 255>,
    /// Initial value of the OCF slot, as the four octets of the CLCW (§4.4.1, LNK-033).
    pub ocf_initial: ClcwWord,
    /// Whether the extractor delivers incomplete packets (§4.4.3).
    pub deliver_incomplete_packets: bool,
    /// Whether the extractor delivers Idle Packets (§4.4.3, LNK-063).
    pub deliver_idle_packets: bool,
}

/// TC uplink (§4.2, APP-040…APP-042).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tc {
    /// Maximum transfer frame length, octets: at most 1024 (Table 5-1), at least 7. The 7 is a
    /// bound of the harness, not of the Blue Book: the primary header (5 octets, 232.0 §4.1.2)
    /// plus the FDU minimum of 2.
    pub max_frame_length: Ranged<u16, 7, 1024>,
    /// FECF present.
    pub fecf: bool,
    /// The uplink VC.
    pub vcid: Ranged<u8, 0, 63>,
    /// MAP of real-time commands.
    pub map_commands: Ranged<u8, 0, 63>,
    /// MAP of the table upload.
    pub map_table_upload: Ranged<u8, 0, 63>,
    /// Maximum frame data unit length, octets: at least the Segment Header and one octet of
    /// data, at most the longest frame (1024) less the primary header (5) (232.0 §4.1.3.1.2,
    /// Table 5-4).
    pub max_fdu_length: Ranged<u16, 2, 1019>,
    /// Maximum packet length, octets.
    pub max_packet_length: u16,
    /// Valid Packet Version Number of the packets on the uplink VC (§4.4.3).
    pub packet_version: Ranged<u8, 0, 7>,
    /// Whether the uplink VC carries a Segment Header (§4.4.3).
    pub segment_header: bool,
    /// Blocking and segmentation of MAP 0 (§4.4.3).
    pub map_commands_services: MapServices,
    /// Blocking and segmentation of MAP 1 (§4.4.3).
    pub map_table_upload_services: MapServices,
}

/// The four octets of a CLCW as a word, valid by 232.0 §4.2.1: Control Word Type 0, version 00,
/// and the Reserved Spares zero (TC-030, TC-031, RS-020). Checked with the strict codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ClcwWord(u32);

impl ClcwWord {
    /// The word `word`.
    ///
    /// # Errors
    /// If the strict CLCW decoder rejects its octets.
    pub fn new(word: u32) -> Result<Self, String> {
        aldis::tc::Clcw::decode(word.to_be_bytes())
            .map(|_| Self(word))
            .map_err(|e| format!("{word:#010x} is not a CLCW: {e:?}"))
    }

    /// The word.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ClcwWord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u32::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// What a MAP permits (232.0 Table 5-4, §4.4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapServices {
    /// Blocking permitted.
    pub blocking: bool,
    /// Segmentation permitted.
    pub segmentation: bool,
}

impl Tc {
    /// The relation between the lengths: the data field (`max_fdu_length`) and the frame around it.
    /// The longest data field is 232.0 §4.1.3.1.2; the frame adds the 5-octet primary header
    /// (§4.1.2) and the FECF of 2 octets if present (§4.1.4). The minimum 7 of
    /// `max_frame_length` is a bound of the harness, not of the Blue Book: the header (5 octets)
    /// plus the FDU minimum of 2.
    fn check(&self) -> Result<(), String> {
        let fecf = if self.fecf { FECF_LEN } else { 0 };
        let fdu = u64::from(self.max_fdu_length.get());
        let longest = DATA_FIELD_MAX - fecf;
        if fdu > longest {
            return Err(format!(
                "tc.max_fdu_length {fdu} is above {longest}, the longest data field \
                 (232.0 §4.1.3.1.2)"
            ));
        }
        let frame = TC_HEADER_LEN + fdu + fecf;
        let max = u64::from(self.max_frame_length.get());
        if frame > max {
            return Err(format!(
                "tc.max_fdu_length {fdu} needs a frame of {frame} octets, above \
                 tc.max_frame_length {max}"
            ));
        }
        Ok(())
    }
}

/// Octets of the TC primary header (232.0 §4.1.2).
const TC_HEADER_LEN: u64 = 5;
/// Octets of the FECF (232.0 §4.1.4).
const FECF_LEN: u64 = 2;
/// Longest Transfer Frame Data Field without the FECF (232.0 §4.1.3.1.2).
const DATA_FIELD_MAX: u64 = 1019;

/// COP-1 parameters (§4.2, APP-044).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cop1 {
    /// FOP sliding window width K.
    pub k: Ranged<u8, 1, 255>,
    /// FARM sliding window width W.
    pub w: EvenRanged<u8, 2, 254>,
    /// `T1_Initial`, µs.
    pub t1_initial_us: Micros,
    /// `Transmission_Limit`.
    pub transmission_limit: Ranged<u8, 1, 255>,
    /// `Timeout_Type` (0 or 1).
    pub timeout_type: Ranged<u8, 0, 1>,
    /// FARM-1 internal buffer depth (COP-032).
    pub farm_buffer_depth: Ranged<u8, 1, 255>,
    /// Status Field of the CLCW (§4.4.3).
    pub farm_status_field: Ranged<u8, 0, 7>,
    /// Initial V(R) of FARM-1 (§4.4.3).
    pub farm_initial_vr: u8,
    /// Repetitions of a Type-A frame (§4.4.3).
    pub fop_repetitions_type_a: Ranged<u8, 1, 255>,
    /// Repetitions of a control command (§4.4.3).
    pub fop_repetitions_control: Ranged<u8, 1, 255>,
}

/// AOS downlink of the variant `aos` (§4.3.1, APP-169…APP-174, APP-189).
///
/// The numbers and flags of the tables of §4.3.1. The rows that name an engine policy (the
/// multiplexing scheme, the idle policy of each VC) are choices of the nodes and have no field,
/// and neither have the constants of the standard that the codec writes: the Transfer Frame
/// Version Number, the Insert Zone (absent: the nodes configure none) and the edition (Issue 5,
/// AOS-001).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags are the presence rows of 732.0 Tables 5-1, 5-3 and 5-4, one field each in the file (APP-189)"
)]
pub struct Aos {
    /// Transfer Frame Length, octets, constant (732.0 Table 5-1, §4.1.1.2). At least the primary
    /// header, one octet of the `M_PDU` Packet Zone, and the `M_PDU` Header; [`Aos::check`] adds the
    /// fields the other parameters make present.
    pub frame_length: Ranged<u16, 9, 65_535>,
    /// Frame Header Error Control present (732.0 Table 5-1, §4.1.2.6.2–3).
    pub fhec: bool,
    /// Frame Error Control Field present (732.0 Table 5-1, §4.1.6.1.2–3).
    pub fecf: bool,
    /// VC of housekeeping and events (732.0 Table 5-2).
    pub vcid_realtime: Ranged<u8, 0, 62>,
    /// VC of payload and dump (732.0 Table 5-2).
    pub vcid_payload: Ranged<u8, 0, 62>,
    /// VC of the idle `M_PDU`s (732.0 Table 5-2); VCID 63 is the OID frames', never a data VC.
    pub vcid_idle: Ranged<u8, 0, 62>,
    /// `VC_OCF` present on each of the three VCs (732.0 Table 5-3, §4.1.5.2–3).
    pub vc_ocf: bool,
    /// Release deadline of the real-time VC after its first octet is written, µs (LNK-012).
    pub release_deadline_realtime_us: Micros,
    /// Release deadline of the payload VC, µs (LNK-012).
    pub release_deadline_payload_us: Micros,
    /// Maximum Packet Length, octets (732.0 Table 5-4): at least a header and one data octet.
    pub max_packet_length: Ranged<u16, 7, 65_535>,
    /// Idle data octet (SPP-010).
    pub idle_pattern: u8,
    /// Valid Packet Version Number of the packets (732.0 Table 5-4).
    pub packet_version: Ranged<u8, 0, 7>,
    /// Multiplexing priority of the real-time VC; 1 is served first (732.0 §4.2.5.3, LNK-031).
    pub priority_realtime: Ranged<u8, 1, 255>,
    /// Multiplexing priority of the payload VC (LNK-031).
    pub priority_payload: Ranged<u8, 1, 255>,
    /// Multiplexing priority of the idle VC (LNK-031): offered only when no other VC has a
    /// released data field (LNK-014).
    pub priority_idle: Ranged<u8, 1, 255>,
    /// Whether the extractor delivers incomplete packets (732.0 Table 5-4).
    pub deliver_incomplete_packets: bool,
}

/// Octets of the AOS primary header (732.0 §4.1.2.1).
const AOS_HEADER_LEN: u64 = 6;
/// Octets of the AOS Frame Header Error Control (732.0 §4.1.2.6).
const AOS_FHEC_LEN: u64 = 2;
/// Octets of the `M_PDU` Header (732.0 §4.1.4.2.1.3).
const AOS_MPDU_HEADER_LEN: u64 = 2;
/// Octets of an OCF (732.0 §4.1.5.1).
const OCF_LEN: u64 = 4;

impl Aos {
    /// The relation between the frame length and the fields around the Packet Zone: the primary
    /// header, the FHEC, the `M_PDU` Header, the `VC_OCF`, and the FECF leave at least one octet of
    /// the Packet Zone (732.0 §4.1.1.1, §4.1.4.2.1.3 b).
    fn check(&self) -> Result<(), String> {
        let fixed = AOS_HEADER_LEN
            + AOS_MPDU_HEADER_LEN
            + if self.fhec { AOS_FHEC_LEN } else { 0 }
            + if self.vc_ocf { OCF_LEN } else { 0 }
            + if self.fecf { FECF_LEN } else { 0 };
        let frame = u64::from(self.frame_length.get());
        if frame <= fixed {
            return Err(format!(
                "aos.frame_length {frame} leaves no octet of the Packet Zone after the {fixed} \
                 octets of the other fields (732.0 §4.1.1.1)"
            ));
        }
        Ok(())
    }
}

/// USLP downlink of the variant `uslp` (§4.3.2, APP-175…APP-180, APP-189).
///
/// The numbers and flags of the tables of §4.3.2. The rows that name an engine policy (the
/// multiplexing scheme, the idle policy, the quality of service, the construction rule and the
/// UPID of each VC) are choices of the nodes and have no field, and neither have the constants
/// of the standard that the codec writes (the TFVN, VCID 63 of the OID frames, the Insert Zone
/// that is absent) nor the parameters of the Synchronization and Channel Coding sublayer, which
/// the harness does not model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UslpDown {
    /// Transfer Frame Length, octets, of the fixed-length physical channel (732.1 Table 5-1,
    /// §4.1.2.7): at least the primary header, a TFDF Header of 3 octets, and one octet of the
    /// TFDZ; [`UslpDown::check`] adds the fields the other parameters make present.
    pub frame_length: Ranged<u16, 11, 65_535>,
    /// Frame Error Control Field present (732.1 Table 5-1, §4.1.6.2.1).
    pub fecf: bool,
    /// VC of housekeeping and events (732.1 Table 5-2).
    pub vcid_realtime: Ranged<u8, 0, 62>,
    /// VC of payload and dump (732.1 Table 5-2).
    pub vcid_payload: Ranged<u8, 0, 62>,
    /// "Inclusion of OCF Required" of the master channel: every frame, OID frames included,
    /// carries the OCF (732.1 Table 5-2, §4.1.5.3; OI-76).
    pub ocf_required: bool,
    /// VC Count Length for the Sequence-Controlled service, octets (732.1 Table 5-3, Table 4-2).
    pub vcf_count_length_sequence: Ranged<u8, 0, 7>,
    /// VC Count Length for the Expedited service, octets (732.1 Table 5-3, Table 4-2; OI-99).
    pub vcf_count_length_expedited: Ranged<u8, 0, 7>,
    /// MAP ID of MAP 0, the one MAP of the downlink VCs (732.1 Table 5-3, Table 5-4).
    pub map_id: Ranged<u8, 0, 15>,
    /// Maximum delay for a TFDF to be completed on the real-time VC, µs (732.1 Table 5-3 note
    /// 4, LNK-012).
    pub release_deadline_realtime_us: Micros,
    /// Maximum delay for a TFDF to be completed on the payload VC, µs (LNK-012).
    pub release_deadline_payload_us: Micros,
    /// Maximum Packet Length, octets (732.1 Table 5-5): at least a header and one data octet.
    pub max_packet_length: Ranged<u16, 7, 65_535>,
    /// Idle data octet (SPP-010, 133.1 Table 4-2).
    pub idle_pattern: u8,
    /// Valid Packet Version Number of the Space Packets (732.1 Table 5-5).
    pub packet_version: Ranged<u8, 0, 7>,
    /// Valid Packet Version Number of the Encapsulation Packets, which are the Idle Packets only
    /// (732.1 Table 5-5; OI-06).
    pub encapsulation_packet_version: Ranged<u8, 0, 7>,
    /// Multiplexing priority of the real-time VC; 1 is served first (LNK-031).
    pub priority_realtime: Ranged<u8, 1, 255>,
    /// Multiplexing priority of the payload VC (LNK-031).
    pub priority_payload: Ranged<u8, 1, 255>,
    /// Whether the extractor delivers incomplete packets (732.1 Table 5-5).
    pub deliver_incomplete_packets: bool,
}

/// Octets of the USLP primary header of a non-truncated frame (732.1 §4.1.2.1.2).
const USLP_HEADER_LEN: u64 = 7;
/// Octets of a TFDF Header with the First Header Pointer, rule `000` to `010` (732.1
/// §4.1.4.2.1.2).
const USLP_TFDF_POINTER_HEADER_LEN: u64 = 3;
/// Octets of a TFDF Header without a pointer, rule `011` to `111` (732.1 §4.1.4.2.1.2).
const USLP_TFDF_HEADER_LEN: u64 = 1;

impl UslpDown {
    /// The relation between the frame length and the fields around the TFDZ: the primary header,
    /// the VCF Count of the expedited service, the TFDF Header, the OCF, and the FECF leave at
    /// least one octet of the TFDZ (732.1 §4.1.2.1.2, §4.1.2.12.2, §4.1.4.2.1.2).
    fn check(&self) -> Result<(), String> {
        let fixed = USLP_HEADER_LEN
            + u64::from(self.vcf_count_length_expedited.get())
            + USLP_TFDF_POINTER_HEADER_LEN
            + if self.ocf_required { OCF_LEN } else { 0 }
            + if self.fecf { FECF_LEN } else { 0 };
        let frame = u64::from(self.frame_length.get());
        if frame <= fixed {
            return Err(format!(
                "uslp_down.frame_length {frame} leaves no octet of the TFDZ after the {fixed} \
                 octets of the other fields (732.1 §4.1.2.1.2)"
            ));
        }
        Ok(())
    }
}

/// USLP uplink of the variant `uslp` (§4.3.3, APP-181…APP-184, APP-189).
///
/// The numbers and flags of the tables of §4.3.3. The rows that name an engine policy (COP in
/// effect, the multiplexing scheme) are choices of the nodes and have no field, and neither have
/// the constants of the standard that the codec writes (the TFVN, the Insert Zone that a
/// variable-length channel cannot have), the CLCW version and reporting rate, the repetitions
/// (`cop1.fop_repetitions_type_a` and `cop1.fop_repetitions_control` hold them), the delay
/// between releases (`link.uplink_slot_us` holds it), or the parameters of the Synchronization
/// and Channel Coding sublayer, which the harness does not model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UslpUp {
    /// Maximum Transfer Frame Length, octets, of the variable-length physical channel (732.1
    /// Table 5-1 note 2): at least the primary header (7), a TFDF Header of one octet, and the
    /// shortest packet (7), 15 octets; at most 65 535: the longest frame has 65 536 octets
    /// (§4.1.2.7), which a `u16` does not hold.
    pub max_frame_length: Ranged<u16, 15, 65_535>,
    /// Frame Error Control Field present (732.1 Table 5-1, §4.1.6.2.1).
    pub fecf: bool,
    /// The uplink VC, the one VC of the master channel (732.1 Table 5-2).
    pub vcid: Ranged<u8, 0, 62>,
    /// MAP ID of MAP 0, the one MAP of the VC (732.1 Table 5-3, Table 5-4).
    pub map_id: Ranged<u8, 0, 15>,
    /// VC Count Length for the Sequence-Controlled service, octets (732.1 Table 5-3, Table 4-2):
    /// the frames that carry packets are sequence-controlled (Type-AD).
    pub vcf_count_length_sequence: Ranged<u8, 0, 7>,
    /// VC Count Length for the Expedited service, octets (732.1 Table 5-3, Table 4-2; OI-66).
    pub vcf_count_length_expedited: Ranged<u8, 0, 7>,
    /// "Inclusion of OCF Allowed" of the uplink VC (732.1 Table 5-3, §4.1.5.4).
    pub ocf_allowed: bool,
    /// Maximum Packet Length of MAP 0, octets (732.1 Table 5-5): at least a header and one data
    /// octet, and at most what one frame holds ([`UslpUp::tfdz_capacity`], APP-182): the longest
    /// frame, 65 535 octets, less the primary header (7) and the TFDF Header (1).
    pub max_packet_length: Ranged<u16, 7, 65_527>,
    /// Blocking and segmentation of MAP 0 (732.1 §4.2.2.2.8, Table 5-4).
    pub map_services: MapServices,
}

impl UslpUp {
    /// The longest TFDZ a frame of the sequence-controlled service holds: the maximum frame
    /// length less the primary header, the VCF Count, the TFDF Header of one octet (rule `111`,
    /// no pointer), and the FECF (732.1 §4.1.2.1.2, §4.1.2.12.2, §4.1.4.2.1.2, §4.1.6.2.2;
    /// APP-182). Never below 1: [`UslpUp::check`] rejects a length that leaves no octet.
    fn tfdz_capacity(&self) -> u64 {
        u64::from(self.max_frame_length.get())
            .saturating_sub(USLP_HEADER_LEN)
            .saturating_sub(u64::from(self.vcf_count_length_sequence.get()))
            .saturating_sub(USLP_TFDF_HEADER_LEN)
            .saturating_sub(if self.fecf { FECF_LEN } else { 0 })
    }

    /// The relations between the lengths: the frame leaves at least one TFDZ octet, and the
    /// Maximum Packet Length fits it (APP-182).
    fn check(&self) -> Result<(), String> {
        let capacity = self.tfdz_capacity();
        let longest = u64::from(self.max_packet_length.get());
        if capacity == 0 {
            return Err(format!(
                "uslp_up.max_frame_length {} leaves no octet of the TFDZ",
                self.max_frame_length.get()
            ));
        }
        if longest > capacity {
            return Err(format!(
                "uslp_up.max_packet_length {longest} is above the {capacity} octets a frame of \
                 uslp_up.max_frame_length {} holds (APP-182)",
                self.max_frame_length.get()
            ));
        }
        Ok(())
    }
}

/// Link level of a case (§1, §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// A1: one Space Packet per link unit, sent when output.
    Packets,
    /// A2: one frame per slot, Type-BD frames only.
    Frames,
    /// A3 on: one frame per slot with COP-1 (FOP-1 and FARM-1): Type-AD, Type-BD, and Type-BC
    /// frames, and the FOP-1 directives of the ops script (APP-019, APP-148).
    Cop1,
}

impl Level {
    /// The link carries frames in slots: every level from A2 on (APP-071).
    #[must_use]
    pub const fn has_frames(self) -> bool {
        match self {
            Self::Packets => false,
            Self::Frames | Self::Cop1 => true,
        }
    }
}

/// The frame protocols of a case (§4.3, §6.5, APP-187).
///
/// The file's own type; [`Variant::to_node`] converts it for the nodes, as
/// [`OpsEntry::to_node`] converts an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    /// TM frames down and TC frames up (§4.1, §4.2). The value when `variant` is absent.
    #[default]
    TmTc,
    /// AOS frames down (§4.3.1) and TC frames up. At the levels `frames` and `cop1`.
    Aos,
    /// USLP frames down (§4.3.2) and USLP frames up (§4.3.3). At the level `cop1` only.
    Uslp,
}

impl Variant {
    /// The variant as the nodes take it.
    #[must_use]
    pub const fn to_node(self) -> refapp_scenario::Variant {
        match self {
            Self::TmTc => refapp_scenario::Variant::TmTc,
            Self::Aos => refapp_scenario::Variant::Aos,
            Self::Uslp => refapp_scenario::Variant::Uslp,
        }
    }

    /// The name the scenario file gives the variant (§6.5).
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.to_node().name()
    }

    /// Whether a case at `level` may have this variant (APP-187): `aos` at the levels `frames`
    /// and `cop1`, `uslp` at `cop1` only, `tm_tc` at every level.
    #[must_use]
    pub const fn applies_at(self, level: Level) -> bool {
        match (self, level) {
            (Self::TmTc, Level::Packets | Level::Frames | Level::Cop1)
            | (Self::Aos, Level::Frames | Level::Cop1)
            | (Self::Uslp, Level::Cop1) => true,
            (Self::Aos, Level::Packets) | (Self::Uslp, Level::Packets | Level::Frames) => false,
        }
    }
}

impl std::fmt::Display for Variant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// One case (§6.4, APP-076).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Case ID, also the golden-trace file name.
    #[schemars(regex(pattern = r"^[a-z0-9]+(-[a-z0-9]+)*$"))]
    pub id: String,
    /// Link level.
    pub level: Level,
    /// The frame protocols (§4.3, §6.5, APP-187); absent: `tm_tc`. The loader rejects a variant
    /// the level does not allow.
    #[serde(default)]
    pub variant: Variant,
    /// End instant, exclusive, µs.
    pub end_us: Micros,
    /// Version of the case's golden trace (§7.5, APP-087).
    pub trace_version: Ranged<u32, 1, 4_294_967_295>,
    /// Requirement IDs the case verifies, sorted (§7.2).
    pub requirements: Vec<String>,
    /// The ops script: what the ground node releases, and when (§6.5, APP-126). Entry `i` is
    /// the ops-script entry index `i` of the node interface (§8.1).
    #[serde(default)]
    pub ops: Vec<OpsEntry>,
    /// The fault script: faults by (direction, unit index) (§6.3, §6.5, APP-075, APP-126).
    #[serde(default)]
    pub faults: Vec<FaultEntry>,
}

/// One entry of a case's ops script (§6.5, APP-126).
///
/// Entries are in release order. An entry names its kind and the parameters of the packet the
/// ground node builds from it (§3.3.5, §3.3.6), never its octets, so that a node in another
/// language reads the same file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpsEntry {
    /// A command packet on the commands APID (§3.3.5).
    Command {
        /// Release instant, µs: the ground node outputs the packet then (§6.2).
        at_us: Micros,
        /// Execution mode and time.
        execution: Execution,
        /// Function and arguments.
        call: Call,
        /// Octets appended after the arguments, so that the packet length does not match the
        /// function code (APP-024). Absent: none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extra_octets: Option<u8>,
    },
    /// A table-upload packet on the table-upload APID (§3.3.6). Table octet `i` is
    /// `(3 · i + 1) mod 256`.
    TableUpload {
        /// Release instant, µs.
        at_us: Micros,
        /// Table ID.
        table_id: Ranged<u8, { TableId::MIN as u64 }, { TableId::MAX as u64 }>,
        /// Table length, octets.
        length: Ranged<u16, { TableLength::MIN as u64 }, { TableLength::MAX as u64 }>,
    },
    /// A FOP-1 directive: reserved from A1, issued at the level `cop1`, ignored below it
    /// (APP-019, APP-148).
    Fop1Directive {
        /// Release instant, µs.
        at_us: Micros,
        /// The directive; A3 adds the others it needs (APP-002).
        directive: Fop1Directive,
    },
}

impl OpsEntry {
    /// The entry as the ground node takes it (§6.5).
    ///
    /// # Errors
    /// `InvalidField` or `OutOfRange` for a table upload whose ID or length the node's types
    /// refuse. A parsed entry holds both in range: the types of its fields take their bounds from
    /// `TableId` and `TableLength`, so this does not fail for one.
    pub fn to_node(&self) -> Result<refapp_ground::OpsEntry, String> {
        use refapp_ground as node;
        Ok(match *self {
            Self::Command {
                execution,
                call,
                extra_octets,
                ..
            } => node::OpsEntry::Command {
                execution: match execution {
                    Execution::Immediate {} => node::Execution::Immediate,
                    Execution::TimeTagged { exec_us } => node::Execution::TimeTagged {
                        after_t0: Duration::from_micros(exec_us.get()),
                    },
                },
                call: match call {
                    Call::Noop {} => node::Call::Noop,
                    Call::DumpMemory { address, length } => node::Call::DumpMemory {
                        address,
                        length: length.get(),
                    },
                    Call::ActivateTable { table_id, crc } => node::Call::ActivateTable {
                        table_id: table_id.get(),
                        crc: Crc16::from_wire(crc),
                    },
                    Call::SetPayloadMode { mode } => {
                        node::Call::SetPayloadMode { mode: mode.get() }
                    }
                    Call::Unknown { code } => node::Call::Unknown { code: code.get() },
                },
                extra_octets: extra_octets.unwrap_or(0),
            },
            Self::TableUpload {
                table_id, length, ..
            } => node::OpsEntry::TableUpload {
                table_id: TableId::new(table_id.get())
                    .map_err(|e| format!("table_id: {:?}", e.kind()))?,
                length: TableLength::new(length.get())
                    .map_err(|e| format!("table length: {:?}", e.kind()))?,
            },
            Self::Fop1Directive { directive, .. } => node::OpsEntry::Fop1Directive(directive),
        })
    }

    /// The release instant, µs (§6.5).
    #[must_use]
    pub const fn at_us(&self) -> Micros {
        match self {
            Self::Command { at_us, .. }
            | Self::TableUpload { at_us, .. }
            | Self::Fop1Directive { at_us, .. } => *at_us,
        }
    }
}

/// Execution mode of a command (§2.3, §3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Execution {
    /// Executes when delivered; the execution-time field is zero.
    Immediate {},
    /// Executes at the on-board time T0 + `exec_us`, a multiple of the frame slot (APP-061).
    TimeTagged {
        /// Execution instant, µs after T0.
        exec_us: Micros,
    },
}

/// Function and arguments of a command (§3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "function", rename_all = "snake_case", deny_unknown_fields)]
pub enum Call {
    /// `NOOP`, function code `0x01`.
    Noop {},
    /// `DUMP_MEMORY`, function code `0x02`.
    DumpMemory {
        /// First address of the dump.
        address: u32,
        /// Length, octets.
        length: Ranged<u16, 1, 1024>,
    },
    /// `ACTIVATE_TABLE`, function code `0x03`.
    ActivateTable {
        /// Table ID.
        table_id: Ranged<u8, { TableId::MIN as u64 }, { TableId::MAX as u64 }>,
        /// CRC-16 of the table that the command claims (CMN-001).
        crc: u16,
    },
    /// `SET_PAYLOAD_MODE`, function code `0x04`.
    SetPayloadMode {
        /// New mode: 0 off, 1 on.
        mode: Ranged<u8, 0, 1>,
    },
    /// A function code that no function has (reason 1 of `CMD_REJECTED`, §3.3.2).
    Unknown {
        /// The code; not one of `0x01`…`0x04`.
        code: UnknownCode,
    },
}

impl Call {
    /// Octets of the command packet with `extra` octets after the arguments (§3.3.5).
    #[must_use]
    pub const fn packet_length(&self, extra: u8) -> u64 {
        let base: u64 = match self {
            Self::Noop {} | Self::Unknown { .. } => 14,
            Self::DumpMemory { .. } => 20,
            Self::ActivateTable { .. } => 17,
            Self::SetPayloadMode { .. } => 15,
        };
        base + extra as u64
    }
}

/// A function code that no function has: any octet but `0x01`…`0x04` (§3.3.5, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct UnknownCode(u8);

impl UnknownCode {
    /// The code `code`.
    ///
    /// # Errors
    /// If `code` is one of `0x01`…`0x04`, which have a function.
    pub fn new(code: u8) -> Result<Self, String> {
        if (1..=4).contains(&code) {
            Err(format!(
                "{code} is the code of a function, not unknown (the function codes are 1 to 4)"
            ))
        } else {
            Ok(Self(code))
        }
    }

    /// The code, not one of `0x01`…`0x04`.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for UnknownCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for UnknownCode {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        "UnknownCode".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = u8::json_schema(generator);
        schema.insert(
            "not".to_owned(),
            serde_json::json!({"minimum": 1, "maximum": 4}),
        );
        schema
    }
}

pub use refapp_ground::Fop1Directive;

/// Direction of a link unit (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Flight node to ground node.
    Down,
    /// Ground node to flight node.
    Up,
}

/// One entry of a case's fault script (§6.3, APP-075): the fault applied to the unit sent in
/// direction `dir` with index `index`, counted from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaultEntry {
    /// Direction in which the unit travels.
    pub dir: Direction,
    /// Index of the unit among those sent in that direction.
    pub index: u64,
    /// The fault.
    pub fault: Fault,
}

/// A fault and its arguments (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fault {
    /// The unit is not delivered.
    Drop {},
    /// The unit is delivered twice at the same instant.
    Duplicate {},
    /// The unit arrives `delay_us` later than normal.
    Delay {
        /// Extra delay, µs.
        delay_us: Micros,
    },
    /// One bit is inverted; frame levels only, where the FECF catches it.
    Flip {
        /// Octet number, from 0.
        octet: u32,
        /// Bit number in the octet, 0 the most significant (GEN-050).
        bit: Ranged<u8, 0, 7>,
    },
    /// Only the first `octets` octets are delivered.
    Truncate {
        /// Octets kept.
        octets: u32,
    },
}

impl Scenario {
    /// Parses and checks a scenario file.
    ///
    /// # Errors
    /// If the text does not match the model, or breaks a rule the schema cannot express. The
    /// entries of the scripts are checked first, so an error in an `ops` or `faults` entry is
    /// reported, by its entry, before an error elsewhere.
    pub fn parse(text: &str) -> Result<Self, String> {
        check_entries(text)?;
        let s: Self = serde_json::from_str(text).map_err(|e| format!("scenario.json: {e}"))?;
        s.check()?;
        Ok(s)
    }

    /// The scenario compiled into the harness.
    ///
    /// # Errors
    /// As [`Scenario::parse`].
    pub fn load() -> Result<Self, String> {
        Self::parse(SCENARIO_JSON)
    }

    /// The case with the given ID.
    ///
    /// # Errors
    /// If there is no such case.
    pub fn case(&self, id: &str) -> Result<&Case, String> {
        self.cases
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("no case `{id}` in scenario.json"))
    }

    fn check(&self) -> Result<(), String> {
        if self.scenario != "aldis-refapp" {
            return Err(format!(
                "scenario.json: scenario is `{}`, expected `aldis-refapp`",
                self.scenario
            ));
        }
        self.tc.check()?;
        self.aos.check()?;
        self.uslp_down.check()?;
        self.uslp_up.check()?;
        let mut ids = BTreeSet::new();
        for c in &self.cases {
            if !ids.insert(c.id.as_str()) {
                return Err(format!("scenario.json: duplicate case `{}`", c.id));
            }
            let valid_id = !c.id.is_empty()
                && c.id.split('-').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                });
            if !valid_id {
                return Err(format!("scenario.json: invalid case ID `{}`", c.id));
            }
            let mut sorted = c.requirements.clone();
            sorted.sort();
            sorted.dedup();
            if sorted != c.requirements || !c.requirements.iter().all(|r| is_requirement_id(r)) {
                return Err(format!(
                    "scenario.json: case `{}`: requirements must be sorted, unique requirement IDs",
                    c.id
                ));
            }
            if !c.variant.applies_at(c.level) {
                return Err(format!(
                    "scenario.json: case `{}`: the variant `{}` does not apply at the level \
                     `{:?}` (APP-187)",
                    c.id, c.variant, c.level
                ));
            }
            self.check_scripts(c)
                .map_err(|e| format!("scenario.json: case `{}`: {e}", c.id))?;
        }
        Ok(())
    }

    /// The table-upload packet of `length` table octets fits the uplink of `variant` (APP-017,
    /// APP-182): at `tm_tc` and `aos` the type of the entry holds the limit of 503 octets (the
    /// 512-octet Maximum Packet Length less the 9 octets of the packet header, APP-017); at
    /// `uslp` the packet must fit the Maximum Packet Length of MAP 0, which one frame limits.
    fn check_table_length(&self, variant: Variant, length: u16) -> Result<(), String> {
        match variant {
            Variant::TmTc | Variant::Aos => Ok(()),
            Variant::Uslp => {
                let packet = u64::from(length).saturating_add(TABLE_PACKET_OVERHEAD);
                let longest = u64::from(self.uslp_up.max_packet_length.get());
                if packet > longest {
                    return Err(format!(
                        "a table of {length} octets makes a packet of {packet} octets, above \
                         the {longest} of uslp_up.max_packet_length: at most {} octets at the \
                         variant `uslp` (APP-182)",
                        longest.saturating_sub(TABLE_PACKET_OVERHEAD)
                    ));
                }
                Ok(())
            }
        }
    }

    /// The rules of §6.5 that the schema cannot express (APP-061, APP-126, APP-128, APP-141).
    fn check_scripts(&self, c: &Case) -> Result<(), String> {
        let slot = self.link.frame_slot_us.get();
        let mut uplink = Uplink::new(self, c.variant);
        let mut previous = Micros::default();
        // Execution times of the time-tagged commands the flight node holds (§2.3).
        let mut queue: Vec<Micros> = Vec::new();
        for (i, entry) in c.ops.iter().enumerate() {
            let at_us = match entry {
                OpsEntry::Command {
                    at_us,
                    execution,
                    call,
                    extra_octets,
                } => {
                    let arrival = match c.level {
                        Level::Packets => at_us.saturating_add(self.link.one_way_delay_us),
                        // At `cop1` the nominal schedule: no fault, no retransmission, and no
                        // directive considered (APP-152).
                        Level::Frames | Level::Cop1 => {
                            uplink.command(*at_us, call.packet_length(extra_octets.unwrap_or(0)))
                        }
                    };
                    match execution {
                        Execution::Immediate {} if c.level.has_frames() => {
                            return Err(format!(
                                "ops[{i}]: an immediate command acts at its delivery, off the \
                                 {slot} µs grid at a frame level (APP-141)"
                            ));
                        }
                        Execution::Immediate {} if !arrival.is_multiple_of(slot) => {
                            return Err(format!(
                                "ops[{i}]: an immediate command must arrive on a multiple of \
                                 {slot} µs (APP-128)"
                            ));
                        }
                        Execution::Immediate {} => {}
                        Execution::TimeTagged { exec_us } => {
                            check_tagged(&mut queue, arrival, *exec_us, slot, c.level)
                                .map_err(|e| format!("ops[{i}]: {e}"))?;
                        }
                    }
                    *at_us
                }
                OpsEntry::TableUpload { at_us, length, .. } => {
                    self.check_table_length(c.variant, length.get())
                        .map_err(|e| format!("ops[{i}]: {e}"))?;
                    uplink.table_upload(*at_us, length.get());
                    *at_us
                }
                OpsEntry::Fop1Directive { at_us, .. } => *at_us,
            };
            if at_us < previous {
                return Err(format!("ops[{i}]: release instants must not decrease"));
            }
            if at_us >= c.end_us {
                return Err(format!("ops[{i}]: released at or after the end instant"));
            }
            previous = at_us;
        }
        for (i, f) in c.faults.iter().enumerate() {
            match f.fault {
                Fault::Flip { .. } if c.level == Level::Packets => {
                    return Err(format!(
                        "faults[{i}]: `flip` needs a FECF, so frame level only"
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Time-tag queue capacity of the flight node (§2.3).
const QUEUE_CAPACITY: usize = 8;

/// A time-tagged command arriving at `arrival`: its execution time is on the grid (APP-061),
/// and one that the flight node rejects at delivery (§2.3: a time tag before the arrival, or a
/// full queue) arrives on the grid too at the packet level (APP-128) and is refused at a frame
/// level (APP-141). `queue` holds the execution times of the commands the flight node still
/// has queued.
fn check_tagged(
    queue: &mut Vec<Micros>,
    arrival: Micros,
    exec_us: Micros,
    slot: Micros,
    level: Level,
) -> Result<(), String> {
    if !exec_us.is_multiple_of(slot) {
        return Err(format!(
            "execution time must be a multiple of {slot} µs (APP-061)"
        ));
    }
    queue.retain(|e| *e >= arrival);
    if exec_us >= arrival && queue.len() < QUEUE_CAPACITY {
        queue.push(exec_us);
    } else if level.has_frames() {
        return Err(
            "a command rejected at delivery has an effect off the grid at a frame level (APP-141)"
                .to_owned(),
        );
    } else if !arrival.is_multiple_of(slot) {
        return Err(format!(
            "a command rejected at delivery must arrive on a multiple of {slot} µs (APP-128)"
        ));
    }
    Ok(())
}

/// The uplink of a frame-level case as far as the script fixes it (§4.2): the segmenter sends
/// one frame at each uplink slot, the commands released at one instant share a frame data unit
/// (APP-042), and a table upload takes consecutive slots. The model serves the entries in
/// release order and finds when each command arrives, to follow the queue of the flight node
/// (APP-141); the trace has the arrivals as they happen.
struct Uplink {
    slot_us: Micros,
    delay_us: Micros,
    /// Packet octets one frame carries.
    max_data: u64,
    /// First uplink slot with no frame data unit assigned.
    free_slot: Micros,
    /// The data unit still open for blocking: release instant, length, arrival instant.
    open: Option<(Micros, u64, Micros)>,
}

/// Octets of a table-upload packet before the table (§3.3.6): header 6, ID 1, length 2.
const TABLE_PACKET_OVERHEAD: u64 = 9;

impl Uplink {
    fn new(s: &Scenario, variant: Variant) -> Self {
        Self {
            slot_us: s.link.uplink_slot_us.get(),
            delay_us: s.link.one_way_delay_us,
            max_data: match variant {
                // The Segment Header, when the VC has one, takes one octet of the frame data
                // unit (§4.2); the range of `max_fdu_length` leaves at least one more.
                Variant::TmTc | Variant::Aos => {
                    u64::from(s.tc.max_fdu_length.get()) - u64::from(s.tc.segment_header)
                }
                // The TFDZ of one frame holds the packets of one blocked request (§4.3.3,
                // APP-184); a table-upload packet is one TFDZ and is not segmented.
                Variant::Uslp => s.uslp_up.tfdz_capacity(),
            },
            free_slot: Micros::default(),
            open: None,
        }
    }

    /// First uplink slot at or after `release` that is free.
    fn slot_for(&self, release: Micros) -> Micros {
        release.next_multiple_of(self.slot_us).max(self.free_slot)
    }

    /// A command packet of `length` octets released at `release`: its arrival instant.
    fn command(&mut self, release: Micros, length: u64) -> Micros {
        if let Some((at, used, arrival)) = self.open.as_mut()
            && *at == release
            && used.saturating_add(length) <= self.max_data
        {
            *used = used.saturating_add(length);
            return *arrival;
        }
        let slot = self.slot_for(release);
        self.free_slot = slot.saturating_add(self.slot_us);
        let arrival = slot.saturating_add(self.delay_us);
        self.open = Some((release, length, arrival));
        arrival
    }

    /// A table upload of `length` table octets released at `release`.
    fn table_upload(&mut self, release: Micros, length: u16) {
        self.open = None;
        let packet = u64::from(length).saturating_add(TABLE_PACKET_OVERHEAD);
        let frames = packet.div_ceil(self.max_data);
        let slot = self.slot_for(release);
        self.free_slot = slot.saturating_add(Micros::times(self.slot_us, frames));
    }
}

/// The entries of the scripts of `text`, each parsed on its own. serde reports an error inside
/// a tagged entry at the end of the entry, so the position says little; this reports
/// ``scenario.json: case `a1-rejects`: ops[5]: 504 is outside the range 1 to 503``, the entry
/// with the error of its field, which carries the value and its range. Text that is not JSON,
/// or has another shape, is left to the parse of the whole [`Scenario`].
fn check_entries(text: &str) -> Result<(), String> {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return Ok(());
    };
    let cases = doc.get("cases").and_then(serde_json::Value::as_array);
    for case in cases.into_iter().flatten() {
        let id = case.get("id").and_then(serde_json::Value::as_str);
        entries_of::<OpsEntry>(case, "ops", id.unwrap_or("?"))?;
        entries_of::<FaultEntry>(case, "faults", id.unwrap_or("?"))?;
    }
    Ok(())
}

/// The entries of the list `list` of `case` (the case `id`) as `T`.
fn entries_of<T: serde::de::DeserializeOwned>(
    case: &serde_json::Value,
    list: &str,
    id: &str,
) -> Result<(), String> {
    let entries = case.get(list).and_then(serde_json::Value::as_array);
    for (i, entry) in entries.into_iter().flatten().enumerate() {
        serde_json::from_value::<T>(entry.clone())
            .map_err(|e| format!("scenario.json: case `{id}`: {list}[{i}]: {e}"))?;
    }
    Ok(())
}

/// `AREA-NNN` with an upper-case area and three digits.
fn is_requirement_id(s: &str) -> bool {
    s.split_once('-').is_some_and(|(area, n)| {
        !area.is_empty()
            && area.bytes().all(|b| b.is_ascii_uppercase())
            && n.len() == 3
            && n.bytes().all(|b| b.is_ascii_digit())
    })
}

/// The `refapp/` directory.
#[must_use]
pub fn refapp_dir() -> PathBuf {
    let harness = Path::new(env!("CARGO_MANIFEST_DIR"));
    harness
        .parent()
        .map_or_else(|| harness.join(".."), Path::to_path_buf)
}

/// Path of the generated JSON Schema, relative to `refapp/`.
pub const SCHEMA_PATH: &str = "scenario/scenario.schema.json";
/// Path of the generated Rust constants, relative to `refapp/`.
pub const CONSTANTS_PATH: &str = "refapp-scenario/src/generated.rs";

/// The published JSON Schema of the scenario file (APP-107).
///
/// # Errors
/// If serialization fails.
pub fn schema() -> Result<String, String> {
    let schema = schemars::schema_for!(Scenario);
    serde_json::to_string_pretty(&schema)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}

/// A decimal literal with `_` every three digits when it has more than four digits.
fn literal(v: u64) -> String {
    let digits = v.to_string();
    if digits.len() <= 4 {
        return digits;
    }
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

/// One generated constant: name, Rust type, value, documentation.
type Const<'a> = (&'a str, &'a str, String, &'a str);

fn module(out: &mut String, name: &str, doc: &str, consts: &[Const<'_>]) {
    let _ = writeln!(out, "\n/// {doc}\npub mod {name} {{");
    for (i, (c, ty, value, doc)) in consts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "    /// {doc}\n    pub const {c}: {ty} = {value};");
    }
    out.push_str("}\n");
}

/// Rust constants for `refapp-scenario` (APP-107).
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one flat table of every generated constant, easier to review than split up"
)]
pub fn rust_constants(s: &Scenario) -> String {
    let n = |v: u64| literal(v);
    let mut out = String::from(
        "//! Scenario values generated from `refapp/scenario/scenario.json` (APP-107).\n\
         //!\n\
         //! Do not edit: run `cargo run -p refapp-harness -- gen-scenario` after changing the\n\
         //! scenario file. Section numbers refer to `docs/reference-application.md`.\n\n",
    );
    let _ = writeln!(
        out,
        "/// Scenario identifier in every golden-trace header (§7.2).\n\
         pub const SCENARIO: &str = {:?};\n\n\
         /// Scenario version (§7.2, APP-002).\n\
         pub const SCENARIO_VERSION: u16 = {};\n\n\
         /// Spacecraft ID of every frame (§3.4, APP-026); fictional (OI-26).\n\
         pub const SPACECRAFT_ID: u16 = {};",
        s.scenario,
        n(s.scenario_version.into()),
        n(s.spacecraft_id.get().into())
    );
    module(
        &mut out,
        "time",
        "Time model (§5, APP-060).",
        &[
            (
                "T0_COARSE_S",
                "u32",
                n(s.time.t0_coarse_s.into()),
                "CUC coarse time at the start of the run, s since 1958-01-01 TAI.",
            ),
            (
                "CUC_COARSE_OCTETS",
                "u8",
                n(s.time.cuc_coarse_octets.get().into()),
                "CUC coarse octets.",
            ),
            (
                "CUC_FINE_OCTETS",
                "u8",
                n(s.time.cuc_fine_octets.get().into()),
                "CUC fractional octets.",
            ),
            (
                "CUC_EXPLICIT_PFIELD",
                "u8",
                n(s.time.cuc_explicit_pfield.into()),
                "Explicit P-field octet (301.0 §3.2.2).",
            ),
        ],
    );
    module(
        &mut out,
        "link",
        "Link model (§6.1).",
        &[
            (
                "ONE_WAY_DELAY_US",
                "u64",
                n(s.link.one_way_delay_us.get()),
                "One-way delay, both directions, µs (APP-072).",
            ),
            (
                "FRAME_SLOT_US",
                "u64",
                n(s.link.frame_slot_us.get().get()),
                "Downlink frame slot period, µs (APP-071).",
            ),
            (
                "UPLINK_SLOT_US",
                "u64",
                n(s.link.uplink_slot_us.get().get()),
                "Uplink slot period, µs (APP-071).",
            ),
        ],
    );
    let a = &s.apid;
    module(
        &mut out,
        "apid",
        "APIDs (§3.1, APP-020).",
        &[
            (
                "HOUSEKEEPING",
                "u16",
                n(a.housekeeping.get().into()),
                "Housekeeping.",
            ),
            ("EVENTS", "u16", n(a.events.get().into()), "Event reports."),
            (
                "PAYLOAD",
                "u16",
                n(a.payload.get().into()),
                "Payload science.",
            ),
            ("DUMP", "u16", n(a.dump.get().into()), "Memory dump."),
            ("COMMANDS", "u16", n(a.commands.get().into()), "Commands."),
            (
                "TABLE_UPLOAD",
                "u16",
                n(a.table_upload.get().into()),
                "Table upload.",
            ),
        ],
    );
    let tm = &s.tm;
    module(
        &mut out,
        "tm",
        "TM downlink (§4.1).",
        &[
            (
                "FRAME_LENGTH",
                "u16",
                n(tm.frame_length.get().into()),
                "Transfer frame length, octets (APP-030).",
            ),
            (
                "FECF",
                "bool",
                tm.fecf.to_string(),
                "FECF present (APP-030).",
            ),
            (
                "VCID_REALTIME",
                "u8",
                n(tm.vcid_realtime.get().into()),
                "VC of housekeeping and events (APP-032).",
            ),
            (
                "VCID_PAYLOAD",
                "u8",
                n(tm.vcid_payload.get().into()),
                "VC of payload and dump (APP-032).",
            ),
            (
                "VCID_IDLE",
                "u8",
                n(tm.vcid_idle.get().into()),
                "VC of OID frames (APP-034).",
            ),
            (
                "RELEASE_DEADLINE_REALTIME_US",
                "u64",
                n(tm.release_deadline_realtime_us.get()),
                "Release deadline of the real-time VC, µs (APP-033).",
            ),
            (
                "RELEASE_DEADLINE_PAYLOAD_US",
                "u64",
                n(tm.release_deadline_payload_us.get()),
                "Release deadline of the payload VC, µs (APP-033).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(tm.max_packet_length.into()),
                "Maximum packet length, octets (APP-032).",
            ),
            (
                "IDLE_PATTERN",
                "u8",
                n(tm.idle_pattern.into()),
                "Idle Packet data octet (APP-033).",
            ),
            (
                "PACKET_VERSION",
                "u8",
                n(tm.packet_version.get().into()),
                "Valid Packet Version Number (APP-136).",
            ),
            (
                "PRIORITY_REALTIME",
                "u8",
                n(tm.priority_realtime.get().into()),
                "Multiplexing priority of the real-time VC (APP-136).",
            ),
            (
                "PRIORITY_PAYLOAD",
                "u8",
                n(tm.priority_payload.get().into()),
                "Multiplexing priority of the payload VC (APP-136).",
            ),
            (
                "OCF_INITIAL",
                "u32",
                format!(
                    "{:#06x}_{:04x}",
                    tm.ocf_initial.get() >> 16,
                    tm.ocf_initial.get() & 0xFFFF
                ),
                "Initial value of the OCF slot (APP-136).",
            ),
            (
                "DELIVER_INCOMPLETE_PACKETS",
                "bool",
                tm.deliver_incomplete_packets.to_string(),
                "The extractor delivers incomplete packets (APP-136).",
            ),
            (
                "DELIVER_IDLE_PACKETS",
                "bool",
                tm.deliver_idle_packets.to_string(),
                "The extractor delivers Idle Packets (APP-136).",
            ),
        ],
    );
    let tc = &s.tc;
    module(
        &mut out,
        "tc",
        "TC uplink (§4.2).",
        &[
            (
                "MAX_FRAME_LENGTH",
                "u16",
                n(tc.max_frame_length.get().into()),
                "Maximum transfer frame length, octets (APP-040).",
            ),
            (
                "FECF",
                "bool",
                tc.fecf.to_string(),
                "FECF present (APP-040).",
            ),
            (
                "VCID",
                "u8",
                n(tc.vcid.get().into()),
                "The uplink VC (APP-041).",
            ),
            (
                "MAP_COMMANDS",
                "u8",
                n(tc.map_commands.get().into()),
                "MAP of real-time commands (APP-041).",
            ),
            (
                "MAP_TABLE_UPLOAD",
                "u8",
                n(tc.map_table_upload.get().into()),
                "MAP of the table upload (APP-041).",
            ),
            (
                "MAX_FDU_LENGTH",
                "u16",
                n(tc.max_fdu_length.get().into()),
                "Maximum frame data unit length, octets (APP-041).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(tc.max_packet_length.into()),
                "Maximum packet length, octets (APP-041).",
            ),
            (
                "PACKET_VERSION",
                "u8",
                n(tc.packet_version.get().into()),
                "Valid Packet Version Number (APP-136).",
            ),
            (
                "SEGMENT_HEADER",
                "bool",
                tc.segment_header.to_string(),
                "The uplink VC carries a Segment Header (APP-136).",
            ),
            (
                "MAP_COMMANDS_BLOCKING",
                "bool",
                tc.map_commands_services.blocking.to_string(),
                "MAP 0 permits blocking (APP-136).",
            ),
            (
                "MAP_COMMANDS_SEGMENTATION",
                "bool",
                tc.map_commands_services.segmentation.to_string(),
                "MAP 0 permits segmentation (APP-136).",
            ),
            (
                "MAP_TABLE_UPLOAD_BLOCKING",
                "bool",
                tc.map_table_upload_services.blocking.to_string(),
                "MAP 1 permits blocking (APP-136).",
            ),
            (
                "MAP_TABLE_UPLOAD_SEGMENTATION",
                "bool",
                tc.map_table_upload_services.segmentation.to_string(),
                "MAP 1 permits segmentation (APP-136).",
            ),
        ],
    );
    let c = &s.cop1;
    module(
        &mut out,
        "cop1",
        "COP-1 parameters (§4.2, APP-044).",
        &[
            (
                "K",
                "u8",
                n(c.k.get().into()),
                "FOP sliding window width K.",
            ),
            (
                "W",
                "u8",
                n(c.w.get().into()),
                "FARM sliding window width W.",
            ),
            (
                "T1_INITIAL_US",
                "u64",
                n(c.t1_initial_us.get()),
                "`T1_Initial`, µs.",
            ),
            (
                "TRANSMISSION_LIMIT",
                "u8",
                n(c.transmission_limit.get().into()),
                "`Transmission_Limit`.",
            ),
            (
                "TIMEOUT_TYPE",
                "u8",
                n(c.timeout_type.get().into()),
                "`Timeout_Type`.",
            ),
            (
                "FARM_BUFFER_DEPTH",
                "u8",
                n(c.farm_buffer_depth.get().into()),
                "FARM-1 internal buffer depth (COP-032).",
            ),
            (
                "FARM_STATUS_FIELD",
                "u8",
                n(c.farm_status_field.get().into()),
                "Status Field of the CLCW (APP-136).",
            ),
            (
                "FARM_INITIAL_VR",
                "u8",
                n(c.farm_initial_vr.into()),
                "Initial V(R) of FARM-1 (APP-136).",
            ),
            (
                "FOP_REPETITIONS_TYPE_A",
                "u8",
                n(c.fop_repetitions_type_a.get().into()),
                "Repetitions of a Type-A frame (APP-136).",
            ),
            (
                "FOP_REPETITIONS_CONTROL",
                "u8",
                n(c.fop_repetitions_control.get().into()),
                "Repetitions of a control command (APP-136).",
            ),
        ],
    );
    let aos = &s.aos;
    module(
        &mut out,
        "aos",
        "AOS downlink of the variant `aos` (§4.3.1, APP-189).",
        &[
            (
                "FRAME_LENGTH",
                "u16",
                n(aos.frame_length.get().into()),
                "Transfer Frame Length, octets (APP-169).",
            ),
            (
                "FHEC",
                "bool",
                aos.fhec.to_string(),
                "Frame Header Error Control present (APP-169).",
            ),
            (
                "FECF",
                "bool",
                aos.fecf.to_string(),
                "Frame Error Control Field present (APP-169).",
            ),
            (
                "VCID_REALTIME",
                "u8",
                n(aos.vcid_realtime.get().into()),
                "VC of housekeeping and events (APP-170).",
            ),
            (
                "VCID_PAYLOAD",
                "u8",
                n(aos.vcid_payload.get().into()),
                "VC of payload and dump (APP-170).",
            ),
            (
                "VCID_IDLE",
                "u8",
                n(aos.vcid_idle.get().into()),
                "VC of the idle `M_PDU`s (APP-170).",
            ),
            (
                "VC_OCF",
                "bool",
                aos.vc_ocf.to_string(),
                "`VC_OCF` present on each VC (APP-173).",
            ),
            (
                "RELEASE_DEADLINE_REALTIME_US",
                "u64",
                n(aos.release_deadline_realtime_us.get()),
                "Release deadline of the real-time VC, µs (APP-172).",
            ),
            (
                "RELEASE_DEADLINE_PAYLOAD_US",
                "u64",
                n(aos.release_deadline_payload_us.get()),
                "Release deadline of the payload VC, µs (APP-172).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(aos.max_packet_length.get().into()),
                "Maximum Packet Length, octets (APP-172).",
            ),
            (
                "IDLE_PATTERN",
                "u8",
                n(aos.idle_pattern.into()),
                "Idle data octet (APP-172).",
            ),
            (
                "PACKET_VERSION",
                "u8",
                n(aos.packet_version.get().into()),
                "Valid Packet Version Number (APP-189).",
            ),
            (
                "PRIORITY_REALTIME",
                "u8",
                n(aos.priority_realtime.get().into()),
                "Multiplexing priority of the real-time VC (APP-170).",
            ),
            (
                "PRIORITY_PAYLOAD",
                "u8",
                n(aos.priority_payload.get().into()),
                "Multiplexing priority of the payload VC (APP-170).",
            ),
            (
                "PRIORITY_IDLE",
                "u8",
                n(aos.priority_idle.get().into()),
                "Multiplexing priority of the idle VC (APP-170).",
            ),
            (
                "DELIVER_INCOMPLETE_PACKETS",
                "bool",
                aos.deliver_incomplete_packets.to_string(),
                "The extractor delivers incomplete packets (APP-189).",
            ),
        ],
    );
    let down = &s.uslp_down;
    module(
        &mut out,
        "uslp_down",
        "USLP downlink of the variant `uslp` (§4.3.2, APP-189).",
        &[
            (
                "FRAME_LENGTH",
                "u16",
                n(down.frame_length.get().into()),
                "Transfer Frame Length, octets (APP-175).",
            ),
            (
                "FECF",
                "bool",
                down.fecf.to_string(),
                "Frame Error Control Field present (APP-175).",
            ),
            (
                "VCID_REALTIME",
                "u8",
                n(down.vcid_realtime.get().into()),
                "VC of housekeeping and events (APP-175).",
            ),
            (
                "VCID_PAYLOAD",
                "u8",
                n(down.vcid_payload.get().into()),
                "VC of payload and dump (APP-175).",
            ),
            (
                "OCF_REQUIRED",
                "bool",
                down.ocf_required.to_string(),
                "Inclusion of OCF Required of the master channel (APP-175).",
            ),
            (
                "VCF_COUNT_LENGTH_SEQUENCE",
                "u8",
                n(down.vcf_count_length_sequence.get().into()),
                "VC Count Length for the Sequence-Controlled service, octets (APP-176).",
            ),
            (
                "VCF_COUNT_LENGTH_EXPEDITED",
                "u8",
                n(down.vcf_count_length_expedited.get().into()),
                "VC Count Length for the Expedited service, octets (APP-176).",
            ),
            (
                "MAP_ID",
                "u8",
                n(down.map_id.get().into()),
                "MAP ID of MAP 0 (APP-176).",
            ),
            (
                "RELEASE_DEADLINE_REALTIME_US",
                "u64",
                n(down.release_deadline_realtime_us.get()),
                "Maximum delay for a TFDF to be completed on the real-time VC, µs (APP-177).",
            ),
            (
                "RELEASE_DEADLINE_PAYLOAD_US",
                "u64",
                n(down.release_deadline_payload_us.get()),
                "Maximum delay for a TFDF to be completed on the payload VC, µs (APP-177).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(down.max_packet_length.get().into()),
                "Maximum Packet Length, octets (APP-177).",
            ),
            (
                "IDLE_PATTERN",
                "u8",
                n(down.idle_pattern.into()),
                "Idle data octet (APP-177).",
            ),
            (
                "PACKET_VERSION",
                "u8",
                n(down.packet_version.get().into()),
                "Valid Packet Version Number of the Space Packets (APP-189).",
            ),
            (
                "ENCAPSULATION_PACKET_VERSION",
                "u8",
                n(down.encapsulation_packet_version.get().into()),
                "Valid Packet Version Number of the Encapsulation Packets (APP-189).",
            ),
            (
                "PRIORITY_REALTIME",
                "u8",
                n(down.priority_realtime.get().into()),
                "Multiplexing priority of the real-time VC (APP-175).",
            ),
            (
                "PRIORITY_PAYLOAD",
                "u8",
                n(down.priority_payload.get().into()),
                "Multiplexing priority of the payload VC (APP-175).",
            ),
            (
                "DELIVER_INCOMPLETE_PACKETS",
                "bool",
                down.deliver_incomplete_packets.to_string(),
                "The extractor delivers incomplete packets (APP-189).",
            ),
        ],
    );
    let up = &s.uslp_up;
    module(
        &mut out,
        "uslp_up",
        "USLP uplink of the variant `uslp` (§4.3.3, APP-189).",
        &[
            (
                "MAX_FRAME_LENGTH",
                "u16",
                n(up.max_frame_length.get().into()),
                "Maximum Transfer Frame Length, octets (APP-182).",
            ),
            (
                "FECF",
                "bool",
                up.fecf.to_string(),
                "Frame Error Control Field present (APP-181).",
            ),
            (
                "VCID",
                "u8",
                n(up.vcid.get().into()),
                "The uplink VC (APP-181).",
            ),
            (
                "MAP_ID",
                "u8",
                n(up.map_id.get().into()),
                "MAP ID of MAP 0 (APP-181).",
            ),
            (
                "VCF_COUNT_LENGTH_SEQUENCE",
                "u8",
                n(up.vcf_count_length_sequence.get().into()),
                "VC Count Length for the Sequence-Controlled service, octets (APP-181).",
            ),
            (
                "VCF_COUNT_LENGTH_EXPEDITED",
                "u8",
                n(up.vcf_count_length_expedited.get().into()),
                "VC Count Length for the Expedited service, octets (APP-181).",
            ),
            (
                "OCF_ALLOWED",
                "bool",
                up.ocf_allowed.to_string(),
                "Inclusion of OCF Allowed (APP-181).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(up.max_packet_length.get().into()),
                "Maximum Packet Length of MAP 0, octets (APP-182).",
            ),
            (
                "MAP_BLOCKING",
                "bool",
                up.map_services.blocking.to_string(),
                "MAP 0 permits blocking (APP-181).",
            ),
            (
                "MAP_SEGMENTATION",
                "bool",
                up.map_services.segmentation.to_string(),
                "MAP 0 permits segmentation (APP-181).",
            ),
        ],
    );
    out
}

/// Generated files and their expected contents, paths relative to `refapp/`.
///
/// # Errors
/// If the scenario file is invalid.
pub fn generated_files() -> Result<Vec<(&'static str, String)>, String> {
    let s = Scenario::load()?;
    Ok(vec![
        (SCHEMA_PATH, schema()?),
        (CONSTANTS_PATH, rust_constants(&s)),
    ])
}

/// Writes the generated files, or with `check` reports the stale ones. Returns the number of
/// stale files.
///
/// # Errors
/// If the scenario file is invalid or a file cannot be written.
pub fn generate(check: bool) -> Result<usize, String> {
    let dir = refapp_dir();
    let mut stale = 0;
    for (path, body) in generated_files()? {
        let file = dir.join(path);
        if check {
            if std::fs::read_to_string(&file).ok().as_deref() != Some(body.as_str()) {
                println!(
                    "error: refapp/{path} is out of date; run `cargo run -p refapp-harness -- gen-scenario`"
                );
                stale += 1;
            }
        } else {
            std::fs::write(&file, body).map_err(|e| format!("refapp/{path}: {e}"))?;
            println!("wrote refapp/{path}");
        }
    }
    if check && stale == 0 {
        println!("gen-scenario: up to date");
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::{
        Call, Direction, Execution, Fault, FaultEntry, Level, Micros, OpsEntry, Scenario,
        is_requirement_id, literal,
    };
    use crate::ranged::Ranged;

    #[test]
    fn literals_and_ids() {
        // req: APP-107
        assert_eq!(literal(7), "7");
        assert_eq!(literal(1024), "1024");
        assert_eq!(literal(10_000), "10_000");
        assert_eq!(literal(2_100_000_000), "2_100_000_000");
        assert!(is_requirement_id("APP-080"));
        assert!(!is_requirement_id("APP-80"));
        assert!(!is_requirement_id("app-080"));
    }

    #[test]
    fn rejects_bad_files() {
        // req: APP-107
        let good = super::SCENARIO_JSON;
        assert!(Scenario::parse(good).is_ok());
        let unknown = good.replacen(
            "\"scenario_version\"",
            "\"extra\": 1, \"scenario_version\"",
            1,
        );
        assert!(Scenario::parse(&unknown).is_err());
        let unsorted = good.replace(
            "[\"APP-080\", \"APP-081\", \"APP-085\"]",
            "[\"APP-081\", \"APP-080\"]",
        );
        assert!(Scenario::parse(&unsorted).is_err());
        let bad_id = good.replace("\"a0-empty\"", "\"A0 empty\"");
        assert!(Scenario::parse(&bad_id).is_err());
        let wrong_name = good.replace("\"aldis-refapp\"", "\"other\"");
        assert!(Scenario::parse(&wrong_name).is_err());
    }

    /// The scenario file with the value at JSON pointer `pointer` replaced by `value`.
    fn mutated(pointer: &str, value: u64) -> String {
        let mut doc: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        *doc.pointer_mut(pointer).unwrap() = value.into();
        doc.to_string()
    }

    /// Every field of the scenario with a range: the pointer, the lowest and the highest
    /// value.
    const RANGES: &[(&str, u64, u64)] = &[
        ("/spacecraft_id", 0, 1023),
        ("/time/cuc_coarse_octets", 1, 4),
        ("/time/cuc_fine_octets", 0, 3),
        ("/link/frame_slot_us", 1, u64::MAX),
        ("/link/uplink_slot_us", 1, u64::MAX),
        ("/apid/housekeeping", 0, 2046),
        ("/apid/events", 0, 2046),
        ("/apid/payload", 0, 2046),
        ("/apid/dump", 0, 2046),
        ("/apid/commands", 0, 2046),
        ("/apid/table_upload", 0, 2046),
        ("/tm/frame_length", 0, 2048),
        ("/tm/vcid_realtime", 0, 7),
        ("/tm/vcid_payload", 0, 7),
        ("/tm/vcid_idle", 0, 7),
        ("/tc/max_frame_length", 7, 1024),
        ("/tc/vcid", 0, 63),
        ("/tc/map_commands", 0, 63),
        ("/tc/map_table_upload", 0, 63),
        ("/tc/max_fdu_length", 2, 1019),
        ("/cop1/k", 1, 255),
        ("/cop1/w", 2, 254),
        ("/cop1/transmission_limit", 1, 255),
        ("/cop1/timeout_type", 0, 1),
        ("/cop1/farm_buffer_depth", 1, 255),
        ("/aos/frame_length", 9, 65_535),
        ("/aos/vcid_realtime", 0, 62),
        ("/aos/vcid_payload", 0, 62),
        ("/aos/vcid_idle", 0, 62),
        ("/aos/max_packet_length", 7, 65_535),
        ("/aos/packet_version", 0, 7),
        ("/aos/priority_realtime", 1, 255),
        ("/aos/priority_payload", 1, 255),
        ("/aos/priority_idle", 1, 255),
        ("/uslp_down/frame_length", 11, 65_535),
        ("/uslp_down/vcid_realtime", 0, 62),
        ("/uslp_down/vcid_payload", 0, 62),
        ("/uslp_down/vcf_count_length_sequence", 0, 7),
        ("/uslp_down/vcf_count_length_expedited", 0, 7),
        ("/uslp_down/map_id", 0, 15),
        ("/uslp_down/max_packet_length", 7, 65_535),
        ("/uslp_down/packet_version", 0, 7),
        ("/uslp_down/encapsulation_packet_version", 0, 7),
        ("/uslp_down/priority_realtime", 1, 255),
        ("/uslp_down/priority_payload", 1, 255),
        ("/uslp_up/max_frame_length", 15, 65_535),
        ("/uslp_up/vcid", 0, 62),
        ("/uslp_up/map_id", 0, 15),
        ("/uslp_up/vcf_count_length_sequence", 0, 7),
        ("/uslp_up/vcf_count_length_expedited", 0, 7),
        ("/uslp_up/max_packet_length", 7, 65_527),
    ];

    #[test]
    fn a_value_outside_its_range_is_rejected() {
        // req: APP-107
        for &(pointer, min, max) in RANGES {
            if let Some(below) = min.checked_sub(1) {
                let e = Scenario::parse(&mutated(pointer, below)).unwrap_err();
                assert!(e.contains("outside the range"), "{pointer} = {below}: {e}");
            }
            if let Some(above) = max.checked_add(1) {
                // A value above the range of the field's type fails in the type, not the range.
                let e = Scenario::parse(&mutated(pointer, above)).unwrap_err();
                assert!(e.contains("scenario.json"), "{pointer} = {above}: {e}");
            }
        }
    }

    #[test]
    fn an_odd_window_is_rejected() {
        // req: APP-107
        let e = Scenario::parse(&mutated("/cop1/w", 5)).unwrap_err();
        assert!(e.contains("odd"), "{e}");
        let schema: serde_json::Value = serde_json::from_str(&super::schema().unwrap()).unwrap();
        assert_eq!(schema["$defs"]["Cop1"]["properties"]["w"]["multipleOf"], 2);
    }

    #[test]
    fn the_ends_of_every_range_are_accepted() {
        // req: APP-107
        for &(pointer, min, max) in RANGES {
            for v in [min, max] {
                let mut doc: serde_json::Value =
                    serde_json::from_str(&mutated(pointer, v)).unwrap();
                if pointer.starts_with("/link/") || pointer.starts_with("/tc/") {
                    // The ops' execution times must lie on the slot grid (APP-061), and the
                    // frame-level cases' arrivals on the uplink grid (APP-141), rules across
                    // fields (slots, TC frame lengths) that no value but the shipped one meets; this test is of
                    // the range.
                    for case in doc["cases"].as_array_mut().unwrap() {
                        case.as_object_mut().unwrap().remove("ops");
                    }
                }
                // The lengths of the TC frame relate to each other (232.0 §4.1.3.1.2); this
                // test is of the range, so the other length gives the field room.
                if pointer == "/tc/max_frame_length" {
                    doc["tc"]["max_fdu_length"] = 2.into();
                    doc["tc"]["fecf"] = false.into();
                } else if pointer == "/tc/max_fdu_length" {
                    doc["tc"]["max_frame_length"] = 1024.into();
                    doc["tc"]["fecf"] = false.into();
                }
                // Likewise the frame lengths of the variants relate to the fields around the
                // packet zone, the TFDZ, and the packet (APP-169, APP-175, APP-182).
                match pointer {
                    "/aos/frame_length" => {
                        for flag in ["fhec", "fecf", "vc_ocf"] {
                            doc["aos"][flag] = false.into();
                        }
                    }
                    "/uslp_down/frame_length" => {
                        doc["uslp_down"]["fecf"] = false.into();
                        doc["uslp_down"]["ocf_required"] = false.into();
                        doc["uslp_down"]["vcf_count_length_expedited"] = 0.into();
                    }
                    "/uslp_up/max_frame_length" | "/uslp_up/max_packet_length" => {
                        doc["uslp_up"]["fecf"] = false.into();
                        doc["uslp_up"]["vcf_count_length_sequence"] = 0.into();
                        if pointer == "/uslp_up/max_frame_length" {
                            doc["uslp_up"]["max_packet_length"] = 7.into();
                        } else {
                            doc["uslp_up"]["max_frame_length"] = 65_535.into();
                        }
                    }
                    "/uslp_up/vcf_count_length_sequence" => {
                        doc["uslp_up"]["max_packet_length"] = 7.into();
                    }
                    _ => {}
                }
                let r = Scenario::parse(&doc.to_string());
                assert!(r.is_ok(), "{pointer} = {v}: {:?}", r.err());
            }
        }
    }

    #[test]
    fn the_schema_states_the_ranges_the_parser_enforces() {
        // req: APP-107
        let schema: serde_json::Value = serde_json::from_str(&super::schema().unwrap()).unwrap();
        for &(pointer, min, max) in RANGES {
            let (section, field) = pointer[1..].split_once('/').unwrap_or(("", &pointer[1..]));
            let def = if section.is_empty() {
                &schema["properties"][field]
            } else {
                let name = match section {
                    "apid" => "Apids".to_owned(),
                    "uslp_down" => "UslpDown".to_owned(),
                    "uslp_up" => "UslpUp".to_owned(),
                    _ => format!("{}{}", section[..1].to_uppercase(), &section[1..]),
                };
                &schema["$defs"][name]["properties"][field]
            };
            assert_eq!(def["minimum"], min, "{pointer}");
            assert_eq!(def["maximum"], max, "{pointer}");
        }
    }

    fn rejects(s: &Scenario, what: &str) -> bool {
        s.check().is_err_and(|e| e.contains(what))
    }

    #[test]
    fn script_entries_of_the_a1_cases_parse_and_check() {
        // req: APP-126, APP-127, APP-128
        let s = Scenario::load().unwrap();
        s.check().unwrap();
        let rejects_case = s.case("a1-rejects").unwrap();
        assert_eq!(rejects_case.ops.len(), 18);
        assert_eq!(s.case("a1-downlink-faults").unwrap().faults.len(), 5);
        // An entry names parameters, not octets: it serializes back to the same JSON.
        let file: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        let entries = file["cases"].as_array().unwrap();
        for (case, json) in s.cases.iter().zip(entries).skip(1) {
            assert_eq!(&serde_json::to_value(case).unwrap()["ops"], &json["ops"]);
        }
    }

    #[test]
    fn script_rules_the_schema_cannot_express() {
        // req: APP-061, APP-126, APP-128
        let ok = Scenario::load().unwrap();
        let mut s = ok.clone();
        // An immediate command that arrives off the 125 000 µs grid.
        s.cases[3].ops[0] = OpsEntry::Command {
            at_us: Micros::new(100_000),
            execution: Execution::Immediate {},
            call: Call::Noop {},
            extra_octets: None,
        };
        assert!(rejects(&s, "APP-128"));
        // An execution time off the grid.
        let mut s = ok.clone();
        s.cases[1].ops[0] = OpsEntry::Command {
            at_us: Micros::new(1_000_000),
            execution: Execution::TimeTagged {
                exec_us: Micros::new(6_000_001),
            },
            call: Call::Noop {},
            extra_octets: None,
        };
        assert!(rejects(&s, "APP-061"));
        // Release order, and the end instant.
        let mut s = ok.clone();
        s.cases[3].ops.swap(0, 1);
        assert!(rejects(&s, "must not decrease"));
        let mut s = ok.clone();
        s.cases[3].end_us = Micros::new(10_000_000);
        assert!(rejects(&s, "end instant"));
        // A bit flip needs a FECF.
        let mut s = ok;
        s.cases[4].faults.push(FaultEntry {
            dir: Direction::Down,
            index: 0,
            fault: Fault::Flip {
                octet: 0,
                bit: Ranged::new(0).unwrap(),
            },
        });
        assert!(rejects(&s, "flip"));
    }

    /// The `a1-rejects` case with its ops entry `i` replaced.
    fn rejects_with(i: usize, entry: OpsEntry) -> Scenario {
        let mut s = Scenario::load().unwrap();
        s.cases[3].ops[i] = entry;
        s
    }

    fn command(at_us: u64, execution: Execution, call: Call) -> OpsEntry {
        OpsEntry::Command {
            at_us: Micros::new(at_us),
            execution,
            call,
            extra_octets: None,
        }
    }

    #[test]
    fn rejected_at_delivery_commands_arrive_on_the_grid() {
        // req: APP-128
        // Reason 2: a time tag before the arrival, released off the grid.
        let tagged = Execution::TimeTagged {
            exec_us: Micros::new(3_000_000),
        };
        let s = rejects_with(3, command(3_600_000, tagged, Call::Noop {}));
        assert!(rejects(&s, "rejected at delivery"));
        // The same command on the grid is fine.
        assert!(
            rejects_with(3, command(3_615_000, tagged, Call::Noop {}))
                .check()
                .is_ok()
        );
        // Reason 4: the ninth command of a full queue, released off the grid.
        let mut s = Scenario::load().unwrap();
        s.cases[3].ops[17] = command(
            10_115_001,
            Execution::TimeTagged {
                exec_us: Micros::new(20_000_000),
            },
            Call::Noop {},
        );
        assert!(rejects(&s, "rejected at delivery"));
        // A command that only queues needs no grid: `a1-nominal` releases on whole seconds.
        Scenario::load().unwrap().check().unwrap();
    }

    /// Parameters of the entries with a range: the entry's JSON with `{}` for the value, the
    /// lowest and the highest value.
    const ENTRY_RANGES: &[(&str, u64, u64)] = &[
        (
            r#"{"kind":"table_upload","at_us":0,"table_id":{},"length":1}"#,
            1,
            255,
        ),
        (
            r#"{"kind":"table_upload","at_us":0,"table_id":1,"length":{}}"#,
            1,
            503,
        ),
        (
            r#"{"kind":"command","at_us":0,"execution":{"mode":"immediate"},"call":{"function":"dump_memory","address":0,"length":{}}}"#,
            1,
            1024,
        ),
        (
            r#"{"kind":"command","at_us":0,"execution":{"mode":"immediate"},"call":{"function":"activate_table","table_id":{},"crc":0}}"#,
            1,
            255,
        ),
        (
            r#"{"kind":"command","at_us":0,"execution":{"mode":"immediate"},"call":{"function":"set_payload_mode","mode":{}}}"#,
            0,
            1,
        ),
    ];

    #[test]
    fn script_ranges_are_enforced() {
        // req: APP-126
        for &(template, min, max) in ENTRY_RANGES {
            let parse =
                |v: u64| serde_json::from_str::<OpsEntry>(&template.replace("{}", &v.to_string()));
            for v in [min, max] {
                assert!(parse(v).is_ok(), "{template}: {v}: {:?}", parse(v).err());
            }
            if let Some(below) = min.checked_sub(1) {
                let e = parse(below).unwrap_err().to_string();
                assert!(e.contains("outside the range"), "{template}: {below}: {e}");
            }
            // Some of the values above the range (255 + 1 of a `u8`) fail in the type of the
            // field, not in the range, so only that they fail is asserted.
            if let Some(above) = max.checked_add(1) {
                assert!(parse(above).is_err(), "{template}: {above}");
            }
        }
        let flip = |bit: u8| {
            serde_json::from_str::<Fault>(&format!(r#"{{"kind":"flip","octet":0,"bit":{bit}}}"#))
        };
        assert!(flip(7).is_ok());
        assert!(
            flip(8)
                .unwrap_err()
                .to_string()
                .contains("outside the range")
        );
        // The version of a trace starts at 1.
        let mut doc: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        doc["cases"][0]["trace_version"] = 0.into();
        let e = Scenario::parse(&doc.to_string()).unwrap_err();
        assert!(e.contains("outside the range"), "{e}");
        // A function code that exists is not an unknown one.
        let unknown = |code: u8| {
            serde_json::from_str::<Call>(&format!(r#"{{"function":"unknown","code":{code}}}"#))
        };
        assert!(unknown(0).is_ok() && unknown(5).is_ok() && unknown(255).is_ok());
        for code in 1..=4 {
            let e = unknown(code).unwrap_err().to_string();
            assert!(e.contains("not unknown"), "{code}: {e}");
        }
    }

    #[test]
    fn a_range_error_names_its_entry_and_carries_the_range() {
        // req: APP-107, APP-126
        let error_of = |from: &str, to: &str| {
            assert!(super::SCENARIO_JSON.contains(from), "{from}");
            Scenario::parse(&super::SCENARIO_JSON.replacen(from, to, 1)).unwrap_err()
        };
        let e = error_of(
            r#""table_id": 1, "length": 300"#,
            r#""table_id": 1, "length": 504"#,
        );
        assert!(
            e.contains("ops[1]: 504 is outside the range 1 to 503"),
            "{e}"
        );
        let e = error_of(
            r#""function": "unknown", "code": 127"#,
            r#""function": "unknown", "code": 3"#,
        );
        assert!(
            e.contains("ops[1]: 3 is the code of a function, not unknown"),
            "{e}"
        );
        let e = error_of(
            r#""fault": {"kind": "drop"}"#,
            r#""fault": {"kind": "flip", "octet": 0, "bit": 8}"#,
        );
        assert!(
            e.contains("faults[0]: 8 is outside the range 0 to 7"),
            "{e}"
        );
        // Outside the scripts the error is serde's.
        let e = error_of(r#""trace_version": 1"#, r#""trace_version": 0"#);
        assert!(e.contains("0 is outside the range 1 to 4294967295"), "{e}");
        // A document on one line has no lines to point at; the entry is named all the same.
        let mut doc: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        let case = doc["cases"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|c| c["ops"].as_array().is_some_and(|ops| ops.len() > 1))
            .unwrap();
        let id = case["id"].as_str().unwrap().to_owned();
        case["ops"][1]["length"] = 504.into();
        let one_line = doc.to_string();
        assert!(!one_line.contains('\n'));
        let e = Scenario::parse(&one_line).unwrap_err();
        assert!(
            e.contains(&format!(
                "case `{id}`: ops[1]: 504 is outside the range 1 to 503"
            )),
            "{e}"
        );
    }

    #[test]
    fn every_range_of_the_scenario_is_in_a_type() {
        // req: APP-107
        // A `schemars(range ...)` attribute states a range for the schema only; the parser
        // would need a second, hand-written check of it (RS-020). `Ranged` holds both.
        let source = include_str!("scenario.rs");
        let attribute = ["schemars(", "range"].concat();
        let (code, _) = source.split_once("#[cfg(test)]").unwrap();
        assert!(!code.contains(&attribute), "use a `Ranged` for the field");
    }

    #[test]
    fn table_length_limit_is_503() {
        // req: APP-017
        // req: APP-126
        let table = |length| OpsEntry::TableUpload {
            at_us: Micros::new(5_115_000),
            table_id: Ranged::new(1).unwrap(),
            length,
        };
        rejects_with(5, table(Ranged::new(503).unwrap()))
            .check()
            .unwrap();
        let parse = |length: u16| {
            serde_json::from_str::<OpsEntry>(&format!(
                r#"{{"kind":"table_upload","at_us":0,"table_id":1,"length":{length}}}"#
            ))
        };
        assert!(parse(503).is_ok());
        assert!(parse(504).is_err());
    }

    #[test]
    fn script_entries_reject_unknown_fields_and_kinds() {
        // req: APP-126
        let entry = |text: &str| serde_json::from_str::<super::OpsEntry>(text);
        assert!(entry(r#"{"kind":"table_upload","at_us":1,"table_id":1,"length":3}"#).is_ok());
        assert!(
            entry(r#"{"kind":"fop1_directive","at_us":0,"directive":"initiate_ad_without_clcw"}"#)
                .is_ok()
        );
        assert!(
            entry(r#"{"kind":"table_upload","at_us":1,"table_id":1,"length":3,"hex":"00"}"#)
                .is_err()
        );
        assert!(entry(r#"{"kind":"octets","at_us":1,"hex":"00"}"#).is_err());
        let fault = |text: &str| serde_json::from_str::<super::FaultEntry>(text);
        assert!(
            fault(r#"{"dir":"up","index":0,"fault":{"kind":"flip","octet":3,"bit":7}}"#).is_ok()
        );
        assert!(fault(r#"{"dir":"up","index":0,"fault":{"kind":"drop","octet":3}}"#).is_err());
    }
    /// A frame-level case with the ops script `ops`, a copy of `a1-rejects` otherwise.
    fn frames_with(ops: Vec<OpsEntry>) -> Scenario {
        let mut s = Scenario::load().unwrap();
        s.cases[3].level = Level::Frames;
        s.cases[3].ops = ops;
        s.cases[3].faults.clear();
        s
    }

    #[test]
    fn frame_level_cases_hold_no_command_that_acts_at_delivery() {
        // req: APP-141
        let tagged = |exec_us: u64| Execution::TimeTagged {
            exec_us: Micros::new(exec_us),
        };
        // Time-tagged commands that queue: accepted, whatever the release instant.
        let ok = frames_with(vec![
            command(1_000_000, tagged(7_000_000), Call::Noop {}),
            command(1_000_000, tagged(7_000_000), Call::Noop {}),
            OpsEntry::TableUpload {
                at_us: Micros::new(2_000_000),
                table_id: Ranged::new(1).unwrap(),
                length: Ranged::new(300).unwrap(),
            },
            command(3_000_001, tagged(9_000_000), Call::Noop {}),
        ]);
        ok.check().unwrap();
        // An immediate command, on the grid or not.
        for at_us in [115_000, 100_000] {
            let s = frames_with(vec![command(at_us, Execution::Immediate {}, Call::Noop {})]);
            assert!(rejects(&s, "APP-141"), "{at_us}");
        }
        // A time tag before the arrival (reason 2): released at 4.0 s, sent at the slot
        // 4.0 s, delivered at 4.01 s.
        let s = frames_with(vec![command(4_000_000, tagged(4_000_000), Call::Noop {})]);
        assert!(rejects(&s, "APP-141"));
        // The same time tag one slot later is queued.
        frames_with(vec![command(4_000_000, tagged(4_125_000), Call::Noop {})])
            .check()
            .unwrap();
        // The table upload delays the command released behind it: three slots from 2.0 s
        // leave the command at the slot 2.75 s, delivered at 2.76 s.
        let upload = |at_us: u64| OpsEntry::TableUpload {
            at_us: Micros::new(at_us),
            table_id: Ranged::new(1).unwrap(),
            length: Ranged::new(300).unwrap(),
        };
        let s = frames_with(vec![
            upload(2_000_000),
            command(2_000_000, tagged(2_750_000), Call::Noop {}),
        ]);
        assert!(rejects(&s, "APP-141"));
        frames_with(vec![
            upload(2_000_000),
            command(2_000_000, tagged(2_875_000), Call::Noop {}),
        ])
        .check()
        .unwrap();
        // A full queue (reason 4): nine commands that wait for the same execution time.
        let nine = (0..9)
            .map(|k| command(250_000 * (k + 1), tagged(9_000_000), Call::Noop {}))
            .collect();
        assert!(rejects(&frames_with(nine), "APP-141"));
    }

    #[test]
    fn packet_level_rules_are_unchanged_by_the_frame_level_rule() {
        // req: APP-128
        // An immediate command that arrives on the grid is accepted at packet level.
        let mut s = Scenario::load().unwrap();
        s.cases[3].ops[0] = command(115_000, Execution::Immediate {}, Call::Noop {});
        s.check().unwrap();
        // The same release instant is refused at frame level (the frame arrives off the grid).
        s.cases[3].level = Level::Frames;
        assert!(rejects(&s, "APP-141"));
    }

    #[test]
    fn the_initial_ocf_is_a_clcw() {
        // req: APP-136
        use super::ClcwWord;
        assert_eq!(ClcwWord::new(0x0100_0000).unwrap().get(), 0x0100_0000);
        // Control Word Type 1 is not a CLCW (232.0 §4.2.1.2).
        assert!(ClcwWord::new(0x8100_0000).is_err());
        // A version other than 00.
        assert!(ClcwWord::new(0x2100_0000).is_err());
        let bad = super::SCENARIO_JSON.replace("16777216", "2164260864");
        assert!(Scenario::parse(&bad).is_err());
    }

    #[test]
    fn the_data_field_fits_the_frame() {
        // req: APP-107
        let ok = Scenario::load().unwrap();
        ok.check().unwrap();
        // With the FECF the data field is at most 1017 octets (232.0 §4.1.3.1.2); the frame
        // length is raised so that only this limit applies.
        let mut s = ok.clone();
        s.tc.max_frame_length = Ranged::new(1024).unwrap();
        s.tc.max_fdu_length = Ranged::new(1017).unwrap();
        s.check().unwrap();
        s.tc.max_fdu_length = Ranged::new(1018).unwrap();
        assert!(rejects(&s, "longest data field"));
        // Without the FECF 1019 is allowed.
        s.tc.fecf = false;
        s.tc.max_fdu_length = Ranged::new(1019).unwrap();
        s.check().unwrap();
        // The header, the data field and the FECF must fit the maximum frame length.
        let mut s = ok;
        s.tc.max_fdu_length = Ranged::new(122).unwrap();
        assert!(rejects(&s, "tc.max_frame_length"));
        s.tc.fecf = false;
        s.check().unwrap();
    }

    /// The scenario file with the first case at `level` and with `variant`, when given.
    fn with_variant(level: &str, variant: Option<&str>) -> Result<Scenario, String> {
        let mut doc: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        let case = &mut doc["cases"][0];
        case["level"] = level.into();
        if let Some(variant) = variant {
            case["variant"] = variant.into();
        }
        Scenario::parse(&doc.to_string())
    }

    #[test]
    fn the_variant_applies_at_its_levels_only() {
        // req: APP-187
        // `aos` at `frames` and `cop1`, `uslp` at `cop1`, `tm_tc` at every level.
        for (level, variant) in [
            ("cop1", "aos"),
            ("frames", "aos"),
            ("cop1", "uslp"),
            ("packets", "tm_tc"),
            ("frames", "tm_tc"),
            ("cop1", "tm_tc"),
        ] {
            let s = with_variant(level, Some(variant));
            assert!(s.is_ok(), "{level}/{variant}: {:?}", s.err());
        }
        // `aos` not at `packets`, `uslp` neither at `packets` nor at `frames`.
        for (level, variant) in [("packets", "aos"), ("packets", "uslp"), ("frames", "uslp")] {
            let e = with_variant(level, Some(variant)).unwrap_err();
            assert!(
                e.contains(&format!(
                    "the variant `{variant}` does not apply at the level"
                )) && e.contains("APP-187"),
                "{level}/{variant}: {e}"
            );
        }
        // The rule is the type's: it holds for every pair.
        for level in [Level::Packets, Level::Frames, Level::Cop1] {
            assert!(super::Variant::TmTc.applies_at(level));
        }
        assert_eq!(super::Variant::Uslp.name(), "uslp");
    }

    #[test]
    fn an_absent_variant_is_tm_tc_and_an_unknown_one_is_rejected() {
        // req: APP-187
        // None of the cases of A0 to A3 names a variant: all are `tm_tc`.
        assert!(!super::SCENARIO_JSON.contains("\"variant\""));
        let s = Scenario::load().unwrap();
        assert!(s.cases.iter().all(|c| c.variant == super::Variant::TmTc));
        assert_eq!(
            with_variant("cop1", None).unwrap().cases[0].variant,
            super::Variant::TmTc
        );
        // Any other value is an error, a different spelling included.
        for bad in ["tm", "AOS", "Uslp", "tmtc", "tm-tc", ""] {
            let e = with_variant("cop1", Some(bad)).unwrap_err();
            assert!(e.contains("unknown variant"), "{bad:?}: {e}");
        }
        // The schema says the same: three values, `tm_tc` when absent.
        let schema: serde_json::Value = serde_json::from_str(&super::schema().unwrap()).unwrap();
        assert!(schema["$defs"]["Case"]["properties"]["variant"].is_object());
        let values: Vec<_> = schema["$defs"]["Variant"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["const"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(values, ["tm_tc", "aos", "uslp"]);
        assert!(
            !schema["$defs"]["Case"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f == "variant")
        );
    }

    /// The scenario with a copy of `a3-nominal` as the case `x` at `variant`, uploading a table
    /// of `length` octets.
    fn with_upload(variant: super::Variant, length: u16) -> Scenario {
        let mut s = Scenario::load().unwrap();
        let mut case = s.case("a3-nominal").unwrap().clone();
        "x".clone_into(&mut case.id);
        case.variant = variant;
        let upload = case
            .ops
            .iter_mut()
            .find(|o| matches!(o, OpsEntry::TableUpload { .. }))
            .unwrap();
        if let OpsEntry::TableUpload { length: l, .. } = upload {
            *l = Ranged::new(length).unwrap();
        }
        s.cases.push(case);
        s
    }

    #[test]
    fn the_table_limit_depends_on_the_variant() {
        // req: APP-182, APP-017
        // At `tm_tc` and `aos` a table of 503 octets, a packet of 512, is the longest (APP-017);
        // at `uslp` the packet must fit the 501 octets of one frame, so the table has 492 at most.
        for variant in [super::Variant::TmTc, super::Variant::Aos] {
            with_upload(variant, 503).check().unwrap();
        }
        with_upload(super::Variant::Uslp, 492).check().unwrap();
        let e = with_upload(super::Variant::Uslp, 493).check().unwrap_err();
        assert!(
            e.contains("case `x`: ops[2]: a table of 493 octets makes a packet of 502 octets")
                && e.contains("501")
                && e.contains("at most 492 octets")
                && e.contains("APP-182"),
            "{e}"
        );
        // The type of the entry still ends at 503, whatever the variant (APP-017).
        assert!(
            serde_json::from_str::<OpsEntry>(
                r#"{"kind":"table_upload","at_us":0,"table_id":1,"length":504}"#
            )
            .is_err()
        );
        // The limit follows the Maximum Packet Length of MAP 0.
        let mut s = with_upload(super::Variant::Uslp, 400);
        s.check().unwrap();
        s.uslp_up.max_packet_length = Ranged::new(408).unwrap();
        assert!(rejects(&s, "at most 399 octets"));
    }

    #[test]
    fn the_uplink_model_of_uslp_sends_a_table_in_one_frame() {
        // req: APP-182, APP-141
        // At `tm_tc` the table of 300 octets takes three uplink slots (APP-042); at `uslp` the
        // packet of 309 octets is one TFDZ in one frame of 320 octets (APP-182, APP-184), so the
        // command released at the same instant as the upload is sent in the slot behind it.
        let tagged = |exec_us: u64| Execution::TimeTagged {
            exec_us: Micros::new(exec_us),
        };
        let upload = OpsEntry::TableUpload {
            at_us: Micros::new(2_000_000),
            table_id: Ranged::new(1).unwrap(),
            length: Ranged::new(300).unwrap(),
        };
        let command = |exec_us: u64| command(2_000_000, tagged(exec_us), Call::Noop {});
        let with = |variant: super::Variant, exec_us: u64| {
            let mut s = frames_with(vec![upload.clone(), command(exec_us)]);
            s.cases[3].level = Level::Cop1;
            s.cases[3].variant = variant;
            s
        };
        // The command arrives at 2.75 s + 10 ms at `tm_tc` and at 2.25 s + 10 ms at `uslp`.
        assert!(rejects(&with(super::Variant::TmTc, 2_750_000), "APP-141"));
        with(super::Variant::TmTc, 2_875_000).check().unwrap();
        assert!(rejects(&with(super::Variant::Uslp, 2_250_000), "APP-141"));
        with(super::Variant::Uslp, 2_375_000).check().unwrap();
    }

    #[test]
    fn the_frame_lengths_of_the_variants_leave_room_for_their_fields() {
        // req: APP-189, APP-182
        let ok = Scenario::load().unwrap();
        // AOS: header 6, FHEC 2, `M_PDU` Header 2, `VC_OCF` 4, FECF 2, and one octet of Packet Zone.
        let mut s = ok.clone();
        s.aos.frame_length = Ranged::new(17).unwrap();
        s.check().unwrap();
        s.aos.frame_length = Ranged::new(16).unwrap();
        assert!(rejects(&s, "aos.frame_length 16"));
        s.aos.vc_ocf = false;
        s.check().unwrap();
        // USLP down: header 7, VCF Count 1, TFDF Header 3, OCF 4, FECF 2, and one octet of TFDZ.
        let mut s = ok.clone();
        s.uslp_down.frame_length = Ranged::new(18).unwrap();
        s.check().unwrap();
        s.uslp_down.frame_length = Ranged::new(17).unwrap();
        assert!(rejects(&s, "uslp_down.frame_length 17"));
        s.uslp_down.ocf_required = false;
        s.check().unwrap();
        // USLP up: 512 − 7 − 1 − 1 − 2 = 501 octets of TFDZ (APP-182).
        let s = ok.clone();
        assert_eq!(s.uslp_up.tfdz_capacity(), 501);
        let mut s = ok;
        s.uslp_up.max_packet_length = Ranged::new(502).unwrap();
        assert!(rejects(
            &s,
            "uslp_up.max_packet_length 502 is above the 501 octets"
        ));
        s.uslp_up.max_frame_length = Ranged::new(513).unwrap();
        s.check().unwrap();
    }

    #[test]
    fn the_sections_of_the_variants_hold_the_values_of_the_tables() {
        // req: APP-189
        // The values of the tables of §4.3.1 to §4.3.3 that are numbers or flags, as the file
        // and the generated constants have them.
        use refapp_scenario::generated::{aos, uslp_down, uslp_up};
        let s = Scenario::load().unwrap();
        assert_eq!(
            (aos::FRAME_LENGTH, aos::FHEC, aos::FECF, aos::VC_OCF),
            (223, true, true, true)
        );
        assert_eq!(
            (aos::VCID_REALTIME, aos::VCID_PAYLOAD, aos::VCID_IDLE),
            (0, 1, 7)
        );
        assert_eq!(
            (
                aos::RELEASE_DEADLINE_REALTIME_US,
                aos::RELEASE_DEADLINE_PAYLOAD_US
            ),
            (250_000, 1_000_000)
        );
        assert_eq!((aos::MAX_PACKET_LENGTH, aos::PACKET_VERSION), (512, 0));
        assert_eq!(
            (
                aos::PRIORITY_REALTIME,
                aos::PRIORITY_PAYLOAD,
                aos::PRIORITY_IDLE
            ),
            (1, 2, 3)
        );
        assert_eq!(
            (
                uslp_down::FRAME_LENGTH,
                uslp_down::FECF,
                uslp_down::OCF_REQUIRED
            ),
            (223, true, true)
        );
        assert_eq!(
            (
                uslp_down::VCF_COUNT_LENGTH_SEQUENCE,
                uslp_down::VCF_COUNT_LENGTH_EXPEDITED
            ),
            (0, 1)
        );
        assert_eq!((uslp_down::VCID_REALTIME, uslp_down::VCID_PAYLOAD), (0, 1));
        assert_eq!(
            (
                uslp_down::MAX_PACKET_LENGTH,
                uslp_down::PACKET_VERSION,
                uslp_down::ENCAPSULATION_PACKET_VERSION
            ),
            (512, 0, 7)
        );
        assert_eq!(
            (
                uslp_up::MAX_FRAME_LENGTH,
                uslp_up::MAX_PACKET_LENGTH,
                uslp_up::FECF
            ),
            (512, 501, true)
        );
        assert_eq!(
            (
                uslp_up::VCF_COUNT_LENGTH_SEQUENCE,
                uslp_up::VCF_COUNT_LENGTH_EXPEDITED,
                uslp_up::OCF_ALLOWED
            ),
            (1, 0, false)
        );
        assert_eq!(
            (uslp_up::MAP_BLOCKING, uslp_up::MAP_SEGMENTATION),
            (true, false)
        );
        // The generated constants are the file's values.
        assert_eq!(
            u64::from(s.aos.frame_length.get()),
            u64::from(aos::FRAME_LENGTH)
        );
        assert_eq!(
            u64::from(s.uslp_up.max_packet_length.get()),
            u64::from(uslp_up::MAX_PACKET_LENGTH)
        );
        // 501 octets of packet, 492 of table.
        assert_eq!(
            u64::from(uslp_up::MAX_PACKET_LENGTH) - super::TABLE_PACKET_OVERHEAD,
            492
        );
    }
}
