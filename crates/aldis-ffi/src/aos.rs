//! C ABI of the AOS Transfer Frame codec `aldis-aos` (CCSDS 732.0-B-5, and the wire format of
//! 732.0-B-4; M2 plan D4, D14; CABI-010…012, CABI-020…022).
//!
//! Calls: the Frame Header Error Control, [`aldis_aos_fhec_encode`] and
//! [`aldis_aos_fhec_decode`]; [`aldis_aos_config_check`] and [`aldis_aos_mc_config_check`];
//! [`aldis_aos_peek_length`] and [`aldis_aos_header_decode`]; the frame decoder
//! [`aldis_aos_decode`]; the data unit decoders [`aldis_aos_mpdu_decode`] and
//! [`aldis_aos_bpdu_decode`]; the one-shot [`aldis_aos_encode`], which takes the frame's variable
//! fields in an [`aldis_aos_content_t`]; and the one-shot OID frame encoder on VCID 63,
//! [`aldis_aos_encode_oid`], which takes an `aldis_oid_generator_t`.
//!
//! **Channels.** A physical channel is an [`aldis_aos_config_t`] (732.0 §5.2 Table 5-1, with
//! the edition; AOS-001), the configuration that `peek_length` and the header decoder take
//! (D14). A master channel is an [`aldis_aos_mc_config_t`], which holds its physical channel
//! and its Spacecraft ID, with an array of [`aldis_aos_vc_config_t`] and its count (Tables 5-2,
//! 5-3, 6-1; D14): the C form of a `VcTable` whose virtual channels all have that SCID, built
//! and checked on every call. The array is a C array of the struct
//! (`abi::configs`; c-abi.md §11). A physical channel that carries several
//! master channels is decoded by reading the SCID first with [`aldis_aos_header_decode`]; the
//! Valid Spacecraft IDs of a call are those of its master channel's virtual channels, as for
//! `VcTable` (Table 5-1).
//!
//! **Security layouts** (API-070; c-abi.md §11). Every call that takes a master channel uses
//! the Security Header and Trailer lengths of its virtual channels. `aldis-aos` has one frame
//! decoder, which needs the virtual channel's Data Field Content as well, so the decoder takes
//! the master channel and has no `_with` pair.
//!
//! **Encoders.** A frame is built in one call, with no state between calls (API-023, API-060):
//! [`aldis_aos_encode`] and [`aldis_aos_encode_oid`] take the frame's variable fields and write
//! the headers, the FHEC, and the FECF. The C ABI has no two-phase writer (OI-57).

use aldis::aos::bpdu::BPDU_HEADER_LEN;
use aldis::aos::fhec;
use aldis::aos::header::{FHEC_LEN, PRIMARY_HEADER_LEN};
use aldis::aos::mpdu::MPDU_HEADER_LEN;
use aldis::aos::{
    AosAnomaly, AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig,
    AosPrimaryHeader, AosVirtualChannelConfig, BitstreamPointer, Bpdu, DataFieldContent, DataUnit,
    FrameCountCycle, Mpdu, PacketOffset, PacketPointer, PduHeader, ValidBitCount, VcFrameCount,
    VcTable, encode, encode_oid, encode_oid_galois,
};
use aldis::core::field;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::layout::{FrameLayout, OCF_LEN};
use aldis::core::ocf::Ocf;
use aldis::core::security::SecurityLayout;
use aldis::core::{AnomalySet, Error, ErrorKind, Result};

use crate::abi::{self, Configs, invalid_argument};
use crate::mirror::{mirror, parse};
use crate::oid::{GeneratorState, aldis_oid_generator_t, generator};
use crate::range::aldis_range_t;

/// Length of the AOS Transfer Frame Primary Header without the FHEC, in octets (732.0
/// §4.1.2.1).
pub const ALDIS_AOS_PRIMARY_HEADER_LEN: u32 = 6;
/// Length of the Frame Header Error Control, in octets (732.0 §4.1.2.6.1; AOS-013).
pub const ALDIS_AOS_FHEC_LEN: u32 = 2;
/// Length of the `M_PDU` and of the `B_PDU` Header, in octets: the zone of a data unit starts
/// this many octets into the data field (732.0 §4.1.4.2.1.3, §4.1.4.3.1.3).
pub const ALDIS_AOS_PDU_HEADER_LEN: u32 = 2;
/// VCID 63, the VCID of OID Transfer Frames (732.0 §4.1.4.1.5.1, Table 5-2 note 2; AOS-016).
pub const ALDIS_AOS_OID_VCID: u8 = 63;
/// The most virtual channels one master channel can configure: VCIDs 0 to 62 (732.0 Table 5-3
/// note 2). A longer array names some VCID twice.
pub const ALDIS_AOS_MAX_VCS: usize = 63;
/// Largest Virtual Channel Frame Count, 2²⁴ − 1 (732.0 §4.1.2.4.2).
pub const ALDIS_AOS_VC_FRAME_COUNT_MAX: u32 = 0x00FF_FFFF;
/// Largest Virtual Channel Frame Count Cycle, `1111` (732.0 §4.1.2.5.5.1; AOS-012).
pub const ALDIS_AOS_CYCLE_MAX: u8 = 0x0F;
/// Largest packet offset of `ALDIS_AOS_POINTER_STARTS_AT`, `0xFFFD`, below the reserved values
/// of the Issue 5 pointer; `0x7FD` in Issue 4 (732.0 §4.1.4.2.2.3–5; AOS-003).
pub const ALDIS_AOS_PACKET_OFFSET_MAX: u16 = 0xFFFD;
/// Smallest count of valid bits of `ALDIS_AOS_POINTER_VALID_BITS` (732.0 §4.1.4.3.3.2).
pub const ALDIS_AOS_VALID_BITS_MIN: u16 = 1;
/// Largest count of valid bits of `ALDIS_AOS_POINTER_VALID_BITS`, `0x3FFE`: pointer `0x3FFD`,
/// the last index below the reserved values (732.0 §4.1.4.3.3.3–4; AOS-017).
pub const ALDIS_AOS_VALID_BITS_MAX: u16 = 0x3FFE;

/// The AOS edition (AOS-001; 732.0-B-5, 732.0-B-4).
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
pub enum aldis_aos_edition {
    /// Edition: 732.0-B-5, a 10-bit SCID and a 16-bit First Header Pointer; the default (732.0
    /// §4.1.2.2.3, §4.1.4.2.1.4; AOS-001, GEN-062).
    ALDIS_AOS_EDITION_ISSUE5 = 0,
    /// Edition: the wire format of 732.0-B-4 and earlier (AOS-002, AOS-003).
    ///
    /// An 8-bit SCID, Primary Header bits 42–43 a spare, and an `M_PDU` Header of a 5-bit spare and
    /// an 11-bit First Header Pointer (732.0-B-4 §4.1.2.2.3, §4.1.2.5.4, §4.1.4.2.2–3).
    ALDIS_AOS_EDITION_ISSUE4 = 1,
}
mirror!(pub aldis_aos_edition: u8 { ALDIS_AOS_EDITION_ISSUE5, ALDIS_AOS_EDITION_ISSUE4 });

/// The Data Field Content of a virtual channel (732.0 Table 5-3).
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
pub enum aldis_aos_content {
    /// Data Field Content: one `M_PDU` per frame (732.0 §4.1.4.2, Table 5-3).
    ALDIS_AOS_CONTENT_MPDU = 0,
    /// Data Field Content: one `B_PDU` per frame (732.0 §4.1.4.3, Table 5-3; AOS-018).
    ALDIS_AOS_CONTENT_BPDU = 1,
    /// Data Field Content: one `VCA_SDU` per frame, opaque (732.0 §4.1.4.1.3, Table 5-3).
    ALDIS_AOS_CONTENT_VCA_SDU = 2,
    /// Data Field Content: Idle Data, the content of an OID frame's data field too (732.0
    /// §4.1.4.1.3, §4.1.4.1.5, Table 5-3).
    ALDIS_AOS_CONTENT_IDLE_DATA = 3,
}
mirror!(pub aldis_aos_content: u8 { ALDIS_AOS_CONTENT_MPDU, ALDIS_AOS_CONTENT_BPDU, ALDIS_AOS_CONTENT_VCA_SDU, ALDIS_AOS_CONTENT_IDLE_DATA });

/// The kind of `M_PDU` or `B_PDU` Header pointer of a frame (732.0 §4.1.4.2, §4.1.4.3).
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
pub enum aldis_aos_pointer {
    /// Pointer: no `M_PDU` or `B_PDU` Header: a `VCA_SDU` or Idle Data (732.0 §4.1.4.1.3).
    ALDIS_AOS_POINTER_NONE = 0,
    /// `M_PDU` First Header Pointer: the first Packet that starts in the Packet Zone starts at the
    /// octet `pointer_value` (732.0 §4.1.4.2.2.3; AOS-017).
    ALDIS_AOS_POINTER_STARTS_AT = 1,
    /// `M_PDU` First Header Pointer all ones: no Packet starts in the Packet Zone (732.0
    /// §4.1.4.2.2.4).
    ALDIS_AOS_POINTER_NO_PACKET_START = 2,
    /// `M_PDU` First Header Pointer all ones minus one: the Packet Zone holds only Idle Data (732.0
    /// §4.1.4.2.2.5).
    ALDIS_AOS_POINTER_ONLY_IDLE_DATA = 3,
    /// `B_PDU` Bitstream Data Pointer: the first `pointer_value` bits of the Bitstream Data Zone are
    /// user data, the rest idle data (732.0 §4.1.4.3.3.2; AOS-017, GEN-053).
    ALDIS_AOS_POINTER_VALID_BITS = 4,
    /// `B_PDU` Bitstream Data Pointer all ones: the zone holds only user data (732.0
    /// §4.1.4.3.3.3).
    ALDIS_AOS_POINTER_ALL_VALID = 5,
    /// `B_PDU` Bitstream Data Pointer all ones minus one: the zone holds only idle data (732.0
    /// §4.1.4.3.3.4).
    ALDIS_AOS_POINTER_ONLY_IDLE = 6,
}
mirror!(pub aldis_aos_pointer: u8 { ALDIS_AOS_POINTER_NONE, ALDIS_AOS_POINTER_STARTS_AT, ALDIS_AOS_POINTER_NO_PACKET_START, ALDIS_AOS_POINTER_ONLY_IDLE_DATA, ALDIS_AOS_POINTER_VALID_BITS, ALDIS_AOS_POINTER_ALL_VALID, ALDIS_AOS_POINTER_ONLY_IDLE });

/// Anomaly bit: Virtual Channel Frame Count Cycle other than `0000` with its Use Flag `0`
/// (bit 0 of `AosAnomaly`; 732.0 §4.1.2.5.5.3; AOS-012, API-032).
pub const ALDIS_AOS_ANOMALY_CYCLE_NON_ZERO_WHILE_UNUSED: u32 = 1 << 0;
/// Anomaly bit: `B_PDU` Reserved Spare other than `00` (bit 1; 732.0 §4.1.4.3.2.2; API-032).
pub const ALDIS_AOS_ANOMALY_BPDU_SPARE_SET: u32 = 1 << 1;
/// Anomaly bit: in Issue 4, Primary Header bits 42–43 or the `M_PDU` Reserved Spare not zero
/// (bit 2; 732.0-B-4 §4.1.2.5.4, §4.1.4.2.2; AOS-002, AOS-003, API-032).
pub const ALDIS_AOS_ANOMALY_ISSUE4_SPARE_SET: u32 = 1 << 2;

/// Managed parameters of a physical channel (732.0 §5.2 Table 5-1; AOS-001, AOS-015, API-041). The configuration `aldis_aos_peek_length` takes (M2 plan D14).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_config_t {
    /// 1 if every Primary Header ends with the Frame Header Error Control, else 0 (732.0
    /// §4.1.2.6.2; AOS-013).
    pub fhec: u8,
    /// 1 if every frame ends with a Frame Error Control Field, else 0 (732.0 §4.1.6.1.2).
    pub fecf: u8,
    /// The wire format, `ALDIS_AOS_EDITION_*`; 0 is Issue 5 (AOS-001, GEN-062).
    pub edition: u8,
    /// Transfer Frame Length in octets: at least the Primary Header, the Insert Zone, one data
    /// octet, and the FECF (732.0 §4.1.1.2; CMN-041).
    pub frame_len: usize,
    /// Insert Zone length in octets, 0 for none (732.0 §4.1.3; AOS-015).
    pub insert_zone_len: usize,
}

/// A master channel: the Spacecraft ID of the virtual channels passed beside it, and its
/// physical channel (732.0 §5.3 Table 5-2; API-040; c-abi.md §11).
///
/// Its virtual channels are passed beside it, as an array of [`aldis_aos_vc_config_t`] and its
/// count (M2 plan D14).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_mc_config_t {
    /// Spacecraft Identifier, 0 to 1023; at most 255 for a virtual channel in Issue 4 (732.0
    /// §4.1.2.2.3; AOS-002).
    pub scid: u16,
    /// The physical channel.
    pub pc: aldis_aos_config_t,
}

/// Managed parameters of a virtual channel (732.0 §5.4 Table 5-3, §6.6.2 Table 6-1; API-040,
/// API-070).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_vc_config_t {
    /// Virtual Channel Identifier, 0 to 62; 63 is reserved for OID frames (732.0 §4.1.2.3,
    /// Table 5-3 note 2).
    pub vcid: u8,
    /// Data Field Content, `ALDIS_AOS_CONTENT_*` (732.0 §4.1.4.1.4, Table 5-3).
    pub content: u8,
    /// 1 for a `VC_OCF`, else 0 (732.0 §4.1.5.2, Table 5-3).
    pub ocf: u8,
    /// Security Header length in octets, at most 64; 0 without SDLS (Table 6-1; API-070).
    pub security_header_len: u8,
    /// Security Trailer length in octets; 0 without SDLS (Table 6-1; API-070).
    pub security_trailer_len: u16,
}

/// The Primary Header fields and the data unit header a sender chooses (732.0 §4.1.2,
/// §4.1.4.2.2, §4.1.4.3.3; AOS-019).
///
/// The SCID comes from the master channel, and the version, the spare bits, and the FHEC are
/// the library's (API-021).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_fields_t {
    /// Virtual Channel Identifier: a configured VC, or `ALDIS_AOS_OID_VCID` for
    /// `aldis_aos_encode_oid` (732.0 §4.1.2.3).
    pub vcid: u8,
    /// Virtual Channel Frame Count, at most `ALDIS_AOS_VC_FRAME_COUNT_MAX` (732.0 §4.1.2.4).
    pub vc_frame_count: u32,
    /// Replay Flag, 0 or 1 (732.0 §4.1.2.5.2).
    pub replay_flag: u8,
    /// Virtual Channel Frame Count Cycle Use Flag, 0 or 1; with 0 the cycle is written `0000`
    /// and `cycle` is ignored (732.0 §4.1.2.5.3; AOS-012).
    pub cycle_use_flag: u8,
    /// With `cycle_use_flag` 1: the Virtual Channel Frame Count Cycle, at most
    /// `ALDIS_AOS_CYCLE_MAX` (732.0 §4.1.2.5.5).
    pub cycle: u8,
    /// The data unit header, `ALDIS_AOS_POINTER_*`: one of the `M_PDU` pointers on a VC that
    /// carries `M_PDUs`, one of the `B_PDU` pointers on a VC that carries `B_PDUs`, else
    /// `ALDIS_AOS_POINTER_NONE` (732.0 §4.1.4.1.4; AOS-019).
    pub pointer: u8,
    /// With `ALDIS_AOS_POINTER_STARTS_AT`, the packet offset in the Packet Zone, inside it and
    /// at most `ALDIS_AOS_PACKET_OFFSET_MAX` (`0x7FD` in Issue 4); with `ALDIS_AOS_POINTER_VALID_BITS`, the count of
    /// valid bits, `ALDIS_AOS_VALID_BITS_MIN` to `ALDIS_AOS_VALID_BITS_MAX` and below the zone's
    /// length in bits; else ignored (732.0 §4.1.4.2.2.3, §4.1.4.3.3.2; AOS-017).
    pub pointer_value: u16,
}

/// The variable fields of a frame for [`aldis_aos_encode`], as `(pointer, length)` buffers
/// (732.0 §4.1, §6.3; AOS-015, AOS-019, API-072, CABI-011; c-abi.md §11).
///
/// Each buffer has exactly the length the configuration gives its field, and may be `NULL`
/// with length 0. The struct is read once per call and no pointer is kept (API-060).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_aos_content_t {
    /// The Insert Zone, an `IN_SDU`; empty without one (732.0 §4.1.3).
    pub insert_zone: *const u8,
    /// Length of `insert_zone`.
    pub insert_zone_len: usize,
    /// The Security Header, as a security hook filled it; empty without SDLS (732.0 §6.3.4).
    pub security_header: *const u8,
    /// Length of `security_header`.
    pub security_header_len: usize,
    /// The octets after the `M_PDU` or `B_PDU` Header, or the whole data field of a `VCA_SDU`
    /// or Idle Data, filling it exactly (732.0 §4.1.4).
    pub zone: *const u8,
    /// Length of `zone`.
    pub zone_len: usize,
    /// The Security Trailer; empty without SDLS (732.0 §6.3.6).
    pub security_trailer: *const u8,
    /// Length of `security_trailer`.
    pub security_trailer_len: usize,
    /// The Operational Control Field: `ALDIS_OCF_LEN` octets when the VC has one, else empty
    /// (732.0 §4.1.5).
    pub ocf: *const u8,
    /// Length of `ocf`: `ALDIS_OCF_LEN` or 0.
    pub ocf_len: usize,
}

impl Default for aldis_aos_content_t {
    /// Every buffer empty and `NULL`.
    fn default() -> aldis_aos_content_t {
        aldis_aos_content_t {
            insert_zone: core::ptr::null(),
            insert_zone_len: 0,
            security_header: core::ptr::null(),
            security_header_len: 0,
            zone: core::ptr::null(),
            zone_len: 0,
            security_trailer: core::ptr::null(),
            security_trailer_len: 0,
            ocf: core::ptr::null(),
            ocf_len: 0,
        }
    }
}

impl aldis_aos_content_t {
    /// The five buffers, which an output buffer must not overlap (CABI-022).
    const fn buffers(&self) -> [(*const u8, usize); 5] {
        [
            (self.insert_zone, self.insert_zone_len),
            (self.security_header, self.security_header_len),
            (self.zone, self.zone_len),
            (self.security_trailer, self.security_trailer_len),
            (self.ocf, self.ocf_len),
        ]
    }
}

/// A primary header that passed the FHEC check, corrected where needed (732.0 §4.1.2.6,
/// §4.3.7.2; AOS-013, AOS-014).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_corrected_header_t {
    /// The eight header octets after correction, the FHEC included; octets 2–4, which the code
    /// does not protect, as received (§4.1.2.6.1 note 2).
    pub octets: [u8; 8],
    /// How many symbols were corrected: 0, 1, or 2 (AOS-014, M2 plan D9).
    pub corrected_symbols: u8,
}

/// An AOS Transfer Frame Primary Header as plain values, after FHEC correction (732.0 §4.1.2;
/// AOS-002, AOS-010…AOS-014, API-014).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_header_t {
    /// Spacecraft Identifier, as the edition defines it (732.0 §4.1.2.2.3; AOS-002).
    pub scid: u16,
    /// Virtual Channel Identifier; `ALDIS_AOS_OID_VCID` for an OID frame (732.0 §4.1.2.3).
    pub vcid: u8,
    /// Virtual Channel Frame Count, never corrected (732.0 §4.1.2.4, §4.1.2.6.1 note 2).
    pub vc_frame_count: u32,
    /// Replay Flag (732.0 §4.1.2.5.2).
    pub replay_flag: bool,
    /// Virtual Channel Frame Count Cycle Use Flag (732.0 §4.1.2.5.3).
    pub cycle_use_flag: bool,
    /// Bits 44–47 as received, whether or not the Use Flag is set (732.0 §4.1.2.5.5; AOS-012).
    pub cycle_field: u8,
    /// In Issue 4, bits 42–43, the Reserved Spare, as received; always 0 in Issue 5
    /// (732.0-B-4 §4.1.2.5.4; AOS-002).
    pub issue4_spare: u8,
    /// Whether the header ends with an FHEC (732.0 §4.1.2.6).
    pub has_fhec: bool,
    /// The FHEC after correction, when `has_fhec`.
    pub fhec: u16,
    /// How many symbols the FHEC corrected: 0, 1, or 2; 0 without the FHEC (AOS-014).
    pub corrected_symbols: u8,
    /// The header octets after correction: 8 with the FHEC, else 6 and then two zero octets
    /// (732.0 §4.1.2; API-014).
    pub octets: [u8; 8],
}

/// A decoded `M_PDU` or `B_PDU` Header, or none (732.0 §4.1.4.2.2, §4.1.4.3.2–3; AOS-003,
/// AOS-017).
///
/// The zone, the Packet Zone or the Bitstream Data Zone, follows the
/// `ALDIS_AOS_PDU_HEADER_LEN` header octets.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_pdu_t {
    /// What the data field holds, `ALDIS_AOS_CONTENT_*` (Table 5-3).
    pub content: u8,
    /// The pointer classified, `ALDIS_AOS_POINTER_*`; `ALDIS_AOS_POINTER_NONE` for a
    /// `VCA_SDU` or Idle Data.
    pub pointer: u8,
    /// With `ALDIS_AOS_POINTER_STARTS_AT`, the offset of the first Packet in the Packet Zone;
    /// with `ALDIS_AOS_POINTER_VALID_BITS`, the count of valid bits; else 0.
    pub pointer_value: u16,
    /// The First Header Pointer (16 bits, 11 in Issue 4) or the Bitstream Data Pointer (14 bits)
    /// as received; 0 without a header (732.0 §4.1.4.2.2.1, §4.1.4.3.3.1).
    pub pointer_raw: u16,
    /// The Reserved Spare as received: the `B_PDU`'s, or in Issue 4 the `M_PDU`'s; else 0
    /// (732.0 §4.1.4.3.2; 732.0-B-4 §4.1.4.2.2).
    pub spare: u8,
    /// Lenient-mode anomaly flags of the data unit, `ALDIS_AOS_ANOMALY_*`; 0 in strict mode
    /// (API-032).
    pub anomalies: u32,
}

/// Where each field of a frame lies (732.0 §4.1, §6.3; CMN-040, API-072, CABI-012).
///
/// Offsets and lengths are in octets from the start of the frame. An absent field has length 0
/// at the offset where it would start.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_layout_t {
    /// Transfer Frame Length: the octets the frame takes (732.0 §4.1.1.2; API-012).
    pub frame_len: usize,
    /// The range of the Insert Zone; length 0 without one (§4.1.3; AOS-015).
    pub insert_zone: aldis_range_t,
    /// The range of the Security Header; length 0 without SDLS (§6.3.4).
    pub security_header: aldis_range_t,
    /// The range of the Transfer Frame Data Field; length never 0 (§4.1.4).
    pub data: aldis_range_t,
    /// The range of the zone after the `M_PDU` or `B_PDU` Header, or of the data field; length never 0 (§4.1.4.2.3, §4.1.4.3.4).
    pub zone: aldis_range_t,
    /// The range of the Security Trailer; length 0 without SDLS (§6.3.6).
    pub security_trailer: aldis_range_t,
    /// The range of the Operational Control Field; its length is `ALDIS_OCF_LEN` or 0 (§4.1.5).
    pub ocf: aldis_range_t,
    /// The range of the Frame Error Control Field; its length is 2 or 0 (§4.1.6).
    pub fecf: aldis_range_t,
}

/// A decoded AOS Transfer Frame: its fields and where they lie in the input (732.0 §4.1, §6.3;
/// AOS-010…AOS-017, API-010, CABI-012). The OCF is classified with `aldis_ocf_classify` on its
/// octets (CMN-010).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_aos_frame_t {
    /// The Primary Header after FHEC correction (732.0 §4.1.2; API-014).
    pub header: aldis_aos_header_t,
    /// The data unit: `ALDIS_AOS_CONTENT_IDLE_DATA` for an OID frame (AOS-016, AOS-017).
    pub pdu: aldis_aos_pdu_t,
    /// Whether the frame has an FECF (732.0 §4.1.6).
    pub has_fecf: bool,
    /// The FECF, when `has_fecf`.
    pub fecf: u16,
    /// Lenient-mode anomaly flags of the header and the data unit, `ALDIS_AOS_ANOMALY_*`; 0 in
    /// strict mode (API-032).
    pub anomalies: u32,
    /// Where each field lies in the input.
    pub layout: aldis_aos_layout_t,
}

/// The edition of a C constant (AOS-001; CABI-010, CABI-022).
fn edition(value: u8) -> Result<AosEdition> {
    match parse::<aldis_aos_edition>(value)? {
        aldis_aos_edition::ALDIS_AOS_EDITION_ISSUE5 => Ok(AosEdition::Issue5),
        aldis_aos_edition::ALDIS_AOS_EDITION_ISSUE4 => Ok(AosEdition::Issue4Compatible),
    }
}

/// The physical channel of a C struct (732.0 §5.2).
fn physical(c: &aldis_aos_config_t) -> Result<AosPhysicalChannelConfig> {
    AosPhysicalChannelConfig::new(
        c.frame_len,
        abi::flag(c.fhec)?,
        c.insert_zone_len,
        abi::flag(c.fecf)?,
        edition(c.edition)?,
    )
}

/// The virtual channel of a C struct on master channel `scid`, each parameter parsed into its
/// type (Table 5-3, Table 6-1; RS-020).
fn virtual_channel(
    scid: SpacecraftId<10>,
    v: aldis_aos_vc_config_t,
) -> Result<AosVirtualChannelConfig> {
    let vcid = VcId::try_from(v.vcid)?;
    let content = match parse::<aldis_aos_content>(v.content)? {
        aldis_aos_content::ALDIS_AOS_CONTENT_MPDU => DataFieldContent::Mpdu,
        aldis_aos_content::ALDIS_AOS_CONTENT_BPDU => DataFieldContent::Bpdu,
        aldis_aos_content::ALDIS_AOS_CONTENT_VCA_SDU => DataFieldContent::VcaSdu,
        aldis_aos_content::ALDIS_AOS_CONTENT_IDLE_DATA => DataFieldContent::IdleData,
    };
    let mut vc = AosVirtualChannelConfig::new(scid, vcid, content)?;
    if abi::flag(v.ocf)? {
        vc = vc.with_ocf();
    }
    let security = SecurityLayout::new(
        usize::from(v.security_header_len),
        usize::from(v.security_trailer_len),
    )?;
    Ok(vc.with_security(security))
}

/// Room for one more virtual channel than a master channel can hold, so that `VcTable::new`
/// itself rejects a VCID given twice in a longer array, and never reports its capacity
/// (GEN-011).
const TABLE: usize = ALDIS_AOS_MAX_VCS + 1;

/// A master channel parsed from C structs: its physical channel, its Spacecraft ID, and the
/// table of its virtual channels (732.0 §5; GEN-060, API-040).
struct Master {
    pc: AosPhysicalChannelConfig,
    scid: SpacecraftId<10>,
    vcs: VcTable<TABLE>,
}

/// The master channel of C structs: its physical channel and SCID, then each virtual channel in
/// array order, and then the checks of `VcTable::new` (Tables 5-1 to 5-3; API-040).
fn master_channel(
    mc: &aldis_aos_mc_config_t,
    vcs: &Configs<'_, aldis_aos_vc_config_t>,
) -> Result<Master> {
    let pc = physical(&mc.pc)?;
    let scid = SpacecraftId::try_from(mc.scid)?;
    let mut list = [None; TABLE];
    for i in 0..vcs.len() {
        let vc = virtual_channel(scid, vcs.get(i)?)?;
        if let Some(slot) = list.get_mut(i) {
            *slot = Some(vc);
        }
    }
    // The first `TABLE` channels, in array order; a longer array names a VCID twice among them.
    let n = vcs.len().min(TABLE);
    let vcs = match list.first().copied().flatten() {
        None => VcTable::new(pc, &[])?,
        Some(first) => {
            let channels = list.map(|vc| vc.unwrap_or(first));
            VcTable::new(pc, channels.get(..n).unwrap_or_default())?
        }
    };
    Ok(Master { pc, scid, vcs })
}

/// The master channel at `mc` with the `vc_count` virtual channels at `vcs` (CABI-022).
///
/// # Safety
/// As for `abi::read` and `abi::configs`.
#[expect(unsafe_code, reason = "checked access to caller memory (CABI-022)")]
unsafe fn master_channel_at(
    mc: *const aldis_aos_mc_config_t,
    vcs: *const aldis_aos_vc_config_t,
    vc_count: usize,
) -> Result<Master> {
    // SAFETY: forwarded from the caller.
    let (mc, vcs) = unsafe { (abi::read(mc)?, abi::configs(vcs, vc_count)?) };
    master_channel(&mc, &vcs)
}

/// The sender's fields of a C struct on master channel `scid`, each parsed into its type
/// (732.0 §4.1.2; RS-020). The packet offset becomes a `PacketOffset` and the count of valid
/// bits a `ValidBitCount` here, at the boundary.
fn fields(scid: SpacecraftId<10>, f: aldis_aos_fields_t) -> Result<AosFrameFields> {
    let vcid = VcId::try_from(f.vcid)?;
    let vc_frame_count = VcFrameCount::try_from(f.vc_frame_count)?;
    let replay_flag = abi::flag(f.replay_flag)?;
    let cycle = if abi::flag(f.cycle_use_flag)? {
        Some(FrameCountCycle::try_from(f.cycle)?)
    } else {
        None
    };
    let pdu = match parse::<aldis_aos_pointer>(f.pointer)? {
        aldis_aos_pointer::ALDIS_AOS_POINTER_NONE => PduHeader::None,
        aldis_aos_pointer::ALDIS_AOS_POINTER_STARTS_AT => PduHeader::Mpdu(PacketPointer::StartsAt(
            PacketOffset::try_from(f.pointer_value)?,
        )),
        aldis_aos_pointer::ALDIS_AOS_POINTER_NO_PACKET_START => {
            PduHeader::Mpdu(PacketPointer::NoPacketStart)
        }
        aldis_aos_pointer::ALDIS_AOS_POINTER_ONLY_IDLE_DATA => {
            PduHeader::Mpdu(PacketPointer::OnlyIdleData)
        }
        aldis_aos_pointer::ALDIS_AOS_POINTER_VALID_BITS => PduHeader::Bpdu(
            BitstreamPointer::ValidBits(ValidBitCount::try_from(f.pointer_value)?),
        ),
        aldis_aos_pointer::ALDIS_AOS_POINTER_ALL_VALID => {
            PduHeader::Bpdu(BitstreamPointer::AllValid)
        }
        aldis_aos_pointer::ALDIS_AOS_POINTER_ONLY_IDLE => {
            PduHeader::Bpdu(BitstreamPointer::OnlyIdle)
        }
    };
    Ok(AosFrameFields {
        scid,
        vcid,
        vc_frame_count,
        replay_flag,
        cycle,
        pdu,
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

/// The C form of a Primary Header.
const fn header(h: &AosPrimaryHeader) -> aldis_aos_header_t {
    aldis_aos_header_t {
        scid: h.scid().get(),
        vcid: h.vcid().get(),
        vc_frame_count: h.vc_frame_count().get(),
        replay_flag: h.replay_flag(),
        cycle_use_flag: h.cycle_use_flag(),
        cycle_field: h.cycle_field().get(),
        issue4_spare: h.issue4_spare(),
        has_fhec: h.fhec().is_some(),
        fhec: match h.fhec() {
            Some(f) => f,
            None => 0,
        },
        corrected_symbols: h.corrected_symbols().get(),
        octets: h.to_octets(),
    }
}

/// The C form of an `M_PDU` Header.
fn mpdu(m: &Mpdu<'_>) -> aldis_aos_pdu_t {
    let (pointer, pointer_value) = match m.pointer() {
        PacketPointer::StartsAt(n) => (ALDIS_AOS_POINTER_STARTS_AT, n.get()),
        PacketPointer::NoPacketStart => (ALDIS_AOS_POINTER_NO_PACKET_START, 0),
        PacketPointer::OnlyIdleData => (ALDIS_AOS_POINTER_ONLY_IDLE_DATA, 0),
    };
    aldis_aos_pdu_t {
        content: ALDIS_AOS_CONTENT_MPDU,
        pointer,
        pointer_value,
        pointer_raw: m.first_header_pointer(),
        spare: m.issue4_spare(),
        anomalies: if m.issue4_spare() == 0 {
            0
        } else {
            AnomalySet::from_flags(&[AosAnomaly::Issue4SpareSet]).bits()
        },
    }
}

/// The C form of a `B_PDU` Header.
fn bpdu(b: &Bpdu<'_>) -> aldis_aos_pdu_t {
    let (pointer, pointer_value) = match b.pointer() {
        BitstreamPointer::ValidBits(n) => (ALDIS_AOS_POINTER_VALID_BITS, n.get()),
        BitstreamPointer::AllValid => (ALDIS_AOS_POINTER_ALL_VALID, 0),
        BitstreamPointer::OnlyIdle => (ALDIS_AOS_POINTER_ONLY_IDLE, 0),
    };
    aldis_aos_pdu_t {
        content: ALDIS_AOS_CONTENT_BPDU,
        pointer,
        pointer_value,
        pointer_raw: b.bitstream_data_pointer(),
        spare: b.spare(),
        anomalies: if b.spare() == 0 {
            0
        } else {
            AnomalySet::from_flags(&[AosAnomaly::BpduSpareSet]).bits()
        },
    }
}

/// A data field without a data unit header: a `VCA_SDU` or Idle Data.
const fn plain(content: u8) -> aldis_aos_pdu_t {
    aldis_aos_pdu_t {
        content,
        pointer: ALDIS_AOS_POINTER_NONE,
        pointer_value: 0,
        pointer_raw: 0,
        spare: 0,
        anomalies: 0,
    }
}

/// The C form of a frame layout, whose zone follows `pdu_header_len` octets of the data field
/// (732.0 §4.1, §4.1.4; CMN-040).
///
/// # Errors
/// `InternalError` if the PDU Header is longer than the data field, which a configuration that
/// passed `aldis_aos_validate` rules out (GEN-020).
fn layout(l: &FrameLayout, pdu_header_len: usize) -> Result<aldis_aos_layout_t> {
    let data = l.data();
    let zone_start = data
        .start
        .checked_add(pdu_header_len)
        .filter(|&start| start <= data.end)
        .ok_or(Error::new(ErrorKind::InternalError).with_field(field::FRAME_DATA_FIELD))?;
    let zone = zone_start..data.end;
    let insert_zone = aldis_range_t::from(&l.secondary());
    let security_header = aldis_range_t::from(&l.security_header());
    let data = aldis_range_t::from(&data);
    let zone = aldis_range_t::from(&zone);
    let security_trailer = aldis_range_t::from(&l.security_trailer());
    let ocf = aldis_range_t::from(&l.ocf());
    let fecf = aldis_range_t::from(&l.fecf());
    Ok(aldis_aos_layout_t {
        frame_len: l.frame_len(),
        insert_zone,
        security_header,
        data,
        zone,
        security_trailer,
        ocf,
        fecf,
    })
}

/// The C view of a decoded frame.
///
/// # Errors
/// `InternalError` from [`layout`].
pub(crate) fn view(
    f: &AosFrame<'_>,
    anomalies: AnomalySet<AosAnomaly>,
) -> Result<aldis_aos_frame_t> {
    let (pdu, header_len) = match f.data_unit() {
        DataUnit::Mpdu(m) => (mpdu(&m), MPDU_HEADER_LEN),
        DataUnit::Bpdu(b) => (bpdu(&b), BPDU_HEADER_LEN),
        DataUnit::VcaSdu(_) => (plain(ALDIS_AOS_CONTENT_VCA_SDU), 0),
        DataUnit::IdleData(_) => (plain(ALDIS_AOS_CONTENT_IDLE_DATA), 0),
    };
    Ok(aldis_aos_frame_t {
        header: header(&f.header()),
        pdu,
        has_fecf: f.fecf().is_some(),
        fecf: f.fecf().unwrap_or(0),
        anomalies: anomalies.bits(),
        layout: layout(f.layout(), header_len)?,
    })
}

/// Computes the FHEC of the six octets of a Primary Header: the four parity symbols, which form
/// octets 6 and 7, from octets 0, 1, and 5 (732.0 §4.1.2.6.5; AOS-013, AOS-019).
///
/// `*out` is the FHEC as a big-endian 16-bit value: octet 6 in its high octet.
///
/// # Errors
/// CABI-022 violations, including a `header_len` other than `ALDIS_AOS_PRIMARY_HEADER_LEN`.
///
/// # Safety
/// `header` is valid for `header_len` octets (or `NULL` with 0); `out` is valid for one write;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_fhec_encode(
    header: *const u8,
    header_len: usize,
    out: *mut u16,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let header = <&[u8; PRIMARY_HEADER_LEN]>::try_from(abi::bytes(header, header_len)?)
                .map_err(|_| invalid_argument())?;
            let out = abi::out(out)?;
            out.write(u16::from_be_bytes(fhec::encode(header)));
            Ok(())
        })
    }
}

/// Checks a received Primary Header with its FHEC and corrects up to two symbol errors (732.0
/// §4.1.2.6.5, §4.3.7.2; AOS-013, AOS-014), as `aldis_aos::fhec::decode` does.
///
/// The input is not changed. Three or more symbol errors can be miscorrected to another
/// codeword and reported as one or two corrections (§4.1.2.6.5 note 3).
///
/// # Errors
/// `ALDIS_ERR_HEADER_UNCORRECTABLE` if no pattern of at most two symbol errors explains the
/// syndromes; CABI-022 violations, including a `received_len` other than 8.
///
/// # Safety
/// `received` is valid for `received_len` octets (or `NULL` with 0); `out` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_fhec_decode(
    received: *const u8,
    received_len: usize,
    out: *mut aldis_aos_corrected_header_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let received = <&[u8; PRIMARY_HEADER_LEN + FHEC_LEN]>::try_from(abi::bytes(
                received,
                received_len,
            )?)
            .map_err(|_| invalid_argument())?;
            let out = abi::out(out)?;
            let corrected = fhec::decode(received)?;
            out.write(aldis_aos_corrected_header_t {
                octets: corrected.octets(),
                corrected_symbols: corrected.corrected_symbols().get(),
            });
            Ok(())
        })
    }
}

/// Checks a physical channel configuration (732.0 §5.2 Table 5-1; AOS-001, API-040, CMN-041).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`frame_len`) for a frame too short for the Primary Header (6
/// octets, 8 with the FHEC), the Insert Zone, one data octet, and the FECF, with that minimum;
/// CABI-022 violations, including an `edition` that is no `ALDIS_AOS_EDITION_*` constant.
///
/// # Safety
/// `pc` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_config_check(
    pc: *const aldis_aos_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || physical(&abi::read(pc)?).map(drop)) }
}

/// Checks a master channel configuration and its virtual channels (732.0 §5 Tables 5-1 to 5-3,
/// §6.6 Table 6-1; AOS-002, AOS-018, API-040, API-070, CMN-041), as `VcTable::new` checks them.
///
/// The physical channel is checked first, then the SCID, then each virtual channel in array
/// order: its VCID, content, `VC_OCF` flag, and security layout; and then, in array order again,
/// whether each fits the master channel.
///
/// # Errors
/// The errors of [`aldis_aos_config_check`]; `ALDIS_ERR_OUT_OF_RANGE` (`spacecraft_id`, `vc_id`)
/// for an SCID above 1023 or a VCID above 63; `ALDIS_ERR_INVALID_CONFIG` (`vc_id`) for VCID 63,
/// which is reserved for OID frames, and (`security_header_len`) for a Security Header above 64
/// octets; `ALDIS_ERR_INVALID_CONFIG` (`vc_id`) for `vc_count` 0 (AOS-020, OI-55); then, for
/// the first virtual channel that fails: `ALDIS_ERR_INVALID_CONFIG` (`vc_id`)
/// for a VCID given twice, (`spacecraft_id`) for an SCID above 255 in Issue 4, (`frame_data_field`)
/// for a VC left no data octet or, for an `M_PDU` or `B_PDU`, no octet after its header, and
/// for a VC that carries `B_PDUs` (`frame_len`) for frames longer than 2 048 octets in Issue 5
/// and (`frame_data_field`) for a Bitstream Data Zone longer than 2 047 octets (AOS-018);
/// CABI-022 violations.
///
/// # Safety
/// `mc` is valid for a read; `vcs` is valid for reads of `vc_count` structs (or `NULL` with 0); `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_mc_config_check(
    mc: *const aldis_aos_mc_config_t,
    vcs: *const aldis_aos_vc_config_t,
    vc_count: usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || master_channel_at(mc, vcs, vc_count).map(drop)) }
}

/// The length of the frame that `prefix` begins: the configured frame length (732.0 §4.1.1.2;
/// API-013, M2 plan D14), as `AosFrame::peek_length` gives it.
///
/// Any prefix will do; the header it holds is checked as [`aldis_aos_decode`] checks it: with
/// the FHEC once all eight header octets are present, corrected first, and without it the
/// version as soon as the first octet is present.
///
/// # Errors
/// `ALDIS_ERR_HEADER_UNCORRECTABLE` (AOS-014); `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`)
/// for a version other than `01` (AOS-011); the errors of [`aldis_aos_config_check`]; CABI-022
/// violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `cfg` is valid for a read;
/// `out_len` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_peek_length(
    prefix: *const u8,
    prefix_len: usize,
    cfg: *const aldis_aos_config_t,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let pc = physical(&abi::read(cfg)?)?;
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out_len = abi::out(out_len)?;
            out_len.write(AosFrame::peek_length(prefix, &pc)?);
            Ok(())
        })
    }
}

/// Decodes the Primary Header that `prefix` begins, after the FHEC has corrected it if
/// configured (732.0 §4.1.2, §4.3.7.2; AOS-002, AOS-010…AOS-014, API-014).
///
/// The SCID is read as the edition of `cfg` defines it. The rest of the frame is not read: for
/// example to find the SCID of a frame on a physical channel that carries several master
/// channels. The checks that depend on the mode are [`aldis_aos_decode`]'s; here the fields
/// are given raw.
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (`needed` 6, or 8 with the FHEC) for a shorter prefix;
/// `ALDIS_ERR_HEADER_UNCORRECTABLE` (AOS-014); `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`)
/// for a version other than `01` (AOS-011); the errors of [`aldis_aos_config_check`]; CABI-022
/// violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `cfg` is valid for a read;
/// `out` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_header_decode(
    prefix: *const u8,
    prefix_len: usize,
    cfg: *const aldis_aos_config_t,
    out: *mut aldis_aos_header_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let pc = physical(&abi::read(cfg)?)?;
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out = abi::out(out)?;
            out.write(header(&AosPrimaryHeader::decode(prefix, &pc)?));
            Ok(())
        })
    }
}

/// Decodes one frame of the master channel `mc`, with the parameters of its virtual channel
/// from `vcs` (732.0 §4.1, §4.3.7, §6.3; AOS-010…AOS-017, API-010, API-012, API-070).
///
/// It decodes as `AosFrame::decode` does with a `VcTable`.
///
/// The header is corrected with the FHEC first and the FECF checked over the frame with the
/// corrected header (§4.3.7.2–3; AOS-014). An OID frame (VCID 63) needs only the SCID of `mc`
/// with at least one virtual channel: it has no OCF and no SDLS fields (§4.1.4.1.5).
///
/// # Errors
/// The errors of [`aldis_aos_mc_config_check`]; then, in this order:
/// `ALDIS_ERR_LENGTH_MISMATCH` (`frame_len`) if `len` is not the frame length (API-012);
/// `ALDIS_ERR_HEADER_UNCORRECTABLE` (AOS-014); `ALDIS_ERR_CRC_MISMATCH` for a wrong FECF;
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`) for a version other than `01` (AOS-011);
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id` for another SCID, `vc_id` for a VC `vcs` does not
/// hold); the pointer errors of [`aldis_aos_mpdu_decode`] and [`aldis_aos_bpdu_decode`], in
/// both modes (AOS-017); in strict mode, for the first deviation: `ALDIS_ERR_INVALID_FIELD`
/// (`vc_frame_count_cycle`) for a cycle other than `0000` with its Use Flag `0` (AOS-012),
/// `ALDIS_ERR_RESERVED_VALUE` (`aos_header_spare`) for Issue 4 bits 42–43 other than `00`
/// (AOS-002), and the spare errors of the data unit; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `mc` and `vcs` as for
/// [`aldis_aos_mc_config_check`]; `out` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_decode(
    bytes: *const u8,
    len: usize,
    mc: *const aldis_aos_mc_config_t,
    vcs: *const aldis_aos_vc_config_t,
    vc_count: usize,
    mode: u32,
    out: *mut aldis_aos_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let m = master_channel_at(mc, vcs, vc_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let (frame, anomalies) = mode.dispatch(
                || AosFrame::decode(bytes, &m.pc, &m.vcs),
                || AosFrame::decode_lenient(bytes, &m.pc, &m.vcs),
            )?;
            out.write(view(&frame, anomalies)?);
            Ok(())
        })
    }
}

/// Decodes an `M_PDU` that fills `field`, the data field of a frame of a VC that carries
/// `M_PDUs`, in `edition` (732.0 §4.1.4.2; 732.0-B-4 §4.1.4.2; AOS-003, AOS-017).
///
/// It decodes as `Mpdu::decode` does. Its Packet Zone is the rest of `field`.
///
/// # Errors
/// `ALDIS_ERR_MALFORMED` (`frame_data_field`) if `field` has no octet after the 2-octet header,
/// and (`first_header_pointer`) for a pointer outside the Packet Zone that is not reserved, in
/// both modes; in strict mode `ALDIS_ERR_RESERVED_VALUE` (`mpdu_spare`) for an Issue 4 Reserved
/// Spare other than `00000`; CABI-022 violations, including an `edition` that is no
/// `ALDIS_AOS_EDITION_*` constant.
///
/// # Safety
/// `field` is valid for `len` octets (or `NULL` with 0); `out` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_mpdu_decode(
    field: *const u8,
    len: usize,
    edition: u8,
    mode: u32,
    out: *mut aldis_aos_pdu_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let field = abi::bytes(field, len)?;
            let edition = self::edition(edition)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let (unit, _) = mode.dispatch(
                || Mpdu::decode(field, edition),
                || Mpdu::decode_lenient(field, edition),
            )?;
            out.write(mpdu(&unit));
            Ok(())
        })
    }
}

/// Decodes a `B_PDU` that fills `field`, the data field of a frame of a VC that carries
/// `B_PDUs` (732.0 §4.1.4.3; AOS-017), as `Bpdu::decode` does. Its Bitstream Data Zone is the
/// rest of `field`.
///
/// # Errors
/// `ALDIS_ERR_MALFORMED` (`frame_data_field`) if `field` has no octet after the 2-octet header;
/// in both modes, `ALDIS_ERR_INVALID_FIELD` (`bitstream_data_pointer`) for the index of the
/// zone's last bit, which must be all ones instead (§4.1.4.3.3.3), and `ALDIS_ERR_MALFORMED`
/// (`bitstream_data_pointer`) for an index beyond the zone that is not reserved; in strict mode
/// `ALDIS_ERR_RESERVED_VALUE` (`bpdu_spare`) for a Reserved Spare other than `00`; CABI-022
/// violations.
///
/// # Safety
/// `field` is valid for `len` octets (or `NULL` with 0); `out` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_bpdu_decode(
    field: *const u8,
    len: usize,
    mode: u32,
    out: *mut aldis_aos_pdu_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let field = abi::bytes(field, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            let (unit, _) =
                mode.dispatch(|| Bpdu::decode(field), || Bpdu::decode_lenient(field))?;
            out.write(bpdu(&unit));
            Ok(())
        })
    }
}

/// Encodes one frame of (`mc->scid`, `fields->vcid`) into the start of `out` in one call (732.0
/// §4.1, §6.3; AOS-019, API-020…API-022, API-072), as `aldis_aos::encode` does.
///
/// `content` gives the variable fields, each exactly the length the configuration gives it
/// ([`aldis_aos_content_t`]). The library writes the FHEC, the `M_PDU` or `B_PDU` Header from
/// `fields->pointer`, and the FECF (API-021).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_aos_mc_config_check`]; then
/// `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `vcf_count`, `vc_frame_count_cycle`,
/// `first_header_pointer`, `bitstream_data_pointer`) for a value of `fields` outside its field:
/// a packet offset above `0xFFFD`, or a count of valid bits of 0 or above
/// `ALDIS_AOS_VALID_BITS_MAX`; then, as `AosFrameWriter::begin` checks them,
/// `ALDIS_ERR_INVALID_FIELD` (`vc_id`) for VCID 63, whose frames `aldis_aos_encode_oid`
/// writes; `ALDIS_ERR_UNKNOWN_CHANNEL` (`spacecraft_id` or `vc_id`) for a VC not in `vcs`;
/// `ALDIS_ERR_INVALID_FIELD` (`data_field_content`) for a pointer not of the VC's data unit;
/// `ALDIS_ERR_OUT_OF_RANGE` (`first_header_pointer`, `bitstream_data_pointer`) for a pointer
/// value outside the zone (AOS-017); `ALDIS_ERR_BUFFER_TOO_SMALL` (`needed` = frame length);
/// then `ALDIS_ERR_LENGTH_MISMATCH` (`insert_zone_len`, `security_header_len`,
/// `frame_data_field`, `security_trailer_len`) for a buffer of another length than its field's,
/// and `ALDIS_ERR_INVALID_FIELD` (`ocf`) for an OCF given where none is configured or missing
/// where one is; CABI-022 violations, including an `ocf_len` other than 0 or `ALDIS_OCF_LEN`
/// and `out` overlapping a buffer of `content`.
///
/// # Safety
/// `mc` and `vcs` as for [`aldis_aos_mc_config_check`]; `fields` and `content` are valid for a
/// read; each buffer of `content` is valid for its length (or `NULL` with 0); `out` is valid for
/// `cap` octets; `out_len` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_encode(
    mc: *const aldis_aos_mc_config_t,
    vcs: *const aldis_aos_vc_config_t,
    vc_count: usize,
    fields: *const aldis_aos_fields_t,
    content: *const aldis_aos_content_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let m = master_channel_at(mc, vcs, vc_count)?;
            let fields = self::fields(m.scid, abi::read(fields)?)?;
            let c = abi::read(content)?;
            let content = AosFrameContent {
                insert_zone: abi::bytes(c.insert_zone, c.insert_zone_len)?,
                security_header: abi::bytes(c.security_header, c.security_header_len)?,
                zone: abi::bytes(c.zone, c.zone_len)?,
                security_trailer: abi::bytes(c.security_trailer, c.security_trailer_len)?,
                ocf: self::ocf(abi::bytes(c.ocf, c.ocf_len)?)?,
            };
            let out = abi::bytes_mut(out, cap, &c.buffers())?;
            let out_len = abi::out(out_len)?;
            out_len.write(encode(&m.pc, &m.vcs, &fields, &content, out)?);
            Ok(())
        })
    }
}

/// `InvalidField` (no field) unless a buffer the call does not take is empty (CABI-022).
const fn empty(len: usize) -> Result<()> {
    if len == 0 {
        Ok(())
    } else {
        Err(invalid_argument())
    }
}

/// Encodes an OID Transfer Frame of `mc->scid` on VCID 63 in one call (732.0 §4.1.4.1.5;
/// AOS-016, CMN-020…022, API-020).
///
/// The frame goes into the start of `out`, its data field filled with the next octets of the OID
/// generator `oid`, as `aldis_aos::encode_oid` does with the
/// Fibonacci form, and `encode_oid_galois` with the Galois form, which gives the same octets.
///
/// The frame has no OCF or SDLS fields. `fields->vcid` is `ALDIS_AOS_OID_VCID` and
/// `fields->pointer` `ALDIS_AOS_POINTER_NONE`. `content` gives the Insert Zone (AOS-015),
/// empty when none is configured; its other buffers must be empty.
///
/// # Errors
/// On error neither `out` nor the generator is changed (GEN-042). The errors of
/// [`aldis_aos_mc_config_check`] and the `fields` errors of [`aldis_aos_encode`]; then
/// `ALDIS_ERR_INVALID_FIELD` (`vc_id`) unless `fields->vcid` is 63, and (`data_field_content`)
/// unless `fields->pointer` is `ALDIS_AOS_POINTER_NONE`; `ALDIS_ERR_UNKNOWN_CHANNEL`
/// (`spacecraft_id`) if `vcs` is empty; `ALDIS_ERR_BUFFER_TOO_SMALL` (`needed` = frame length);
/// `ALDIS_ERR_LENGTH_MISMATCH` (`insert_zone_len`) for an Insert Zone of another length than the
/// configured one; `ALDIS_ERR_INVALID_HANDLE` for an invalid generator; CABI-022 violations,
/// including a non-empty `content->zone`, `content->security_header`,
/// `content->security_trailer`, or `content->ocf`, and `out` overlapping the Insert Zone or the
/// generator's storage.
///
/// # Safety
/// As [`aldis_aos_encode`]; `oid` is a handle from `aldis_oid_generator_init` whose storage is
/// still valid, or detectably invalid storage.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_aos_encode_oid(
    mc: *const aldis_aos_mc_config_t,
    vcs: *const aldis_aos_vc_config_t,
    vc_count: usize,
    fields: *const aldis_aos_fields_t,
    content: *const aldis_aos_content_t,
    oid: *mut aldis_oid_generator_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let m = master_channel_at(mc, vcs, vc_count)?;
            let fields = self::fields(m.scid, abi::read(fields)?)?;
            let c = abi::read(content)?;
            empty(c.zone_len)?;
            empty(c.security_header_len)?;
            empty(c.security_trailer_len)?;
            empty(c.ocf_len)?;
            let insert_zone = abi::bytes(c.insert_zone, c.insert_zone_len)?;
            let h = generator(oid)?;
            let [iz, _, _, _, _] = c.buffers();
            let out = abi::bytes_mut(out, cap, &[iz, h.range()])?;
            let out_len = abi::out(out_len)?;
            let mut state = h.load();
            let n = match &mut state {
                GeneratorState::Fibonacci(g) => {
                    encode_oid(&m.pc, &m.vcs, &fields, insert_zone, g, out)?
                }
                GeneratorState::Galois(g) => {
                    encode_oid_galois(&m.pc, &m.vcs, &fields, insert_zone, g, out)?
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
    use aldis::aos::AosAnomaly;
    use aldis::aos::header::OID_VCID;
    use aldis::core::Anomaly as _;
    use aldis::core::layout::LayoutSpec;

    #[test]
    fn layout_zone_follows_the_pdu_header() {
        // req: CMN-040, GEN-020
        let spec = LayoutSpec {
            frame_len: 20,
            primary_header_len: 6,
            secondary_len: 0,
            security: SecurityLayout::NONE,
            ocf: false,
            fecf: false,
        };
        let l = FrameLayout::compute(&spec).unwrap();
        let c = layout(&l, MPDU_HEADER_LEN).unwrap();
        assert_eq!((c.data.offset, c.data.len), (6, 14));
        assert_eq!((c.zone.offset, c.zone.len), (8, 12));
        let c = layout(&l, 0).unwrap();
        assert_eq!((c.zone.offset, c.zone.len), (6, 14));
        let e = layout(&l, 15).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InternalError, Some(field::FRAME_DATA_FIELD))
        );
        let e = layout(&l, usize::MAX).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InternalError);
    }

    #[test]
    fn constants_are_the_crate_values() {
        // req: GEN-043, API-033, CABI-010, AOS-016, AOS-017
        assert_eq!(
            [
                ALDIS_AOS_ANOMALY_CYCLE_NON_ZERO_WHILE_UNUSED,
                ALDIS_AOS_ANOMALY_BPDU_SPARE_SET,
                ALDIS_AOS_ANOMALY_ISSUE4_SPARE_SET
            ],
            AosAnomaly::ALL
                .iter()
                .map(|a| 1_u32 << a.bit())
                .collect::<Vec<_>>()[..]
        );
        assert_eq!(
            usize::try_from(ALDIS_AOS_PRIMARY_HEADER_LEN),
            Ok(PRIMARY_HEADER_LEN)
        );
        assert_eq!(usize::try_from(ALDIS_AOS_FHEC_LEN), Ok(FHEC_LEN));
        assert_eq!(
            [usize::try_from(ALDIS_AOS_PDU_HEADER_LEN); 2],
            [Ok(MPDU_HEADER_LEN), Ok(BPDU_HEADER_LEN)]
        );
        assert_eq!(ALDIS_AOS_OID_VCID, OID_VCID.get());
        assert_eq!(usize::from(ALDIS_AOS_OID_VCID), ALDIS_AOS_MAX_VCS);
        assert_eq!(ALDIS_AOS_VC_FRAME_COUNT_MAX, VcFrameCount::MAX);
        assert_eq!(ALDIS_AOS_CYCLE_MAX, FrameCountCycle::MAX);
        assert_eq!(ALDIS_AOS_PACKET_OFFSET_MAX, PacketOffset::MAX);
        assert_eq!(
            (ALDIS_AOS_VALID_BITS_MIN, ALDIS_AOS_VALID_BITS_MAX),
            (ValidBitCount::MIN, ValidBitCount::MAX)
        );
    }

    #[test]
    fn enumerations_take_only_their_constants() {
        // req: CABI-010, CABI-022, AOS-001, AOS-019
        let scid = SpacecraftId::new(1).unwrap();
        let f = aldis_aos_fields_t {
            pointer: 7,
            ..aldis_aos_fields_t::default()
        };
        assert_eq!(fields(scid, f), Err(invalid_argument()));
        for f in [
            aldis_aos_fields_t {
                replay_flag: 2,
                ..aldis_aos_fields_t::default()
            },
            aldis_aos_fields_t {
                cycle_use_flag: 2,
                ..aldis_aos_fields_t::default()
            },
        ] {
            assert_eq!(fields(scid, f), Err(invalid_argument()));
        }
        // With the Use Flag 0 the cycle is not read.
        let f = aldis_aos_fields_t {
            cycle: 0xFF,
            ..aldis_aos_fields_t::default()
        };
        assert_eq!(fields(scid, f).map(|f| f.cycle), Ok(None));
        let vc = aldis_aos_vc_config_t {
            content: 4,
            ..aldis_aos_vc_config_t::default()
        };
        assert_eq!(virtual_channel(scid, vc), Err(invalid_argument()));
        assert_eq!(edition(2), Err(invalid_argument()));
        assert_eq!(ocf(&[1, 2, 3]), Err(invalid_argument()));
        assert_eq!(ocf(&[]), Ok(None));
    }
}
