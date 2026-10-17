//! Lenient-mode anomaly flags (API-032, API-033, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/encapsulation-packet.md` §3.

use aldis_core::Anomaly;

/// A deviation a lenient Encapsulation Packet decoder accepts and reports (API-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EppAnomaly {
    /// Bit 0. Non-zero Protocol ID Extension while the EPI is not `110`, which 133.1
    /// §4.1.2.6.3 forbids (EPP-006).
    NonZeroEpiExtension,
    /// Bit 1. Non-zero CCSDS Defined Field, by convention all zeros (133.1 §4.1.2.7.2, EPP-007).
    NonZeroCcsdsDefined,
    /// Bit 2. Encapsulated Data Field absent while the EPI is not `000` (idle), which 133.1
    /// §4.1.3.1.4–5 forbid (EPP-005).
    EmptyNonIdle,
}

impl Anomaly for EppAnomaly {
    fn bit(self) -> u8 {
        match self {
            EppAnomaly::NonZeroEpiExtension => 0,
            EppAnomaly::NonZeroCcsdsDefined => 1,
            EppAnomaly::EmptyNonIdle => 2,
        }
    }

    fn from_bit(bit: u8) -> Option<EppAnomaly> {
        match bit {
            0 => Some(EppAnomaly::NonZeroEpiExtension),
            1 => Some(EppAnomaly::NonZeroCcsdsDefined),
            2 => Some(EppAnomaly::EmptyNonIdle),
            _ => None,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::EppAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn stable_bit_indices() {
        // req: API-033, GEN-043
        let all = [
            EppAnomaly::NonZeroEpiExtension,
            EppAnomaly::NonZeroCcsdsDefined,
            EppAnomaly::EmptyNonIdle,
        ];
        for (bit, flag) in (0..).zip(all) {
            assert_eq!(flag.bit(), bit);
            assert_eq!(EppAnomaly::from_bit(bit), Some(flag));
        }
        assert_eq!(EppAnomaly::from_bit(3), None);
        let set: AnomalySet<EppAnomaly> = all.into_iter().collect();
        assert_eq!(set.bits(), 0b111);
        assert!(set.iter().eq(all));
    }
}
