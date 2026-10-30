//! Lenient-mode anomaly flags (GEN-043, API-032, GEN-081).
//!
//! A lenient decoder returns an [`AnomalySet`] alongside a successful result, in a
//! [`Lenient`](crate::mode::Lenient). Each codec crate declares its own flag enum
//! with [`anomaly_flags!`](crate::anomaly_flags), which makes the enum `#[non_exhaustive]`
//! (GEN-081) and implements [`Anomaly`] from one table of bit indices. The indices are listed
//! in each standard document (API-033) and are shared with the C ABI, so a flag is never
//! renumbered. A strict decoder returns the view alone, with no anomaly set (API-031).

use core::fmt;
use core::hash::Hash;
use core::iter::FusedIterator;
use core::marker::PhantomData;

/// Number of distinct bit indices an [`AnomalySet`] can hold.
pub const CAPACITY: u8 = 32;

/// Sealing for [`Anomaly`]: implemented by [`anomaly_flags!`](crate::anomaly_flags).
///
/// Not part of the public API. Only the macro and deliberately broken test doubles in Aldis's own
/// tests, fuzz targets, and exerciser implement it (RS-020).
#[doc(hidden)]
pub mod __private {
    /// Supertrait that seals [`Anomaly`](super::Anomaly).
    pub trait Sealed {}
}

/// A lenient-mode anomaly flag with a stable bit index (GEN-043).
///
/// The trait is sealed: declare flag types with [`anomaly_flags!`](crate::anomaly_flags), which
/// implements it and guarantees its invariants at compile time (RS-020). The invariants are:
/// - every flag's index is below [`CAPACITY`];
/// - no two flags share an index;
/// - `from_bit(f.bit()) == Some(f)` for every flag `f`, and `from_bit` returns `None` for an
///   index no flag uses.
///
/// If they are broken, [`AnomalySet::insert`] drops a flag whose index is too large, and
/// [`AnomalySet::iter`] skips set bits that `from_bit` does not map back to a flag, so `iter`
/// and `Debug` disagree with `len`, `is_empty`, and `bits`.
pub trait Anomaly: __private::Sealed + Copy + Eq + Hash + fmt::Debug {
    /// The flag's bit index, below [`CAPACITY`]. Stable: never renumbered.
    fn bit(self) -> u8;

    /// The flag with bit index `bit`, or `None` if no flag uses it.
    fn from_bit(bit: u8) -> Option<Self>;
}

/// Declares a `#[non_exhaustive]` anomaly flag enum and its [`Anomaly`] implementation from one
/// table of stable bit indices (GEN-043, GEN-081).
///
/// Each index must be a literal below [`CAPACITY`] and unique within the enum; otherwise the
/// crate does not compile. The enum also gets `ALL`, every flag in declaration order.
///
/// ```
/// aldis_core::anomaly_flags! {
///     /// Flags of a hypothetical codec.
///     pub enum DemoAnomaly {
///         /// Bit 0.
///         SpareBitSet = 0,
///         /// Bit 1.
///         ConventionalZeroSet = 1,
///     }
/// }
/// # use aldis_core::Anomaly;
/// assert_eq!(DemoAnomaly::from_bit(1), Some(DemoAnomaly::ConventionalZeroSet));
/// ```
///
/// An index of 32 or more, or one used twice, fails const evaluation:
///
/// ```compile_fail,E0080
/// aldis_core::anomaly_flags! {
///     /// Out of range.
///     pub enum TooHigh {
///         /// Bit 32.
///         Flag = 32,
///     }
/// }
/// ```
///
/// ```compile_fail,E0080
/// aldis_core::anomaly_flags! {
///     /// Duplicate index.
///     pub enum Twice {
///         /// Bit 3.
///         First = 3,
///         /// Bit 3 again.
///         Second = 3,
///     }
/// }
/// ```
#[macro_export]
macro_rules! anomaly_flags {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$flag_meta:meta])* $flag:ident = $bit:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        $vis enum $name {
            $( $(#[$flag_meta])* $flag, )+
        }

        impl $name {
            /// Every flag, in declaration order.
            $vis const ALL: &'static [$name] = &[$($name::$flag),+];
        }

        impl $crate::anomaly::__private::Sealed for $name {}

        impl $crate::anomaly::Anomaly for $name {
            fn bit(self) -> u8 {
                match self {
                    $( $name::$flag => $bit, )+
                }
            }

            fn from_bit(bit: u8) -> ::core::option::Option<$name> {
                match bit {
                    $( $bit => ::core::option::Option::Some($name::$flag), )+
                    _ => ::core::option::Option::None,
                }
            }
        }

        // Compile-time check of the `Anomaly` invariants: every index in range and unique.
        const _: () = {
            let mut seen: u32 = 0;
            $(
                assert!($bit < $crate::anomaly::CAPACITY, "anomaly bit index must be below 32");
                assert!(seen & (1 << $bit) == 0, "anomaly bit index used twice");
                seen |= 1 << $bit;
            )+
            let _ = seen;
        };
    };
}

/// A set of up to [`CAPACITY`] anomaly flags of type `A`, stored as a `u32` bit set (GEN-043).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnomalySet<A> {
    bits: u32,
    flags: PhantomData<A>,
}

impl<A> AnomalySet<A> {
    /// The empty set.
    #[must_use]
    pub const fn empty() -> AnomalySet<A> {
        AnomalySet {
            bits: 0,
            flags: PhantomData,
        }
    }

    /// Whether no bit is set.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// Number of bits set; equal to the number of flags while the [`Anomaly`] invariants hold.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::try_from(self.bits.count_ones()).unwrap_or(usize::MAX)
    }

    /// The raw bit set: bit `n` is set when the flag with index `n` is present.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }
}

/// The single-bit mask for index `bit`; 0 if `bit` is not below [`CAPACITY`].
fn bit_mask(bit: u8) -> u32 {
    1_u32.checked_shl(u32::from(bit)).unwrap_or(0)
}

/// The single-bit mask for `flag`; 0 if its index is not below [`CAPACITY`].
fn mask<A: Anomaly>(flag: A) -> u32 {
    bit_mask(flag.bit())
}

impl<A: Anomaly> AnomalySet<A> {
    /// The set holding each of `flags`.
    #[must_use]
    pub fn from_flags(flags: &[A]) -> AnomalySet<A> {
        let mut set = AnomalySet::empty();
        for &flag in flags {
            set.insert(flag);
        }
        set
    }

    /// Adds `flag`. A flag whose index is not below [`CAPACITY`] cannot be represented and is
    /// ignored.
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
    pub const fn iter(&self) -> Iter<A> {
        Iter {
            bits: self.bits,
            next: 0,
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
    /// Lowest index not yet visited; only rises, up to [`CAPACITY`].
    next: u8,
    flags: PhantomData<A>,
}

impl<A: Anomaly> Iterator for Iter<A> {
    type Item = A;

    fn next(&mut self) -> Option<A> {
        // `next` rises on every pass and stops at CAPACITY, so a whole iteration visits each
        // index at most once (GEN-022).
        for bit in self.next..CAPACITY {
            self.next = bit.saturating_add(1);
            if self.bits & bit_mask(bit) != 0
                && let Some(flag) = A::from_bit(bit)
            {
                return Some(flag);
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let unvisited = u32::MAX.checked_shl(u32::from(self.next)).unwrap_or(0);
        (
            0,
            usize::try_from((self.bits & unvisited).count_ones()).ok(),
        )
    }
}

impl<A: Anomaly> FusedIterator for Iter<A> {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Anomaly, AnomalySet, CAPACITY};

    crate::anomaly_flags! {
        /// Flags declared through the macro, with indices out of declaration order.
        enum Demo {
            /// Bit 4.
            Four = 4,
            /// Bit 0.
            Zero = 0,
            /// Bit 31.
            Last = 31,
        }
    }

    /// A hand-written flag type whose index is its value, so any index (including ≥ 32) can be
    /// tried. It breaks the first invariant on purpose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Flag(u8);

    impl super::__private::Sealed for Flag {}

    impl Anomaly for Flag {
        fn bit(self) -> u8 {
            self.0
        }

        fn from_bit(bit: u8) -> Option<Flag> {
            (bit < CAPACITY).then_some(Flag(bit))
        }
    }

    /// Only even indices map back: breaks the third invariant on purpose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Even(u8);

    impl super::__private::Sealed for Even {}

    impl Anomaly for Even {
        fn bit(self) -> u8 {
            self.0
        }

        fn from_bit(bit: u8) -> Option<Even> {
            bit.is_multiple_of(2).then_some(Even(bit))
        }
    }

    /// No index maps back.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Unknown(u8);

    impl super::__private::Sealed for Unknown {}

    impl Anomaly for Unknown {
        fn bit(self) -> u8 {
            self.0
        }

        fn from_bit(_: u8) -> Option<Unknown> {
            None
        }
    }

    #[test]
    fn macro_keeps_the_invariants() {
        // req: GEN-043, GEN-081
        assert_eq!(Demo::ALL, [Demo::Four, Demo::Zero, Demo::Last]);
        for (&flag, bit) in Demo::ALL.iter().zip([4, 0, 31]) {
            assert_eq!(flag.bit(), bit);
            assert_eq!(Demo::from_bit(bit), Some(flag));
        }
        for bit in (0..=u8::MAX).filter(|b| ![0, 4, 31].contains(b)) {
            assert_eq!(Demo::from_bit(bit), None);
        }
        let set = AnomalySet::from_flags(Demo::ALL);
        assert_eq!(set.bits(), 0x8000_0011);
        assert!(set.iter().eq([Demo::Zero, Demo::Four, Demo::Last]));
    }

    #[test]
    fn empty_and_default() {
        // req: GEN-043
        let set = AnomalySet::<Flag>::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.bits(), 0);
        assert_eq!(set.iter().size_hint(), (0, Some(0)));
        assert_eq!(set.iter().next(), None);
        assert_eq!(set, AnomalySet::default());
        assert_eq!(set, AnomalySet::from_flags(&[]));
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
        assert_eq!(
            set,
            AnomalySet::from_flags(&[Flag(5), Flag(31), Flag(0), Flag(5)])
        );
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
        let set = AnomalySet::from_flags(&[Flag(31), Flag(2), Flag(17), Flag(0)]);
        let mut it = set.iter();
        assert_eq!(it.size_hint(), (0, Some(4)));
        assert_eq!(it.next(), Some(Flag(0)));
        assert_eq!(it.size_hint(), (0, Some(3)));
        assert_eq!(it.next(), Some(Flag(2)));
        assert_eq!(it.next(), Some(Flag(17)));
        assert_eq!(it.next(), Some(Flag(31)));
        assert_eq!(it.size_hint(), (0, Some(0)));
        assert_eq!(it.next(), None);
        assert_eq!(it.next(), None);
        assert_eq!((&set).into_iter().count(), 4);
        assert_eq!(set.into_iter().count(), 4);
        let all: [Flag; 32] = core::array::from_fn(|i| Flag(u8::try_from(i).unwrap()));
        let full = AnomalySet::from_flags(&all);
        assert_eq!(full.bits(), u32::MAX);
        assert_eq!(full.len(), 32);
        assert!(full.iter().eq(all));
    }

    #[test]
    fn iterator_skips_unmapped_bits() {
        // req: GEN-043, GEN-022
        let set = AnomalySet::from_flags(&[Even(1), Even(2), Even(3), Even(4)]);
        assert_eq!(set.len(), 4);
        assert!(set.iter().eq([Even(2), Even(4)]));
        assert_eq!(AnomalySet::from_flags(&[Even(7)]).iter().next(), None);
        // All 32 bits set and none maps back: the loop runs its full bound and ends.
        let all: [Unknown; 32] = core::array::from_fn(|i| Unknown(u8::try_from(i).unwrap()));
        let unknown = AnomalySet::from_flags(&all);
        assert_eq!(unknown.bits(), u32::MAX);
        let mut it = unknown.iter();
        assert_eq!(it.next(), None);
        assert_eq!(it.size_hint(), (0, Some(0)));
    }

    #[test]
    fn debug_lists_flags() {
        // req: GEN-043
        extern crate std;
        use std::format;
        let set = AnomalySet::from_flags(&[Flag(3), Flag(1)]);
        assert_eq!(format!("{set:?}"), "{Flag(1), Flag(3)}");
        assert_eq!(format!("{:?}", AnomalySet::<Flag>::empty()), "{}");
    }

    #[test]
    fn equality_and_copy() {
        // req: GEN-043
        let a = AnomalySet::from_flags(&[Flag(4)]);
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, AnomalySet::empty());
    }
}
