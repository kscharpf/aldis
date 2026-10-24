//! Lenient-mode anomaly flags (732.1 §4.1.2.9.2, §4.1.4.1.8–9; API-032, API-033, GEN-043,
//! GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/uslp-frame.md` §4 (API-033).
//! M2 plan D9 withdrew `VcfLengthMismatchForQos` (an error in both modes, USLP-004) and
//! `UnknownUpid` (reported by [`Upid::Other`](crate::upid::Upid::Other), USLP-007).

aldis_core::anomaly_flags! {
    /// A deviation a lenient USLP frame decoder accepts and reports (API-032).
    pub enum UslpAnomaly {
        /// Bit 0. Reserve Spares of the Primary Header not `00` (732.1 §4.1.2.9.2).
        ReserveSparesSet = 0,
        /// Bit 1. A frame on VCID 63 whose MAP ID, TFDZ Construction Rule, UPID, or Last Valid
        /// Octet Pointer is not the value of an OID frame (732.1 §4.1.4.1.8–9): fixed-value
        /// fields of an idle data unit (API-032; proposed, OI-43 item 1). The view keeps the
        /// values.
        OidHeaderMismatch = 1,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::UslpAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn documented_bit_indices() {
        // req: GEN-043, API-033
        assert_eq!(
            UslpAnomaly::ALL,
            [
                UslpAnomaly::ReserveSparesSet,
                UslpAnomaly::OidHeaderMismatch
            ]
        );
        assert!(UslpAnomaly::ALL.iter().map(|f| f.bit()).eq([0, 1]));
        let set = AnomalySet::from_flags(UslpAnomaly::ALL);
        assert_eq!(set.bits(), 0b11);
        assert!(set.iter().eq(UslpAnomaly::ALL.iter().copied()));
        assert_eq!(UslpAnomaly::from_bit(2), None);
    }
}
