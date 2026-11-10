//! The lookup of the channels a frame may belong to (GEN-060, API-040).
//!
//! Every frame codec checks a frame's identifiers against the managed parameters of the
//! channels it was configured with. [`ChannelLookup`] is that question for a Spacecraft ID and
//! a Virtual Channel ID, generic over the identifier widths of the standard and over the
//! configuration a virtual channel carries; [`MapLookup`] adds the MAP of the standards that
//! have one, and [`PhysicalLookup`] the Physical Channel of those whose table holds it. A codec
//! crate names the instance it needs once, as a bound alias of its own (M6 plan WP-1a).
//!
//! A slice of lookups is a lookup (physical channels that carry several master channels).

/// Looks up the virtual channels of a Spacecraft ID (`S`) and a virtual channel identifier
/// (`V`) (GEN-060, API-040).
///
/// An implementation must answer consistently: a virtual channel that [`vc`](Self::vc) finds
/// belongs to a Spacecraft ID that [`has_spacecraft`](Self::has_spacecraft) accepts.
pub trait ChannelLookup<S, V> {
    /// What a virtual channel carries for the decoder: its configuration, or only the part of
    /// it the decoder needs.
    type Vc;

    /// Whether `scid` is a valid Spacecraft ID of the physical channel, so that a frame of it
    /// has an expected Master Channel Identifier (TM 132.0 §4.3.6.3, Table 5-1; AOS 732.0 §4.3.6.3,
    /// Table 5-1; TC 232.0 §4.4.9.3.3 b, Table 5-1; USLP 732.1 §4.3.11.3.4 b).
    fn has_spacecraft(&self, scid: S) -> bool;

    /// The virtual channel (`scid`, `vcid`), or `None` if it is not valid (TM 132.0 §5.2; AOS
    /// 732.0 §4.3.5.4, Table 5-2; TC 232.0 §4.4.7.3; USLP 732.1 §4.3.11.3.4 c).
    fn vc(&self, scid: S, vcid: V) -> Option<Self::Vc>;

    /// The one Spacecraft ID this lookup is configured for, if it has exactly one; an error
    /// for another Spacecraft ID can then name the expected value. `None` by default.
    fn sole_spacecraft(&self) -> Option<S> {
        None
    }
}

/// A [`ChannelLookup`] that also finds the MAPs (`M`) of a virtual channel (GEN-060, API-040).
pub trait MapLookup<S, V, M>: ChannelLookup<S, V> {
    /// What a MAP carries for the decoder.
    type Map;

    /// The MAP (`scid`, `vcid`, `map_id`), or `None` if it is not valid (TC 232.0 §4.4.3.3,
    /// Table 5-4; USLP 732.1 Table 5-4).
    fn map(&self, scid: S, vcid: V, map_id: M) -> Option<Self::Map>;
}

/// A lookup that holds the Physical Channel its channels belong to (GEN-060).
pub trait PhysicalLookup {
    /// The configuration of the Physical Channel.
    type Physical;

    /// The Physical Channel (TC 232.0 Table 5-1; USLP 732.1 Table 5-1).
    fn physical_channel(&self) -> Self::Physical;
}

/// A physical channel that carries several master channels: the first lookup that accepts the
/// Spacecraft ID answers (GEN-060; the clauses of each standard are on the alias of its codec crate).
///
/// The slice names no expected Spacecraft ID: it keeps the default
/// [`sole_spacecraft`](ChannelLookup::sole_spacecraft) (`None`), also for a slice of one lookup,
/// so an error for an unknown Spacecraft ID of a slice does not carry the expected value.
impl<S: Copy, V, T: ChannelLookup<S, V>> ChannelLookup<S, V> for [T] {
    type Vc = T::Vc;

    fn has_spacecraft(&self, scid: S) -> bool {
        self.iter().any(|t| t.has_spacecraft(scid))
    }

    fn vc(&self, scid: S, vcid: V) -> Option<T::Vc> {
        self.iter()
            .find(|t| t.has_spacecraft(scid))
            .and_then(|t| t.vc(scid, vcid))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::ChannelLookup;

    /// Spacecraft `scid` with the VCs 0 to `vcs - 1`, each carrying its (`scid`, `vc`).
    struct One {
        scid: u8,
        vcs: u8,
    }

    impl ChannelLookup<u8, u8> for One {
        type Vc = (u8, u8);

        fn has_spacecraft(&self, scid: u8) -> bool {
            scid == self.scid
        }

        fn vc(&self, scid: u8, vcid: u8) -> Option<(u8, u8)> {
            (self.has_spacecraft(scid) && vcid < self.vcs).then_some((scid, vcid))
        }

        fn sole_spacecraft(&self) -> Option<u8> {
            Some(self.scid)
        }
    }

    struct Many;

    impl ChannelLookup<u8, u8> for Many {
        type Vc = ();

        fn has_spacecraft(&self, _: u8) -> bool {
            true
        }

        fn vc(&self, _: u8, _: u8) -> Option<()> {
            None
        }
    }

    #[test]
    fn a_slice_answers_with_the_first_lookup_of_the_spacecraft() {
        // req: GEN-060, API-040
        let all = [One { scid: 1, vcs: 2 }, One { scid: 2, vcs: 1 }];
        let all = &all[..];
        assert!(all.has_spacecraft(1) && all.has_spacecraft(2) && !all.has_spacecraft(3));
        assert_eq!(all.vc(2, 0), Some((2, 0)));
        assert_eq!(all.vc(1, 1), Some((1, 1)));
        assert_eq!(all.vc(2, 1), None);
        assert_eq!(all.vc(3, 0), None);
        assert_eq!(all.sole_spacecraft(), None);
        assert_eq!(all[0].sole_spacecraft(), Some(1));
        assert_eq!(Many.sole_spacecraft(), None);
    }
}
