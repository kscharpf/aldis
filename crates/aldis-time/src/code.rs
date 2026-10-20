//! Time codes with an explicit or implicit P-field: decode, encode, agency-defined codes
//! (301.0 §3, §3.6; TIME-001…TIME-006).

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::anomaly::TimeAnomaly;
use crate::ccs::{Ccs, CcsFormat};
use crate::cds::{Cds, CdsFormat};
use crate::cuc::{Cuc, CucFormat};
use crate::pfield::{MAX_PFIELD_LEN, PField, PFieldTail, ReservedPField, TimeCodeId};
use crate::util::wide8;

/// Longest agency-defined T-field, 16 octets (301.0 §3.6.2, TIME-006).
pub const MAX_AGENCY_LEN: u8 = 16;

/// Longest T-field of any supported code: CUC with 7 + 10 octets.
const MAX_T_LEN: usize = 17;

/// Layout of an agency-defined code: its T-field length (301.0 §3.6.2, TIME-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgencyFormat {
    t_len: u8,
    tail: PFieldTail,
}

impl AgencyFormat {
    /// A format with a T-field of `t_len` octets, 1–16 (301.0 §3.6.2, TIME-006).
    ///
    /// # Errors
    /// `OutOfRange` (`t_len`) outside 1–16.
    pub const fn new(t_len: u8) -> Result<AgencyFormat> {
        if t_len == 0 || t_len > MAX_AGENCY_LEN {
            // Report the violated bound: 1 for zero, the maximum otherwise (GEN-041).
            let limit = if t_len == 0 { 1 } else { MAX_AGENCY_LEN };
            return Err(Error::out_of_range("t_len", wide8(limit), wide8(t_len)));
        }
        Ok(AgencyFormat {
            t_len,
            tail: PFieldTail::EMPTY,
        })
    }

    /// The same format with undefined P-field octets after octet 1 (301.0 §3.6.2, TIME-005).
    #[must_use]
    pub const fn with_tail(self, tail: PFieldTail) -> AgencyFormat {
        AgencyFormat { tail, ..self }
    }

    /// T-field length in octets (301.0 §3.6.2, TIME-006).
    #[must_use]
    pub fn t_len(&self) -> usize {
        usize::from(self.t_len)
    }

    /// Undefined P-field octets (301.0 §3.6.2, TIME-005).
    #[must_use]
    pub const fn tail(&self) -> PFieldTail {
        self.tail
    }

    /// The P-field: bits 4–7 hold the T-field length minus one (301.0 §3.6.2, TIME-006).
    #[must_use]
    pub fn pfield(&self) -> PField {
        PField::build(
            (TimeCodeId::Agency.bits() << 4) | (self.t_len.wrapping_sub(1) & 0x0F),
            &[],
            self.tail,
        )
    }

    /// The format an agency-defined P-field describes.
    fn from_pfield(pfield: &PField) -> AgencyFormat {
        AgencyFormat {
            t_len: (pfield.first() & 0x0F).wrapping_add(1),
            tail: PFieldTail::from_chain(pfield.rest()),
        }
    }
}

/// An agency-defined code: its raw T-field (301.0 §3.6, TIME-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Agency {
    format: AgencyFormat,
    t: [u8; 16],
}

impl Agency {
    /// An agency-defined code with T-field `t` (301.0 §3.6.1, TIME-006).
    ///
    /// # Errors
    /// `LengthMismatch` (`tfield`) if `t` is not exactly the format's T-field length.
    pub fn new(format: AgencyFormat, t: &[u8]) -> Result<Agency> {
        if t.len() != format.t_len() {
            return Err(Error::new(ErrorKind::LengthMismatch)
                .with_field(FieldId::new("tfield"))
                .with_detail(ErrorDetail::Expected {
                    expected: u64::from(format.t_len),
                    found: u64::try_from(t.len()).unwrap_or(u64::MAX),
                }));
        }
        Ok(Agency::from_t(format, t))
    }

    /// Copies a T-field of the right length.
    fn from_t(format: AgencyFormat, t: &[u8]) -> Agency {
        let mut octets = [0_u8; 16];
        for (d, s) in octets.iter_mut().zip(t) {
            *d = *s;
        }
        Agency { format, t: octets }
    }

    /// The format (301.0 §3.6, TIME-006).
    #[must_use]
    pub const fn format(&self) -> AgencyFormat {
        self.format
    }

    /// The raw T-field (301.0 §3.6.1, TIME-006).
    #[must_use]
    pub fn tfield(&self) -> &[u8] {
        self.t.get(..self.format.t_len()).unwrap_or_default()
    }
}

/// The layout of a binary time code, for decoding with an implicit P-field (301.0 §3.1.1, TIME-001,
/// TIME-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeFormat {
    /// CUC (§3.2).
    Cuc(CucFormat),
    /// CDS (§3.3).
    Cds(CdsFormat),
    /// CCS (§3.4).
    Ccs(CcsFormat),
    /// Agency-defined (§3.6).
    Agency(AgencyFormat),
}

impl TimeFormat {
    /// T-field length in octets (301.0 §3.1.2, TIME-003).
    #[must_use]
    pub fn t_len(&self) -> usize {
        match self {
            TimeFormat::Cuc(f) => f.t_len(),
            TimeFormat::Cds(f) => f.t_len(),
            TimeFormat::Ccs(f) => f.t_len(),
            TimeFormat::Agency(f) => f.t_len(),
        }
    }

    /// The P-field describing this format (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub fn pfield(&self) -> PField {
        match self {
            TimeFormat::Cuc(f) => f.pfield(),
            TimeFormat::Cds(f) => f.pfield(),
            TimeFormat::Ccs(f) => f.pfield(),
            TimeFormat::Agency(f) => f.pfield(),
        }
    }

    /// Whether the format carries undefined P-field octets (TIME-005).
    const fn has_tail(&self) -> bool {
        match self {
            TimeFormat::Cuc(f) => !f.tail().is_empty(),
            TimeFormat::Cds(f) => !f.tail().is_empty(),
            TimeFormat::Ccs(f) => !f.tail().is_empty(),
            TimeFormat::Agency(f) => !f.tail().is_empty(),
        }
    }
}

/// A binary time code (301.0 §3.2–3.4, §3.6; TIME-001, TIME-002, TIME-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeCode {
    /// CUC (301.0 §3.2).
    Cuc(Cuc),
    /// CDS (301.0 §3.3).
    Cds(Cds),
    /// CCS (301.0 §3.4).
    Ccs(Ccs),
    /// Agency-defined (301.0 §3.6, TIME-006).
    Agency(Agency),
    /// A reserved time code ID, returned in lenient mode with its P-field only; the T-field
    /// length is undefined, so the caller parses it (TIME-002).
    Unknown(ReservedPField),
}

/// A successful decode: the code, the octets consumed, and lenient-mode anomalies (301.0 §3.1;
/// TIME-003, GEN-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded {
    code: TimeCode,
    pfield: Option<PField>,
    len: usize,
    anomalies: AnomalySet<TimeAnomaly>,
}

impl Decoded {
    /// The time code (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub const fn code(&self) -> TimeCode {
        self.code
    }

    /// The P-field exactly as received, for [`decode_with_pfield`]; `None` for [`decode_implicit`]
    /// (301.0 §3.1.1; TIME-001, TIME-005). [`TimeCode::pfield`] gives the canonical P-field of the
    /// code, which differs only for a CUC P-field that splits its octet counts non-canonically.
    #[must_use]
    pub const fn pfield(&self) -> Option<PField> {
        self.pfield
    }

    /// Octets consumed: P-field (if explicit) plus T-field, or the P-field alone for
    /// [`TimeCode::Unknown`] (301.0 §3.1, TIME-003).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Always `false`: a decode consumes at least one octet (301.0 §3.1, TIME-003).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (301.0 §3.1.1, §3.4.1.2; API-032,
    /// GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<TimeAnomaly> {
        self.anomalies
    }
}

/// Decodes the T-field `t` (exactly `format.t_len()` octets).
fn decode_t(
    format: TimeFormat,
    t: &[u8],
    mode: DecodeMode,
    anomalies: &mut AnomalySet<TimeAnomaly>,
) -> Result<TimeCode> {
    Ok(match format {
        TimeFormat::Cuc(f) => TimeCode::Cuc(Cuc::from_t(f, t)),
        TimeFormat::Cds(f) => TimeCode::Cds(Cds::from_t(f, t)?),
        TimeFormat::Ccs(f) => TimeCode::Ccs(Ccs::from_t(f, t, mode, anomalies)?),
        TimeFormat::Agency(f) => TimeCode::Agency(Agency::from_t(f, t)),
    })
}

/// The T-field starting at `start` of `bytes`, `len` octets long.
fn t_field(bytes: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    let end = start.saturating_add(len);
    bytes.get(start..end).ok_or(Error::incomplete(end))
}

/// Decodes a time code whose P-field is present at the start of `bytes`; octets after the
/// T-field are ignored (301.0 §3.1; TIME-001…TIME-003, TIME-005, TIME-006).
///
/// # Errors
/// - `Incomplete` if `bytes` ends inside the P-field or the T-field.
/// - `Malformed` for a P-field longer than 8 octets (TIME-005).
/// - `ReservedValue` in strict mode for a reserved time code ID (TIME-002) or undefined
///   P-field extension octets (TIME-005); lenient mode returns [`TimeCode::Unknown`] or keeps
///   the octets, with an anomaly flag.
/// - `ReservedValue` in both modes for CDS submillisecond code `11` or CCS resolution `111`.
/// - The range and BCD errors of the code (TIME-021, TIME-030, TIME-034).
pub fn decode_with_pfield(bytes: &[u8], mode: DecodeMode) -> Result<Decoded> {
    let pfield = PField::parse(bytes)?;
    let mut anomalies = AnomalySet::empty();
    let format =
        match pfield.id() {
            TimeCodeId::CucLevel1 | TimeCodeId::CucLevel2 => {
                TimeFormat::Cuc(CucFormat::from_pfield(&pfield))
            }
            TimeCodeId::Cds => TimeFormat::Cds(CdsFormat::from_pfield(&pfield)?),
            TimeCodeId::Ccs => TimeFormat::Ccs(CcsFormat::from_pfield(&pfield)?),
            TimeCodeId::Agency => TimeFormat::Agency(AgencyFormat::from_pfield(&pfield)),
            TimeCodeId::Reserved000 | TimeCodeId::Reserved011 | TimeCodeId::Reserved111 => {
                return match mode {
                    DecodeMode::Strict => Err(Error::new(ErrorKind::ReservedValue)
                        .with_field(FieldId::new("time_code_id"))),
                    DecodeMode::Lenient => Ok(Decoded {
                        code: TimeCode::Unknown(ReservedPField::from_reserved(pfield)),
                        pfield: Some(pfield),
                        len: pfield.len(),
                        anomalies: AnomalySet::from_flags(&[TimeAnomaly::ReservedTimeCodeId]),
                    }),
                };
            }
        };
    if format.has_tail() {
        match mode {
            DecodeMode::Strict => {
                return Err(Error::new(ErrorKind::ReservedValue).with_field(FieldId::new("pfield")));
            }
            DecodeMode::Lenient => anomalies.insert(TimeAnomaly::UndefinedPFieldExtension),
        }
    }
    let t = t_field(bytes, pfield.len(), format.t_len())?;
    let code = decode_t(format, t, mode, &mut anomalies)?;
    Ok(Decoded {
        code,
        pfield: Some(pfield),
        len: pfield.len().saturating_add(t.len()),
        anomalies,
    })
}

/// Decodes a T-field at the start of `bytes` whose layout is known from management (implicit
/// P-field, 301.0 §3.1.1); octets after it are ignored (TIME-003).
///
/// # Errors
/// `Incomplete` if `bytes` is shorter than the T-field; the range and BCD errors of the code.
pub fn decode_implicit(bytes: &[u8], format: &TimeFormat, mode: DecodeMode) -> Result<Decoded> {
    let t = t_field(bytes, 0, format.t_len())?;
    let mut anomalies = AnomalySet::empty();
    let code = decode_t(*format, t, mode, &mut anomalies)?;
    Ok(Decoded {
        code,
        pfield: None,
        len: t.len(),
        anomalies,
    })
}

impl TimeCode {
    /// The format, or `None` for [`TimeCode::Unknown`] (301.0 §3.1.1, TIME-001).
    #[must_use]
    pub const fn format(&self) -> Option<TimeFormat> {
        match self {
            TimeCode::Cuc(c) => Some(TimeFormat::Cuc(c.format())),
            TimeCode::Cds(c) => Some(TimeFormat::Cds(c.format())),
            TimeCode::Ccs(c) => Some(TimeFormat::Ccs(c.format())),
            TimeCode::Agency(c) => Some(TimeFormat::Agency(c.format())),
            TimeCode::Unknown(_) => None,
        }
    }

    /// The P-field that describes this code (301.0 §3.1.1; TIME-001, TIME-002).
    #[must_use]
    pub fn pfield(&self) -> PField {
        match self {
            TimeCode::Cuc(c) => c.format().pfield(),
            TimeCode::Cds(c) => c.format().pfield(),
            TimeCode::Ccs(c) => c.format().pfield(),
            TimeCode::Agency(c) => c.format().pfield(),
            TimeCode::Unknown(p) => p.pfield(),
        }
    }

    /// The T-field octets and their count.
    fn t_octets(&self) -> ([u8; MAX_T_LEN], usize) {
        let mut out = [0_u8; MAX_T_LEN];
        let (src, n): (&[u8], usize) = match self {
            TimeCode::Cuc(c) => (&c.t_octets(), c.format().t_len()),
            TimeCode::Cds(c) => (&c.t_octets(), c.format().t_len()),
            TimeCode::Ccs(c) => (&c.t_octets(), c.format().t_len()),
            TimeCode::Agency(c) => (c.tfield(), c.format().t_len()),
            TimeCode::Unknown(_) => (&[], 0),
        };
        for (d, s) in out.iter_mut().zip(src) {
            *d = *s;
        }
        (out, n)
    }

    /// Encodes the code into the start of `out`, with its P-field first if `include_pfield`;
    /// returns the length written. Undefined P-field octets kept by a lenient decode are written
    /// back (301.0 §3.1, TIME-005).
    ///
    /// # Errors
    /// `BufferTooSmall` if `out` is too short (`out` is then unchanged); `InvalidField`
    /// (`pfield`) for [`TimeCode::Unknown`] without its P-field, which would write nothing.
    pub fn encode(&self, include_pfield: bool, out: &mut [u8]) -> Result<usize> {
        if !include_pfield && matches!(self, TimeCode::Unknown(_)) {
            return Err(Error::new(ErrorKind::InvalidField).with_field(FieldId::new("pfield")));
        }
        let pfield = self.pfield();
        let (p_octets, p_len) = if include_pfield {
            (pfield.octets(), pfield.len())
        } else {
            ([0; MAX_PFIELD_LEN], 0)
        };
        let (t_octets, t_len) = self.t_octets();
        let total = p_len.saturating_add(t_len);
        let dst = out.get_mut(..total).ok_or(Error::buffer_too_small(total))?;
        let octets = p_octets
            .iter()
            .take(p_len)
            .chain(t_octets.iter().take(t_len));
        for (d, s) in dst.iter_mut().zip(octets) {
            *d = *s;
        }
        Ok(total)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        Agency, AgencyFormat, Decoded, TimeCode, TimeFormat, decode_implicit, decode_with_pfield,
    };
    use crate::anomaly::TimeAnomaly;
    use crate::calendar::{CalendarDate, Hour, Minute, Month, Second, Year};
    use crate::ccs::{Ccs, CcsFormat, CcsValue, CcsVariation};
    use crate::cds::{
        Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, MsOfDay, Picos, SubMillis,
    };
    use crate::cuc::{Cuc, CucEpoch, CucFormat, CucValue};
    use crate::pfield::{PField, PFieldTail};
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind, FieldId};
    use proptest::prelude::*;

    fn samples() -> [TimeCode; 4] {
        let tail = PFieldTail::new(&[0x01]).unwrap();
        let cuc = CucFormat::new(CucEpoch::Agency, 2, 1).unwrap();
        let cds =
            CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits24, CdsSubMs::Picos).with_tail(tail);
        let ccs = CcsFormat::new(CcsVariation::MonthDay, 1).unwrap();
        let agency = AgencyFormat::new(3).unwrap().with_tail(tail);
        [
            TimeCode::Cuc(Cuc::new(cuc, CucValue::new(0x0102, 3 << 120)).unwrap()),
            TimeCode::Cds(
                Cds::new(
                    cds,
                    CdsValue {
                        day: 5,
                        ms_of_day: MsOfDay::new(6).unwrap(),
                        subms: SubMillis::Picos(Picos::new(7).unwrap()),
                    },
                )
                .unwrap(),
            ),
            TimeCode::Ccs(
                Ccs::new(
                    ccs,
                    CcsValue {
                        date: CalendarDate::from_month_day(
                            Year::new(2001).unwrap(),
                            Month::new(2).unwrap(),
                            3,
                        )
                        .unwrap(),
                        hour: Hour::new(4).unwrap(),
                        minute: Minute::new(5).unwrap(),
                        second: Second::new(6).unwrap(),
                        subsecond: 78,
                    },
                )
                .unwrap(),
            ),
            TimeCode::Agency(Agency::new(agency, &[0xA, 0xB, 0xC]).unwrap()),
        ]
    }

    #[test]
    fn agency_format_and_value() {
        // req: TIME-006
        for bad in [0, 17] {
            assert_eq!(
                AgencyFormat::new(bad).unwrap_err().kind(),
                ErrorKind::OutOfRange
            );
        }
        let f = AgencyFormat::new(16).unwrap();
        assert_eq!(
            (f.t_len(), f.tail(), f.pfield().first()),
            (16, PFieldTail::EMPTY, 0x6F)
        );
        let e = Agency::new(f, &[1, 2]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::LengthMismatch, Some("tfield"))
        );
        let a = Agency::new(AgencyFormat::new(2).unwrap(), &[1, 2]).unwrap();
        assert_eq!((a.tfield(), a.format().t_len()), (&[1, 2][..], 2));
    }

    #[test]
    fn errors_report_the_violated_bound() {
        // req: TIME-006, GEN-041
        for (t_len, expected) in [(0, 1), (17, 16)] {
            let e = AgencyFormat::new(t_len).unwrap_err();
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected {
                    expected,
                    found: u64::from(t_len)
                }
            );
        }
        let e = Agency::new(AgencyFormat::new(4).unwrap(), &[1, 2]).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 4,
                found: 2
            }
        );
    }

    #[test]
    fn every_code_round_trips_with_and_without_pfield() {
        // req: TIME-001, TIME-003, TIME-005, API-020
        for code in samples() {
            let format = code.format().unwrap();
            assert_eq!(format.pfield(), code.pfield());
            let mut out = [0_u8; 32];
            let n = code.encode(true, &mut out).unwrap();
            let lenient = decode_with_pfield(&out[..n], DecodeMode::Lenient).unwrap();
            assert_eq!(
                (lenient.code(), lenient.len(), lenient.is_empty()),
                (code, n, false)
            );
            assert_eq!(lenient.pfield(), Some(code.pfield()));
            let tail = matches!(format, TimeFormat::Cds(_) | TimeFormat::Agency(_));
            assert_eq!(
                lenient
                    .anomalies()
                    .contains(TimeAnomaly::UndefinedPFieldExtension),
                tail
            );
            let strict = decode_with_pfield(&out[..n], DecodeMode::Strict);
            if tail {
                let e = strict.unwrap_err();
                assert_eq!(
                    (e.kind(), e.field().map(FieldId::name)),
                    (ErrorKind::ReservedValue, Some("pfield"))
                );
            } else {
                assert_eq!(strict.unwrap().code(), code);
            }
            let m = code.encode(false, &mut out).unwrap();
            assert_eq!(m, format.t_len());
            let implicit = decode_implicit(&out[..m], &format, DecodeMode::Strict).unwrap();
            assert_eq!(
                (implicit.code(), implicit.len(), implicit.pfield()),
                (code, m, None)
            );
            let e = code.encode(true, &mut out[..n - 1]).unwrap_err();
            assert_eq!(
                (e.kind(), e.detail()),
                (ErrorKind::BufferTooSmall, ErrorDetail::Needed(n))
            );
        }
    }

    #[test]
    fn reserved_ids_and_unknown_codes() {
        // req: TIME-002, TIME-003, API-032
        let e = decode_with_pfield(&[0x30, 0], DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::ReservedValue, Some("time_code_id"))
        );
        let d: Decoded = decode_with_pfield(&[0xB0, 0x01, 0xFF], DecodeMode::Lenient).unwrap();
        assert_eq!(d.len(), 2);
        assert!(d.anomalies().contains(TimeAnomaly::ReservedTimeCodeId));
        let TimeCode::Unknown(reserved) = d.code() else {
            panic!("not unknown")
        };
        let p = reserved.pfield();
        assert_eq!(d.code().format(), None);
        assert_eq!(d.code().pfield(), p);
        let mut out = [0_u8; 4];
        assert_eq!(d.code().encode(true, &mut out), Ok(2));
        assert_eq!(out[..2], [0xB0, 0x01]);
        let e = d.code().encode(false, &mut out).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::InvalidField, Some("pfield"))
        );
        assert_eq!(PField::parse(&[0xB0, 0x01]).unwrap(), p);
    }

    #[test]
    fn decode_errors() {
        // req: TIME-003, TIME-020, TIME-030
        let e = decode_with_pfield(&[0x1E, 0, 0], DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(7))
        );
        assert_eq!(
            decode_with_pfield(&[0x43], DecodeMode::Lenient)
                .unwrap_err()
                .kind(),
            ErrorKind::ReservedValue
        );
        assert_eq!(
            decode_with_pfield(&[0x57], DecodeMode::Lenient)
                .unwrap_err()
                .kind(),
            ErrorKind::ReservedValue
        );
        let cds = TimeFormat::Cds(CdsFormat::new(
            CdsEpoch::Ccsds1958,
            CdsDayLen::Bits16,
            CdsSubMs::None,
        ));
        let e = decode_implicit(&[0, 0, 0xFF, 0xFF, 0xFF, 0xFF], &cds, DecodeMode::Lenient)
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        assert_eq!(
            decode_implicit(&[0], &cds, DecodeMode::Strict)
                .unwrap_err()
                .kind(),
            ErrorKind::Incomplete
        );
        assert_eq!(cds.t_len(), 6);
    }

    proptest! {
        #[test]
        fn arbitrary_input(bytes in proptest::collection::vec(any::<u8>(), 0..32)) {
            // req: GEN-020, API-010, API-020, API-030, VER-023
            let strict = decode_with_pfield(&bytes, DecodeMode::Strict);
            let lenient = decode_with_pfield(&bytes, DecodeMode::Lenient);
            match (&strict, &lenient) {
                (Ok(s), Ok(l)) => prop_assert_eq!(s, l),
                (Ok(_), Err(e)) => prop_assert!(false, "lenient rejected a strict code: {}", e),
                (Err(s), Ok(l)) => {
                    prop_assert_eq!(s.kind(), ErrorKind::ReservedValue);
                    prop_assert!(!l.anomalies().is_empty());
                }
                // Strict stops at the first reserved value; lenient may fail later on.
                (Err(s), Err(l)) => prop_assert!(s == l || s.kind() == ErrorKind::ReservedValue),
            }
            if let Ok(d) = lenient {
                prop_assert!(d.len() <= bytes.len());
                // A lenient decode re-encodes to a code that decodes the same (API-020, TIME-005).
                // The octets may differ: a CUC count split re-encodes canonically, and a CCS
                // day-of-year high nibble is written as zero.
                let mut out = [0_u8; 32];
                let n = d.code().encode(true, &mut out).unwrap();
                let again = decode_with_pfield(&out[..n], DecodeMode::Lenient).unwrap();
                prop_assert_eq!(again.code(), d.code());
                prop_assert_eq!(again.len(), n);
            }
        }
    }
}
