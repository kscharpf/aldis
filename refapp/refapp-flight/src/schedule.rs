//! Generation instants of the periodic applications (docs/reference-application.md §2.1).

/// The end of generation: applications generate packets while t < 30 s (§2.1, §2.4).
pub const GENERATION_END_US: u64 = 30_000_000;

/// A periodic schedule: instants `first_us + k * period_us` for k = 0, 1, … while below
/// [`GENERATION_END_US`] (§2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Periodic {
    first_us: u64,
    period_us: u64,
}

impl Periodic {
    /// Housekeeping: t = k s (§2.1, APP-010).
    pub const HOUSEKEEPING: Periodic = Periodic {
        first_us: 0,
        period_us: 1_000_000,
    };

    /// Payload: t = 0.5 + 2k s (§2.1, APP-015).
    pub const PAYLOAD: Periodic = Periodic {
        first_us: 500_000,
        period_us: 2_000_000,
    };

    /// The instant of slot `index`, or `None` from the end of generation on.
    #[must_use]
    pub fn at(&self, index: u32) -> Option<u64> {
        u64::from(index)
            .checked_mul(self.period_us)
            .and_then(|offset| offset.checked_add(self.first_us))
            .filter(|t| *t < GENERATION_END_US)
    }
}

#[cfg(test)]
mod tests {
    use super::Periodic;

    #[test]
    fn housekeeping_has_thirty_slots() {
        // req: APP-010
        assert_eq!(Periodic::HOUSEKEEPING.at(0), Some(0));
        assert_eq!(Periodic::HOUSEKEEPING.at(29), Some(29_000_000));
        assert_eq!(Periodic::HOUSEKEEPING.at(30), None);
    }

    #[test]
    fn payload_slots_are_half_seconds_every_two() {
        // req: APP-015
        assert_eq!(Periodic::PAYLOAD.at(0), Some(500_000));
        assert_eq!(Periodic::PAYLOAD.at(14), Some(28_500_000));
        assert_eq!(Periodic::PAYLOAD.at(15), None);
        assert_eq!(Periodic::PAYLOAD.at(u32::MAX), None);
    }
}
