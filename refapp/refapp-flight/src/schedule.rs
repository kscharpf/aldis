//! Generation instants of the periodic applications (docs/reference-application.md §2.1).

use refapp_scenario::{Duration, Instant};

/// The end of generation: applications generate packets while t < 30 s (§2.1, §2.4).
pub const GENERATION_END: Instant = Instant::from_micros(30_000_000);

/// A periodic schedule: instants `first + k * period` for k = 0, 1, … while below
/// [`GENERATION_END`] (§2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Periodic {
    first: Instant,
    period: Duration,
}

impl Periodic {
    /// Housekeeping: t = k s (§2.1, APP-010).
    pub const HOUSEKEEPING: Periodic = Periodic {
        first: Instant::from_micros(0),
        period: Duration::from_secs(1),
    };

    /// Payload: t = 0.5 + 2k s (§2.1, APP-015).
    pub const PAYLOAD: Periodic = Periodic {
        first: Instant::from_micros(500_000),
        period: Duration::from_secs(2),
    };

    /// The instant of slot `index`, or `None` from the end of generation on.
    #[must_use]
    pub fn at(&self, index: u32) -> Option<Instant> {
        u64::from(index)
            .checked_mul(self.period.as_micros())
            .and_then(|offset| offset.checked_add(self.first.as_micros()))
            .map(Instant::from_micros)
            .filter(|t| *t < GENERATION_END)
    }
}

#[cfg(test)]
mod tests {
    use refapp_scenario::Instant;

    use super::Periodic;

    #[test]
    fn housekeeping_has_thirty_slots() {
        // req: APP-010
        assert_eq!(Periodic::HOUSEKEEPING.at(0), Some(Instant::from_micros(0)));
        assert_eq!(
            Periodic::HOUSEKEEPING.at(29),
            Some(Instant::from_micros(29_000_000))
        );
        assert_eq!(Periodic::HOUSEKEEPING.at(30), None);
    }

    #[test]
    fn payload_slots_are_half_seconds_every_two() {
        // req: APP-015
        assert_eq!(Periodic::PAYLOAD.at(0), Some(Instant::from_micros(500_000)));
        assert_eq!(
            Periodic::PAYLOAD.at(14),
            Some(Instant::from_micros(28_500_000))
        );
        assert_eq!(Periodic::PAYLOAD.at(15), None);
        assert_eq!(Periodic::PAYLOAD.at(u32::MAX), None);
    }
}
