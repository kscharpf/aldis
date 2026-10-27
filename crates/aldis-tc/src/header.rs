//! Primary Header and Segment Header (232.0 §4.1.2, §4.1.3.2.2; TC-001…TC-004, TC-008,
//! API-014, GEN-052, GEN-053).

use aldis_core::ids::{FrameSeqNum, MapId, SpacecraftId, VcId};
use aldis_core::{Error, ErrorKind, FieldId};

/// The Spacecraft Identifier of a TC frame: 10 bits (232.0 §4.1.2.5; API-002).
pub type TcSpacecraftId = SpacecraftId<10>;
/// The Virtual Channel Identifier of a TC frame and of the CLCW: 6 bits (232.0 §4.1.2.6,
/// §4.2.1.6).
pub type TcVcId = VcId<6>;
/// The MAP Identifier of the Segment Header: 6 bits (232.0 §4.1.3.2.2.3).
pub type TcMapId = MapId<6>;

/// Length of the Transfer Frame Primary Header in octets (232.0 §4.1.2.1).
pub const PRIMARY_HEADER_LEN: usize = 5;

/// Length of the Segment Header in octets (232.0 §4.1.3.2.2.1.4).
pub const SEGMENT_HEADER_LEN: usize = 1;

/// Shortest TC frame: the Primary Header and one data octet (232.0 §4.1.1;
/// `docs/standards/tc-frame.md` §1).
pub const MIN_FRAME_LEN: usize = 6;

/// Longest TC frame: the 10-bit Frame Length field counts up to 1024 octets (232.0 §4.1.2.7.3
/// note 2; TC-007).
pub const MAX_FRAME_LEN: usize = 1024;

/// Transfer Frame Version Number of a TC frame, `00` (232.0 §4.1.2.2.2).
const VERSION: u8 = 0;

/// Version field written by an unfinished two-phase writer, which every decoder rejects
/// (API-023).
pub(crate) const UNFINISHED_VERSION: u8 = 0b11;

/// The frame type set by the Bypass and Control Command Flags (232.0 §4.1.2.3, Table 4-1;
/// TC-002).
///
/// The combination Bypass Flag `0` with Control Command Flag `1` is reserved and has no
/// variant: the decoder rejects it in both modes (TC-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameType {
    /// `00`: Type-AD, a Frame Data Unit under the Sequence-Controlled Service.
    Ad,
    /// `10`: Type-BD, a Frame Data Unit under the Expedited Service.
    Bd,
    /// `11`: Type-BC, Control Commands (232.0 §4.1.3.3).
    Bc,
}

impl FrameType {
    /// The frame type of the two flags, or `None` for the reserved Bypass `0` / Control
    /// Command `1` (232.0 Table 4-1; TC-002).
    #[must_use]
    pub const fn from_flags(bypass: bool, control_command: bool) -> Option<FrameType> {
        match (bypass, control_command) {
            (false, false) => Some(FrameType::Ad),
            (true, false) => Some(FrameType::Bd),
            (true, true) => Some(FrameType::Bc),
            (false, true) => None,
        }
    }

    /// The Bypass Flag: `true` for a Type-B frame (232.0 §4.1.2.3.1).
    #[must_use]
    pub const fn bypass_flag(self) -> bool {
        !matches!(self, FrameType::Ad)
    }

    /// The Control Command Flag: `true` for a Type-C frame (232.0 §4.1.2.3.2).
    #[must_use]
    pub const fn control_command_flag(self) -> bool {
        matches!(self, FrameType::Bc)
    }
}

/// Sequence Flags of the Segment Header (232.0 §4.1.3.2.2.2, Table 4-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequenceFlags {
    /// `00`: a continuing portion of a service data unit.
    Continuing,
    /// `01`: the first portion of a service data unit.
    First,
    /// `10`: the last portion of a service data unit.
    Last,
    /// `11`: no segmentation: one complete service data unit, or several Packets.
    Unsegmented,
}

impl SequenceFlags {
    /// The flags in the low two bits of `bits`; the other bits are ignored (232.0 Table 4-2).
    #[must_use]
    pub const fn from_bits(bits: u8) -> SequenceFlags {
        match bits & 0b11 {
            0b00 => SequenceFlags::Continuing,
            0b01 => SequenceFlags::First,
            0b10 => SequenceFlags::Last,
            _ => SequenceFlags::Unsegmented,
        }
    }

    /// The 2-bit field value (232.0 Table 4-2).
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            SequenceFlags::Continuing => 0b00,
            SequenceFlags::First => 0b01,
            SequenceFlags::Last => 0b10,
            SequenceFlags::Unsegmented => 0b11,
        }
    }
}

/// The Segment Header of a Frame Data Unit (232.0 §4.1.3.2.2; TC-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SegmentHeader {
    /// Sequence Flags (232.0 §4.1.3.2.2.2).
    pub sequence_flags: SequenceFlags,
    /// MAP Identifier (232.0 §4.1.3.2.2.3).
    pub map_id: TcMapId,
}

impl SegmentHeader {
    /// The Segment Header octet: Sequence Flags in bits 0–1, MAP ID in bits 2–7 (232.0
    /// §4.1.3.2.2; API-020).
    #[must_use]
    pub const fn octet(self) -> u8 {
        (self.sequence_flags.bits() << 6) | (self.map_id.get() & 0x3F)
    }

    /// Parses the Segment Header octet; every value is a Segment Header (232.0 §4.1.3.2.2).
    pub(crate) fn from_octet(octet: u8) -> Option<SegmentHeader> {
        TcMapId::new(octet & 0x3F).map(|map_id| SegmentHeader {
            sequence_flags: SequenceFlags::from_bits(octet >> 6),
            map_id,
        })
    }
}

/// The Primary Header fields a sender chooses (232.0 §4.1.2). The version, the spare, and the
/// Frame Length are derived by the writers (API-021, GEN-052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameFields {
    /// The frame type: the Bypass and Control Command Flags (232.0 Table 4-1).
    pub frame_type: FrameType,
    /// Spacecraft Identifier (232.0 §4.1.2.5).
    pub spacecraft_id: TcSpacecraftId,
    /// Virtual Channel Identifier (232.0 §4.1.2.6).
    pub vc_id: TcVcId,
    /// Frame Sequence Number N(S), written as given on Type-B frames too (232.0 §4.1.2.8;
    /// TC-008).
    pub sequence_number: FrameSeqNum,
}

/// The five Primary Header octets as read, before any check (232.0 §4.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawHeader {
    pub(crate) version: u8,
    pub(crate) bypass: bool,
    pub(crate) control_command: bool,
    pub(crate) spare: u8,
    pub(crate) spacecraft_id: TcSpacecraftId,
    pub(crate) vc_id: TcVcId,
    /// The total frame length: the Frame Length field plus one, 1–1024 (GEN-053).
    pub(crate) frame_len: usize,
    pub(crate) sequence_number: FrameSeqNum,
}

impl RawHeader {
    /// Reads the first five octets; `None` if `bytes` is shorter (232.0 §4.1.2).
    pub(crate) fn parse(bytes: &[u8]) -> Option<RawHeader> {
        let octets = bytes.first_chunk::<PRIMARY_HEADER_LEN>().copied();
        let [b0, b1, b2, b3, b4] = octets.unwrap_or_default();
        let scid = u16::from_be_bytes([b0 & 0b11, b1]);
        let count = u16::from_be_bytes([b2 & 0b11, b3]);
        let ids = TcSpacecraftId::new(scid).zip(TcVcId::new(b2 >> 2));
        octets
            .zip(ids)
            .map(|(_, (spacecraft_id, vc_id))| RawHeader {
                version: b0 >> 6,
                bypass: b0 & 0x20 != 0,
                control_command: b0 & 0x10 != 0,
                spare: (b0 >> 2) & 0b11,
                spacecraft_id,
                vc_id,
                frame_len: usize::from(count).saturating_add(1),
                sequence_number: FrameSeqNum::new(b4),
            })
    }

    /// `UnsupportedVersion` naming `frame_version` unless the version is `00` (232.0
    /// §4.1.2.2.2; TC-002).
    pub(crate) fn check_version(&self) -> Result<(), Error> {
        if self.version == VERSION {
            return Ok(());
        }
        Err(Error::new(ErrorKind::UnsupportedVersion)
            .with_field(crate::field::FRAME_VERSION)
            .with_detail(aldis_core::ErrorDetail::Expected {
                expected: 0,
                found: u64::from(self.version),
            }))
    }

    /// The frame type, or `ReservedValue` naming `control_command_flag` for the reserved
    /// combination, in both modes (232.0 Table 4-1; TC-002).
    pub(crate) const fn frame_type(&self) -> Result<FrameType, Error> {
        match FrameType::from_flags(self.bypass, self.control_command) {
            Some(t) => Ok(t),
            None => Err(reserved(crate::field::CONTROL_COMMAND_FLAG)),
        }
    }
}

/// `InvalidField` naming `field`.
pub(crate) const fn invalid(field: FieldId) -> Error {
    Error::new(ErrorKind::InvalidField).with_field(field)
}

/// `ReservedValue` naming `field`.
pub(crate) const fn reserved(field: FieldId) -> Error {
    Error::new(ErrorKind::ReservedValue).with_field(field)
}

/// `UnknownChannel` naming `field`.
pub(crate) const fn unknown(field: FieldId) -> Error {
    Error::new(ErrorKind::UnknownChannel).with_field(field)
}

/// `usize` → `u64` for error details; saturates on (hypothetical) wider targets.
pub(crate) fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The five Primary Header octets with `version` in bits 0–1 and the spare `00` (232.0 §4.1.2;
/// GEN-052, GEN-053). `frame_len` is the total length, 1–1024.
pub(crate) fn primary_octets(
    version: u8,
    fields: FrameFields,
    frame_len: usize,
) -> [u8; PRIMARY_HEADER_LEN] {
    let flags = (u8::from(fields.frame_type.bypass_flag()) << 5)
        | (u8::from(fields.frame_type.control_command_flag()) << 4);
    let [s0, s1] = fields.spacecraft_id.get().to_be_bytes();
    let count = u16::try_from(frame_len.saturating_sub(1)).unwrap_or(0) & 0x03FF;
    let [c0, c1] = count.to_be_bytes();
    [
        ((version & 0b11) << 6) | flags | (s0 & 0b11),
        s1,
        (fields.vc_id.get() << 2) | (c0 & 0b11),
        c1,
        fields.sequence_number.get(),
    ]
}

/// A TC frame header as plain values (232.0 §4.1.2, §4.1.3.2.2; API-014).
///
/// Built only by the decoder, so the frame length is 6–1024 and a Segment Header is present
/// only on a Type-D frame of a Virtual Channel that has one (TC-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcFrameHeader {
    fields: FrameFields,
    frame_len: u16,
    segment_header: Option<SegmentHeader>,
}

impl TcFrameHeader {
    /// A header of a decoded frame; `frame_len` is 6–1024.
    pub(crate) fn new(
        fields: FrameFields,
        frame_len: usize,
        segment_header: Option<SegmentHeader>,
    ) -> TcFrameHeader {
        TcFrameHeader {
            fields,
            frame_len: u16::try_from(frame_len).unwrap_or(u16::MAX),
            segment_header,
        }
    }

    /// The sender-chosen fields (232.0 §4.1.2; API-014).
    #[must_use]
    pub const fn fields(&self) -> FrameFields {
        self.fields
    }

    /// The frame type (232.0 Table 4-1; TC-002).
    #[must_use]
    pub const fn frame_type(&self) -> FrameType {
        self.fields.frame_type
    }

    /// Spacecraft Identifier (232.0 §4.1.2.5).
    #[must_use]
    pub const fn spacecraft_id(&self) -> TcSpacecraftId {
        self.fields.spacecraft_id
    }

    /// Virtual Channel Identifier (232.0 §4.1.2.6).
    #[must_use]
    pub const fn vc_id(&self) -> TcVcId {
        self.fields.vc_id
    }

    /// Frame Sequence Number N(S) (232.0 §4.1.2.8; TC-008).
    #[must_use]
    pub const fn sequence_number(&self) -> FrameSeqNum {
        self.fields.sequence_number
    }

    /// Total frame length in octets, 6–1024: the Frame Length field plus one (232.0 §4.1.2.7;
    /// TC-001, GEN-053).
    #[must_use]
    pub fn frame_len(&self) -> usize {
        usize::from(self.frame_len)
    }

    /// The Segment Header, present only on a Type-D frame of a Virtual Channel configured with
    /// one (232.0 §4.1.3.2.2.1; TC-004).
    #[must_use]
    pub const fn segment_header(&self) -> Option<SegmentHeader> {
        self.segment_header
    }

    /// The five Primary Header octets, with the Reserved Spare `00` (232.0 §4.1.2; API-020,
    /// GEN-052).
    #[must_use]
    pub fn primary_header_octets(&self) -> [u8; PRIMARY_HEADER_LEN] {
        primary_octets(VERSION, self.fields, self.frame_len())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        FrameFields, FrameType, RawHeader, SegmentHeader, SequenceFlags, TcFrameHeader, TcMapId,
        TcSpacecraftId, TcVcId, primary_octets,
    };
    use crate::field;
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::{ErrorDetail, ErrorKind};

    #[test]
    fn frame_types_follow_table_4_1() {
        // req: TC-002
        assert_eq!(FrameType::from_flags(false, false), Some(FrameType::Ad));
        assert_eq!(FrameType::from_flags(true, false), Some(FrameType::Bd));
        assert_eq!(FrameType::from_flags(true, true), Some(FrameType::Bc));
        assert_eq!(FrameType::from_flags(false, true), None);
        for t in [FrameType::Ad, FrameType::Bd, FrameType::Bc] {
            assert_eq!(
                FrameType::from_flags(t.bypass_flag(), t.control_command_flag()),
                Some(t)
            );
        }
    }

    #[test]
    fn sequence_flags_and_segment_header() {
        // req: TC-004
        for bits in 0..=255_u8 {
            assert_eq!(SequenceFlags::from_bits(bits).bits(), bits & 0b11);
            let sh = SegmentHeader::from_octet(bits).unwrap();
            assert_eq!(sh.octet(), bits);
        }
        let sh = SegmentHeader::from_octet(0b0100_0101).unwrap();
        assert_eq!(
            (sh.sequence_flags, sh.map_id.get()),
            (SequenceFlags::First, 5)
        );
    }

    #[test]
    fn raw_header_and_octets() {
        // req: TC-001, TC-008, GEN-053, API-014
        let fields = FrameFields {
            frame_type: FrameType::Bd,
            spacecraft_id: TcSpacecraftId::new(0x2AA).unwrap(),
            vc_id: TcVcId::new(0x2B).unwrap(),
            sequence_number: FrameSeqNum::new(0x5A),
        };
        let octets = primary_octets(0, fields, 1024);
        assert_eq!(octets, [0x22, 0xAA, 0xAF, 0xFF, 0x5A]);
        let raw = RawHeader::parse(&octets).unwrap();
        assert_eq!(
            (raw.version, raw.bypass, raw.control_command, raw.spare),
            (0, true, false, 0)
        );
        assert_eq!(
            (raw.spacecraft_id, raw.vc_id, raw.frame_len),
            (fields.spacecraft_id, fields.vc_id, 1024)
        );
        assert_eq!(raw.frame_type(), Ok(FrameType::Bd));
        assert!(RawHeader::parse(&octets[..4]).is_none());
        let h = TcFrameHeader::new(fields, 1024, None);
        assert_eq!(
            (
                h.fields(),
                h.frame_type(),
                h.frame_len(),
                h.segment_header()
            ),
            (fields, FrameType::Bd, 1024, None)
        );
        assert_eq!(
            (h.spacecraft_id(), h.vc_id(), h.sequence_number()),
            (fields.spacecraft_id, fields.vc_id, fields.sequence_number)
        );
        assert_eq!(h.primary_header_octets(), octets);
        let seg = SegmentHeader {
            sequence_flags: SequenceFlags::Last,
            map_id: TcMapId::new(63).unwrap(),
        };
        assert_eq!(seg.octet(), 0xBF);
    }

    #[test]
    fn raw_header_checks() {
        // req: TC-002, GEN-041
        let raw = RawHeader::parse(&[0xD0, 0, 0, 5, 0]).unwrap();
        let e = raw.check_version().unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnsupportedVersion,
                Some(field::FRAME_VERSION),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 3
                }
            )
        );
        let e = raw.frame_type().unwrap_err();
        assert_eq!(e.field(), Some(field::CONTROL_COMMAND_FLAG));
        assert_eq!(RawHeader::parse(&[0x0C, 0, 0, 5, 0]).unwrap().spare, 0b11);
    }
}
