//! Zero-copy Encapsulation Packet view (133.1 §4.1; EPP-001…EPP-007, EPP-011, API-010, API-011,
//! API-013, API-014, GEN-013, GEN-043).

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::anomaly::EppAnomaly;
use crate::config::EppConfig;
use crate::field;
use crate::header::{EncapsulationHeader, Epi, EpiExtension, HeaderLen, UserDefined};

/// A decoded Encapsulation Packet borrowing its data field from the input (133.1 §4.1;
/// EPP-001…EPP-007, EPP-011, GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncapsulationPacket<'a> {
    header: EncapsulationHeader,
    data: &'a [u8],
    anomalies: AnomalySet<EppAnomaly>,
}

/// `ReservedValue` for a field that must be zero, with the value found.
const fn reserved(field: FieldId, found: u64) -> Error {
    Error::new(ErrorKind::ReservedValue)
        .with_field(field)
        .with_detail(ErrorDetail::Expected { expected: 0, found })
}

impl<'a> EncapsulationPacket<'a> {
    /// Decodes one packet from the start of `bytes`; any octets after it are ignored, so a stream
    /// can be parsed packet by packet with [`len`](Self::len) (133.1 §4.1.2; API-011, EPP-002).
    ///
    /// # Errors
    /// - `Incomplete` (`needed` = total octets required) if `bytes` is shorter than the header
    ///   or than the Packet Length (EPP-003).
    /// - `UnsupportedVersion` if the version is not `111` (EPP-001).
    /// - `LengthMismatch` if the Packet Length is below the header length (EPP-003).
    /// - `InvalidField` (`epi`) for a 1-octet header or an empty data field whose EPI is not
    ///   idle, or for EPI `110` in a 2-octet header, which has no extension field to identify
    ///   the protocol; in both modes (EPP-004, EPP-005, EPP-012).
    /// - `ReservedValue` in strict mode for a non-zero Protocol ID Extension with EPI ≠ `110`
    ///   (EPP-006) or a non-zero CCSDS Defined Field (EPP-007). Lenient mode reports these as
    ///   [`EppAnomaly`] flags instead (API-032).
    pub fn decode(bytes: &'a [u8], mode: DecodeMode) -> Result<EncapsulationPacket<'a>> {
        let header = EncapsulationHeader::parse(bytes)?;
        let total = usize::try_from(header.packet_len()).unwrap_or(usize::MAX);
        let data = bytes
            .get(header.header_len().octets()..total)
            .ok_or(Error::incomplete(total))?;
        let mut anomalies = AnomalySet::empty();
        if let Some(ext) = header.extension()
            && ext.get() != 0
            && header.epi() != Epi::EXTENDED
        {
            match mode {
                DecodeMode::Strict => return Err(reserved(field::EPI_EXTENSION, ext.get().into())),
                DecodeMode::Lenient => anomalies.insert(EppAnomaly::NonZeroEpiExtension),
            }
        }
        if let Some(ccsds) = header.ccsds_defined()
            && ccsds != 0
        {
            match mode {
                DecodeMode::Strict => return Err(reserved(field::CCSDS_DEFINED, ccsds.into())),
                DecodeMode::Lenient => anomalies.insert(EppAnomaly::NonZeroCcsdsDefined),
            }
        }
        Ok(EncapsulationPacket {
            header,
            data,
            anomalies,
        })
    }

    /// Total length of the packet that starts `prefix`, from its header alone (133.1 §4.1.2.8,
    /// API-013). It applies every check of [`decode`](Self::decode) that needs only the header and
    /// holds in both modes, so the two never disagree on a header.
    ///
    /// # Errors
    /// `Incomplete` if `prefix` is shorter than the header; `UnsupportedVersion`,
    /// `InvalidField` (EPP-004, EPP-005, EPP-012), or `LengthMismatch` as for
    /// [`decode`](Self::decode).
    pub fn peek_length(prefix: &[u8]) -> Result<usize> {
        let header = EncapsulationHeader::parse(prefix)?;
        Ok(usize::try_from(header.packet_len()).unwrap_or(usize::MAX))
    }

    /// Checks the packet against the managed parameters (133.1 §4.3.1.1.3, §5 Table 5-1, EPP-010);
    /// see [`EppConfig::check`].
    ///
    /// # Errors
    /// As for [`EppConfig::check`].
    pub fn validate(&self, config: &EppConfig) -> Result<()> {
        config.check(self.epi(), self.extension(), self.data.len())
    }

    /// The header as plain values (133.1 §4.1.2, API-014).
    #[must_use]
    pub const fn header(&self) -> EncapsulationHeader {
        self.header
    }

    /// Encapsulation Protocol ID (133.1 §4.1.2.3, EPP-011).
    #[must_use]
    pub const fn epi(&self) -> Epi {
        self.header.epi()
    }

    /// Whether this is an Encapsulation Idle Packet, EPI `000` (133.1 §4.1.3.1.3, EPP-011).
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.header.epi().is_idle()
    }

    /// Protocol ID Extension, if the header has one (133.1 §4.1.2.6, EPP-011).
    #[must_use]
    pub const fn extension(&self) -> Option<EpiExtension> {
        self.header.extension()
    }

    /// User Defined field, if the header has one (133.1 §4.1.2.5, EPP-011).
    #[must_use]
    pub const fn user_defined(&self) -> Option<UserDefined> {
        self.header.user_defined()
    }

    /// CCSDS Defined Field, if the header has one; exposed even when non-zero (133.1 §4.1.2.7,
    /// EPP-007).
    #[must_use]
    pub const fn ccsds_defined(&self) -> Option<u16> {
        self.header.ccsds_defined()
    }

    /// Header length (133.1 §4.1.2.4, EPP-011).
    #[must_use]
    pub const fn header_len(&self) -> HeaderLen {
        self.header.header_len()
    }

    /// The Encapsulated Data Field, borrowed from the input (133.1 §4.1.3; EPP-011, GEN-013); empty
    /// for idle packets without data.
    #[must_use]
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Octets consumed from the input: the total packet length (133.1 §4.1.2.8.2, API-011).
    #[must_use]
    pub fn len(&self) -> usize {
        usize::try_from(self.header.packet_len()).unwrap_or(usize::MAX)
    }

    /// Always `false`: a packet has at least a 1-octet header (133.1 §4.1.1.2, API-011).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (133.1 §4.1.2.6.3, §4.1.2.7.2;
    /// API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<EppAnomaly> {
        self.anomalies
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::field;

    use super::EncapsulationPacket;
    use crate::anomaly::EppAnomaly;
    use crate::header::{Epi, EpiExtension, HeaderLen, UserDefined};
    use aldis_core::{AnomalySet, DecodeMode, ErrorDetail, ErrorKind};

    #[test]
    fn view_accessors() {
        // req: EPP-011, API-010, API-014, GEN-013
        let bytes = [0xFA, 0x5F, 0x00, 0x06, 0x11, 0x22, 0x33];
        let p = EncapsulationPacket::decode(&bytes, DecodeMode::Strict).unwrap();
        assert_eq!(p.epi(), Epi::EXTENDED);
        assert!(!p.is_idle() && !p.is_empty());
        assert_eq!(p.extension().map(EpiExtension::get), Some(0xF));
        assert_eq!(p.user_defined().map(UserDefined::get), Some(5));
        assert_eq!(p.ccsds_defined(), None);
        assert_eq!(p.header_len(), HeaderLen::Four);
        assert_eq!(p.data(), &bytes[4..6]);
        assert_eq!(p.data().as_ptr(), bytes[4..].as_ptr(), "zero-copy");
        assert_eq!(p.len(), 6);
        assert_eq!(p.anomalies(), AnomalySet::empty());
        assert_eq!(p.header().packet_len(), 6);
        let idle = EncapsulationPacket::decode(&[0xE0], DecodeMode::Lenient).unwrap();
        assert!(idle.is_idle() && idle.data().is_empty() && idle.len() == 1);
    }

    #[test]
    fn errors_name_the_field() {
        // req: GEN-041, EPP-005, EPP-006, EPP-007
        let e = EncapsulationPacket::decode(&[0xE5, 0x02], DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::EPI))
        );
        let e = EncapsulationPacket::decode(&[0xE6, 0x03, 0x00, 0x05, 0x00], DecodeMode::Strict)
            .unwrap_err();
        assert_eq!(e.field(), Some(field::EPI_EXTENSION));
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 0,
                found: 3
            }
        );
        let e = EncapsulationPacket::decode(
            &[0xE7, 0x00, 0xBE, 0xEF, 0, 0, 0, 9, 0],
            DecodeMode::Strict,
        )
        .unwrap_err();
        assert_eq!(e.field(), Some(field::CCSDS_DEFINED));
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 0,
                found: 0xBEEF
            }
        );
        let e = EncapsulationPacket::decode(&[0xE5, 0x09, 0x00], DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(9))
        );
    }

    #[test]
    fn lenient_flags_both_anomalies() {
        // req: API-032, GEN-043, EPP-006, EPP-007
        let bytes = [0xE7, 0x01, 0x00, 0x01, 0, 0, 0, 9, 0xAB];
        let p = EncapsulationPacket::decode(&bytes, DecodeMode::Lenient).unwrap();
        assert!(p.anomalies().contains(EppAnomaly::NonZeroEpiExtension));
        assert!(p.anomalies().contains(EppAnomaly::NonZeroCcsdsDefined));
        assert_eq!(p.ccsds_defined(), Some(1));
    }
}
