//! Fixed-capacity action lists and output-area ranges of the engine step (API-057, API-061,
//! RS-022).
//!
//! An engine returns the actions of one input as an [`Actions`] list of at most `N` entries, in
//! the order in which the procedure produces them. The octets of an action (an SDU to deliver,
//! a frame to transmit) are written into the caller's output area, and the action holds an
//! [`OutRange`] of it.

use aldis_core::{Error, ErrorKind, Result};

/// The actions of one input, in order: a list of at most `N` entries (API-057, API-083,
/// RS-022).
///
/// `N` is a compile-time bound that the engine derives and documents (COP-006). The list holds
/// no pointer into the engine and no octets (API-061).
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actions<A: Copy, const N: usize> {
    items: [Option<A>; N],
    len: usize,
}

impl<A: Copy, const N: usize> Actions<A, N> {
    /// An empty list (API-057).
    pub const fn new() -> Actions<A, N> {
        Actions {
            items: [None; N],
            len: 0,
        }
    }

    /// Appends `action` after the actions already in the list (API-057, API-083).
    ///
    /// # Errors
    /// `InternalError` if the list already holds `N` actions: the bound `N` of the engine was
    /// derived wrongly, and no action is ever dropped (API-083).
    pub fn push(&mut self, action: A) -> Result<()> {
        let Some(slot) = self.items.get_mut(self.len) else {
            return Err(Error::new(ErrorKind::InternalError));
        };
        *slot = Some(action);
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// The number of actions (API-057).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the input produced no action (API-057).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The action at `index`, counted from the first action produced (API-057).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&A> {
        if index < self.len {
            self.items.get(index).and_then(Option::as_ref)
        } else {
            None
        }
    }

    /// The actions in the order in which the procedure produces them (API-057).
    pub fn iter(&self) -> impl Iterator<Item = &A> {
        self.items.iter().take(self.len).flatten()
    }
}

impl<A: Copy, const N: usize> Default for Actions<A, N> {
    fn default() -> Actions<A, N> {
        Actions::new()
    }
}

/// A range of the caller-supplied output area: where an action's octets are (API-061).
///
/// The ranges of the actions of one input do not overlap, and stay valid until the caller
/// reuses the area (API-061).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutRange {
    start: usize,
    len: usize,
}

impl OutRange {
    /// The range of `len` octets from offset `start` (API-061).
    #[must_use]
    pub const fn new(start: usize, len: usize) -> OutRange {
        OutRange { start, len }
    }

    /// The offset of the first octet in the output area (API-061).
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// The number of octets (API-061).
    #[must_use]
    pub const fn len(self) -> usize {
        self.len
    }

    /// Whether the range holds no octet (API-061).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// The offset one past the last octet; saturates at `usize::MAX` (API-061).
    #[must_use]
    pub const fn end(self) -> usize {
        self.start.saturating_add(self.len)
    }

    /// The octets of this range in `area`, or `None` if `area` is shorter than the range
    /// (API-061).
    #[must_use]
    pub fn slice(self, area: &[u8]) -> Option<&[u8]> {
        area.get(self.start..self.end())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Actions, OutRange};
    use aldis_core::ErrorKind;

    #[test]
    fn list_keeps_order_and_refuses_overflow() {
        // req: API-057, API-083, RS-022
        let mut a = Actions::<u8, 2>::new();
        assert!(a.is_empty() && a.get(0).is_none());
        assert_eq!(a.push(7), Ok(()));
        assert_eq!(a.push(9), Ok(()));
        assert_eq!(a.push(11).unwrap_err().kind(), ErrorKind::InternalError);
        assert_eq!(a.len(), 2);
        assert!(!a.is_empty());
        let mut it = a.iter().copied();
        assert_eq!((it.next(), it.next(), it.next()), (Some(7), Some(9), None));
        assert_eq!((a.get(0), a.get(1), a.get(2)), (Some(&7), Some(&9), None));
        assert_eq!(Actions::<u8, 2>::default(), Actions::new());
    }

    #[test]
    fn range_slices_the_area() {
        // req: API-061
        let area = [1u8, 2, 3, 4, 5];
        let r = OutRange::new(1, 3);
        assert_eq!((r.start(), r.len(), r.end()), (1, 3, 4));
        assert!(!r.is_empty());
        assert_eq!(r.slice(&area), Some(&area[1..4]));
        assert_eq!(OutRange::new(3, 3).slice(&area), None);
        assert_eq!(OutRange::new(usize::MAX, 2).slice(&area), None);
        assert!(OutRange::new(2, 0).is_empty());
        assert_eq!(OutRange::new(2, 0).slice(&area), Some(&[][..]));
    }
}
