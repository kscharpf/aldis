//! Managed parameters of the Encapsulation Packet Protocol (133.1 §5 Table 5-1; EPP-010,
//! GEN-060, GEN-061, RS-012, API-040, API-041).
//!
//! The valid EPI and extended-EPI sets are supplied by the caller: Aldis embeds no SANA
//! registry (M1 D4, OI-06).

use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::header::{
    EncapsulationFields, EncapsulationHeader, Epi, EpiExtension, HeaderSize, invalid, len_u64,
};

/// Widens `u32` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u32 -> u64")]
const fn wide(n: u32) -> u64 {
    n as u64
}

/// Largest Encapsulated Data Field, 2³² − 9 octets (133.1 §4.1.2.8, Table 4-2; EPP-003, EPP-010).
pub const MAX_DATA_LEN: u32 = u32::MAX - 8;

/// A set of Encapsulation Protocol IDs, 3-bit values: the managed parameter "Valid
/// Encapsulation Protocol Identifiers" (133.1 Table 5-1, EPP-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EpiSet(u8);

impl EpiSet {
    /// The empty set (133.1 §5 Table 5-1, EPP-010).
    pub const EMPTY: EpiSet = EpiSet(0);

    /// The set whose bit `n` is set for each member EPI `n`; every `u8` is a valid set (133.1 §5
    /// Table 5-1, EPP-010).
    #[must_use]
    pub const fn from_bits(bits: u8) -> EpiSet {
        EpiSet(bits)
    }

    /// The set with `epi` added (133.1 §5 Table 5-1, EPP-010).
    #[must_use]
    pub const fn with(self, epi: Epi) -> EpiSet {
        EpiSet(self.0 | (1 << epi.get()))
    }

    /// Whether `epi` is in the set (133.1 §5 Table 5-1, EPP-010).
    #[must_use]
    pub const fn contains(self, epi: Epi) -> bool {
        self.0 & (1 << epi.get()) != 0
    }

    /// The set as a bit mask: bit `n` is set when EPI `n` is a member (133.1 §5 Table 5-1,
    /// EPP-010).
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// A set of extended Encapsulation Protocol IDs, 4-bit Protocol ID Extension values: the
/// managed parameter "Valid Extended Encapsulation Protocol Identifiers" (133.1 Table 5-1,
/// EPP-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ExtendedEpiSet(u16);

impl ExtendedEpiSet {
    /// The empty set (133.1 §5 Table 5-1, EPP-010).
    pub const EMPTY: ExtendedEpiSet = ExtendedEpiSet(0);

    /// The set whose bit `n` is set for each member extended EPI `n`; every `u16` is a valid set
    /// (133.1 §5 Table 5-1, EPP-010).
    #[must_use]
    pub const fn from_bits(bits: u16) -> ExtendedEpiSet {
        ExtendedEpiSet(bits)
    }

    /// The set with `extension` added (133.1 §5 Table 5-1, EPP-010).
    #[must_use]
    pub const fn with(self, extension: EpiExtension) -> ExtendedEpiSet {
        ExtendedEpiSet(self.0 | (1 << extension.get()))
    }

    /// Whether `extension` is in the set (133.1 §5 Table 5-1, EPP-010).
    #[must_use]
    pub const fn contains(self, extension: EpiExtension) -> bool {
        self.0 & (1 << extension.get()) != 0
    }

    /// The set as a bit mask: bit `n` is set when extended EPI `n` is a member (133.1 §5 Table 5-1,
    /// EPP-010).
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }
}

/// Managed parameters (133.1 Table 5-1), validated at construction and immutable (GEN-060,
/// GEN-061).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EppConfig {
    min_data_len: u32,
    max_data_len: u32,
    valid_epis: EpiSet,
    valid_extended_epis: ExtendedEpiSet,
}

impl EppConfig {
    /// A configuration accepting data units of `min_data_len..=max_data_len` octets whose EPI is in
    /// `valid_epis` and, for EPI `110`, whose extension is in `valid_extended_epis` (133.1 §5 Table
    /// 5-1; EPP-010, GEN-060).
    ///
    /// # Errors
    /// `InvalidConfig` if `min_data_len > max_data_len` or `max_data_len` exceeds
    /// [`MAX_DATA_LEN`] (API-040).
    pub const fn new(
        min_data_len: u32,
        max_data_len: u32,
        valid_epis: EpiSet,
        valid_extended_epis: ExtendedEpiSet,
    ) -> Result<EppConfig> {
        if min_data_len > max_data_len {
            return Err(Error::new(ErrorKind::InvalidConfig)
                .with_field(FieldId::new("min_data_len"))
                .with_detail(ErrorDetail::Expected {
                    expected: wide(max_data_len),
                    found: wide(min_data_len),
                }));
        }
        if max_data_len > MAX_DATA_LEN {
            return Err(Error::new(ErrorKind::InvalidConfig)
                .with_field(FieldId::new("max_data_len"))
                .with_detail(ErrorDetail::Expected {
                    expected: wide(MAX_DATA_LEN),
                    found: wide(max_data_len),
                }));
        }
        Ok(EppConfig {
            min_data_len,
            max_data_len,
            valid_epis,
            valid_extended_epis,
        })
    }

    /// Minimum data unit length in octets (133.1 Table 5-1, EPP-010).
    #[must_use]
    pub const fn min_data_len(&self) -> u32 {
        self.min_data_len
    }

    /// Maximum data unit length in octets (133.1 Table 5-1, EPP-010).
    #[must_use]
    pub const fn max_data_len(&self) -> u32 {
        self.max_data_len
    }

    /// Valid Encapsulation Protocol IDs (133.1 Table 5-1, EPP-010).
    #[must_use]
    pub const fn valid_epis(&self) -> EpiSet {
        self.valid_epis
    }

    /// Valid extended Encapsulation Protocol IDs (133.1 Table 5-1, EPP-010).
    #[must_use]
    pub const fn valid_extended_epis(&self) -> ExtendedEpiSet {
        self.valid_extended_epis
    }

    /// Checks a data unit's protocol identifiers and length against the managed parameters
    /// (EPP-010). Used at the receiving end by
    /// [`EncapsulationPacket::validate`](crate::view::EncapsulationPacket::validate); the
    /// sending end goes through [`data_unit`](Self::data_unit) or [`header`](Self::header),
    /// which encoders require (133.1 §4.2.1.1.2). An Idle Packet carries no data unit for a user
    /// (133.1 §4.1.3.1.3, §4.3.1.1.3) and always passes.
    ///
    /// # Errors
    /// - `UnknownChannel` (`epi`) if the EPI is not in the valid set.
    /// - `UnknownChannel` (`epi_extension`) if the EPI is `110` and the extension is absent or
    ///   not in the valid extended set.
    /// - `OutOfRange` (`data_len`) if `data_len` is outside `min_data_len..=max_data_len`.
    pub fn check(&self, epi: Epi, extension: Option<EpiExtension>, data_len: usize) -> Result<()> {
        if epi.is_idle() {
            return Ok(());
        }
        self.check_ids(epi, extension)?;
        self.check_len(data_len)
    }

    /// The identifier part of [`check`](Self::check) for a non-idle EPI.
    fn check_ids(&self, epi: Epi, extension: Option<EpiExtension>) -> Result<()> {
        if !self.valid_epis.contains(epi) {
            return Err(Error::new(ErrorKind::UnknownChannel).with_field(FieldId::new("epi")));
        }
        if epi == Epi::EXTENDED
            && !extension.is_some_and(|ext| self.valid_extended_epis.contains(ext))
        {
            return Err(
                Error::new(ErrorKind::UnknownChannel).with_field(FieldId::new("epi_extension"))
            );
        }
        Ok(())
    }

    /// The length part of [`check`](Self::check) for a non-idle EPI.
    pub(crate) fn check_len(&self, data_len: usize) -> Result<()> {
        let len = len_u64(data_len);
        if len < u64::from(self.min_data_len) {
            return Err(Error::out_of_range(
                "data_len",
                self.min_data_len.into(),
                len,
            ));
        }
        if len > u64::from(self.max_data_len) {
            return Err(Error::out_of_range(
                "data_len",
                self.max_data_len.into(),
                len,
            ));
        }
        Ok(())
    }

    /// Checks the identifiers in `fields` for the sending end (133.1 §4.2.1.1.2); idle fields
    /// always pass.
    pub(crate) fn check_fields(&self, fields: EncapsulationFields) -> Result<()> {
        if fields.epi().is_idle() {
            return Ok(());
        }
        self.check_ids(fields.epi(), Some(fields.extension()))
    }

    /// A data unit ready for encoding: `fields` and `data` checked once against the managed
    /// parameters at the sending end (133.1 §4.2.1.1.2, EPP-010, RS-020).
    ///
    /// # Errors
    /// The errors of [`check`](Self::check), and `InvalidField` (`epi`) for an empty data field
    /// with a non-idle EPI (EPP-005).
    pub fn data_unit<'a>(
        &self,
        fields: EncapsulationFields,
        data: &'a [u8],
    ) -> Result<DataUnit<'a>> {
        self.check(fields.epi(), Some(fields.extension()), data.len())?;
        if data.is_empty() && !fields.epi().is_idle() {
            return Err(invalid("epi"));
        }
        Ok(DataUnit { fields, data })
    }

    /// The header for a checked data unit of `data_len` octets, without the data: for streaming
    /// encoders and for lengths too large to hold in memory (133.1 §4.2.1.1.2, EPP-008,
    /// EPP-010).
    ///
    /// # Errors
    /// The errors of [`check`](Self::check), and those of the header size policy: `InvalidField`
    /// (`epi`) for an empty non-idle data field or `Fixed(One)` with a non-idle EPI;
    /// `InvalidField` (`header_size`) for a header too small for the fields; `OutOfRange`
    /// (`packet_len`) when the total length does not fit.
    pub fn header(
        &self,
        fields: EncapsulationFields,
        size: HeaderSize,
        data_len: usize,
    ) -> Result<EncapsulationHeader> {
        self.check(fields.epi(), Some(fields.extension()), data_len)?;
        EncapsulationHeader::for_data(fields, size, data_len)
    }
}

/// A user data unit checked against the managed parameters (133.1 §4.2.1.1.2, EPP-010).
///
/// Built only by [`EppConfig::data_unit`], so holding one proves its EPI, extended EPI, and length
/// are valid for the configuration (RS-020). [`encode`](crate::writer::encode) requires one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DataUnit<'a> {
    fields: EncapsulationFields,
    data: &'a [u8],
}

impl<'a> DataUnit<'a> {
    /// The header fields (133.1 §4.1.2, EPP-008).
    #[must_use]
    pub const fn fields(&self) -> EncapsulationFields {
        self.fields
    }

    /// The data to encapsulate (133.1 §4.1.3, EPP-010).
    #[must_use]
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }
}

/// Accepts every EPI, every extended EPI, and every data unit length: for encoder tests that
/// are not about the managed parameters.
#[cfg(test)]
pub(crate) const ANY: EppConfig = EppConfig {
    min_data_len: 0,
    max_data_len: MAX_DATA_LEN,
    valid_epis: EpiSet(0xFF),
    valid_extended_epis: ExtendedEpiSet(0xFFFF),
};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{EpiSet, EppConfig, ExtendedEpiSet, MAX_DATA_LEN};
    use crate::header::{Epi, EpiExtension};
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};

    const EXT9: EpiExtension = match EpiExtension::new(9) {
        Some(e) => e,
        None => panic!("9 fits in 4 bits"),
    };

    #[test]
    fn sets_hold_members() {
        // req: EPP-010
        let s = EpiSet::EMPTY.with(Epi::MISSION_SPECIFIC).with(Epi::IDLE);
        assert_eq!(s.bits(), 0b1000_0001);
        assert!(s.contains(Epi::IDLE) && !s.contains(Epi::EXTENDED));
        assert_eq!(s.with(Epi::IDLE), s, "adding a member again keeps it");
        let x = ExtendedEpiSet::default().with(EXT9);
        assert_eq!(x.bits(), 1 << 9);
        assert_eq!(x.with(EXT9), x, "adding a member again keeps it");
        assert_eq!(EpiSet::from_bits(0b1000_0001), s);
        assert_eq!(ExtendedEpiSet::from_bits(1 << 9), x);
        assert_eq!(
            (
                EpiSet::from_bits(0xFF).bits(),
                ExtendedEpiSet::from_bits(0xFFFF).bits()
            ),
            (0xFF, 0xFFFF)
        );
        assert!(x.contains(EXT9) && !x.contains(EpiExtension::default()));
    }

    #[test]
    fn config_is_const_constructible_and_validated() {
        // req: EPP-010, RS-012, GEN-060, GEN-061, API-040
        const CFG: Result<EppConfig, aldis_core::Error> = EppConfig::new(
            3,
            10,
            EpiSet::EMPTY.with(Epi::EXTENDED),
            ExtendedEpiSet::EMPTY.with(EXT9),
        );
        let cfg = CFG.unwrap();
        assert_eq!((cfg.min_data_len(), cfg.max_data_len()), (3, 10));
        assert_eq!(cfg.valid_epis().bits(), 1 << 6);
        assert_eq!(cfg.valid_extended_epis().bits(), 1 << 9);
        let e = EppConfig::new(11, 10, EpiSet::EMPTY, ExtendedEpiSet::EMPTY).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidConfig);
        assert_eq!(e.field().map(FieldId::name), Some("min_data_len"));
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 10,
                found: 11
            }
        );
        let e =
            EppConfig::new(0, MAX_DATA_LEN + 1, EpiSet::EMPTY, ExtendedEpiSet::EMPTY).unwrap_err();
        assert_eq!(e.field().map(FieldId::name), Some("max_data_len"));
    }

    #[test]
    fn data_units_and_headers_are_checked_at_the_sending_end() {
        // req: EPP-010, EPP-005, RS-020
        use crate::header::{EncapsulationFields, HeaderLen, HeaderSize};
        let cfg = EppConfig::new(
            1,
            4,
            EpiSet::EMPTY
                .with(Epi::EXTENDED)
                .with(Epi::MISSION_SPECIFIC),
            ExtendedEpiSet::EMPTY.with(EXT9),
        )
        .unwrap();
        let mission = EncapsulationFields::new(Epi::MISSION_SPECIFIC);
        let unit = cfg.data_unit(mission, &[1, 2]).unwrap();
        assert_eq!((unit.fields(), unit.data()), (mission, &[1, 2][..]));
        let idle = EncapsulationFields::new(Epi::IDLE);
        assert!(
            cfg.data_unit(idle, &[0; 9]).is_ok(),
            "idle data is not a user data unit"
        );
        assert!(cfg.data_unit(idle, &[]).is_ok());
        for (fields, data, kind, field) in [
            (
                EncapsulationFields::new(Epi::new(1).unwrap()),
                &[1][..],
                ErrorKind::UnknownChannel,
                "epi",
            ),
            (
                EncapsulationFields::new(Epi::EXTENDED),
                &[1][..],
                ErrorKind::UnknownChannel,
                "epi_extension",
            ),
            (
                mission,
                &[1, 2, 3, 4, 5][..],
                ErrorKind::OutOfRange,
                "data_len",
            ),
            (mission, &[][..], ErrorKind::OutOfRange, "data_len"),
        ] {
            let e = cfg.data_unit(fields, data).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (kind, Some(field))
            );
        }
        let any_len = EppConfig::new(
            0,
            4,
            EpiSet::EMPTY.with(Epi::MISSION_SPECIFIC),
            ExtendedEpiSet::EMPTY,
        )
        .unwrap();
        let e = any_len.data_unit(mission, &[]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::InvalidField, Some("epi"))
        );
        let h = cfg
            .header(EncapsulationFields::extended(EXT9), HeaderSize::Auto, 4)
            .unwrap();
        assert_eq!((h.header_len(), h.packet_len()), (HeaderLen::Four, 8));
        assert_eq!(
            cfg.header(mission, HeaderSize::Auto, 5).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            cfg.header(mission, HeaderSize::Fixed(HeaderLen::One), 1)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidField
        );
    }

    #[test]
    fn check_rules() {
        // req: EPP-010
        let cfg = EppConfig::new(
            2,
            4,
            EpiSet::EMPTY.with(Epi::EXTENDED),
            ExtendedEpiSet::EMPTY.with(EXT9),
        )
        .unwrap();
        assert_eq!(cfg.check(Epi::IDLE, None, 100), Ok(()));
        assert_eq!(cfg.check(Epi::EXTENDED, Some(EXT9), 3), Ok(()));
        let e = cfg.check(Epi::MISSION_SPECIFIC, None, 3).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::UnknownChannel, Some("epi"))
        );
        let e = cfg.check(Epi::EXTENDED, None, 3).unwrap_err();
        assert_eq!(e.field().map(FieldId::name), Some("epi_extension"));
        let e = cfg.check(Epi::EXTENDED, Some(EXT9), 1).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 1
            }
        );
        let e = cfg.check(Epi::EXTENDED, Some(EXT9), 5).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 4,
                found: 5
            }
        );
    }
}
