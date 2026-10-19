//! Crate-internal helpers: `const` widening and range-checked field types.

use aldis_core::Error;

/// Widens `u32` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u32 -> u64")]
pub(crate) const fn wide(n: u32) -> u64 {
    n as u64
}

/// Widens `u8` to `u64` in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> u64")]
pub(crate) const fn wide8(n: u8) -> u64 {
    n as u64
}

/// `OutOfRange` for `field` naming the violated bound: `min` if `value` is below it, else `max`
/// (GEN-041).
pub(crate) const fn out_of_bounds(field: &'static str, value: u64, min: u64, max: u64) -> Error {
    Error::out_of_range(field, if value < min { min } else { max }, value)
}

/// Defines a field type holding `$min..=$max` (API-001 style: private field, checked
/// constructors).
macro_rules! bounded {
    ($(#[$doc:meta])* $name:ident, $repr:ty, $field:literal, $min:literal, $max:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        impl $name {
            /// Smallest valid value.
            pub const MIN: $repr = $min;
            /// Largest valid value.
            pub const MAX: $repr = $max;

            /// The field, or `None` outside its range.
            #[must_use]
            pub const fn new(value: $repr) -> Option<$name> {
                if matches!(value, $min..=$max) {
                    Some($name(value))
                } else {
                    None
                }
            }

            /// The numeric value.
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

        impl From<$name> for $repr {
            fn from(field: $name) -> $repr {
                field.0
            }
        }
    };
}

pub(crate) use bounded;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{wide, wide8};

    #[test]
    fn widening() {
        // req: GEN-020
        assert_eq!((wide(u32::MAX), wide8(255)), (u64::from(u32::MAX), 255));
    }
}
