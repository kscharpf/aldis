//! The Bitstream Protocol Data Unit: its header and Bitstream Data Zone (732.0-B-5 §4.1.4.3;
//! AOS-017, AOS-018).
//!
//! The `B_PDU` Header is a 2-bit Reserved Spare `00` and a 14-bit Bitstream Data Pointer, the
//! index of the last valid user data bit in the zone, with all ones for "all valid" and all
//! ones minus one for "only idle data" (§4.1.4.3.1.4, §4.1.4.3.2–3). The same in Issue 4
//! (732.0-B-4 §4.1.4.3).

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, Result};

use crate::anomaly::{AosAnomaly, Deviations};
use crate::field;
use crate::header::{len_u64, mismatch};

/// Length of the `B_PDU` Header, in octets (732.0 §4.1.4.3.1.3 a).
pub const BPDU_HEADER_LEN: usize = 2;

/// The longest Bitstream Data Zone, 2 047 octets: the longest zone of whole octets whose every
/// bit index lies below the two reserved pointer values (AOS-018, OI-35 item 7).
pub const MAX_BITSTREAM_ZONE_LEN: usize = 2047;

/// The longest frame of an `Issue5` physical channel with a VC that carries `B_PDUs`, 2 048
/// octets (16 384 bits; 732.0 §4.1.4.3.3.4 note; AOS-018).
pub const MAX_BITSTREAM_FRAME_LEN: usize = 2048;

/// The Bitstream Data Pointer, classified (732.0 §4.1.4.3.3; AOS-017).
///
/// A decoded `B_PDU` holds `ValidBits(n)` only for `n` below the zone's length in bits; a pointer
/// outside the zone is `Malformed` (API-032, OI-33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitstreamPointer {
    /// The first `n` bits of the zone are user data and the rest idle data: the pointer holds
    /// `n − 1`, the index of the last valid bit (§4.1.4.3.3.2; GEN-053). `n` is at least 1.
    ValidBits(u16),
    /// All ones: the zone holds only user data (§4.1.4.3.3.3).
    AllValid,
    /// All ones minus one: the zone holds only idle data (§4.1.4.3.3.4).
    OnlyIdle,
}

impl BitstreamPointer {
    /// Largest value of the 14-bit field (§4.1.4.3.3.1).
    pub const MAX_RAW: u16 = 0x3FFF;
    /// All ones, "all valid" (§4.1.4.3.3.3).
    pub const ALL_VALID_RAW: u16 = 0x3FFF;
    /// All ones minus one, "only idle data" (§4.1.4.3.3.4).
    pub const ONLY_IDLE_RAW: u16 = 0x3FFE;

    /// The pointer for the raw 14-bit field and a zone of `zone_len` octets (AOS-017).
    ///
    /// # Errors
    /// In both modes (API-031, API-032):
    /// - `InvalidField` (`bitstream_data_pointer`) for the index of the zone's last bit: a zone
    ///   with no idle data has the pointer all ones (§4.1.4.3.3.3), with that value;
    /// - `Malformed` (`bitstream_data_pointer`) for an index beyond the zone that is not
    ///   reserved, with the zone's last bit index (OI-33).
    pub fn classify(raw: u16, zone_len: usize) -> Result<BitstreamPointer> {
        let last_bit = zone_len.saturating_mul(8).saturating_sub(1);
        if raw == BitstreamPointer::ALL_VALID_RAW {
            Ok(BitstreamPointer::AllValid)
        } else if raw == BitstreamPointer::ONLY_IDLE_RAW {
            Ok(BitstreamPointer::OnlyIdle)
        } else if usize::from(raw) < last_bit {
            Ok(BitstreamPointer::ValidBits(raw.saturating_add(1)))
        } else if usize::from(raw) == last_bit {
            Err(mismatch(
                ErrorKind::InvalidField,
                field::BITSTREAM_DATA_POINTER,
                u64::from(BitstreamPointer::ALL_VALID_RAW),
                u64::from(raw),
            ))
        } else {
            Err(mismatch(
                ErrorKind::Malformed,
                field::BITSTREAM_DATA_POINTER,
                len_u64(last_bit),
                u64::from(raw),
            ))
        }
    }

    /// The raw 14-bit field. `ValidBits(0)` and counts above `0x3FFE` saturate to "all
    /// valid"; the writers reject them before they reach the wire (AOS-019).
    #[must_use]
    pub const fn to_raw(self) -> u16 {
        match self {
            BitstreamPointer::ValidBits(n) if n >= 1 && n <= BitstreamPointer::ONLY_IDLE_RAW => {
                n.saturating_sub(1)
            }
            BitstreamPointer::ValidBits(_) | BitstreamPointer::AllValid => {
                BitstreamPointer::ALL_VALID_RAW
            }
            BitstreamPointer::OnlyIdle => BitstreamPointer::ONLY_IDLE_RAW,
        }
    }

    /// The two `B_PDU` Header octets of the pointer, the Reserved Spare `00` (GEN-052).
    #[must_use]
    pub const fn header_octets(self) -> [u8; BPDU_HEADER_LEN] {
        self.to_raw().to_be_bytes()
    }
}

/// A decoded `B_PDU` borrowing its Bitstream Data Zone from the input (732.0 §4.1.4.3; AOS-017,
/// GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bpdu<'a> {
    pointer: BitstreamPointer,
    raw: u16,
    spare: u8,
    zone: &'a [u8],
}

impl<'a> Bpdu<'a> {
    /// Decodes a `B_PDU` that fills `field`, the data field of a frame of a VC that carries
    /// `B_PDUs` (732.0 §4.1.4.3; AOS-017).
    ///
    /// # Errors
    /// - `Malformed` (`frame_data_field`) if `field` has no octet after the 2-octet header
    ///   (§4.1.4.3.1.3 b);
    /// - the errors of [`BitstreamPointer::classify`], in both modes;
    /// - in strict mode, `ReservedValue` (`bpdu_spare`) for a Reserved Spare other than `00`;
    ///   lenient mode reports it as [`AosAnomaly::BpduSpareSet`] (§4.1.4.3.2.2).
    pub fn decode(field: &'a [u8], mode: DecodeMode) -> Result<Bpdu<'a>> {
        let mut deviations = Deviations::new(mode);
        let bpdu = Bpdu::parse(field, &mut deviations)?;
        deviations.finish()?;
        Ok(bpdu)
    }

    /// Decodes the `B_PDU`, noting its deviations in `deviations`.
    pub(crate) fn parse(field: &'a [u8], deviations: &mut Deviations) -> Result<Bpdu<'a>> {
        let (Some(&[o0, o1]), Some(zone)) = (
            field.first_chunk::<BPDU_HEADER_LEN>(),
            field.get(BPDU_HEADER_LEN..).filter(|z| !z.is_empty()),
        ) else {
            return Err(Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD));
        };
        let spare = o0 >> 6;
        let raw = u16::from_be_bytes([o0, o1]) & BitstreamPointer::MAX_RAW;
        let pointer = BitstreamPointer::classify(raw, zone.len())?;
        if spare != 0 {
            deviations.note(
                AosAnomaly::BpduSpareSet,
                mismatch(
                    ErrorKind::ReservedValue,
                    field::BPDU_SPARE,
                    0,
                    u64::from(spare),
                ),
            );
        }
        Ok(Bpdu {
            pointer,
            raw,
            spare,
            zone,
        })
    }

    /// The Bitstream Data Pointer, classified (AOS-017).
    #[must_use]
    pub const fn pointer(&self) -> BitstreamPointer {
        self.pointer
    }

    /// The Bitstream Data Pointer as received, 14 bits (732.0 §4.1.4.3.3.1).
    #[must_use]
    pub const fn bitstream_data_pointer(&self) -> u16 {
        self.raw
    }

    /// The Reserved Spare as received (732.0 §4.1.4.3.2).
    #[must_use]
    pub const fn spare(&self) -> u8 {
        self.spare
    }

    /// The Bitstream Data Zone, never empty (732.0 §4.1.4.3.4; GEN-013).
    #[must_use]
    pub const fn data_zone(&self) -> &'a [u8] {
        self.zone
    }

    /// Lenient-mode anomalies of the `B_PDU`; always empty after a strict decode (GEN-043).
    #[must_use]
    pub fn anomalies(&self) -> AnomalySet<AosAnomaly> {
        let mut set = AnomalySet::empty();
        if self.spare != 0 {
            set.insert(AosAnomaly::BpduSpareSet);
        }
        set
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{BitstreamPointer, Bpdu};
    use crate::anomaly::AosAnomaly;
    use crate::field;
    use aldis_core::{AnomalySet, DecodeMode, ErrorDetail, ErrorKind};

    const STRICT: DecodeMode = DecodeMode::Strict;
    const LENIENT: DecodeMode = DecodeMode::Lenient;

    #[test]
    fn classification() {
        // req: AOS-017, API-031, API-032, GEN-053
        // A 2-octet zone: bits 0–15.
        assert_eq!(
            BitstreamPointer::classify(0x3FFF, 2),
            Ok(BitstreamPointer::AllValid)
        );
        assert_eq!(
            BitstreamPointer::classify(0x3FFE, 2),
            Ok(BitstreamPointer::OnlyIdle)
        );
        assert_eq!(
            BitstreamPointer::classify(0, 2),
            Ok(BitstreamPointer::ValidBits(1))
        );
        assert_eq!(
            BitstreamPointer::classify(14, 2),
            Ok(BitstreamPointer::ValidBits(15))
        );
        let e = BitstreamPointer::classify(15, 2).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::InvalidField,
                Some(field::BITSTREAM_DATA_POINTER),
                ErrorDetail::Expected {
                    expected: 0x3FFF,
                    found: 15
                }
            )
        );
        let e = BitstreamPointer::classify(16, 2).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::Malformed,
                Some(field::BITSTREAM_DATA_POINTER),
                ErrorDetail::Expected {
                    expected: 15,
                    found: 16
                }
            )
        );
        // The longest zone, 2 047 octets: last bit 16 375 = 0x3FF7.
        assert_eq!(
            BitstreamPointer::classify(0x3FF6, 2047),
            Ok(BitstreamPointer::ValidBits(0x3FF7))
        );
        assert_eq!(
            BitstreamPointer::classify(0x3FF7, 2047).unwrap_err().kind(),
            ErrorKind::InvalidField
        );
        assert_eq!(
            BitstreamPointer::classify(0x3FF8, 2047).unwrap_err().kind(),
            ErrorKind::Malformed
        );
    }

    #[test]
    fn raw_values() {
        // req: AOS-017, AOS-019, GEN-052
        for (p, raw) in [
            (BitstreamPointer::ValidBits(1), 0),
            (BitstreamPointer::ValidBits(0x3FFE), 0x3FFD),
            (BitstreamPointer::ValidBits(0x3FFF), 0x3FFF),
            (BitstreamPointer::ValidBits(0), 0x3FFF),
            (BitstreamPointer::AllValid, 0x3FFF),
            (BitstreamPointer::OnlyIdle, 0x3FFE),
        ] {
            assert_eq!(p.to_raw(), raw, "{p:?}");
        }
        assert_eq!(
            BitstreamPointer::ValidBits(0x124).header_octets(),
            [0x01, 0x23]
        );
    }

    #[test]
    fn decode_and_spare() {
        // req: AOS-017, API-031, API-032, GEN-043, GEN-013
        let field = [0x00, 0x05, 0xF8, 0x00];
        let b = Bpdu::decode(&field, STRICT).unwrap();
        assert_eq!(
            (b.pointer(), b.bitstream_data_pointer()),
            (BitstreamPointer::ValidBits(6), 5)
        );
        assert_eq!((b.data_zone(), b.spare()), (&field[2..], 0));
        assert_eq!(b.data_zone().as_ptr(), field[2..].as_ptr(), "zero-copy");
        assert!(b.anomalies().is_empty());
        let spare = [0xBF, 0xFF, 0];
        let e = Bpdu::decode(&spare, STRICT).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::ReservedValue,
                Some(field::BPDU_SPARE),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 2
                }
            )
        );
        let b = Bpdu::decode(&spare, LENIENT).unwrap();
        assert_eq!((b.pointer(), b.spare()), (BitstreamPointer::AllValid, 2));
        assert_eq!(
            b.anomalies(),
            AnomalySet::from_flags(&[AosAnomaly::BpduSpareSet])
        );
        for mode in [STRICT, LENIENT] {
            let e = Bpdu::decode(&[0xC0, 0x08, 0], mode).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::Malformed, Some(field::BITSTREAM_DATA_POINTER))
            );
            let e = Bpdu::decode(&[0x3F, 0xFF], mode).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
            );
        }
    }
}
