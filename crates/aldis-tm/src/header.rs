//! Primary Header fields, the Transfer Frame Data Field Status, and the Secondary Header length
//! (132.0-B-3 §4.1.2, §4.1.3.2; TM-002, TM-003, TM-007, TM-008, API-001, API-014).

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::{Error, ErrorDetail, ErrorKind, Result};

use crate::field;

/// Length of the Transfer Frame Primary Header, in octets (132.0 §4.1.2.1).
pub const PRIMARY_HEADER_LEN: usize = 6;

/// Transfer Frame Version Number of a TM Transfer Frame, `00` (132.0 §4.1.2.2.2.2; TM-002).
pub const VERSION: u8 = 0b00;

/// Transfer Frame Secondary Header Version Number, `00` (132.0 §4.1.3.2.2.2; TM-003).
pub const FSH_VERSION: u8 = 0b00;

/// `u8` to `usize` in `const` context (`From` is not `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> usize")]
pub(crate) const fn u8_usize(n: u8) -> usize {
    n as usize
}

/// `usize` to `u64` for an error's [`ErrorDetail::Expected`], in `const` context.
#[expect(
    clippy::as_conversions,
    reason = "supported targets are 32- or 64-bit, so usize fits in u64"
)]
pub(crate) const fn len_u64(n: usize) -> u64 {
    n as u64
}

/// An error of `kind` for `field`, with the value expected and the value found (GEN-041).
pub(crate) const fn mismatch(
    kind: ErrorKind,
    field: aldis_core::FieldId,
    expected: u64,
    found: u64,
) -> Error {
    Error::new(kind)
        .with_field(field)
        .with_detail(ErrorDetail::Expected { expected, found })
}

/// Segment Length Identifier, 2 bits (132.0 §4.1.2.7.5; TM-007, API-001).
///
/// With the Synchronization Flag `0` it is `11` ([`SegmentLengthId::NO_SEGMENTS`]); with the flag
/// `1` its value is undefined and any value is carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegmentLengthId(u8);

impl SegmentLengthId {
    /// Largest value, `11` (132.0 §4.1.2.7.5.1).
    pub const MAX: u8 = 0b11;
    /// `11`, the value that marks non-use of Source Packet Segments, required with the
    /// Synchronization Flag `0` (132.0 §4.1.2.7.5.2; TM-007).
    pub const NO_SEGMENTS: SegmentLengthId = SegmentLengthId(0b11);

    /// The identifier, or `None` if `value` needs more than 2 bits (API-001).
    #[must_use]
    pub const fn new(value: u8) -> Option<SegmentLengthId> {
        if value <= SegmentLengthId::MAX {
            Some(SegmentLengthId(value))
        } else {
            None
        }
    }

    /// The numeric value (132.0 §4.1.2.7.5).
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for SegmentLengthId {
    type Error = Error;

    fn try_from(value: u8) -> Result<SegmentLengthId> {
        SegmentLengthId::new(value).ok_or_else(|| {
            Error::out_of_range(
                field::SEGMENT_LENGTH_ID,
                u64::from(SegmentLengthId::MAX),
                u64::from(value),
            )
        })
    }
}

impl From<SegmentLengthId> for u8 {
    fn from(id: SegmentLengthId) -> u8 {
        id.0
    }
}

/// The raw 11-bit First Header Pointer field (132.0 §4.1.2.7.6; TM-008, API-001).
///
/// [`PacketPointer`] is its meaning when the Synchronization Flag is `0`; with the flag `1` it is
/// undefined and carried raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FirstHeaderPointer(u16);

impl FirstHeaderPointer {
    /// Largest value, `11111111111` (132.0 §4.1.2.7.6.1).
    pub const MAX: u16 = 0x7FF;
    /// `11111111111`: no Packet starts in the data field (132.0 §4.1.2.7.6.4).
    pub const NO_PACKET_START: FirstHeaderPointer = FirstHeaderPointer(0x7FF);
    /// `11111111110`: the data field holds only idle data, an OID frame (132.0 §4.1.2.7.6.5,
    /// §4.1.4.6.1).
    pub const ONLY_IDLE_DATA: FirstHeaderPointer = FirstHeaderPointer(0x7FE);

    /// The pointer, or `None` if `value` needs more than 11 bits (API-001).
    #[must_use]
    pub const fn new(value: u16) -> Option<FirstHeaderPointer> {
        if value <= FirstHeaderPointer::MAX {
            Some(FirstHeaderPointer(value))
        } else {
            None
        }
    }

    /// The numeric value (132.0 §4.1.2.7.6).
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for FirstHeaderPointer {
    type Error = Error;

    fn try_from(value: u16) -> Result<FirstHeaderPointer> {
        FirstHeaderPointer::new(value).ok_or_else(|| {
            Error::out_of_range(
                field::FIRST_HEADER_POINTER,
                u64::from(FirstHeaderPointer::MAX),
                u64::from(value),
            )
        })
    }
}

impl From<FirstHeaderPointer> for u16 {
    fn from(fhp: FirstHeaderPointer) -> u16 {
        fhp.0
    }
}

impl From<PacketPointer> for FirstHeaderPointer {
    fn from(pointer: PacketPointer) -> FirstHeaderPointer {
        pointer.to_raw()
    }
}

/// Total length of a Transfer Frame Secondary Header, 2 to 64 octets: the Identification Field
/// and 1 to 63 data octets (132.0 §4.1.3.1.3, §4.1.3.2.3; Tables 5-2 and 5-3; TM-003, API-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FshLen(u8);

impl FshLen {
    /// Shortest Secondary Header: the Identification Field and one data octet (132.0 §4.1.3.1.3).
    pub const MIN: u8 = 2;
    /// Longest Secondary Header, 64 octets (132.0 §4.1.1.1 b, §4.1.3.1.3; Table 5-2).
    pub const MAX: u8 = 64;

    /// The length, or `None` outside 2–64 (API-040).
    #[must_use]
    pub const fn new(octets: u8) -> Option<FshLen> {
        if octets >= FshLen::MIN && octets <= FshLen::MAX {
            Some(FshLen(octets))
        } else {
            None
        }
    }

    /// Total length in octets, Identification Field included (132.0 §4.1.3.2.3.2).
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Total length in octets, as a `usize` (132.0 §4.1.3.2.3.2).
    #[must_use]
    pub const fn octets(self) -> usize {
        u8_usize(self.0)
    }

    /// The Identification Field: version `00` and the total length minus one (132.0
    /// §4.1.3.2.2–3; TM-009).
    #[must_use]
    pub const fn id_octet(self) -> u8 {
        (FSH_VERSION << 6) | self.0.saturating_sub(1)
    }
}

impl TryFrom<u8> for FshLen {
    type Error = Error;

    /// # Errors
    /// `OutOfRange` (`fsh_len`) outside 2–64, with the bound that was crossed.
    fn try_from(octets: u8) -> Result<FshLen> {
        FshLen::new(octets).ok_or_else(|| {
            let bound = if octets < FshLen::MIN {
                FshLen::MIN
            } else {
                FshLen::MAX
            };
            mismatch(
                ErrorKind::OutOfRange,
                field::FSH_LEN,
                u64::from(bound),
                u64::from(octets),
            )
        })
    }
}

impl From<FshLen> for u8 {
    fn from(len: FshLen) -> u8 {
        len.0
    }
}

/// The First Header Pointer of a frame with the Synchronization Flag `0` (132.0 §4.1.2.7.6;
/// TM-008).
///
/// A decoded frame holds `StartsAt(n)` only for `n` inside its data field; any other pointer is
/// `Malformed` (API-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketPointer {
    /// The first Packet that starts in the data field starts at this octet, numbered from 0
    /// (132.0 §4.1.2.7.6.3).
    StartsAt(u16),
    /// `11111111111`: no Packet starts in the data field (132.0 §4.1.2.7.6.4).
    NoPacketStart,
    /// `11111111110`: only idle data, an OID frame (132.0 §4.1.2.7.6.5, §4.1.4.6.1).
    OnlyIdleData,
}

impl PacketPointer {
    /// The pointer for the raw field and a data field of `data_len` octets (TM-008).
    ///
    /// # Errors
    /// `Malformed` (`first_header_pointer`) for an offset that is not inside the data field
    /// (API-032, OI-33), with the largest valid offset.
    pub fn classify(raw: FirstHeaderPointer, data_len: usize) -> Result<PacketPointer> {
        match raw {
            FirstHeaderPointer::NO_PACKET_START => Ok(PacketPointer::NoPacketStart),
            FirstHeaderPointer::ONLY_IDLE_DATA => Ok(PacketPointer::OnlyIdleData),
            FirstHeaderPointer(n) if usize::from(n) < data_len => Ok(PacketPointer::StartsAt(n)),
            FirstHeaderPointer(n) => Err(mismatch(
                ErrorKind::Malformed,
                field::FIRST_HEADER_POINTER,
                len_u64(data_len.saturating_sub(1)),
                u64::from(n),
            )),
        }
    }

    /// The raw field value. An offset above 11 bits saturates to `11111111111`; writers reject
    /// such offsets before they reach the wire (132.0 §4.1.2.7.6; TM-009).
    #[must_use]
    pub const fn to_raw(self) -> FirstHeaderPointer {
        match self {
            PacketPointer::StartsAt(n) if n < FirstHeaderPointer::ONLY_IDLE_DATA.0 => {
                FirstHeaderPointer(n)
            }
            PacketPointer::StartsAt(_) | PacketPointer::NoPacketStart => {
                FirstHeaderPointer::NO_PACKET_START
            }
            PacketPointer::OnlyIdleData => FirstHeaderPointer::ONLY_IDLE_DATA,
        }
    }
}

/// The Synchronization Flag with the fields whose meaning depends on it (132.0 §4.1.2.7.3–6;
/// TM-007, TM-008, TM-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataFieldStatus {
    /// Synchronization Flag `0`: octet-synchronized, forward-ordered Packets or idle data. The
    /// Packet Order Flag is `0` and the Segment Length Identifier `11` (132.0 §4.1.2.7.3.2,
    /// §4.1.2.7.4, §4.1.2.7.5.2).
    Packets(PacketPointer),
    /// Synchronization Flag `1`: a `VCA_SDU`. The other three fields are undefined by CCSDS and
    /// carried raw (132.0 §4.1.2.7.4 note, §4.1.2.7.5.2 note 2, §4.1.2.7.6.2 note).
    VcaSdu {
        /// Packet Order Flag, use undefined.
        packet_order_flag: bool,
        /// Segment Length Identifier, undefined.
        segment_length_id: SegmentLengthId,
        /// First Header Pointer, undefined.
        first_header_pointer: FirstHeaderPointer,
    },
}

impl DataFieldStatus {
    /// The Synchronization Flag: `false` for Packets, `true` for a `VCA_SDU` (132.0 §4.1.2.7.3.2).
    #[must_use]
    pub const fn sync_flag(&self) -> bool {
        matches!(self, DataFieldStatus::VcaSdu { .. })
    }
}

/// A TM Transfer Frame Primary Header as plain values (132.0 §4.1.2; API-014).
///
/// Every field is valid by its type and no constraint spans them, so the fields are public
/// (RS-020); strict decoding and the writers apply the rules that relate them to the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is a one-bit flag of the header, 132.0 §4.1.2.4, §4.1.2.7.2–4"
)]
pub struct TmPrimaryHeader {
    /// Spacecraft Identifier, 10 bits (132.0 §4.1.2.2.3).
    pub scid: SpacecraftId<10>,
    /// Virtual Channel Identifier, 3 bits (132.0 §4.1.2.3).
    pub vcid: VcId<3>,
    /// Operational Control Field Flag (132.0 §4.1.2.4).
    pub ocf_flag: bool,
    /// Master Channel Frame Count, modulo 256 (132.0 §4.1.2.5).
    pub mc_frame_count: u8,
    /// Virtual Channel Frame Count, modulo 256 (132.0 §4.1.2.6).
    pub vc_frame_count: u8,
    /// Transfer Frame Secondary Header Flag (132.0 §4.1.2.7.2).
    pub fsh_flag: bool,
    /// Synchronization Flag (132.0 §4.1.2.7.3).
    pub sync_flag: bool,
    /// Packet Order Flag (132.0 §4.1.2.7.4).
    pub packet_order_flag: bool,
    /// Segment Length Identifier (132.0 §4.1.2.7.5).
    pub segment_length_id: SegmentLengthId,
    /// First Header Pointer, raw (132.0 §4.1.2.7.6).
    pub first_header_pointer: FirstHeaderPointer,
}

/// The value of SCID 0, the fallback of a masked field that always fits (GEN-020).
const SCID_ZERO: SpacecraftId<10> = SpacecraftId::new(0).expect("0 fits");
/// The value of VCID 0, the fallback of a masked field that always fits (GEN-020).
const VCID_ZERO: VcId<3> = VcId::new(0).expect("0 fits");

impl TmPrimaryHeader {
    /// Decodes the Primary Header at the start of `prefix`; octets after it are ignored
    /// (132.0 §4.1.2; TM-002, API-014).
    ///
    /// # Errors
    /// `Incomplete` (`needed` = 6) if `prefix` is shorter than the header; `UnsupportedVersion`
    /// (`frame_version`) if the Transfer Frame Version Number is not `00` (TM-002).
    pub fn decode(prefix: &[u8]) -> Result<TmPrimaryHeader> {
        let Ok([o0, o1, o2, o3, o4, o5]) = <[u8; PRIMARY_HEADER_LEN]>::try_from(
            prefix.get(..PRIMARY_HEADER_LEN).unwrap_or_default(),
        ) else {
            return Err(Error::incomplete(PRIMARY_HEADER_LEN));
        };
        let version = o0 >> 6;
        if version != VERSION {
            return Err(mismatch(
                ErrorKind::UnsupportedVersion,
                field::FRAME_VERSION,
                u64::from(VERSION),
                u64::from(version),
            ));
        }
        let scid = (u16::from(o0 & 0x3F) << 4) | u16::from(o1 >> 4);
        let fhp = (u16::from(o4 & 0b111) << 8) | u16::from(o5);
        Ok(TmPrimaryHeader {
            scid: SpacecraftId::new(scid).unwrap_or(SCID_ZERO),
            vcid: VcId::new((o1 >> 1) & 0b111).unwrap_or(VCID_ZERO),
            ocf_flag: o1 & 1 == 1,
            mc_frame_count: o2,
            vc_frame_count: o3,
            fsh_flag: o4 & 0x80 != 0,
            sync_flag: o4 & 0x40 != 0,
            packet_order_flag: o4 & 0x20 != 0,
            segment_length_id: SegmentLengthId((o4 >> 3) & 0b11),
            first_header_pointer: FirstHeaderPointer(fhp),
        })
    }

    /// The six header octets (132.0 §4.1.2; API-014, API-020).
    #[must_use]
    pub const fn to_octets(&self) -> [u8; PRIMARY_HEADER_LEN] {
        let [scid_hi, scid_lo] = self.scid.get().to_be_bytes();
        let [fhp_hi, fhp_lo] = self.first_header_pointer.0.to_be_bytes();
        let o0 = (VERSION << 6) | ((scid_hi & 0b11) << 4) | (scid_lo >> 4);
        let o1 = (scid_lo << 4) | (self.vcid.get() << 1) | flag(self.ocf_flag, 0);
        let o4 = flag(self.fsh_flag, 7)
            | flag(self.sync_flag, 6)
            | flag(self.packet_order_flag, 5)
            | (self.segment_length_id.0 << 3)
            | (fhp_hi & 0b111);
        [o0, o1, self.mc_frame_count, self.vc_frame_count, o4, fhp_lo]
    }
}

/// `1 << bit` if `set`, else 0.
const fn flag(set: bool, bit: u32) -> u8 {
    if set { 1 << bit } else { 0 }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        DataFieldStatus, FirstHeaderPointer, FshLen, PacketPointer, SegmentLengthId,
        TmPrimaryHeader,
    };
    use crate::field;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::{ErrorDetail, ErrorKind};

    fn header() -> TmPrimaryHeader {
        TmPrimaryHeader {
            scid: SpacecraftId::new(0x2A5).unwrap(),
            vcid: VcId::new(5).unwrap(),
            ocf_flag: true,
            mc_frame_count: 0x12,
            vc_frame_count: 0x34,
            fsh_flag: true,
            sync_flag: false,
            packet_order_flag: false,
            segment_length_id: SegmentLengthId::NO_SEGMENTS,
            first_header_pointer: FirstHeaderPointer::new(0x123).unwrap(),
        }
    }

    #[test]
    fn header_bits_follow_figure_4_2() {
        // req: TM-002, TM-009, API-014
        // SCID 0x2A5 = 10_1010_0101: octet 0 = 00 101010, octet 1 = 0101 101 1.
        let h = header();
        let octets = h.to_octets();
        assert_eq!(octets, [0x2A, 0x5B, 0x12, 0x34, 0x99, 0x23]);
        assert_eq!(TmPrimaryHeader::decode(&octets), Ok(h));
        let all = TmPrimaryHeader {
            sync_flag: true,
            packet_order_flag: true,
            fsh_flag: false,
            ocf_flag: false,
            segment_length_id: SegmentLengthId::new(1).unwrap(),
            first_header_pointer: FirstHeaderPointer::NO_PACKET_START,
            ..h
        };
        assert_eq!(all.to_octets()[4], 0b0110_1111);
        assert_eq!(TmPrimaryHeader::decode(&all.to_octets()), Ok(all));
    }

    #[test]
    fn header_rejects_version_and_short_input() {
        // req: TM-002, GEN-041
        let e = TmPrimaryHeader::decode(&[0x40, 0, 0, 0, 0, 0]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnsupportedVersion,
                Some(field::FRAME_VERSION),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 1
                }
            )
        );
        let e = TmPrimaryHeader::decode(&[0; 5]).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(6))
        );
    }

    #[test]
    fn identifiers_are_range_checked() {
        // req: API-001, TM-007, TM-008
        assert_eq!(SegmentLengthId::new(3), Some(SegmentLengthId::NO_SEGMENTS));
        assert_eq!(SegmentLengthId::new(4), None);
        assert_eq!(SegmentLengthId::try_from(2).map(u8::from), Ok(2));
        let e = SegmentLengthId::try_from(4).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::SEGMENT_LENGTH_ID))
        );
        assert_eq!(FirstHeaderPointer::new(0x800), None);
        assert_eq!(
            FirstHeaderPointer::try_from(0x7FF).map(u16::from),
            Ok(0x7FF)
        );
        let e = FirstHeaderPointer::try_from(0x800).unwrap_err();
        assert_eq!(e.field(), Some(field::FIRST_HEADER_POINTER));
    }

    #[test]
    fn fsh_len_is_2_to_64() {
        // req: TM-003, API-040
        assert_eq!(FshLen::new(1), None);
        assert_eq!(FshLen::new(65), None);
        let len = FshLen::new(64).unwrap();
        assert_eq!((len.get(), len.octets(), u8::from(len)), (64, 64, 64));
        assert_eq!(len.id_octet(), 0x3F);
        assert_eq!(FshLen::new(2).unwrap().id_octet(), 0x01);
        for (n, bound) in [(1, 2), (0, 2), (65, 64), (255, 64)] {
            let e = FshLen::try_from(n).unwrap_err();
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::OutOfRange,
                    Some(field::FSH_LEN),
                    ErrorDetail::Expected {
                        expected: bound,
                        found: u64::from(n)
                    }
                )
            );
        }
        assert_eq!(FshLen::try_from(2), Ok(FshLen::new(2).unwrap()));
    }

    #[test]
    fn pointer_classification() {
        // req: TM-008, API-032
        let raw = |n| FirstHeaderPointer::new(n).unwrap();
        assert_eq!(
            PacketPointer::classify(raw(0x7FF), 10),
            Ok(PacketPointer::NoPacketStart)
        );
        assert_eq!(
            PacketPointer::classify(raw(0x7FE), 10),
            Ok(PacketPointer::OnlyIdleData)
        );
        assert_eq!(
            PacketPointer::classify(raw(9), 10),
            Ok(PacketPointer::StartsAt(9))
        );
        let e = PacketPointer::classify(raw(10), 10).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::Malformed,
                Some(field::FIRST_HEADER_POINTER),
                ErrorDetail::Expected {
                    expected: 9,
                    found: 10
                }
            )
        );
        // A data field longer than 2046 octets: the two reserved values keep their meaning.
        assert_eq!(
            PacketPointer::classify(raw(0x7FE), 4000),
            Ok(PacketPointer::OnlyIdleData)
        );
        assert_eq!(PacketPointer::StartsAt(7).to_raw(), raw(7));
        assert_eq!(PacketPointer::StartsAt(0x7FE).to_raw(), raw(0x7FF));
        assert_eq!(
            FirstHeaderPointer::from(PacketPointer::OnlyIdleData),
            raw(0x7FE)
        );
        assert_eq!(PacketPointer::NoPacketStart.to_raw(), raw(0x7FF));
        assert!(!DataFieldStatus::Packets(PacketPointer::NoPacketStart).sync_flag());
        assert!(
            DataFieldStatus::VcaSdu {
                packet_order_flag: false,
                segment_length_id: SegmentLengthId::NO_SEGMENTS,
                first_header_pointer: raw(0),
            }
            .sync_flag()
        );
    }
}
