//! Types shared by the link engines: counters, occupancy, quality of service, the valid-PVN
//! set, and the idle parameters (LNK-001, LNK-002, LNK-003, LNK-014, API-056).
//!
//! The engines of M4 (the `PacketGenerator`, the `PacketExtractor`, and those of WP-4 to WP-6) are
//! defined over these types once, so that their housekeeping status and their configuration
//! constructors agree (docs/04-engines.md §1, §5).

#[cfg(any(feature = "spp", feature = "epp"))]
use aldis_core::{Error, ErrorKind, Result, field};

/// What a MAP carries (232.0 §4.4, §5.3 Table 5-3; 732.1 §4.3.2-4, §5.4 Table 5-4): the kind both
/// MAP engines serve (LNK-040…LNK-043, LNK-070…LNK-073; OI-96).
///
/// What only one engine has stays beside this enum in that engine: the `MapSegmenter`'s
/// [`SegmenterService`](crate::segmenter::SegmenterService) (a TC MAP that carries either, and the
/// release deadline of a stream), so that neither engine can hold a state it does not support
/// (RS-020, RS-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapService {
    /// Packets: a segmented packet is delivered once, whole packets are split by their length
    /// fields (LNK-071).
    Packets,
    /// `MAP_SDUs` or `MAPA_SDUs` (and the `VCA_SDU` of a TC VC without a Segment Header).
    Sdu,
    /// An octet stream (USLP rule `011`), delivered as it arrives (LNK-072).
    OctetStream,
}

/// A saturating counter of occurrences (LNK-002, API-056).
///
/// A counter is state, not an event (API-083): it counts what an engine did, and never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Counter(u64);

impl Counter {
    /// A counter at 0 (LNK-002).
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
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
    pub const fn new() -> Self {
        Self {
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
/// configuration error (LNK-078, RS-020). It holds only the PVNs of the packet codecs compiled
/// in: Space Packets with the feature `spp`, Encapsulation Packets with `epp` (GEN-009), and
/// the type exists with either (a build with neither has no packet engine and no valid PVN).
#[cfg(any(feature = "spp", feature = "epp"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValidPvns {
    space: bool,
    encapsulation: bool,
}

#[cfg(any(feature = "spp", feature = "epp"))]
impl ValidPvns {
    /// Space Packets only (feature `spp`).
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003, GEN-009; docs/04-engines.md §1, §5.
    #[cfg(feature = "spp")]
    pub const SPACE: Self = Self {
        space: true,
        encapsulation: false,
    };
    /// Encapsulation Packets only (feature `epp`).
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003, GEN-009; docs/04-engines.md §1, §5.
    #[cfg(feature = "epp")]
    pub const ENCAPSULATION: Self = Self {
        space: false,
        encapsulation: true,
    };
    /// Space Packets and Encapsulation Packets (features `spp` and `epp`).
    ///
    /// Requirements: LNK-001, LNK-002, LNK-003, GEN-009; docs/04-engines.md §1, §5.
    #[cfg(all(feature = "spp", feature = "epp"))]
    pub const BOTH: Self = Self {
        space: true,
        encapsulation: true,
    };

    /// The set that holds the Space Packet PVN if `space` and the Encapsulation Packet PVN if
    /// `encapsulation`.
    ///
    /// # Errors
    /// `InvalidConfig` (`valid_pvns`) if neither is set, or if one that is set has its codec
    /// not compiled in: Space Packets need the feature `spp`, Encapsulation Packets `epp`
    /// (LNK-078, GEN-009).
    pub const fn new(space: bool, encapsulation: bool) -> Result<Self> {
        let compiled =
            (!space || cfg!(feature = "spp")) && (!encapsulation || cfg!(feature = "epp"));
        if (space || encapsulation) && compiled {
            Ok(Self {
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
    /// `InvalidConfig` (`valid_pvns`) for an empty list, a version other than 0 and 7 (the
    /// engines delimit only these two packets, 133.0 §4.1.3.5, 133.1 §4.1.2.8), or the version
    /// of a codec not compiled in (LNK-078, GEN-009).
    pub fn from_versions(versions: &[u8]) -> Result<Self> {
        let mut space = false;
        let mut encapsulation = false;
        for &v in versions {
            match v {
                0 => space = true,
                7 => encapsulation = true,
                _ => return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VALID_PVNS)),
            }
        }
        Self::new(space, encapsulation)
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
///
/// Both variants exist in every build that has a packet codec, because the enum is closed
/// (GEN-081); a configuration that names the kind of a codec not compiled in is rejected with
/// `InvalidConfig` (`idle_packet_kind`) when the engine is configured (LNK-013, LNK-078,
/// GEN-009).
#[cfg(any(feature = "spp", feature = "epp"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdlePacketKind {
    /// A Space Packet Idle Packet, APID 2047 (133.0 §4.1.3.3.4.4).
    SpacePacket,
    /// An Encapsulation Idle Packet, EPI `000` (133.1 §4.1.2.3).
    Encapsulation,
}

#[cfg(any(feature = "spp", feature = "epp"))]
impl IdlePacketKind {
    /// Whether the PVN of the kind is in `pvns` (LNK-013). A set holds only PVNs of compiled
    /// codecs, so a kind whose codec is not compiled in is in none (GEN-009).
    #[must_use]
    pub const fn is_valid_in(self, pvns: ValidPvns) -> bool {
        match self {
            Self::SpacePacket => pvns.space,
            Self::Encapsulation => pvns.encapsulation,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Counter, Occupancy, Qos};
    #[cfg(any(feature = "spp", feature = "epp"))]
    use super::{IdlePacketKind, ValidPvns};
    #[cfg(any(feature = "spp", feature = "epp"))]
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

    #[cfg(any(feature = "spp", feature = "epp"))]
    fn assert_invalid_pvns(e: aldis_core::Error) {
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::VALID_PVNS))
        );
    }

    #[cfg(all(feature = "spp", feature = "epp"))]
    #[test]
    fn the_valid_pvn_set_is_never_empty() {
        // req: LNK-078, RS-020
        assert_invalid_pvns(ValidPvns::new(false, false).unwrap_err());
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

    #[cfg(all(feature = "spp", not(feature = "epp")))]
    #[test]
    fn a_build_with_spp_alone_has_only_the_space_packet_pvn() {
        // req: LNK-078, GEN-009
        assert_invalid_pvns(ValidPvns::new(false, true).unwrap_err());
        assert_invalid_pvns(ValidPvns::new(true, true).unwrap_err());
        assert_invalid_pvns(ValidPvns::from_versions(&[7]).unwrap_err());
        assert_invalid_pvns(ValidPvns::from_versions(&[0, 7]).unwrap_err());
        assert_eq!(ValidPvns::from_versions(&[0]), Ok(ValidPvns::SPACE));
        assert!(IdlePacketKind::SpacePacket.is_valid_in(ValidPvns::SPACE));
        assert!(!IdlePacketKind::Encapsulation.is_valid_in(ValidPvns::SPACE));
    }

    #[cfg(all(feature = "epp", not(feature = "spp")))]
    #[test]
    fn a_build_with_epp_alone_has_only_the_encapsulation_pvn() {
        // req: LNK-078, GEN-009
        assert_invalid_pvns(ValidPvns::new(true, false).unwrap_err());
        assert_invalid_pvns(ValidPvns::new(true, true).unwrap_err());
        assert_invalid_pvns(ValidPvns::from_versions(&[0]).unwrap_err());
        assert_invalid_pvns(ValidPvns::from_versions(&[0, 7]).unwrap_err());
        assert_eq!(ValidPvns::from_versions(&[7]), Ok(ValidPvns::ENCAPSULATION));
        assert!(IdlePacketKind::Encapsulation.is_valid_in(ValidPvns::ENCAPSULATION));
        assert!(!IdlePacketKind::SpacePacket.is_valid_in(ValidPvns::ENCAPSULATION));
    }
}
