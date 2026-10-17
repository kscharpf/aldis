//! Lenient-mode anomaly flags (GEN-043, API-032, GEN-081).
//!
//! A decoder in [`DecodeMode::Lenient`](crate::mode::DecodeMode::Lenient) returns an
//! [`AnomalySet`] alongside a successful result. Each codec crate defines its own
//! `#[non_exhaustive]` flag enum (GEN-081) and implements [`Anomaly`] for it, giving every flag
//! a stable bit index. The indices are listed in each standard document (API-033) and are
//! shared with the C ABI, so a flag is never renumbered. A strict decoder always returns
//! [`AnomalySet::empty`].

use core::fmt;
use core::hash::Hash;
use core::marker::PhantomData;

/// A lenient-mode anomaly flag with a stable bit index (GEN-043).
pub trait Anomaly: Copy + Eq + Hash + fmt::Debug {
    /// The flag's bit index, 0 to [`AnomalySet::CAPACITY`] − 1. Stable: never renumbered.
    fn bit(self) -> u8;

    /// The flag with bit index `bit`, or `None` if no flag uses it.
    fn from_bit(bit: u8) -> Option<Self>;
}

/// A set of up to 32 anomaly flags of type `A`, stored as a `u32` bit set (GEN-043).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnomalySet<A> {
    bits: u32,
    flags: PhantomData<A>,
}

impl<A> AnomalySet<A> {
    /// Number of distinct bit indices a set can hold.
    pub const CAPACITY: u8 = 32;

    /// The empty set, which every strict decode returns.
    #[must_use]
    pub const fn empty() -> AnomalySet<A> {
        AnomalySet {
            bits: 0,
            flags: PhantomData,
        }
    }

    /// Whether no flag is set.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// Number of flags set.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.bits.count_ones()
    }

    /// The raw bit set: bit `n` is set when the flag with index `n` is present.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }
}

/// The single-bit mask for `flag`; 0 if its index is not below [`AnomalySet::CAPACITY`].
fn mask<A: Anomaly>(flag: A) -> u32 {
    1_u32.checked_shl(u32::from(flag.bit())).unwrap_or(0)
}

impl<A: Anomaly> AnomalySet<A> {
    /// Adds `flag`. A flag whose index is not below [`CAPACITY`](Self::CAPACITY) cannot be
    /// represented and is ignored.
    pub fn insert(&mut self, flag: A) {
        self.bits |= mask(flag);
    }

    /// Whether `flag` is set.
    #[must_use]
    pub fn contains(&self, flag: A) -> bool {
        self.bits & mask(flag) != 0
    }

    /// The flags that are set, in ascending bit order.
    #[must_use]
    pub fn iter(&self) -> Iter<A> {
        Iter {
            bits: self.bits,
            flags: PhantomData,
        }
    }
}

impl<A> Default for AnomalySet<A> {
    fn default() -> AnomalySet<A> {
        AnomalySet::empty()
    }
}

impl<A: Anomaly> fmt::Debug for AnomalySet<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<A: Anomaly> FromIterator<A> for AnomalySet<A> {
    fn from_iter<I: IntoIterator<Item = A>>(flags: I) -> AnomalySet<A> {
        let mut set = AnomalySet::empty();
        for flag in flags {
            set.insert(flag);
        }
        set
    }
}

impl<A: Anomaly> IntoIterator for AnomalySet<A> {
    type Item = A;
    type IntoIter = Iter<A>;

    fn into_iter(self) -> Iter<A> {
        self.iter()
    }
}

impl<A: Anomaly> IntoIterator for &AnomalySet<A> {
    type Item = A;
    type IntoIter = Iter<A>;

    fn into_iter(self) -> Iter<A> {
        self.iter()
    }
}

/// Iterator over the flags of an [`AnomalySet`], in ascending bit order.
#[derive(Debug, Clone)]
pub struct Iter<A> {
    bits: u32,
    flags: PhantomData<A>,
}

impl<A: Anomaly> Iterator for Iter<A> {
    type Item = A;

    fn next(&mut self) -> Option<A> {
        // Each pass clears the lowest set bit: at most 32 passes (GEN-022).
        while self.bits != 0 {
            let low = self.bits.trailing_zeros();
            self.bits &= self.bits.wrapping_sub(1);
            if let Some(flag) = u8::try_from(low).ok().and_then(A::from_bit) {
                return Some(flag);
            }
        }
        None
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Anomaly, AnomalySet};

    /// A flag type whose index is its value, so any index (including ≥ 32) can be tried.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Flag(u8);

    impl Anomaly for Flag {
        fn bit(self) -> u8 {
            self.0
        }

        fn from_bit(bit: u8) -> Option<Flag> {
            (bit < AnomalySet::<Flag>::CAPACITY).then_some(Flag(bit))
        }
    }

    /// Only even indices are flags: odd bits, if ever set, are skipped by the iterator.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Even(u8);

    impl Anomaly for Even {
        fn bit(self) -> u8 {
            self.0
        }

        fn from_bit(bit: u8) -> Option<Even> {
            bit.is_multiple_of(2).then_some(Even(bit))
        }
    }

    #[test]
    fn empty_and_default() {
        // req: GEN-043
        let set = AnomalySet::<Flag>::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.bits(), 0);
        assert_eq!(set.iter().next(), None);
        assert_eq!(set, AnomalySet::default());
        assert!(!set.contains(Flag(0)));
    }

    #[test]
    fn insert_contains_bits() {
        // req: GEN-043
        let mut set = AnomalySet::empty();
        set.insert(Flag(0));
        set.insert(Flag(31));
        set.insert(Flag(5));
        set.insert(Flag(5));
        assert!(!set.is_empty());
        assert_eq!(set.len(), 3);
        assert_eq!(set.bits(), 0x8000_0021);
        assert!(set.contains(Flag(0)) && set.contains(Flag(5)) && set.contains(Flag(31)));
        assert!(!set.contains(Flag(1)) && !set.contains(Flag(30)));
        let mut one = AnomalySet::empty();
        one.insert(Flag(1));
        assert_eq!(one.bits(), 0b10);
        assert!(one.contains(Flag(1)) && !one.contains(Flag(0)));
    }

    #[test]
    fn unrepresentable_index_is_ignored() {
        // req: GEN-043, GEN-020
        let mut set = AnomalySet::empty();
        for bit in [32, 33, 63, 64, 255] {
            set.insert(Flag(bit));
            assert!(!set.contains(Flag(bit)));
        }
        assert!(set.is_empty());
    }

    #[test]
    fn iterates_in_bit_order() {
        // req: GEN-043
        let set: AnomalySet<Flag> = [Flag(31), Flag(2), Flag(17), Flag(0)].into_iter().collect();
        let mut it = set.iter();
        assert_eq!(it.next(), Some(Flag(0)));
        assert_eq!(it.next(), Some(Flag(2)));
        assert_eq!(it.next(), Some(Flag(17)));
        assert_eq!(it.next(), Some(Flag(31)));
        assert_eq!(it.next(), None);
        assert_eq!(it.next(), None);
        assert_eq!((&set).into_iter().count(), 4);
        assert_eq!(set.into_iter().count(), 4);
        let all: AnomalySet<Flag> = (0..32).map(Flag).collect();
        assert_eq!(all.bits(), u32::MAX);
        assert!(all.iter().map(|f| f.0).eq(0..32));
    }

    #[test]
    fn iterator_skips_unknown_bits() {
        // req: GEN-043
        let set: AnomalySet<Even> = [Even(1), Even(2), Even(3), Even(4)].into_iter().collect();
        assert_eq!(set.len(), 4);
        assert!(set.iter().eq([Even(2), Even(4)]));
        let odd: AnomalySet<Even> = [Even(7)].into_iter().collect();
        assert_eq!(odd.iter().next(), None);
    }

    #[test]
    fn debug_lists_flags() {
        // req: GEN-043
        extern crate std;
        use std::format;
        let set: AnomalySet<Flag> = [Flag(3), Flag(1)].into_iter().collect();
        assert_eq!(format!("{set:?}"), "{Flag(1), Flag(3)}");
        assert_eq!(format!("{:?}", AnomalySet::<Flag>::empty()), "{}");
    }

    #[test]
    fn equality_and_copy() {
        // req: GEN-043
        let a: AnomalySet<Flag> = [Flag(4)].into_iter().collect();
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, AnomalySet::empty());
    }
}
