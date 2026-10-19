//! Zero-copy Encapsulation Packet view (133.1 §4.1; EPP-001…EPP-007, EPP-011, API-010, API-011,
//! API-013, API-014, GEN-013, GEN-043).

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::anomaly::EppAnomaly;
use crate::config::EppConfig;
use crate::header::{EncapsulationHeader, Epi, EpiExtension, HeaderLen, UserDefined};

/// A decoded Encapsulation Packet borrowing its data field from the input (GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncapsulationPacket<'a> {
    header: EncapsulationHeader,
    data: &'a [u8],
    anomalies: AnomalySet<EppAnomaly>,
}

/// `ReservedValue` for a field that must be zero, with the value found.
const fn reserved(field: &'static str, found: u64) -> Error {
    Error::new(ErrorKind::ReservedValue)
        .with_field(FieldId::new(field))
        .with_detail(ErrorDetail::Expected { expected: 0, found })
}

impl<'a> EncapsulationPacket<'a> {
    /// Decodes one packet from the start of `bytes`; any octets after it are ignored, so a
    /// stream can be parsed packet by packet with [`len`](Self::len) (API-011, EPP-002).
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
                DecodeMode::Strict => return Err(reserved("epi_extension", ext.get().into())),
                DecodeMode::Lenient => anomalies.insert(EppAnomaly::NonZeroEpiExtension),
            }
        }
        if let Some(ccsds) = header.ccsds_defined()
            && ccsds != 0
        {
            match mode {
                DecodeMode::Strict => return Err(reserved("ccsds_defined", ccsds.into())),
                DecodeMode::Lenient => anomalies.insert(EppAnomaly::NonZeroCcsdsDefined),
            }
        }
        Ok(EncapsulationPacket {
            header,
            data,
            anomalies,
        })
    }

    /// Total length of the packet that starts `prefix`, from its header alone (API-013). It
    /// applies every check of [`decode`](Self::decode) that needs only the header and holds in
    /// both modes, so the two never disagree on a header.
    ///
    /// # Errors
    /// `Incomplete` if `prefix` is shorter than the header; `UnsupportedVersion`,
    /// `InvalidField` (EPP-004, EPP-005, EPP-012), or `LengthMismatch` as for
    /// [`decode`](Self::decode).
    pub fn peek_length(prefix: &[u8]) -> Result<usize> {
        let header = EncapsulationHeader::parse(prefix)?;
        Ok(usize::try_from(header.packet_len()).unwrap_or(usize::MAX))
    }

    /// Checks the packet against the managed parameters (EPP-010); see [`EppConfig::check`].
    ///
    /// # Errors
    /// As for [`EppConfig::check`].
    pub fn validate(&self, config: &EppConfig) -> Result<()> {
        config.check(self.epi(), self.extension(), self.data.len())
    }

    /// The header as plain values (API-014).
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

    /// Protocol ID Extension, if the header has one (EPP-011).
    #[must_use]
    pub const fn extension(&self) -> Option<EpiExtension> {
        self.header.extension()
    }

    /// User Defined field, if the header has one (EPP-011).
    #[must_use]
    pub const fn user_defined(&self) -> Option<UserDefined> {
        self.header.user_defined()
    }

    /// CCSDS Defined Field, if the header has one; exposed even when non-zero (EPP-007).
    #[must_use]
    pub const fn ccsds_defined(&self) -> Option<u16> {
        self.header.ccsds_defined()
    }

    /// Header length (133.1 §4.1.2.4, EPP-011).
    #[must_use]
    pub const fn header_len(&self) -> HeaderLen {
        self.header.header_len()
    }

    /// The Encapsulated Data Field, borrowed from the input (GEN-013); empty for idle packets
    /// without data.
    #[must_use]
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Octets consumed from the input: the total packet length (API-011).
    #[must_use]
    pub fn len(&self) -> usize {
        usize::try_from(self.header.packet_len()).unwrap_or(usize::MAX)
    }

    /// Always `false`: a packet has at least a 1-octet header (133.1 §4.1.1.2).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<EppAnomaly> {
        self.anomalies
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test buffer sizing on small bounded lengths (RS-018)"
)]
mod tests {
    extern crate std;
    use std::vec;

    use super::EncapsulationPacket;
    use crate::anomaly::EppAnomaly;
    use crate::config::ANY;
    use crate::header::{
        EncapsulationFields, Epi, EpiExtension, HeaderLen, HeaderSize, UserDefined,
    };
    use crate::writer::encode;
    use aldis_core::{AnomalySet, DecodeMode, ErrorDetail, ErrorKind, FieldId};
    use proptest::prelude::*;

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
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::InvalidField, Some("epi"))
        );
        let e = EncapsulationPacket::decode(&[0xE6, 0x03, 0x00, 0x05, 0x00], DecodeMode::Strict)
            .unwrap_err();
        assert_eq!(e.field().map(FieldId::name), Some("epi_extension"));
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
        assert_eq!(e.field().map(FieldId::name), Some("ccsds_defined"));
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

    fn fields() -> impl Strategy<Value = EncapsulationFields> {
        (0_u8..8, 0_u8..16, 0_u8..16).prop_map(|(epi, ext, user)| {
            let user = UserDefined::new(user).unwrap();
            if epi == 6 {
                EncapsulationFields::extended(EpiExtension::new(ext).unwrap())
                    .with_user_defined(user)
            } else {
                EncapsulationFields::new(Epi::new(epi).unwrap()).with_user_defined(user)
            }
        })
    }

    fn size() -> impl Strategy<Value = HeaderSize> {
        prop_oneof![
            Just(HeaderSize::Auto),
            Just(HeaderSize::Fixed(HeaderLen::One)),
            Just(HeaderSize::Fixed(HeaderLen::Two)),
            Just(HeaderSize::Fixed(HeaderLen::Four)),
            Just(HeaderSize::Fixed(HeaderLen::Eight)),
        ]
    }

    proptest! {
        #[test]
        fn encode_then_decode_round_trips(
            f in fields(),
            s in size(),
            data in proptest::collection::vec(any::<u8>(), 0..300),
            tail in proptest::collection::vec(any::<u8>(), 0..4),
        ) {
            // req: API-020, API-011, EPP-002, VER-023
            let mut out = vec![0_u8; data.len() + 8];
            let Ok(unit) = ANY.data_unit(f, &data) else { return Ok(()) };
            let Ok(n) = encode(&unit, s, &mut out) else { return Ok(()) };
            out.truncate(n);
            out.extend_from_slice(&tail);
            let p = EncapsulationPacket::decode(&out, DecodeMode::Strict).unwrap();
            prop_assert_eq!(p.epi(), f.epi());
            prop_assert_eq!(p.data(), data.as_slice());
            prop_assert_eq!(p.len(), n);
            prop_assert_eq!(EncapsulationPacket::peek_length(&out), Ok(n));
            if p.header_len().has_extension() {
                prop_assert_eq!(p.extension(), Some(f.extension()));
                prop_assert_eq!(p.user_defined(), Some(f.user_defined()));
            }
        }

        #[test]
        fn strict_decode_re_encodes_exactly(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            // req: API-020, API-010, GEN-020, VER-023
            let Ok(p) = EncapsulationPacket::decode(&bytes, DecodeMode::Strict) else { return Ok(()) };
            let fields = EncapsulationFields::from_parts(
                p.epi(),
                p.extension().unwrap_or_default(),
                p.user_defined().unwrap_or_default(),
            )
            .unwrap();
            let mut out = vec![0_u8; p.len()];
            let unit = ANY.data_unit(fields, p.data()).unwrap();
            let n = encode(&unit, HeaderSize::Fixed(p.header_len()), &mut out).unwrap();
            prop_assert_eq!(&out[..n], &bytes[..p.len()]);
            let lenient = EncapsulationPacket::decode(&bytes, DecodeMode::Lenient).unwrap();
            prop_assert_eq!(lenient, p);
        }

        #[test]
        fn lenient_accepts_whatever_strict_accepts_and_more(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            // req: API-030, API-032, GEN-020, VER-023
            let strict = EncapsulationPacket::decode(&bytes, DecodeMode::Strict);
            let lenient = EncapsulationPacket::decode(&bytes, DecodeMode::Lenient);
            match (strict, lenient) {
                (Ok(s), Ok(l)) => prop_assert_eq!(s, l),
                (Ok(_), Err(e)) => prop_assert!(false, "lenient rejected a strict packet: {e}"),
                (Err(s), Ok(l)) => {
                    prop_assert_eq!(s.kind(), ErrorKind::ReservedValue);
                    prop_assert!(!l.anomalies().is_empty());
                }
                (Err(s), Err(l)) => prop_assert_eq!(s, l),
            }
        }
    }
}
