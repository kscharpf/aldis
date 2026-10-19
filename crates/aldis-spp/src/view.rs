//! Zero-copy Space Packet view (133.0 §4.1; SPP-001…SPP-004, SPP-007, SPP-009, API-010,
//! API-011, API-013, API-014, GEN-013, GEN-043).

use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{AnomalySet, DecodeMode, Error, Result};

use crate::anomaly::SppAnomaly;
use crate::header::{PRIMARY_HEADER_LEN, PacketType, SequenceFlags, SpacePacketHeader, invalid};
use crate::secondary::{SecondaryHeaderLayout, SplitPacket};

/// A decoded Space Packet borrowing its data field from the input (GEN-013).
///
/// The view keeps its decode mode, which [`split`](Self::split) applies to a time code; two
/// views of the same octets decoded in different modes are not equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpacePacket<'a> {
    header: SpacePacketHeader,
    data: &'a [u8],
    mode: DecodeMode,
    anomalies: AnomalySet<SppAnomaly>,
}

impl<'a> SpacePacket<'a> {
    /// Decodes one packet from the start of `bytes`; any octets after it are ignored, so a
    /// stream can be parsed packet by packet with [`len`](Self::len) (SPP-002, API-011).
    ///
    /// # Errors
    /// - `Incomplete` (`needed` = 6) if `bytes` is shorter than the primary header, then
    ///   `needed` = 7 + Packet Data Length if it is shorter than the packet (SPP-002).
    /// - `UnsupportedVersion` if the version is not `000`, in both modes (SPP-001).
    /// - `InvalidField` (`secondary_header_flag`) in strict mode for an Idle Packet with the
    ///   Secondary Header Flag set; lenient mode reports
    ///   [`SppAnomaly::IdleWithSecondaryHeader`] instead (SPP-004, API-032).
    pub fn decode(bytes: &'a [u8], mode: DecodeMode) -> Result<SpacePacket<'a>> {
        let header = SpacePacketHeader::parse(bytes)?;
        let total = header.packet_len();
        let data = bytes
            .get(PRIMARY_HEADER_LEN..total)
            .ok_or(Error::incomplete(total))?;
        let mut anomalies = AnomalySet::empty();
        if header.is_idle() && header.has_secondary_header() {
            match mode {
                DecodeMode::Strict => return Err(invalid("secondary_header_flag")),
                DecodeMode::Lenient => anomalies.insert(SppAnomaly::IdleWithSecondaryHeader),
            }
        }
        Ok(SpacePacket {
            header,
            data,
            mode,
            anomalies,
        })
    }

    /// Total length of the packet that starts `prefix`, from its first 6 octets (SPP-003,
    /// API-013). It applies every check of [`decode`](Self::decode) that needs only the header
    /// and holds in both modes.
    ///
    /// # Errors
    /// `Incomplete` (`needed` = 6) if `prefix` is shorter; `UnsupportedVersion` (SPP-001).
    pub fn peek_length(prefix: &[u8]) -> Result<usize> {
        SpacePacketHeader::parse(prefix).map(|h| h.packet_len())
    }

    /// Splits the data field into the secondary header and the user data with the APID's
    /// layout, which the caller looks up (SPP-007, SPP-008, OI-30). A time code is decoded
    /// in this packet's decode mode.
    ///
    /// # Errors
    /// - `InvalidField` (`secondary_header_flag`) if the Secondary Header Flag disagrees with
    ///   the layout: set with [`SecondaryHeaderLayout::Absent`], or clear with a secondary
    ///   header. 133.0 §4.1.3.3.3.3 fixes the flag per APID, so this holds in both modes.
    /// - `LengthMismatch` (`secondary_header`) if the data field is shorter than the secondary
    ///   header, including a time code cut off inside its P-field or T-field.
    /// - With a time layout: `InvalidField` (`time_code_id`) for a P-field with another time
    ///   code ID than the configured one, and the other errors of the time code decoder.
    pub fn split(&self, layout: &SecondaryHeaderLayout) -> Result<SplitPacket<'a>> {
        layout.split(self.header.has_secondary_header(), self.data, self.mode)
    }

    /// The header as plain values (API-014).
    #[must_use]
    pub const fn header(&self) -> SpacePacketHeader {
        self.header
    }

    /// Packet Type (133.0 §4.1.3.3.2, SPP-009).
    #[must_use]
    pub const fn packet_type(&self) -> PacketType {
        self.header.packet_type()
    }

    /// Secondary Header Flag (133.0 §4.1.3.3.3).
    #[must_use]
    pub const fn has_secondary_header(&self) -> bool {
        self.header.has_secondary_header()
    }

    /// Application Process Identifier (133.0 §4.1.3.3.4).
    #[must_use]
    pub const fn apid(&self) -> Apid {
        self.header.apid()
    }

    /// Whether this is an Idle Packet, APID 2047 (133.0 §4.1.3.3.4.4).
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.header.is_idle()
    }

    /// Sequence Flags (133.0 §4.1.3.4.2).
    #[must_use]
    pub const fn sequence_flags(&self) -> SequenceFlags {
        self.header.sequence_flags()
    }

    /// The 14-bit Packet Sequence Count or Packet Name (SPP-009).
    #[must_use]
    pub const fn sequence_count(&self) -> SeqCount {
        self.header.sequence_count()
    }

    /// The Packet Name of a telecommand packet; `None` for telemetry (SPP-009).
    #[must_use]
    pub const fn packet_name(&self) -> Option<SeqCount> {
        self.header.packet_name()
    }

    /// The whole Packet Data Field, borrowed from the input (SPP-007, GEN-013).
    #[must_use]
    pub const fn data_field(&self) -> &'a [u8] {
        self.data
    }

    /// Octets consumed from the input: the total packet length (SPP-002, API-011).
    #[must_use]
    pub fn len(&self) -> usize {
        self.header.packet_len()
    }

    /// Always `false`: a packet has at least 7 octets (133.0 §4.1.2.2).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<SppAnomaly> {
        self.anomalies
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::SpacePacket;
    use crate::anomaly::SppAnomaly;
    use crate::header::{PacketType, SequenceFlags};
    use crate::secondary::SecondaryHeaderLayout;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind, FieldId};
    use core::num::NonZeroUsize;
    use proptest::prelude::*;

    #[test]
    fn view_accessors() {
        // req: SPP-002, SPP-009, API-010, API-011, API-014, GEN-013
        let bytes = [0x18, 0x23, 0x40, 0x07, 0x00, 0x01, 0xA0, 0xB0, 0xFF];
        let p = SpacePacket::decode(&bytes, DecodeMode::Strict).unwrap();
        assert_eq!(
            (p.packet_type(), p.apid().get(), p.sequence_flags()),
            (PacketType::Telecommand, 0x023, SequenceFlags::First)
        );
        assert_eq!(
            (
                p.sequence_count().get(),
                p.packet_name().map(aldis_core::ids::SeqCount::get)
            ),
            (7, Some(7))
        );
        assert!(p.has_secondary_header() && !p.is_idle() && !p.is_empty());
        assert_eq!(
            (p.data_field(), p.len(), p.anomalies().is_empty()),
            (&[0xA0, 0xB0][..], 8, true)
        );
        assert_eq!(p.header().packet_len(), 8);
        assert_eq!(SpacePacket::peek_length(&bytes[..6]), Ok(8));
        let s = p
            .split(&SecondaryHeaderLayout::Opaque(NonZeroUsize::MIN))
            .unwrap();
        assert_eq!(
            (s.secondary_header(), s.user_data()),
            (&[0xA0][..], &[0xB0][..])
        );
    }

    #[test]
    fn errors_and_lenient_idle() {
        // req: SPP-002, SPP-004, API-032, GEN-041
        let e = SpacePacket::decode(&[0, 1, 0xC0, 0, 0, 2, 1, 2], DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(9))
        );
        let idle = [0x0F, 0xFF, 0xC0, 0, 0, 0, 0];
        let e = SpacePacket::decode(&idle, DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::InvalidField, Some("secondary_header_flag"))
        );
        let p = SpacePacket::decode(&idle, DecodeMode::Lenient).unwrap();
        assert!(p.is_idle() && p.anomalies().contains(SppAnomaly::IdleWithSecondaryHeader));
        assert_eq!(SpacePacket::peek_length(&idle), Ok(7));
    }

    proptest! {
        #[test]
        fn lenient_accepts_whatever_strict_accepts(bytes in proptest::collection::vec(any::<u8>(), 0..32)) {
            // req: GEN-020, API-010, API-031, SPP-003
            let strict = SpacePacket::decode(&bytes, DecodeMode::Strict);
            let lenient = SpacePacket::decode(&bytes, DecodeMode::Lenient);
            if let Ok(s) = strict {
                let l = lenient.unwrap();
                prop_assert_eq!((l.header(), l.data_field(), l.anomalies()), (s.header(), s.data_field(), s.anomalies()));
                prop_assert!(s.anomalies().is_empty());
            }
            if let Ok(l) = lenient {
                prop_assert_eq!(SpacePacket::peek_length(&bytes), Ok(l.len()));
                prop_assert!(l.len() <= bytes.len());
            }
        }
    }
}
