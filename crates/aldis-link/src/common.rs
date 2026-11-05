//! Types shared by the link engines: counters, occupancy, quality of service, the valid-PVN
//! set, and the idle parameters (LNK-001, LNK-002, LNK-003, LNK-014, API-056).
//!
//! The engines of M4 (the `PacketGenerator`, the `PacketExtractor`, and those of WP-4 to WP-6) are
//! defined over these types once, so that their housekeeping status and their configuration
//! constructors agree (docs/04-engines.md §1, §5).

use aldis_core::{Error, ErrorKind, Result, field};

/// A saturating counter of occurrences (LNK-002, API-056).
///
/// A counter is state, not an event (API-083): it counts what an engine did, and never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Counter(u64);

impl Counter {
    /// A counter at 0 (LNK-002).
    #[must_use]
    pub const fn new() -> Counter {
        Counter(0)
    }

    /// The count (LNK-002).
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Counts one occurrence, saturating at `u64::MAX` (LNK-002).
    pub const fn incr(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    /// Counts `n` occurrences, saturating at `u64::MAX` (LNK-002).
    pub fn add(&mut self, n: usize) {
        self.0 = self.0.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
    }
}

/// The occupancy of storage an engine retains between inputs, with its high-water mark
/// (LNK-002, API-060).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Occupancy {
    current: usize,
    high_water: usize,
}

impl Occupancy {
    /// An empty store whose high-water mark is 0 (LNK-002).
    #[must_use]
    pub const fn new() -> Occupancy {
        Occupancy {
            current: 0,
            high_water: 0,
        }
    }

    /// The octets held now (LNK-002).
    #[must_use]
    pub const fn current(self) -> usize {
        self.current
    }

    /// The most octets held at any time since the engine was built (LNK-002).
    #[must_use]
    pub const fn high_water(self) -> usize {
        self.high_water
    }

    /// Records that the store now holds `octets`, and raises the high-water mark if needed.
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003; docs/04-engines.md §1, §5.
    pub const fn set(&mut self, octets: usize) {
        self.current = octets;
        if octets > self.high_water {
            self.high_water = octets;
        }
    }
}

/// The quality of service of a transfer: sequence-controlled or expedited (232.0 §4.1.2.3.1;
/// 732.1 §4.1.2.8; LNK-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Qos {
    /// The Sequence-Controlled Service (Type-AD, FOP-1 `RequestAd`).
    #[default]
    SequenceControlled,
    /// The Expedited Service (Type-BD, FOP-1 `RequestBd`).
    Expedited,
}

/// The Packet Version Numbers a VC or MAP used for Packet Transfer accepts (LNK-010, LNK-078).
///
/// Space Packets (`000`), Encapsulation Packets (`111`), or both (132.0 §5.4 Table 5-4; 732.0
/// §5.5 Table 5-4; 732.1 §5.6 Table 5-5).
///
/// The set is never empty, because a VC in a Packets role without a valid-PVN set is a
/// configuration error (LNK-078, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValidPvns {
    space: bool,
    encapsulation: bool,
}

impl ValidPvns {
    /// Space Packets only.
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003; docs/04-engines.md §1, §5.
    pub const SPACE: ValidPvns = ValidPvns {
        space: true,
        encapsulation: false,
    };
    /// Encapsulation Packets only.
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003; docs/04-engines.md §1, §5.
    pub const ENCAPSULATION: ValidPvns = ValidPvns {
        space: false,
        encapsulation: true,
    };
    /// Space Packets and Encapsulation Packets.
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003; docs/04-engines.md §1, §5.
    pub const BOTH: ValidPvns = ValidPvns {
        space: true,
        encapsulation: true,
    };

    /// The set that holds the Space Packet PVN if `space` and the Encapsulation Packet PVN if
    /// `encapsulation`.
    ///
    /// # Errors
    /// `InvalidConfig` (`valid_pvns`) if neither is set (LNK-078).
    pub const fn new(space: bool, encapsulation: bool) -> Result<ValidPvns> {
        if space || encapsulation {
            Ok(ValidPvns {
                space,
                encapsulation,
            })
        } else {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VALID_PVNS))
        }
    }

    /// The set of the given Packet Version Numbers (`0` Space Packet, `7` Encapsulation Packet).
    ///
    /// # Errors
    /// `InvalidConfig` (`valid_pvns`) for an empty list or a version other than 0 and 7: the
    /// engines delimit only these two packets (133.0 §4.1.3.5, 133.1 §4.1.2.8).
    pub fn from_versions(versions: &[u8]) -> Result<ValidPvns> {
        let mut space = false;
        let mut encapsulation = false;
        for &v in versions {
            match v {
                0 => space = true,
                7 => encapsulation = true,
                _ => return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VALID_PVNS)),
            }
        }
        ValidPvns::new(space, encapsulation)
    }

    /// Whether Space Packets are valid.
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003; docs/04-engines.md §1, §5.
    #[must_use]
    pub const fn space(self) -> bool {
        self.space
    }

    /// Whether Encapsulation Packets are valid.
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003; docs/04-engines.md §1, §5.
    #[must_use]
    pub const fn encapsulation(self) -> bool {
        self.encapsulation
    }
}

/// What a VC offers when no field is releasable (LNK-014, OI-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IdlePolicy {
    /// The VC offers nothing, and the multiplexer sends an OID frame when no VC offers a field.
    #[default]
    Oid,
    /// The VC always has a field to offer: an idle field when no data field is releasable.
    IdleField,
}

/// The idle packet that completes a data field (LNK-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdlePacketKind {
    /// A Space Packet Idle Packet, APID 2047 (133.0 §4.1.3.3.4.4).
    SpacePacket,
    /// An Encapsulation Idle Packet, EPI `000` (133.1 §4.1.2.3).
    Encapsulation,
}

impl IdlePacketKind {
    /// Whether the PVN of the kind is in `pvns` (LNK-013).
    #[must_use]
    pub const fn is_valid_in(self, pvns: ValidPvns) -> bool {
        match self {
            IdlePacketKind::SpacePacket => pvns.space,
            IdlePacketKind::Encapsulation => pvns.encapsulation,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Counter, IdlePacketKind, Occupancy, Qos, ValidPvns};
    use aldis_core::{ErrorKind, field};

    #[test]
    fn counters_saturate_and_add() {
        // req: LNK-002
        let mut c = Counter::new();
        c.incr();
        c.add(4);
        assert_eq!(c.get(), 5);
        let mut top = Counter(u64::MAX);
        top.incr();
        top.add(3);
        assert_eq!(top.get(), u64::MAX);
        assert_eq!(Counter::default().get(), 0);
    }

    #[test]
    fn occupancy_keeps_the_high_water_mark() {
        // req: LNK-002, API-056
        let mut o = Occupancy::new();
        assert_eq!((o.current(), o.high_water()), (0, 0));
        o.set(7);
        o.set(3);
        assert_eq!((o.current(), o.high_water()), (3, 7));
        o.set(9);
        assert_eq!((o.current(), o.high_water()), (9, 9));
        assert_eq!(Qos::default(), Qos::SequenceControlled);
    }

    #[test]
    fn the_valid_pvn_set_is_never_empty() {
        // req: LNK-078, RS-020
        let e = ValidPvns::new(false, false).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::VALID_PVNS))
        );
        assert_eq!(ValidPvns::from_versions(&[0, 7]), Ok(ValidPvns::BOTH));
        assert_eq!(ValidPvns::from_versions(&[7]), Ok(ValidPvns::ENCAPSULATION));
        assert_eq!(ValidPvns::from_versions(&[0, 0]), Ok(ValidPvns::SPACE));
        assert!(ValidPvns::from_versions(&[]).is_err());
        assert!(ValidPvns::from_versions(&[1]).is_err());
        let both = ValidPvns::BOTH;
        assert!(both.space() && both.encapsulation());
        assert!(IdlePacketKind::SpacePacket.is_valid_in(ValidPvns::SPACE));
        assert!(!IdlePacketKind::SpacePacket.is_valid_in(ValidPvns::ENCAPSULATION));
        assert!(IdlePacketKind::Encapsulation.is_valid_in(ValidPvns::ENCAPSULATION));
        assert!(!IdlePacketKind::Encapsulation.is_valid_in(ValidPvns::SPACE));
    }
}
