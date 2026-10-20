//! Lenient-mode anomaly flags (301.0 §3.1.1, §3.4.1.2; API-032, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/time-codes.md` §7 (API-033).
//! Bit 1 is reserved and never raised (OI-15).

aldis_core::anomaly_flags! {
    /// A deviation a lenient time-code decoder accepts and reports (301.0 §3.1.1, §3.4.1.2;
    /// API-032).
    pub enum TimeAnomaly {
        /// Bit 0. P-field time code ID `000`, `011`, or `111`, reserved by 301.0 §3.1.1. The
        /// decoder returns the P-field as `Unknown` and consumes only the P-field (TIME-002).
        ReservedTimeCodeId = 0,
        /// Bit 2. Non-zero high nibble of the CCS day-of-year segment, "not used and set to
        /// zero" by 301.0 §3.4.1.2 (TIME-034).
        NonZeroDoyHighNibble = 2,
        /// Bit 3. P-field extension octet that 301.0 does not define for the code (§3.1.1); kept
        /// raw for round trip (TIME-005).
        UndefinedPFieldExtension = 3,
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
                TimeAnomaly::NonZeroDoyHighNibble,
                TimeAnomaly::UndefinedPFieldExtension,
            ]
        );
        assert!(TimeAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 2, 3]));
        assert_eq!(TimeAnomaly::from_bit(1), None, "bit 1 is reserved (OI-15)");
        let set = AnomalySet::from_flags(TimeAnomaly::ALL);
        assert_eq!(set.bits(), 0b1101);
        assert!(set.iter().eq(TimeAnomaly::ALL.iter().copied()));
    }
}
