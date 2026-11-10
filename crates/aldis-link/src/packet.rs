//! Delimiting packets by their length fields (LNK-010, LNK-060, LNK-063).
//!
//! The engines handle Space Packets (PVN `000`, 133.0 §4.1.3) and Encapsulation Packets (PVN
//! `111`, 133.1 §4.1.2); the length of either comes from the length peek of its crate.

use aldis_core::ErrorKind;
use aldis_core::ids::Apid;
use aldis_epp::{EncapsulationPacket, Epi};
use aldis_spp::SpacePacket;

/// The two kinds of packet the engines delimit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A Space Packet, PVN `000`.
    Space,
    /// An Encapsulation Packet, PVN `111`.
    Encapsulation,
}

/// What the octets of a packet collected so far say about its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Length {
    /// The packet is this many octets in all.
    Total(usize),
    /// The length field is not complete yet.
    More,
    /// The octets are no valid header (a length shorter than the header, a reserved value).
    Invalid,
}

/// The fields of the first octet of a packet by bit position (133.0 §4.1.3.2, 133.1 §4.1.2.2,
/// §4.1.2.3; GEN-051).
mod first_octet {
    use aldis_core::bits::Field;

    pub(super) type Pvn = Field<1, 0, 3>;
    pub(super) type Epi = Field<1, 3, 3>;
}

/// The first two octets of a Space Packet by bit position (133.0 §4.1.3.3.4; GEN-051).
mod space_header {
    use aldis_core::bits::Field;

    pub(super) type Apid = Field<2, 5, 11>;
}

/// The kind of packet whose first octet is `first` (the PVN is its top three bits, 133.0
/// §4.1.3.2, 133.1 §4.1.2.2), or `None` for a PVN the engines do not delimit.
pub(crate) const fn kind_of(first: u8) -> Option<Kind> {
    match first_octet::Pvn::get_u8(&[first]) {
        0b000 => Some(Kind::Space),
        0b111 => Some(Kind::Encapsulation),
        _ => None,
    }
}

/// The PVN in the first octet.
pub(crate) const fn pvn(first: u8) -> u8 {
    first_octet::Pvn::get_u8(&[first])
}

/// The length of the packet that starts `prefix` (not empty) of kind `kind`.
pub(crate) fn length(kind: Kind, prefix: &[u8]) -> Length {
    let peeked = match kind {
        Kind::Space => SpacePacket::peek_length(prefix),
        Kind::Encapsulation => EncapsulationPacket::peek_length(prefix),
    };
    match peeked {
        Ok(total) => Length::Total(total),
        Err(e) if e.kind() == ErrorKind::Incomplete => Length::More,
        Err(_) => Length::Invalid,
    }
}

/// Whether the packet that starts `prefix` is an idle packet: a Space Packet with APID 2047
/// (133.0 §4.1.3.3.4.4) or an Encapsulation Packet with EPI `000` (133.1 §4.1.2.3). A Space
/// Packet needs its first two octets to tell.
pub(crate) fn is_idle(kind: Kind, prefix: &[u8]) -> bool {
    match (kind, prefix) {
        (Kind::Space, [b0, b1, ..]) => {
            Apid::new(space_header::Apid::get_u16(&[*b0, *b1])).is_some_and(Apid::is_idle)
        }
        (Kind::Encapsulation, [b0, ..]) => {
            Epi::new(first_octet::Epi::get_u8(&[*b0])).is_some_and(Epi::is_idle)
        }
        _ => false,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Kind, Length, is_idle, kind_of, length, pvn};

    #[test]
    fn kinds_lengths_and_idle() {
        // req: LNK-010, LNK-063
        assert_eq!(kind_of(0x00), Some(Kind::Space));
        assert_eq!(kind_of(0xE0), Some(Kind::Encapsulation));
        assert_eq!(kind_of(0x20), None);
        assert_eq!((pvn(0x00), pvn(0xFD), pvn(0x20)), (0, 7, 1));
        let space = [0x00, 0x01, 0xC0, 0x00, 0x00, 0x02, 1, 2, 3];
        assert_eq!(length(Kind::Space, &space), Length::Total(9));
        assert_eq!(length(Kind::Space, &space[..5]), Length::More);
        assert_eq!(length(Kind::Encapsulation, &[0xE0]), Length::Total(1));
        assert_eq!(length(Kind::Encapsulation, &[0xE1]), Length::More);
        assert_eq!(length(Kind::Encapsulation, &[0xE1, 0x05]), Length::Total(5));
        assert_eq!(length(Kind::Encapsulation, &[0xE1, 0x01]), Length::Invalid);
        assert!(is_idle(Kind::Space, &[0x07, 0xFF, 0xC0]));
        assert!(!is_idle(Kind::Space, &[0x07]));
        assert!(!is_idle(Kind::Space, &space));
        assert!(is_idle(Kind::Encapsulation, &[0xE0]));
        assert!(!is_idle(Kind::Encapsulation, &[0xFD]));
        assert!(!is_idle(Kind::Encapsulation, &[]));
    }
}
