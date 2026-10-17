//! Lenient-mode anomaly flags (API-032, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/space-packet.md` §3 (API-033).
//! Bit 0 is provisional until OI-16 is resolved.

aldis_core::anomaly_flags! {
    /// A deviation a lenient Space Packet decoder accepts and reports (API-032).
    pub enum SppAnomaly {
        /// Bit 0. Idle Packet (APID 2047) with Secondary Header Flag = 1, which 133.0
        /// §4.1.3.3.3.4 sets to 0 (SPP-004).
        IdleWithSecondaryHeader = 0,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::SppAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043
        assert_eq!(SppAnomaly::ALL, [SppAnomaly::IdleWithSecondaryHeader]);
        assert!(SppAnomaly::ALL.iter().map(|f| f.bit()).eq([0]));
        let set = AnomalySet::from_flags(SppAnomaly::ALL);
        assert_eq!(set.bits(), 0b1);
        assert!(set.iter().eq(SppAnomaly::ALL.iter().copied()));
    }
}
