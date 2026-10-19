//! Packet Primary Header (133.0 §4.1.3; SPP-001…003, SPP-009, API-014, API-021).

use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

/// Length of the Packet Primary Header in octets (133.0 §4.1.3.1).
pub const PRIMARY_HEADER_LEN: usize = 6;

/// Shortest Space Packet: the primary header and one data octet (133.0 §4.1.2.2).
pub const MIN_PACKET_LEN: usize = 7;

/// Longest Space Packet: the primary header and 65 536 data octets (133.0 §4.1.2.2).
pub const MAX_PACKET_LEN: usize = 65_542;

/// Longest Packet Data Field (133.0 §4.1.3.5).
pub(crate) const MAX_DATA_LEN: usize = 65_536;

/// Packet Version Number of a Space Packet, `000` (133.0 §4.1.3.2.2).
const VERSION: u8 = 0;

/// Version field written by an unfinished two-phase writer, which every decoder rejects
/// (API-023).
pub(crate) const UNFINISHED_VERSION: u8 = 0b111;

/// Packet Type (133.0 §4.1.3.3.2, SPP-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketType {
    /// `0`: telemetry or reporting.
    Telemetry,
    /// `1`: telecommand or requesting.
    Telecommand,
}

/// Sequence Flags (133.0 §4.1.3.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequenceFlags {
    /// `00`: a continuation segment of user data.
    Continuation,
    /// `01`: the first segment of user data.
    First,
    /// `10`: the last segment of user data.
    Last,
    /// `11`: unsegmented user data.
    Unsegmented,
}

impl SequenceFlags {
    /// The flags in the low two bits of `bits`; the other bits are ignored (133.0 §4.1.3.4.2).
    #[must_use]
    pub const fn from_bits(bits: u8) -> SequenceFlags {
        match bits & 0b11 {
            0b00 => SequenceFlags::Continuation,
            0b01 => SequenceFlags::First,
            0b10 => SequenceFlags::Last,
            _ => SequenceFlags::Unsegmented,
        }
    }

    /// The 2-bit field value (133.0 §4.1.3.4.2).
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            SequenceFlags::Continuation => 0b00,
            SequenceFlags::First => 0b01,
            SequenceFlags::Last => 0b10,
            SequenceFlags::Unsegmented => 0b11,
        }
    }
}

/// The primary header fields a sender chooses (133.0 §4.1.3). The Secondary Header Flag and
/// the Packet Data Length are derived by the encoders (API-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketFields {
    /// Packet Type (133.0 §4.1.3.3.2).
    pub packet_type: PacketType,
    /// Application Process Identifier (133.0 §4.1.3.3.4).
    pub apid: Apid,
    /// Sequence Flags (133.0 §4.1.3.4.2).
    pub sequence_flags: SequenceFlags,
    /// Packet Sequence Count or Packet Name (133.0 §4.1.3.4.3).
    pub sequence_count: SeqCount,
}

/// `InvalidField` naming `field`.
pub(crate) const fn invalid(field: &'static str) -> Error {
    Error::new(ErrorKind::InvalidField).with_field(FieldId::new(field))
}

/// `usize` → `u64` for error details; saturates on (hypothetical) wider targets.
pub(crate) fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// A Packet Primary Header as plain values (API-014). The Packet Data Length is stored as the
/// data field length it encodes, 1–65 536 octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpacePacketHeader {
    fields: PacketFields,
    secondary_header: bool,
    data_len: u32,
}

impl SpacePacketHeader {
    /// Builds a header; `data_len` must be 1–65 536, which the crate's callers guarantee.
    pub(crate) fn new(fields: PacketFields, secondary_header: bool, data_len: usize) -> Self {
        SpacePacketHeader {
            fields,
            secondary_header,
            data_len: u32::try_from(data_len).unwrap_or(1),
        }
    }

    /// Parses the first 6 octets (SPP-001, SPP-003).
    ///
    /// # Errors
    /// `Incomplete` (`needed` = 6) if `bytes` is shorter; `UnsupportedVersion` if the version is
    /// not `000`.
    pub(crate) fn parse(bytes: &[u8]) -> Result<SpacePacketHeader> {
        let Some(&[b0, b1, b2, b3, b4, b5]) = bytes.first_chunk::<PRIMARY_HEADER_LEN>() else {
            return Err(Error::incomplete(PRIMARY_HEADER_LEN));
        };
        let version = b0 >> 5;
        if version != VERSION {
            return Err(Error::new(ErrorKind::UnsupportedVersion)
                .with_field(FieldId::new("version"))
                .with_detail(ErrorDetail::Expected {
                    expected: 0,
                    found: version.into(),
                }));
        }
        let id = u16::from_be_bytes([b0, b1]);
        let control = u16::from_be_bytes([b2, b3]);
        let length = u16::from_be_bytes([b4, b5]);
        Ok(SpacePacketHeader {
            fields: PacketFields {
                packet_type: if id & 0x1000 == 0 {
                    PacketType::Telemetry
                } else {
                    PacketType::Telecommand
                },
                apid: Apid::new(id & Apid::MAX).unwrap_or(Apid::IDLE),
                sequence_flags: SequenceFlags::from_bits(b2 >> 6),
                sequence_count: SeqCount::new(control & SeqCount::MAX).unwrap_or_default(),
            },
            secondary_header: id & 0x0800 != 0,
            data_len: u32::from(length).wrapping_add(1),
        })
    }

    /// The 6 header octets with `version` in bits 0–2 (133.0 §4.1.3).
    pub(crate) fn octets_with_version(&self, version: u8) -> [u8; PRIMARY_HEADER_LEN] {
        let packet_type = match self.fields.packet_type {
            PacketType::Telemetry => 0,
            PacketType::Telecommand => 0x1000,
        };
        let shf = if self.secondary_header { 0x0800 } else { 0 };
        let id = (u16::from(version & 0b111) << 13) | packet_type | shf | self.fields.apid.get();
        let control =
            (u16::from(self.fields.sequence_flags.bits()) << 14) | self.fields.sequence_count.get();
        let length = u16::try_from(self.data_len.wrapping_sub(1)).unwrap_or(u16::MAX);
        let [i0, i1] = id.to_be_bytes();
        let [c0, c1] = control.to_be_bytes();
        let [l0, l1] = length.to_be_bytes();
        [i0, i1, c0, c1, l0, l1]
    }

    /// The 6 header octets as they appear on the wire (133.0 §4.1.3, API-020).
    #[must_use]
    pub fn octets(&self) -> [u8; PRIMARY_HEADER_LEN] {
        self.octets_with_version(VERSION)
    }

    /// The sender-chosen fields (133.0 §4.1.3).
    #[must_use]
    pub const fn fields(&self) -> PacketFields {
        self.fields
    }

    /// Packet Type (133.0 §4.1.3.3.2, SPP-009).
    #[must_use]
    pub const fn packet_type(&self) -> PacketType {
        self.fields.packet_type
    }

    /// Secondary Header Flag (133.0 §4.1.3.3.3).
    #[must_use]
    pub const fn has_secondary_header(&self) -> bool {
        self.secondary_header
    }

    /// Application Process Identifier (133.0 §4.1.3.3.4).
    #[must_use]
    pub const fn apid(&self) -> Apid {
        self.fields.apid
    }

    /// Whether this is an Idle Packet, APID 2047 (133.0 §4.1.3.3.4.4).
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.fields.apid.is_idle()
    }

    /// Sequence Flags (133.0 §4.1.3.4.2).
    #[must_use]
    pub const fn sequence_flags(&self) -> SequenceFlags {
        self.fields.sequence_flags
    }

    /// The 14-bit Packet Sequence Count or Packet Name, for either packet type (133.0
    /// §4.1.3.4.3.2, SPP-009).
    #[must_use]
    pub const fn sequence_count(&self) -> SeqCount {
        self.fields.sequence_count
    }

    /// The same 14 bits as a Packet Name, for telecommand packets only; `None` for telemetry
    /// packets, which always carry a count (133.0 §4.1.3.4.3.2, SPP-009).
    #[must_use]
    pub const fn packet_name(&self) -> Option<SeqCount> {
        match self.fields.packet_type {
            PacketType::Telecommand => Some(self.fields.sequence_count),
            PacketType::Telemetry => None,
        }
    }

    /// Length of the Packet Data Field, 1–65 536 octets: the Packet Data Length plus one (133.0
    /// §4.1.3.5, SPP-002).
    #[must_use]
    pub fn data_len(&self) -> usize {
        usize::try_from(self.data_len).unwrap_or(MAX_DATA_LEN)
    }

    /// Total packet length, 7–65 542 octets (133.0 §4.1.2.2, SPP-002).
    #[must_use]
    pub fn packet_len(&self) -> usize {
        self.data_len().saturating_add(PRIMARY_HEADER_LEN)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{PacketFields, PacketType, SequenceFlags, SpacePacketHeader};
    use aldis_core::ids::{Apid, SeqCount};
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};

    fn tc_idle() -> PacketFields {
        PacketFields {
            packet_type: PacketType::Telecommand,
            apid: Apid::IDLE,
            sequence_flags: SequenceFlags::Last,
            sequence_count: SeqCount::default(),
        }
    }

    #[test]
    fn sequence_flags_round_trip() {
        // req: SPP-009
        for bits in 0..=255_u8 {
            assert_eq!(SequenceFlags::from_bits(bits).bits(), bits & 0b11);
        }
    }

    #[test]
    fn parse_and_octets() {
        // req: SPP-001, SPP-002, SPP-003, SPP-009, API-014, GEN-053
        let wire = [0x17, 0xFF, 0x80, 0x00, 0xFF, 0xFF];
        let h = SpacePacketHeader::parse(&wire).unwrap();
        assert_eq!(h.fields(), tc_idle());
        assert!(!h.has_secondary_header() && h.is_idle());
        assert_eq!(
            (h.packet_type(), h.apid(), h.sequence_flags()),
            (PacketType::Telecommand, Apid::IDLE, SequenceFlags::Last)
        );
        assert_eq!(
            (h.sequence_count(), h.packet_name()),
            (SeqCount::default(), Some(SeqCount::default()))
        );
        assert_eq!(
            (h.data_len(), h.packet_len(), h.octets()),
            (65_536, 65_542, wire)
        );
        let tm = SpacePacketHeader::parse(&[0x08, 0x01, 0x3F, 0xFF, 0x00, 0x00]).unwrap();
        assert!(tm.has_secondary_header());
        assert_eq!(
            (
                tm.packet_type(),
                tm.packet_name(),
                tm.sequence_count().get()
            ),
            (PacketType::Telemetry, None, 0x3FFF)
        );
        assert_eq!((tm.data_len(), tm.octets_with_version(7)[0]), (1, 0xE8));
        assert_eq!(
            SpacePacketHeader::new(tc_idle(), true, 3).octets(),
            [0x1F, 0xFF, 0x80, 0x00, 0x00, 0x02]
        );
    }

    #[test]
    fn parse_errors() {
        // req: SPP-001, SPP-003, GEN-041
        let e = SpacePacketHeader::parse(&[0; 5]).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(6))
        );
        let e = SpacePacketHeader::parse(&[0xA0, 0, 0, 0, 0, 0]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::UnsupportedVersion, Some("version"))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 0,
                found: 5
            }
        );
    }
}
