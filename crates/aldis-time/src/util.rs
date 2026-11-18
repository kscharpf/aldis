//! Crate-internal helpers: `const` widening and range-checked field types.

use aldis_core::{Error, FieldId};

/// Widens `u32` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u32 -> u64")]
pub(crate) const fn wide(n: u32) -> u64 {
    n as u64
}

/// Widens `u16` to `u64` in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u16 -> u64")]
pub(crate) const fn wide16(n: u16) -> u64 {
    n as u64
}

/// Widens `u16` to `u32` in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u16 -> u32")]
pub(crate) const fn widen16(n: u16) -> u32 {
    n as u32
}

/// Widens `u8` to `u64` in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> u64")]
pub(crate) const fn wide8(n: u8) -> u64 {
    n as u64
}

/// Widens `u8` to `usize` in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> usize")]
pub(crate) const fn octets(n: u8) -> usize {
    n as usize
}

/// The low octet of `n`: all of `n` when the caller has bounded it below 256 (`const`, no cast).
const fn low8(n: u64) -> u8 {
    let [low, ..] = n.to_le_bytes();
    low
}

/// `n % 10`, the last decimal digit of `n`: below 10, so it is the whole of its octet (301.0
/// §3.4.1 BCD, §3.5.1 ASCII digits; TIME-031, TIME-034).
pub(crate) const fn rem10(n: u64) -> u8 {
    low8(n % 10)
}

/// `n % 100`, the last two decimal digits of `n`: below 100, so it is the whole of its octet
/// (301.0 §3.4.1 BCD; TIME-034).
pub(crate) const fn rem100(n: u64) -> u8 {
    low8(n % 100)
}

/// The low 32 bits of `n`: all of `n` when the caller has bounded it below 2³² (`const`, no cast).
pub(crate) const fn low32(n: u128) -> u32 {
    let [b0, b1, b2, b3, ..] = n.to_le_bytes();
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// A non-zero constant `$n` of the `NonZero` type `$ty`, so that dividing by it has no zero case.
/// `$n` is at least 1: the subtraction is checked when the constant is built (RS-020).
macro_rules! nonzero {
    ($ty:ty, $n:literal) => {
        <$ty>::MIN.saturating_add($n - 1)
    };
}

pub(crate) use nonzero;

/// A length as `u64` for an error detail (`usize` is at most 64 bits on every supported target).
#[expect(
    clippy::as_conversions,
    reason = "lossless widening usize -> u64: supported targets are 32- or 64-bit"
)]
pub(crate) const fn len64(n: usize) -> u64 {
    n as u64
}

/// `OutOfRange` for `field` naming the violated bound: `min` if `value` is below it, else `max`
/// (GEN-041).
pub(crate) const fn out_of_bounds(field: FieldId, value: u64, min: u64, max: u64) -> Error {
    Error::out_of_range(field, if value < min { min } else { max }, value)
}

/// Defines a field type holding `$min..=$max` (API-001 style: private field, checked
/// constructors).
macro_rules! bounded {
    ($(#[$doc:meta])* $name:ident, $repr:ty, $field:expr, $min:literal, $max:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        impl $name {
            /// Smallest valid value (301.0 Annex A; TIME-030, RS-020).
            pub const MIN: $repr = $min;
            /// Largest valid value (301.0 Annex A; TIME-030, RS-020).
            pub const MAX: $repr = $max;

            /// The field, or `None` outside its range (301.0 Annex A; TIME-030, RS-020).
            #[must_use]
            pub const fn new(value: $repr) -> Option<$name> {
                if matches!(value, $min..=$max) {
                    Some($name(value))
                } else {
                    None
                }
            }

            /// The numeric value (301.0 Annex A, TIME-030).
            #[must_use]
            pub const fn get(self) -> $repr {
                self.0
            }
        }

        impl TryFrom<$repr> for $name {
            type Error = aldis_core::Error;

            /// # Errors
            /// `OutOfRange` naming the violated bound (GEN-041).
            fn try_from(value: $repr) -> aldis_core::Result<$name> {
                $name::new(value).ok_or_else(|| {
                    $crate::util::out_of_bounds($field, value.into(), $name::MIN.into(), $name::MAX.into())
                })
            }
        }

        impl Default for $name {
            /// The smallest valid value.
            fn default() -> $name {
                $name($min)
            }
        }

        impl From<$name> for $repr {
            fn from(field: $name) -> $repr {
                field.0
            }
        }
    };
}

pub(crate) use bounded;

/// Gives a [`bounded!`] field type whose smallest value is 0 and largest `$max` the constructor
/// [`reduced`](Self::reduced), for a value that the caller's arithmetic already bounds.
macro_rules! reduced {
    ($name:ident, $repr:ty, $max:literal) => {
        const _: () = assert!($name::MIN == 0 && $name::MAX == $max);

        impl $name {
            /// The field for `value` taken modulo the number of values in its range (301.0
            /// Annex A; TIME-030, RS-020): `value` itself for one in range, so a caller whose
            /// arithmetic bounds the value builds the field with no `Option` and no fallback.
            #[expect(
                clippy::as_conversions,
                reason = "the remainder is at most the field's largest value, which its \
                          representation holds"
            )]
            pub(crate) const fn reduced(value: u64) -> $name {
                $name((value % ($max + 1)) as $repr)
            }
        }
    };
}

pub(crate) use reduced;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{len64, low32, octets, rem10, rem100, wide, wide8, wide16, widen16};
    use crate::calendar::{Hour, Minute, Second};
    use crate::cds::{Micros, MsOfDay, Picos};
    use core::num::{NonZeroU64, NonZeroU128};

    #[test]
    fn widening() {
        // req: GEN-020
        assert_eq!((wide(u32::MAX), wide8(255)), (u64::from(u32::MAX), 255));
        assert_eq!((wide16(u16::MAX), widen16(u16::MAX)), (65_535, 65_535));
        assert_eq!(octets(255), 255);
    }

    #[test]
    fn decimal_digits_and_truncations() {
        // req: TIME-031, TIME-034, RS-020
        assert_eq!(
            (rem10(0), rem10(9), rem10(10), rem10(u64::MAX)),
            (0, 9, 0, 5)
        );
        assert_eq!(
            (rem100(0), rem100(99), rem100(100), rem100(u64::MAX)),
            (0, 99, 0, 15)
        );
        assert_eq!(low32(999_999_999), 999_999_999);
        assert_eq!(low32(u128::from(u32::MAX)), u32::MAX);
        assert_eq!(low32(1 << 32), 0);
        assert_eq!(len64(usize::MAX), u64::MAX);
        assert_eq!(len64(7), 7);
    }

    #[test]
    fn nonzero_constants() {
        // req: RS-020
        assert_eq!(nonzero!(NonZeroU64, 1), NonZeroU64::MIN);
        assert_eq!(
            nonzero!(NonZeroU64, 1_000_000_000_000).get(),
            1_000_000_000_000
        );
        assert_eq!(nonzero!(NonZeroU128, 10).get(), 10);
    }

    /// `reduced` is the identity on a field's range and wraps just above it.
    macro_rules! reduced_cases {
        ($($name:ident),*) => {$(
            let max = u64::from($name::MAX);
            assert_eq!($name::reduced(0).get(), 0);
            assert_eq!($name::reduced(max), $name::new($name::MAX).unwrap());
            assert_eq!($name::reduced(max / 2).get(), $name::new($name::MAX / 2).unwrap().get());
            assert_eq!($name::reduced(max + 1).get(), 0);
            assert_eq!($name::reduced(max + 2).get(), 1);
        )*};
    }

    #[test]
    fn reduced_fields_are_valid() {
        // req: TIME-030, TIME-021, RS-020
        reduced_cases!(Hour, Minute, Second, MsOfDay, Micros, Picos);
        assert_eq!(Hour::reduced(u64::MAX).get(), 15);
    }
}
