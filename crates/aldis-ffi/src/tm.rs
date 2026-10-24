//! C ABI of the TM Transfer Frame codec `aldis-tm` (CCSDS 132.0-B-3; M2 plan D4, D14;
//! CABI-010…012, CABI-020…022, CABI-033).
//!
//! Calls: [`aldis_tm_config_check`] and [`aldis_tm_mc_config_check`]; [`aldis_tm_peek_length`]
//! and [`aldis_tm_header_decode`]; the decoders [`aldis_tm_decode`], [`aldis_tm_decode_with`],
//! [`aldis_tm_decode_unchecked`], and [`aldis_tm_decode_unchecked_with`]; the check against
//! the managed parameters [`aldis_tm_validate`]; the one-shot [`aldis_tm_encode`], which takes
//! the frame's variable fields in an [`aldis_tm_content_t`]; the two-phase writer as [`aldis_tm_writer_begin`] and
//! [`aldis_tm_writer_finish`]; and the OID frame writer as [`aldis_tm_oid_writer_begin`] and
//! [`aldis_tm_oid_writer_finish`].
//!
//! **Channels.** A physical channel is an [`aldis_tm_config_t`] (132.0 §5.1 Table 5-1), the
//! configuration that `peek_length` takes (D14). A master channel is an
//! [`aldis_tm_mc_config_t`], which holds its physical channel, with an array of
//! [`aldis_tm_vc_config_t`] and its count (Tables 5-2, 5-3, 6-1; D14): the C form of
//! `TmMasterChannelConfig`, built and checked on every call. The array's stride is the
//! `struct_size` of its structs, which must all be the same (`abi::configs`).
//!
//! **Security layouts** (API-070; decisions of #90 and #94, c-abi.md §11). The `_with`
//! decoders take the Security Header and Trailer lengths of a frame's virtual channel from the
//! master channel, as `TmFrame::decode_with` does with a `TmMasterChannelConfig` as its
//! `VcLookup`; their plain forms take the physical channel alone and decode as on a channel
//! without SDLS, as `TmFrame::decode` does with `NoSdls`. Every other call that takes a master
//! channel, [`aldis_tm_validate`] among them, uses its layouts. A physical
//! channel that carries several master channels is decoded by reading the SCID first with
//! [`aldis_tm_header_decode`].
//!
//! **Writers.** No call keeps a pointer or state (API-023, API-060): `_writer_begin` writes the
//! headers and zero-fills the frame, the caller fills the fields at the offsets of the
//! [`aldis_tm_layout_t`] it returns, the OCF included, and `_writer_finish`, given the same
//! arguments, rewrites the headers and computes the FECF (`TmFrameWriter::resume`).

use core::ops::Range;

use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::layout::{FrameLayout, OCF_LEN};
use aldis::core::ocf::Ocf;
use aldis::core::security::SecurityLayout;
use aldis::core::{Error, ErrorDetail, ErrorKind, Result};
use aldis::tm::{
    DataFieldContent, DataFieldStatus, FirstHeaderPointer, FshLen, NoSdls, PacketOffset,
    PacketPointer, SegmentLengthId, TmFrame, TmFrameContent, TmFrameFields, TmFrameWriter,
    TmMasterChannelConfig, TmPhysicalChannelConfig, TmPrimaryHeader, TmVirtualChannelConfig,
    encode,
};

use crate::abi::{self, Configs, invalid_argument};
use crate::oid::{GeneratorState, aldis_oid_generator_t, generator};

/// Length of the TM Transfer Frame Primary Header in octets (132.0 §4.1.2.1).
pub const ALDIS_TM_PRIMARY_HEADER_LEN: u32 = 6;
/// Shortest Transfer Frame Secondary Header, in octets, Identification Field included
/// (132.0 §4.1.3.2.3.2; TM-003). A `fsh_len` of 0 means no Secondary Header.
pub const ALDIS_TM_FSH_MIN_LEN: u8 = 2;
/// Longest Transfer Frame Secondary Header, in octets (132.0 §4.1.3.2.3.2; TM-003).
pub const ALDIS_TM_FSH_MAX_LEN: u8 = 64;
/// First Header Pointer `11111111111`: no packet starts in the data field (132.0
/// §4.1.2.7.6.4; TM-008).
pub const ALDIS_TM_FHP_NO_PACKET_START: u16 = 0x7FF;
/// First Header Pointer `11111111110`: the data field holds only Idle Data (132.0
/// §4.1.2.7.6.5; TM-008, TM-010).
pub const ALDIS_TM_FHP_ONLY_IDLE_DATA: u16 = 0x7FE;

/// Data Field Content: Packets, and Idle Data in OID frames; Synchronization Flag `0`
/// (132.0 §4.1.4.4, Table 5-3).
pub const ALDIS_TM_CONTENT_PACKETS: u8 = 0;
/// Data Field Content: one `VCA_SDU` per frame; Synchronization Flag `1` (132.0 §4.1.4.4,
/// Table 5-3).
pub const ALDIS_TM_CONTENT_VCA_SDU: u8 = 1;

/// Pointer, in a view: Synchronization Flag `1`, where the First Header Pointer is not defined
/// and is given raw (132.0 §4.1.2.7.6.2; TM-007). Not valid in [`aldis_tm_fields_t`].
pub const ALDIS_TM_POINTER_NONE: u8 = 0;
/// Pointer: a packet starts at `packet_offset` in the data field (132.0 §4.1.2.7.6.3; TM-008).
pub const ALDIS_TM_POINTER_STARTS_AT: u8 = 1;
/// Pointer: no packet starts in the data field, FHP `0x7FF` (132.0 §4.1.2.7.6.4; TM-008).
pub const ALDIS_TM_POINTER_NO_PACKET_START: u8 = 2;
/// Pointer: Only Idle Data, FHP `0x7FE` (132.0 §4.1.2.7.6.5; TM-008, TM-010).
pub const ALDIS_TM_POINTER_ONLY_IDLE_DATA: u8 = 3;

/// Anomaly bit: Packet Order Flag `1` with the Synchronization Flag `0` (bit 0 of `TmAnomaly`;
/// 132.0 §4.1.2.7.4; TM-007, API-032).
pub const ALDIS_TM_ANOMALY_PACKET_ORDER_FLAG_SET: u32 = 1 << 0;
/// Anomaly bit: Segment Length Identifier other than `11` with the Synchronization Flag `0`
/// (bit 1; 132.0 §4.1.2.7.5.2; TM-007, API-032).
pub const ALDIS_TM_ANOMALY_SEGMENT_LENGTH_ID_NOT_11: u32 = 1 << 1;

/// Managed parameters of a physical channel (132.0 §5.1 Table 5-1; TM-001, API-041,
/// CABI-033). The configuration `aldis_tm_peek_length` takes (M2 plan D14).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_config_t {
    /// `sizeof(aldis_tm_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// 1 if every frame ends with a Frame Error Control Field, else 0 (132.0 §4.1.6.1.2).
    pub fecf: u8,
    /// Transfer Frame Length in octets: at least the Primary Header, one data octet, and the
    /// FECF (132.0 §4.1.1.2; CMN-041).
    pub frame_len: usize,
}

/// Managed parameters of a master channel (132.0 §5.2 Table 5-2; TM-011, API-040, CABI-033).
///
/// Its virtual channels are passed beside it, as an array of [`aldis_tm_vc_config_t`] and its
/// count (M2 plan D14).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_mc_config_t {
    /// `sizeof(aldis_tm_mc_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// Spacecraft Identifier, 0 to 1023 (132.0 §4.1.2.2.3).
    pub scid: u16,
    /// `MC_FSH` length in octets, `ALDIS_TM_FSH_MIN_LEN` to `ALDIS_TM_FSH_MAX_LEN`, or 0 for none
    /// (132.0 §4.1.3.2.3, Table 5-2).
    pub fsh_len: u8,
    /// 1 for an `MC_OCF`, else 0 (132.0 §4.1.5.2, Table 5-2).
    pub ocf: u8,
    /// The physical channel, with its own `struct_size` (CABI-033).
    pub pc: aldis_tm_config_t,
}

/// Managed parameters of a virtual channel (132.0 §5.3 Table 5-3, §6.6 Table 6-1; API-070,
/// CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_vc_config_t {
    /// `sizeof(aldis_tm_vc_config_t)`, set by the caller, the same for every struct of an
    /// array (CABI-033).
    pub struct_size: u32,
    /// Virtual Channel Identifier, 0 to 7 (132.0 §4.1.2.3).
    pub vcid: u8,
    /// Data Field Content, `ALDIS_TM_CONTENT_*` (132.0 §4.1.4.4, Table 5-3).
    pub content: u8,
    /// `VC_FSH` length in octets, `ALDIS_TM_FSH_MIN_LEN` to `ALDIS_TM_FSH_MAX_LEN`, or 0 for none
    /// (Table 5-3).
    pub fsh_len: u8,
    /// 1 for a `VC_OCF`, else 0 (Table 5-3).
    pub ocf: u8,
    /// Security Header length in octets, at most 64; 0 without SDLS (Table 6-1; API-070).
    pub security_header_len: u8,
    /// Security Trailer length in octets; 0 without SDLS (Table 6-1; API-070).
    pub security_trailer_len: u16,
}

/// The Primary Header fields a sender chooses (132.0 §4.1.2; TM-009). The SCID, the OCF Flag,
/// and the Secondary Header Flag come from the configuration (API-021).
///
/// With `sync_flag` 0 the data field status is `pointer` (and `packet_offset` for
/// `ALDIS_TM_POINTER_STARTS_AT`); with `sync_flag` 1 it is `packet_order_flag`,
/// `segment_length_id`, and `first_header_pointer`, raw. The members of the other value of the
/// flag are ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_fields_t {
    /// Virtual Channel Identifier, which must be configured (132.0 §4.1.2.3).
    pub vcid: u8,
    /// Master Channel Frame Count (132.0 §4.1.2.5).
    pub mc_frame_count: u8,
    /// Virtual Channel Frame Count (132.0 §4.1.2.6).
    pub vc_frame_count: u8,
    /// Synchronization Flag, 0 or 1; it must match the VC's Data Field Content (132.0
    /// §4.1.2.7.3).
    pub sync_flag: u8,
    /// With `sync_flag` 0: `ALDIS_TM_POINTER_STARTS_AT`, `_NO_PACKET_START`, or
    /// `_ONLY_IDLE_DATA` (132.0 §4.1.2.7.6).
    pub pointer: u8,
    /// With `sync_flag` 1: Packet Order Flag, 0 or 1 (132.0 §4.1.2.7.4).
    pub packet_order_flag: u8,
    /// With `sync_flag` 1: Segment Length Identifier, 0 to 3 (132.0 §4.1.2.7.5).
    pub segment_length_id: u8,
    /// With `ALDIS_TM_POINTER_STARTS_AT`: the offset of the first packet header in the data
    /// field, inside it and at most `0x7FD` (132.0 §4.1.2.7.6.3).
    pub packet_offset: u16,
    /// With `sync_flag` 1: First Header Pointer, 0 to `0x7FF`, raw (132.0 §4.1.2.7.6.2).
    pub first_header_pointer: u16,
}

/// The variable fields of a frame for [`aldis_tm_encode`], as `(pointer, length)` buffers
/// (132.0 §4.1, §6.3; TM-009, API-072, CABI-011; maintainer decision on #94).
///
/// Each buffer has exactly the length the configuration gives its field, and may be `NULL`
/// with length 0. The struct is read once per call and no pointer is kept (API-060).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_tm_content_t {
    /// The Secondary Header Data Field, after the Identification Field the library writes;
    /// empty without a Secondary Header (132.0 §4.1.3.3).
    pub fsh_data: *const u8,
    /// Length of `fsh_data`.
    pub fsh_data_len: usize,
    /// The Security Header, as a security hook filled it; empty without SDLS (132.0 §6.3.4).
    pub security_header: *const u8,
    /// Length of `security_header`.
    pub security_header_len: usize,
    /// The Transfer Frame Data Field, filling it exactly (132.0 §4.1.4.2).
    pub data: *const u8,
    /// Length of `data`.
    pub data_len: usize,
    /// The Security Trailer; empty without SDLS (132.0 §6.3.6).
    pub security_trailer: *const u8,
    /// Length of `security_trailer`.
    pub security_trailer_len: usize,
    /// The Operational Control Field: `ALDIS_OCF_LEN` octets when the frame carries one, else
    /// empty (132.0 §4.1.5).
    pub ocf: *const u8,
    /// Length of `ocf`: `ALDIS_OCF_LEN` or 0.
    pub ocf_len: usize,
}

impl Default for aldis_tm_content_t {
    /// Every buffer empty and `NULL`.
    fn default() -> aldis_tm_content_t {
        aldis_tm_content_t {
            fsh_data: core::ptr::null(),
            fsh_data_len: 0,
            security_header: core::ptr::null(),
            security_header_len: 0,
            data: core::ptr::null(),
            data_len: 0,
            security_trailer: core::ptr::null(),
            security_trailer_len: 0,
            ocf: core::ptr::null(),
            ocf_len: 0,
        }
    }
}

impl aldis_tm_content_t {
    /// The five buffers, which an output buffer must not overlap (CABI-022).
    const fn buffers(&self) -> [(*const u8, usize); 5] {
        [
            (self.fsh_data, self.fsh_data_len),
            (self.security_header, self.security_header_len),
            (self.data, self.data_len),
            (self.security_trailer, self.security_trailer_len),
            (self.ocf, self.ocf_len),
        ]
    }
}

/// A TM Transfer Frame Primary Header as plain values (132.0 §4.1.2; TM-002, API-014).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_header_t {
    /// Spacecraft Identifier (132.0 §4.1.2.2.3).
    pub scid: u16,
    /// Virtual Channel Identifier (132.0 §4.1.2.3).
    pub vcid: u8,
    /// Operational Control Field Flag (132.0 §4.1.2.4).
    pub ocf_flag: bool,
    /// Master Channel Frame Count (132.0 §4.1.2.5).
    pub mc_frame_count: u8,
    /// Virtual Channel Frame Count (132.0 §4.1.2.6).
    pub vc_frame_count: u8,
    /// Transfer Frame Secondary Header Flag (132.0 §4.1.2.7.2).
    pub fsh_flag: bool,
    /// Synchronization Flag (132.0 §4.1.2.7.3).
    pub sync_flag: bool,
    /// Packet Order Flag, raw (132.0 §4.1.2.7.4).
    pub packet_order_flag: bool,
    /// Segment Length Identifier, raw (132.0 §4.1.2.7.5).
    pub segment_length_id: u8,
    /// First Header Pointer, raw (132.0 §4.1.2.7.6).
    pub first_header_pointer: u16,
}

/// Where each field of a frame lies (132.0 §4.1, §6.3; CMN-040, API-072, CABI-012).
///
/// Offsets and lengths are in octets from the start of the frame. An absent field has length 0
/// at the offset where it would start.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_layout_t {
    /// Transfer Frame Length: the octets the frame takes (132.0 §4.1.1.2; API-012).
    pub frame_len: usize,
    /// Offset of the Secondary Header Data Field, after the Identification Field.
    pub fsh_data_offset: usize,
    /// Length of the Secondary Header Data Field; 0 without a Secondary Header (§4.1.3.3).
    pub fsh_data_len: usize,
    /// Offset of the Security Header.
    pub security_header_offset: usize,
    /// Length of the Security Header; 0 without SDLS (§6.3.4).
    pub security_header_len: usize,
    /// Offset of the Transfer Frame Data Field.
    pub data_offset: usize,
    /// Length of the Transfer Frame Data Field, never 0 (§4.1.4).
    pub data_len: usize,
    /// Offset of the Security Trailer.
    pub security_trailer_offset: usize,
    /// Length of the Security Trailer; 0 without SDLS (§6.3.6).
    pub security_trailer_len: usize,
    /// Offset of the Operational Control Field.
    pub ocf_offset: usize,
    /// Length of the Operational Control Field: `ALDIS_OCF_LEN` or 0 (§4.1.5).
    pub ocf_len: usize,
    /// Offset of the Frame Error Control Field.
    pub fecf_offset: usize,
    /// Length of the Frame Error Control Field: 2 or 0 (§4.1.6).
    pub fecf_len: usize,
}

/// A decoded TM Transfer Frame: its fields and where they lie in the input (132.0 §4.1, §6.3;
/// TM-001…TM-008, API-010, CABI-012). The OCF is classified with `aldis_ocf_classify` on its
/// octets (CMN-010).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_tm_frame_t {
    /// The Primary Header (132.0 §4.1.2; API-014).
    pub header: aldis_tm_header_t,
    /// The First Header Pointer classified, `ALDIS_TM_POINTER_*`: `ALDIS_TM_POINTER_NONE` with
    /// the Synchronization Flag `1` (132.0 §4.1.2.7.6; TM-008).
    pub pointer: u8,
    /// With `ALDIS_TM_POINTER_STARTS_AT`, the offset of the first packet header in the data
    /// field; else 0.
    pub packet_offset: u16,
    /// Total Secondary Header length from its Identification Field; 0 without one (132.0
    /// §4.1.3.2.3; TM-003).
    pub fsh_len: u8,
    /// Whether the frame has an FECF (132.0 §4.1.6; TM-005).
    pub has_fecf: bool,
    /// The FECF, when `has_fecf`.
    pub fecf: u16,
    /// Lenient-mode anomaly flags, `ALDIS_TM_ANOMALY_*`; 0 in strict mode (TM-007, API-032).
    pub anomalies: u32,
    /// Where each field lies in the input.
    pub layout: aldis_tm_layout_t,
}

/// The physical channel of a C struct, with its `struct_size` checked as an embedded struct's
/// (132.0 §5.1; CABI-033).
fn physical(c: &aldis_tm_config_t) -> Result<TmPhysicalChannelConfig> {
    if usize::try_from(c.struct_size).map_or(true, |n| n < size_of::<aldis_tm_config_t>()) {
        return Err(invalid_argument());
    }
    TmPhysicalChannelConfig::new(c.frame_len, abi::flag(c.fecf)?)
}

/// A Secondary Header length of a C struct: 0 for none (Tables 5-2, 5-3; RS-020).
fn fsh_len(octets: u8) -> Result<Option<FshLen>> {
    match octets {
        0 => Ok(None),
        n => FshLen::try_from(n).map(Some),
    }
}

/// The virtual channel of a C struct, each parameter parsed into its type (Table 5-3,
/// Table 6-1; RS-020).
fn virtual_channel(v: &aldis_tm_vc_config_t) -> Result<TmVirtualChannelConfig> {
    let vcid = VcId::try_from(v.vcid)?;
    let content = match v.content {
        ALDIS_TM_CONTENT_PACKETS => DataFieldContent::Packets,
        ALDIS_TM_CONTENT_VCA_SDU => DataFieldContent::VcaSdu,
        _ => return Err(invalid_argument()),
    };
    let mut vc = TmVirtualChannelConfig::new(vcid, content);
    if let Some(len) = fsh_len(v.fsh_len)? {
        vc = vc.with_fsh(len);
    }
    if abi::flag(v.ocf)? {
        vc = vc.with_ocf();
    }
    let security = SecurityLayout::new(
        usize::from(v.security_header_len),
        usize::from(v.security_trailer_len),
    )?;
    Ok(vc.with_security(security))
}

/// The master channel of C structs: its parameters, then each virtual channel in array order
/// (Tables 5-1 to 5-3; TM-011, API-040).
fn master_channel(
    mc: &aldis_tm_mc_config_t,
    vcs: &Configs<'_, aldis_tm_vc_config_t>,
) -> Result<TmMasterChannelConfig> {
    let pc = physical(&mc.pc)?;
    let scid = SpacecraftId::try_from(mc.scid)?;
    let fsh = fsh_len(mc.fsh_len)?;
    let mut config = TmMasterChannelConfig::new(pc, scid, fsh, abi::flag(mc.ocf)?);
    for i in 0..vcs.len() {
        config = config.with_vc(virtual_channel(&vcs.get(i)?)?)?;
    }
    Ok(config)
}

/// The master channel at `mc` with the `vc_count` virtual channels at `vcs` (CABI-022,
/// CABI-033).
///
/// # Safety
/// As for `abi::config` and `abi::configs`.
#[expect(unsafe_code, reason = "checked access to caller memory (CABI-022)")]
unsafe fn master_channel_at(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
) -> Result<TmMasterChannelConfig> {
    // SAFETY: forwarded from the caller.
    let (mc, vcs) = unsafe { (abi::config(mc)?, abi::configs(vcs, vc_count)?) };
    master_channel(&mc, &vcs)
}

/// The sender's fields of a C struct, and a packet offset that no [`PacketOffset`] holds.
///
/// Such an offset (above `0x7FD`) has no [`PacketPointer`]. Before RS-020 gave the offset its
/// type, it reached the writer, which rejected it; [`Fields::checked`] returns the error the
/// writer returned, where the writer returned it, so the C ABI behaves as before (CABI-020,
/// TM-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fields {
    /// The fields; with an offset beyond, the pointer stands in as `NoPacketStart`.
    fields: TmFrameFields,
    /// The packet offset no `PacketOffset` holds, if the C struct gave one.
    offset_beyond: Option<u16>,
}

impl Fields {
    /// The fields for the writers, or for an offset beyond, the error of the writer's checks of
    /// `fields` on `mc`, in their order: the VC, the Synchronization Flag, then the offset
    /// (132.0 §4.1.2.7.6.3; TM-009).
    fn checked(&self, mc: &TmMasterChannelConfig) -> Result<&TmFrameFields> {
        let Some(n) = self.offset_beyond else {
            return Ok(&self.fields);
        };
        let vcid = self.fields.vcid;
        Err(match (mc.vc(vcid), mc.frame_layout(vcid)) {
            (Some(vc), Some(_)) if vc.content().sync_flag() => Error::new(ErrorKind::InvalidField)
                .with_field(aldis::tm::field::SYNC_FLAG)
                .with_detail(ErrorDetail::Expected {
                    expected: u64::from(vc.content().sync_flag()),
                    found: u64::from(false),
                }),
            (Some(_), Some(layout)) => {
                let limit = layout
                    .data()
                    .len()
                    .min(usize::from(FirstHeaderPointer::ONLY_IDLE_DATA.get()))
                    .saturating_sub(1);
                Error::out_of_range(
                    aldis::tm::field::FIRST_HEADER_POINTER,
                    u64::try_from(limit).unwrap_or(u64::MAX),
                    u64::from(n),
                )
            }
            _ => Error::new(ErrorKind::UnknownChannel).with_field(aldis::tm::field::VC_ID),
        })
    }
}

/// The sender's fields of a C struct, each parsed into its type (132.0 §4.1.2; RS-020).
fn fields(f: aldis_tm_fields_t) -> Result<Fields> {
    let mut offset_beyond = None;
    let vcid = VcId::try_from(f.vcid)?;
    let status = if abi::flag(f.sync_flag)? {
        DataFieldStatus::VcaSdu {
            packet_order_flag: abi::flag(f.packet_order_flag)?,
            segment_length_id: SegmentLengthId::try_from(f.segment_length_id)?,
            first_header_pointer: FirstHeaderPointer::try_from(f.first_header_pointer)?,
        }
    } else {
        DataFieldStatus::Packets(match f.pointer {
            ALDIS_TM_POINTER_STARTS_AT => {
                if let Ok(offset) = PacketOffset::try_from(f.packet_offset) {
                    PacketPointer::StartsAt(offset)
                } else {
                    offset_beyond = Some(f.packet_offset);
                    PacketPointer::NoPacketStart
                }
            }
            ALDIS_TM_POINTER_NO_PACKET_START => PacketPointer::NoPacketStart,
            ALDIS_TM_POINTER_ONLY_IDLE_DATA => PacketPointer::OnlyIdleData,
            _ => return Err(invalid_argument()),
        })
    };
    Ok(Fields {
        fields: TmFrameFields::new(vcid, f.mc_frame_count, f.vc_frame_count, status),
        offset_beyond,
    })
}

/// The fields of an Only Idle Data frame, as `TmFrameWriter::begin_oid` sets them (TM-010).
fn oid_fields(vcid: u8, mc_frame_count: u8, vc_frame_count: u8) -> Result<TmFrameFields> {
    Ok(TmFrameFields::new(
        VcId::try_from(vcid)?,
        mc_frame_count,
        vc_frame_count,
        DataFieldStatus::Packets(PacketPointer::OnlyIdleData),
    ))
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
const fn header(h: &TmPrimaryHeader) -> aldis_tm_header_t {
    aldis_tm_header_t {
        scid: h.scid.get(),
        vcid: h.vcid.get(),
        ocf_flag: h.ocf_flag,
        mc_frame_count: h.mc_frame_count,
        vc_frame_count: h.vc_frame_count,
        fsh_flag: h.fsh_flag,
        sync_flag: h.sync_flag,
        packet_order_flag: h.packet_order_flag,
        segment_length_id: h.segment_length_id.get(),
        first_header_pointer: h.first_header_pointer.get(),
    }
}

/// The offset and length of a range.
fn span(r: &Range<usize>) -> (usize, usize) {
    (r.start, r.len())
}

/// The C form of a frame layout; the Secondary Header Data Field follows its Identification
/// Field (132.0 §4.1.3; CMN-040).
fn layout(l: &FrameLayout) -> aldis_tm_layout_t {
    let fsh = l.secondary();
    let fsh_data = fsh.start.saturating_add(1).min(fsh.end)..fsh.end;
    let (fsh_data_offset, fsh_data_len) = span(&fsh_data);
    let (security_header_offset, security_header_len) = span(&l.security_header());
    let (data_offset, data_len) = span(&l.data());
    let (security_trailer_offset, security_trailer_len) = span(&l.security_trailer());
    let (ocf_offset, ocf_len) = span(&l.ocf());
    let (fecf_offset, fecf_len) = span(&l.fecf());
    aldis_tm_layout_t {
        frame_len: l.frame_len(),
        fsh_data_offset,
        fsh_data_len,
        security_header_offset,
        security_header_len,
        data_offset,
        data_len,
        security_trailer_offset,
        security_trailer_len,
        ocf_offset,
        ocf_len,
        fecf_offset,
        fecf_len,
    }
}

/// The C view of a decoded frame.
pub(crate) fn view(f: &TmFrame<'_>) -> aldis_tm_frame_t {
    let (pointer, packet_offset) = match f.data_field_status() {
        DataFieldStatus::Packets(PacketPointer::StartsAt(n)) => {
            (ALDIS_TM_POINTER_STARTS_AT, n.get())
        }
        DataFieldStatus::Packets(PacketPointer::NoPacketStart) => {
            (ALDIS_TM_POINTER_NO_PACKET_START, 0)
        }
        DataFieldStatus::Packets(PacketPointer::OnlyIdleData) => {
            (ALDIS_TM_POINTER_ONLY_IDLE_DATA, 0)
        }
        DataFieldStatus::VcaSdu { .. } => (ALDIS_TM_POINTER_NONE, 0),
    };
    aldis_tm_frame_t {
        header: header(&f.header()),
        pointer,
        packet_offset,
        fsh_len: f.fsh_len().map_or(0, FshLen::get),
        has_fecf: f.fecf().is_some(),
        fecf: f.fecf().unwrap_or(0),
        anomalies: f.anomalies().bits(),
        layout: layout(f.layout()),
    }
}

/// Checks a physical channel configuration (132.0 §5.1 Table 5-1; TM-001, API-040, CMN-041).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`frame_len`) for a frame too short for the Primary Header, one
/// data octet, and the FECF, with that minimum; CABI-022 violations.
///
/// # Safety
/// `pc` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_config_check(
    pc: *const aldis_tm_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || physical(&abi::config(pc)?).map(drop)) }
}

/// Checks a master channel configuration and its virtual channels (132.0 §5 Tables 5-1 to 5-3,
/// §6.6 Table 6-1; TM-011, API-040, API-070, CMN-041).
///
/// The physical channel is checked first, then the SCID, the `MC_FSH`, the `MC_OCF` flag, and
/// each virtual channel in array order: its VCID, content, `VC_FSH`, `VC_OCF` flag, and
/// security layout, and then whether it fits the master channel.
///
/// # Errors
/// The errors of [`aldis_tm_config_check`]; `ALDIS_ERR_OUT_OF_RANGE` (`spacecraft_id`, `vc_id`,
/// `fsh_len`); `ALDIS_ERR_INVALID_CONFIG` (`security_header_len`) for a Security Header above 64
/// octets, and (`fsh_len`, `ocf_flag`, `frame_data_field`, `vc_id`) for a Secondary Header or an
/// OCF on both the master and the virtual channel, a frame with no data octet left, or a VCID
/// configured twice (132.0 §4.1.3.1.4, §4.1.5.2); CABI-022 violations, including a VC struct
/// whose `struct_size` differs from the first one's.
///
/// # Safety
/// `mc` is valid for a read; `vcs` is valid for reads of `vc_count` structs of the first one's
/// `struct_size` (or `NULL` with 0); `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_mc_config_check(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || master_channel_at(mc, vcs, vc_count).map(drop)) }
}

/// The length of the frame that `prefix` begins: the configured frame length (132.0 §4.1.1.2;
/// API-013, M2 plan D14).
///
/// Any prefix will do; if it holds the first octet, its Transfer Frame Version Number is
/// checked as [`aldis_tm_decode`] checks it.
///
/// # Errors
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`) for a version other than `00` (TM-002);
/// the errors of [`aldis_tm_config_check`]; CABI-022 violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `cfg` is valid for a read;
/// `out_len` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_peek_length(
    prefix: *const u8,
    prefix_len: usize,
    cfg: *const aldis_tm_config_t,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let pc = physical(&abi::config(cfg)?)?;
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out_len = abi::out(out_len)?;
            out_len.write(TmFrame::peek_length(prefix, &pc)?);
            Ok(())
        })
    }
}

/// Decodes the Primary Header that `prefix` begins (132.0 §4.1.2; TM-002, API-014).
///
/// The rest of the frame is not read: for example to find the SCID of a frame on a physical
/// channel that carries several master channels.
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (`needed` 6) for a prefix shorter than the header;
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`) for a version other than `00` (TM-002);
/// CABI-022 violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `out` is valid for one write;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_header_decode(
    prefix: *const u8,
    prefix_len: usize,
    out: *mut aldis_tm_header_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out = abi::out(out)?;
            out.write(header(&TmPrimaryHeader::decode(prefix)?));
            Ok(())
        })
    }
}

/// Decodes one frame of a physical channel on which no virtual channel uses SDLS (132.0 §4.1;
/// TM-001…TM-008, API-010, API-012, API-030), as `TmFrame::decode` does.
///
/// The Secondary Header and the OCF are placed by the frame's own flags; whether they match a
/// configuration is checked by [`aldis_tm_validate`] (TM-011).
///
/// # Errors
/// In this order: `ALDIS_ERR_LENGTH_MISMATCH` (`frame_len`) if `len` is not the frame length
/// (TM-001); `ALDIS_ERR_CRC_MISMATCH` for a wrong FECF (TM-005);
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`frame_version`, then `fsh_version`); `ALDIS_ERR_INVALID_FIELD`
/// (`fsh_len`) for a Secondary Header Length of 0 and `ALDIS_ERR_MALFORMED` (`fsh_len`) for one
/// past the frame (TM-003); `ALDIS_ERR_MALFORMED` (`frame_data_field`) if no data octet is left
/// (TM-006) and (`first_header_pointer`) for a pointer outside the data field (TM-008); in
/// strict mode `ALDIS_ERR_RESERVED_VALUE` (`packet_order_flag`) and `ALDIS_ERR_INVALID_FIELD`
/// (`segment_length_id`) with the Synchronization Flag `0` (TM-007); the errors of
/// [`aldis_tm_config_check`]; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `cfg` is valid for a read; `out` is
/// valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_decode(
    bytes: *const u8,
    len: usize,
    cfg: *const aldis_tm_config_t,
    mode: u32,
    out: *mut aldis_tm_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let pc = physical(&abi::config(cfg)?)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(view(&TmFrame::decode(bytes, &pc, mode)?));
            Ok(())
        })
    }
}

/// Decodes one frame of the master channel `mc` with the security layout of its virtual
/// channel (132.0 §4.1, §6.3; TM-001…TM-008, API-012, API-070).
///
/// The Security Header and Trailer lengths come from `vcs`, as `TmFrame::decode_with` takes
/// them from a `TmMasterChannelConfig`. The frame length and FECF are those of `mc.pc`.
///
/// # Errors
/// As [`aldis_tm_decode`], with, after the version check, `ALDIS_ERR_UNKNOWN_CHANNEL`
/// (`spacecraft_id`, with the configured and found SCID, or `vc_id`) for a channel `mc` does not
/// configure; the errors of [`aldis_tm_mc_config_check`].
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `mc` and `vcs` as for
/// [`aldis_tm_mc_config_check`]; `out` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_decode_with(
    bytes: *const u8,
    len: usize,
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    mode: u32,
    out: *mut aldis_tm_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(view(&TmFrame::decode_with(bytes, mc.pc(), &mc, mode)?));
            Ok(())
        })
    }
}

/// Decodes one frame as [`aldis_tm_decode`] does, but without checking the FECF: for
/// diagnostics of frames that failed it (TM-005), as `TmFrame::decode_unchecked` does with
/// `NoSdls`.
///
/// # Errors
/// As [`aldis_tm_decode`], except `ALDIS_ERR_CRC_MISMATCH`.
///
/// # Safety
/// As [`aldis_tm_decode`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_decode_unchecked(
    bytes: *const u8,
    len: usize,
    cfg: *const aldis_tm_config_t,
    mode: u32,
    out: *mut aldis_tm_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let pc = physical(&abi::config(cfg)?)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(view(&TmFrame::decode_unchecked(bytes, &pc, &NoSdls, mode)?));
            Ok(())
        })
    }
}

/// Decodes one frame as [`aldis_tm_decode_with`] does, but without checking the FECF (TM-005),
/// as `TmFrame::decode_unchecked` does with a `TmMasterChannelConfig`.
///
/// # Errors
/// As [`aldis_tm_decode_with`], except `ALDIS_ERR_CRC_MISMATCH`.
///
/// # Safety
/// As [`aldis_tm_decode_with`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_decode_unchecked_with(
    bytes: *const u8,
    len: usize,
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    mode: u32,
    out: *mut aldis_tm_frame_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(view(&TmFrame::decode_unchecked(bytes, mc.pc(), &mc, mode)?));
            Ok(())
        })
    }
}

/// Checks a frame against the managed parameters of a master channel (132.0 §5 Tables 5-1 to
/// 5-3, §6.6 Table 6-1; TM-011, API-070).
///
/// The frame is decoded as [`aldis_tm_decode_with`] decodes it, with the security layout of its
/// virtual channel from `mc`, and checked as `TmFrame::validate` checks it. A call that takes a
/// master channel always uses its layouts (c-abi.md §11).
///
/// # Errors
/// The errors of [`aldis_tm_decode_with`]; then, in this order, each with the configured and
/// the found value: `ALDIS_ERR_INVALID_FIELD` (`ocf_flag`, `fsh_flag`, `sync_flag`) for a flag
/// that disagrees with the configuration, and `ALDIS_ERR_LENGTH_MISMATCH` (`fsh_len`) for
/// another Secondary Header length.
///
/// # Safety
/// As [`aldis_tm_decode_with`], without `out`.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_validate(
    bytes: *const u8,
    len: usize,
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    mode: u32,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let bytes = abi::bytes(bytes, len)?;
            let frame = TmFrame::decode_with(bytes, mc.pc(), &mc, abi::mode(mode)?)?;
            frame.validate(&mc)
        })
    }
}

/// Encodes one frame of `fields->vcid` into the start of `out` in one call (132.0 §4.1, §6.3;
/// TM-009, API-020…022, API-072), as `aldis_tm::encode` does.
///
/// `content` gives the variable fields, each exactly the length the configuration gives it
/// ([`aldis_tm_content_t`]). The library writes the Identification Field, the OCF Flag, and the
/// FECF (API-021).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_tm_mc_config_check`]; then
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`vc_id`); `ALDIS_ERR_INVALID_FIELD` (`sync_flag`) for a status
/// that does not match the VC's content; `ALDIS_ERR_OUT_OF_RANGE` (`first_header_pointer`) for
/// a packet offset outside the data field or above `0x7FD`; `ALDIS_ERR_LENGTH_MISMATCH`
/// (`fsh_len`, `security_header_len`, `frame_data_field`, `security_trailer_len`) for a field
/// of another length than the configured one; `ALDIS_ERR_INVALID_FIELD` (`ocf_flag`) for an OCF
/// given where none is configured or missing where one is; `ALDIS_ERR_BUFFER_TOO_SMALL`
/// (`needed` = frame length); `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `segment_length_id`,
/// `first_header_pointer`) for a value of `fields` outside its field; CABI-022 violations,
/// including an `ocf_len` other than 0 or `ALDIS_OCF_LEN` and `out` overlapping a buffer of
/// `content`.
///
/// # Safety
/// `mc` and `vcs` as for [`aldis_tm_mc_config_check`]; `fields` and `content` are valid for a
/// read; each buffer of `content` is valid for its length (or `NULL` with 0); `out` is valid
/// for `cap` octets; `out_len` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_encode(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    fields: *const aldis_tm_fields_t,
    content: *const aldis_tm_content_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let c = abi::read(content)?;
            let content = TmFrameContent {
                secondary_header_data: abi::bytes(c.fsh_data, c.fsh_data_len)?,
                security_header: abi::bytes(c.security_header, c.security_header_len)?,
                data: abi::bytes(c.data, c.data_len)?,
                security_trailer: abi::bytes(c.security_trailer, c.security_trailer_len)?,
                ocf: self::ocf(abi::bytes(c.ocf, c.ocf_len)?)?,
            };
            let out = abi::bytes_mut(out, cap, &c.buffers())?;
            let out_len = abi::out(out_len)?;
            out_len.write(encode(&mc, fields.checked(&mc)?, &content, out)?);
            Ok(())
        })
    }
}

/// Starts a two-phase write of one frame of `fields->vcid` (132.0 §4.1; TM-009, API-023, M2
/// plan D4).
///
/// Checks the arguments, writes the Primary Header and the Secondary Header Identification
/// Field in the first frame-length octets of `out`, zero-fills every other octet of the frame,
/// and reports where each field lies.
///
/// The caller fills the fields at the offsets of `layout`, the OCF included (check a report
/// with `aldis_ocf_check`), and calls [`aldis_tm_writer_finish`] with the same arguments. No
/// state is kept.
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`aldis_tm_mc_config_check`]; then
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`vc_id`), `ALDIS_ERR_INVALID_FIELD` (`sync_flag`),
/// `ALDIS_ERR_OUT_OF_RANGE` (`first_header_pointer`), and `ALDIS_ERR_BUFFER_TOO_SMALL` as for
/// [`aldis_tm_encode`]; CABI-022 violations.
///
/// # Safety
/// `mc` and `vcs` as for [`aldis_tm_mc_config_check`]; `fields` is valid for a read; `out` is
/// valid for `cap` octets; `layout` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_writer_begin(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    fields: *const aldis_tm_fields_t,
    out: *mut u8,
    cap: usize,
    layout: *mut aldis_tm_layout_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let layout = abi::out(layout)?;
            let writer = TmFrameWriter::begin(out, &mc, fields.checked(&mc)?)?;
            layout.write(self::layout(writer.layout()));
            Ok(())
        })
    }
}

/// Finishes a two-phase write begun with the same arguments (132.0 §4.1.6.2; TM-009, API-021,
/// API-023).
///
/// Rewrites the headers, leaves every field the caller filled, computes the FECF if
/// configured, and returns the frame length.
///
/// # Errors
/// As [`aldis_tm_writer_begin`]; on error `out` is unchanged.
///
/// # Safety
/// As [`aldis_tm_writer_begin`]; `out_len` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_writer_finish(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    fields: *const aldis_tm_fields_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let fields = self::fields(abi::read(fields)?)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            out_len.write(TmFrameWriter::resume(out, &mc, fields.checked(&mc)?)?.finish());
            Ok(())
        })
    }
}

/// Starts an Only Idle Data frame on `vcid` (132.0 §4.1.4.6; TM-010, CMN-020, CMN-021).
///
/// The First Header Pointer is `0x7FE` and the data field holds the next octets of the OID
/// generator `oid`, as `TmFrameWriter::begin_oid` writes them; the Galois form gives the same
/// octets (CMN-022). Reports where each field lies, as [`aldis_tm_writer_begin`] does.
///
/// The caller may fill the Secondary Header and the OCF (132.0 §4.1.4.6.3 note 1) and calls
/// [`aldis_tm_oid_writer_finish`] with the same arguments, without `oid`.
///
/// # Errors
/// As [`aldis_tm_writer_begin`]; `ALDIS_ERR_INVALID_FIELD` (`sync_flag`) if the VC carries
/// `VCA_SDUs` (132.0 §4.1.4.6.3); `ALDIS_ERR_INVALID_HANDLE` for an invalid generator; CABI-022
/// violations, including `out` overlapping the generator's storage. On error neither `out` nor
/// the generator is changed (GEN-042).
///
/// # Safety
/// `mc` and `vcs` as for [`aldis_tm_mc_config_check`]; `oid` is a handle from
/// `aldis_oid_generator_init` whose storage is still valid, or detectably invalid storage; `out`
/// is valid for `cap` octets; `layout` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_oid_writer_begin(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    vcid: u8,
    mc_frame_count: u8,
    vc_frame_count: u8,
    oid: *mut aldis_oid_generator_t,
    out: *mut u8,
    cap: usize,
    layout: *mut aldis_tm_layout_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let vcid = VcId::try_from(vcid)?;
            let h = generator(oid)?;
            let out = abi::bytes_mut(out, cap, &[h.range()])?;
            let layout = abi::out(layout)?;
            let mut state = h.load();
            let writer = match &mut state {
                GeneratorState::Fibonacci(g) => {
                    TmFrameWriter::begin_oid(out, &mc, vcid, mc_frame_count, vc_frame_count, g)?
                }
                // `begin_oid` with the Galois form, which gives the same sequence (CMN-022).
                GeneratorState::Galois(g) => {
                    let fields = oid_fields(vcid.get(), mc_frame_count, vc_frame_count)?;
                    let mut writer = TmFrameWriter::begin(out, &mc, &fields)?;
                    g.fill(writer.data_field_mut());
                    writer
                }
            };
            layout.write(self::layout(writer.layout()));
            h.store(state);
            Ok(())
        })
    }
}

/// Finishes an Only Idle Data frame begun with [`aldis_tm_oid_writer_begin`] (132.0 §4.1.4.6,
/// §4.1.6.2; TM-010, API-023).
///
/// Given the same arguments, it rewrites the headers, leaves the Idle Data and every field the
/// caller filled, computes the FECF if configured, and returns the frame length.
///
/// # Errors
/// As [`aldis_tm_writer_finish`] for the fields of an OID frame; on error `out` is unchanged.
///
/// # Safety
/// `mc` and `vcs` as for [`aldis_tm_mc_config_check`]; `out` is valid for `cap` octets;
/// `out_len` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_tm_oid_writer_finish(
    mc: *const aldis_tm_mc_config_t,
    vcs: *const aldis_tm_vc_config_t,
    vc_count: usize,
    vcid: u8,
    mc_frame_count: u8,
    vc_frame_count: u8,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let mc = master_channel_at(mc, vcs, vc_count)?;
            let fields = oid_fields(vcid, mc_frame_count, vc_frame_count)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            out_len.write(TmFrameWriter::resume(out, &mc, &fields)?.finish());
            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::Anomaly as _;
    use aldis::tm::TmAnomaly;

    #[test]
    fn constants_are_the_crate_values() {
        // req: GEN-043, API-033, CABI-010, TM-008
        assert_eq!(
            ALDIS_TM_ANOMALY_PACKET_ORDER_FLAG_SET,
            1 << TmAnomaly::PacketOrderFlagSet.bit()
        );
        assert_eq!(
            ALDIS_TM_ANOMALY_SEGMENT_LENGTH_ID_NOT_11,
            1 << TmAnomaly::SegmentLengthIdNot11.bit()
        );
        assert_eq!(
            usize::try_from(ALDIS_TM_PRIMARY_HEADER_LEN),
            Ok(aldis::tm::header::PRIMARY_HEADER_LEN)
        );
        assert_eq!(
            (ALDIS_TM_FSH_MIN_LEN, ALDIS_TM_FSH_MAX_LEN),
            (FshLen::MIN, FshLen::MAX)
        );
        assert_eq!(
            ALDIS_TM_FHP_NO_PACKET_START,
            FirstHeaderPointer::NO_PACKET_START.get()
        );
        assert_eq!(
            ALDIS_TM_FHP_ONLY_IDLE_DATA,
            FirstHeaderPointer::ONLY_IDLE_DATA.get()
        );
        assert_eq!(core::mem::offset_of!(aldis_tm_config_t, struct_size), 0);
        assert_eq!(core::mem::offset_of!(aldis_tm_mc_config_t, struct_size), 0);
        assert_eq!(core::mem::offset_of!(aldis_tm_vc_config_t, struct_size), 0);
    }

    #[test]
    fn enumerations_take_only_their_constants() {
        // req: CABI-010, CABI-022, TM-009
        let f = aldis_tm_fields_t {
            pointer: ALDIS_TM_POINTER_NONE,
            ..aldis_tm_fields_t::default()
        };
        assert_eq!(fields(f), Err(invalid_argument()));
        let f = aldis_tm_fields_t {
            sync_flag: 2,
            ..aldis_tm_fields_t::default()
        };
        assert_eq!(fields(f), Err(invalid_argument()));
        let vc = aldis_tm_vc_config_t {
            struct_size: 12,
            content: 2,
            ..aldis_tm_vc_config_t::default()
        };
        assert_eq!(virtual_channel(&vc), Err(invalid_argument()));
        assert_eq!(ocf(&[1, 2, 3]), Err(invalid_argument()));
        assert_eq!(ocf(&[]), Ok(None));
        let pc = aldis_tm_config_t {
            struct_size: 4,
            fecf: 0,
            frame_len: 16,
        };
        assert_eq!(physical(&pc), Err(invalid_argument()));
    }
}
