//! Lenient-mode anomaly flags (API-032, API-033, GEN-043, GEN-081).
//!
//! The flags and their bit indices are listed in `docs/standards/space-packet.md` §3.

use aldis_core::Anomaly;

/// A deviation a lenient Space Packet decoder accepts and reports (API-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SppAnomaly {
    /// Bit 0. Idle Packet (APID 2047) with Secondary Header Flag = 1, which 133.0
    /// §4.1.3.3.3.4 sets to 0 (SPP-004).
    IdleWithSecondaryHeader,
}

impl Anomaly for SppAnomaly {
    fn bit(self) -> u8 {
        match self {
            SppAnomaly::IdleWithSecondaryHeader => 0,
        }
    }

    fn from_bit(bit: u8) -> Option<SppAnomaly> {
        match bit {
            0 => Some(SppAnomaly::IdleWithSecondaryHeader),
            _ => None,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::SppAnomaly;
    use aldis_core::{Anomaly, AnomalySet};

    #[test]
    fn stable_bit_indices() {
        // req: API-033, GEN-043
        assert_eq!(SppAnomaly::IdleWithSecondaryHeader.bit(), 0);
        assert_eq!(
            SppAnomaly::from_bit(0),
            Some(SppAnomaly::IdleWithSecondaryHeader)
        );
        assert_eq!(SppAnomaly::from_bit(1), None);
        let set: AnomalySet<SppAnomaly> =
            [SppAnomaly::IdleWithSecondaryHeader].into_iter().collect();
        assert_eq!(set.bits(), 0b1);
    }
}
