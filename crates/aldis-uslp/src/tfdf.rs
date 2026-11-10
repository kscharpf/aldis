//! Transfer Frame Data Field Header: TFDZ Construction Rule, UPID, and First Header/Last Valid
//! Octet Pointer (732.1 §4.1.4.2; USLP-005, USLP-006, USLP-016, OI-05).
//!
//! The pointer is present exactly when the rule is `000`, `001`, or `010` (USLP-005), which
//! are the rules of a fixed-length TFDZ (USLP-016); [`TfdzRule`] carries the pointer in those
//! variants only, so a header without a pointer for a rule that needs one cannot be built
//! (RS-020).

use aldis_core::{Error, ErrorKind};

use crate::field;
use crate::header::{invalid, len_u64};
use crate::upid::Upid;

/// The pointer value that means "none in this TFDZ": no packet starts or ends (rule `000`),
/// or the SDU continues (rules `001`, `010`) (732.1 §4.1.4.2.4.4, §4.1.4.2.4.6).
pub const POINTER_NONE: u16 = 0xFFFF;

/// The TFDZ Construction Rule, the 3-bit field of 732.1 §4.1.4.2.2 (Table 4-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConstructionRule {
    /// `000`: Packets spanning multiple frames, fixed-length TFDZ (§4.1.4.2.2.2.1).
    PacketsSpanningFrames,
    /// `001`: start of a `MAPA_SDU` or `VCA_SDU`, fixed-length TFDZ (§4.1.4.2.2.2.2).
    SduStart,
    /// `010`: continuing portion of a `MAPA_SDU` or `VCA_SDU`, fixed-length TFDZ (§4.1.4.2.2.2.3).
    SduContinuation,
    /// `011`: octet stream, variable-length TFDZ (§4.1.4.2.2.2.4).
    OctetStream,
    /// `100`: starting segment, variable-length TFDZ (§4.1.4.2.2.2.5).
    StartingSegment,
    /// `101`: continuing segment, variable-length TFDZ (§4.1.4.2.2.2.6).
    ContinuingSegment,
    /// `110`: last segment, variable-length TFDZ (§4.1.4.2.2.2.7).
    LastSegment,
    /// `111`: no segmentation, variable-length TFDZ (§4.1.4.2.2.2.8).
    NoSegmentation,
}

impl ConstructionRule {
    /// The rule in the low three bits of `bits`; the other bits are ignored (Table 4-3).
    #[must_use]
    pub const fn from_bits(bits: u8) -> ConstructionRule {
        match bits & 0b111 {
            0b000 => ConstructionRule::PacketsSpanningFrames,
            0b001 => ConstructionRule::SduStart,
            0b010 => ConstructionRule::SduContinuation,
            0b011 => ConstructionRule::OctetStream,
            0b100 => ConstructionRule::StartingSegment,
            0b101 => ConstructionRule::ContinuingSegment,
            0b110 => ConstructionRule::LastSegment,
            _ => ConstructionRule::NoSegmentation,
        }
    }

    /// The 3-bit field value (Table 4-3).
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            ConstructionRule::PacketsSpanningFrames => 0b000,
            ConstructionRule::SduStart => 0b001,
            ConstructionRule::SduContinuation => 0b010,
            ConstructionRule::OctetStream => 0b011,
            ConstructionRule::StartingSegment => 0b100,
            ConstructionRule::ContinuingSegment => 0b101,
            ConstructionRule::LastSegment => 0b110,
            ConstructionRule::NoSegmentation => 0b111,
        }
    }

    /// Whether the rule is one of a fixed-length TFDZ, `000`–`010`; the others are those of a
    /// variable-length TFDZ (§4.1.4.2.2.1.1, Table 4-3; USLP-016).
    #[must_use]
    pub const fn is_fixed_length(self) -> bool {
        self.bits() <= 0b010
    }

    /// Whether the TFDF Header carries the pointer: exactly for the fixed-length rules
    /// (§4.1.4.2.4.1; USLP-005).
    #[must_use]
    pub const fn has_pointer(self) -> bool {
        self.is_fixed_length()
    }
}

/// An octet offset in a TFDZ that a 16-bit pointer can hold: 0 to `0xFFFE`, every value below
/// [`POINTER_NONE`] (732.1 §4.1.4.2.4.3–6; USLP-006, RS-020).
///
/// So every offset of this type has a pointer value, and none is read back as "none". Whether it
/// lies inside a given TFDZ is the check of the decoder (USLP-006) and the writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TfdzOffset(u16);

impl TfdzOffset {
    /// Largest offset, `0xFFFE`: the last value below [`POINTER_NONE`] (§4.1.4.2.4.4,
    /// §4.1.4.2.4.6).
    pub const MAX: u16 = POINTER_NONE.saturating_sub(1);

    /// The offset, or `None` for [`POINTER_NONE`] (API-001).
    #[must_use]
    pub const fn new(offset: u16) -> Option<TfdzOffset> {
        if offset <= TfdzOffset::MAX {
            Some(TfdzOffset(offset))
        } else {
            None
        }
    }

    /// The offset in octets (§4.1.4.2.4.3, §4.1.4.2.4.5).
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for TfdzOffset {
    type Error = Error;

    /// # Errors
    /// `OutOfRange` (`fhp_lvo_pointer`) for [`POINTER_NONE`], with the largest offset.
    fn try_from(offset: u16) -> Result<TfdzOffset, Error> {
        TfdzOffset::new(offset).ok_or(Error::out_of_range(
            field::FHP_LVO_POINTER,
            u64::from(TfdzOffset::MAX),
            u64::from(offset),
        ))
    }
}

impl From<TfdzOffset> for u16 {
    fn from(offset: TfdzOffset) -> u16 {
        offset.0
    }
}

/// The First Header Pointer of rule `000` (732.1 §4.1.4.2.4.3–4; USLP-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirstHeaderPointer {
    /// The first packet header that starts in the TFDZ starts at this offset (RS-020).
    PacketStartsAt(TfdzOffset),
    /// `0xFFFF`: no packet starts or ends in the TFDZ.
    NoPacketStartOrEnd,
}

/// The Last Valid Octet Pointer of rules `001` and `010` (732.1 §4.1.4.2.4.5–6; USLP-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LastValidOctet {
    /// The SDU ends at this offset; idle data follows (RS-020).
    At(TfdzOffset),
    /// `0xFFFF`: the SDU does not end in this TFDZ.
    SduContinues,
}

impl FirstHeaderPointer {
    /// The pointer of a 16-bit field value (§4.1.4.2.4.3–4).
    #[must_use]
    pub const fn from_raw(raw: u16) -> FirstHeaderPointer {
        match TfdzOffset::new(raw) {
            Some(offset) => FirstHeaderPointer::PacketStartsAt(offset),
            None => FirstHeaderPointer::NoPacketStartOrEnd,
        }
    }

    /// The 16-bit field value.
    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            FirstHeaderPointer::PacketStartsAt(n) => n.get(),
            FirstHeaderPointer::NoPacketStartOrEnd => POINTER_NONE,
        }
    }
}

impl LastValidOctet {
    /// The pointer of a 16-bit field value (§4.1.4.2.4.5–6).
    #[must_use]
    pub const fn from_raw(raw: u16) -> LastValidOctet {
        match TfdzOffset::new(raw) {
            Some(offset) => LastValidOctet::At(offset),
            None => LastValidOctet::SduContinues,
        }
    }

    /// The 16-bit field value.
    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            LastValidOctet::At(n) => n.get(),
            LastValidOctet::SduContinues => POINTER_NONE,
        }
    }
}

/// A TFDZ Construction Rule with the pointer the rule requires (732.1 §4.1.4.2.2,
/// §4.1.4.2.4; USLP-005, USLP-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TfdzRule {
    /// `000`, with its First Header Pointer.
    PacketsSpanningFrames(FirstHeaderPointer),
    /// `001`, with its Last Valid Octet Pointer.
    SduStart(LastValidOctet),
    /// `010`, with its Last Valid Octet Pointer.
    SduContinuation(LastValidOctet),
    /// `011`.
    OctetStream,
    /// `100`.
    StartingSegment,
    /// `101`.
    ContinuingSegment,
    /// `110`.
    LastSegment,
    /// `111`.
    NoSegmentation,
}

impl TfdzRule {
    /// The rule of a field value and, for rules `000`–`010`, the raw pointer that follows it;
    /// the pointer is ignored for the other rules (USLP-005).
    #[must_use]
    pub const fn new(rule: ConstructionRule, pointer: u16) -> TfdzRule {
        match rule {
            ConstructionRule::PacketsSpanningFrames => {
                TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(pointer))
            }
            ConstructionRule::SduStart => TfdzRule::SduStart(LastValidOctet::from_raw(pointer)),
            ConstructionRule::SduContinuation => {
                TfdzRule::SduContinuation(LastValidOctet::from_raw(pointer))
            }
            ConstructionRule::OctetStream => TfdzRule::OctetStream,
            ConstructionRule::StartingSegment => TfdzRule::StartingSegment,
            ConstructionRule::ContinuingSegment => TfdzRule::ContinuingSegment,
            ConstructionRule::LastSegment => TfdzRule::LastSegment,
            ConstructionRule::NoSegmentation => TfdzRule::NoSegmentation,
        }
    }

    /// The construction rule (Table 4-3).
    #[must_use]
    pub const fn rule(self) -> ConstructionRule {
        match self {
            TfdzRule::PacketsSpanningFrames(_) => ConstructionRule::PacketsSpanningFrames,
            TfdzRule::SduStart(_) => ConstructionRule::SduStart,
            TfdzRule::SduContinuation(_) => ConstructionRule::SduContinuation,
            TfdzRule::OctetStream => ConstructionRule::OctetStream,
            TfdzRule::StartingSegment => ConstructionRule::StartingSegment,
            TfdzRule::ContinuingSegment => ConstructionRule::ContinuingSegment,
            TfdzRule::LastSegment => ConstructionRule::LastSegment,
            TfdzRule::NoSegmentation => ConstructionRule::NoSegmentation,
        }
    }

    /// The raw 16-bit pointer, present for rules `000`–`010` (§4.1.4.2.4; USLP-005).
    #[must_use]
    pub const fn pointer(self) -> Option<u16> {
        match self {
            TfdzRule::PacketsSpanningFrames(p) => Some(p.raw()),
            TfdzRule::SduStart(p) | TfdzRule::SduContinuation(p) => Some(p.raw()),
            TfdzRule::OctetStream
            | TfdzRule::StartingSegment
            | TfdzRule::ContinuingSegment
            | TfdzRule::LastSegment
            | TfdzRule::NoSegmentation => None,
        }
    }
}

/// The TFDF Header of a decoded or written frame (732.1 §4.1.4.2; USLP-005…USLP-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TfdfHeader {
    /// The construction rule and its pointer (§4.1.4.2.2, §4.1.4.2.4).
    pub rule: TfdzRule,
    /// The USLP Protocol Identifier (§4.1.4.2.3; USLP-007).
    pub upid: Upid,
}

/// A data field of a VC of packets, read by [`TfdfHeader::parse_packet_field`] (732.1
/// §4.1.4.2; LNK-060).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketField<'a> {
    /// The TFDF is of only idle data (UPID "Only Idle Data").
    OnlyIdle {
        /// The TFDZ.
        zone: &'a [u8],
    },
    /// Packets, rule `000`: the First Header Pointer, below the TFDZ length if it is an offset.
    Packets {
        /// The pointer.
        pointer: FirstHeaderPointer,
        /// The TFDZ.
        zone: &'a [u8],
    },
}

/// The fields of the TFDF Header by bit position (732.1 §4.1.4.2, Figure 4-5; GEN-051).
mod layout {
    use aldis_core::bits::Field;

    const N: usize = 3;

    pub(super) type Rule = Field<N, 0, 3>;
    pub(super) type Upid = Field<N, 3, 5>;
    pub(super) type Pointer = Field<N, 8, 16>;
}

/// Length of the TFDF Header of a fixed-length TFDZ, and of the VCA SDU and OID data of a
/// fixed-length frame: 3 octets (§4.1.4.2.1.2; USLP-005).
pub const FIXED_TFDF_HEADER_LEN: usize = 3;

/// Length of the TFDF Header of a VC: 3 octets with the pointer of a fixed-length TFDZ, 1
/// without (§4.1.4.2.1.2; USLP-005).
#[must_use]
pub(crate) const fn header_len(fixed_tfdz: bool) -> usize {
    if fixed_tfdz { FIXED_TFDF_HEADER_LEN } else { 1 }
}

impl TfdfHeader {
    /// Length of the header: 3 octets with the pointer, 1 without (§4.1.4.2.1.2; USLP-005).
    #[must_use]
    pub const fn len(&self) -> usize {
        if self.rule.rule().has_pointer() {
            FIXED_TFDF_HEADER_LEN
        } else {
            1
        }
    }

    /// Always `false`: the header has at least one octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The header octets, in the first [`len`](Self::len) octets of the array (§4.1.4.2;
    /// API-020).
    #[must_use]
    pub const fn octets(&self) -> [u8; 3] {
        let mut h = [0_u8; FIXED_TFDF_HEADER_LEN];
        layout::Rule::put_u8(&mut h, self.rule.rule().bits());
        layout::Upid::put_u8(&mut h, self.upid.bits());
        if let Some(pointer) = self.rule.pointer() {
            layout::Pointer::put_u16(&mut h, pointer);
        }
        h
    }

    /// The header of an Only Idle Data frame whose data field has `data_len` octets: rule `001`,
    /// UPID "Only Idle Data", and the Last Valid Octet Pointer at the last octet of the TFDZ
    /// (732.1 §4.1.4.1.9–11; USLP-009, LNK-030).
    ///
    /// A data field has at most 65 536 octets, so the pointer fits its 16 bits.
    #[must_use]
    pub const fn only_idle_data(data_len: usize) -> TfdfHeader {
        let [.., a, b] = data_len
            .saturating_sub(FIXED_TFDF_HEADER_LEN.saturating_add(1))
            .to_be_bytes();
        TfdfHeader {
            rule: TfdzRule::SduStart(LastValidOctet::from_raw(u16::from_be_bytes([a, b]))),
            upid: Upid::OnlyIdleData,
        }
    }

    /// Reads the header at the start of `tfdf`, whose type the VC sets: `fixed_tfdz` for a
    /// fixed-length TFDZ, which has the pointer (USLP-005, USLP-016). Returns the header and the
    /// TFDZ after it.
    ///
    /// # Errors
    /// `InvalidField` (`tfdz_construction_rule`) for a rule of the other TFDZ type, before the
    /// pointer is read (USLP-016); `Malformed` (`fhp_lvo_pointer`) for a pointer other than
    /// `0xFFFF` that is not below the TFDZ length (USLP-006); `InvalidField`
    /// (`frame_data_field`) if `tfdf` holds no TFDZ octet after the header (CMN-041).
    pub(crate) fn parse(data_field: &[u8], fixed_tfdz: bool) -> Result<(TfdfHeader, &[u8]), Error> {
        let &first = data_field
            .first()
            .ok_or_else(|| invalid(field::FRAME_DATA_FIELD))?;
        let rule = ConstructionRule::from_bits(layout::Rule::get_u8(&[first, 0, 0]));
        if rule.is_fixed_length() != fixed_tfdz {
            return Err(invalid(field::TFDZ_CONSTRUCTION_RULE));
        }
        let upid = Upid::from_field(first);
        let header_len = header_len(fixed_tfdz);
        let (head, zone) = data_field
            .split_at_checked(header_len)
            .filter(|(_, z)| !z.is_empty())
            .ok_or(invalid(field::FRAME_DATA_FIELD))?;
        let pointer = match head {
            [_, p0, p1] => u16::from_be_bytes([*p0, *p1]),
            _ => POINTER_NONE,
        };
        if pointer != POINTER_NONE && usize::from(pointer) >= zone.len() {
            return Err(Error::new(ErrorKind::Malformed).with_field(field::FHP_LVO_POINTER));
        }
        let header = TfdfHeader {
            rule: TfdzRule::new(rule, pointer),
            upid,
        };
        Ok((header, zone))
    }

    /// Reads the data field of a VC of packets, whose frames are fixed in length (732.1
    /// §4.1.4.2; USLP-005, USLP-006, LNK-060, LNK-064).
    ///
    /// The UPID is read before the rule: an Only Idle Data TFDF, which an OID frame carries on a
    /// VC that has packets, has rule `001` there (§4.1.4.1.9), and its zone is idle whatever the
    /// rule, so the field is idle data and not an error. Any other UPID is not compared with the
    /// VC's: the receiver routes a TFDF to its service by the UPID. Then the rule must be `000`.
    ///
    /// # Errors
    /// `InvalidField` (`frame_data_field`) if the field holds no TFDZ octet after the three
    /// octets of the header; `InvalidField` (`tfdz_construction_rule`) for a rule other than
    /// `000` on a field that is not idle data; `Malformed` (`fhp_lvo_pointer`) for a pointer
    /// other than `0xFFFF` that is not below the TFDZ length.
    pub fn parse_packet_field(data_field: &[u8]) -> Result<PacketField<'_>, Error> {
        let [first, p0, p1, zone @ ..] = data_field else {
            return Err(invalid(field::FRAME_DATA_FIELD));
        };
        if zone.is_empty() {
            return Err(invalid(field::FRAME_DATA_FIELD));
        }
        let head = [*first, 0, 0];
        if Upid::from_field(layout::Upid::get_u8(&head)) == Upid::OnlyIdleData {
            return Ok(PacketField::OnlyIdle { zone });
        }
        if ConstructionRule::from_bits(layout::Rule::get_u8(&head))
            != ConstructionRule::PacketsSpanningFrames
        {
            return Err(invalid(field::TFDZ_CONSTRUCTION_RULE));
        }
        match FirstHeaderPointer::from_raw(u16::from_be_bytes([*p0, *p1])) {
            FirstHeaderPointer::PacketStartsAt(n) if usize::from(n.get()) < zone.len() => {
                Ok(PacketField::Packets {
                    pointer: FirstHeaderPointer::PacketStartsAt(n),
                    zone,
                })
            }
            FirstHeaderPointer::PacketStartsAt(_) => {
                Err(Error::new(ErrorKind::Malformed).with_field(field::FHP_LVO_POINTER))
            }
            FirstHeaderPointer::NoPacketStartOrEnd => Ok(PacketField::Packets {
                pointer: FirstHeaderPointer::NoPacketStartOrEnd,
                zone,
            }),
        }
    }

    /// `OutOfRange` (`fhp_lvo_pointer`) unless the pointer is `0xFFFF` or below `tfdz_len`
    /// (§4.1.4.2.4; USLP-006).
    pub(crate) fn check_pointer(self, tfdz_len: usize) -> Result<(), Error> {
        match self.rule.pointer() {
            Some(p) if p != POINTER_NONE && usize::from(p) >= tfdz_len => Err(Error::out_of_range(
                field::FHP_LVO_POINTER,
                len_u64(tfdz_len.saturating_sub(1)),
                u64::from(p),
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    use super::{
        ConstructionRule, FirstHeaderPointer, LastValidOctet, POINTER_NONE, TfdfHeader, TfdzOffset,
        TfdzRule,
    };
    use crate::field;
    use crate::upid::Upid;
    use aldis_core::{ErrorDetail, ErrorKind};

    #[test]
    fn packet_field_reads_the_upid_before_the_rule() {
        // req: USLP-005, USLP-006, USLP-009, LNK-064
        use super::PacketField;
        let idle = Upid::OnlyIdleData.bits();
        // An Only Idle Data TFDF has rule `001` in an OID frame: idle, not a rule error.
        let oid = [(0b001 << 5) | idle, 0x00, 0x05, 1, 2];
        assert_eq!(
            TfdfHeader::parse_packet_field(&oid),
            Ok(PacketField::OnlyIdle { zone: &[1, 2] })
        );
        let packets = |rule: u8, p: u16, zone: [u8; 2]| {
            let [a, b] = p.to_be_bytes();
            [(rule << 5) | Upid::Packets.bits(), a, b, zone[0], zone[1]]
        };
        let ok = packets(0b000, 1, [9, 9]);
        assert_eq!(
            TfdfHeader::parse_packet_field(&ok),
            Ok(PacketField::Packets {
                pointer: FirstHeaderPointer::PacketStartsAt(TfdzOffset::new(1).unwrap()),
                zone: &[9, 9]
            })
        );
        let none = packets(0b000, POINTER_NONE, [9, 8]);
        assert_eq!(
            TfdfHeader::parse_packet_field(&none),
            Ok(PacketField::Packets {
                pointer: FirstHeaderPointer::NoPacketStartOrEnd,
                zone: &[9, 8]
            })
        );
        let e = TfdfHeader::parse_packet_field(&packets(0b000, 2, [9, 9])).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Malformed);
        assert_eq!(e.field(), Some(field::FHP_LVO_POINTER));
        let e = TfdfHeader::parse_packet_field(&packets(0b111, 0, [9, 9])).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
        assert_eq!(e.field(), Some(field::TFDZ_CONSTRUCTION_RULE));
        for short in [&[][..], &[0, 0][..], &[0, 0, 0][..]] {
            let e = TfdfHeader::parse_packet_field(short).unwrap_err();
            assert_eq!(e.field(), Some(field::FRAME_DATA_FIELD));
        }
    }

    #[test]
    fn the_oid_header_points_at_the_last_octet_of_the_zone() {
        // req: USLP-009, LNK-030
        let h = TfdfHeader::only_idle_data(20);
        assert_eq!(h.upid, Upid::OnlyIdleData);
        assert_eq!(h.rule.rule(), ConstructionRule::SduStart);
        assert_eq!(h.rule.pointer(), Some(16));
        assert_eq!(TfdfHeader::only_idle_data(0).rule.pointer(), Some(0));
    }

    #[test]
    fn rules_and_types() {
        // req: USLP-005, USLP-016
        for bits in 0..=255_u8 {
            let r = ConstructionRule::from_bits(bits);
            assert_eq!(r.bits(), bits & 7);
            assert_eq!(r.is_fixed_length(), bits & 7 <= 2);
            assert_eq!(r.has_pointer(), r.is_fixed_length());
            let t = TfdzRule::new(r, 7);
            assert_eq!(t.rule(), r);
            assert_eq!(t.pointer().is_some(), r.has_pointer());
        }
    }

    pub(crate) fn offset(n: u16) -> TfdzOffset {
        TfdzOffset::new(n).unwrap()
    }

    #[test]
    fn offsets_lie_below_the_none_value() {
        // req: USLP-006, RS-020
        assert_eq!(TfdzOffset::MAX, POINTER_NONE - 1);
        for n in [0, 1, 0x7FFF, 0xFFFE] {
            assert_eq!((offset(n).get(), u16::from(offset(n))), (n, n));
            assert_eq!(TfdzOffset::try_from(n), Ok(offset(n)));
        }
        assert_eq!(TfdzOffset::new(POINTER_NONE), None);
        let e = TfdzOffset::try_from(POINTER_NONE).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::OutOfRange,
                Some(field::FHP_LVO_POINTER),
                ErrorDetail::Expected {
                    expected: 0xFFFE,
                    found: 0xFFFF
                }
            )
        );
    }

    #[test]
    fn pointers_classify() {
        // req: USLP-006
        assert_eq!(
            FirstHeaderPointer::from_raw(POINTER_NONE),
            FirstHeaderPointer::NoPacketStartOrEnd
        );
        assert_eq!(
            FirstHeaderPointer::from_raw(4),
            FirstHeaderPointer::PacketStartsAt(offset(4))
        );
        assert_eq!(FirstHeaderPointer::PacketStartsAt(offset(4)).raw(), 4);
        assert_eq!(FirstHeaderPointer::NoPacketStartOrEnd.raw(), POINTER_NONE);
        assert_eq!(
            LastValidOctet::from_raw(POINTER_NONE),
            LastValidOctet::SduContinues
        );
        assert_eq!(LastValidOctet::from_raw(9), LastValidOctet::At(offset(9)));
        assert_eq!(LastValidOctet::At(offset(9)).raw(), 9);
        // The largest offset is a pointer value, not "none" (RS-020).
        assert_eq!(FirstHeaderPointer::from_raw(0xFFFE).raw(), 0xFFFE);
        assert_eq!(LastValidOctet::from_raw(0xFFFE).raw(), 0xFFFE);
        assert_eq!(LastValidOctet::SduContinues.raw(), POINTER_NONE);
        assert_eq!(
            TfdzRule::new(ConstructionRule::SduContinuation, 3).pointer(),
            Some(3)
        );
    }

    #[test]
    fn parse_and_octets() {
        // req: USLP-005, USLP-006, USLP-007, USLP-016, CMN-041
        let (h, z) = TfdfHeader::parse(&[0x00, 0x00, 0x02, 1, 2, 3], true).unwrap();
        assert_eq!(
            h.rule,
            TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(offset(2)))
        );
        assert_eq!(
            (h.upid, z, h.len(), h.is_empty()),
            (Upid::Packets, &[1, 2, 3][..], 3, false)
        );
        assert_eq!(h.octets(), [0x00, 0x00, 0x02]);
        let (h, z) = TfdfHeader::parse(&[0xFF, 9], false).unwrap();
        assert_eq!(
            (h.rule, h.upid, z),
            (TfdzRule::NoSegmentation, Upid::OnlyIdleData, &[9][..])
        );
        assert_eq!((h.len(), h.octets()), (1, [0xFF, 0, 0]));
        let e = |bytes: &[u8], fixed| TfdfHeader::parse(bytes, fixed).unwrap_err();
        let fe = |bytes: &[u8], fixed| {
            let x = e(bytes, fixed);
            (x.kind(), x.field())
        };
        assert_eq!(
            fe(&[0x60, 1], true),
            (ErrorKind::InvalidField, Some(field::TFDZ_CONSTRUCTION_RULE))
        );
        assert_eq!(
            fe(&[0x40, 0, 0, 1], false),
            (ErrorKind::InvalidField, Some(field::TFDZ_CONSTRUCTION_RULE))
        );
        assert_eq!(
            fe(&[0x20, 0, 3, 1, 2, 3], true),
            (ErrorKind::Malformed, Some(field::FHP_LVO_POINTER))
        );
        assert_eq!(
            fe(&[0x20, 0, 0], true),
            (ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD))
        );
        assert_eq!(fe(&[], false).1, Some(field::FRAME_DATA_FIELD));
        assert_eq!(fe(&[], true).1, Some(field::FRAME_DATA_FIELD));
        let (h, _) = TfdfHeader::parse(&[0x20, 0xFF, 0xFF, 1], true).unwrap();
        assert_eq!(h.rule, TfdzRule::SduStart(LastValidOctet::SduContinues));
        assert!(h.check_pointer(1).is_ok());
        let lvo = TfdfHeader {
            rule: TfdzRule::SduStart(LastValidOctet::At(offset(4))),
            upid: Upid::Packets,
        };
        assert!(lvo.check_pointer(5).is_ok());
        let x = lvo.check_pointer(4).unwrap_err();
        assert_eq!(
            (x.kind(), x.field()),
            (ErrorKind::OutOfRange, Some(field::FHP_LVO_POINTER))
        );
        let none = TfdfHeader {
            rule: TfdzRule::OctetStream,
            upid: Upid::Packets,
        };
        assert!(none.check_pointer(0).is_ok());
    }
}
