//! A span or instant of simulated time in microseconds (APP-060, APP-107).
//!
//! The scenario file holds every time in µs as a plain JSON integer; this type keeps a
//! microsecond value apart from the counts and octet lengths next to it. It serializes as the
//! integer, so neither the scenario file, its schema, nor a trace changes.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::ranged::Bounded;

/// Microseconds of simulated time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Micros(u64);

impl Micros {
    /// The value `us` µs.
    #[must_use]
    pub const fn new(us: u64) -> Self {
        Micros(us)
    }

    /// The value in µs.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The sum, stopping at the largest value.
    #[must_use]
    pub const fn saturating_add(self, other: Micros) -> Micros {
        Micros(self.0.saturating_add(other.0))
    }

    /// Whether the value is a multiple of `period`; false for a zero period.
    #[must_use]
    pub const fn is_multiple_of(self, period: Micros) -> bool {
        self.0.is_multiple_of(period.0)
    }

    /// The first multiple of `period` at or after the value, stopping at the largest value.
    /// A zero `period` gives the value itself.
    #[must_use]
    pub const fn next_multiple_of(self, period: Micros) -> Micros {
        if period.0 == 0 {
            return self;
        }
        Micros(self.0.div_ceil(period.0).saturating_mul(period.0))
    }

    /// `periods` periods of `period` µs, stopping at the largest value.
    #[must_use]
    pub const fn times(period: Micros, periods: u64) -> Micros {
        Micros(period.0.saturating_mul(periods))
    }
}

impl From<Micros> for u64 {
    fn from(us: Micros) -> u64 {
        us.0
    }
}

impl Bounded for Micros {
    const LIMIT: u64 = u64::MAX;
}

impl JsonSchema for Micros {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        u64::schema_name()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        u64::json_schema(generator)
    }
}

impl std::fmt::Display for Micros {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::Micros;

    #[test]
    fn arithmetic_and_serialized_form() {
        // req: APP-107
        let slot = Micros::new(250_000);
        assert_eq!(Micros::new(1).next_multiple_of(slot), slot);
        assert_eq!(slot.next_multiple_of(slot), slot);
        assert_eq!(Micros::new(5).next_multiple_of(Micros::default()).get(), 5);
        assert!(slot.is_multiple_of(Micros::new(125_000)));
        assert_eq!(Micros::times(slot, 3).get(), 750_000);
        assert_eq!(slot.saturating_add(Micros::new(u64::MAX)).get(), u64::MAX);
        // A plain integer in JSON, as before.
        assert_eq!(serde_json::to_string(&slot).unwrap(), "250000");
        assert_eq!(serde_json::from_str::<Micros>("7").unwrap().get(), 7);
        assert_eq!(slot.to_string(), "250000");
    }
}
