//! An integer of the scenario file with a closed range (RS-020, APP-107).
//!
//! The range is a pair of const parameters, so the one declaration of a field's range serves
//! the parser ([`Ranged::new`], [`Deserialize`]) and the published JSON Schema ([`JsonSchema`]):
//! a value outside the range is not a `Ranged`, and the schema cannot state another range than
//! the parser enforces.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

/// An integer of type `T` in `MIN..=MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Ranged<T, const MIN: u64, const MAX: u64>(T);

impl<T, const MIN: u64, const MAX: u64> Ranged<T, MIN, MAX>
where
    T: Copy + Into<u64>,
{
    /// The value `value`.
    ///
    /// # Errors
    /// If `value` is below `MIN` or above `MAX`.
    pub fn new(value: T) -> Result<Self, String> {
        let wide: u64 = value.into();
        if (MIN..=MAX).contains(&wide) {
            Ok(Ranged(value))
        } else {
            Err(format!("{wide} is outside the range {MIN} to {MAX}"))
        }
    }

    /// The value, in `MIN..=MAX`.
    #[must_use]
    pub const fn get(self) -> T {
        self.0
    }
}

impl<'de, T, const MIN: u64, const MAX: u64> Deserialize<'de> for Ranged<T, MIN, MAX>
where
    T: Copy + Into<u64> + Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(T::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<T, const MIN: u64, const MAX: u64> JsonSchema for Ranged<T, MIN, MAX>
where
    T: JsonSchema,
{
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("Ranged<{}, {MIN}, {MAX}>", T::schema_name()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = T::json_schema(generator);
        schema.insert("minimum".to_owned(), MIN.into());
        schema.insert("maximum".to_owned(), MAX.into());
        schema
    }
}

#[cfg(test)]
mod tests {
    use super::Ranged;

    type OneToFour = Ranged<u8, 1, 4>;

    #[test]
    fn the_range_is_closed() {
        // req: APP-107
        assert!(OneToFour::new(0).is_err());
        assert_eq!(OneToFour::new(1).unwrap().get(), 1);
        assert_eq!(OneToFour::new(4).unwrap().get(), 4);
        assert!(OneToFour::new(5).is_err());
    }

    #[test]
    fn deserialization_checks_the_range() {
        // req: APP-107
        assert_eq!(serde_json::from_str::<OneToFour>("3").unwrap().get(), 3);
        assert!(serde_json::from_str::<OneToFour>("0").is_err());
        assert!(serde_json::from_str::<OneToFour>("5").is_err());
        assert!(serde_json::from_str::<OneToFour>("256").is_err());
        assert!(serde_json::from_str::<OneToFour>("-1").is_err());
        assert_eq!(
            serde_json::to_string(&OneToFour::new(2).unwrap()).unwrap(),
            "2"
        );
    }
}
