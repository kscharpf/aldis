//! Lenient-mode anomaly flags (API-032, API-033, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/time-codes.md` §7.

use aldis_core::Anomaly;

/// A deviation a lenient time-code decoder accepts and reports (API-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TimeAnomaly {
    /// Bit 0. P-field time code ID `000`, `011`, or `111`, reserved by 301.0 §3.1.1 (TIME-002).
    ReservedTimeCodeId,
    /// Bit 1. CDS submillisecond length code `11`, reserved by 301.0 §3.3.2 (TIME-020).
    ReservedSubmsCode,
    /// Bit 2. Non-zero high nibble of the CCS day-of-year segment, set to zero by 301.0
    /// §3.4.1.2 (TIME-030).
    NonZeroDoyHighNibble,
}

impl Anomaly for TimeAnomaly {
    fn bit(self) -> u8 {
        match self {
            TimeAnomaly::ReservedTimeCodeId => 0,
            TimeAnomaly::ReservedSubmsCode => 1,
            TimeAnomaly::NonZeroDoyHighNibble => 2,
        }
    }

    fn from_bit(bit: u8) -> Option<TimeAnomaly> {
        match bit {
            0 => Some(TimeAnomaly::ReservedTimeCodeId),
            1 => Some(TimeAnomaly::ReservedSubmsCode),
            2 => Some(TimeAnomaly::NonZeroDoyHighNibble),
            _ => None,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::TimeAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn stable_bit_indices() {
        // req: API-033, GEN-043
        let all = [
            TimeAnomaly::ReservedTimeCodeId,
            TimeAnomaly::ReservedSubmsCode,
            TimeAnomaly::NonZeroDoyHighNibble,
        ];
        for (bit, flag) in (0..).zip(all) {
            assert_eq!(flag.bit(), bit);
            assert_eq!(TimeAnomaly::from_bit(bit), Some(flag));
        }
        assert_eq!(TimeAnomaly::from_bit(3), None);
        let set: AnomalySet<TimeAnomaly> = all.into_iter().collect();
        assert_eq!(set.bits(), 0b111);
        assert!(set.iter().eq(all));
    }
}
