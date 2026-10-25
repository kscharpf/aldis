//! The Multiplexing Protocol Data Unit: its header and Packet Zone (732.0-B-5 §4.1.4.2;
//! 732.0-B-4 §4.1.4.2; AOS-003, AOS-017, GEN-062).
//!
//! | Edition | `M_PDU` Header | No Packet starts | Only Idle Data |
//! |---|---|---|---|
//! | `Issue5` | First Header Pointer, 16 bits (§4.1.4.2.1.4) | `0xFFFF` (§4.1.4.2.2.4) | `0xFFFE` (§4.1.4.2.2.5) |
//! | `Issue4Compatible` | Reserved Spare `00000`, 5 bits, then the First Header Pointer, 11 bits (732.0-B-4 §4.1.4.2.1.4) | `0x7FF` | `0x7FE` |

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, Result};

use crate::anomaly::{AosAnomaly, Deviations};
use crate::config::AosEdition;
use crate::field;
use crate::header::{len_u64, mismatch};

/// Length of the `M_PDU` Header, in octets (732.0 §4.1.4.2.1.3 a).
pub const MPDU_HEADER_LEN: usize = 2;

/// The octet offset of the first Packet that starts in a Packet Zone, 0 to `0xFFFD`
/// (732.0 §4.1.4.2.2.3–5; AOS-003, AOS-017, RS-020).
///
/// The offsets of this type are those the 16-bit pointer of `Issue5` holds below its two
/// reserved values, so every offset has a pointer value there (§4.1.4.2.2.4–5). Whether it fits
/// the 11-bit pointer of `Issue4Compatible`, up to [`PacketPointer::max_offset`], and whether it
/// lies inside a given Packet Zone are the writer's checks (AOS-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PacketOffset(u16);

impl PacketOffset {
    /// Largest offset, `0xFFFD`: the largest `Issue5` pointer below the reserved values
    /// (§4.1.4.2.2.3–5).
    pub const MAX: u16 = PacketPointer::max_offset(AosEdition::Issue5);

    /// The offset, or `None` above `0xFFFD` (API-001).
    #[must_use]
    pub const fn new(offset: u16) -> Option<PacketOffset> {
        if offset <= PacketOffset::MAX {
            Some(PacketOffset(offset))
        } else {
            None
        }
    }

    /// The offset in octets (§4.1.4.2.2.3).
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for PacketOffset {
    type Error = Error;

    /// # Errors
    /// `OutOfRange` (`first_header_pointer`) above `0xFFFD`, with that bound.
    fn try_from(offset: u16) -> Result<PacketOffset> {
        PacketOffset::new(offset).ok_or_else(|| {
            mismatch(
                ErrorKind::OutOfRange,
                field::FIRST_HEADER_POINTER,
                u64::from(PacketOffset::MAX),
                u64::from(offset),
            )
        })
    }
}

impl From<PacketOffset> for u16 {
    fn from(offset: PacketOffset) -> u16 {
        offset.0
    }
}

/// The First Header Pointer, classified (732.0 §4.1.4.2.2; 732.0-B-4 §4.1.4.2.3; AOS-003,
/// AOS-017).
///
/// A decoded `M_PDU` holds `StartsAt(n)` only for `n` inside its Packet Zone; any other value
/// that is not reserved is `Malformed` (API-032, OI-33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketPointer {
    /// The first Packet that starts in the Packet Zone starts at this octet, numbered from 0
    /// (§4.1.4.2.2.3; RS-020).
    StartsAt(PacketOffset),
    /// All ones: no Packet starts in the Packet Zone (§4.1.4.2.2.4).
    NoPacketStart,
    /// All ones minus one: the Packet Zone holds only Idle Data (§4.1.4.2.2.5).
    OnlyIdleData,
}

impl PacketPointer {
    /// The raw pointer that means "no Packet starts": all ones of the edition's width,
    /// `0xFFFF` or `0x7FF` (AOS-003).
    #[must_use]
    pub const fn no_packet_start_raw(edition: AosEdition) -> u16 {
        match edition {
            AosEdition::Issue5 => 0xFFFF,
            AosEdition::Issue4Compatible => 0x07FF,
        }
    }

    /// The raw pointer that means "only Idle Data": all ones minus one, `0xFFFE` or `0x7FE`
    /// (AOS-003).
    #[must_use]
    pub const fn only_idle_data_raw(edition: AosEdition) -> u16 {
        PacketPointer::no_packet_start_raw(edition).saturating_sub(1)
    }

    /// The largest packet offset the edition's pointer can hold, below its two reserved values:
    /// `0xFFFD` or `0x7FD`.
    #[must_use]
    pub const fn max_offset(edition: AosEdition) -> u16 {
        PacketPointer::no_packet_start_raw(edition).saturating_sub(2)
    }

    /// The pointer for the raw field of `edition` and a Packet Zone of `zone_len` octets
    /// (AOS-017).
    ///
    /// `raw` is the masked pointer field of `edition`, as the decoder reads it: 16 bits in
    /// Issue 5, 11 bits in Issue 4 (AOS-003). A wider value cannot come off the wire, and is
    /// classified as given.
    ///
    /// # Errors
    /// `Malformed` (`first_header_pointer`) for an offset that is not inside the Packet Zone,
    /// with the largest offset that is (API-032, OI-33).
    pub fn classify(raw: u16, zone_len: usize, edition: AosEdition) -> Result<PacketPointer> {
        if raw == PacketPointer::no_packet_start_raw(edition) {
            Ok(PacketPointer::NoPacketStart)
        } else if raw == PacketPointer::only_idle_data_raw(edition) {
            Ok(PacketPointer::OnlyIdleData)
        } else {
            match PacketOffset::new(raw) {
                Some(offset) if usize::from(raw) < zone_len => Ok(PacketPointer::StartsAt(offset)),
                _ => Err(mismatch(
                    ErrorKind::Malformed,
                    field::FIRST_HEADER_POINTER,
                    len_u64(zone_len.saturating_sub(1)),
                    u64::from(raw),
                )),
            }
        }
    }

    /// The raw field value in `edition`. Every [`PacketOffset`] fits `Issue5`; in
    /// `Issue4Compatible`, an offset above [`max_offset`](Self::max_offset) saturates to "no
    /// Packet starts", and the writers reject such offsets before they reach the wire (AOS-003,
    /// AOS-019).
    #[must_use]
    pub const fn to_raw(self, edition: AosEdition) -> u16 {
        match self {
            PacketPointer::StartsAt(n) if n.get() <= PacketPointer::max_offset(edition) => n.get(),
            PacketPointer::StartsAt(_) | PacketPointer::NoPacketStart => {
                PacketPointer::no_packet_start_raw(edition)
            }
            PacketPointer::OnlyIdleData => PacketPointer::only_idle_data_raw(edition),
        }
    }

    /// The two `M_PDU` Header octets of the pointer in `edition`, spare bits zero (GEN-052).
    #[must_use]
    pub const fn header_octets(self, edition: AosEdition) -> [u8; MPDU_HEADER_LEN] {
        self.to_raw(edition).to_be_bytes()
    }
}

/// A decoded `M_PDU` borrowing its Packet Zone from the input (732.0 §4.1.4.2; AOS-003, AOS-017,
/// GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mpdu<'a> {
    pointer: PacketPointer,
    raw: u16,
    spare: u8,
    zone: &'a [u8],
}

impl<'a> Mpdu<'a> {
    /// Decodes an `M_PDU` that fills `field`, the data field of a frame of a VC that carries
    /// `M_PDUs`, in `edition` (732.0 §4.1.4.2; AOS-003, AOS-017).
    ///
    /// # Errors
    /// - `Malformed` (`frame_data_field`) if `field` has no octet after the 2-octet header
    ///   (§4.1.4.2.1.3 b);
    /// - `Malformed` (`first_header_pointer`) for a pointer outside the Packet Zone that is not
    ///   reserved, in both modes (AOS-017);
    /// - in strict mode, `ReservedValue` (`mpdu_spare`) for an Issue 4 Reserved Spare other
    ///   than `00000`; lenient mode reports it as [`AosAnomaly::Issue4SpareSet`] (AOS-003).
    pub fn decode(field: &'a [u8], edition: AosEdition, mode: DecodeMode) -> Result<Mpdu<'a>> {
        let mut deviations = Deviations::new(mode);
        let mpdu = Mpdu::parse(field, edition, &mut deviations)?;
        deviations.finish()?;
        Ok(mpdu)
    }

    /// Decodes the `M_PDU`, noting its deviations in `deviations`.
    pub(crate) fn parse(
        field: &'a [u8],
        edition: AosEdition,
        deviations: &mut Deviations,
    ) -> Result<Mpdu<'a>> {
        let (Some(&[o0, o1]), Some(zone)) = (
            field.first_chunk::<MPDU_HEADER_LEN>(),
            field.get(MPDU_HEADER_LEN..).filter(|z| !z.is_empty()),
        ) else {
            return Err(Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD));
        };
        let word = u16::from_be_bytes([o0, o1]);
        let (spare, raw) = match edition {
            AosEdition::Issue5 => (0, word),
            AosEdition::Issue4Compatible => (o0 >> 3, word & 0x07FF),
        };
        let pointer = PacketPointer::classify(raw, zone.len(), edition)?;
        if spare != 0 {
            deviations.note(
                AosAnomaly::Issue4SpareSet,
                mismatch(
                    ErrorKind::ReservedValue,
                    field::MPDU_SPARE,
                    0,
                    u64::from(spare),
                ),
            );
        }
        Ok(Mpdu {
            pointer,
            raw,
            spare,
            zone,
        })
    }

    /// The First Header Pointer, classified (AOS-017).
    #[must_use]
    pub const fn pointer(&self) -> PacketPointer {
        self.pointer
    }

    /// The First Header Pointer as received: 16 bits in `Issue5`, 11 in `Issue4Compatible`
    /// (732.0 §4.1.4.2.2.1; AOS-003).
    #[must_use]
    pub const fn first_header_pointer(&self) -> u16 {
        self.raw
    }

    /// In `Issue4Compatible`, the 5-bit Reserved Spare as received; always 0 in `Issue5`
    /// (732.0-B-4 §4.1.4.2.2; AOS-003).
    #[must_use]
    pub const fn issue4_spare(&self) -> u8 {
        self.spare
    }

    /// The `M_PDU` Packet Zone, never empty (732.0 §4.1.4.2.3; GEN-013).
    #[must_use]
    pub const fn packet_zone(&self) -> &'a [u8] {
        self.zone
    }

    /// Lenient-mode anomalies of the `M_PDU`; always empty after a strict decode (AOS-003,
    /// GEN-043).
    #[must_use]
    pub fn anomalies(&self) -> AnomalySet<AosAnomaly> {
        let mut set = AnomalySet::empty();
        if self.spare != 0 {
            set.insert(AosAnomaly::Issue4SpareSet);
        }
        set
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Mpdu, PacketOffset, PacketPointer};
    use crate::anomaly::AosAnomaly;
    use crate::config::AosEdition::{Issue4Compatible, Issue5};
    use crate::config::tests::starts_at;
    use crate::field;
    use aldis_core::{AnomalySet, DecodeMode, ErrorDetail, ErrorKind};

    const STRICT: DecodeMode = DecodeMode::Strict;
    const LENIENT: DecodeMode = DecodeMode::Lenient;

    #[test]
    fn reserved_values_per_edition() {
        // req: AOS-003, AOS-017, GEN-062
        assert_eq!(PacketPointer::no_packet_start_raw(Issue5), 0xFFFF);
        assert_eq!(PacketPointer::only_idle_data_raw(Issue5), 0xFFFE);
        assert_eq!(PacketPointer::max_offset(Issue5), 0xFFFD);
        assert_eq!(PacketPointer::no_packet_start_raw(Issue4Compatible), 0x7FF);
        assert_eq!(PacketPointer::only_idle_data_raw(Issue4Compatible), 0x7FE);
        assert_eq!(PacketPointer::max_offset(Issue4Compatible), 0x7FD);
        for (p, five, four) in [
            (starts_at(7), 7, 7),
            (starts_at(0x7FD), 0x7FD, 0x7FD),
            (starts_at(0x7FE), 0x7FE, 0x7FF),
            (starts_at(0xFFFD), 0xFFFD, 0x7FF),
            (PacketPointer::NoPacketStart, 0xFFFF, 0x7FF),
            (PacketPointer::OnlyIdleData, 0xFFFE, 0x7FE),
        ] {
            assert_eq!((p.to_raw(Issue5), p.to_raw(Issue4Compatible)), (five, four));
        }
        assert_eq!(starts_at(0x123).header_octets(Issue5), [0x01, 0x23]);
    }

    #[test]
    fn offsets_lie_below_the_reserved_values() {
        // req: AOS-003, AOS-017, RS-020
        assert_eq!(PacketOffset::MAX, PacketPointer::max_offset(Issue5));
        for n in [0, 1, 0x7FD, 0x7FE, 0xFFFD] {
            let offset = PacketOffset::new(n).unwrap();
            assert_eq!((offset.get(), u16::from(offset)), (n, n));
        }
        assert_eq!(PacketOffset::new(0xFFFE), None);
        assert_eq!(PacketOffset::new(0xFFFF), None);
        assert_eq!(
            PacketOffset::try_from(0xFFFD),
            Ok(PacketOffset::new(0xFFFD).unwrap())
        );
        let e = PacketOffset::try_from(0xFFFE).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::OutOfRange,
                Some(field::FIRST_HEADER_POINTER),
                ErrorDetail::Expected {
                    expected: 0xFFFD,
                    found: 0xFFFE
                }
            )
        );
    }

    #[test]
    fn classification() {
        // req: AOS-017, AOS-003, API-032
        for edition in [Issue5, Issue4Compatible] {
            let none = PacketPointer::no_packet_start_raw(edition);
            let idle = PacketPointer::only_idle_data_raw(edition);
            assert_eq!(
                PacketPointer::classify(none, 4, edition),
                Ok(PacketPointer::NoPacketStart)
            );
            assert_eq!(
                PacketPointer::classify(idle, 4, edition),
                Ok(PacketPointer::OnlyIdleData)
            );
            assert_eq!(PacketPointer::classify(3, 4, edition), Ok(starts_at(3)));
            let e = PacketPointer::classify(4, 4, edition).unwrap_err();
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::Malformed,
                    Some(field::FIRST_HEADER_POINTER),
                    ErrorDetail::Expected {
                        expected: 3,
                        found: 4
                    }
                )
            );
        }
        // A zone longer than the pointer's range: the reserved values keep their meaning.
        assert_eq!(
            PacketPointer::classify(0x7FE, 4000, Issue4Compatible),
            Ok(PacketPointer::OnlyIdleData)
        );
        assert_eq!(
            PacketPointer::classify(0x7FE, 4000, Issue5),
            Ok(starts_at(0x7FE))
        );
        // A 16-bit value given with the 11-bit edition: inside the zone, but above every
        // offset a pointer can hold (RS-020).
        let e = PacketPointer::classify(0xFFFE, 70_000, Issue4Compatible).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::Malformed,
                Some(field::FIRST_HEADER_POINTER),
                ErrorDetail::Expected {
                    expected: 69_999,
                    found: 0xFFFE
                }
            )
        );
    }

    #[test]
    fn decode_issue_5() {
        // req: AOS-017, GEN-013
        let field = [0x00, 0x02, 0xA0, 0xA1, 0xA2];
        let m = Mpdu::decode(&field, Issue5, STRICT).unwrap();
        assert_eq!((m.pointer(), m.first_header_pointer()), (starts_at(2), 2));
        assert_eq!(m.packet_zone(), &field[2..]);
        assert_eq!(m.packet_zone().as_ptr(), field[2..].as_ptr(), "zero-copy");
        assert_eq!((m.issue4_spare(), m.anomalies()), (0, AnomalySet::empty()));
        let m = Mpdu::decode(&[0xFF, 0xFE, 0], Issue5, STRICT).unwrap();
        assert_eq!(m.pointer(), PacketPointer::OnlyIdleData);
        for mode in [STRICT, LENIENT] {
            let e = Mpdu::decode(&[0x00, 0x03, 0, 0, 0], Issue5, mode).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::Malformed, Some(field::FIRST_HEADER_POINTER))
            );
            for short in [&[][..], &[0xFF][..], &[0xFF, 0xFF][..]] {
                let e = Mpdu::decode(short, Issue5, mode).unwrap_err();
                assert_eq!(
                    (e.kind(), e.field()),
                    (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
                );
            }
        }
    }

    #[test]
    fn decode_issue_4_spare() {
        // req: AOS-003, API-031, API-032, GEN-043
        // Spare 00000, FHP 0x7FF; and spare 10101, FHP 0x001.
        let m = Mpdu::decode(&[0x07, 0xFF, 0], Issue4Compatible, STRICT).unwrap();
        assert_eq!(
            (m.pointer(), m.first_header_pointer()),
            (PacketPointer::NoPacketStart, 0x7FF)
        );
        let spare = [0xA8, 0x01, 0, 0];
        let e = Mpdu::decode(&spare, Issue4Compatible, STRICT).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::ReservedValue,
                Some(field::MPDU_SPARE),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 0x15
                }
            )
        );
        let m = Mpdu::decode(&spare, Issue4Compatible, LENIENT).unwrap();
        assert_eq!((m.pointer(), m.issue4_spare()), (starts_at(1), 0x15));
        assert_eq!(
            m.anomalies(),
            AnomalySet::from_flags(&[AosAnomaly::Issue4SpareSet])
        );
        // In Issue 5 the same octets are a 16-bit pointer outside the zone.
        let e = Mpdu::decode(&spare, Issue5, LENIENT).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Malformed);
        // A malformed pointer wins over the spare in both modes.
        for mode in [STRICT, LENIENT] {
            let e = Mpdu::decode(&[0xF8, 0x05, 0, 0], Issue4Compatible, mode).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::Malformed, Some(field::FIRST_HEADER_POINTER))
            );
        }
    }
}
