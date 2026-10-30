//! C ABI of the USLP Transfer Frame codec `aldis-uslp` (CCSDS 732.1-B-3; M2 plan D4, D12, D14,
//! D17; CABI-010…012, CABI-020…022).
//!
//! Calls: [`aldis_uslp_config_check`] and [`aldis_uslp_vc_table_check`];
//! [`aldis_uslp_peek_length`]; the decoder [`aldis_uslp_decode`]; the unified Frame Intake
//! Stream [`aldis_uslp_stream_next`]; the one-shot [`aldis_uslp_encode`], which takes the frame's
//! variable fields in an [`aldis_uslp_content_t`]; [`aldis_uslp_encode_truncated`];
//! and the OID frame encoder [`aldis_uslp_encode_oid`], which takes an `aldis_oid_generator_t`.
//!
//! **Channels.** A physical channel is an [`aldis_uslp_config_t`] (732.1 §5 Table 5-1). The
//! virtual channels and MAPs it carries are passed beside it as two arrays, of
//! [`aldis_uslp_vc_config_t`] (Tables 5-3, 6-1) and [`aldis_uslp_map_config_t`] (Table 5-4),
//! each with its count: the C form of `VcTable::new`, which takes the physical channel and
//! the two lists, built and checked on every call into a `VcTable` of the build-time
//! capacities `ALDIS_USLP_MAX_VCS` and `ALDIS_USLP_MAX_MAPS` (CABI-030). As in `aldis-uslp`,
//! each virtual channel and MAP names its own SCID, so one table holds every master channel of
//! the physical channel. Each array is a C array of the struct
//! (`abi::configs`; c-abi.md §11). `aldis_uslp_peek_length` takes the
//! virtual channels only (M2 plan D14), since only a VC's truncated frame length delimits a frame.
//!
//! **Security layouts** (API-070, API-075; c-abi.md §11). Every call that takes the channels
//! uses their layouts: a USLP frame cannot be decoded without its virtual channel's
//! configuration (the VCF Count Length, the OCF use, the TFDZ type, the truncated frame
//! length), so there is no decoder that takes the physical channel alone, and no `_with` pair.
//!
//! **Frames.** `UslpFrame` is a full or a truncated frame (M2 plan D12); it crosses as one
//! [`aldis_uslp_frame_t`] whose `truncated` member says which, with the members of a full
//! frame's Primary Header 0 for a truncated one, except `kind`, which is
//! `ALDIS_USLP_KIND_EXPEDITED` (USLP-002; OI-52). Every field lies at an offset and a length of
//! its [`aldis_uslp_layout_t`] (CABI-012).
//!
//! **Encoders.** A frame is built in one call, with no state between calls (API-023, API-060):
//! [`aldis_uslp_encode`], [`aldis_uslp_encode_truncated`], and [`aldis_uslp_encode_oid`] write
//! the headers, the Frame Length, the OCF, and the FECF. The C ABI has no two-phase writer
//! (OI-57).

use aldis::core::layout::{FrameLayout, OCF_LEN};
use aldis::core::ocf::Ocf;
use aldis::core::security::SecurityLayout;
use aldis::core::{AnomalySet, DecodeMode, Error, ErrorKind, Lenient, Result};
use aldis::uslp::field;
use aldis::uslp::{
    ChannelFields, ConstructionRule, FrameContent, FrameFields, FrameKind, FrameStream, IntakeItem,
    LenientFrameStream, MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope,
    SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen, Upid,
    UslpAnomaly, UslpFrame, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable,
    VcfCountLen, encode, encode_oid, encode_oid_galois, encode_truncated,
};

use crate::abi::{self, Configs, invalid_argument};
use crate::capacities::{ALDIS_USLP_MAX_MAPS, ALDIS_USLP_MAX_VCS};
use crate::mirror::{mirror, parse};
use crate::oid::{GeneratorState, aldis_oid_generator_t, generator};
use crate::range::aldis_range_t;

/// Length of the non-truncated Primary Header without its VCF Count, in octets (732.1
/// §4.1.2.1.2).
pub const ALDIS_USLP_PRIMARY_HEADER_LEN: u32 = 7;
/// Length of the truncated Primary Header, in octets (732.1 Annex D1.2.2).
pub const ALDIS_USLP_TRUNCATED_HEADER_LEN: u32 = 4;
/// Longest USLP Transfer Frame, in octets: the 16-bit Frame Length field plus one (732.1
/// §4.1.2.7; USLP-003).
pub const ALDIS_USLP_MAX_FRAME_LEN: u32 = 65_536;
/// The VCID of OID frames (732.1 §4.1.4.1.6; USLP-009).
pub const ALDIS_USLP_OID_VCID: u8 = 63;
/// First Header or Last Valid Octet Pointer `0xFFFF`: no packet starts or ends in the TFDZ
/// (rule `000`), or the SDU continues (rules `001`, `010`) (732.1 §4.1.4.2.4; USLP-006).
pub const ALDIS_USLP_POINTER_NONE: u16 = 0xFFFF;
/// Shortest truncated frame, in octets (732.1 Annex D1.3.2; USLP-002). A `truncated_frame_len`
/// of 0 means the VC carries no truncated frames.
pub const ALDIS_USLP_TRUNCATED_MIN_LEN: u8 = 6;
/// Longest truncated frame, in octets (732.1 Annex D1.3.2; USLP-002).
pub const ALDIS_USLP_TRUNCATED_MAX_LEN: u8 = 32;
/// Shortest truncated frame on a VC or MAP with SDLS, in octets (732.1 Annex D1.2.4;
/// USLP-012, API-075).
pub const ALDIS_USLP_TRUNCATED_MIN_LEN_WITH_SDLS: u8 = 28;

/// The USLP Transfer Frame Type (732.1 Table 5-1).
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_frame {
    /// Transfer Frame Type: fixed-length frames (732.1 §5 Tables 5-1, 5-3).
    ALDIS_USLP_FRAME_FIXED = 0,
    /// Transfer Frame Type: variable-length frames (732.1 §5 Tables 5-1, 5-3).
    ALDIS_USLP_FRAME_VARIABLE = 1,
}
mirror!(pub aldis_uslp_frame: u8 { ALDIS_USLP_FRAME_FIXED, ALDIS_USLP_FRAME_VARIABLE });

/// Whether a virtual channel carries an OCF (732.1 Table 5-3).
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_ocf {
    /// OCF use of a virtual channel: no frame carries an OCF (732.1 §4.1.5, Table 5-3).
    ALDIS_USLP_OCF_ABSENT = 0,
    /// OCF use: a frame may carry an OCF, on a variable-length VC (732.1 §4.1.5.4).
    ALDIS_USLP_OCF_ALLOWED = 1,
    /// OCF use: every frame carries an OCF, on a fixed-length VC (732.1 §4.1.5.3).
    ALDIS_USLP_OCF_REQUIRED = 2,
}
mirror!(pub aldis_uslp_ocf: u8 { ALDIS_USLP_OCF_ABSENT, ALDIS_USLP_OCF_ALLOWED, ALDIS_USLP_OCF_REQUIRED });

/// The scope of a security layout (732.1 Table 5-3).
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_security {
    /// Security of a virtual channel: one layout for the VC, its lengths in the VC struct (732.1
    /// §6.6 Table 6-1; API-070).
    ALDIS_USLP_SECURITY_VC = 0,
    /// Security of a virtual channel: a layout per MAP, in the MAP structs (732.1 §6.6; API-070).
    ALDIS_USLP_SECURITY_PER_MAP = 1,
}
mirror!(pub aldis_uslp_security: u8 { ALDIS_USLP_SECURITY_VC, ALDIS_USLP_SECURITY_PER_MAP });

/// The Source-or-Destination Identifier (732.1 §4.1.2.2).
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_direction {
    /// Source-or-Destination Identifier `0`: the SCID is the frame's source (732.1 §4.1.2.3).
    ALDIS_USLP_SOURCE = 0,
    /// Source-or-Destination Identifier `1`: the SCID is the frame's destination (732.1 §4.1.2.3).
    ALDIS_USLP_DESTINATION = 1,
}
mirror!(pub aldis_uslp_direction: u8 { ALDIS_USLP_SOURCE, ALDIS_USLP_DESTINATION });

/// The kind of a USLP frame (732.1 Table 5-3).
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_kind {
    /// Frame kind: Bypass/Sequence Control and Protocol Control Command Flags `00`, user data of
    /// the Sequence-Controlled service (732.1 Table 4-1; USLP-001).
    ALDIS_USLP_KIND_SEQUENCE_CONTROLLED = 0,
    /// Frame kind: flags `10`, user data of the Expedited service (732.1 Table 4-1; USLP-001).
    ALDIS_USLP_KIND_EXPEDITED = 1,
    /// Frame kind: flags `11`, Protocol Control Commands (732.1 Table 4-1; USLP-001, USLP-013).
    ALDIS_USLP_KIND_PROTOCOL_CONTROL = 2,
}
mirror!(pub aldis_uslp_kind: u8 { ALDIS_USLP_KIND_SEQUENCE_CONTROLLED, ALDIS_USLP_KIND_EXPEDITED, ALDIS_USLP_KIND_PROTOCOL_CONTROL });

/// The TFDF Construction Rule (732.1 Table 4-2).
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_rule {
    /// TFDZ Construction Rule `000`: packets spanning frames, fixed-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES = 0b000,
    /// Rule `001`: start of a `MAPA_SDU` or `VCA_SDU`, fixed-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_SDU_START = 0b001,
    /// Rule `010`: continuing portion of a `MAPA_SDU` or `VCA_SDU`, fixed-length TFDZ (732.1
    /// Table 4-3).
    ALDIS_USLP_RULE_SDU_CONTINUATION = 0b010,
    /// Rule `011`: octet stream, variable-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_OCTET_STREAM = 0b011,
    /// Rule `100`: starting segment, variable-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_STARTING_SEGMENT = 0b100,
    /// Rule `101`: continuing segment, variable-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_CONTINUING_SEGMENT = 0b101,
    /// Rule `110`: last segment, variable-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_LAST_SEGMENT = 0b110,
    /// Rule `111`: no segmentation, variable-length TFDZ (732.1 Table 4-3).
    ALDIS_USLP_RULE_NO_SEGMENTATION = 0b111,
}
mirror!(pub aldis_uslp_rule: u8 { ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES, ALDIS_USLP_RULE_SDU_START, ALDIS_USLP_RULE_SDU_CONTINUATION, ALDIS_USLP_RULE_OCTET_STREAM, ALDIS_USLP_RULE_STARTING_SEGMENT, ALDIS_USLP_RULE_CONTINUING_SEGMENT, ALDIS_USLP_RULE_LAST_SEGMENT, ALDIS_USLP_RULE_NO_SEGMENTATION });

/// Largest UPID: the field has 5 bits; every value from 0 up to it crosses the ABI, assigned
/// by the SANA registry or not (732.1 §4.1.4.2.3; USLP-007).
pub const ALDIS_USLP_UPID_MAX: u8 = 0x1F;
/// UPID "Space Packets or Encapsulation packets", from the SANA registry snapshot
/// `specs/sana/uslp_protocol_id.tsv` (732.1 §4.1.4.2.3; USLP-007, OI-06).
pub const ALDIS_USLP_UPID_PACKETS: u8 = 0b00000;
/// UPID "COP-1 Control Commands", from the SANA snapshot (USLP-007, USLP-013).
pub const ALDIS_USLP_UPID_COP1_CONTROL_COMMANDS: u8 = 0b00001;
/// UPID "COP-P Control Commands", from the SANA snapshot (USLP-007).
pub const ALDIS_USLP_UPID_COPP_CONTROL_COMMANDS: u8 = 0b00010;
/// UPID "User-defined Octet Stream", from the SANA snapshot (USLP-007).
pub const ALDIS_USLP_UPID_OCTET_STREAM: u8 = 0b00100;
/// UPID "Mission-specific Information-1", from the SANA snapshot (USLP-007; Annex D1.4.1.2).
pub const ALDIS_USLP_UPID_MISSION_SPECIFIC_INFORMATION_1: u8 = 0b00101;
/// UPID "Proximity-1 Pseudo Packet Identifier 1 for Segmentation", from the SANA snapshot
/// (USLP-007).
pub const ALDIS_USLP_UPID_PROXIMITY1_PSEUDO_PACKET_ID_1: u8 = 0b00110;
/// UPID "Proximity-1 SPDUs", from the SANA snapshot (USLP-007).
pub const ALDIS_USLP_UPID_PROXIMITY1_SPDUS: u8 = 0b00111;
/// UPID "Proximity-1 Pseudo Packet Identifier 2 for Segmentation", from the SANA snapshot
/// (USLP-007).
pub const ALDIS_USLP_UPID_PROXIMITY1_PSEUDO_PACKET_ID_2: u8 = 0b01000;
/// UPID "Only Idle Data", from the SANA snapshot (USLP-007, USLP-009).
pub const ALDIS_USLP_UPID_ONLY_IDLE_DATA: u8 = 0b11111;

/// Anomaly bit: Reserve Spares not `00` (bit 0 of `UslpAnomaly`; 732.1 §4.1.2.9.2; API-032).
pub const ALDIS_USLP_ANOMALY_RESERVE_SPARES_SET: u32 = 1 << 0;
/// Anomaly bit: a frame on VCID 63 whose OID header fields deviate (bit 1; 732.1
/// §4.1.4.1.8–9; API-032, OI-44 item 1).
pub const ALDIS_USLP_ANOMALY_OID_HEADER_MISMATCH: u32 = 1 << 1;

/// The result of one step of a USLP frame stream.
///
/// A `uint8_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_stream {
    /// Stream item: no octets of the unit remain (732.1 §4.3.11.2; USLP-015).
    ALDIS_USLP_STREAM_END = 0,
    /// Stream item: one validated frame (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
    ALDIS_USLP_STREAM_ACCEPTED = 1,
    /// Stream item: discarded octets (fill, incomplete frame, or failed validation) (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
    ALDIS_USLP_STREAM_DISCARDED = 2,
}
mirror!(pub aldis_uslp_stream: u8 { ALDIS_USLP_STREAM_END, ALDIS_USLP_STREAM_ACCEPTED, ALDIS_USLP_STREAM_DISCARDED });

/// Why a USLP frame was discarded.
///
/// A `uint32_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_uslp_discard {
    /// Discard reason: accepted frame (not discarded).
    ALDIS_USLP_DISCARD_NONE = 0,
    /// Discard reason: test (b), Spacecraft ID names no Master Channel of the Physical Channel (732.1 §4.3.11.3; USLP-014).
    ALDIS_USLP_DISCARD_UNEXPECTED_SPACECRAFT = 2,
    /// Discard reason: test (c), Bypass Flag `0` with Protocol Control Command Flag `1` (732.1 Table 4-1; USLP-001).
    ALDIS_USLP_DISCARD_RESERVED_FLAGS = 3,
    /// Discard reason: test (c), strict mode only: Reserve Spares are not `00` (732.1 §4.1.2.9.2).
    ALDIS_USLP_DISCARD_RESERVED_SPARE = 4,
    /// Discard reason: test (c), VCID is not valid on the Master Channel (732.1 §4.3.11.3; USLP-014).
    ALDIS_USLP_DISCARD_INVALID_VCID = 5,
    /// Discard reason: test (c), MAP ID is not valid on the VC (732.1 §4.3.11.3; USLP-014).
    ALDIS_USLP_DISCARD_INVALID_MAP_ID = 6,
    /// Discard reason: test (c), a truncated frame on a VC without a truncated frame length (732.1 Annex D1.2.1; USLP-002).
    ALDIS_USLP_DISCARD_TRUNCATED_FRAME_NOT_ALLOWED = 7,
    /// Discard reason: test (c), VCF Count Length is not the VC's for the frame's quality of service (732.1 §4.1.2.11.4; USLP-004).
    ALDIS_USLP_DISCARD_VCF_COUNT_LENGTH_MISMATCH = 8,
    /// Discard reason: test (c), OCF Flag disagrees with the VC's OCF use (732.1 §4.1.5.3–4).
    ALDIS_USLP_DISCARD_OCF_FLAG_MISMATCH = 9,
    /// Discard reason: test (c), on a fixed-length Physical Channel, Frame Length is not the configured length (732.1 §4.1.2.7; USLP-003).
    ALDIS_USLP_DISCARD_FIXED_LENGTH_MISMATCH = 10,
    /// Discard reason: test (c), on a variable-length Physical Channel, Frame Length exceeds the maximum (732.1 Table 5-1; USLP-003).
    ALDIS_USLP_DISCARD_FRAME_TOO_LONG = 11,
    /// Discard reason: test (c), Frame Length leaves no TFDZ octet after the fields present (CMN-041; OI-44 item 2).
    ALDIS_USLP_DISCARD_FRAME_TOO_SHORT = 12,
    /// Discard reason: test (d), octets present disagree with Frame Length or truncated frame length (732.1 §4.3.11.3.4 d).
    ALDIS_USLP_DISCARD_LENGTH_MISMATCH = 13,
    /// Discard reason: test (e), FECF does not match recomputed CRC (732.1 §4.1.6; Annex B).
    ALDIS_USLP_DISCARD_FECF_MISMATCH = 14,
    /// Discard reason: TFDZ Construction Rule is of the other TFDZ type than the VC's (732.1 USLP-016).
    ALDIS_USLP_DISCARD_CONSTRUCTION_RULE_MISMATCH = 15,
    /// Discard reason: First Header or Last Valid Octet Pointer lies beyond the TFDZ (732.1 USLP-006).
    ALDIS_USLP_DISCARD_POINTER_BEYOND_ZONE = 16,
    /// Discard reason: OID frame on VCID 63 deviates from required header or length (732.1 Annex D1.2.4; USLP-007).
    ALDIS_USLP_DISCARD_OID_HEADER_MISMATCH = 17,
    /// Discard reason: a non-truncated frame on a VC that has a truncated frame length (732.1 Annex D1.2.1; USLP-002; OI-48).
    ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED = 18,
    /// Discard reason: fill data (732.1 §4.3.11.2.4 a–b; USLP-015).
    ALDIS_USLP_DISCARD_FILL = 20,
    /// Discard reason: incomplete frame whose declared length exceeds remaining octets (732.1 §4.3.11.2.3; USLP-015).
    ALDIS_USLP_DISCARD_INCOMPLETE_FRAME = 21,
    /// Discard reason: truncated header whose SCID and VCID name no configured VC or no truncated length (732.1 USLP-015).
    ALDIS_USLP_DISCARD_NO_TRUNCATED_LENGTH = 22,
    /// Discard reason: leftover fill, more octets than the longest fill the coding sublayer appends
    /// that start no valid header, perhaps after a Frame Length error (732.1 §4.3.11.2.4 note 2; USLP-015, OI-47).
    ALDIS_USLP_DISCARD_LEFTOVER_FILL = 23,
}
mirror!(pub aldis_uslp_discard: u32 { ALDIS_USLP_DISCARD_NONE, ALDIS_USLP_DISCARD_UNEXPECTED_SPACECRAFT, ALDIS_USLP_DISCARD_RESERVED_FLAGS, ALDIS_USLP_DISCARD_RESERVED_SPARE, ALDIS_USLP_DISCARD_INVALID_VCID, ALDIS_USLP_DISCARD_INVALID_MAP_ID, ALDIS_USLP_DISCARD_TRUNCATED_FRAME_NOT_ALLOWED, ALDIS_USLP_DISCARD_VCF_COUNT_LENGTH_MISMATCH, ALDIS_USLP_DISCARD_OCF_FLAG_MISMATCH, ALDIS_USLP_DISCARD_FIXED_LENGTH_MISMATCH, ALDIS_USLP_DISCARD_FRAME_TOO_LONG, ALDIS_USLP_DISCARD_FRAME_TOO_SHORT, ALDIS_USLP_DISCARD_LENGTH_MISMATCH, ALDIS_USLP_DISCARD_FECF_MISMATCH, ALDIS_USLP_DISCARD_CONSTRUCTION_RULE_MISMATCH, ALDIS_USLP_DISCARD_POINTER_BEYOND_ZONE, ALDIS_USLP_DISCARD_OID_HEADER_MISMATCH, ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED, ALDIS_USLP_DISCARD_FILL, ALDIS_USLP_DISCARD_INCOMPLETE_FRAME, ALDIS_USLP_DISCARD_NO_TRUNCATED_LENGTH, ALDIS_USLP_DISCARD_LEFTOVER_FILL });
/// Discard reason: test (a), TFVN is not `1100` (732.1 §4.1.2.2.2; USLP-001).
pub const ALDIS_USLP_DISCARD_UNEXPECTED_VERSION: u32 = 1;
/// Discard reason: non-truncated header on a VC that carries only truncated frames (732.1 OI-48; USLP-015).
pub const ALDIS_USLP_DISCARD_NON_TRUNCATED_NOT_ALLOWED: u32 =
    ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED;

/// Managed parameters of a physical channel (732.1 §5 Table 5-1; USLP-003, USLP-008, API-040).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_config_t {
    /// Transfer Frame Type, `ALDIS_USLP_FRAME_*` (Table 5-1).
    pub frame_type: u8,
    /// 1 if every frame ends with a Frame Error Control Field, else 0 (732.1 §4.1.6).
    pub fecf: u8,
    /// The frame length of a fixed-length channel, or the maximum of a variable-length one, in
    /// octets: 9 to `ALDIS_USLP_MAX_FRAME_LEN` (732.1 §4.1.2.7, Table 5-1).
    pub frame_len: usize,
    /// Insert Zone length in octets, on a fixed-length channel only; 0 for none (732.1 §4.1.3;
    /// USLP-008).
    pub insert_zone_len: usize,
}

/// Managed parameters of a virtual channel (732.1 §5 Table 5-3, §6.6 Table 6-1, Annex D1.2;
/// USLP-002, USLP-004, USLP-012, API-070).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_vc_config_t {
    /// Spacecraft Identifier of the VC's master channel (732.1 §4.1.2.2.3).
    pub scid: u16,
    /// Virtual Channel Identifier, 0 to 63; 63 for OID frames (732.1 §4.1.2.4).
    pub vcid: u8,
    /// Transfer Frame Type of the VC, `ALDIS_USLP_FRAME_*` (Table 5-3).
    pub frame_type: u8,
    /// VCF Count Length of Sequence-Controlled frames, 0 to 7 octets (732.1 §4.1.2.11;
    /// USLP-004).
    pub vcf_count_len_sequence: u8,
    /// VCF Count Length of Expedited and protocol control frames, 0 to 7 octets (USLP-004).
    pub vcf_count_len_expedited: u8,
    /// OCF use, `ALDIS_USLP_OCF_*` (732.1 §4.1.5).
    pub ocf: u8,
    /// Truncated Transfer Frame Length, `ALDIS_USLP_TRUNCATED_MIN_LEN` to
    /// `ALDIS_USLP_TRUNCATED_MAX_LEN`, on a variable-length VC; 0 for none (Annex D1.2.1;
    /// USLP-002).
    pub truncated_frame_len: u8,
    /// `ALDIS_USLP_SECURITY_VC` for the layout below, or `ALDIS_USLP_SECURITY_PER_MAP` for the
    /// layouts of the MAP structs, which ignores the two lengths below (Table 6-1; API-070).
    pub security: u8,
    /// Security Header length in octets, at most 64; 0 without SDLS (Table 6-1; API-070).
    pub security_header_len: u8,
    /// Security Trailer length in octets, at most 65 535; 0 without SDLS (Table 6-1; API-070).
    pub security_trailer_len: u32,
}

/// Managed parameters of a MAP channel (732.1 §5 Table 5-4, §6.6; API-070).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_map_config_t {
    /// Spacecraft Identifier of the MAP's virtual channel (732.1 §4.1.2.2.3).
    pub scid: u16,
    /// Virtual Channel Identifier of the MAP's virtual channel, which must be configured
    /// (732.1 §4.1.2.4).
    pub vcid: u8,
    /// MAP Identifier, 0 to 15 (732.1 §4.1.2.5).
    pub map_id: u8,
    /// Security Header length on a VC with `ALDIS_USLP_SECURITY_PER_MAP`, at most 64; otherwise
    /// 0 (Table 6-1; API-070).
    pub security_header_len: u8,
    /// Security Trailer length on a VC with `ALDIS_USLP_SECURITY_PER_MAP`, at most 65 535;
    /// otherwise 0 (Table 6-1; API-070).
    pub security_trailer_len: u32,
}

/// The fields of a frame's first four octets, which truncated and non-truncated Primary Headers
/// share (732.1 §4.1.2.2–5, Annex D1.3.1).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_channel_t {
    /// Spacecraft Identifier (732.1 §4.1.2.2.3).
    pub scid: u16,
    /// Source-or-Destination Identifier, `ALDIS_USLP_SOURCE` or `ALDIS_USLP_DESTINATION`
    /// (732.1 §4.1.2.3).
    pub source_or_destination: u8,
    /// Virtual Channel Identifier, 0 to 63 (732.1 §4.1.2.4).
    pub vcid: u8,
    /// MAP Identifier, 0 to 15 (732.1 §4.1.2.5).
    pub map_id: u8,
}

/// The Primary Header fields a sender chooses (732.1 §4.1.2; USLP-010, API-021). The TFVN, the
/// Frame Length, the Reserve Spares, the OCF Flag, and the VCF Count Length are derived.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_fields_t {
    /// The SCID, the Source-or-Destination Identifier, the VCID, and the MAP ID.
    pub channel: aldis_uslp_channel_t,
    /// Frame kind, `ALDIS_USLP_KIND_*` (732.1 Table 4-1).
    pub kind: u8,
    /// VCF Count, written in the VC's VCF Count Length for the frame's service (732.1
    /// §4.1.2.12; USLP-004).
    pub vcf_count: u64,
}

/// A TFDF Header (732.1 §4.1.4.2; USLP-005…USLP-007).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_tfdf_t {
    /// TFDZ Construction Rule, `ALDIS_USLP_RULE_*` (Table 4-3).
    pub rule: u8,
    /// UPID, 0 to `ALDIS_USLP_UPID_MAX`; an assigned value has an `ALDIS_USLP_UPID_*` constant
    /// (732.1 §4.1.4.2.3; USLP-007).
    pub upid: u8,
    /// For rules `000` to `010`, the First Header or Last Valid Octet Pointer, raw:
    /// `ALDIS_USLP_POINTER_NONE` or an offset in the TFDZ (§4.1.4.2.4; USLP-006). The other
    /// rules have no pointer: 0 in a view and ignored by the writers (USLP-005).
    pub pointer: u16,
}

/// The variable fields of a frame, as `(pointer, length)` buffers, and its TFDF Header (732.1
/// §4.1.3–5; USLP-010, CABI-011; c-abi.md §11).
///
/// Each buffer may be `NULL` with length 0. The struct is read once per call and no pointer is
/// kept (API-060). A call that does not take a buffer requires it empty, and says so.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_uslp_content_t {
    /// The TFDF Header: rule, UPID, pointer (§4.1.4.2).
    pub tfdf: aldis_uslp_tfdf_t,
    /// The Insert Zone, exactly the physical channel's `insert_zone_len` octets (§4.1.3).
    pub insert_zone: *const u8,
    /// Length of `insert_zone`.
    pub insert_zone_len: usize,
    /// The Transfer Frame Data Zone (§4.1.4.3).
    pub tfdz: *const u8,
    /// Length of `tfdz`.
    pub tfdz_len: usize,
    /// The Operational Control Field: `ALDIS_OCF_LEN` octets when the frame carries one, else
    /// empty (§4.1.5).
    pub ocf: *const u8,
    /// Length of `ocf`: `ALDIS_OCF_LEN` or 0.
    pub ocf_len: usize,
}

impl Default for aldis_uslp_content_t {
    /// The TFDF Header with rule `000` and UPID 0, and every buffer empty and `NULL`.
    fn default() -> aldis_uslp_content_t {
        aldis_uslp_content_t {
            tfdf: aldis_uslp_tfdf_t::default(),
            insert_zone: core::ptr::null(),
            insert_zone_len: 0,
            tfdz: core::ptr::null(),
            tfdz_len: 0,
            ocf: core::ptr::null(),
            ocf_len: 0,
        }
    }
}

impl aldis_uslp_content_t {
    /// The three buffers, which an output buffer must not overlap (CABI-022).
    const fn buffers(&self) -> [(*const u8, usize); 3] {
        [
            (self.insert_zone, self.insert_zone_len),
            (self.tfdz, self.tfdz_len),
            (self.ocf, self.ocf_len),
        ]
    }
}

/// Where each field of a frame lies (732.1 §4.1, §6.3, Annex D; API-072, CABI-012).
///
/// Offsets and lengths are in octets from the start of the frame. An absent field has length 0
/// at the offset where it would start; the Primary Header is the octets before the Insert Zone.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_layout_t {
    /// Transfer Frame length: the octets the frame takes (732.1 §4.1.2.7, Annex D1.3.2).
    pub frame_len: usize,
    /// The range of the Insert Zone, after the Primary Header and its VCF Count; length 0 without one (§4.1.3).
    pub insert_zone: aldis_range_t,
    /// The range of the Security Header; length 0 without SDLS (§6.3.4).
    pub security_header: aldis_range_t,
    /// The range of the TFDF Header; its length is 3 for rules `000` to `010`, else 1 (§4.1.4.2.1.2; USLP-005).
    pub tfdf_header: aldis_range_t,
    /// The range of the Transfer Frame Data Zone; length never 0 (§4.1.4.3).
    pub tfdz: aldis_range_t,
    /// The range of the Security Trailer; length 0 without one (§6.3.6).
    pub security_trailer: aldis_range_t,
    /// The range of the Operational Control Field; its length is `ALDIS_OCF_LEN` or 0 (§4.1.5).
    pub ocf: aldis_range_t,
    /// The range of the Frame Error Control Field; its length is 2 or 0 (§4.1.6).
    pub fecf: aldis_range_t,
}

/// A decoded USLP Transfer Frame, full or truncated (732.1 §4.1, §6.3, Annex D; USLP-001…USLP-007,
/// API-010, CABI-012; M2 plan D12).
///
/// Its fields, and where they lie in the input. The OCF is classified with
/// `aldis_ocf_classify` on its octets (CMN-010).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_frame_t {
    /// Whether the End of Frame Primary Header Flag is 1: a truncated frame, whose length is
    /// its VC's truncated frame length and which has no Frame Length, flags, VCF Count, Insert
    /// Zone, OCF, or FECF (732.1 §4.1.2.6, Annex D; USLP-002).
    pub truncated: bool,
    /// The fields of the first four octets (732.1 §4.1.2.2–5, Annex D1.3.1).
    pub channel: aldis_uslp_channel_t,
    /// Frame kind, `ALDIS_USLP_KIND_*` (Table 4-1); `ALDIS_USLP_KIND_EXPEDITED` for a truncated
    /// frame, which is processed as expedited user data (Annex D1.3.1 note 3; USLP-002; OI-52).
    pub kind: u8,
    /// OCF Flag (§4.1.2.10); `false` for a truncated frame.
    pub ocf_flag: bool,
    /// VCF Count Length in octets (§4.1.2.11); 0 for a truncated frame.
    pub vcf_count_len: u8,
    /// VCF Count (§4.1.2.12); 0 for a truncated frame.
    pub vcf_count: u64,
    /// The TFDF Header (§4.1.4.2; USLP-005…USLP-007).
    pub tfdf: aldis_uslp_tfdf_t,
    /// Whether the SANA registry assigns the UPID (USLP-007).
    pub upid_assigned: bool,
    /// Whether the frame has an FECF (§4.1.6).
    pub has_fecf: bool,
    /// The FECF, when `has_fecf`.
    pub fecf: u16,
    /// Lenient-mode anomaly flags, `ALDIS_USLP_ANOMALY_*`; 0 in strict mode (API-032).
    pub anomalies: u32,
    /// Where each field lies in the input.
    pub layout: aldis_uslp_layout_t,
}

/// One item yielded by [`aldis_uslp_stream_next`] (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_uslp_stream_item_t {
    /// Item kind: [`ALDIS_USLP_STREAM_END`], [`ALDIS_USLP_STREAM_ACCEPTED`], or [`ALDIS_USLP_STREAM_DISCARDED`].
    pub kind: u8,
    /// Reserved padding octets, set to zero.
    pub reserved: [u8; 3],
    /// The range of this item in the caller's unit buffer: where it starts and the octets it
    /// consumes.
    pub range: aldis_range_t,
    /// The frame length declared in the header or VC config (valid when accepted or incomplete).
    pub declared_len: usize,
    /// The decoded frame view (valid when `kind == ALDIS_USLP_STREAM_ACCEPTED`).
    pub frame: aldis_uslp_frame_t,
    /// Discard reason code: `ALDIS_USLP_DISCARD_*` (valid when `kind == ALDIS_USLP_STREAM_DISCARDED`).
    pub discard_reason: u32,
    /// The error code of the discard (e.g. `ALDIS_ERR_CRC_MISMATCH`).
    pub error_code: u32,
    /// The field code of the error (0 if none).
    pub error_field: u16,
    /// Reserved padding octets, set to zero.
    pub reserved2: [u8; 2],
}

impl Default for aldis_uslp_stream_item_t {
    fn default() -> Self {
        Self {
            kind: ALDIS_USLP_STREAM_END,
            reserved: [0; 3],
            range: aldis_range_t { offset: 0, len: 0 },
            declared_len: 0,
            frame: aldis_uslp_frame_t::default(),
            discard_reason: 0,
            error_code: 0,
            error_field: 0,
            reserved2: [0; 2],
        }
    }
}

/// The channel tables of the build-time capacities (CABI-030).
type Table = VcTable<ALDIS_USLP_MAX_VCS, ALDIS_USLP_MAX_MAPS>;

/// A Transfer Frame Type of a C struct (Tables 5-1, 5-3; CABI-022).
fn frame_type(value: u8) -> Result<TransferFrameType> {
    match parse::<aldis_uslp_frame>(value)? {
        aldis_uslp_frame::ALDIS_USLP_FRAME_FIXED => Ok(TransferFrameType::Fixed),
        aldis_uslp_frame::ALDIS_USLP_FRAME_VARIABLE => Ok(TransferFrameType::Variable),
    }
}

/// The physical channel of a C struct, its parameters parsed into their types (Table 5-1;
/// RS-020).
fn physical(c: &aldis_uslp_config_t) -> Result<PhysicalChannelConfig> {
    PhysicalChannelConfig::new(
        frame_type(c.frame_type)?,
        c.frame_len,
        c.insert_zone_len,
        abi::flag(c.fecf)?,
    )
}

/// A security length of a C struct; a value beyond `usize` fails `SecurityLayout::new` (API-070).
fn length(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The virtual channel of a C struct, each parameter parsed into its type in the order of the
/// Rust constructors (Table 5-3, Table 6-1; RS-020).
fn virtual_channel(v: &aldis_uslp_vc_config_t) -> Result<VcConfig> {
    let scid = UslpSpacecraftId::try_from(v.scid)?;
    let vcid = UslpVcId::try_from(v.vcid)?;
    let frame_type = frame_type(v.frame_type)?;
    let vcf_count_len_sequence = VcfCountLen::new(v.vcf_count_len_sequence)?;
    let vcf_count_len_expedited = VcfCountLen::new(v.vcf_count_len_expedited)?;
    let ocf = match parse::<aldis_uslp_ocf>(v.ocf)? {
        aldis_uslp_ocf::ALDIS_USLP_OCF_ABSENT => OcfUse::Absent,
        aldis_uslp_ocf::ALDIS_USLP_OCF_ALLOWED => OcfUse::Allowed,
        aldis_uslp_ocf::ALDIS_USLP_OCF_REQUIRED => OcfUse::Required,
    };
    let truncated_frame_len = match v.truncated_frame_len {
        0 => None,
        n => Some(TruncatedFrameLen::new(usize::from(n))?),
    };
    let security = match parse::<aldis_uslp_security>(v.security)? {
        aldis_uslp_security::ALDIS_USLP_SECURITY_VC => SecurityScope::Vc(SecurityLayout::new(
            usize::from(v.security_header_len),
            length(v.security_trailer_len),
        )?),
        aldis_uslp_security::ALDIS_USLP_SECURITY_PER_MAP => SecurityScope::PerMap,
    };
    let params = VcParams {
        frame_type,
        vcf_count_len_sequence,
        vcf_count_len_expedited,
        ocf,
        truncated_frame_len,
        security,
    };
    VcConfig::new(scid, vcid, params)
}

/// The MAP of a C struct (Table 5-4; RS-020).
fn map_channel(m: &aldis_uslp_map_config_t) -> Result<MapConfig> {
    let scid = UslpSpacecraftId::try_from(m.scid)?;
    let vcid = UslpVcId::try_from(m.vcid)?;
    let map_id = UslpMapId::try_from(m.map_id)?;
    let security = SecurityLayout::new(
        usize::from(m.security_header_len),
        length(m.security_trailer_len),
    )?;
    Ok(MapConfig::new(scid, vcid, map_id, security))
}

/// `CapacityExceeded` naming `field`, for more structs than the build-time capacity (CABI-030).
const fn capacity(field: aldis::core::FieldId) -> Error {
    Error::new(ErrorKind::CapacityExceeded).with_field(field)
}

/// The channel table of C structs: the physical channel, then each virtual channel and each MAP
/// in array order, then `VcTable::new` (Tables 5-1 to 5-4; API-040, GEN-060).
fn table(
    pc: &aldis_uslp_config_t,
    vcs: &Configs<'_, aldis_uslp_vc_config_t>,
    maps: &Configs<'_, aldis_uslp_map_config_t>,
) -> Result<Table> {
    let physical = physical(pc)?;
    if vcs.len() > ALDIS_USLP_MAX_VCS {
        return Err(capacity(field::VC_ID));
    }
    if maps.len() > ALDIS_USLP_MAX_MAPS {
        return Err(capacity(field::MAP_ID));
    }
    let (scid, vcid, map_id) = (
        UslpSpacecraftId::try_from(0)?,
        UslpVcId::try_from(0)?,
        UslpMapId::try_from(0)?,
    );
    let mut v =
        [VcConfig::new(scid, vcid, VcParams::new(TransferFrameType::Fixed))?; ALDIS_USLP_MAX_VCS];
    for (i, slot) in v.iter_mut().enumerate().take(vcs.len()) {
        *slot = virtual_channel(&vcs.get(i)?)?;
    }
    let mut m = [MapConfig::new(scid, vcid, map_id, SecurityLayout::NONE); ALDIS_USLP_MAX_MAPS];
    for (i, slot) in m.iter_mut().enumerate().take(maps.len()) {
        *slot = map_channel(&maps.get(i)?)?;
    }
    Table::new(
        physical,
        v.get(..vcs.len()).unwrap_or_default(),
        m.get(..maps.len()).unwrap_or_default(),
    )
}

/// The channel table of the physical channel at `pc`, the `vc_count` virtual channels at
/// `vcs`, and the `map_count` MAPs at `maps` (CABI-022).
///
/// # Safety
/// As for `abi::read` and `abi::configs`.
#[expect(unsafe_code, reason = "checked access to caller memory (CABI-022)")]
unsafe fn table_at(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
) -> Result<Table> {
    // SAFETY: forwarded from the caller.
    let (pc, vcs, maps) = unsafe {
        (
            abi::read(pc)?,
            abi::configs(vcs, vc_count)?,
            abi::configs(maps, map_count)?,
        )
    };
    table(&pc, &vcs, &maps)
}

/// The fields of the first four octets of a C struct (732.1 §4.1.2.2–5; RS-020).
fn channel(c: aldis_uslp_channel_t) -> Result<ChannelFields> {
    let spacecraft_id = UslpSpacecraftId::try_from(c.scid)?;
    let source_or_destination = match parse::<aldis_uslp_direction>(c.source_or_destination)? {
        aldis_uslp_direction::ALDIS_USLP_SOURCE => SourceOrDestination::Source,
        aldis_uslp_direction::ALDIS_USLP_DESTINATION => SourceOrDestination::Destination,
    };
    Ok(ChannelFields {
        spacecraft_id,
        source_or_destination,
        vc_id: UslpVcId::try_from(c.vcid)?,
        map_id: UslpMapId::try_from(c.map_id)?,
    })
}

/// The sender's fields of a C struct (732.1 §4.1.2; RS-020).
fn fields(f: aldis_uslp_fields_t) -> Result<FrameFields> {
    let ch = channel(f.channel)?;
    let kind = match parse::<aldis_uslp_kind>(f.kind)? {
        aldis_uslp_kind::ALDIS_USLP_KIND_SEQUENCE_CONTROLLED => FrameKind::SequenceControlled,
        aldis_uslp_kind::ALDIS_USLP_KIND_EXPEDITED => FrameKind::Expedited,
        aldis_uslp_kind::ALDIS_USLP_KIND_PROTOCOL_CONTROL => FrameKind::ProtocolControl,
    };
    Ok(FrameFields {
        spacecraft_id: ch.spacecraft_id,
        source_or_destination: ch.source_or_destination,
        vc_id: ch.vc_id,
        map_id: ch.map_id,
        kind,
        vcf_count: f.vcf_count,
    })
}

/// The TFDF Header of a C struct: the rule a constant, the UPID a 5-bit value (Table 4-3;
/// USLP-005, USLP-007).
///
/// # Errors
/// `InvalidField` (no field) for a rule above `111`; `OutOfRange` (`upid`) above 31.
fn tfdf(t: aldis_uslp_tfdf_t) -> Result<TfdfHeader> {
    if t.rule > ALDIS_USLP_RULE_NO_SEGMENTATION {
        return Err(invalid_argument());
    }
    let upid = Upid::from_bits(t.upid).ok_or(Error::out_of_range(
        field::UPID,
        u64::from(ALDIS_USLP_UPID_MAX),
        u64::from(t.upid),
    ))?;
    Ok(TfdfHeader {
        rule: TfdzRule::new(ConstructionRule::from_bits(t.rule), t.pointer),
        upid,
    })
}

/// An optional OCF buffer: 0 octets for none, `ALDIS_OCF_LEN` for one (CABI-011, CABI-022).
fn ocf(octets: &[u8]) -> Result<Option<Ocf>> {
    if octets.is_empty() {
        return Ok(None);
    }
    <[u8; OCF_LEN]>::try_from(octets)
        .map(|o| Some(Ocf::decode(o)))
        .map_err(|_| invalid_argument())
}

/// `InvalidField` (no field) unless a buffer the call does not take is empty (CABI-022).
const fn empty(len: usize) -> Result<()> {
    if len == 0 {
        Ok(())
    } else {
        Err(invalid_argument())
    }
}

/// The TFDF Header, the Insert Zone, and the OCF of a content struct (732.1 §4.1.3–5).
///
/// # Safety
/// Each buffer of `c` is valid for its length (or `NULL` with 0) during `'a`.
#[expect(unsafe_code, reason = "checked access to caller memory (CABI-022)")]
unsafe fn frame_content<'a>(c: &aldis_uslp_content_t) -> Result<FrameContent<'a>> {
    // SAFETY: forwarded from the caller.
    let (insert_zone, octets) = unsafe {
        (
            abi::bytes(c.insert_zone, c.insert_zone_len)?,
            abi::bytes(c.ocf, c.ocf_len)?,
        )
    };
    Ok(FrameContent {
        tfdf: tfdf(c.tfdf)?,
        insert_zone,
        ocf: ocf(octets)?,
    })
}

/// The C form of a TFDF Header (§4.1.4.2).
fn tfdf_view(h: TfdfHeader) -> aldis_uslp_tfdf_t {
    aldis_uslp_tfdf_t {
        rule: h.rule.rule().bits(),
        upid: h.upid.bits(),
        pointer: h.rule.pointer().unwrap_or(0),
    }
}

/// The C form of the first four octets' fields (§4.1.2.2–5).
fn channel_view(c: ChannelFields) -> aldis_uslp_channel_t {
    aldis_uslp_channel_t {
        scid: c.spacecraft_id.get(),
        source_or_destination: u8::from(c.source_or_destination.bit()),
        vcid: c.vc_id.get(),
        map_id: c.map_id.get(),
    }
}

/// The C constant of a frame kind (Table 4-1).
const fn kind_view(k: FrameKind) -> u8 {
    match k {
        FrameKind::SequenceControlled => ALDIS_USLP_KIND_SEQUENCE_CONTROLLED,
        FrameKind::Expedited => ALDIS_USLP_KIND_EXPEDITED,
        FrameKind::ProtocolControl => ALDIS_USLP_KIND_PROTOCOL_CONTROL,
    }
}

/// The C form of a frame layout whose data field holds a TFDF Header of `tfdf_len` octets
/// before the TFDZ (732.1 §4.1, Annex D1.2; CMN-040, CABI-012).
///
/// # Errors
/// `InternalError` if the TFDF Header is longer than the data field, which the callers' frames
/// rule out.
fn layout(l: &FrameLayout, tfdf_len: usize) -> Result<aldis_uslp_layout_t> {
    let insert_zone = aldis_range_t::from(&l.secondary());
    let security_header = aldis_range_t::from(&l.security_header());
    let data = l.data();
    let security_trailer = aldis_range_t::from(&l.security_trailer());
    let ocf = aldis_range_t::from(&l.ocf());
    let fecf = aldis_range_t::from(&l.fecf());
    let tfdz = data
        .len()
        .checked_sub(tfdf_len)
        .ok_or(Error::new(ErrorKind::InternalError).with_field(field::FRAME_LEN))?;
    Ok(aldis_uslp_layout_t {
        frame_len: l.frame_len(),
        insert_zone,
        security_header,
        tfdf_header: aldis_range_t {
            offset: data.start,
            len: tfdf_len,
        },
        tfdz: aldis_range_t {
            offset: data.start.saturating_add(tfdf_len),
            len: tfdz,
        },
        security_trailer,
        ocf,
        fecf,
    })
}

/// The C view of a decoded frame (CABI-012).
fn view(f: &UslpFrame<'_>, anomalies: AnomalySet<UslpAnomaly>) -> Result<aldis_uslp_frame_t> {
    let tfdf = f.tfdf_header();
    let mut v = aldis_uslp_frame_t {
        truncated: f.is_truncated(),
        channel: channel_view(f.channel()),
        tfdf: tfdf_view(tfdf),
        upid_assigned: !matches!(tfdf.upid, Upid::Other(_)),
        anomalies: anomalies.bits(),
        layout: layout(f.layout(), tfdf.len())?,
        ..aldis_uslp_frame_t::default()
    };
    if let Some(full) = f.full() {
        let h = full.header();
        v.kind = kind_view(h.kind());
        v.ocf_flag = h.ocf_flag();
        v.vcf_count_len = h.vcf_count().len().get();
        v.vcf_count = h.vcf_count().value();
        v.has_fecf = full.fecf().is_some();
        v.fecf = full.fecf().unwrap_or(0);
    } else {
        v.kind = kind_view(FrameKind::Expedited);
    }
    Ok(v)
}

/// Checks a physical channel configuration (732.1 §5 Table 5-1; USLP-003, USLP-008, API-040).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`frame_len` for a fixed-length channel, `max_frame_len` for a
/// variable-length one) for a length below 9 or above `ALDIS_USLP_MAX_FRAME_LEN`, and
/// (`insert_zone_len`) for an Insert Zone on a variable-length channel; CABI-022 violations,
/// including a `frame_type` or `fecf` that is no constant.
///
/// # Safety
/// `pc` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_config_check(
    pc: *const aldis_uslp_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || physical(&abi::read(pc)?).map(drop)) }
}

/// Checks a channel table: a physical channel with its virtual channels and MAPs (732.1 §5
/// Tables 5-1 to 5-4, §6.6, Annex D1.2; USLP-002, USLP-008, USLP-012, API-040, API-070,
/// API-075, GEN-060).
///
/// The physical channel is checked first, then the counts against `ALDIS_USLP_MAX_VCS` and
/// `ALDIS_USLP_MAX_MAPS`, each virtual channel in array order, each MAP in array order, and the
/// table as `VcTable::new` checks it.
///
/// # Errors
/// The errors of [`aldis_uslp_config_check`]; `ALDIS_ERR_CAPACITY_EXCEEDED` (`vc_id`, `map_id`)
/// above the capacities; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `map_id`); `ALDIS_ERR_INVALID_CONFIG`
/// (`vcf_count_len`, `truncated_frame_len`, `security_header_len`, `security_trailer_len`,
/// `ocf_flag`, `transfer_frame_type`, `vc_id`, `map_id`, `frame_data_field`) as `aldis-uslp`'s
/// constructors return them (`docs/standards/uslp-frame.md` §2.1); CABI-022 violations,
/// including an enumeration member that is no constant. `vc_count` 0 is `ALDIS_ERR_INVALID_CONFIG` (`vc_id`) once
/// the structs have parsed (USLP-017, OI-55).
///
/// # Safety
/// `pc` is valid for a read; `vcs` is valid for reads of `vc_count` structs, and `maps` of `map_count` (each `NULL` with 0); `err` is `NULL` or valid for
/// one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_vc_table_check(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            table_at(pc, vcs, vc_count, maps, map_count).map(drop)
        })
    }
}

/// The length of the frame that `prefix` begins (732.1 §4.1.2.7, Annex D1.3.2; API-013, M2 plan
/// D12, D14).
///
/// As `UslpFrame::peek_length` gives it: a truncated frame's VC's truncated frame length, a
/// fixed-length channel's frame length, or the Frame Length field plus one.
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (`needed` 4) for a shorter prefix; `ALDIS_ERR_UNSUPPORTED_VERSION`
/// (`frame_version`); for a truncated header `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id`,
/// `vc_id`) or `ALDIS_ERR_INVALID_FIELD` (`end_of_frame_primary_header_flag`) for a VC without
/// a truncated frame length; for a non-truncated header on a configured VC with a truncated frame
/// length, `ALDIS_ERR_INVALID_FIELD` (`end_of_frame_primary_header_flag`, OI-48); on a
/// variable-length channel `ALDIS_ERR_INCOMPLETE` (`needed` 6); the errors of
/// [`aldis_uslp_vc_table_check`] without MAPs; CABI-022 violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `pc` and `vcs` as for
/// [`aldis_uslp_vc_table_check`]; `out_len` is valid for one write; `err` is `NULL` or valid
/// for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_peek_length(
    prefix: *const u8,
    prefix_len: usize,
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, core::ptr::null(), 0)?;
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out_len = abi::out(out_len)?;
            out_len.write(UslpFrame::peek_length(prefix, &t)?);
            Ok(())
        })
    }
}

/// Decodes one frame, full or truncated, with the managed parameters of the channel table
/// (732.1 §4.1, §6.3, Annex D; USLP-001…USLP-007, USLP-016, API-010…API-012, API-030), as
/// `UslpFrame::decode` does.
///
/// A variable-length or truncated frame may be followed by other octets, which are ignored
/// (`out->layout.frame_len` is the frame's length; API-011); on a fixed-length channel `len`
/// must be the frame length (API-012).
///
/// # Errors
/// The decoder errors of `docs/standards/uslp-frame.md` §3.1, in its order: among them
/// `ALDIS_ERR_INCOMPLETE` (`needed`), `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`),
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id`, `vc_id`, `map_id`), `ALDIS_ERR_RESERVED_VALUE`
/// (`protocol_control_command_flag`, and `uslp_header_spare` in strict mode),
/// `ALDIS_ERR_LENGTH_MISMATCH` (`vcf_count_len`, `frame_len`), `ALDIS_ERR_CRC_MISMATCH`,
/// `ALDIS_ERR_INVALID_FIELD` (`end_of_frame_primary_header_flag`, `tfdz_construction_rule`,
/// `frame_data_field`, and the OID header fields in strict mode), and `ALDIS_ERR_MALFORMED`
/// (`fhp_lvo_pointer`); the errors of [`aldis_uslp_vc_table_check`]; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `pc`, `vcs`, and `maps` as for
/// [`aldis_uslp_vc_table_check`]; `out` is valid for one write; `err` is `NULL` or valid for
/// one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_decode(
    bytes: *const u8,
    len: usize,
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    mode: u32,
    out: *mut aldis_uslp_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let (frame, anomalies) = mode.dispatch(
                || UslpFrame::decode(bytes, &t),
                || UslpFrame::decode_lenient(bytes, &t),
            )?;
            out.write(view(&frame, anomalies)?);
            Ok(())
        })
    }
}

/// Steps through the clean data unit `unit` using the unified Frame Intake Stream
/// (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
///
/// Reads from `*offset` and advances `*offset` by the octets consumed for the yielded item.
/// When `out.kind == ALDIS_USLP_STREAM_ACCEPTED`, `out.frame` holds the validated frame view.
/// When `out.kind == ALDIS_USLP_STREAM_DISCARDED`, `out.discard_reason` holds the `ALDIS_USLP_DISCARD_*`
/// code, and `out.error_code` holds the error status.
/// When `out.kind == ALDIS_USLP_STREAM_END`, the unit is exhausted.
///
/// `max_fill` is the longest fill the coding sublayer appends (six octets for the BCH codeblock,
/// seven or 31 for the LDPC codeblocks, 732.1 §4.3.11.2.4 note 2): a rest that starts no valid
/// header is discarded as `ALDIS_USLP_DISCARD_FILL` up to `max_fill` octets and as
/// `ALDIS_USLP_DISCARD_LEFTOVER_FILL` above (OI-47).
///
/// # Errors
/// The errors of [`aldis_uslp_vc_table_check`]; `ALDIS_ERR_INVALID_ARGUMENT` if `*offset > unit_len`;
/// CABI-022 violations.
///
/// # Safety
/// `unit` is valid for `unit_len` octets (or `NULL` with 0); `offset` is valid for read and write;
/// `pc`, `vcs`, and `maps` as for [`aldis_uslp_vc_table_check`]; `out` is valid for one write; `err`
/// is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[expect(
    clippy::too_many_lines,
    reason = "C ABI stream intake dispatch with channel table and item population"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_stream_next(
    unit: *const u8,
    unit_len: usize,
    offset: *mut usize,
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    max_fill: usize,
    mode: u32,
    out: *mut aldis_uslp_stream_item_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and overlap.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let unit = abi::bytes(unit, unit_len)?;
            let offset_ref = abi::inout(offset, &[(unit.as_ptr(), unit.len())])?;
            let current_offset = *offset_ref;
            let mode = abi::mode(mode)?;
            let out = abi::out_disjoint(
                out,
                &[
                    (unit.as_ptr(), unit.len()),
                    (offset.cast::<u8>(), core::mem::size_of::<usize>()),
                ],
            )?;

            if current_offset > unit_len {
                return Err(invalid_argument());
            }
            if current_offset == unit_len {
                out.write(aldis_uslp_stream_item_t {
                    kind: ALDIS_USLP_STREAM_END,
                    reserved: [0; 3],
                    range: aldis_range_t {
                        offset: current_offset,
                        len: 0,
                    },
                    declared_len: 0,
                    frame: aldis_uslp_frame_t::default(),
                    discard_reason: 0,
                    error_code: 0,
                    error_field: 0,
                    reserved2: [0; 2],
                });
                return Ok(());
            }

            let Some(rest) = unit.get(current_offset..) else {
                return Err(invalid_argument());
            };
            let item = match mode {
                DecodeMode::Strict => FrameStream::new(rest, &t, max_fill)
                    .next()
                    .map(|item| item.map_frame(|frame| (frame, AnomalySet::empty()))),
                DecodeMode::Lenient => LenientFrameStream::new(rest, &t, max_fill)
                    .next()
                    .map(|item| item.map_frame(Lenient::into_parts)),
            };
            match item {
                None => {
                    out.write(aldis_uslp_stream_item_t {
                        kind: ALDIS_USLP_STREAM_END,
                        reserved: [0; 3],
                        range: aldis_range_t {
                            offset: current_offset,
                            len: 0,
                        },
                        declared_len: 0,
                        frame: aldis_uslp_frame_t::default(),
                        discard_reason: 0,
                        error_code: 0,
                        error_field: 0,
                        reserved2: [0; 2],
                    });
                }
                Some(IntakeItem::Accepted {
                    frame: (frame, anomalies),
                    ..
                }) => {
                    let len = frame.len();
                    *offset_ref = current_offset
                        .checked_add(len)
                        .ok_or_else(invalid_argument)?;
                    out.write(aldis_uslp_stream_item_t {
                        kind: ALDIS_USLP_STREAM_ACCEPTED,
                        reserved: [0; 3],
                        range: aldis_range_t {
                            offset: current_offset,
                            len,
                        },
                        declared_len: len,
                        frame: view(&frame, anomalies)?,
                        discard_reason: 0,
                        error_code: 0,
                        error_field: 0,
                        reserved2: [0; 2],
                    });
                }
                Some(IntakeItem::Discarded {
                    reason,
                    octets,
                    error,
                }) => {
                    let len = octets.len();
                    *offset_ref = current_offset
                        .checked_add(len)
                        .ok_or_else(invalid_argument)?;
                    let declared_len = match reason {
                        aldis::uslp::DiscardReason::IncompleteFrame { declared_len } => {
                            declared_len
                        }
                        _ => 0,
                    };
                    out.write(aldis_uslp_stream_item_t {
                        kind: ALDIS_USLP_STREAM_DISCARDED,
                        reserved: [0; 3],
                        range: aldis_range_t {
                            offset: current_offset,
                            len,
                        },
                        declared_len,
                        frame: aldis_uslp_frame_t::default(),
                        discard_reason: u32::from(reason.code()),
                        error_code: crate::error::status(error),
                        error_field: error.field().map_or(0, |f| f.code().get()),
                        reserved2: [0; 2],
                    });
                }
            }
            Ok(())
        })
    }
}

/// Encodes one non-truncated frame into the start of `out` in one call (732.1 §4.1, §6.3;
/// USLP-010, USLP-016, API-020…022), as `aldis_uslp::encode` does.
///
/// `content` gives the TFDF Header, the Insert Zone, the TFDZ, and the OCF. The library writes
/// the Frame Length, the OCF Flag, the VCF Count Length, and the FECF; with SDLS, the Security
/// Header and Trailer are zeros (API-072).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_uslp_vc_table_check`]; then
/// those of `docs/standards/uslp-frame.md` §3.3, in its order: `ALDIS_ERR_UNKNOWN_CHANNEL`
/// (`spacecraft_id`, `vc_id`, `map_id`); `ALDIS_ERR_INVALID_FIELD` (`vc_id`) for VCID 63, whose
/// frames [`aldis_uslp_encode_oid`] writes; `ALDIS_ERR_INVALID_FIELD`
/// (`end_of_frame_primary_header_flag`) for a VC with a truncated frame length (OI-48);
/// `ALDIS_ERR_INVALID_FIELD` (`tfdz_construction_rule`); `ALDIS_ERR_OUT_OF_RANGE` (`vcf_count`); `ALDIS_ERR_INVALID_FIELD`
/// (`ocf_flag`); `ALDIS_ERR_LENGTH_MISMATCH` (`insert_zone_len`); `ALDIS_ERR_INVALID_FIELD` and
/// `ALDIS_ERR_LENGTH_MISMATCH` (`frame_data_field`); `ALDIS_ERR_OUT_OF_RANGE` (`frame_len`,
/// `fhp_lvo_pointer`); `ALDIS_ERR_BUFFER_TOO_SMALL` (`needed` = frame length);
/// `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `map_id`, `upid`) for a value of `fields` or `content`
/// outside its field; CABI-022 violations, including an `ocf_len` other than 0 or
/// `ALDIS_OCF_LEN`, a member that is no constant, and `out` overlapping a buffer of `content`.
///
/// # Safety
/// `pc`, `vcs`, and `maps` as for [`aldis_uslp_vc_table_check`]; `fields` and `content` are
/// valid for a read; each buffer of `content` is valid for its length (or `NULL` with 0); `out`
/// is valid for `cap` octets; `out_len` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_encode(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    fields: *const aldis_uslp_fields_t,
    content: *const aldis_uslp_content_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let c = abi::read(content)?;
            let content = frame_content(&c)?;
            let tfdz = abi::bytes(c.tfdz, c.tfdz_len)?;
            let out = abi::bytes_mut(out, cap, &c.buffers())?;
            let out_len = abi::out(out_len)?;
            out_len.write(encode(&t, fields, content, tfdz, out)?);
            Ok(())
        })
    }
}

/// Encodes one truncated frame into the start of `out`; its length is the VC's truncated frame
/// length (732.1 Annex D; USLP-002, USLP-012, USLP-016, API-075), as
/// `aldis_uslp::encode_truncated` does.
///
/// `content` gives the TFDF Header and the TFDZ; its Insert Zone and OCF must be empty, since a
/// truncated frame has neither (Annex D1.2.2). With SDLS on the VC or MAP, the 6-octet Security
/// Header and 16-octet Security Trailer are written as zeros (Annex D1.2.3).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_uslp_vc_table_check`]; then
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id`, `vc_id`); `ALDIS_ERR_INVALID_FIELD` (`vc_id`) for
/// VCID 63; `ALDIS_ERR_UNKNOWN_CHANNEL` (`map_id`); `ALDIS_ERR_INVALID_FIELD`
/// (`end_of_frame_primary_header_flag`) for a VC without a truncated frame length,
/// (`tfdz_construction_rule`) for a rule of a fixed-length TFDZ, and (`frame_data_field`) for an
/// empty TFDZ; `ALDIS_ERR_LENGTH_MISMATCH` (`frame_data_field`) for a TFDZ that does not fill the
/// frame; `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations, including a non-empty Insert Zone or
/// OCF in `content`.
///
/// # Safety
/// As [`aldis_uslp_encode`], with `channel` in place of `fields`.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_encode_truncated(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    channel: *const aldis_uslp_channel_t,
    content: *const aldis_uslp_content_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let channel = self::channel(abi::read(channel)?)?;
            let c = abi::read(content)?;
            empty(c.insert_zone_len)?;
            empty(c.ocf_len)?;
            let header = tfdf(c.tfdf)?;
            let tfdz = abi::bytes(c.tfdz, c.tfdz_len)?;
            let out = abi::bytes_mut(out, cap, &c.buffers())?;
            let out_len = abi::out(out_len)?;
            out_len.write(encode_truncated(&t, channel, header, tfdz, out)?);
            Ok(())
        })
    }
}

/// Encodes an OID frame on VCID 63 into the start of `out` and fills its TFDZ with the next
/// octets of the OID generator `oid` (732.1 §4.1.4.1.5–12; USLP-009, CMN-020…022).
///
/// As `aldis_uslp::encode_oid` does with the Fibonacci form, and `encode_oid_galois` with the
/// Galois form, which gives the same octets.
///
/// `fields->channel.vcid` must be 63 and `map_id` 0. The library writes the TFDF Header: rule
/// `001`, UPID "Only Idle Data", and the Last Valid Octet Pointer at the last TFDZ octet, so
/// `content->tfdf` is not read; `content` gives the Insert Zone and the OCF, and its TFDZ must be
/// empty. The TFDZ fills the fixed-length frame. On a fixed-length channel, VCID 63 is valid
/// without an entry in `vcs` (732.1 Table 5-2 note 2): the frame then has no VCF Count, OCF, or
/// Security fields.
///
/// # Errors
/// The errors of [`aldis_uslp_encode`], with `ALDIS_ERR_INVALID_FIELD` (`vc_id`) for another
/// VCID, (`map_id`) for a MAP ID other than 0, and (`protocol_control_command_flag`) for
/// protocol control frames; `ALDIS_ERR_INVALID_HANDLE` for an invalid generator; CABI-022
/// violations, including a non-empty `content->tfdz` and `out` overlapping the generator's
/// storage. On error neither `out` nor the generator is changed (GEN-042).
///
/// # Safety
/// As [`aldis_uslp_encode`]; `oid` is a handle from `aldis_oid_generator_init` whose storage is
/// still valid, or detectably invalid storage.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_encode_oid(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    fields: *const aldis_uslp_fields_t,
    content: *const aldis_uslp_content_t,
    oid: *mut aldis_oid_generator_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let c = abi::read(content)?;
            empty(c.tfdz_len)?;
            let insert_zone = abi::bytes(c.insert_zone, c.insert_zone_len)?;
            let ocf = ocf(abi::bytes(c.ocf, c.ocf_len)?)?;
            let h = generator(oid)?;
            let [iz, tz, oc] = c.buffers();
            let out = abi::bytes_mut(out, cap, &[iz, tz, oc, h.range()])?;
            let out_len = abi::out(out_len)?;
            let mut state = h.load();
            let n = match &mut state {
                GeneratorState::Fibonacci(g) => encode_oid(&t, fields, insert_zone, ocf, g, out)?,
                GeneratorState::Galois(g) => {
                    encode_oid_galois(&t, fields, insert_zone, ocf, g, out)?
                }
            };
            h.store(state);
            out_len.write(n);
            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::Anomaly as _;
    use aldis::uslp::{Reason, UnassignedUpid, UslpAnomaly};

    #[test]
    fn constants_are_the_crate_values() {
        // req: GEN-043, API-033, CABI-010, USLP-002, USLP-005, USLP-007, USLP-009, USLP-014
        assert_eq!(
            ALDIS_USLP_ANOMALY_RESERVE_SPARES_SET,
            1 << UslpAnomaly::ReserveSparesSet.bit()
        );
        assert_eq!(
            ALDIS_USLP_ANOMALY_OID_HEADER_MISMATCH,
            1 << UslpAnomaly::OidHeaderMismatch.bit()
        );
        assert_eq!(
            usize::try_from(ALDIS_USLP_PRIMARY_HEADER_LEN),
            Ok(aldis::uslp::PRIMARY_HEADER_LEN)
        );
        assert_eq!(
            usize::try_from(ALDIS_USLP_TRUNCATED_HEADER_LEN),
            Ok(aldis::uslp::TRUNCATED_HEADER_LEN)
        );
        assert_eq!(
            usize::try_from(ALDIS_USLP_MAX_FRAME_LEN),
            Ok(aldis::uslp::MAX_FRAME_LEN)
        );
        assert_eq!(ALDIS_USLP_OID_VCID, aldis::uslp::OID_VCID);
        assert_eq!(ALDIS_USLP_POINTER_NONE, aldis::uslp::POINTER_NONE);
        let (min, max) = (
            usize::from(ALDIS_USLP_TRUNCATED_MIN_LEN),
            usize::from(ALDIS_USLP_TRUNCATED_MAX_LEN),
        );
        assert!(TruncatedFrameLen::new(min - 1).is_err());
        assert_eq!(
            TruncatedFrameLen::new(min).map(TruncatedFrameLen::get),
            Ok(min)
        );
        assert_eq!(
            TruncatedFrameLen::new(max).map(TruncatedFrameLen::get),
            Ok(max)
        );
        assert!(TruncatedFrameLen::new(max + 1).is_err());
        assert_eq!(
            usize::from(ALDIS_USLP_TRUNCATED_MIN_LEN_WITH_SDLS),
            TruncatedFrameLen::MIN_WITH_SDLS
        );
        let rules = [
            ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES,
            ALDIS_USLP_RULE_SDU_START,
            ALDIS_USLP_RULE_SDU_CONTINUATION,
            ALDIS_USLP_RULE_OCTET_STREAM,
            ALDIS_USLP_RULE_STARTING_SEGMENT,
            ALDIS_USLP_RULE_CONTINUING_SEGMENT,
            ALDIS_USLP_RULE_LAST_SEGMENT,
            ALDIS_USLP_RULE_NO_SEGMENTATION,
        ];
        let crate_rules = [
            ConstructionRule::PacketsSpanningFrames,
            ConstructionRule::SduStart,
            ConstructionRule::SduContinuation,
            ConstructionRule::OctetStream,
            ConstructionRule::StartingSegment,
            ConstructionRule::ContinuingSegment,
            ConstructionRule::LastSegment,
            ConstructionRule::NoSegmentation,
        ];
        assert!(
            rules
                .iter()
                .copied()
                .eq(crate_rules.iter().map(|r| r.bits()))
        );
    }

    #[test]
    fn upid_and_reason_constants_are_the_crate_values() {
        // req: CABI-010, USLP-007, USLP-009, USLP-014
        // Each assigned UPID of the SANA snapshot has its constant, and no other value does
        // (USLP-007).
        let upids = [
            (ALDIS_USLP_UPID_PACKETS, Upid::Packets),
            (
                ALDIS_USLP_UPID_COP1_CONTROL_COMMANDS,
                Upid::Cop1ControlCommands,
            ),
            (
                ALDIS_USLP_UPID_COPP_CONTROL_COMMANDS,
                Upid::CopPControlCommands,
            ),
            (ALDIS_USLP_UPID_OCTET_STREAM, Upid::OctetStream),
            (
                ALDIS_USLP_UPID_MISSION_SPECIFIC_INFORMATION_1,
                Upid::MissionSpecificInformation1,
            ),
            (
                ALDIS_USLP_UPID_PROXIMITY1_PSEUDO_PACKET_ID_1,
                Upid::Proximity1PseudoPacketId1,
            ),
            (ALDIS_USLP_UPID_PROXIMITY1_SPDUS, Upid::Proximity1Spdus),
            (
                ALDIS_USLP_UPID_PROXIMITY1_PSEUDO_PACKET_ID_2,
                Upid::Proximity1PseudoPacketId2,
            ),
            (ALDIS_USLP_UPID_ONLY_IDLE_DATA, Upid::OnlyIdleData),
        ];
        for (c, u) in upids {
            assert_eq!(Upid::from_bits(c), Some(u));
        }
        let assigned = (0..=ALDIS_USLP_UPID_MAX)
            .filter(|&b| UnassignedUpid::new(b).is_none())
            .count();
        assert_eq!(assigned, upids.len());
        assert_eq!(Upid::from_bits(ALDIS_USLP_UPID_MAX + 1), None);
        let reasons = [
            ALDIS_USLP_DISCARD_UNEXPECTED_VERSION,
            ALDIS_USLP_DISCARD_UNEXPECTED_SPACECRAFT,
            ALDIS_USLP_DISCARD_RESERVED_FLAGS,
            ALDIS_USLP_DISCARD_RESERVED_SPARE,
            ALDIS_USLP_DISCARD_INVALID_VCID,
            ALDIS_USLP_DISCARD_INVALID_MAP_ID,
            ALDIS_USLP_DISCARD_TRUNCATED_FRAME_NOT_ALLOWED,
            ALDIS_USLP_DISCARD_VCF_COUNT_LENGTH_MISMATCH,
            ALDIS_USLP_DISCARD_OCF_FLAG_MISMATCH,
            ALDIS_USLP_DISCARD_FIXED_LENGTH_MISMATCH,
            ALDIS_USLP_DISCARD_FRAME_TOO_LONG,
            ALDIS_USLP_DISCARD_FRAME_TOO_SHORT,
            ALDIS_USLP_DISCARD_LENGTH_MISMATCH,
            ALDIS_USLP_DISCARD_FECF_MISMATCH,
            ALDIS_USLP_DISCARD_CONSTRUCTION_RULE_MISMATCH,
            ALDIS_USLP_DISCARD_POINTER_BEYOND_ZONE,
            ALDIS_USLP_DISCARD_OID_HEADER_MISMATCH,
            ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED,
        ];
        assert!(
            reasons
                .iter()
                .copied()
                .eq(Reason::ALL.iter().map(|r| u32::from(r.code())))
        );
        assert_eq!(Reason::from_code(0), None);
        assert_eq!(ALDIS_USLP_DISCARD_NONE, 0);
        assert_eq!(
            (ALDIS_USLP_SOURCE, ALDIS_USLP_DESTINATION),
            (
                u8::from(SourceOrDestination::Source.bit()),
                u8::from(SourceOrDestination::Destination.bit())
            )
        );
    }

    #[test]
    fn enumerations_take_only_their_constants() {
        // req: CABI-010, CABI-022, USLP-005, USLP-007
        assert_eq!(frame_type(2), Err(invalid_argument()));
        let pc = aldis_uslp_config_t {
            fecf: 2,
            frame_len: 64,
            ..aldis_uslp_config_t::default()
        };
        assert_eq!(physical(&pc), Err(invalid_argument()));
        let vc = aldis_uslp_vc_config_t::default();
        assert!(virtual_channel(&vc).is_ok());
        for bad in [
            aldis_uslp_vc_config_t { ocf: 3, ..vc },
            aldis_uslp_vc_config_t { security: 2, ..vc },
            aldis_uslp_vc_config_t {
                frame_type: 2,
                ..vc
            },
        ] {
            assert_eq!(virtual_channel(&bad), Err(invalid_argument()));
        }
        let ch = aldis_uslp_channel_t {
            source_or_destination: 2,
            ..aldis_uslp_channel_t::default()
        };
        assert_eq!(channel(ch), Err(invalid_argument()));
        let f = aldis_uslp_fields_t {
            kind: 3,
            ..aldis_uslp_fields_t::default()
        };
        assert_eq!(fields(f), Err(invalid_argument()));
        let t = aldis_uslp_tfdf_t {
            rule: 8,
            ..aldis_uslp_tfdf_t::default()
        };
        assert_eq!(tfdf(t), Err(invalid_argument()));
        let t = aldis_uslp_tfdf_t {
            upid: ALDIS_USLP_UPID_MAX + 1,
            ..aldis_uslp_tfdf_t::default()
        };
        assert_eq!(tfdf(t), Err(Error::out_of_range(field::UPID, 31, 32)));
        // Every 5-bit UPID crosses, assigned or not (USLP-007).
        for upid in 0..=ALDIS_USLP_UPID_MAX {
            let t = aldis_uslp_tfdf_t {
                rule: ALDIS_USLP_RULE_NO_SEGMENTATION,
                upid,
                pointer: 0,
            };
            assert_eq!(tfdf(t).map(tfdf_view), Ok(t));
        }
        assert_eq!(ocf(&[1, 2, 3]), Err(invalid_argument()));
        assert_eq!(ocf(&[]), Ok(None));
        assert_eq!(empty(1), Err(invalid_argument()));
    }

    #[test]
    fn a_tfdf_header_longer_than_the_data_field_is_an_internal_error() {
        // req: CABI-012, GEN-020
        use aldis::core::layout::{FrameLayout, LayoutSpec};
        let spec = LayoutSpec {
            frame_len: 16,
            primary_header_len: 4,
            secondary_len: 0,
            security: aldis::core::security::SecurityLayout::NONE,
            ocf: false,
            fecf: false,
        };
        let l = FrameLayout::compute(&spec).unwrap();
        assert_eq!(layout(&l, 3).map(|v| v.tfdz.len), Ok(9));
        assert_eq!(
            layout(&l, 13).err().map(|e| e.kind()),
            Some(ErrorKind::InternalError)
        );
    }
}
