//! Lenient-mode anomaly flags (133.1 §4.1.2.6.3, §4.1.2.7.2; API-032, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/encapsulation-packet.md` §3
//! (API-033). Bit 2 is reserved and never raised (OI-16).

aldis_core::anomaly_flags! {
    /// A deviation a lenient Encapsulation Packet decoder accepts and reports (133.1 §4.1.2.6.3,
    /// §4.1.2.7.2; API-032).
    pub enum EppAnomaly {
        /// Bit 0. Non-zero Protocol ID Extension while the EPI is not `110`, which 133.1
        /// §4.1.2.6.3 forbids (EPP-006).
        NonZeroEpiExtension = 0,
        /// Bit 1. Non-zero CCSDS Defined Field, by convention all zeros (133.1 §4.1.2.7.2,
        /// EPP-007).
        NonZeroCcsdsDefined = 1,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::EppAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043
        assert_eq!(
            EppAnomaly::ALL,
            [
                EppAnomaly::NonZeroEpiExtension,
                EppAnomaly::NonZeroCcsdsDefined
            ]
        );
        assert!(EppAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 1]));
        assert_eq!(EppAnomaly::from_bit(2), None, "bit 2 is reserved (OI-16)");
        let set = AnomalySet::from_flags(EppAnomaly::ALL);
        assert_eq!(set.bits(), 0b11);
        assert!(set.iter().eq(EppAnomaly::ALL.iter().copied()));
    }
}
