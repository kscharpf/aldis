//! Range-checked identifier types (API-001, API-002, RS-010).
//!
//! Identifiers are constructed only through validating functions, so an encoder can never
//! emit an out-of-range field. Widths that differ between protocols are type parameters
//! (API-002): for example `SpacecraftId<10>` (TM, TC, AOS Issue 5), `SpacecraftId<8>`
//! (AOS Issue ≤4), and `SpacecraftId<16>` (USLP).

use crate::counter::{Continuity, ModCounter, classify_masked, from_valid};
use crate::error::{Error, Result, u8_to_u64, u16_to_u64};
use crate::field;

/// Defines an unsigned identifier whose valid values are `0..=2^BITS - 1`.
macro_rules! width_id {
    ($(#[$doc:meta])* $name:ident, $field:expr, $repr:ty, $max_bits:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name<const BITS: u32>($repr);

        impl<const BITS: u32> $name<BITS> {
            const VALID_WIDTH: () = assert!(BITS >= 1 && BITS <= $max_bits, "unsupported identifier width");

            /// Largest valid value, `2^BITS - 1`.
            pub const MAX: $repr = {
                let () = Self::VALID_WIDTH;
                <$repr>::MAX >> ($max_bits - BITS)
            };

            /// The identifier, or `None` if `value` needs more than `BITS` bits.
            #[must_use]
            pub const fn new(value: $repr) -> Option<Self> {
                if value <= Self::MAX { Some(Self(value)) } else { None }
            }

            /// The numeric value.
            #[must_use]
            pub const fn get(self) -> $repr {
                self.0
            }
        }

        impl<const BITS: u32> TryFrom<$repr> for $name<BITS> {
            type Error = Error;

            fn try_from(value: $repr) -> Result<Self> {
                Self::new(value).ok_or_else(|| {
                    Error::out_of_range($field, u64::from(Self::MAX), u64::from(value))
                })
            }
        }

        impl<const BITS: u32> From<$name<BITS>> for $repr {
            fn from(id: $name<BITS>) -> $repr {
                id.0
            }
        }
    };
}

width_id!(
    /// Spacecraft Identifier of `BITS` width (132.0 §4.1.2.2.3, 232.0 §4.1.2.5, 732.0 §4.1.2.2.3,
    /// 732.1 §4.1.2.2.3).
    SpacecraftId, field::SPACECRAFT_ID, u16, 16
);
width_id!(
    /// Virtual Channel Identifier of `BITS` width: 3 (TM) or 6 (TC, AOS, USLP).
    VcId, field::VC_ID, u8, 8
);
width_id!(
    /// Multiplexer Access Point Identifier of `BITS` width: 6 (TC Segment Header) or 4 (USLP).
    MapId, field::MAP_ID, u8, 8
);

/// Application Process Identifier, 0–2047 (133.0 §4.1.3.3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Apid(u16);

impl Apid {
    /// Largest APID.
    pub const MAX: u16 = 0x07FF;
    /// APID reserved for Idle Packets: all ones (133.0 §4.1.3.3.4.4).
    pub const IDLE: Apid = Apid(Apid::MAX);

    /// The APID, or `None` if `value` exceeds 11 bits.
    #[must_use]
    pub const fn new(value: u16) -> Option<Apid> {
        if value <= Apid::MAX {
            Some(Apid(value))
        } else {
            None
        }
    }

    /// The numeric value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Whether this is the Idle Packet APID.
    #[must_use]
    pub const fn is_idle(self) -> bool {
        self.0 == Apid::MAX
    }
}

impl TryFrom<u16> for Apid {
    type Error = Error;

    fn try_from(value: u16) -> Result<Apid> {
        Apid::new(value)
            .ok_or_else(|| Error::out_of_range(field::APID, u64::from(Apid::MAX), u64::from(value)))
    }
}

impl From<Apid> for u16 {
    fn from(id: Apid) -> u16 {
        id.0
    }
}

/// Space Packet Sequence Count or Packet Name, 0–16383, wrapping (133.0 §4.1.3.4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SeqCount(u16);

impl SeqCount {
    /// Largest value.
    pub const MAX: u16 = 0x3FFF;

    /// The count, or `None` if `value` exceeds 14 bits.
    #[must_use]
    pub const fn new(value: u16) -> Option<SeqCount> {
        if value <= SeqCount::MAX {
            Some(SeqCount(value))
        } else {
            None
        }
    }

    /// The numeric value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The next count, modulo 16384 (133.0 §4.1.3.4.3.4).
    #[must_use]
    pub const fn next(self) -> SeqCount {
        SeqCount(self.0.wrapping_add(1) & SeqCount::MAX)
    }

    /// Classifies `next` as received after `self` modulo 16384 (133.0 §4.1.3.4.3, CMN-031).
    #[must_use]
    pub const fn classify(self, next: SeqCount) -> Continuity {
        classify_masked(14, u16_to_u64(self.0), u16_to_u64(next.0))
    }
}

impl TryFrom<u16> for SeqCount {
    type Error = Error;

    fn try_from(value: u16) -> Result<SeqCount> {
        SeqCount::new(value).ok_or_else(|| {
            Error::out_of_range(
                field::SEQUENCE_COUNT,
                u64::from(SeqCount::MAX),
                u64::from(value),
            )
        })
    }
}

impl From<SeqCount> for u16 {
    fn from(count: SeqCount) -> u16 {
        count.0
    }
}

impl From<SeqCount> for ModCounter {
    fn from(count: SeqCount) -> ModCounter {
        from_valid(14, u64::from(count.0))
    }
}

/// TC Frame Sequence Number N(S), and COP-1 V(S) / V(R): 0–255, wrapping (232.0 §4.1.2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameSeqNum(u8);

impl FrameSeqNum {
    /// Largest value.
    pub const MAX: u8 = u8::MAX;

    /// The sequence number (`0..=255` all fit in 8 bits).
    #[must_use]
    pub const fn new(value: u8) -> FrameSeqNum {
        FrameSeqNum(value)
    }

    /// The numeric value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The next number, modulo 256.
    #[must_use]
    pub const fn next(self) -> FrameSeqNum {
        FrameSeqNum(self.0.wrapping_add(1))
    }

    /// Classifies `next` as received after `self` modulo 256 (CMN-031).
    #[must_use]
    pub const fn classify(self, next: FrameSeqNum) -> Continuity {
        classify_masked(8, u8_to_u64(self.0), u8_to_u64(next.0))
    }
}

impl From<u8> for FrameSeqNum {
    fn from(value: u8) -> FrameSeqNum {
        FrameSeqNum(value)
    }
}

impl From<FrameSeqNum> for u8 {
    fn from(seq: FrameSeqNum) -> u8 {
        seq.0
    }
}

impl From<FrameSeqNum> for ModCounter {
    fn from(seq: FrameSeqNum) -> ModCounter {
        from_valid(8, u64::from(seq.0))
    }
}

/// Packet Version Number, 0–7 (SANA Packet Version Number registry; 133.0 §4.1.3.2,
/// 133.1 §4.1.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PacketVersion(u8);

impl PacketVersion {
    /// Largest value.
    pub const MAX: u8 = 7;
    /// Version-1 CCSDS Packet (Space Packet), binary `000`.
    pub const SPACE_PACKET: PacketVersion = PacketVersion(0);
    /// Version-8 CCSDS Packet (Encapsulation Packet), binary `111`.
    pub const ENCAPSULATION: PacketVersion = PacketVersion(7);

    /// The version, or `None` if `value` exceeds 3 bits.
    #[must_use]
    pub const fn new(value: u8) -> Option<PacketVersion> {
        if value <= PacketVersion::MAX {
            Some(PacketVersion(value))
        } else {
            None
        }
    }

    /// The numeric value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for PacketVersion {
    type Error = Error;

    fn try_from(value: u8) -> Result<PacketVersion> {
        PacketVersion::new(value).ok_or_else(|| {
            Error::out_of_range(
                field::PACKET_VERSION,
                u64::from(PacketVersion::MAX),
                u64::from(value),
            )
        })
    }
}

impl From<PacketVersion> for u8 {
    fn from(version: PacketVersion) -> u8 {
        version.0
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Apid, FrameSeqNum, MapId, PacketVersion, SeqCount, SpacecraftId, VcId};
    use crate::counter::{Continuity, ModCounter};
    use crate::error::ErrorKind;
    use crate::field;

    #[test]
    fn widths_are_enforced() {
        // req: API-001, API-002, RS-010
        assert_eq!(SpacecraftId::<10>::MAX, 1023);
        assert_eq!(SpacecraftId::<8>::MAX, 255);
        assert_eq!(SpacecraftId::<16>::MAX, u16::MAX);
        assert!(SpacecraftId::<10>::new(1023).is_some());
        assert!(SpacecraftId::<10>::new(1024).is_none());
        assert!(SpacecraftId::<16>::new(u16::MAX).is_some());
        assert_eq!(VcId::<3>::MAX, 7);
        assert!(VcId::<3>::new(8).is_none());
        assert_eq!(VcId::<6>::MAX, 63);
        assert_eq!(MapId::<4>::MAX, 15);
        assert!(MapId::<6>::new(63).is_some());
        assert!(MapId::<6>::new(64).is_none());
        let e = SpacecraftId::<10>::try_from(2000).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        assert_eq!(e.field(), Some(field::SPACECRAFT_ID));
        assert_eq!(
            VcId::<3>::try_from(8).unwrap_err().field(),
            Some(field::VC_ID)
        );
        assert_eq!(
            MapId::<4>::try_from(16).unwrap_err().field(),
            Some(field::MAP_ID)
        );
        assert_eq!(u16::from(SpacecraftId::<10>::try_from(42).unwrap()), 42);
        assert_eq!(VcId::<6>::new(5).map(VcId::get), Some(5));
        assert_eq!(u8::from(VcId::<6>::try_from(5).unwrap()), 5);
        assert_eq!(MapId::<4>::new(3).map(MapId::get), Some(3));
        assert_eq!(u8::from(MapId::<4>::try_from(3).unwrap()), 3);
    }

    #[test]
    fn packet_identifiers() {
        // req: API-001, CMN-031
        const SEQ_CONT: Continuity = SeqCount::new(SeqCount::MAX)
            .unwrap()
            .classify(SeqCount::new(0).unwrap());
        const FSN_CONT: Continuity =
            FrameSeqNum::new(FrameSeqNum::MAX).classify(FrameSeqNum::new(0));
        assert_eq!(SEQ_CONT, Continuity::InSequence);
        assert_eq!(FSN_CONT, Continuity::InSequence);
        assert!(Apid::new(2047).is_some_and(Apid::is_idle));
        assert!(!Apid::new(0).unwrap().is_idle());
        assert!(Apid::new(2048).is_none());
        assert_eq!(u16::from(Apid::IDLE), 2047);
        assert_eq!(
            Apid::try_from(4096).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(Apid::try_from(7).map(Apid::get), Ok(7));
        let s_max = SeqCount::new(SeqCount::MAX).unwrap();
        let s0 = SeqCount::new(0).unwrap();
        assert_eq!(s_max.next(), s0);
        assert_eq!(s_max.classify(s0), Continuity::InSequence);
        assert_eq!(ModCounter::from(s_max).value(), u64::from(SeqCount::MAX));
        assert_eq!(u16::from(SeqCount::new(5).unwrap().next()), 6);
        assert!(SeqCount::new(16384).is_none());
        assert_eq!(
            SeqCount::try_from(20000).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(SeqCount::try_from(1).map(SeqCount::get), Ok(1));
        assert_eq!(s_max.get(), SeqCount::MAX);
        let f_max = FrameSeqNum::new(FrameSeqNum::MAX);
        assert_eq!(f_max.get(), 255);
        assert_eq!(f_max.next(), FrameSeqNum::from(0));
        assert_eq!(u8::from(f_max.next()), 0);
        assert_eq!(FrameSeqNum::from(200).get(), 200);
        assert_eq!(u8::from(f_max), 255);
        assert_eq!(f_max.classify(FrameSeqNum::new(0)), Continuity::InSequence);
        assert_eq!(ModCounter::from(f_max).value(), 255);
        assert_eq!(PacketVersion::SPACE_PACKET.get(), 0);
        assert_eq!(PacketVersion::ENCAPSULATION.get(), 7);
        assert_eq!(u8::from(PacketVersion::ENCAPSULATION), 7);
        assert!(PacketVersion::new(8).is_none());
        assert_eq!(
            PacketVersion::try_from(9).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(PacketVersion::try_from(7), Ok(PacketVersion::ENCAPSULATION));
    }
}
