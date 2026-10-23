//! Lenient-mode anomaly flags (232.0 §4.1.2.4, §4.1.3.3.4, §4.2.1.7, §4.2.1.10; API-032,
//! API-033, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/tc-frame.md` §6 (API-033).

aldis_core::anomaly_flags! {
    /// A deviation a lenient TC frame or CLCW decoder accepts and reports (API-032).
    pub enum TcAnomaly {
        /// Bit 0. Reserved Spare of the Primary Header not `00` (232.0 §4.1.2.4.2; TC-003).
        ReservedSpareSet = 0,
        /// Bit 1. A Type-C data field that is neither Unlock nor Set V(R), a pattern CCSDS
        /// reserves (232.0 §4.1.3.3.4; TC-005). The view keeps the octets in its data field.
        UnknownControlCommand = 1,
        /// Bit 2. A Reserved Spare of the CLCW, bits 14–15 or bit 23, not zero (232.0
        /// §4.2.1.7.2, §4.2.1.10.2; TC-030).
        ClcwSpareSet = 2,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::TcAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043, API-033
        assert_eq!(
            TcAnomaly::ALL,
            [
                TcAnomaly::ReservedSpareSet,
                TcAnomaly::UnknownControlCommand,
                TcAnomaly::ClcwSpareSet
            ]
        );
        assert!(TcAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 1, 2]));
        let set = AnomalySet::from_flags(TcAnomaly::ALL);
        assert_eq!(set.bits(), 0b111);
        assert!(set.iter().eq(TcAnomaly::ALL.iter().copied()));
        assert_eq!(TcAnomaly::from_bit(3), None);
    }
}
