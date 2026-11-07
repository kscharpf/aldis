//! An integer of the scenario file with a closed range (RS-020, APP-107).
//!
//! The range is a pair of const parameters, so the one declaration of a field's range serves
//! the parser ([`Ranged::new`], [`Deserialize`]) and the published JSON Schema ([`JsonSchema`]):
//! a value outside the range is not a `Ranged`, and the schema cannot state another range than
//! the parser enforces. A range with `MIN > MAX`, or a `MAX` beyond the largest value of `T`,
//! does not compile.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

/// An integer type that can carry a [`Ranged`] value, with its largest value.
pub trait Bounded: Copy + Into<u64> {
    /// The largest value of the type.
    const LIMIT: u64;
}

impl Bounded for u8 {
    const LIMIT: u64 = u8::MAX as u64;
}
impl Bounded for u16 {
    const LIMIT: u64 = u16::MAX as u64;
}
impl Bounded for u32 {
    const LIMIT: u64 = u32::MAX as u64;
}
impl Bounded for u64 {
    const LIMIT: u64 = u64::MAX;
}

/// An integer of type `T` in `MIN..=MAX`.
///
/// A range with `MIN > MAX` does not compile:
///
/// ```compile_fail
/// use refapp_harness::ranged::Ranged;
/// let _ = Ranged::<u8, 5, 4>::new(4);
/// ```
///
/// Nor does a `MAX` beyond the largest value of `T`:
///
/// ```compile_fail
/// use refapp_harness::ranged::Ranged;
/// let _ = Ranged::<u8, 0, 256>::new(4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Ranged<T, const MIN: u64, const MAX: u64>(T);

impl<T: Bounded, const MIN: u64, const MAX: u64> Ranged<T, MIN, MAX> {
    /// Evaluated where the range is used: fails to compile for an empty range or one beyond `T`.
    const VALID: () = assert!(MIN <= MAX && MAX <= T::LIMIT, "range outside the type");

    /// The value `value`.
    ///
    /// # Errors
    /// If `value` is below `MIN` or above `MAX`.
    pub fn new(value: T) -> Result<Self, String> {
        let () = Self::VALID;
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
    T: Bounded + Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(T::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<T, const MIN: u64, const MAX: u64> JsonSchema for Ranged<T, MIN, MAX>
where
    T: Bounded + JsonSchema,
{
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("Ranged<{}, {MIN}, {MAX}>", T::schema_name()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let () = Self::VALID;
        let mut schema = T::json_schema(generator);
        schema.insert("minimum".to_owned(), MIN.into());
        schema.insert("maximum".to_owned(), MAX.into());
        schema
    }
}

/// An even integer of type `T` in `MIN..=MAX`, such as the COP-1 window W (232.1-B-2 Cor. 1
/// §6.1.8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct EvenRanged<T, const MIN: u64, const MAX: u64>(Ranged<T, MIN, MAX>);

impl<T: Bounded, const MIN: u64, const MAX: u64> EvenRanged<T, MIN, MAX> {
    /// The value `value`.
    ///
    /// # Errors
    /// If `value` is odd or outside `MIN..=MAX`.
    pub fn new(value: T) -> Result<Self, String> {
        let inner = Ranged::new(value)?;
        let wide: u64 = value.into();
        if wide.is_multiple_of(2) {
            Ok(Self(inner))
        } else {
            Err(format!("{wide} is odd, the value must be even"))
        }
    }

    /// The value, even and in `MIN..=MAX`.
    #[must_use]
    pub const fn get(self) -> T {
        self.0.get()
    }
}

impl<'de, T, const MIN: u64, const MAX: u64> Deserialize<'de> for EvenRanged<T, MIN, MAX>
where
    T: Bounded + Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(T::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<T, const MIN: u64, const MAX: u64> JsonSchema for EvenRanged<T, MIN, MAX>
where
    T: Bounded + JsonSchema,
{
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("EvenRanged<{}, {MIN}, {MAX}>", T::schema_name()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = Ranged::<T, MIN, MAX>::json_schema(generator);
        schema.insert("multipleOf".to_owned(), 2.into());
        schema
    }
}

#[cfg(test)]
mod tests {
    use super::{EvenRanged, Ranged};

    #[test]
    fn an_even_value_must_be_even_and_in_range() {
        // req: APP-107
        type W = EvenRanged<u8, 2, 254>;
        assert_eq!(W::new(2).unwrap().get(), 2);
        assert_eq!(W::new(254).unwrap().get(), 254);
        assert!(W::new(3).is_err());
        assert!(W::new(0).is_err());
        assert!(serde_json::from_str::<W>("7").is_err());
        assert_eq!(serde_json::from_str::<W>("8").unwrap().get(), 8);
    }

    #[test]
    fn the_range_is_checked_at_compile_time() {
        // req: APP-107
        // A range with MIN > MAX or MAX beyond u8 would fail to build when `new` is
        // instantiated; the valid bounds of u8 build.
        assert_eq!(Ranged::<u8, 0, 255>::new(255).unwrap().get(), 255);
    }

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
