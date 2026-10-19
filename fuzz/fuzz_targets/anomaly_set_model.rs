//! `AnomalySet` against a `BTreeSet<u8>` model under arbitrary inserts and `from_flags` calls,
//! including indices that do not fit in 32 bits (GEN-043, GEN-020).
#![no_main]

// req: GEN-043, GEN-020, VER-022

use std::collections::BTreeSet;

use aldis_core::anomaly::{Anomaly, AnomalySet, CAPACITY};
use libfuzzer_sys::fuzz_target;

/// Every index below 32 is a flag except those in `HOLES`, so the iterator's skip path runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Flag(u8);

const HOLES: [u8; 2] = [7, 30];

// A deliberately unconstrained test double: implements the sealing trait directly.
impl aldis_core::anomaly::__private::Sealed for Flag {}

impl Anomaly for Flag {
    fn bit(self) -> u8 {
        self.0
    }

    fn from_bit(bit: u8) -> Option<Flag> {
        (bit < CAPACITY && !HOLES.contains(&bit)).then_some(Flag(bit))
    }
}

fuzz_target!(|data: &[u8]| {
    let mut set: AnomalySet<Flag> = AnomalySet::empty();
    let mut model: BTreeSet<u8> = BTreeSet::new();
    for &bit in data {
        set.insert(Flag(bit));
        if bit < 32 {
            model.insert(bit);
        }
        assert_eq!(set.contains(Flag(bit)), bit < 32);
        let bits = model.iter().fold(0_u32, |acc, b| acc | 1 << b);
        assert_eq!(set.bits(), bits);
        assert_eq!(set.len(), model.len());
        assert_eq!(set.is_empty(), model.is_empty());
        let listed: Vec<u8> = model
            .iter()
            .copied()
            .filter(|b| !HOLES.contains(b))
            .collect();
        assert!(set.iter().map(|f| f.0).eq(listed.iter().copied()));
        assert_eq!(set.iter().size_hint(), (0, Some(model.len())));
    }
    let flags: Vec<Flag> = data.iter().copied().map(Flag).collect();
    assert_eq!(AnomalySet::from_flags(&flags), set);
    let listed: Vec<Flag> = set.iter().collect();
    let rebuilt = AnomalySet::from_flags(&listed);
    assert_eq!(
        rebuilt.bits(),
        set.bits() & !HOLES.iter().fold(0, |acc, b| acc | 1 << b)
    );
});
