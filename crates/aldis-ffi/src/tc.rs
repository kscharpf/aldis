//! C ABI of the TC Space Data Link Protocol codec `aldis-tc` and the CLCW (CCSDS 232.0-B-4;
//! M2 plan D4, D14; CABI-010…012, CABI-020…022).
//!
//! Calls: [`aldis_tc_config_check`] and [`aldis_tc_mc_config_check`]; [`aldis_tc_peek_length`]
//! and [`aldis_tc_header_decode`]; the decoder [`aldis_tc_decode`]; the unified Frame Intake
//! Stream [`aldis_tc_stream_next`]; the one-shot [`aldis_tc_encode`], which takes the frame's data
//! field in an [`aldis_tc_content_t`]; the Control Commands [`aldis_tc_control_decode`] and
//! [`aldis_tc_control_encode`]; and the CLCW [`aldis_clcw_decode`] and [`aldis_clcw_encode`].
//!
//! **Channels** (c-abi.md §11). A physical channel is an [`aldis_tc_config_t`] (232.0 §5.1
//! Table 5-1). A master channel is an [`aldis_tc_mc_config_t`], which holds its physical
//! channel and its Spacecraft ID (Table 5-2), with an array of [`aldis_tc_vc_config_t`] (Tables
//! 5-3, 6-2) and an array of [`aldis_tc_map_config_t`] (Tables 5-4, 6-3), each with its count:
//! the C form of the `VcTable` of one Spacecraft ID, built and checked on every call. Each
//! array is a C array of the struct (`abi::configs`). A physical channel that carries several master channels is decoded by
//! reading the Spacecraft ID first with [`aldis_tc_header_decode`].
//!
//! **Security layouts** (API-070, c-abi.md §11). `TcFrame::decode` always takes the layouts of
//! the frame's Virtual Channel or MAP from its channels; there is no decoder without them, so
//! the TC C ABI has no `_with` pair: [`aldis_tc_decode`], [`aldis_tc_stream_next`], and the
//! encoder take a master channel and use its layouts.
//!
//! **Encoders.** A frame is built in one call, with no state between calls (API-023, API-060):
//! [`aldis_tc_encode`] takes the frame's data field and writes the header and the FECF. The C
//! ABI has no two-phase writer (OI-57).

use aldis::core::field;
use aldis::core::ids::FrameSeqNum;
use aldis::core::layout::FECF_LEN;
use aldis::core::security::SecurityLayout;
use aldis::core::{AnomalySet, DecodeMode, Error, ErrorKind, Lenient, Result};
use aldis::tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, FrameData, FrameFields,
    FrameStream, FrameType, IntakeItem, LenientFrameStream, MapConfig, PRIMARY_HEADER_LEN,
    PhysicalChannelConfig, SegmentHeader, SegmentHeaderUse, SequenceFlags, TcAnomaly, TcFrame,
    TcFrameHeader, TcMapId, TcSpacecraftId, TcVcId, VcConfig, VcTable, encode,
};

use crate::abi::{self, Configs, invalid_argument};
use crate::capacities::ALDIS_TC_MAX_MAPS;
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// Length of the TC Transfer Frame Primary Header in octets (232.0 §4.1.2).
pub const ALDIS_TC_PRIMARY_HEADER_LEN: u32 = 5;
/// Length of the Segment Header in octets (232.0 §4.1.3.2.2).
pub const ALDIS_TC_SEGMENT_HEADER_LEN: u32 = 1;
/// Shortest TC Transfer Frame in octets (232.0 §4.1.1, §4.1.2.7).
pub const ALDIS_TC_MIN_FRAME_LEN: u32 = 6;
/// Longest TC Transfer Frame in octets (232.0 §4.1.2.7; TC-007).
pub const ALDIS_TC_MAX_FRAME_LEN: u32 = 1024;
/// Virtual channels a master channel can have: VCIDs 0 to 63 (232.0 §4.1.2; M2 plan D14).
pub const ALDIS_TC_MAX_VCS: usize = 64;

/// The TC frame type (232.0 §4.1.2.3).
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
pub enum aldis_tc_frame {
    /// Frame type AD: Bypass Flag `0`, Control Command Flag `0` (232.0 §4.1.2, Table 4-1).
    ALDIS_TC_FRAME_AD = 0,
    /// Frame type BD: Bypass Flag `1`, Control Command Flag `0` (232.0 Table 4-1).
    ALDIS_TC_FRAME_BD = 1,
    /// Frame type BC: Bypass Flag `1`, Control Command Flag `1` (232.0 Table 4-1).
    ALDIS_TC_FRAME_BC = 2,
}
mirror!(pub aldis_tc_frame: u8 { ALDIS_TC_FRAME_AD, ALDIS_TC_FRAME_BD, ALDIS_TC_FRAME_BC });

/// The TC Sequence Flags (232.0 §4.1.4.2.2).
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
pub enum aldis_tc_sequence_flags {
    /// Sequence Flags `00`: a continuing segment (232.0 §4.1.3.2.2).
    ALDIS_TC_SEQ_CONTINUING = 0b00,
    /// Sequence Flags `01`: the first segment (232.0 §4.1.3.2.2).
    ALDIS_TC_SEQ_FIRST = 0b01,
    /// Sequence Flags `10`: the last segment (232.0 §4.1.3.2.2).
    ALDIS_TC_SEQ_LAST = 0b10,
    /// Sequence Flags `11`: no segmentation (232.0 §4.1.3.2.2).
    ALDIS_TC_SEQ_UNSEGMENTED = 0b11,
}
mirror!(pub aldis_tc_sequence_flags: u8 { ALDIS_TC_SEQ_CONTINUING, ALDIS_TC_SEQ_FIRST, ALDIS_TC_SEQ_LAST, ALDIS_TC_SEQ_UNSEGMENTED });

/// The Control Command of a BC frame (232.1 §4.1.3).
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
pub enum aldis_tc_control {
    /// No Control Command: the data field of a Type-D frame, or, in lenient mode, a Type-C data
    /// field that is neither command (232.0 §4.1.3.3.4; TC-005).
    ALDIS_TC_CONTROL_NONE = 0,
    /// The Unlock Control Command, one octet of zeros (232.0 §4.1.3.3.2; TC-005).
    ALDIS_TC_CONTROL_UNLOCK = 1,
    /// The Set V(R) Control Command, `82 00` and the new V(R) (232.0 §4.1.3.3.3; TC-005).
    ALDIS_TC_CONTROL_SET_VR = 2,
}
mirror!(pub aldis_tc_control: u8 { ALDIS_TC_CONTROL_NONE, ALDIS_TC_CONTROL_UNLOCK, ALDIS_TC_CONTROL_SET_VR });

/// Anomaly bit: Reserved Spare of the Primary Header not `00` (bit 0 of `TcAnomaly`; 232.0
/// §4.1.2.4; TC-003, API-032).
pub const ALDIS_TC_ANOMALY_RESERVED_SPARE_SET: u32 = 1 << 0;
/// Anomaly bit: a Type-C data field that is neither Unlock nor Set V(R) (bit 1; 232.0
/// §4.1.3.3.4; TC-005, API-032).
pub const ALDIS_TC_ANOMALY_UNKNOWN_CONTROL_COMMAND: u32 = 1 << 1;
/// Anomaly bit: a Reserved Spare of the CLCW not zero (bit 2; 232.0 §4.2.1.7, §4.2.1.10;
/// TC-030, API-032).
pub const ALDIS_TC_ANOMALY_CLCW_SPARE_SET: u32 = 1 << 2;

/// The result of one step of a TC frame stream.
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
pub enum aldis_tc_stream {
    /// Stream item: no octets of the unit remain (232.0 §4.4.9.2; TC-020).
    ALDIS_TC_STREAM_END = 0,
    /// Stream item: one validated frame (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
    ALDIS_TC_STREAM_ACCEPTED = 1,
    /// Stream item: discarded octets (fill, truncated frame, or failed validation) (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
    ALDIS_TC_STREAM_DISCARDED = 2,
}
mirror!(pub aldis_tc_stream: u8 { ALDIS_TC_STREAM_END, ALDIS_TC_STREAM_ACCEPTED, ALDIS_TC_STREAM_DISCARDED });

/// Why a TC frame was discarded.
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
pub enum aldis_tc_discard {
    /// Discard reason: accepted frame (not discarded).
    ALDIS_TC_DISCARD_NONE = 0,
    /// Discard reason: check (b), Spacecraft ID is not that of the master channel (232.0 §4.4.9.3.3; TC-021).
    ALDIS_TC_DISCARD_UNEXPECTED_SPACECRAFT = 2,
    /// Discard reason: check (c), Bypass Flag `0` with Control Command Flag `1` (232.0 Table 4-1; TC-002, TC-021).
    ALDIS_TC_DISCARD_RESERVED_FLAGS = 3,
    /// Discard reason: check (c), strict mode only: Reserved Spare is not `00` (232.0 §4.1.2.4; TC-003).
    ALDIS_TC_DISCARD_RESERVED_SPARE = 4,
    /// Discard reason: check (c), VCID is not configured (232.0 §4.4.7.3; TC-022).
    ALDIS_TC_DISCARD_INVALID_VCID = 5,
    /// Discard reason: check (c), Frame Length exceeds the channel's maximum (232.0 Tables 5-1, 5-3; TC-021).
    ALDIS_TC_DISCARD_FRAME_TOO_LONG = 6,
    /// Discard reason: check (c), Frame Length leaves no data octet (232.0 §4.1.1; TC-021, CMN-041).
    ALDIS_TC_DISCARD_FRAME_TOO_SHORT = 7,
    /// Discard reason: check (d), Frame Length disagrees with the octets present (232.0 §4.4.9.3.3; TC-021).
    ALDIS_TC_DISCARD_LENGTH_MISMATCH = 8,
    /// Discard reason: check (e), FECF does not match (232.0 §4.1.4; TC-006, TC-021).
    ALDIS_TC_DISCARD_FECF_MISMATCH = 9,
    /// Discard reason: MAP ID is not configured on the Virtual Channel (232.0 §4.4.3.3; TC-022).
    ALDIS_TC_DISCARD_INVALID_MAP_ID = 10,
    /// Discard reason: strict mode only, Type-C data field is not a recognized command (232.0 §4.1.3.3.4; TC-005).
    ALDIS_TC_DISCARD_RESERVED_CONTROL_COMMAND = 11,
    /// Discard reason: fill data (232.0 §4.4.9.2.6; TC-020).
    ALDIS_TC_DISCARD_FILL = 20,
    /// Discard reason: truncated frame whose declared length exceeds remaining octets (232.0 §4.4.9.2.5; TC-020).
    ALDIS_TC_DISCARD_TRUNCATED_FRAME = 21,
    /// Discard reason: leftover fill, more octets than the longest fill the coding sublayer appends
    /// that start no valid header, perhaps after a Frame Length error (232.0 §4.4.9.2.6 note 2; TC-020, OI-47).
    ALDIS_TC_DISCARD_LEFTOVER_FILL = 22,
}
mirror!(pub aldis_tc_discard: u32 { ALDIS_TC_DISCARD_NONE, ALDIS_TC_DISCARD_UNEXPECTED_SPACECRAFT, ALDIS_TC_DISCARD_RESERVED_FLAGS, ALDIS_TC_DISCARD_RESERVED_SPARE, ALDIS_TC_DISCARD_INVALID_VCID, ALDIS_TC_DISCARD_FRAME_TOO_LONG, ALDIS_TC_DISCARD_FRAME_TOO_SHORT, ALDIS_TC_DISCARD_LENGTH_MISMATCH, ALDIS_TC_DISCARD_FECF_MISMATCH, ALDIS_TC_DISCARD_INVALID_MAP_ID, ALDIS_TC_DISCARD_RESERVED_CONTROL_COMMAND, ALDIS_TC_DISCARD_FILL, ALDIS_TC_DISCARD_TRUNCATED_FRAME, ALDIS_TC_DISCARD_LEFTOVER_FILL });
/// Discard reason: check (a), Transfer Frame Version Number is not `00` (232.0 §4.4.9.3.3; TC-021).
pub const ALDIS_TC_DISCARD_UNEXPECTED_VERSION: u32 = 1;

/// COP in Effect `01`: COP-1 (232.0 §4.2.1.5).
pub const ALDIS_CLCW_COP_1: u8 = 0b01;

/// Managed parameters of a physical channel (232.0 §5.1 Table 5-1; GEN-060).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_config_t {
    /// 1 if every frame ends with a Frame Error Control Field, else 0 (232.0 §4.1.4.1).
    pub fecf: u8,
    /// Maximum Transfer Frame Length in octets: at most `ALDIS_TC_MAX_FRAME_LEN`, and at least
    /// `ALDIS_TC_MIN_FRAME_LEN`, two more with the FECF (232.0 Table 5-1; TC-007).
    pub max_frame_len: usize,
}

/// Managed parameters of a master channel (232.0 §5.2 Table 5-2; GEN-060).
///
/// Its virtual channels and MAPs are passed beside it, each as an array and its count (M2 plan
/// D14).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_mc_config_t {
    /// Spacecraft ID, 0 to 1023 (232.0 §4.1.2, Table 5-2).
    pub scid: u16,
    /// The physical channel.
    pub pc: aldis_tc_config_t,
}

/// Managed parameters of a virtual channel (232.0 §5.3 Table 5-3, §6.6.1 Table 6-2; TC-004,
/// API-070).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_vc_config_t {
    /// Virtual Channel Identifier, 0 to 63 (232.0 §4.1.2).
    pub vcid: u8,
    /// 1 if the VC's Type-D frames carry a Segment Header, else 0 (232.0 §4.1.3.2.2.1.2;
    /// TC-004). With a Segment Header the Security Header and Trailer lengths are set per MAP
    /// and both lengths here must be 0.
    pub segment_header: u8,
    /// Security Header length in octets, at most 64; 0 without SDLS (Table 6-2; API-070).
    pub security_header_len: u8,
    /// Security Trailer length in octets; 0 without SDLS (Table 6-2; API-070).
    pub security_trailer_len: u16,
    /// Maximum Transfer Frame Length of the VC in octets, `ALDIS_TC_MIN_FRAME_LEN` to
    /// `ALDIS_TC_MAX_FRAME_LEN` (Table 5-3; TC-007).
    pub max_frame_len: usize,
}

/// Managed parameters of a MAP channel (232.0 §5.4 Table 5-4, §6.6.2 Table 6-3; TC-022,
/// API-070).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_map_config_t {
    /// The VCID of its virtual channel, which must have Segment Headers (232.0
    /// §4.1.3.2.2.1.2).
    pub vcid: u8,
    /// MAP Identifier, 0 to 63 (232.0 §4.1.3.2.2).
    pub map_id: u8,
    /// Security Header length in octets, at most 64; 0 without SDLS (Table 6-3; API-070).
    pub security_header_len: u8,
    /// Security Trailer length in octets; 0 without SDLS (Table 6-3; API-070).
    pub security_trailer_len: u16,
}

/// The header fields a sender chooses (232.0 §4.1.2, §4.1.3.2.2; TC-004, TC-008). The Frame
/// Length and the Reserved Spare are written by the library (API-021).
///
/// With `segment_header` 0, `sequence_flags` and `map_id` are ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_fields_t {
    /// Frame type, `ALDIS_TC_FRAME_*` (232.0 Table 4-1).
    pub frame_type: u8,
    /// Spacecraft ID, which must be the master channel's (232.0 §4.1.2).
    pub scid: u16,
    /// Virtual Channel Identifier, which must be configured (232.0 §4.1.2).
    pub vcid: u8,
    /// Frame Sequence Number N(S), as given also on Type-B frames (232.0 §4.1.2; TC-008).
    pub sequence_number: u8,
    /// 1 for a Segment Header, which a Type-D frame of a VC with Segment Headers must have,
    /// else 0 (232.0 §4.1.3.2.2.1.2; TC-004).
    pub segment_header: u8,
    /// Sequence Flags, `ALDIS_TC_SEQ_*` (232.0 §4.1.3.2.2).
    pub sequence_flags: u8,
    /// MAP Identifier, 0 to 63, which must be configured (232.0 §4.1.3.2.2; TC-022).
    pub map_id: u8,
}

/// A Control Command (232.0 §4.1.3.3; TC-005).
///
/// `set_vr_value` is read only with `ALDIS_TC_CONTROL_SET_VR`, and written as 0 otherwise.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_control_t {
    /// `ALDIS_TC_CONTROL_*`.
    pub command: u8,
    /// The new V(R) of a Set V(R) command (232.0 §4.1.3.3.3).
    pub set_vr_value: u8,
}

/// The data field of a frame for [`aldis_tc_encode`] (232.0 §4.1.3; TC-004, TC-005, API-072,
/// CABI-011; c-abi.md §11).
///
/// With `control.command` `ALDIS_TC_CONTROL_NONE` the frame is Type-D and carries
/// `user_data`, at least one octet; otherwise it is Type-C and carries the command, and
/// `user_data` must be empty. The Security Header and Trailer are written as zeros for the SDLS
/// layer to fill (API-072). The struct is read once per call and no pointer is kept (API-060).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_tc_content_t {
    /// The User Data of a Type-D frame (232.0 §4.1.3.2); may be `NULL` with length 0.
    pub user_data: *const u8,
    /// Length of `user_data`.
    pub user_data_len: usize,
    /// The Control Command of a Type-C frame, or `ALDIS_TC_CONTROL_NONE` (232.0 §4.1.3.3).
    pub control: aldis_tc_control_t,
}

impl Default for aldis_tc_content_t {
    /// No User Data and no Control Command.
    fn default() -> aldis_tc_content_t {
        aldis_tc_content_t {
            user_data: core::ptr::null(),
            user_data_len: 0,
            control: aldis_tc_control_t::default(),
        }
    }
}

/// A TC Transfer Frame Primary Header as plain values (232.0 §4.1.2; TC-001, TC-002, API-014,
/// GEN-053).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_header_t {
    /// Frame type, `ALDIS_TC_FRAME_*`, from the Bypass and Control Command Flags (232.0 Table
    /// 4-1).
    pub frame_type: u8,
    /// Spacecraft ID (232.0 §4.1.2).
    pub scid: u16,
    /// Virtual Channel Identifier (232.0 §4.1.2).
    pub vcid: u8,
    /// Frame Sequence Number N(S) (232.0 §4.1.2).
    pub sequence_number: u8,
    /// Total frame length in octets: the Frame Length field plus one (232.0 §4.1.2.7;
    /// GEN-053).
    pub frame_len: usize,
}

/// Where each field of a frame lies (232.0 §4.1, §6.3; API-072, CABI-012).
///
/// Offsets and lengths are in octets from the start of the frame. An absent field has length 0
/// at the offset where it would start.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_layout_t {
    /// The octets the frame takes (232.0 §4.1.2.7; API-012).
    pub frame_len: usize,
    /// The range of the Segment Header; its length is `ALDIS_TC_SEGMENT_HEADER_LEN` or 0 (232.0 §4.1.3.2.2).
    pub segment_header: aldis_range_t,
    /// The range of the Security Header; length 0 without SDLS (232.0 §6.3.4).
    pub security_header: aldis_range_t,
    /// The range of the data field: the User Data or the Control Command.
    pub data: aldis_range_t,
    /// The range of the Security Trailer; length 0 without SDLS (232.0 §6.3.6).
    pub security_trailer: aldis_range_t,
    /// The range of the Frame Error Control Field; its length is 2 or 0 (232.0 §4.1.4).
    pub fecf: aldis_range_t,
}

/// A decoded TC Transfer Frame: its fields and where they lie in the input (232.0 §4.1, §6.3;
/// TC-001…TC-006, API-010, CABI-012).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tc_frame_t {
    /// The Primary Header (232.0 §4.1.2; API-014).
    pub header: aldis_tc_header_t,
    /// Whether the frame has a Segment Header (232.0 §4.1.3.2.2; TC-004).
    pub has_segment_header: bool,
    /// With `has_segment_header`, the Sequence Flags, `ALDIS_TC_SEQ_*`; else 0.
    pub sequence_flags: u8,
    /// With `has_segment_header`, the MAP Identifier; else 0.
    pub map_id: u8,
    /// The Control Command of a Type-C frame; `ALDIS_TC_CONTROL_NONE` for a Type-D frame, and
    /// for a reserved pattern in lenient mode (232.0 §4.1.3.3; TC-005).
    pub control: aldis_tc_control_t,
    /// Whether the frame has an FECF (232.0 §4.1.4; TC-006).
    pub has_fecf: bool,
    /// The FECF, when `has_fecf`.
    pub fecf: u16,
    /// Lenient-mode anomaly flags, `ALDIS_TC_ANOMALY_*`; 0 in strict mode (API-032).
    pub anomalies: u32,
    /// Where each field lies in the input.
    pub layout: aldis_tc_layout_t,
}

/// One item yielded by [`aldis_tc_stream_next`] (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_tc_stream_item_t {
    /// Item kind: [`ALDIS_TC_STREAM_END`], [`ALDIS_TC_STREAM_ACCEPTED`], or [`ALDIS_TC_STREAM_DISCARDED`].
    pub kind: u8,
    /// Reserved padding octets, set to zero.
    pub reserved: [u8; 3],
    /// The range of this item in the caller's unit buffer: where it starts and the octets it
    /// consumes.
    pub range: aldis_range_t,
    /// The frame length declared in the header (valid when accepted or truncated).
    pub declared_len: usize,
    /// The decoded frame view (valid when `kind == ALDIS_TC_STREAM_ACCEPTED`).
    pub frame: aldis_tc_frame_t,
    /// Discard reason code: `ALDIS_TC_DISCARD_*` (valid when `kind == ALDIS_TC_STREAM_DISCARDED`).
    pub discard_reason: u32,
    /// The error code of the discard (e.g. `ALDIS_ERR_CRC_MISMATCH`).
    pub error_code: u32,
    /// The field code of the error (0 if none).
    pub error_field: u16,
    /// Reserved padding octets, set to zero.
    pub reserved2: [u8; 2],
}

impl Default for aldis_tc_stream_item_t {
    fn default() -> Self {
        Self {
            kind: ALDIS_TC_STREAM_END,
            reserved: [0; 3],
            range: aldis_range_t { offset: 0, len: 0 },
            declared_len: 0,
            frame: aldis_tc_frame_t::default(),
            discard_reason: 0,
            error_code: 0,
            error_field: 0,
            reserved2: [0; 2],
        }
    }
}

/// A Communications Link Control Word as plain values (232.0 §4.2.1; TC-030, TC-031, M2 plan
/// D14).
///
/// The struct is an input of [`aldis_clcw_encode`] and an output of [`aldis_clcw_decode`], so
/// its flags are `uint8_t`, 0 or 1, as an input struct's are (c-abi.md §10). The Control Word
/// Type, the CLCW Version Number, and the Reserved Spares are not held: the encoder writes them
/// as zeros.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_clcw_t {
    /// Status Field, 0 to 7, mission-specific and passed through (232.0 §4.2.1.4; TC-031).
    pub status: u8,
    /// COP in Effect, 0 to 3; `ALDIS_CLCW_COP_1` is COP-1 (232.0 §4.2.1.5).
    pub cop_in_effect: u8,
    /// Virtual Channel Identification, 0 to 63 (232.0 §4.2.1.6).
    pub vcid: u8,
    /// No RF Available Flag (232.0 §4.2.1.8.2).
    pub no_rf_available: u8,
    /// No Bit Lock Flag (232.0 §4.2.1.8.3).
    pub no_bit_lock: u8,
    /// Lockout Flag (232.0 §4.2.1.8.4).
    pub lockout: u8,
    /// Wait Flag (232.0 §4.2.1.8.5).
    pub wait: u8,
    /// Retransmit Flag (232.0 §4.2.1.8.6).
    pub retransmit: u8,
    /// FARM-B Counter, 0 to 3 (232.0 §4.2.1.9).
    pub farm_b_counter: u8,
    /// Report Value N(R) (232.0 §4.2.1.11).
    pub report_value: u8,
}

/// The channels of one master channel (232.0 §5; M2 plan D14): every VCID of one Spacecraft ID,
/// and up to `ALDIS_TC_MAX_MAPS` MAPs (CABI-030).
type Table = VcTable<ALDIS_TC_MAX_VCS, ALDIS_TC_MAX_MAPS>;

/// The physical channel of a C struct (232.0 §5.1).
fn physical(c: &aldis_tc_config_t) -> Result<PhysicalChannelConfig> {
    PhysicalChannelConfig::new(c.max_frame_len, abi::flag(c.fecf)?)
}

/// A security layout of a C struct (Tables 6-2, 6-3; API-070).
fn security(header_len: u8, trailer_len: u16) -> Result<SecurityLayout> {
    SecurityLayout::new(usize::from(header_len), usize::from(trailer_len))
}

/// The virtual channel of a C struct on `scid`, each parameter parsed into its type (Table
/// 5-3, Table 6-2; RS-020).
fn virtual_channel(scid: TcSpacecraftId, v: &aldis_tc_vc_config_t) -> Result<VcConfig> {
    let vcid = TcVcId::try_from(v.vcid)?;
    let segment_header = if abi::flag(v.segment_header)? {
        if v.security_header_len != 0 || v.security_trailer_len != 0 {
            // A VC with Segment Headers has its SDLS lengths per MAP (Table 6-3).
            return Err(invalid_argument());
        }
        SegmentHeaderUse::Present
    } else {
        SegmentHeaderUse::Absent(security(v.security_header_len, v.security_trailer_len)?)
    };
    VcConfig::new(scid, vcid, v.max_frame_len, segment_header)
}

/// The MAP of a C struct on `scid` (Table 5-4, Table 6-3; RS-020).
fn map_channel(scid: TcSpacecraftId, m: aldis_tc_map_config_t) -> Result<MapConfig> {
    let vcid = TcVcId::try_from(m.vcid)?;
    let map_id = TcMapId::try_from(m.map_id)?;
    let security = security(m.security_header_len, m.security_trailer_len)?;
    Ok(MapConfig::new(scid, vcid, map_id, security))
}

/// Up to `N` parsed configurations, in array order.
struct Parsed<T, const N: usize> {
    items: [T; N],
    len: usize,
}

/// The configurations of `configs`, each parsed by `parse`, or `full` for more than `N`
/// (GEN-011, GEN-022: the loop is bounded by `N`).
fn parse_all<C: Copy, T: Copy, const N: usize>(
    configs: &Configs<'_, C>,
    full: Error,
    parse: impl Fn(&C) -> Result<T>,
) -> Result<Option<Parsed<T, N>>> {
    if configs.len() > N {
        return Err(full);
    }
    if configs.len() == 0 {
        return Ok(None);
    }
    let first = parse(&configs.get(0)?)?;
    let mut items = [first; N];
    for (i, slot) in items.iter_mut().enumerate().take(configs.len()).skip(1) {
        *slot = parse(&configs.get(i)?)?;
    }
    Ok(Some(Parsed {
        items,
        len: configs.len(),
    }))
}

/// The parsed configurations as a slice; empty for none.
fn slice<T, const N: usize>(parsed: Option<&Parsed<T, N>>) -> &[T] {
    parsed.map_or(&[], |p| p.items.get(..p.len).unwrap_or_default())
}

/// The master channel of C structs: its physical channel and Spacecraft ID, then each virtual
/// channel and each MAP in array order, then the checks of `VcTable::new` (232.0 §5; GEN-060,
/// API-040, API-070).
fn master_channel(
    mc: &aldis_tc_mc_config_t,
    vcs: &Configs<'_, aldis_tc_vc_config_t>,
    maps: &Configs<'_, aldis_tc_map_config_t>,
) -> Result<Table> {
    let pc = physical(&mc.pc)?;
    let scid = TcSpacecraftId::try_from(mc.scid)?;
    let vc_full = Error::new(ErrorKind::CapacityExceeded).with_field(field::VC_ID);
    let vcs = parse_all::<_, _, ALDIS_TC_MAX_VCS>(vcs, vc_full, |v| virtual_channel(scid, v))?;
    let map_full = Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID);
    let maps = parse_all::<_, _, ALDIS_TC_MAX_MAPS>(maps, map_full, |m| map_channel(scid, *m))?;
    Table::new(pc, slice(vcs.as_ref()), slice(maps.as_ref()))
}

/// The master channel at `mc` with the `vc_count` virtual channels at `vcs` and the
/// `map_count` MAPs at `maps` (CABI-022).
///
/// # Safety
/// As for `abi::read` and `abi::configs`.
#[expect(unsafe_code, reason = "checked access to caller memory (CABI-022)")]
unsafe fn master_channel_at(
    mc: *const aldis_tc_mc_config_t,
    vcs: *const aldis_tc_vc_config_t,
    vc_count: usize,
    maps: *const aldis_tc_map_config_t,
    map_count: usize,
) -> Result<Table> {
    // SAFETY: forwarded from the caller.
    let (mc, vcs, maps) = unsafe {
        (
            abi::read(mc)?,
            abi::configs(vcs, vc_count)?,
            abi::configs(maps, map_count)?,
        )
    };
    master_channel(&mc, &vcs, &maps)
}

/// The sender's fields of a C struct, each parsed into its type (232.0 §4.1.2, §4.1.3.2.2;
/// RS-020).
fn fields(f: aldis_tc_fields_t) -> Result<(FrameFields, Option<SegmentHeader>)> {
    let frame_type = match parse::<aldis_tc_frame>(f.frame_type)? {
        aldis_tc_frame::ALDIS_TC_FRAME_AD => FrameType::Ad,
        aldis_tc_frame::ALDIS_TC_FRAME_BD => FrameType::Bd,
        aldis_tc_frame::ALDIS_TC_FRAME_BC => FrameType::Bc,
    };
    let fields = FrameFields {
        frame_type,
        spacecraft_id: TcSpacecraftId::try_from(f.scid)?,
        vc_id: TcVcId::try_from(f.vcid)?,
        sequence_number: FrameSeqNum::new(f.sequence_number),
    };
    let segment_header = if abi::flag(f.segment_header)? {
        if f.sequence_flags > ALDIS_TC_SEQ_UNSEGMENTED {
            return Err(invalid_argument());
        }
        Some(SegmentHeader {
            sequence_flags: SequenceFlags::from_bits(f.sequence_flags),
            map_id: TcMapId::try_from(f.map_id)?,
        })
    } else {
        None
    };
    Ok((fields, segment_header))
}

/// The Control Command of a C struct, or `None` for `ALDIS_TC_CONTROL_NONE` (TC-005).
fn command(c: aldis_tc_control_t) -> Result<Option<ControlCommand>> {
    match parse::<aldis_tc_control>(c.command)? {
        aldis_tc_control::ALDIS_TC_CONTROL_NONE => Ok(None),
        aldis_tc_control::ALDIS_TC_CONTROL_UNLOCK => Ok(Some(ControlCommand::Unlock)),
        aldis_tc_control::ALDIS_TC_CONTROL_SET_VR => Ok(Some(ControlCommand::SetVr(
            FrameSeqNum::new(c.set_vr_value),
        ))),
    }
}

/// The C form of a Control Command.
const fn control(c: Option<ControlCommand>) -> aldis_tc_control_t {
    match c {
        None => aldis_tc_control_t {
            command: ALDIS_TC_CONTROL_NONE,
            set_vr_value: 0,
        },
        Some(ControlCommand::Unlock) => aldis_tc_control_t {
            command: ALDIS_TC_CONTROL_UNLOCK,
            set_vr_value: 0,
        },
        Some(ControlCommand::SetVr(vr)) => aldis_tc_control_t {
            command: ALDIS_TC_CONTROL_SET_VR,
            set_vr_value: vr.get(),
        },
    }
}

/// The C form of a frame type.
const fn frame_type(t: FrameType) -> u8 {
    match t {
        FrameType::Ad => ALDIS_TC_FRAME_AD,
        FrameType::Bd => ALDIS_TC_FRAME_BD,
        FrameType::Bc => ALDIS_TC_FRAME_BC,
    }
}

/// The C form of a Primary Header.
fn header(h: &TcFrameHeader) -> aldis_tc_header_t {
    aldis_tc_header_t {
        frame_type: frame_type(h.frame_type()),
        scid: h.spacecraft_id().get(),
        vcid: h.vc_id().get(),
        sequence_number: h.sequence_number().get(),
        frame_len: h.frame_len(),
    }
}

/// Where the fields of a frame lie, in the order of 232.0 §4.1 and §6.3: the Primary Header, the
/// Segment Header, the Security Header, the data field, the Security Trailer, and the FECF.
fn layout(
    segment_header: bool,
    security_header_len: usize,
    data_len: usize,
    security_trailer_len: usize,
    fecf: bool,
) -> aldis_tc_layout_t {
    let segment_len = usize::from(segment_header);
    let security_header_offset = PRIMARY_HEADER_LEN.saturating_add(segment_len);
    let data_offset = security_header_offset.saturating_add(security_header_len);
    let security_trailer_offset = data_offset.saturating_add(data_len);
    let fecf_offset = security_trailer_offset.saturating_add(security_trailer_len);
    let fecf_len = if fecf { FECF_LEN } else { 0 };
    aldis_tc_layout_t {
        frame_len: fecf_offset.saturating_add(fecf_len),
        segment_header: aldis_range_t {
            offset: PRIMARY_HEADER_LEN,
            len: segment_len,
        },
        security_header: aldis_range_t {
            offset: security_header_offset,
            len: security_header_len,
        },
        data: aldis_range_t {
            offset: data_offset,
            len: data_len,
        },
        security_trailer: aldis_range_t {
            offset: security_trailer_offset,
            len: security_trailer_len,
        },
        fecf: aldis_range_t {
            offset: fecf_offset,
            len: fecf_len,
        },
    }
}

/// The C view of a decoded frame.
pub(crate) fn view(f: &TcFrame<'_>, anomalies: AnomalySet<TcAnomaly>) -> aldis_tc_frame_t {
    let segment = f.segment_header();
    aldis_tc_frame_t {
        header: header(&f.header()),
        has_segment_header: segment.is_some(),
        sequence_flags: segment.map_or(0, |s| s.sequence_flags.bits()),
        map_id: segment.map_or(0, |s| s.map_id.get()),
        control: control(f.control_command()),
        has_fecf: f.fecf().is_some(),
        fecf: f.fecf().unwrap_or(0),
        anomalies: anomalies.bits(),
        layout: layout(
            segment.is_some(),
            f.security_header().len(),
            f.data_field().len(),
            f.security_trailer().len(),
            f.fecf().is_some(),
        ),
    }
}

/// The CLCW of a C struct, each field parsed into its type (232.0 §4.2.1; RS-020).
fn clcw(c: &aldis_clcw_t) -> Result<Clcw> {
    Ok(Clcw {
        status: ClcwStatus::try_from(c.status)?,
        cop_in_effect: CopInEffect::try_from(c.cop_in_effect)?,
        vc_id: TcVcId::try_from(c.vcid)?,
        no_rf_available: abi::flag(c.no_rf_available)?,
        no_bit_lock: abi::flag(c.no_bit_lock)?,
        lockout: abi::flag(c.lockout)?,
        wait: abi::flag(c.wait)?,
        retransmit: abi::flag(c.retransmit)?,
        farm_b_counter: FarmBCounter::try_from(c.farm_b_counter)?,
        report_value: FrameSeqNum::new(c.report_value),
    })
}

/// The C form of a CLCW.
fn clcw_struct(c: &Clcw) -> aldis_clcw_t {
    aldis_clcw_t {
        status: c.status.get(),
        cop_in_effect: c.cop_in_effect.get(),
        vcid: c.vc_id.get(),
        no_rf_available: u8::from(c.no_rf_available),
        no_bit_lock: u8::from(c.no_bit_lock),
        lockout: u8::from(c.lockout),
        wait: u8::from(c.wait),
        retransmit: u8::from(c.retransmit),
        farm_b_counter: c.farm_b_counter.get(),
        report_value: c.report_value.get(),
    }
}

/// Checks a physical channel configuration (232.0 §5.1 Table 5-1; GEN-060, API-040, TC-007).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`max_frame_len`) above 1024 octets, or below six, eight with the
/// FECF; CABI-022 violations.
///
/// # Safety
/// `pc` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_config_check(
    pc: *const aldis_tc_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || physical(&abi::read(pc)?).map(drop)) }
}

/// Checks a master channel configuration with its virtual channels and MAPs (232.0 §5 Tables
/// 5-1 to 5-4, §6.6 Tables 6-2, 6-3; GEN-060, API-040, API-070, CMN-041).
///
/// The physical channel is checked first, then the Spacecraft ID, each virtual channel in array
/// order (its VCID, Segment Header flag, security layout, and maximum frame length), each MAP
/// in array order (its VCID, MAP ID, and security layout), and then the channels together, as
/// `VcTable::new` checks them.
///
/// # Errors
/// The errors of [`aldis_tc_config_check`]; `ALDIS_ERR_OUT_OF_RANGE` (`spacecraft_id`, `vc_id`,
/// `map_id`); `ALDIS_ERR_CAPACITY_EXCEEDED` (`vc_id`) for more than `ALDIS_TC_MAX_VCS` virtual
/// channels and (`map_id`) for more than `ALDIS_TC_MAX_MAPS` MAPs, before any of them is read
/// (CABI-030); `ALDIS_ERR_INVALID_CONFIG` (`security_header_len`) for a Security Header above 64
/// octets, (`max_frame_len`) for a VC maximum outside 6 to 1024 octets, (`vc_id`) for
/// `vc_count` 0 (OI-55) or a VCID given twice, (`map_id`) for a MAP given twice or on a VC that is not given or has no Segment
/// Header, and (`frame_data_field`) for a VC or MAP whose fields leave no data octet; CABI-022
/// violations, including a VC
/// with Segment Headers whose security lengths are not 0.
///
/// # Safety
/// `mc` is valid for a read; `vcs` and `maps` are valid for reads of `vc_count` and
/// `map_count` structs (or `NULL` with 0); `err` is `NULL`
/// or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_mc_config_check(
    mc: *const aldis_tc_mc_config_t,
    vcs: *const aldis_tc_vc_config_t,
    vc_count: usize,
    maps: *const aldis_tc_map_config_t,
    map_count: usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            master_channel_at(mc, vcs, vc_count, maps, map_count).map(drop)
        })
    }
}

/// The length of the frame that `prefix` begins, from its Frame Length field (232.0
/// §4.1.2.7; TC-001, API-013, GEN-053, M2 plan D14).
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (`needed` 5) for a prefix shorter than the Primary Header;
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`) for a version other than `00`;
/// `ALDIS_ERR_RESERVED_VALUE` (`control_command_flag`) for Bypass Flag `0` with Control Command
/// Flag `1` (TC-002); CABI-022 violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `out_len` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_peek_length(
    prefix: *const u8,
    prefix_len: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out_len = abi::out(out_len)?;
            out_len.write(TcFrame::peek_length(prefix)?);
            Ok(())
        })
    }
}

/// Decodes the Primary Header that `prefix` begins (232.0 §4.1.2; TC-001, TC-002, API-014), as
/// `TcFrame::peek_header` does.
///
/// The rest of the frame is not read and no channel is looked up: for example to find the
/// Spacecraft ID of a frame on a physical channel that carries several master channels.
///
/// # Errors
/// As [`aldis_tc_peek_length`].
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `out` is valid for one write;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_header_decode(
    prefix: *const u8,
    prefix_len: usize,
    out: *mut aldis_tc_header_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out = abi::out(out)?;
            out.write(header(&TcFrame::peek_header(prefix)?));
            Ok(())
        })
    }
}

/// Decodes the frame that `bytes` begins, on the master channel `mc` (232.0 §4.1, §6.3;
/// TC-001…TC-006, API-010…API-014, API-070), as `TcFrame::decode` does.
///
/// The frame takes as many octets as its Frame Length declares (`out->layout.frame_len`); the
/// octets after it are not read. The Security Header and Trailer lengths are those of the
/// frame's virtual channel, or of its MAP when it has a Segment Header.
///
/// # Errors
/// The errors of [`aldis_tc_mc_config_check`]; then, in this order: `ALDIS_ERR_INCOMPLETE`
/// (`needed`) for fewer octets than the Primary Header or the frame; `ALDIS_ERR_UNSUPPORTED_VERSION`
/// (`frame_version`); `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id`) for another Spacecraft ID;
/// `ALDIS_ERR_RESERVED_VALUE` (`control_command_flag`) (TC-002) and, in strict mode,
/// (`tc_header_spare`) (TC-003); `ALDIS_ERR_UNKNOWN_CHANNEL` (`vc_id`); `ALDIS_ERR_OUT_OF_RANGE`
/// (`frame_len`) above the channel's maximum and `ALDIS_ERR_INVALID_FIELD` (`frame_data_field`)
/// for no data octet; `ALDIS_ERR_CRC_MISMATCH` (TC-006); `ALDIS_ERR_UNKNOWN_CHANNEL` (`map_id`);
/// in strict mode `ALDIS_ERR_RESERVED_VALUE` (`control_command`) for a Type-C data field that
/// is neither command (TC-005); CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `mc`, `vcs`, and `maps` as for
/// [`aldis_tc_mc_config_check`]; `out` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_decode(
    bytes: *const u8,
    len: usize,
    mc: *const aldis_tc_mc_config_t,
    vcs: *const aldis_tc_vc_config_t,
    vc_count: usize,
    maps: *const aldis_tc_map_config_t,
    map_count: usize,
    mode: u32,
    out: *mut aldis_tc_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let table = master_channel_at(mc, vcs, vc_count, maps, map_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let (frame, anomalies) = mode.dispatch(
                || TcFrame::decode(bytes, &table),
                || TcFrame::decode_lenient(bytes, &table),
            )?;
            out.write(view(&frame, anomalies));
            Ok(())
        })
    }
}

/// Steps through the clean data unit `unit` using the unified Frame Intake Stream
/// (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
///
/// Reads from `*offset` and advances `*offset` by the octets consumed for the yielded item.
/// When `out.kind == ALDIS_TC_STREAM_ACCEPTED`, `out.frame` holds the validated frame view.
/// When `out.kind == ALDIS_TC_STREAM_DISCARDED`, `out.discard_reason` holds the `ALDIS_TC_DISCARD_*`
/// code, and `out.error_code` holds the error status.
/// When `out.kind == ALDIS_TC_STREAM_END`, the unit is exhausted.
///
/// `max_fill` is the longest fill the coding sublayer appends (six octets for the BCH codeblock,
/// 232.0 §4.4.9.2.6 note 2): a rest that starts no valid header is discarded as
/// `ALDIS_TC_DISCARD_FILL` up to `max_fill` octets and as `ALDIS_TC_DISCARD_LEFTOVER_FILL` above
/// (OI-47). `max_fill == SIZE_MAX` means no bound, as before the argument existed; `0` reports every octet
/// of fill as leftover. The Fill item names the Frame Data Field in `error_field`, the leftover item the
/// Frame Length.
///
/// # Errors
/// The errors of [`aldis_tc_mc_config_check`]; `ALDIS_ERR_INVALID_ARGUMENT` if `*offset > unit_len`;
/// CABI-022 violations.
///
/// # Safety
/// `unit` is valid for `unit_len` octets (or `NULL` with 0); `offset` is valid for read and write;
/// `mc`, `vcs`, and `maps` as for [`aldis_tc_mc_config_check`]; `out` is valid for one write; `err`
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
pub unsafe extern "C" fn aldis_tc_stream_next(
    unit: *const u8,
    unit_len: usize,
    offset: *mut usize,
    mc: *const aldis_tc_mc_config_t,
    vcs: *const aldis_tc_vc_config_t,
    vc_count: usize,
    maps: *const aldis_tc_map_config_t,
    map_count: usize,
    max_fill: usize,
    mode: u32,
    out: *mut aldis_tc_stream_item_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and overlap.
    unsafe {
        abi::call(err, || {
            let table = master_channel_at(mc, vcs, vc_count, maps, map_count)?;
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
                out.write(aldis_tc_stream_item_t {
                    kind: ALDIS_TC_STREAM_END,
                    reserved: [0; 3],
                    range: aldis_range_t {
                        offset: current_offset,
                        len: 0,
                    },
                    declared_len: 0,
                    frame: aldis_tc_frame_t::default(),
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
                DecodeMode::Strict => FrameStream::new(rest, &table, max_fill)
                    .next()
                    .map(|item| item.map_frame(|frame| (frame, AnomalySet::empty()))),
                DecodeMode::Lenient => LenientFrameStream::new(rest, &table, max_fill)
                    .next()
                    .map(|item| item.map_frame(Lenient::into_parts)),
            };
            match item {
                None => {
                    out.write(aldis_tc_stream_item_t {
                        kind: ALDIS_TC_STREAM_END,
                        reserved: [0; 3],
                        range: aldis_range_t {
                            offset: current_offset,
                            len: 0,
                        },
                        declared_len: 0,
                        frame: aldis_tc_frame_t::default(),
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
                    out.write(aldis_tc_stream_item_t {
                        kind: ALDIS_TC_STREAM_ACCEPTED,
                        reserved: [0; 3],
                        range: aldis_range_t {
                            offset: current_offset,
                            len,
                        },
                        declared_len: len,
                        frame: view(&frame, anomalies),
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
                        aldis::tc::DiscardReason::TruncatedFrame { declared_len } => declared_len,
                        _ => 0,
                    };
                    out.write(aldis_tc_stream_item_t {
                        kind: ALDIS_TC_STREAM_DISCARDED,
                        reserved: [0; 3],
                        range: aldis_range_t {
                            offset: current_offset,
                            len,
                        },
                        declared_len,
                        frame: aldis_tc_frame_t::default(),
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

/// Encodes one frame into the start of `out` in one call (232.0 §4.1, §6.3; TC-004, TC-005,
/// TC-007, TC-008, API-020…API-022, API-072), as `aldis_tc::encode` does.
///
/// The library writes the Frame Length, the Reserved Spare `00`, zeros in the Security Header
/// and Trailer, and the FECF (API-021).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_tc_mc_config_check`]; then
/// `ALDIS_ERR_OUT_OF_RANGE` (`spacecraft_id`, `vc_id`, `map_id`) for a value of `fields`
/// outside its field; `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id`, `vc_id`, `map_id`) for a
/// channel the master channel does not have; `ALDIS_ERR_INVALID_FIELD` (`control_command_flag`)
/// for a frame type that does not match the content, (`segment_header`) for a Segment Header
/// where the VC has none or none where it has them, and (`frame_data_field`) for empty User
/// Data; `ALDIS_ERR_OUT_OF_RANGE` (`frame_len`) above the channel's maximum (TC-007);
/// `ALDIS_ERR_BUFFER_TOO_SMALL` (`needed` = frame length); CABI-022 violations, including User
/// Data or a Segment Header with a Control Command, and `out` overlapping `user_data`.
///
/// # Safety
/// `mc`, `vcs`, and `maps` as for [`aldis_tc_mc_config_check`]; `fields` and `content` are
/// valid for a read; `content->user_data` is valid for its length (or `NULL` with 0); `out` is
/// valid for `cap` octets; `out_len` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_encode(
    mc: *const aldis_tc_mc_config_t,
    vcs: *const aldis_tc_vc_config_t,
    vc_count: usize,
    maps: *const aldis_tc_map_config_t,
    map_count: usize,
    fields: *const aldis_tc_fields_t,
    content: *const aldis_tc_content_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let table = master_channel_at(mc, vcs, vc_count, maps, map_count)?;
            let (fields, segment_header) = self::fields(abi::read(fields)?)?;
            let c = abi::read(content)?;
            let user_data = abi::bytes(c.user_data, c.user_data_len)?;
            let data = match command(c.control)? {
                None => FrameData::Fdu {
                    segment_header,
                    user_data,
                },
                Some(_) if !user_data.is_empty() || segment_header.is_some() => {
                    return Err(invalid_argument());
                }
                Some(command) => FrameData::Control(command),
            };
            let out = abi::bytes_mut(out, cap, &[(c.user_data, c.user_data_len)])?;
            let out_len = abi::out(out_len)?;
            out_len.write(encode(&table, fields, data, out)?);
            Ok(())
        })
    }
}

/// Decodes the Control Command that the data field `octets` of a Type-C frame holds exactly
/// (232.0 §4.1.3.3; TC-005), as `ControlCommand::parse` does.
///
/// # Errors
/// `ALDIS_ERR_RESERVED_VALUE` (`control_command`) for any other pattern, including a known one
/// with octets missing or added, as a strict decoder reports it; CABI-022 violations.
///
/// # Safety
/// `octets` is valid for `len` octets (or `NULL` with 0); `out` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_control_decode(
    octets: *const u8,
    len: usize,
    out: *mut aldis_tc_control_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let octets = abi::bytes(octets, len)?;
            let out = abi::out(out)?;
            let command = ControlCommand::parse(octets)
                .ok_or(Error::new(ErrorKind::ReservedValue).with_field(field::CONTROL_COMMAND))?;
            out.write(control(Some(command)));
            Ok(())
        })
    }
}

/// Encodes a Control Command into the start of `out`, as a Type-C frame's data field carries it
/// (232.0 §4.1.3.3; TC-005, API-020): one octet for Unlock, three for Set V(R).
///
/// # Errors
/// On error `out` is unchanged. `ALDIS_ERR_BUFFER_TOO_SMALL` (`needed`); CABI-022 violations,
/// including `ALDIS_TC_CONTROL_NONE`.
///
/// # Safety
/// `command` is valid for a read; `out` is valid for `cap` octets; `out_len` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tc_control_encode(
    command: *const aldis_tc_control_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let command = self::command(abi::read(command)?)?.ok_or_else(invalid_argument)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            let octets = command.to_octets();
            let octets = octets.as_slice();
            out.get_mut(..octets.len())
                .ok_or(Error::buffer_too_small(octets.len()))?
                .copy_from_slice(octets);
            out_len.write(octets.len());
            Ok(())
        })
    }
}

/// Decodes a CLCW given as a 32-bit word, bit 0 of 232.0 §4.2.1 the most significant (232.0
/// §4.2.1; TC-030, TC-031, M2 plan D14), as `Clcw::decode` does with its four octets.
///
/// Any COP in Effect is accepted and exposed; the Status Field is passed through. A CLCW in an
/// Operational Control Field is its four octets read as a big-endian word.
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` (`control_word_type`) for a Control Word Type of `1` and
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`clcw_version`) for a version other than `00`, in both
/// modes; in strict mode `ALDIS_ERR_RESERVED_VALUE` (`clcw_spare_14_15`, `clcw_spare_23`) for a
/// Reserved Spare that is not zero, which lenient mode reports as
/// `ALDIS_TC_ANOMALY_CLCW_SPARE_SET` in `anomalies`; CABI-022 violations.
///
/// # Safety
/// `out` and `anomalies` are valid for one write each; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_clcw_decode(
    word: u32,
    mode: u32,
    out: *mut aldis_clcw_t,
    anomalies: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let anomalies = abi::out(anomalies)?;
            let (c, found) = mode.dispatch(
                || Clcw::decode(word.to_be_bytes()),
                || Clcw::decode_lenient(word.to_be_bytes()),
            )?;
            out.write(clcw_struct(&c));
            anomalies.write(found.bits());
            Ok(())
        })
    }
}

/// Encodes a CLCW as a 32-bit word, bit 0 of 232.0 §4.2.1 the most significant (232.0 §4.2.1;
/// TC-030, TC-031, GEN-052, M2 plan D14), as `Clcw::to_word` does.
///
/// The Control Word Type is `0`, the version `00`, and the Reserved Spares zero. The word's four
/// octets, most significant first, are the Operational Control Field that carries the CLCW
/// (CMN-011).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`clcw_status`, `cop_in_effect`, `vc_id`, `farm_b_counter`) for a
/// value outside its field; CABI-022 violations, including a flag other than 0 or 1.
///
/// # Safety
/// `clcw` is valid for a read; `out` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_clcw_encode(
    clcw: *const aldis_clcw_t,
    out: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let c = self::clcw(&abi::read(clcw)?)?;
            let out = abi::out(out)?;
            out.write(c.to_word());
            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::Anomaly as _;
    use aldis::tc::{MAX_FRAME_LEN, MIN_FRAME_LEN, Reason, SEGMENT_HEADER_LEN, TcAnomaly};

    #[test]
    fn constants_are_the_crate_values() {
        // req: GEN-043, API-033, CABI-010, TC-021
        assert_eq!(
            ALDIS_TC_ANOMALY_RESERVED_SPARE_SET,
            1 << TcAnomaly::ReservedSpareSet.bit()
        );
        assert_eq!(
            ALDIS_TC_ANOMALY_UNKNOWN_CONTROL_COMMAND,
            1 << TcAnomaly::UnknownControlCommand.bit()
        );
        assert_eq!(
            ALDIS_TC_ANOMALY_CLCW_SPARE_SET,
            1 << TcAnomaly::ClcwSpareSet.bit()
        );
        let lens = [
            ALDIS_TC_PRIMARY_HEADER_LEN,
            ALDIS_TC_SEGMENT_HEADER_LEN,
            ALDIS_TC_MIN_FRAME_LEN,
            ALDIS_TC_MAX_FRAME_LEN,
        ]
        .map(|n| usize::try_from(n).unwrap());
        assert_eq!(
            lens,
            [
                PRIMARY_HEADER_LEN,
                SEGMENT_HEADER_LEN,
                MIN_FRAME_LEN,
                MAX_FRAME_LEN
            ]
        );
        assert_eq!(usize::from(TcVcId::MAX) + 1, ALDIS_TC_MAX_VCS);
        let reasons = [
            ALDIS_TC_DISCARD_UNEXPECTED_VERSION,
            ALDIS_TC_DISCARD_UNEXPECTED_SPACECRAFT,
            ALDIS_TC_DISCARD_RESERVED_FLAGS,
            ALDIS_TC_DISCARD_RESERVED_SPARE,
            ALDIS_TC_DISCARD_INVALID_VCID,
            ALDIS_TC_DISCARD_FRAME_TOO_LONG,
            ALDIS_TC_DISCARD_FRAME_TOO_SHORT,
            ALDIS_TC_DISCARD_LENGTH_MISMATCH,
            ALDIS_TC_DISCARD_FECF_MISMATCH,
            ALDIS_TC_DISCARD_INVALID_MAP_ID,
            ALDIS_TC_DISCARD_RESERVED_CONTROL_COMMAND,
        ];
        assert_eq!(reasons, Reason::ALL.map(|r| u32::from(r.code())));
        assert_eq!(ALDIS_TC_DISCARD_NONE, 0);
        assert!(Reason::from_code(0).is_none());
        for (c, s) in [
            (ALDIS_TC_SEQ_CONTINUING, SequenceFlags::Continuing),
            (ALDIS_TC_SEQ_FIRST, SequenceFlags::First),
            (ALDIS_TC_SEQ_LAST, SequenceFlags::Last),
            (ALDIS_TC_SEQ_UNSEGMENTED, SequenceFlags::Unsegmented),
        ] {
            assert_eq!(c, s.bits());
        }
        assert_eq!(ALDIS_CLCW_COP_1, CopInEffect::COP_1.get());
    }

    #[test]
    fn enumerations_take_only_their_constants() {
        // req: CABI-010, CABI-022, TC-004, TC-005
        let f = aldis_tc_fields_t {
            frame_type: 3,
            ..aldis_tc_fields_t::default()
        };
        assert_eq!(fields(f), Err(invalid_argument()));
        let f = aldis_tc_fields_t {
            segment_header: 1,
            sequence_flags: 4,
            ..aldis_tc_fields_t::default()
        };
        assert_eq!(fields(f), Err(invalid_argument()));
        let f = aldis_tc_fields_t {
            segment_header: 2,
            ..aldis_tc_fields_t::default()
        };
        assert_eq!(fields(f), Err(invalid_argument()));
        let c = aldis_tc_control_t {
            command: 3,
            set_vr_value: 0,
        };
        assert_eq!(command(c), Err(invalid_argument()));
        let vc = aldis_tc_vc_config_t {
            segment_header: 1,
            security_trailer_len: 1,
            max_frame_len: 64,
            ..aldis_tc_vc_config_t::default()
        };
        assert_eq!(
            virtual_channel(TcSpacecraftId::new(0).unwrap(), &vc),
            Err(invalid_argument())
        );
        let flag = aldis_clcw_t {
            wait: 2,
            ..aldis_clcw_t::default()
        };
        assert_eq!(clcw(&flag), Err(invalid_argument()));
    }

    #[test]
    fn layout_follows_the_field_order() {
        // req: CABI-012, API-072
        let l = layout(true, 3, 7, 2, true);
        assert_eq!(
            (
                l.segment_header.offset,
                l.security_header.offset,
                l.data.offset
            ),
            (5, 6, 9)
        );
        assert_eq!(
            (l.security_trailer.offset, l.fecf.offset, l.frame_len),
            (16, 18, 20)
        );
        let l = layout(false, 0, 1, 0, false);
        assert_eq!((l.segment_header.len, l.fecf.len, l.frame_len), (0, 0, 6));
    }
}
