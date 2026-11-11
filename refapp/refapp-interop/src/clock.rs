//! The clock behind a trait (APP-160, plan D5 c).
//!
//! The nodes read no clock (APP-091); only the driver does, through [`Clock`]. The binary uses
//! [`Monotonic`], the tests [`Manual`], so that they run in virtual time.

use std::time::Instant;

use refapp_harness::micros::Micros;

/// A source of the run's time: microseconds since the start of the run.
pub trait Clock {
    /// The instant now.
    fn now(&self) -> Micros;

    /// Whether time passes by itself, so that the driver waits on its port until an instant.
    /// False for a virtual clock, which the driver advances.
    fn passes_by_itself(&self) -> bool;

    /// Moves a virtual clock forward to `t`, never backward; a real clock ignores it.
    fn advance_to(&mut self, t: Micros);
}

/// The monotonic clock of the binary, started at the beginning of the run.
#[derive(Debug, Clone, Copy)]
pub struct Monotonic {
    start: Instant,
}

impl Monotonic {
    /// A clock whose instant 0 is now.
    #[must_use]
    pub fn start() -> Self {
        Monotonic {
            start: Instant::now(),
        }
    }
}

impl Clock for Monotonic {
    fn now(&self) -> Micros {
        // A run of 584 000 years overflows; the clamp is on a value the clock builds itself.
        Micros::new(u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX))
    }

    fn passes_by_itself(&self) -> bool {
        true
    }

    fn advance_to(&mut self, _t: Micros) {}
}

/// A virtual clock that only the driver moves.
#[derive(Debug, Clone, Copy, Default)]
pub struct Manual {
    now: Micros,
}

impl Manual {
    /// A clock at instant 0.
    #[must_use]
    pub fn new() -> Self {
        Manual::default()
    }
}

impl Clock for Manual {
    fn now(&self) -> Micros {
        self.now
    }

    fn passes_by_itself(&self) -> bool {
        false
    }

    fn advance_to(&mut self, t: Micros) {
        self.now = self.now.max(t);
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, Manual, Monotonic};
    use refapp_harness::micros::Micros;

    #[test]
    fn manual_clock_moves_only_forward_when_told() {
        // req: APP-160
        let mut c = Manual::new();
        assert_eq!(c.now(), Micros::new(0));
        c.advance_to(Micros::new(125_000));
        c.advance_to(Micros::new(10));
        assert_eq!(c.now(), Micros::new(125_000));
        assert!(!c.passes_by_itself());
    }

    #[test]
    fn monotonic_clock_does_not_go_back() {
        // req: APP-160
        let c = Monotonic::start();
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
        assert!(c.passes_by_itself());
    }
}
