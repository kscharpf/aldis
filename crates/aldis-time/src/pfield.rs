//! Preamble field (P-field) parsing and the undefined-extension tail (301.0 §3.1.1; TIME-001,
//! TIME-002, TIME-005).
//!
//! CCSDS numbers bits from the most significant: bit 0 of an octet is its MSB.

use aldis_core::{Error, ErrorKind, Result};

use crate::field;

/// Largest P-field Aldis accepts, in octets (301.0 §3.1.1, TIME-005).
pub const MAX_PFIELD_LEN: usize = 8;

/// The extension flag, bit 0 of every P-field octet (301.0 §3.1.1).
pub(crate) const EXTENSION: u8 = 0x80;

/// Time code identification, P-field octet 1 bits 1–3 (301.0 §3.1.1, TIME-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeCodeId {
    /// `001`: CUC with the 1958 January 1 epoch (Level 1, §3.2.2).
    CucLevel1,
    /// `010`: CUC with an agency-defined epoch (Level 2, §3.2.2).
    CucLevel2,
    /// `100`: CDS (§3.3.2).
    Cds,
    /// `101`: CCS (§3.4.2).
    Ccs,
    /// `110`: agency-defined code, Level 3 or 4 (§3.6.2).
    Agency,
    /// `000`: reserved for future application (§3.1.1, TIME-002).
    Reserved000,
    /// `011`: reserved for future application (§3.1.1, TIME-002).
    Reserved011,
    /// `111`: reserved for future application (§3.1.1, TIME-002).
    Reserved111,
}

impl TimeCodeId {
    /// The identification in bits 1–3 of `octet` (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub const fn from_octet(octet: u8) -> TimeCodeId {
        match (octet >> 4) & 0b111 {
            0b001 => TimeCodeId::CucLevel1,
            0b010 => TimeCodeId::CucLevel2,
            0b100 => TimeCodeId::Cds,
            0b101 => TimeCodeId::Ccs,
            0b110 => TimeCodeId::Agency,
            0b000 => TimeCodeId::Reserved000,
            0b011 => TimeCodeId::Reserved011,
            _ => TimeCodeId::Reserved111,
        }
    }

    /// Whether the identification is reserved (301.0 §3.1.1, TIME-002).
    #[must_use]
    pub const fn is_reserved(self) -> bool {
        matches!(
            self,
            TimeCodeId::Reserved000 | TimeCodeId::Reserved011 | TimeCodeId::Reserved111
        )
    }

    /// The 3-bit field value (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            TimeCodeId::CucLevel1 => 0b001,
            TimeCodeId::CucLevel2 => 0b010,
            TimeCodeId::Cds => 0b100,
            TimeCodeId::Ccs => 0b101,
            TimeCodeId::Agency => 0b110,
            TimeCodeId::Reserved000 => 0b000,
            TimeCodeId::Reserved011 => 0b011,
            TimeCodeId::Reserved111 => 0b111,
        }
    }
}

/// A P-field as received: octet 1 and up to seven extension octets (301.0 §3.1.1; TIME-001,
/// TIME-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PField {
    first: u8,
    rest: [u8; MAX_PFIELD_LEN - 1],
    rest_len: u8,
}

impl PField {
    /// Parses the P-field at the start of `bytes`: octet 1, then one more octet for as long
    /// as the previous one has its extension flag (bit 0) set (301.0 §3.1.1, TIME-001).
    ///
    /// # Errors
    /// - `Incomplete` (`needed` = octets required so far) if `bytes` ends inside the chain.
    /// - `Malformed` if the chain is longer than [`MAX_PFIELD_LEN`] octets (TIME-005).
    pub fn parse(bytes: &[u8]) -> Result<PField> {
        let Some((&first, tail)) = bytes.split_first() else {
            return Err(Error::incomplete(1));
        };
        let mut rest = [0_u8; MAX_PFIELD_LEN - 1];
        let mut rest_len: u8 = 0;
        let mut more = first & EXTENSION != 0;
        for (slot, &octet) in rest.iter_mut().zip(tail) {
            if !more {
                break;
            }
            *slot = octet;
            rest_len = rest_len.wrapping_add(1);
            more = octet & EXTENSION != 0;
        }
        if !more {
            return Ok(PField {
                first,
                rest,
                rest_len,
            });
        }
        let used = usize::from(rest_len).wrapping_add(1);
        if used == MAX_PFIELD_LEN {
            Err(Error::new(ErrorKind::Malformed).with_field(field::PFIELD))
        } else {
            Err(Error::incomplete(used.wrapping_add(1)))
        }
    }

    /// Octet 1 (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub const fn first(&self) -> u8 {
        self.first
    }

    /// The extension octets after octet 1 (301.0 §3.1.1; TIME-001, TIME-005).
    #[must_use]
    pub fn rest(&self) -> &[u8] {
        self.rest
            .get(..usize::from(self.rest_len))
            .unwrap_or_default()
    }

    /// Length in octets, 1 to [`MAX_PFIELD_LEN`] (301.0 §3.1.1; TIME-003, TIME-005).
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.rest_len).wrapping_add(1)
    }

    /// Always `false`: a P-field has at least octet 1 (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The time code identification (301.0 §3.1.1, TIME-001, TIME-002).
    #[must_use]
    pub const fn id(&self) -> TimeCodeId {
        TimeCodeId::from_octet(self.first)
    }

    /// The octets in order, into an array whose first [`len`](Self::len) entries are used.
    pub(crate) fn octets(&self) -> [u8; MAX_PFIELD_LEN] {
        let mut out = [0_u8; MAX_PFIELD_LEN];
        for (d, s) in out
            .iter_mut()
            .zip(core::iter::once(&self.first).chain(self.rest()))
        {
            *d = *s;
        }
        out
    }

    /// Builds a P-field from octet 1, defined extension octets, and an undefined tail; sets
    /// every extension flag from the chain (encoder side).
    pub(crate) fn build(first: u8, defined: &[u8], tail: PFieldTail) -> PField {
        let mut rest = [0_u8; MAX_PFIELD_LEN - 1];
        let mut rest_len: u8 = 0;
        for (slot, &octet) in rest.iter_mut().zip(defined.iter().chain(tail.as_bytes())) {
            *slot = octet;
            rest_len = rest_len.wrapping_add(1);
        }
        // Every octet but the last carries the extension flag.
        let n = usize::from(rest_len);
        for (i, slot) in rest.iter_mut().enumerate().take(n) {
            if i.wrapping_add(1) < n {
                *slot |= EXTENSION;
            } else {
                *slot &= !EXTENSION;
            }
        }
        let first = if n > 0 {
            first | EXTENSION
        } else {
            first & !EXTENSION
        };
        PField {
            first,
            rest,
            rest_len,
        }
    }
}

/// P-field extension octets that 301.0 does not define for a code, kept raw so a lenient decode
/// re-encodes unchanged (301.0 §3.1.1, TIME-005). Each octet but the last has its extension flag
/// set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PFieldTail {
    octets: [u8; MAX_PFIELD_LEN - 1],
    len: u8,
}

impl PFieldTail {
    /// No undefined octets (301.0 §3.1.1, TIME-005).
    pub const EMPTY: PFieldTail = PFieldTail {
        octets: [0; MAX_PFIELD_LEN - 1],
        len: 0,
    };

    /// A tail of `octets`, up to 7 (301.0 §3.1.1, TIME-005).
    ///
    /// # Errors
    /// `InvalidField` (`pfield_tail`) if there are more than 7 octets, or the extension flags do
    /// not form a chain: set on every octet but the last, clear on the last.
    pub fn new(octets: &[u8]) -> Result<PFieldTail> {
        let invalid = Error::new(ErrorKind::InvalidField).with_field(field::PFIELD_TAIL);
        if octets.len() >= MAX_PFIELD_LEN {
            return Err(invalid);
        }
        let mut tail = PFieldTail::EMPTY;
        let n = octets.len();
        for (i, (slot, &octet)) in tail.octets.iter_mut().zip(octets).enumerate() {
            let last = i.wrapping_add(1) == n;
            if (octet & EXTENSION != 0) == last {
                return Err(invalid);
            }
            *slot = octet;
            tail.len = tail.len.wrapping_add(1);
        }
        Ok(tail)
    }

    /// A tail copied from octets already known to form a chain (decoder side).
    pub(crate) fn from_chain(octets: &[u8]) -> PFieldTail {
        let mut tail = PFieldTail::EMPTY;
        for (slot, &octet) in tail.octets.iter_mut().zip(octets) {
            *slot = octet;
            tail.len = tail.len.wrapping_add(1);
        }
        tail
    }

    /// The octets (301.0 §3.1.1, TIME-005).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.octets.get(..usize::from(self.len)).unwrap_or_default()
    }

    /// Number of octets (301.0 §3.1.1, TIME-005).
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    /// Whether there are no undefined octets (301.0 §3.1.1, TIME-005).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A P-field whose time code ID is reserved (`000`, `011`, or `111`): what a lenient decode
/// returns as [`TimeCode::Unknown`](crate::code::TimeCode::Unknown) (301.0 §3.1.1, TIME-002,
/// RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReservedPField(PField);

impl ReservedPField {
    /// `pfield` if its time code ID is reserved, else `None` (301.0 §3.1.1, TIME-002).
    #[must_use]
    pub const fn new(pfield: PField) -> Option<ReservedPField> {
        if pfield.id().is_reserved() {
            Some(ReservedPField(pfield))
        } else {
            None
        }
    }

    /// Wraps a P-field the caller has already found to have a reserved ID (decoder side).
    pub(crate) const fn from_reserved(pfield: PField) -> ReservedPField {
        ReservedPField(pfield)
    }

    /// The P-field (301.0 §3.1.1, TIME-002).
    #[must_use]
    pub const fn pfield(&self) -> PField {
        self.0
    }
}

impl From<ReservedPField> for PField {
    fn from(reserved: ReservedPField) -> PField {
        reserved.0
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{EXTENSION, MAX_PFIELD_LEN, PField, PFieldTail, ReservedPField, TimeCodeId};
    use aldis_core::{ErrorDetail, ErrorKind};

    #[test]
    fn time_code_ids() {
        // req: TIME-001, TIME-002
        for (bits, id) in [
            (0b000, TimeCodeId::Reserved000),
            (0b001, TimeCodeId::CucLevel1),
            (0b010, TimeCodeId::CucLevel2),
            (0b011, TimeCodeId::Reserved011),
            (0b100, TimeCodeId::Cds),
            (0b101, TimeCodeId::Ccs),
            (0b110, TimeCodeId::Agency),
            (0b111, TimeCodeId::Reserved111),
        ] {
            assert_eq!(TimeCodeId::from_octet(bits << 4), id);
            assert_eq!(TimeCodeId::from_octet((bits << 4) | 0x8F), id);
            assert_eq!(id.bits(), bits);
        }
        let reserved = [
            TimeCodeId::Reserved000,
            TimeCodeId::Reserved011,
            TimeCodeId::Reserved111,
        ];
        assert!(reserved.iter().all(|id| id.is_reserved()));
        assert!(!TimeCodeId::CucLevel1.is_reserved() && !TimeCodeId::Agency.is_reserved());
    }

    #[test]
    fn reserved_pfields_hold_only_reserved_ids() {
        // req: TIME-002, RS-020
        let reserved = PField::parse(&[0xB0, 0x01]).unwrap();
        let r = ReservedPField::new(reserved).unwrap();
        assert_eq!(r.pfield(), reserved);
        assert_eq!(PField::from(r), reserved);
        assert_eq!(ReservedPField::new(PField::parse(&[0x1E]).unwrap()), None);
        assert_eq!(ReservedPField::from_reserved(reserved), r);
    }

    #[test]
    fn parse_chains() {
        // req: TIME-001, TIME-005
        let p = PField::parse(&[0x1E, 0xFF]).unwrap();
        assert_eq!(
            (p.first(), p.rest(), p.len(), p.is_empty()),
            (0x1E, &[][..], 1, false)
        );
        assert_eq!(p.id(), TimeCodeId::CucLevel1);
        let p = PField::parse(&[0x9F, 0x80, 0x01, 0xAA]).unwrap();
        assert_eq!((p.rest(), p.len()), (&[0x80, 0x01][..], 3));
        assert_eq!(p.octets()[..3], [0x9F, 0x80, 0x01]);
        let mut eight = [EXTENSION; MAX_PFIELD_LEN];
        eight[7] = 0;
        assert_eq!(PField::parse(&eight).unwrap().len(), 8);
        let e = PField::parse(&[EXTENSION; 9]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Malformed);
        let e = PField::parse(&[0x80, 0x80]).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(3))
        );
        assert_eq!(
            PField::parse(&[]).unwrap_err().detail(),
            ErrorDetail::Needed(1)
        );
    }

    #[test]
    fn build_sets_the_extension_flags() {
        // req: TIME-001, TIME-005
        let tail = PFieldTail::new(&[0x81, 0x02]).unwrap();
        let p = PField::build(0x9F, &[0x7E], tail);
        assert_eq!((p.first(), p.rest()), (0x9F, &[0xFE, 0x81, 0x02][..]));
        let p = PField::build(0x9F, &[], PFieldTail::EMPTY);
        assert_eq!((p.first(), p.len()), (0x1F, 1));
        let p = PField::build(0x40, &[0x83], PFieldTail::EMPTY);
        assert_eq!((p.first(), p.rest()), (0xC0, &[0x03][..]));
    }

    #[test]
    fn tails_must_form_a_chain() {
        // req: TIME-005
        assert_eq!(PFieldTail::new(&[]), Ok(PFieldTail::EMPTY));
        let t = PFieldTail::new(&[0x80, 0x80, 0x7F]).unwrap();
        assert_eq!(
            (t.as_bytes(), t.len(), t.is_empty()),
            (&[0x80, 0x80, 0x7F][..], 3, false)
        );
        assert!(PFieldTail::EMPTY.is_empty() && PFieldTail::default().is_empty());
        for bad in [
            &[0x80][..],
            &[0x00, 0x00],
            &[0x80; 7],
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
        ] {
            let e = PFieldTail::new(bad).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidField, "{bad:?}");
        }
        assert_eq!(
            PFieldTail::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00])
                .unwrap()
                .len(),
            7
        );
        assert_eq!(
            PFieldTail::from_chain(&[0x81, 0x02]).as_bytes(),
            [0x81, 0x02]
        );
    }
}
