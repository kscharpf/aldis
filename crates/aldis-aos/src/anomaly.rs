//! Lenient-mode anomaly flags (732.0-B-5 §4.1.2.5.5.3, §4.1.4.3.2.2; 732.0-B-4 §4.1.2.5.4,
//! §4.1.4.2.2; API-032, API-033, GEN-043).
//!
//! The flags and their bit indices are listed in `docs/standards/aos-frame.md` §5 (API-033). A
//! pointer outside its zone is `Malformed`, and a corrected header is reported by
//! `corrected_symbols()`, in both modes; neither has a flag (AOS-014, AOS-017, OI-33).

use aldis_core::{AnomalySet, DecodeMode, Error, Result};

aldis_core::anomaly_flags! {
    /// A deviation a lenient AOS Transfer Frame, `M_PDU`, or `B_PDU` decoder accepts and reports
    /// (API-032).
    pub enum AosAnomaly {
        /// Bit 0. Virtual Channel Frame Count Cycle other than `0000` with its Use Flag `0`
        /// (732.0 §4.1.2.5.5.3; AOS-012).
        CycleNonZeroWhileUnused = 0,
        /// Bit 1. `B_PDU` Reserved Spare other than `00` (732.0 §4.1.4.3.2.2).
        BpduSpareSet = 1,
        /// Bit 2. In `Issue4Compatible`, Primary Header bits 42–43 other than `00` (AOS-002) or
        /// an `M_PDU` Reserved Spare other than `00000` (AOS-003) (732.0-B-4 §4.1.2.5.4,
        /// §4.1.4.2.2).
        Issue4SpareSet = 2,
    }
}

/// The deviations found while decoding one data unit: in lenient mode the flags, in strict mode
/// the error of the first (API-031, API-032, GEN-043).
///
/// A decoder notes a deviation where it finds it and goes on with the checks that fail in both
/// modes, so that strict and lenient report the same error for an input that is invalid in both.
#[derive(Debug)]
pub(crate) struct Deviations {
    mode: DecodeMode,
    anomalies: AnomalySet<AosAnomaly>,
    strict: Option<Error>,
}

impl Deviations {
    /// No deviation yet.
    pub(crate) const fn new(mode: DecodeMode) -> Deviations {
        Deviations {
            mode,
            anomalies: AnomalySet::empty(),
            strict: None,
        }
    }

    /// Notes `flag`, which strict mode reports as `error`.
    pub(crate) fn note(&mut self, flag: AosAnomaly, error: Error) {
        match self.mode {
            DecodeMode::Strict => {
                if self.strict.is_none() {
                    self.strict = Some(error);
                }
            }
            DecodeMode::Lenient => self.anomalies.insert(flag),
        }
    }

    /// The flags noted, or in strict mode the error of the first deviation.
    pub(crate) const fn finish(self) -> Result<AnomalySet<AosAnomaly>> {
        match self.strict {
            Some(e) => Err(e),
            None => Ok(self.anomalies),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{AosAnomaly, Deviations};
    use aldis_core::{Anomaly, AnomalySet, DecodeMode, Error, ErrorKind};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043, API-033
        assert_eq!(
            AosAnomaly::ALL,
            [
                AosAnomaly::CycleNonZeroWhileUnused,
                AosAnomaly::BpduSpareSet,
                AosAnomaly::Issue4SpareSet
            ]
        );
        assert!(AosAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 1, 2]));
        assert_eq!(AosAnomaly::from_bit(3), None);
        assert_eq!(AnomalySet::from_flags(AosAnomaly::ALL).bits(), 0b111);
    }

    #[test]
    fn strict_reports_the_first_deviation() {
        // req: API-031, API-032, GEN-043
        let first = Error::new(ErrorKind::InvalidField);
        let second = Error::new(ErrorKind::ReservedValue);
        assert_eq!(
            Deviations::new(DecodeMode::Strict).finish(),
            Ok(AnomalySet::empty())
        );
        let mut strict = Deviations::new(DecodeMode::Strict);
        strict.note(AosAnomaly::CycleNonZeroWhileUnused, first);
        strict.note(AosAnomaly::BpduSpareSet, second);
        assert_eq!(strict.finish(), Err(first));
        let mut lenient = Deviations::new(DecodeMode::Lenient);
        lenient.note(AosAnomaly::CycleNonZeroWhileUnused, first);
        lenient.note(AosAnomaly::Issue4SpareSet, second);
        assert_eq!(
            lenient.finish(),
            Ok(AnomalySet::from_flags(&[
                AosAnomaly::CycleNonZeroWhileUnused,
                AosAnomaly::Issue4SpareSet
            ]))
        );
    }
}
