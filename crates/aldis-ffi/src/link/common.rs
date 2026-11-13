//! Helpers and constants shared by the link engines of the C ABI: the C form of an output range
//! (API-061), the receiver constants the extractor, the receiver, and the mux share, and the MAP
//! constants the segmenter and the reassembler share.

use aldis::core::engine::OutRange;
use aldis::core::{Error, ErrorKind, Result, field};
use aldis::link::segmenter::Framing;

use crate::mirror::mirror;
use crate::range::aldis_range_t;

/// The C form of a range of `out`.
pub(crate) const fn range(r: OutRange) -> aldis_range_t {
    aldis_range_t {
        offset: r.start(),
        len: r.len(),
    }
}

/// Where the OCF or the Secondary Header of a frame is delivered (132.0 §3.5–3.9, 732.1
/// §3.11.2.4; LNK-053).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_service {
    /// The part is not delivered.
    ALDIS_LINK_SERVICE_NONE = 0,
    /// The part is delivered for the virtual channel of the frame (`VC_OCF`, `VC_FSH`).
    ALDIS_LINK_SERVICE_VC = 1,
    /// The part is delivered for the master channel (`MC_OCF`, `MC_FSH`).
    ALDIS_LINK_SERVICE_MC = 2,
}
mirror!(pub aldis_link_service: u32 {
    ALDIS_LINK_SERVICE_NONE,
    ALDIS_LINK_SERVICE_VC,
    ALDIS_LINK_SERVICE_MC
});

/// What a virtual channel of a `FrameReceiver` carries (LNK-050, LNK-077).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_content {
    /// Packets (TM, AOS): the data field goes to a `PacketExtractor`.
    ALDIS_LINK_CONTENT_PACKETS = 0,
    /// `VCA_SDU`s (TM, AOS): each data field is one SDU. USLP and TC use this for a served VC.
    ALDIS_LINK_CONTENT_VCA = 1,
    /// `B_PDU`s (AOS): the valid bits of each are delivered.
    ALDIS_LINK_CONTENT_BITSTREAM = 2,
    /// A VC whose service the receiver does not provide: its frames are `NotServed`.
    ALDIS_LINK_CONTENT_UNSERVED = 3,
}
mirror!(pub aldis_link_content: u32 {
    ALDIS_LINK_CONTENT_PACKETS,
    ALDIS_LINK_CONTENT_VCA,
    ALDIS_LINK_CONTENT_BITSTREAM,
    ALDIS_LINK_CONTENT_UNSERVED
});

/// Why a frame was discarded (LNK-050, LNK-080; docs/04-engines.md §5).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_discard {
    /// The VC (or, for USLP, the MAP) is not configured.
    ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL = 0,
    /// The VC is configured, but the receiver does not provide its service.
    ALDIS_LINK_DISCARD_NOT_SERVED = 1,
    /// The FECF did not match (a reason a caller reports).
    ALDIS_LINK_DISCARD_CRC_MISMATCH = 2,
    /// The AOS FHEC could not be corrected (a reason a caller reports).
    ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE = 3,
    /// SDLS verification failed (a reason a caller reports).
    ALDIS_LINK_DISCARD_SECURITY = 4,
}
mirror!(pub aldis_link_discard: u32 {
    ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL,
    ALDIS_LINK_DISCARD_NOT_SERVED,
    ALDIS_LINK_DISCARD_CRC_MISMATCH,
    ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE,
    ALDIS_LINK_DISCARD_SECURITY
});

/// The channel an action of a `FrameReceiver` is about (LNK-051, LNK-053).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_scope {
    /// The master channel.
    ALDIS_LINK_SCOPE_MASTER_CHANNEL = 0,
    /// A virtual channel (`vc_id`).
    ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL = 1,
}
mirror!(pub aldis_link_scope: u32 {
    ALDIS_LINK_SCOPE_MASTER_CHANNEL,
    ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL
});

/// What a MAP carries (232.0 §5.3 Table 5-3; 732.1 §5.4 Table 5-4; LNK-040, LNK-042, LNK-070).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_map_service {
    /// Packets and SDUs alike: a MAP of a TC VC (the segmenter only; the reassembler has none).
    ALDIS_MAP_SERVICE_ANY = 0,
    /// Packets (USLP; the reassembler also on TC).
    ALDIS_MAP_SERVICE_PACKETS = 1,
    /// `MAP_SDUs` or `MAPA_SDUs`.
    ALDIS_MAP_SERVICE_SDU = 2,
    /// An octet stream (USLP rule `011`).
    ALDIS_MAP_SERVICE_OCTET_STREAM = 3,
}
mirror!(pub aldis_map_service: u32 {
    ALDIS_MAP_SERVICE_ANY,
    ALDIS_MAP_SERVICE_PACKETS,
    ALDIS_MAP_SERVICE_SDU,
    ALDIS_MAP_SERVICE_OCTET_STREAM
});

/// How an FDU or TFDF relates to the SDU it carries (232.0 Table 4-2; 732.1 Table 4-3;
/// LNK-041).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_map_framing {
    /// A whole SDU, or several whole packets: flags `11` (TC), rule `111` (USLP).
    ALDIS_MAP_FRAMING_WHOLE = 0,
    /// The first portion of an SDU: flags `01`, rule `100`.
    ALDIS_MAP_FRAMING_FIRST = 1,
    /// A continuing portion: flags `00`, rule `101`.
    ALDIS_MAP_FRAMING_CONTINUING = 2,
    /// The last portion: flags `10`, rule `110`.
    ALDIS_MAP_FRAMING_LAST = 3,
    /// A TFDZ of an octet stream: rule `011` (USLP).
    ALDIS_MAP_FRAMING_STREAM = 4,
}
mirror!(pub aldis_map_framing: u32 {
    ALDIS_MAP_FRAMING_WHOLE,
    ALDIS_MAP_FRAMING_FIRST,
    ALDIS_MAP_FRAMING_CONTINUING,
    ALDIS_MAP_FRAMING_LAST,
    ALDIS_MAP_FRAMING_STREAM
});

pub(super) const fn framing_code(framing: Framing) -> u32 {
    match framing {
        Framing::Whole => ALDIS_MAP_FRAMING_WHOLE,
        Framing::First => ALDIS_MAP_FRAMING_FIRST,
        Framing::Continuing => ALDIS_MAP_FRAMING_CONTINUING,
        Framing::Last => ALDIS_MAP_FRAMING_LAST,
        Framing::Stream => ALDIS_MAP_FRAMING_STREAM,
    }
}

/// The error for a MAP service the protocol does not have (LNK-040, LNK-070).
pub(super) const fn wrong_service() -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(field::DATA_FIELD_CONTENT)
}

/// The MAP table of `count` entries read by `entry`, as the engines' configurations take it.
///
/// # Errors
/// The error of `entry`; `CapacityExceeded` (`map_id`) for a `count` above the capacity.
pub(super) fn listed<T: Copy, C: Copy, const N: usize>(
    entries: &[C; N],
    count: usize,
    entry: impl Fn(&C) -> Result<T>,
    out: &mut [Option<T>; N],
) -> Result<usize> {
    if count > N {
        return Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID));
    }
    for (dst, src) in out.iter_mut().zip(entries).take(count) {
        *dst = Some(entry(src)?);
    }
    Ok(count)
}
