//! Multiplexing policies of the `ChannelMux` (LNK-031; docs/04-engines.md §2.4).
//!
//! CCSDS gives no algorithm for choosing the Virtual Channel of the next frame (132.0 §4.2.4.3;
//! 732.0 §4.2.5.3; 732.1 §4.2.8.3), so the library provides three: fixed priority, round robin,
//! and weighted round robin. Each is a pure function of the policy state and of which VCs have a
//! field to offer (GEN-030). A physical channel has one master channel, so there is no MC
//! selection (OI-75).
//!
//! The VCs are walked by their position in ascending VCID order, which the configuration of the
//! mux fixes (LNK-031).

use aldis_core::{Error, ErrorKind, Result, field};

/// The policy that chooses the Virtual Channel of the next frame (LNK-031; field `mux_policy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MuxPolicy {
    /// The offered VC with the highest `vc_priority` (a higher value is served first, not a rank
    /// in which 1 is served first; see `VcConfig::with_priority`); priorities are unique.
    #[default]
    FixedPriority,
    /// The next offered VC after the one served last, in ascending VCID order, wrapping.
    RoundRobin,
    /// Each VC gets up to `vc_weight` consecutive selections per cycle while it has a field. A
    /// VC with none forfeits the rest of its turn, and so does the VC in mid-turn when an
    /// Opportunity finds no VC with a field and the frame is an OID frame.
    WeightedRoundRobin,
}

/// The weight of a VC under weighted round robin, at least 1 (LNK-031; field `vc_weight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Weight(u16);

impl Weight {
    /// A weight of 1: one selection per cycle, the weight of round robin.
    ///
    /// Requirements: LNK-031; docs/04-engines.md §2.4.
    pub const ONE: Weight = Weight(1);

    /// The weight `n`.
    ///
    /// # Errors
    /// `InvalidConfig` (`vc_weight`) for 0: a VC with no selection per cycle is never served.
    ///
    /// Requirements: LNK-031; docs/04-engines.md §2.4.
    pub const fn new(n: u16) -> Result<Weight> {
        if n == 0 {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_WEIGHT))
        } else {
            Ok(Weight(n))
        }
    }

    /// The number of consecutive selections per cycle.
    ///
    /// Requirements: LNK-031; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// The state of round robin and weighted round robin: the position served last and the
/// selections left in its turn (LNK-031).
///
/// Fixed priority has no state. A reconfiguration of the mux starts the walk afresh
/// ([`Selector::new`]), because positions refer to the old configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selector {
    last: Option<usize>,
    left: u16,
}

impl Selector {
    /// The state at start-up: the first selection takes the first offered VC, with its full
    /// weight.
    ///
    /// Requirements: LNK-031; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn new() -> Selector {
        Selector {
            last: None,
            left: 0,
        }
    }

    /// The position of the VC for the next frame among `count` VCs, or `None` if none is
    /// offered (LNK-031).
    ///
    /// `offered(i)` says whether the VC at position `i` has a field, `priority(i)` and
    /// `weight(i)` are its parameters. A selection updates the state; `None` leaves it as it
    /// was (the caller then sends an OID frame and calls [`forfeit`](Self::forfeit)).
    pub fn select(
        &mut self,
        policy: MuxPolicy,
        count: usize,
        offered: impl Fn(usize) -> bool,
        priority: impl Fn(usize) -> u8,
        weight: impl Fn(usize) -> Weight,
    ) -> Option<usize> {
        match policy {
            MuxPolicy::FixedPriority => {
                let mut best: Option<(usize, u8)> = None;
                for i in 0..count {
                    let p = priority(i);
                    if offered(i) && best.is_none_or(|(_, b)| p > b) {
                        best = Some((i, p));
                    }
                }
                best.map(|(i, _)| i)
            }
            MuxPolicy::RoundRobin => self.walk(count, &offered, |_| Weight::ONE),
            MuxPolicy::WeightedRoundRobin => self.walk(count, &offered, weight),
        }
    }

    /// The walk of round robin: the VC in mid-turn again while it has a field, else the next
    /// offered VC after the last one, wrapping.
    fn walk(
        &mut self,
        count: usize,
        offered: &impl Fn(usize) -> bool,
        weight: impl Fn(usize) -> Weight,
    ) -> Option<usize> {
        if let Some(last) = self.last
            && self.left > 0
            && offered(last)
        {
            self.left = self.left.saturating_sub(1);
            return Some(last);
        }
        let start = self.last.map_or(0, |last| last.saturating_add(1));
        for i in (start..count).chain(0..start.min(count)) {
            if offered(i) {
                self.last = Some(i);
                self.left = weight(i).get().saturating_sub(1);
                return Some(i);
            }
        }
        None
    }

    /// Ends the turn of the VC in mid-turn: the frame was an OID frame, so no VC had a field
    /// (LNK-031, OI-74). The next selection starts after the position served last.
    pub const fn forfeit(&mut self) {
        self.left = 0;
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{MuxPolicy, Selector, Weight};
    use aldis_core::{ErrorKind, field};

    fn one(_: usize) -> Weight {
        Weight::ONE
    }

    #[test]
    fn weight_is_at_least_one() {
        // req: LNK-031, LNK-078
        let e = Weight::new(0).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::VC_WEIGHT))
        );
        assert_eq!(Weight::new(3).unwrap().get(), 3);
    }

    #[test]
    fn fixed_priority_takes_the_highest_offered() {
        // req: LNK-031
        let mut s = Selector::new();
        let prio = |i: usize| [1_u8, 3, 2, 0][i];
        let offered = |i: usize| i != 1;
        assert_eq!(
            s.select(MuxPolicy::FixedPriority, 4, offered, prio, one),
            Some(2)
        );
        assert_eq!(
            s.select(MuxPolicy::FixedPriority, 4, |_| false, prio, one),
            None
        );
        // Stateless: the same offer gives the same VC.
        assert_eq!(s, Selector::new());
    }

    #[test]
    fn round_robin_walks_in_order_and_wraps() {
        // req: LNK-031
        let mut s = Selector::new();
        let all = |_: usize| true;
        let mut got = [0_usize; 5];
        for g in &mut got {
            *g = s.select(MuxPolicy::RoundRobin, 3, all, |_| 0, one).unwrap();
        }
        assert_eq!(got, [0, 1, 2, 0, 1]);
        let mut s = Selector::new();
        let only_two = |i: usize| i == 2;
        assert_eq!(
            s.select(MuxPolicy::RoundRobin, 3, only_two, |_| 0, one),
            Some(2)
        );
        assert_eq!(s.select(MuxPolicy::RoundRobin, 0, all, |_| 0, one), None);
    }

    #[test]
    fn weighted_gives_each_vc_its_weight_and_forfeits() {
        // req: LNK-031
        let weights = |i: usize| Weight::new([2_u16, 1, 3][i]).unwrap();
        let mut s = Selector::new();
        let all = |_: usize| true;
        let mut got = [0_usize; 8];
        for g in &mut got {
            *g = s
                .select(MuxPolicy::WeightedRoundRobin, 3, all, |_| 0, weights)
                .unwrap();
        }
        assert_eq!(got, [0, 0, 1, 2, 2, 2, 0, 0]);
        // A VC in mid-turn that loses its field forfeits the rest of the turn.
        let mut s = Selector::new();
        assert_eq!(
            s.select(MuxPolicy::WeightedRoundRobin, 3, all, |_| 0, weights),
            Some(0)
        );
        let not_zero = |i: usize| i != 0;
        assert_eq!(
            s.select(MuxPolicy::WeightedRoundRobin, 3, not_zero, |_| 0, weights),
            Some(1)
        );
        // An OID frame ends the turn of the VC in mid-turn.
        let mut s = Selector::new();
        assert_eq!(
            s.select(MuxPolicy::WeightedRoundRobin, 3, all, |_| 0, weights),
            Some(0)
        );
        s.forfeit();
        assert_eq!(
            s.select(MuxPolicy::WeightedRoundRobin, 3, all, |_| 0, weights),
            Some(1)
        );
    }
}
