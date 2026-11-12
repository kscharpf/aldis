//! A FIFO of variable-length octet records in a ring of `N` octets, for test models.
//!
//! It was a container of `aldis-core`; the flight crates have no user for it, and the tests,
//! fuzz targets, and strategies that exercise it live here (VER-054). Each record costs its
//! length plus a 4-octet length prefix. The type has no dependency on the flight crates, so
//! every tooling crate can use it without a feature.

/// Why a [`ByteQueue`] operation failed; the queue is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteQueueError {
    /// The record can never fit: `len + 4 > N`. `max_len` is the longest record the queue
    /// can hold (0 when `N < 4`).
    RecordTooLong {
        /// The longest record the queue can hold.
        max_len: usize,
        /// The length of the rejected record.
        len: usize,
    },
    /// The record does not fit now.
    WouldBlock,
    /// The output buffer is shorter than the oldest record (GEN-042).
    BufferTooSmall {
        /// The length of the oldest record.
        needed: usize,
    },
}

/// FIFO of variable-length octet records in a ring of `N` octets.
#[derive(Debug, Clone)]
pub struct ByteQueue<const N: usize> {
    ring: [u8; N],
    head: usize,
    used: usize,
    records: usize,
}

const PREFIX: usize = 4;

/// `(i + k) mod n` for `i < n` and `k <= n`; `n` is the queue capacity, nonzero when called.
const fn wrap_add(i: usize, k: usize, n: usize) -> usize {
    let s = i + k;
    if s >= n { s - n } else { s }
}

impl<const N: usize> ByteQueue<N> {
    /// Record lengths are stored in a 4-octet prefix, so the capacity must fit in `u32`.
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

    /// Copies `out.len()` (<= N) octets starting at ring offset `at` (<= N), wrapping.
    fn read_at(&self, at: usize, out: &mut [u8]) {
        // `at` is always a ring offset the queue computed, so it is at most N.
        let (before, from_at) = self.ring.split_at(at);
        for (o, &r) in out.iter_mut().zip(from_at.iter().chain(before)) {
            *o = r;
        }
    }

    /// Writes `data` (<= N octets) starting at ring offset `at` (<= N), wrapping; returns the
    /// offset after the last octet written.
    fn write_at(&mut self, at: usize, data: &[u8]) -> usize {
        // `at` is always a ring offset the queue computed, so it is at most N.
        let (before, from_at) = self.ring.split_at_mut(at);
        for (slot, &d) in from_at.iter_mut().chain(before).zip(data) {
            *slot = d;
        }
        wrap_add(at, data.len(), N)
    }

    /// Appends a record.
    ///
    /// # Errors
    /// `RecordTooLong` if the record can never fit (`len + 4 > N`); `WouldBlock` if it does
    /// not fit now.
    pub fn push(&mut self, data: &[u8]) -> Result<(), ByteQueueError> {
        let too_long = ByteQueueError::RecordTooLong {
            max_len: N.saturating_sub(PREFIX),
            len: data.len(),
        };
        let len = u32::try_from(data.len()).map_err(|_| too_long)?;
        let need = data
            .len()
            .checked_add(PREFIX)
            .filter(|&n| n <= N)
            .ok_or(too_long)?;
        if need > self.free() {
            return Err(ByteQueueError::WouldBlock);
        }
        let tail = wrap_add(self.head, self.used, N);
        let after_prefix = self.write_at(tail, &len.to_be_bytes());
        self.write_at(after_prefix, data);
        self.used += need;
        self.records += 1;
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
    /// queued (GEN-042).
    pub fn pop_into(&mut self, out: &mut [u8]) -> Result<Option<usize>, ByteQueueError> {
        let Some(len) = self.peek_len() else {
            return Ok(None);
        };
        let dest = out
            .get_mut(..len)
            .ok_or(ByteQueueError::BufferTooSmall { needed: len })?;
        let start = wrap_add(self.head, PREFIX, N);
        self.read_at(start, dest);
        self.discard();
        Ok(Some(len))
    }

    /// Discards the oldest record, returning its length.
    pub fn discard(&mut self) -> Option<usize> {
        let len = self.peek_len()?;
        let consumed = len + PREFIX;
        self.head = wrap_add(self.head, consumed, N);
        self.used -= consumed;
        self.records -= 1;
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

#[cfg(test)]
mod tests {
    use super::{ByteQueue, ByteQueueError};

    #[test]
    fn peek_len_reports_the_oldest_record() {
        // req: VER-054
        let mut q: ByteQueue<16> = ByteQueue::new();
        assert_eq!(q.peek_len(), None);
        q.push(&[7; 5]).unwrap();
        q.push(&[]).unwrap();
        assert_eq!(q.peek_len(), Some(5));
        assert_eq!(q.discard(), Some(5));
        assert_eq!(q.peek_len(), Some(0));
        assert_eq!(q.discard(), Some(0));
        assert_eq!(q.peek_len(), None);
    }

    #[test]
    fn free_and_contents_survive_wrap_around() {
        // req: VER-054
        let mut q: ByteQueue<16> = ByteQueue::new();
        assert_eq!((q.free(), q.capacity(), q.is_empty()), (16, 16, true));
        q.push(&[1; 8]).unwrap();
        assert_eq!(q.free(), 4);
        let mut out = [0_u8; 8];
        assert_eq!(q.pop_into(&mut out), Ok(Some(8)));
        assert_eq!(q.free(), 16);
        // The head is now at offset 12: this record's prefix and data wrap past the end.
        q.push(&[2, 3, 4, 5, 6, 7]).unwrap();
        assert_eq!(q.free(), 6);
        assert_eq!(q.peek_len(), Some(6));
        assert_eq!(q.pop_into(&mut out), Ok(Some(6)));
        assert_eq!(&out[..6], &[2, 3, 4, 5, 6, 7]);
        assert_eq!((q.free(), q.records()), (16, 0));
    }

    #[test]
    fn zero_capacity_holds_nothing() {
        // req: VER-054
        let mut q: ByteQueue<0> = ByteQueue::default();
        assert_eq!((q.capacity(), q.free(), q.is_empty()), (0, 0, true));
        assert_eq!(
            q.push(&[1]),
            Err(ByteQueueError::RecordTooLong { max_len: 0, len: 1 })
        );
        assert_eq!(q.peek_len(), None);
        assert_eq!(q.pop_into(&mut [0_u8; 4]), Ok(None));
        assert_eq!(q.discard(), None);
        q.clear();
        assert!(q.is_empty());
    }

    #[test]
    fn errors_keep_records() {
        // req: VER-054
        let mut q: ByteQueue<16> = ByteQueue::default();
        assert_eq!(q.capacity(), 16);
        assert_eq!(
            q.push(&[0; 13]),
            Err(ByteQueueError::RecordTooLong {
                max_len: 12,
                len: 13
            })
        );
        q.push(&[1, 2, 3]).unwrap();
        assert_eq!(q.push(&[0; 9]), Err(ByteQueueError::WouldBlock));
        let mut small = [0_u8; 2];
        assert_eq!(
            q.pop_into(&mut small),
            Err(ByteQueueError::BufferTooSmall { needed: 3 })
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
        assert_eq!(
            ByteQueue::<0>::new().push(&[]),
            Err(ByteQueueError::RecordTooLong { max_len: 0, len: 0 })
        );
    }
}

#[cfg(all(test, feature = "strategies"))]
mod model_tests {
    use std::collections::VecDeque;

    use super::{ByteQueue, ByteQueueError};
    use crate::strategies::common::{QueueOp, byte_queue_ops};
    use crate::strategies::config;
    use proptest::prelude::*;

    /// Failing cases are saved here and replayed first.
    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/byte_queue.txt"
    );

    proptest! {
        #![proptest_config(config(256, REGRESSIONS))]

        #[test]
        fn matches_model(ops in byte_queue_ops(20, 200)) {
            // req: VER-054
            let mut q: ByteQueue<37> = ByteQueue::new();
            let mut model: VecDeque<Vec<u8>> = VecDeque::new();
            let mut used = 0;
            for op in ops {
                match op {
                    QueueOp::Push(data) => {
                        let need = data.len() + 4;
                        let r = q.push(&data);
                        if need > 37 {
                            let too_long = matches!(r, Err(ByteQueueError::RecordTooLong { .. }));
                            prop_assert!(too_long);
                        } else if used + need > 37 {
                            prop_assert_eq!(r, Err(ByteQueueError::WouldBlock));
                        } else {
                            prop_assert!(r.is_ok());
                            used += need;
                            model.push_back(data);
                        }
                    }
                    QueueOp::Pop => {
                        let mut out = [0_u8; 40];
                        let got = q.pop_into(&mut out).unwrap();
                        let expected = model.pop_front();
                        let freed = expected.as_ref().map_or(0, |e| e.len() + 4);
                        prop_assert_eq!(got.map(|n| out[..n].to_vec()), expected);
                        used -= freed;
                    }
                    QueueOp::Discard => {
                        let expected = model.pop_front();
                        prop_assert_eq!(q.discard(), expected.as_ref().map(Vec::len));
                        used -= expected.map_or(0, |e| e.len() + 4);
                    }
                }
                prop_assert_eq!(q.records(), model.len());
                prop_assert_eq!(q.free(), 37 - used);
            }
        }
    }
}
