//! Managed parameters of the Encapsulation Packet Protocol (133.1 §5 Table 5-1; EPP-010,
//! GEN-060, GEN-061, RS-012, API-040, API-041).
//!
//! The valid EPI and extended-EPI sets are supplied by the caller: Aldis embeds no SANA
//! registry (M1 D4, OI-06).

use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::header::{Epi, EpiExtension};

/// Widens `u32` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u32 -> u64")]
const fn wide(n: u32) -> u64 {
    n as u64
}

/// Largest Encapsulated Data Field, 2³² − 9 octets (133.1 Table 4-2).
pub const MAX_DATA_LEN: u32 = u32::MAX - 8;

/// A set of Encapsulation Protocol IDs (3-bit values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EpiSet(u8);

impl EpiSet {
    /// The empty set.
    pub const EMPTY: EpiSet = EpiSet(0);

    /// The set with `epi` added.
    #[must_use]
    pub const fn with(self, epi: Epi) -> EpiSet {
        EpiSet(self.0 | (1 << epi.get()))
    }

    /// Whether `epi` is in the set.
    #[must_use]
    pub const fn contains(self, epi: Epi) -> bool {
        self.0 & (1 << epi.get()) != 0
    }

    /// The set as a bit mask: bit `n` is set when EPI `n` is a member.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// A set of extended Encapsulation Protocol IDs (4-bit Protocol ID Extension values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ExtendedEpiSet(u16);

impl ExtendedEpiSet {
    /// The empty set.
    pub const EMPTY: ExtendedEpiSet = ExtendedEpiSet(0);

    /// The set with `extension` added.
    #[must_use]
    pub const fn with(self, extension: EpiExtension) -> ExtendedEpiSet {
        ExtendedEpiSet(self.0 | (1 << extension.get()))
    }

    /// Whether `extension` is in the set.
    #[must_use]
    pub const fn contains(self, extension: EpiExtension) -> bool {
        self.0 & (1 << extension.get()) != 0
    }

    /// The set as a bit mask: bit `n` is set when extended EPI `n` is a member.
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
    /// A configuration accepting data units of `min_data_len..=max_data_len` octets whose EPI
    /// is in `valid_epis` and, for EPI `110`, whose extension is in `valid_extended_epis`.
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

    /// Minimum data unit length in octets.
    #[must_use]
    pub const fn min_data_len(&self) -> u32 {
        self.min_data_len
    }

    /// Maximum data unit length in octets.
    #[must_use]
    pub const fn max_data_len(&self) -> u32 {
        self.max_data_len
    }

    /// Valid Encapsulation Protocol IDs.
    #[must_use]
    pub const fn valid_epis(&self) -> EpiSet {
        self.valid_epis
    }

    /// Valid extended Encapsulation Protocol IDs.
    #[must_use]
    pub const fn valid_extended_epis(&self) -> ExtendedEpiSet {
        self.valid_extended_epis
    }

    /// Checks a data unit against the managed parameters, at the sending end before encoding
    /// (133.1 §4.2.1.1.2) or the receiving end after decoding (EPP-010). An Idle Packet carries
    /// no data unit for a user (133.1 §4.1.3.1.3, §4.3.1.1.3) and always passes.
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
        let len = u64::try_from(data_len).unwrap_or(u64::MAX);
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
}

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
