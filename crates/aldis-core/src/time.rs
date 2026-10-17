//! Caller-supplied monotonic time (API-003).
//!
//! Engines never read clocks (GEN-031): the caller passes an [`Instant`], an unsigned count of
//! microseconds since an arbitrary epoch. Arithmetic saturates instead of wrapping, and time
//! going backwards is treated as zero elapsed time.

/// A point in caller-supplied monotonic time, in microseconds since an arbitrary epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Instant(u64);

/// A span of time in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(u64);

impl Instant {
    /// The instant `micros` microseconds after the epoch.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Instant {
        Instant(micros)
    }

    /// Microseconds since the epoch.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// This instant plus `d`, saturating at the largest representable instant.
    #[must_use]
    pub const fn saturating_add(self, d: Duration) -> Instant {
        Instant(self.0.saturating_add(d.0))
    }

    /// Time elapsed since `earlier`; zero if `earlier` is later than `self` (API-003).
    #[must_use]
    pub const fn duration_since(self, earlier: Instant) -> Duration {
        Duration(self.0.saturating_sub(earlier.0))
    }
}

impl Duration {
    /// Zero duration.
    pub const ZERO: Duration = Duration(0);
    /// Largest duration.
    pub const MAX: Duration = Duration(u64::MAX);

    /// A duration of `micros` microseconds.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Duration {
        Duration(micros)
    }

    /// A duration of `millis` milliseconds, saturating.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Duration {
        Duration(millis.saturating_mul(1_000))
    }

    /// A duration of `secs` seconds, saturating.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Duration {
        Duration(secs.saturating_mul(1_000_000))
    }

    /// The duration in microseconds.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Sum, saturating.
    #[must_use]
    pub const fn saturating_add(self, other: Duration) -> Duration {
        Duration(self.0.saturating_add(other.0))
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, Instant};

    #[test]
    fn saturating_and_monotonic() {
        // req: API-003
        let t = Instant::from_micros(1_000);
        assert_eq!(t.as_micros(), 1_000);
        assert_eq!(
            t.saturating_add(Duration::from_millis(2)),
            Instant::from_micros(3_000)
        );
        assert_eq!(
            Instant::from_micros(u64::MAX)
                .saturating_add(Duration::from_secs(1))
                .as_micros(),
            u64::MAX
        );
        assert_eq!(
            Instant::from_micros(5).duration_since(Instant::from_micros(2)),
            Duration::from_micros(3)
        );
        // Time going backwards is zero elapsed time, never a wrap or panic.
        assert_eq!(
            Instant::from_micros(2).duration_since(Instant::from_micros(5)),
            Duration::ZERO
        );
        assert_eq!(Duration::from_secs(u64::MAX), Duration::MAX);
        assert_eq!(Duration::from_millis(u64::MAX), Duration::MAX);
        assert_eq!(Duration::from_secs(2).as_micros(), 2_000_000);
        assert_eq!(
            Duration::MAX.saturating_add(Duration::from_micros(1)),
            Duration::MAX
        );
        assert_eq!(Instant::default(), Instant::from_micros(0));
    }
}
