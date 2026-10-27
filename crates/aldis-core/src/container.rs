//! Fixed-capacity containers for heapless engines (GEN-010, GEN-011, GEN-012, API-061).
//!
//! Capacities are const generics, so an engine's memory footprint is known at compile time
//! (`core::mem::size_of`). Nothing allocates, and no operation panics: a full container
//! rejects the item (engines report `WouldBlock`, API-052), and a too-small output buffer
//! leaves the item queued (API-061).

use crate::error::{Error, ErrorKind, Result};
use crate::field;

/// `(i + k) mod n` for `i < n` and `k <= n`, without division. Callers guarantee `n > 0`
/// (a zero-capacity container is always full and always empty, so it never indexes).
const fn wrap_add(i: usize, k: usize, n: usize) -> usize {
    let s = i.wrapping_add(k);
    if s >= n { s.wrapping_sub(n) } else { s }
}

/// `(i + 1) mod n` for `i < n`.
const fn wrap_inc(i: usize, n: usize) -> usize {
    wrap_add(i, 1, n)
}

/// FIFO queue of up to `N` items.
#[derive(Debug, Clone)]
pub struct FixedQueue<T, const N: usize> {
    slots: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T, const N: usize> FixedQueue<T, N> {
    /// An empty queue.
    #[must_use]
    pub const fn new() -> FixedQueue<T, N> {
        FixedQueue {
            slots: [const { None }; N],
            head: 0,
            len: 0,
        }
    }

    /// Number of queued items.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the queue is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the queue is full.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.len >= N
    }

    /// The capacity `N`.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Appends `item`, or returns it if the queue is full.
    ///
    /// # Errors
    /// Returns the item unchanged when the queue is full.
    pub fn push(&mut self, item: T) -> core::result::Result<(), T> {
        // When full, the tail wraps onto the head slot and the guard rejects the item; with
        // N = 0 there is no slot at all. Both cases share the rejecting arm.
        let tail = wrap_add(self.head, self.len, N);
        match self.slots.get_mut(tail) {
            Some(slot) if self.len < N => {
                *slot = Some(item);
                self.len = self.len.wrapping_add(1);
                Ok(())
            }
            _ => Err(item),
        }
    }

    /// Removes and returns the oldest item.
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let item = self.slots.get_mut(self.head).and_then(Option::take);
        self.head = wrap_inc(self.head, N);
        self.len = self.len.wrapping_sub(1);
        item
    }

    /// The oldest item, without removing it.
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            self.slots.get(self.head).and_then(Option::as_ref)
        }
    }

    /// Removes all items.
    pub fn clear(&mut self) {
        while self.pop().is_some() {}
        self.head = 0;
    }
}

impl<T, const N: usize> Default for FixedQueue<T, N> {
    fn default() -> FixedQueue<T, N> {
        FixedQueue::new()
    }
}

/// FIFO of variable-length octet records in a ring of `N` octets.
///
/// Each record costs its length plus a 4-octet length prefix. Used for queued packets and
/// SDUs whose sizes vary (API-060, API-061).
#[derive(Debug, Clone)]
pub struct ByteQueue<const N: usize> {
    ring: [u8; N],
    head: usize,
    used: usize,
    records: usize,
}

const PREFIX: usize = 4;

const fn record_len_err(cap: usize, len: usize) -> Error {
    if let Some(max_len) = cap.checked_sub(PREFIX) {
        Error::out_of_range(
            field::RECORD_LEN,
            crate::error::as_u64(max_len),
            crate::error::as_u64(len),
        )
    } else {
        Error::out_of_range(
            field::CAPACITY,
            crate::error::as_u64(PREFIX),
            crate::error::as_u64(cap),
        )
    }
}

impl<const N: usize> ByteQueue<N> {
    /// Record lengths are stored in a 4-octet prefix, so the capacity must fit in `u32`.
    #[expect(
        clippy::as_conversions,
        reason = "u32::MAX fits in usize on 32- and 64-bit targets"
    )]
    const CAPACITY_FITS_PREFIX: () =
        assert!(N <= u32::MAX as usize, "ByteQueue capacity must fit in u32");

    /// An empty queue.
    #[must_use]
    pub const fn new() -> ByteQueue<N> {
        let () = Self::CAPACITY_FITS_PREFIX;
        ByteQueue {
            ring: [0; N],
            head: 0,
            used: 0,
            records: 0,
        }
    }

    /// Number of queued records.
    #[must_use]
    pub const fn records(&self) -> usize {
        self.records
    }

    /// Whether no record is queued.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.records == 0
    }

    /// Free octets, including the space a new record's prefix needs.
    #[must_use]
    pub const fn free(&self) -> usize {
        N.saturating_sub(self.used)
    }

    /// The capacity `N` in octets.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Copies `out.len()` (≤ N) octets starting at ring offset `at` (< N), wrapping.
    fn read_at(&self, at: usize, out: &mut [u8]) {
        let (before, from_at) = self.ring.split_at_checked(at).unwrap_or_default();
        for (o, &r) in out.iter_mut().zip(from_at.iter().chain(before)) {
            *o = r;
        }
    }

    /// Writes `data` (≤ N octets) starting at ring offset `at` (< N), wrapping; returns the
    /// offset after the last octet written.
    fn write_at(&mut self, at: usize, data: &[u8]) -> usize {
        let (before, from_at) = self.ring.split_at_mut_checked(at).unwrap_or_default();
        for (slot, &d) in from_at.iter_mut().chain(before).zip(data) {
            *slot = d;
        }
        wrap_add(at, data.len(), N)
    }

    /// Appends a record.
    ///
    /// # Errors
    /// `OutOfRange` if the record can never fit (`len + 4 > N`); `WouldBlock` if it does not
    /// fit now. The queue is unchanged on error.
    pub fn push(&mut self, data: &[u8]) -> Result<()> {
        let need = data
            .len()
            .checked_add(PREFIX)
            .filter(|&n| n <= N)
            .ok_or_else(|| record_len_err(N, data.len()))?;
        if need > self.free() {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        // need <= N <= u32::MAX, so the length always fits the prefix.
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let tail = wrap_add(self.head, self.used, N);
        let after_prefix = self.write_at(tail, &len.to_be_bytes());
        self.write_at(after_prefix, data);
        self.used = self.used.wrapping_add(need);
        self.records = self.records.wrapping_add(1);
        Ok(())
    }

    /// Length of the oldest record.
    #[must_use]
    pub fn peek_len(&self) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let mut prefix = [0_u8; PREFIX];
        self.read_at(self.head, &mut prefix);
        usize::try_from(u32::from_be_bytes(prefix)).ok()
    }

    /// Removes the oldest record into `out`, returning its length.
    ///
    /// # Errors
    /// `BufferTooSmall { needed }` if `out` is shorter than the record; the record stays
    /// queued (API-061).
    pub fn pop_into(&mut self, out: &mut [u8]) -> Result<Option<usize>> {
        let Some(len) = self.peek_len() else {
            return Ok(None);
        };
        let dest = out.get_mut(..len).ok_or(Error::buffer_too_small(len))?;
        let start = wrap_add(self.head, PREFIX, N);
        self.read_at(start, dest);
        let consumed = len.wrapping_add(PREFIX);
        self.head = wrap_add(self.head, consumed, N);
        self.used = self.used.wrapping_sub(consumed);
        self.records = self.records.wrapping_sub(1);
        Ok(Some(len))
    }

    /// Discards the oldest record, returning its length.
    pub fn discard(&mut self) -> Option<usize> {
        let len = self.peek_len()?;
        let consumed = len.wrapping_add(PREFIX);
        self.head = wrap_add(self.head, consumed, N);
        self.used = self.used.wrapping_sub(consumed);
        self.records = self.records.wrapping_sub(1);
        Some(len)
    }

    /// Removes all records.
    pub const fn clear(&mut self) {
        self.head = 0;
        self.used = 0;
        self.records = 0;
    }
}

impl<const N: usize> Default for ByteQueue<N> {
    fn default() -> ByteQueue<N> {
        ByteQueue::new()
    }
}

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
    use super::{ByteQueue, FixedQueue, FixedVec};
    use crate::error::{ErrorDetail, ErrorKind};

    #[test]
    fn queue_basics_and_zero_capacity() {
        // req: GEN-010, GEN-011, API-051
        let mut q: FixedQueue<u8, 2> = FixedQueue::default();
        assert_eq!((q.capacity(), q.peek(), q.is_full()), (2, None, false));
        assert_eq!(q.push(1), Ok(()));
        assert!(!q.is_full());
        assert_eq!(q.push(2), Ok(()));
        assert!(q.is_full());
        assert_eq!(q.push(3), Err(3));
        assert_eq!(q.peek(), Some(&1));
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.push(3), Ok(()));
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.pop(), None);
        let mut z: FixedQueue<u8, 0> = FixedQueue::new();
        assert!(z.is_full());
        assert_eq!(z.push(1), Err(1));
        assert_eq!(z.pop(), None);
    }

    #[test]
    fn byte_queue_errors_keep_records() {
        // req: API-061, GEN-042
        let mut q: ByteQueue<16> = ByteQueue::default();
        assert_eq!(q.capacity(), 16);
        let too_long = q.push(&[0; 13]).unwrap_err();
        assert_eq!(too_long.kind(), ErrorKind::OutOfRange);
        assert_eq!(
            too_long.detail(),
            ErrorDetail::Expected {
                expected: 12,
                found: 13
            }
        );
        q.push(&[1, 2, 3]).unwrap();
        assert_eq!(q.push(&[0; 9]).unwrap_err().kind(), ErrorKind::WouldBlock);
        let mut small = [0_u8; 2];
        let e = q.pop_into(&mut small).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(3))
        );
        assert_eq!(q.records(), 1);
        let mut out = [0_u8; 8];
        assert_eq!(q.pop_into(&mut out), Ok(Some(3)));
        assert_eq!(&out[..3], &[1, 2, 3]);
        assert_eq!(q.pop_into(&mut out), Ok(None));
        assert_eq!(q.discard(), None);
        q.push(&[]).unwrap();
        assert_eq!(q.discard(), Some(0));
        q.push(&[1]).unwrap();
        q.clear();
        assert!(q.is_empty());
        let tiny = ByteQueue::<0>::new().push(&[]).unwrap_err();
        assert_eq!(tiny.kind(), ErrorKind::OutOfRange);
        assert_eq!(
            tiny.detail(),
            ErrorDetail::Expected {
                expected: 4,
                found: 0
            }
        );
    }

    #[test]
    fn footprint_is_known_at_compile_time() {
        // req: GEN-012
        // Evaluated by the compiler: an engine's footprint is a compile-time constant.
        const _: () =
            assert!(core::mem::size_of::<FixedQueue<u32, 8>>() >= 8 * core::mem::size_of::<u32>());
        const _: () = assert!(core::mem::size_of::<ByteQueue<256>>() >= 256);
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
