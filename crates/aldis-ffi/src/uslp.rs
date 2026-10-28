//! C ABI of the USLP Transfer Frame codec `aldis-uslp` (CCSDS 732.1-B-3; M2 plan D4, D12, D14,
//! D17; CABI-010…012, CABI-020…022, CABI-033).
//!
//! Calls: [`aldis_uslp_config_check`] and [`aldis_uslp_vc_table_check`];
//! [`aldis_uslp_peek_length`]; the decoder [`aldis_uslp_decode`]; the unified Frame Intake
//! Stream [`aldis_uslp_stream_next`]; the one-shot [`aldis_uslp_encode`], which takes the frame's
//! variable fields in an [`aldis_uslp_content_t`]; the two-phase writer as
//! [`aldis_uslp_writer_begin`] and [`aldis_uslp_writer_finish`]; [`aldis_uslp_encode_truncated`];
//! and the OID frame writer [`aldis_uslp_encode_oid`], which takes an `aldis_oid_generator_t`.
//!
//! **Channels.** A physical channel is an [`aldis_uslp_config_t`] (732.1 §5 Table 5-1). The
//! virtual channels and MAPs it carries are passed beside it as two arrays, of
//! [`aldis_uslp_vc_config_t`] (Tables 5-3, 6-1) and [`aldis_uslp_map_config_t`] (Table 5-4),
//! each with its count: the C form of `VcTable::new`, which takes the physical channel and
//! the two lists, built and checked on every call into a `VcTable` of the build-time
//! capacities `ALDIS_USLP_MAX_VCS` and `ALDIS_USLP_MAX_MAPS` (CABI-030). As in `aldis-uslp`,
//! each virtual channel and MAP names its own SCID, so one table holds every master channel of
//! the physical channel. Each array's stride is the `struct_size` of its first struct, which
//! every struct repeats (`abi::configs`; c-abi.md §11). `aldis_uslp_peek_length` takes the
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
//! **Writers.** No call keeps a pointer or state (API-023, API-060). `_writer_begin` writes the
//! unfinished header, the Insert Zone, and the TFDF Header, zero-fills the Security Header and
//! Trailer, and reports where each field lies; the caller fills the TFDZ and the security
//! fields in place; `_writer_finish`, given the same arguments and the TFDZ length used,
//! checks that the buffer holds the headers `_writer_begin` wrote, else
//! `ALDIS_ERR_INVALID_STATE` (`UslpFrameWriter::resume`; OI-50), and finishes the frame:
//! the Frame Length, the OCF, and the FECF.

use aldis::core::layout::{FECF_LEN, OCF_LEN};
use aldis::core::ocf::Ocf;
use aldis::core::security::SecurityLayout;
use aldis::core::{Error, ErrorKind, Result};
use aldis::uslp::field;
use aldis::uslp::{
    ChannelFields, ConstructionRule, Delimited, FrameContent, FrameFields, FrameKind, FrameStream,
    IntakeItem, MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope, SourceOrDestination,
    TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen, Upid, UslpFrame, UslpFrameWriter,
    UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen, encode,
    encode_oid, encode_oid_galois, encode_truncated, next_frame, validate_frame,
};

use crate::abi::{self, Configs, invalid_argument};
use crate::capacities::{ALDIS_USLP_MAX_MAPS, ALDIS_USLP_MAX_VCS};
use crate::oid::{GeneratorState, aldis_oid_generator_t, generator};

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

/// Transfer Frame Type: fixed-length frames (732.1 §5 Tables 5-1, 5-3).
pub const ALDIS_USLP_FRAME_FIXED: u8 = 0;
/// Transfer Frame Type: variable-length frames (732.1 §5 Tables 5-1, 5-3).
pub const ALDIS_USLP_FRAME_VARIABLE: u8 = 1;

/// OCF use of a virtual channel: no frame carries an OCF (732.1 §4.1.5, Table 5-3).
pub const ALDIS_USLP_OCF_ABSENT: u8 = 0;
/// OCF use: a frame may carry an OCF, on a variable-length VC (732.1 §4.1.5.4).
pub const ALDIS_USLP_OCF_ALLOWED: u8 = 1;
/// OCF use: every frame carries an OCF, on a fixed-length VC (732.1 §4.1.5.3).
pub const ALDIS_USLP_OCF_REQUIRED: u8 = 2;

/// Security of a virtual channel: one layout for the VC, its lengths in the VC struct (732.1
/// §6.6 Table 6-1; API-070).
pub const ALDIS_USLP_SECURITY_VC: u8 = 0;
/// Security of a virtual channel: a layout per MAP, in the MAP structs (732.1 §6.6; API-070).
pub const ALDIS_USLP_SECURITY_PER_MAP: u8 = 1;

/// Source-or-Destination Identifier `0`: the SCID is the frame's source (732.1 §4.1.2.3).
pub const ALDIS_USLP_SOURCE: u8 = 0;
/// Source-or-Destination Identifier `1`: the SCID is the frame's destination (732.1 §4.1.2.3).
pub const ALDIS_USLP_DESTINATION: u8 = 1;

/// Frame kind: Bypass/Sequence Control and Protocol Control Command Flags `00`, user data of
/// the Sequence-Controlled service (732.1 Table 4-1; USLP-001).
pub const ALDIS_USLP_KIND_SEQUENCE_CONTROLLED: u8 = 0;
/// Frame kind: flags `10`, user data of the Expedited service (732.1 Table 4-1; USLP-001).
pub const ALDIS_USLP_KIND_EXPEDITED: u8 = 1;
/// Frame kind: flags `11`, Protocol Control Commands (732.1 Table 4-1; USLP-001, USLP-013).
pub const ALDIS_USLP_KIND_PROTOCOL_CONTROL: u8 = 2;

/// TFDZ Construction Rule `000`: packets spanning frames, fixed-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES: u8 = 0b000;
/// Rule `001`: start of a `MAPA_SDU` or `VCA_SDU`, fixed-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_SDU_START: u8 = 0b001;
/// Rule `010`: continuing portion of a `MAPA_SDU` or `VCA_SDU`, fixed-length TFDZ (732.1
/// Table 4-3).
pub const ALDIS_USLP_RULE_SDU_CONTINUATION: u8 = 0b010;
/// Rule `011`: octet stream, variable-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_OCTET_STREAM: u8 = 0b011;
/// Rule `100`: starting segment, variable-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_STARTING_SEGMENT: u8 = 0b100;
/// Rule `101`: continuing segment, variable-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_CONTINUING_SEGMENT: u8 = 0b101;
/// Rule `110`: last segment, variable-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_LAST_SEGMENT: u8 = 0b110;
/// Rule `111`: no segmentation, variable-length TFDZ (732.1 Table 4-3).
pub const ALDIS_USLP_RULE_NO_SEGMENTATION: u8 = 0b111;

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

/// Stream item: no octets of the unit remain (732.1 §4.3.11.2; USLP-015).
pub const ALDIS_USLP_STREAM_END: u8 = 0;
/// Stream item: one validated frame (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
pub const ALDIS_USLP_STREAM_ACCEPTED: u8 = 1;
/// Stream item: discarded octets (fill, incomplete frame, or failed validation) (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
pub const ALDIS_USLP_STREAM_DISCARDED: u8 = 2;

/// Discard reason: accepted frame (not discarded).
pub const ALDIS_USLP_DISCARD_NONE: u32 = 0;
/// Discard reason: test (a), TFVN is not `1100` (732.1 §4.1.2.2.2; USLP-001).
pub const ALDIS_USLP_DISCARD_UNEXPECTED_VERSION: u32 = 1;
/// Discard reason: test (b), Spacecraft ID names no Master Channel of the Physical Channel (732.1 §4.3.11.3; USLP-014).
pub const ALDIS_USLP_DISCARD_UNEXPECTED_SPACECRAFT: u32 = 2;
/// Discard reason: test (c), Bypass Flag `0` with Protocol Control Command Flag `1` (732.1 Table 4-1; USLP-001).
pub const ALDIS_USLP_DISCARD_RESERVED_FLAGS: u32 = 3;
/// Discard reason: test (c), strict mode only: Reserve Spares are not `00` (732.1 §4.1.2.9.2).
pub const ALDIS_USLP_DISCARD_RESERVED_SPARE: u32 = 4;
/// Discard reason: test (c), VCID is not valid on the Master Channel (732.1 §4.3.11.3; USLP-014).
pub const ALDIS_USLP_DISCARD_INVALID_VCID: u32 = 5;
/// Discard reason: test (c), MAP ID is not valid on the VC (732.1 §4.3.11.3; USLP-014).
pub const ALDIS_USLP_DISCARD_INVALID_MAP_ID: u32 = 6;
/// Discard reason: test (c), a truncated frame on a VC without a truncated frame length (732.1 Annex D1.2.1; USLP-002).
pub const ALDIS_USLP_DISCARD_TRUNCATED_FRAME_NOT_ALLOWED: u32 = 7;
/// Discard reason: test (c), VCF Count Length is not the VC's for the frame's quality of service (732.1 §4.1.2.11.4; USLP-004).
pub const ALDIS_USLP_DISCARD_VCF_COUNT_LENGTH_MISMATCH: u32 = 8;
/// Discard reason: test (c), OCF Flag disagrees with the VC's OCF use (732.1 §4.1.5.3–4).
pub const ALDIS_USLP_DISCARD_OCF_FLAG_MISMATCH: u32 = 9;
/// Discard reason: test (c), on a fixed-length Physical Channel, Frame Length is not the configured length (732.1 §4.1.2.7; USLP-003).
pub const ALDIS_USLP_DISCARD_FIXED_LENGTH_MISMATCH: u32 = 10;
/// Discard reason: test (c), on a variable-length Physical Channel, Frame Length exceeds the maximum (732.1 Table 5-1; USLP-003).
pub const ALDIS_USLP_DISCARD_FRAME_TOO_LONG: u32 = 11;
/// Discard reason: test (c), Frame Length leaves no TFDZ octet after the fields present (CMN-041; OI-44 item 2).
pub const ALDIS_USLP_DISCARD_FRAME_TOO_SHORT: u32 = 12;
/// Discard reason: test (d), octets present disagree with Frame Length or truncated frame length (732.1 §4.3.11.3.4 d).
pub const ALDIS_USLP_DISCARD_LENGTH_MISMATCH: u32 = 13;
/// Discard reason: test (e), FECF does not match recomputed CRC (732.1 §4.1.6; Annex B).
pub const ALDIS_USLP_DISCARD_FECF_MISMATCH: u32 = 14;
/// Discard reason: TFDZ Construction Rule is of the other TFDZ type than the VC's (732.1 USLP-016).
pub const ALDIS_USLP_DISCARD_CONSTRUCTION_RULE_MISMATCH: u32 = 15;
/// Discard reason: First Header or Last Valid Octet Pointer lies beyond the TFDZ (732.1 USLP-006).
pub const ALDIS_USLP_DISCARD_POINTER_BEYOND_ZONE: u32 = 16;
/// Discard reason: OID frame on VCID 63 deviates from required header or length (732.1 Annex D1.2.4; USLP-007).
pub const ALDIS_USLP_DISCARD_OID_HEADER_MISMATCH: u32 = 17;
/// Discard reason: a non-truncated frame on a VC that has a truncated frame length (732.1 Annex D1.2.1; USLP-002; OI-48).
pub const ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED: u32 = 18;
/// Discard reason: fill data (732.1 §4.3.11.2.4 a–b; USLP-015).
pub const ALDIS_USLP_DISCARD_FILL: u32 = 20;
/// Discard reason: incomplete frame whose declared length exceeds remaining octets (732.1 §4.3.11.2.3; USLP-015).
pub const ALDIS_USLP_DISCARD_INCOMPLETE_FRAME: u32 = 21;
/// Discard reason: truncated header whose SCID and VCID name no configured VC or no truncated length (732.1 USLP-015).
pub const ALDIS_USLP_DISCARD_NO_TRUNCATED_LENGTH: u32 = 22;
/// Discard reason: non-truncated header on a VC that carries only truncated frames (732.1 OI-48; USLP-015).
pub const ALDIS_USLP_DISCARD_NON_TRUNCATED_NOT_ALLOWED: u32 =
    ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED;

// Aliases for validation check codes
/// Alias for [`ALDIS_USLP_DISCARD_NONE`].
pub const ALDIS_USLP_REASON_NONE: u32 = ALDIS_USLP_DISCARD_NONE;
/// Alias for [`ALDIS_USLP_DISCARD_UNEXPECTED_VERSION`].
pub const ALDIS_USLP_REASON_UNEXPECTED_VERSION: u32 = ALDIS_USLP_DISCARD_UNEXPECTED_VERSION;
/// Alias for [`ALDIS_USLP_DISCARD_UNEXPECTED_SPACECRAFT`].
pub const ALDIS_USLP_REASON_UNEXPECTED_SPACECRAFT: u32 = ALDIS_USLP_DISCARD_UNEXPECTED_SPACECRAFT;
/// Alias for [`ALDIS_USLP_DISCARD_RESERVED_FLAGS`].
pub const ALDIS_USLP_REASON_RESERVED_FLAGS: u32 = ALDIS_USLP_DISCARD_RESERVED_FLAGS;
/// Alias for [`ALDIS_USLP_DISCARD_RESERVED_SPARE`].
pub const ALDIS_USLP_REASON_RESERVED_SPARE: u32 = ALDIS_USLP_DISCARD_RESERVED_SPARE;
/// Alias for [`ALDIS_USLP_DISCARD_INVALID_VCID`].
pub const ALDIS_USLP_REASON_INVALID_VCID: u32 = ALDIS_USLP_DISCARD_INVALID_VCID;
/// Alias for [`ALDIS_USLP_DISCARD_INVALID_MAP_ID`].
pub const ALDIS_USLP_REASON_INVALID_MAP_ID: u32 = ALDIS_USLP_DISCARD_INVALID_MAP_ID;
/// Alias for [`ALDIS_USLP_DISCARD_TRUNCATED_FRAME_NOT_ALLOWED`].
pub const ALDIS_USLP_REASON_TRUNCATED_FRAME_NOT_ALLOWED: u32 =
    ALDIS_USLP_DISCARD_TRUNCATED_FRAME_NOT_ALLOWED;
/// Alias for [`ALDIS_USLP_DISCARD_VCF_COUNT_LENGTH_MISMATCH`].
pub const ALDIS_USLP_REASON_VCF_COUNT_LENGTH_MISMATCH: u32 =
    ALDIS_USLP_DISCARD_VCF_COUNT_LENGTH_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_OCF_FLAG_MISMATCH`].
pub const ALDIS_USLP_REASON_OCF_FLAG_MISMATCH: u32 = ALDIS_USLP_DISCARD_OCF_FLAG_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_FIXED_LENGTH_MISMATCH`].
pub const ALDIS_USLP_REASON_FIXED_LENGTH_MISMATCH: u32 = ALDIS_USLP_DISCARD_FIXED_LENGTH_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_FRAME_TOO_LONG`].
pub const ALDIS_USLP_REASON_FRAME_TOO_LONG: u32 = ALDIS_USLP_DISCARD_FRAME_TOO_LONG;
/// Alias for [`ALDIS_USLP_DISCARD_FRAME_TOO_SHORT`].
pub const ALDIS_USLP_REASON_FRAME_TOO_SHORT: u32 = ALDIS_USLP_DISCARD_FRAME_TOO_SHORT;
/// Alias for [`ALDIS_USLP_DISCARD_LENGTH_MISMATCH`].
pub const ALDIS_USLP_REASON_LENGTH_MISMATCH: u32 = ALDIS_USLP_DISCARD_LENGTH_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_FECF_MISMATCH`].
pub const ALDIS_USLP_REASON_FECF_MISMATCH: u32 = ALDIS_USLP_DISCARD_FECF_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_CONSTRUCTION_RULE_MISMATCH`].
pub const ALDIS_USLP_REASON_CONSTRUCTION_RULE_MISMATCH: u32 =
    ALDIS_USLP_DISCARD_CONSTRUCTION_RULE_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_POINTER_BEYOND_ZONE`].
pub const ALDIS_USLP_REASON_POINTER_BEYOND_ZONE: u32 = ALDIS_USLP_DISCARD_POINTER_BEYOND_ZONE;
/// Alias for [`ALDIS_USLP_DISCARD_OID_HEADER_MISMATCH`].
pub const ALDIS_USLP_REASON_OID_HEADER_MISMATCH: u32 = ALDIS_USLP_DISCARD_OID_HEADER_MISMATCH;
/// Alias for [`ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED`].
pub const ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED: u32 =
    ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED;

/// Delimited item: no octets remain at the offset (732.1 §4.3.11.2; USLP-015).
pub const ALDIS_USLP_DELIMITED_END: u8 = 0;
/// Delimited item: one frame, as long as its header declares, for the validation checks
/// (732.1 §4.3.11.2.2, §4.3.11.2.4 c).
pub const ALDIS_USLP_DELIMITED_FRAME: u8 = 1;
/// Delimited item: the rest of the unit, discarded: a valid header that declares more octets
/// than remain, `declared_len` (732.1 §4.3.11.2.3).
pub const ALDIS_USLP_DELIMITED_INCOMPLETE_FRAME: u8 = 2;
/// Delimited item: the rest of the unit, discarded: a truncated header of no configured VC, or
/// of a VC without a truncated frame length (USLP-015; OI-35 item 8).
pub const ALDIS_USLP_DELIMITED_NO_TRUNCATED_LENGTH: u8 = 3;
/// Delimited item: the rest of the unit, discarded as Fill Data (732.1 §4.3.11.2.4 a–b).
pub const ALDIS_USLP_DELIMITED_FILL: u8 = 4;
/// Delimited item: the rest of the unit, discarded: a non-truncated header on a VC that has a
/// truncated frame length and so carries only truncated frames (USLP-015; OI-48).
pub const ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED: u8 = 5;

/// Managed parameters of a physical channel (732.1 §5 Table 5-1; USLP-003, USLP-008, API-040,
/// CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_config_t {
    /// `sizeof(aldis_uslp_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
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
/// USLP-002, USLP-004, USLP-012, API-070, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_vc_config_t {
    /// `sizeof(aldis_uslp_vc_config_t)`, set by the caller, the same for every struct of an
    /// array (CABI-033).
    pub struct_size: u32,
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

/// Managed parameters of a MAP channel (732.1 §5 Table 5-4, §6.6; API-070, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_map_config_t {
    /// `sizeof(aldis_uslp_map_config_t)`, set by the caller, the same for every struct of an
    /// array (CABI-033).
    pub struct_size: u32,
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
    /// Offset of the Insert Zone, after the Primary Header and its VCF Count.
    pub insert_zone_offset: usize,
    /// Length of the Insert Zone; 0 without one (§4.1.3).
    pub insert_zone_len: usize,
    /// Offset of the Security Header.
    pub security_header_offset: usize,
    /// Length of the Security Header; 0 without SDLS (§6.3.4).
    pub security_header_len: usize,
    /// Offset of the TFDF Header.
    pub tfdf_header_offset: usize,
    /// Length of the TFDF Header: 3 for rules `000` to `010`, else 1 (§4.1.4.2.1.2; USLP-005).
    pub tfdf_header_len: usize,
    /// Offset of the Transfer Frame Data Zone.
    pub tfdz_offset: usize,
    /// Length of the TFDZ, never 0 (§4.1.4.3).
    pub tfdz_len: usize,
    /// Offset of the Security Trailer.
    pub security_trailer_offset: usize,
    /// Length of the Security Trailer; 0 without one (§6.3.6).
    pub security_trailer_len: usize,
    /// Offset of the Operational Control Field.
    pub ocf_offset: usize,
    /// Length of the OCF: `ALDIS_OCF_LEN` or 0 (§4.1.5).
    pub ocf_len: usize,
    /// Offset of the Frame Error Control Field.
    pub fecf_offset: usize,
    /// Length of the FECF: 2 or 0 (§4.1.6).
    pub fecf_len: usize,
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

/// One item of a data unit (732.1 §4.3.11.2; USLP-015, M2 plan D14): its octets are the `len`
/// octets of the unit at `offset`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_uslp_delimited_t {
    /// `ALDIS_USLP_DELIMITED_*`.
    pub kind: u8,
    /// Offset of the item in the unit: the offset the call was given.
    pub offset: usize,
    /// Octets the item takes; 0 only for `ALDIS_USLP_DELIMITED_END`. Every item but a frame
    /// takes the rest of the unit.
    pub len: usize,
    /// The frame length the header declares, or its VC's truncated frame length: `len` for a
    /// frame, more than `len` for `ALDIS_USLP_DELIMITED_INCOMPLETE_FRAME`, 0 for the other kinds,
    /// as `aldis_tc_delimited_t` gives it.
    pub declared_len: usize,
}

/// One item yielded by [`aldis_uslp_stream_next`] (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_uslp_stream_item_t {
    /// Item kind: [`ALDIS_USLP_STREAM_END`], [`ALDIS_USLP_STREAM_ACCEPTED`], or [`ALDIS_USLP_STREAM_DISCARDED`].
    pub kind: u8,
    /// Reserved padding octets, set to zero.
    pub reserved: [u8; 3],
    /// Starting offset of this item in the caller's unit buffer.
    pub offset: usize,
    /// Length of this item in octets consumed from the unit buffer.
    pub len: usize,
    /// The decoded frame view (valid when `kind == ALDIS_USLP_STREAM_ACCEPTED`).
    pub frame: aldis_uslp_frame_t,
    /// Discard reason code: [`ALDIS_USLP_DISCARD_*`] (valid when `kind == ALDIS_USLP_STREAM_DISCARDED`).
    pub discard_reason: u32,
    /// The error code of the discard (e.g. `ALDIS_ERR_CRC_MISMATCH`, negative integer).
    pub error_code: i32,
}

impl Default for aldis_uslp_stream_item_t {
    fn default() -> Self {
        Self {
            kind: ALDIS_USLP_STREAM_END,
            reserved: [0; 3],
            offset: 0,
            len: 0,
            frame: aldis_uslp_frame_t::default(),
            discard_reason: 0,
            error_code: 0,
        }
    }
}

/// The channel tables of the build-time capacities (CABI-030).
type Table = VcTable<ALDIS_USLP_MAX_VCS, ALDIS_USLP_MAX_MAPS>;

/// A Transfer Frame Type of a C struct (Tables 5-1, 5-3; CABI-022).
const fn frame_type(value: u8) -> Result<TransferFrameType> {
    match value {
        ALDIS_USLP_FRAME_FIXED => Ok(TransferFrameType::Fixed),
        ALDIS_USLP_FRAME_VARIABLE => Ok(TransferFrameType::Variable),
        _ => Err(invalid_argument()),
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
    let ocf = match v.ocf {
        ALDIS_USLP_OCF_ABSENT => OcfUse::Absent,
        ALDIS_USLP_OCF_ALLOWED => OcfUse::Allowed,
        ALDIS_USLP_OCF_REQUIRED => OcfUse::Required,
        _ => return Err(invalid_argument()),
    };
    let truncated_frame_len = match v.truncated_frame_len {
        0 => None,
        n => Some(TruncatedFrameLen::new(usize::from(n))?),
    };
    let security = match v.security {
        ALDIS_USLP_SECURITY_VC => SecurityScope::Vc(SecurityLayout::new(
            usize::from(v.security_header_len),
            length(v.security_trailer_len),
        )?),
        ALDIS_USLP_SECURITY_PER_MAP => SecurityScope::PerMap,
        _ => return Err(invalid_argument()),
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
/// `vcs`, and the `map_count` MAPs at `maps` (CABI-022, CABI-033).
///
/// # Safety
/// As for `abi::config` and `abi::configs`.
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
            abi::config(pc)?,
            abi::configs(vcs, vc_count)?,
            abi::configs(maps, map_count)?,
        )
    };
    table(&pc, &vcs, &maps)
}

/// The fields of the first four octets of a C struct (732.1 §4.1.2.2–5; RS-020).
fn channel(c: aldis_uslp_channel_t) -> Result<ChannelFields> {
    let spacecraft_id = UslpSpacecraftId::try_from(c.scid)?;
    let source_or_destination = match c.source_or_destination {
        ALDIS_USLP_SOURCE => SourceOrDestination::Source,
        ALDIS_USLP_DESTINATION => SourceOrDestination::Destination,
        _ => return Err(invalid_argument()),
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
    let kind = match f.kind {
        ALDIS_USLP_KIND_SEQUENCE_CONTROLLED => FrameKind::SequenceControlled,
        ALDIS_USLP_KIND_EXPEDITED => FrameKind::Expedited,
        ALDIS_USLP_KIND_PROTOCOL_CONTROL => FrameKind::ProtocolControl,
        _ => return Err(invalid_argument()),
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

/// The offset of `part`, a subslice of `whole`, in `whole` (CABI-012).
fn offset(whole: &[u8], part: &[u8]) -> usize {
    part.as_ptr().addr().saturating_sub(whole.as_ptr().addr())
}

/// The layout of the fields that follow the Security Trailer at `trailer_end`: the OCF and the
/// FECF, each present or not (§4.1.5, §4.1.6).
const fn tail(l: &mut aldis_uslp_layout_t, trailer_end: usize, ocf: bool, fecf: bool) {
    l.ocf_offset = trailer_end;
    l.ocf_len = if ocf { OCF_LEN } else { 0 };
    l.fecf_offset = l.ocf_offset.saturating_add(l.ocf_len);
    l.fecf_len = if fecf { FECF_LEN } else { 0 };
}

/// The C view of a decoded frame, its offsets into `bytes`, the decoder's input (CABI-012).
fn view(f: &UslpFrame<'_>, bytes: &[u8]) -> aldis_uslp_frame_t {
    let tfdf = f.tfdf_header();
    let tfdz_offset = offset(bytes, f.tfdz());
    let security_trailer_offset = offset(bytes, f.security_trailer());
    let mut layout = aldis_uslp_layout_t {
        frame_len: f.len(),
        insert_zone_offset: aldis::uslp::TRUNCATED_HEADER_LEN,
        insert_zone_len: 0,
        security_header_offset: offset(bytes, f.security_header()),
        security_header_len: f.security_header().len(),
        tfdf_header_offset: tfdz_offset.saturating_sub(tfdf.len()),
        tfdf_header_len: tfdf.len(),
        tfdz_offset,
        tfdz_len: f.tfdz().len(),
        security_trailer_offset,
        security_trailer_len: f.security_trailer().len(),
        ..aldis_uslp_layout_t::default()
    };
    let trailer_end = security_trailer_offset.saturating_add(f.security_trailer().len());
    let mut v = aldis_uslp_frame_t {
        truncated: f.is_truncated(),
        channel: channel_view(f.channel()),
        tfdf: tfdf_view(tfdf),
        upid_assigned: !matches!(tfdf.upid, Upid::Other(_)),
        anomalies: f.anomalies().bits(),
        ..aldis_uslp_frame_t::default()
    };
    if let Some(full) = f.full() {
        let h = full.header();
        layout.insert_zone_offset = offset(bytes, full.insert_zone());
        layout.insert_zone_len = full.insert_zone().len();
        tail(
            &mut layout,
            trailer_end,
            full.ocf().is_some(),
            full.fecf().is_some(),
        );
        v.kind = kind_view(h.kind());
        v.ocf_flag = h.ocf_flag();
        v.vcf_count_len = h.vcf_count().len().get();
        v.vcf_count = h.vcf_count().value();
        v.has_fecf = full.fecf().is_some();
        v.fecf = full.fecf().unwrap_or(0);
    } else {
        v.kind = kind_view(FrameKind::Expedited);
        tail(&mut layout, trailer_end, false, false);
    }
    v.layout = layout;
    v
}

/// The layout of a frame reserved by `w` in the buffer that starts at address `base` (§4.1,
/// §6.3; CABI-012).
fn writer_layout(
    base: usize,
    w: &mut UslpFrameWriter<'_>,
    content: &FrameContent<'_>,
    fecf: bool,
) -> aldis_uslp_layout_t {
    let at = |s: &[u8]| (s.as_ptr().addr().saturating_sub(base), s.len());
    let (security_header_offset, security_header_len) = at(w.security_header_mut());
    let (tfdz_offset, tfdz_len) = at(w.tfdz_mut());
    let (security_trailer_offset, security_trailer_len) = at(w.security_trailer_mut());
    let mut l = aldis_uslp_layout_t {
        insert_zone_offset: security_header_offset.saturating_sub(content.insert_zone.len()),
        insert_zone_len: content.insert_zone.len(),
        security_header_offset,
        security_header_len,
        tfdf_header_offset: tfdz_offset.saturating_sub(content.tfdf.len()),
        tfdf_header_len: content.tfdf.len(),
        tfdz_offset,
        tfdz_len,
        security_trailer_offset,
        security_trailer_len,
        ..aldis_uslp_layout_t::default()
    };
    let trailer_end = security_trailer_offset.saturating_add(security_trailer_len);
    tail(&mut l, trailer_end, content.ocf.is_some(), fecf);
    l.frame_len = l.fecf_offset.saturating_add(l.fecf_len);
    l
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
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || physical(&abi::config(pc)?).map(drop)) }
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
/// including an enumeration member that is no constant and a struct whose `struct_size` differs
/// from its array's first one. `vc_count` 0 is `ALDIS_ERR_INVALID_CONFIG` (`vc_id`) once
/// the structs have parsed (USLP-017, OI-55).
///
/// # Safety
/// `pc` is valid for a read; `vcs` is valid for reads of `vc_count` structs of the first one's
/// `struct_size`, and `maps` of `map_count` (each `NULL` with 0); `err` is `NULL` or valid for
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
) -> i32 {
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
) -> i32 {
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
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(view(&UslpFrame::decode(bytes, &t, mode)?, bytes));
            Ok(())
        })
    }
}

/// Applies the USLP Frame Validation Checks to `bytes`, the octets of exactly one frame, as the
/// delimiter yields them (732.1 §4.3.11.3; USLP-014, M2 plan D14, D17), as `validate_frame`
/// does.
///
/// On success the frame is written to `out`, as [`aldis_uslp_decode`] writes it, and `reason`
/// is `ALDIS_USLP_REASON_NONE`. When a check fails, `reason` is its stable code,
/// `ALDIS_USLP_REASON_*` (`docs/standards/uslp-frame.md` §3.1), the call returns the decoder's
/// error for it, and `out` is unchanged.
///
/// # Errors
/// The errors of [`aldis_uslp_decode`], with `ALDIS_ERR_LENGTH_MISMATCH` (`frame_len`) where it
/// returns `ALDIS_ERR_INCOMPLETE`, each with its reason; the errors of
/// [`aldis_uslp_vc_table_check`] and CABI-022 violations, which write no reason.
///
/// # Safety
/// As [`aldis_uslp_decode`]; `reason` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_validate(
    bytes: *const u8,
    len: usize,
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    mode: u32,
    out: *mut aldis_uslp_frame_t,
    reason: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let reason = abi::out(reason)?;
            match validate_frame(bytes, &t, mode) {
                Ok(f) => {
                    out.write(view(&f, bytes));
                    reason.write(ALDIS_USLP_REASON_NONE);
                    Ok(())
                }
                Err(r) => {
                    reason.write(u32::from(r.reason().code()));
                    Err(r.error())
                }
            }
        })
    }
}

/// Steps through the clean data unit `unit` using the unified Frame Intake Stream
/// (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
///
/// Reads from `*offset` and advances `*offset` by the octets consumed for the yielded item.
/// When `out.kind == ALDIS_USLP_STREAM_ACCEPTED`, `out.frame` holds the validated frame view.
/// When `out.kind == ALDIS_USLP_STREAM_DISCARDED`, `out.discard_reason` holds the [`ALDIS_USLP_DISCARD_*`]
/// code, and `out.error_code` holds the negative error code.
/// When `out.kind == ALDIS_USLP_STREAM_END`, the unit is exhausted.
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
    mode: u32,
    out: *mut aldis_uslp_stream_item_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
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
                    offset: current_offset,
                    len: 0,
                    frame: aldis_uslp_frame_t::default(),
                    discard_reason: 0,
                    error_code: 0,
                });
                return Ok(());
            }

            let Some(rest) = unit.get(current_offset..) else {
                return Err(invalid_argument());
            };
            let mut stream = FrameStream::new(rest, &t, mode);
            match stream.next() {
                None => {
                    out.write(aldis_uslp_stream_item_t {
                        kind: ALDIS_USLP_STREAM_END,
                        reserved: [0; 3],
                        offset: current_offset,
                        len: 0,
                        frame: aldis_uslp_frame_t::default(),
                        discard_reason: 0,
                        error_code: 0,
                    });
                }
                Some(IntakeItem::Accepted { frame, .. }) => {
                    let len = frame.len();
                    *offset_ref = current_offset
                        .checked_add(len)
                        .ok_or_else(invalid_argument)?;
                    out.write(aldis_uslp_stream_item_t {
                        kind: ALDIS_USLP_STREAM_ACCEPTED,
                        reserved: [0; 3],
                        offset: current_offset,
                        len,
                        frame: view(&frame, frame.octets()),
                        discard_reason: 0,
                        error_code: 0,
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
                    out.write(aldis_uslp_stream_item_t {
                        kind: ALDIS_USLP_STREAM_DISCARDED,
                        reserved: [0; 3],
                        offset: current_offset,
                        len,
                        frame: aldis_uslp_frame_t::default(),
                        discard_reason: u32::from(reason.code()),
                        error_code: crate::error::status(error),
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
/// Header and Trailer are zeros, for which [`aldis_uslp_writer_begin`] gives the place to fill
/// (API-072).
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
) -> i32 {
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

/// Starts a two-phase write of one non-truncated frame with room for up to `max_tfdz_len` TFDZ
/// octets (732.1 §4.1, §6.3; USLP-010, API-020, API-023, API-072; M2 plan D4).
///
/// Checks the arguments, writes the unfinished Primary Header (TFVN `1111`, which every decoder
/// rejects), the Insert Zone, and the TFDF Header in the first octets of `out`, zero-fills the
/// Security Header and Trailer, and reports where each field lies. On a fixed-length channel
/// `max_tfdz_len` must fill the frame.
///
/// The caller fills the TFDZ and, with SDLS, the Security Header and Trailer at the offsets of
/// `layout`, and calls [`aldis_uslp_writer_finish`] with the same arguments. `content->tfdz`
/// must be empty: the TFDZ is filled in place. No state is kept.
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_uslp_encode`] for a frame with
/// `max_tfdz_len` TFDZ octets, except the pointer, which [`aldis_uslp_writer_finish`] checks;
/// CABI-022 violations, including a non-empty `content->tfdz`.
///
/// # Safety
/// As [`aldis_uslp_encode`], with `layout` valid for one write in place of `out_len`.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_writer_begin(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    fields: *const aldis_uslp_fields_t,
    content: *const aldis_uslp_content_t,
    max_tfdz_len: usize,
    out: *mut u8,
    cap: usize,
    layout: *mut aldis_uslp_layout_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let c = abi::read(content)?;
            empty(c.tfdz_len)?;
            let content = frame_content(&c)?;
            let out = abi::bytes_mut(out, cap, &c.buffers())?;
            let layout = abi::out(layout)?;
            let base = out.as_ptr().addr();
            let mut w = UslpFrameWriter::begin(out, &t, fields, content, max_tfdz_len)?;
            let fecf = t.physical().fecf();
            layout.write(writer_layout(base, &mut w, &content, fecf));
            Ok(())
        })
    }
}

/// Finishes a two-phase write begun with the same arguments, with `tfdz_len` TFDZ octets
/// (732.1 §4.1.2.7, §4.1.5, §4.1.6; USLP-010, API-021, API-023).
///
/// First checks that `out` holds the unfinished Primary Header (with the reserved frame
/// length), the Insert Zone, and the TFDF Header that [`aldis_uslp_writer_begin`] wrote for these
/// arguments (`UslpFrameWriter::resume`; OI-50); the Security Header and Trailer and the TFDZ are
/// not compared. Then keeps the TFDZ and the security fields the caller filled, moves the Security Trailer to follow a TFDZ
/// shorter than the reservation, writes the Frame Length, the OCF, and the FECF if configured,
/// and returns the frame length. The frame is the first `*out_len` octets of `out`; the octets
/// after it keep unspecified values (`UslpFrameWriter::finish`).
///
/// # Errors
/// As [`aldis_uslp_writer_begin`]; `ALDIS_ERR_INVALID_STATE` if `out` does not hold the unfinished
/// headers `aldis_uslp_writer_begin` writes for these arguments (other arguments, or a buffer
/// never begun); then `ALDIS_ERR_OUT_OF_RANGE` (`user_len`) above
/// `max_tfdz_len`, `ALDIS_ERR_INVALID_FIELD` (`frame_data_field`) for 0,
/// `ALDIS_ERR_LENGTH_MISMATCH` (`frame_data_field`) below it on a fixed-length channel, and
/// `ALDIS_ERR_OUT_OF_RANGE` (`fhp_lvo_pointer`) for a pointer not below `tfdz_len`. On error
/// `out` is unchanged, or, after the argument checks, holds the unfinished frame.
///
/// # Safety
/// As [`aldis_uslp_encode`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_writer_finish(
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    maps: *const aldis_uslp_map_config_t,
    map_count: usize,
    fields: *const aldis_uslp_fields_t,
    content: *const aldis_uslp_content_t,
    max_tfdz_len: usize,
    tfdz_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, maps, map_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let c = abi::read(content)?;
            empty(c.tfdz_len)?;
            let content = frame_content(&c)?;
            let out = abi::bytes_mut(out, cap, &c.buffers())?;
            let out_len = abi::out(out_len)?;
            let w = UslpFrameWriter::resume(out, &t, fields, content, max_tfdz_len)?;
            out_len.write(w.finish(tfdz_len)?);
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
) -> i32 {
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
) -> i32 {
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

/// The next item of the data unit `unit` from `offset`, the first octet not yet delimited
/// (732.1 §4.3.11.2; USLP-015, M2 plan D14, D17).
///
/// As `aldis_uslp::next_frame` gives it: a frame, which the caller passes to
/// [`aldis_uslp_validate`] and steps over, or the rest of the unit, discarded. A C caller loops
/// from offset 0, adding each item's `len`, until the kind is `ALDIS_USLP_DELIMITED_END`;
/// every other item takes at least one octet.
///
/// The unit is the octets of one data unit that TC Synchronization and Channel Coding delivered
/// error-free (§4.3.11.2.1). Truncated frames are delimited by their VC's truncated frame
/// length, which is why the call takes the virtual channels.
///
/// # Errors
/// The errors of [`aldis_uslp_vc_table_check`] without MAPs; CABI-022 violations, including an
/// `offset` beyond `unit_len`.
///
/// # Safety
/// `unit` is valid for `unit_len` octets (or `NULL` with 0); `pc` and `vcs` as for
/// [`aldis_uslp_vc_table_check`]; `out` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_uslp_next_frame(
    unit: *const u8,
    unit_len: usize,
    offset: usize,
    pc: *const aldis_uslp_config_t,
    vcs: *const aldis_uslp_vc_config_t,
    vc_count: usize,
    out: *mut aldis_uslp_delimited_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let t = table_at(pc, vcs, vc_count, core::ptr::null(), 0)?;
            let unit = abi::bytes(unit, unit_len)?;
            let rest = unit.get(offset..).ok_or_else(invalid_argument)?;
            let out = abi::out(out)?;
            let item = |kind, len, declared_len| aldis_uslp_delimited_t {
                kind,
                offset,
                len,
                declared_len,
            };
            out.write(match next_frame(rest, &t) {
                None => item(ALDIS_USLP_DELIMITED_END, 0, 0),
                Some(Delimited::Frame(o)) => item(ALDIS_USLP_DELIMITED_FRAME, o.len(), o.len()),
                Some(Delimited::IncompleteFrame {
                    octets,
                    declared_len,
                }) => item(
                    ALDIS_USLP_DELIMITED_INCOMPLETE_FRAME,
                    octets.len(),
                    declared_len,
                ),
                Some(Delimited::NoTruncatedLength(o)) => {
                    item(ALDIS_USLP_DELIMITED_NO_TRUNCATED_LENGTH, o.len(), 0)
                }
                Some(Delimited::NonTruncatedNotAllowed(o)) => {
                    item(ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED, o.len(), 0)
                }
                Some(Delimited::Fill(o)) => item(ALDIS_USLP_DELIMITED_FILL, o.len(), 0),
            });
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
            ALDIS_USLP_REASON_UNEXPECTED_VERSION,
            ALDIS_USLP_REASON_UNEXPECTED_SPACECRAFT,
            ALDIS_USLP_REASON_RESERVED_FLAGS,
            ALDIS_USLP_REASON_RESERVED_SPARE,
            ALDIS_USLP_REASON_INVALID_VCID,
            ALDIS_USLP_REASON_INVALID_MAP_ID,
            ALDIS_USLP_REASON_TRUNCATED_FRAME_NOT_ALLOWED,
            ALDIS_USLP_REASON_VCF_COUNT_LENGTH_MISMATCH,
            ALDIS_USLP_REASON_OCF_FLAG_MISMATCH,
            ALDIS_USLP_REASON_FIXED_LENGTH_MISMATCH,
            ALDIS_USLP_REASON_FRAME_TOO_LONG,
            ALDIS_USLP_REASON_FRAME_TOO_SHORT,
            ALDIS_USLP_REASON_LENGTH_MISMATCH,
            ALDIS_USLP_REASON_FECF_MISMATCH,
            ALDIS_USLP_REASON_CONSTRUCTION_RULE_MISMATCH,
            ALDIS_USLP_REASON_POINTER_BEYOND_ZONE,
            ALDIS_USLP_REASON_OID_HEADER_MISMATCH,
            ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED,
        ];
        assert!(
            reasons
                .iter()
                .copied()
                .eq(Reason::ALL.iter().map(|r| u32::from(r.code())))
        );
        assert_eq!(Reason::from_code(0), None);
        assert_eq!(ALDIS_USLP_REASON_NONE, 0);
        assert_eq!(
            (ALDIS_USLP_SOURCE, ALDIS_USLP_DESTINATION),
            (
                u8::from(SourceOrDestination::Source.bit()),
                u8::from(SourceOrDestination::Destination.bit())
            )
        );
        assert_eq!(core::mem::offset_of!(aldis_uslp_config_t, struct_size), 0);
        assert_eq!(
            core::mem::offset_of!(aldis_uslp_vc_config_t, struct_size),
            0
        );
        assert_eq!(
            core::mem::offset_of!(aldis_uslp_map_config_t, struct_size),
            0
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
}
