//! Lenient-mode anomaly flags (132.0-B-3 §4.1.2.7.4–5; TM-007, API-032, API-033, GEN-043).
//!
//! The flags and their bit indices are listed in `docs/standards/tm-frame.md` §4 (API-033). An
//! FHP beyond the data field is `Malformed` in both modes and has no flag (TM-008, OI-33).

aldis_core::anomaly_flags! {
    /// A deviation a lenient TM Transfer Frame decoder accepts and reports (132.0 §4.1.2.7.4–5;
    /// TM-007, API-032).
    pub enum TmAnomaly {
        /// Bit 0. Packet Order Flag `1` with the Synchronization Flag `0`, where it is reserved
        /// and set to `0` (132.0 §4.1.2.7.4 note; TM-007).
        PacketOrderFlagSet = 0,
        /// Bit 1. Segment Length Identifier other than `11` with the Synchronization Flag `0`, a
        /// fixed-value legacy field (132.0 §4.1.2.7.5.2, notes 1–2; TM-007, API-032).
        SegmentLengthIdNot11 = 1,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::TmAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043, API-033
        assert_eq!(
            TmAnomaly::ALL,
            [
                TmAnomaly::PacketOrderFlagSet,
                TmAnomaly::SegmentLengthIdNot11
            ]
        );
        assert!(TmAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 1]));
        assert_eq!(TmAnomaly::from_bit(2), None);
        let set = AnomalySet::from_flags(TmAnomaly::ALL);
        assert_eq!(set.bits(), 0b11);
    }
}
