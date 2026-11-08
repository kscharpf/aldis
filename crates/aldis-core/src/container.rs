//! Fixed-capacity containers for heapless engines (GEN-010, GEN-011, GEN-012, API-060).
//!
//! Capacities are const generics, so an engine's memory footprint is known at compile time
//! (`core::mem::size_of`). Nothing allocates, and no operation panics: a full container
//! rejects the item. Engines use these containers for input-side storage only (API-052,
//! API-060) and report `WouldBlock` when an input would overflow it; they hold no output.

use crate::error::{Error, ErrorKind, Result};

/// Vector of up to `N` copyable items.
#[derive(Debug, Clone, Copy)]
pub struct FixedVec<T: Copy + Default, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> FixedVec<T, N> {
    /// An empty vector.
    #[must_use]
    pub fn new() -> FixedVec<T, N> {
        FixedVec {
            items: [T::default(); N],
            len: 0,
        }
    }

    /// Appends `item`.
    ///
    /// # Errors
    /// `CapacityExceeded` when `N` items are already stored.
    pub fn push(&mut self, item: T) -> Result<()> {
        let slot = self
            .items
            .get_mut(self.len)
            .ok_or(Error::new(ErrorKind::CapacityExceeded))?;
        *slot = item;
        self.len = self.len.wrapping_add(1);
        Ok(())
    }

    /// Removes and returns the last item.
    pub fn pop(&mut self) -> Option<T> {
        let last = self.len.checked_sub(1)?;
        self.len = last;
        self.items.get(last).copied()
    }

    /// The stored items.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        self.items.get(..self.len).unwrap_or_default()
    }

    /// Mutable access to the stored items.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.items.get_mut(..self.len).unwrap_or_default()
    }

    /// Number of stored items.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// The capacity `N`.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Whether no item is stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether `N` items are stored.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.len >= N
    }

    /// Removes all items.
    pub const fn clear(&mut self) {
        self.len = 0;
    }
}

impl<T: Copy + Default + PartialEq, const N: usize> PartialEq for FixedVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + Default + Eq, const N: usize> Eq for FixedVec<T, N> {}

impl<T: Copy + Default, const N: usize> Default for FixedVec<T, N> {
    fn default() -> FixedVec<T, N> {
        FixedVec::new()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use super::FixedVec;
    use crate::error::ErrorKind;

    #[test]
    fn footprint_is_known_at_compile_time() {
        // req: GEN-012
        // Evaluated by the compiler: an engine's footprint is a compile-time constant.
        const _: () =
            assert!(core::mem::size_of::<FixedVec<u16, 4>>() >= 4 * core::mem::size_of::<u16>());
    }

    #[test]
    fn fixed_vec() {
        // req: GEN-010, RS-012
        let mut v: FixedVec<u16, 2> = FixedVec::default();
        assert_eq!((v.capacity(), v.is_empty(), v.is_full()), (2, true, false));
        v.push(7).unwrap();
        v.push(8).unwrap();
        assert!(v.is_full());
        assert_eq!(v.push(9).unwrap_err().kind(), ErrorKind::CapacityExceeded);
        assert_eq!(
            (v.as_slice(), v.len(), v.is_empty()),
            (&[7, 8][..], 2, false)
        );
        v.as_mut_slice()[0] = 6;
        assert_eq!(v.as_slice(), &[6, 8]);
        assert_eq!(v.pop(), Some(8));
        let mut v2: FixedVec<u16, 2> = FixedVec::new();
        v2.push(6).unwrap();
        // Equality compares only active elements (0..len), ignoring popped slots.
        assert_eq!(v, v2);
        v2.push(7).unwrap();
        assert_ne!(v, v2);
        v.clear();
        assert!(v.is_empty());
        assert_eq!(v.pop(), None);
        assert_eq!(v.as_slice(), &[][..]);
    }
}
