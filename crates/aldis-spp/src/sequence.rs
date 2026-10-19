//! Packet Sequence Count generation and tracking (133.0 §4.1.3.4.3, §4.3.2.2; SPP-011, GEN-011,
//! GEN-024, OI-30).

use aldis_core::container::FixedVec;
use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

/// Generates the Packet Sequence Count of one APID, modulo 16 384 (133.0 §4.1.3.4.3.4,
/// SPP-011, GEN-024). Counts are per APID, so keep one counter per APID (§4.1.3.4.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SequenceCounter {
    next: SeqCount,
}

impl SequenceCounter {
    /// A counter whose next count is `start`.
    #[must_use]
    pub const fn new(start: SeqCount) -> SequenceCounter {
        SequenceCounter { next: start }
    }

    /// The count the next packet will take.
    #[must_use]
    pub const fn peek(&self) -> SeqCount {
        self.next
    }

    /// Takes the next count and advances, wrapping from 16 383 to 0 (SPP-011, GEN-024).
    pub const fn advance(&mut self) -> SeqCount {
        let count = self.next;
        self.next = count.next();
        count
    }
}

/// The result of [`SequenceTracker::track`] (SPP-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tracked {
    /// The first count seen for this APID; tracking starts with it (OI-30).
    First,
    /// The continuity relative to the previous count of this APID (CMN-031): `InSequence`,
    /// `Gap { missing }`, `Repeat`, or `Backwards`, which is the "reset" of SPP-011.
    Next(Continuity),
}

/// One tracked APID; the APID is stored as its value so the entry has a `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Entry {
    apid: u16,
    last: SeqCount,
}

/// Classifies received Packet Sequence Counts per APID for up to `N` APIDs (133.0 §4.3.2.2,
/// SPP-011, GEN-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceTracker<const N: usize> {
    entries: FixedVec<Entry, N>,
}

impl<const N: usize> Default for SequenceTracker<N> {
    fn default() -> SequenceTracker<N> {
        SequenceTracker::new()
    }
}

impl<const N: usize> SequenceTracker<N> {
    /// An empty tracker.
    #[must_use]
    pub fn new() -> SequenceTracker<N> {
        SequenceTracker {
            entries: FixedVec::new(),
        }
    }

    /// Records `count` for `apid` and classifies it against the previous count (SPP-011).
    /// The previous count becomes `count` in every case except an error.
    ///
    /// # Errors
    /// `CapacityExceeded` (`apid`) for a new APID when `N` APIDs are tracked; nothing changes,
    /// and [`forget`](Self::forget) frees a slot (OI-30).
    pub fn track(&mut self, apid: Apid, count: SeqCount) -> Result<Tracked> {
        if let Some(entry) = self
            .entries
            .as_mut_slice()
            .iter_mut()
            .find(|e| e.apid == apid.get())
        {
            let continuity = entry.last.classify(count);
            entry.last = count;
            return Ok(Tracked::Next(continuity));
        }
        let capacity = u64::try_from(N).unwrap_or(u64::MAX);
        self.entries
            .push(Entry {
                apid: apid.get(),
                last: count,
            })
            .map_err(|_| {
                Error::new(ErrorKind::CapacityExceeded)
                    .with_field(FieldId::new("apid"))
                    .with_detail(ErrorDetail::Expected {
                        expected: capacity,
                        found: capacity.saturating_add(1),
                    })
            })?;
        Ok(Tracked::First)
    }

    /// Stops tracking `apid`; returns whether it was tracked.
    pub fn forget(&mut self, apid: Apid) -> bool {
        let slice = self.entries.as_mut_slice();
        let Some(at) = slice.iter().position(|e| e.apid == apid.get()) else {
            return false;
        };
        let last = slice.len().saturating_sub(1);
        slice.swap(at, last);
        self.entries.pop().is_some()
    }

    /// The last count recorded for `apid`, if it is tracked.
    #[must_use]
    pub fn last(&self, apid: Apid) -> Option<SeqCount> {
        self.entries
            .as_slice()
            .iter()
            .find(|e| e.apid == apid.get())
            .map(|e| e.last)
    }

    /// Number of APIDs tracked.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no APID is tracked.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The compile-time capacity `N` (GEN-011).
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{SequenceCounter, SequenceTracker, Tracked};
    use aldis_core::counter::Continuity;
    use aldis_core::ids::{Apid, SeqCount};
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};
    use proptest::prelude::*;

    fn apid(n: u16) -> Apid {
        Apid::new(n).unwrap()
    }

    fn count(n: u16) -> SeqCount {
        SeqCount::new(n).unwrap()
    }

    #[test]
    fn counter_wraps() {
        // req: SPP-011, GEN-024
        let mut c = SequenceCounter::new(count(16_383));
        assert_eq!(
            (c.peek(), c.advance(), c.advance(), c.peek()),
            (count(16_383), count(16_383), count(0), count(1))
        );
        assert_eq!(SequenceCounter::default().peek(), count(0));
    }

    #[test]
    fn tracker_paths() {
        // req: SPP-011, GEN-011, OI-30
        let mut t = SequenceTracker::<2>::default();
        assert!(t.is_empty() && t.capacity() == 2 && t.last(apid(1)).is_none());
        assert_eq!(t.track(apid(1), count(5)), Ok(Tracked::First));
        assert_eq!(t.track(apid(2), count(0)), Ok(Tracked::First));
        assert!(!t.is_empty());
        assert_eq!(
            t.track(apid(1), count(6)),
            Ok(Tracked::Next(Continuity::InSequence))
        );
        assert_eq!(
            t.track(apid(1), count(9)),
            Ok(Tracked::Next(Continuity::Gap { missing: 2 }))
        );
        let e = t.track(apid(3), count(0)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::CapacityExceeded, Some("apid"))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 3
            }
        );
        assert_eq!(
            (t.len(), t.last(apid(1)), t.last(apid(2))),
            (2, Some(count(9)), Some(count(0)))
        );
        assert!(t.forget(apid(1)) && !t.forget(apid(1)));
        assert_eq!((t.len(), t.last(apid(2))), (1, Some(count(0))));
        assert_eq!(t.track(apid(3), count(4)), Ok(Tracked::First));
        assert_eq!(
            SequenceTracker::<0>::new()
                .track(apid(1), count(0))
                .unwrap_err()
                .kind(),
            ErrorKind::CapacityExceeded
        );
    }

    proptest! {
        #[test]
        fn tracker_matches_a_model(events in proptest::collection::vec((0_u16..4, 0_u16..=0x3FFF, any::<bool>()), 0..60)) {
            // req: SPP-011, CMN-031, VER-023
            let mut t = SequenceTracker::<3>::new();
            let mut model: [Option<u16>; 4] = [None; 4];
            for (a, c, forget) in events {
                let slot = usize::from(a);
                if forget {
                    prop_assert_eq!(t.forget(apid(a)), model[slot].take().is_some());
                    continue;
                }
                let r = t.track(apid(a), count(c));
                match model[slot] {
                    Some(prev) => {
                        prop_assert_eq!(r, Ok(Tracked::Next(count(prev).classify(count(c)))));
                        model[slot] = Some(c);
                    }
                    None if model.iter().flatten().count() == 3 => {
                        prop_assert_eq!(r.unwrap_err().kind(), ErrorKind::CapacityExceeded);
                    }
                    None => {
                        prop_assert_eq!(r, Ok(Tracked::First));
                        model[slot] = Some(c);
                    }
                }
                prop_assert_eq!(t.len(), model.iter().flatten().count());
            }
        }
    }
}
