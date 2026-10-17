//! Lenient-mode anomaly flags (API-032, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/time-codes.md` §7 (API-033).
//! Bits 0 and 1 are provisional until OI-15 is resolved.

aldis_core::anomaly_flags! {
    /// A deviation a lenient time-code decoder accepts and reports (API-032).
    pub enum TimeAnomaly {
        /// Bit 0. P-field time code ID `000`, `011`, or `111`, reserved by 301.0 §3.1.1
        /// (TIME-002).
        ReservedTimeCodeId = 0,
        /// Bit 1. CDS submillisecond length code `11`, reserved by 301.0 §3.3.2 (TIME-020).
        ReservedSubmsCode = 1,
        /// Bit 2. Non-zero high nibble of the CCS day-of-year segment, "not used and set to
        /// zero" by 301.0 §3.4.1.2.
        NonZeroDoyHighNibble = 2,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::TimeAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043
        assert_eq!(
            TimeAnomaly::ALL,
            [
                TimeAnomaly::ReservedTimeCodeId,
                TimeAnomaly::ReservedSubmsCode,
                TimeAnomaly::NonZeroDoyHighNibble,
            ]
        );
        assert!(TimeAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 1, 2]));
        let set = AnomalySet::from_flags(TimeAnomaly::ALL);
        assert_eq!(set.bits(), 0b111);
        assert!(set.iter().eq(TimeAnomaly::ALL.iter().copied()));
    }
}
