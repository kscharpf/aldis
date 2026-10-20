//! CCSDS Unsegmented Code, CUC (301.0 §3.2; TIME-010…TIME-014).
//!
//! Level 1 CUC counts TAI seconds from 1958 January 1. It is not UTC-based, and the library
//! applies no leap-second correction to it (TIME-014). Level 2 uses an agency-defined epoch;
//! Aldis treats its value the same way and does not interpret the epoch.

use aldis_core::{Error, ErrorKind, FieldId, Result};

use crate::pfield::{PField, PFieldTail, TimeCodeId};
use crate::util::wide8;

/// Nanoseconds per second.
const NANOS: u32 = 1_000_000_000;

/// The epoch of a CUC code (301.0 §3.2.2, TIME-010, TIME-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CucEpoch {
    /// 1958 January 1 TAI, Level 1 (time code ID `001`) (301.0 §3.2.2, TIME-014).
    Tai1958,
    /// Agency-defined epoch, Level 2 (time code ID `010`) (301.0 §3.2.2).
    Agency,
}

/// CUC layout: epoch, coarse and fine octet counts, and the P-field details (301.0 §3.2.2;
/// TIME-010, TIME-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CucFormat {
    epoch: CucEpoch,
    coarse: u8,
    fine: u8,
    mission: u8,
    octet2: bool,
    tail: PFieldTail,
}

/// `OutOfRange` for `field` with its limit and the value found.
const fn range(field: &'static str, limit: u64, found: u64) -> Error {
    Error::out_of_range(field, limit, found)
}

impl CucFormat {
    /// Most coarse (basic time) octets: 4 in octet 1, 3 more in octet 2 (301.0 §3.2.2, TIME-010).
    pub const MAX_COARSE: u8 = 7;
    /// Most fine (fractional) octets: 3 in octet 1, 7 more in octet 2 (301.0 §3.2.2, TIME-010).
    pub const MAX_FINE: u8 = 10;

    /// A format with `coarse_octets` (1–7) and `fine_octets` (0–10). P-field octet 2 is used
    /// only when the counts need it (above 4 coarse or 3 fine octets).
    ///
    /// # Errors
    /// `OutOfRange` (`coarse_octets` or `fine_octets`) outside those ranges (301.0 §3.2.2,
    /// TIME-010).
    pub const fn new(epoch: CucEpoch, coarse_octets: u8, fine_octets: u8) -> Result<CucFormat> {
        if coarse_octets == 0 || coarse_octets > CucFormat::MAX_COARSE {
            // Report the violated bound: 1 for zero, the maximum otherwise (GEN-041).
            let limit = if coarse_octets == 0 {
                1
            } else {
                CucFormat::MAX_COARSE
            };
            return Err(Error::out_of_range(
                "coarse_octets",
                wide8(limit),
                wide8(coarse_octets),
            ));
        }
        if fine_octets > CucFormat::MAX_FINE {
            return Err(Error::out_of_range(
                "fine_octets",
                wide8(CucFormat::MAX_FINE),
                wide8(fine_octets),
            ));
        }
        Ok(CucFormat {
            epoch,
            coarse: coarse_octets,
            fine: fine_octets,
            mission: 0,
            octet2: coarse_octets > 4 || fine_octets > 3,
            tail: PFieldTail::EMPTY,
        })
    }

    /// The same format with the mission-defined bits of P-field octet 2 (0–3), which forces
    /// octet 2 (301.0 §3.2.2, TIME-013).
    ///
    /// # Errors
    /// `OutOfRange` (`mission_bits`) above 3.
    pub const fn with_mission_bits(self, bits: u8) -> Result<CucFormat> {
        if bits > 0b11 {
            return Err(Error::out_of_range("mission_bits", 3, wide8(bits)));
        }
        Ok(CucFormat {
            mission: bits,
            octet2: true,
            ..self
        })
    }

    /// The same format with P-field octet 2 present even if the counts do not need it (301.0
    /// §3.2.2, TIME-013).
    #[must_use]
    pub const fn with_octet2(self) -> CucFormat {
        CucFormat {
            octet2: true,
            ..self
        }
    }

    /// The same format with undefined P-field octets after octet 2, which forces octet 2
    /// (301.0 §3.2.2, TIME-005).
    ///
    /// # Errors
    /// `InvalidField` (`pfield_tail`) if the tail is longer than 6 octets, which would make the
    /// P-field longer than 8.
    pub fn with_tail(self, tail: PFieldTail) -> Result<CucFormat> {
        if tail.len() > 6 {
            return Err(Error::new(ErrorKind::InvalidField).with_field(FieldId::new("pfield_tail")));
        }
        Ok(CucFormat {
            tail,
            octet2: true,
            ..self
        })
    }

    /// The epoch (301.0 §3.2.2, TIME-010).
    #[must_use]
    pub const fn epoch(&self) -> CucEpoch {
        self.epoch
    }

    /// Number of coarse octets, 1–7 (301.0 §3.2.2, TIME-010).
    #[must_use]
    pub const fn coarse_octets(&self) -> u8 {
        self.coarse
    }

    /// Number of fine octets, 0–10 (301.0 §3.2.2, TIME-010).
    #[must_use]
    pub const fn fine_octets(&self) -> u8 {
        self.fine
    }

    /// Mission-defined bits of P-field octet 2 (301.0 §3.2.2, TIME-013).
    #[must_use]
    pub const fn mission_bits(&self) -> u8 {
        self.mission
    }

    /// Whether the P-field has octet 2 (301.0 §3.2.2, TIME-010).
    #[must_use]
    pub const fn has_octet2(&self) -> bool {
        self.octet2
    }

    /// Undefined P-field octets after octet 2 (301.0 §3.2.2, TIME-005).
    #[must_use]
    pub const fn tail(&self) -> PFieldTail {
        self.tail
    }

    /// T-field length in octets (301.0 §3.2.1, TIME-003).
    #[must_use]
    pub fn t_len(&self) -> usize {
        usize::from(self.coarse).wrapping_add(usize::from(self.fine))
    }

    /// The P-field for this format (301.0 §3.2.2, TIME-010, TIME-013). Octet 1 carries as many
    /// coarse (up to 4) and fine (up to 3) octets as it can and octet 2 the rest. A received
    /// P-field that splits the counts differently decodes to the same format and re-encodes in this
    /// canonical form.
    #[must_use]
    pub fn pfield(&self) -> PField {
        let id = match self.epoch {
            CucEpoch::Tai1958 => TimeCodeId::CucLevel1,
            CucEpoch::Agency => TimeCodeId::CucLevel2,
        };
        let c1 = self.coarse.min(4);
        let f1 = self.fine.min(3);
        let first = (id.bits() << 4) | (c1.wrapping_sub(1) << 2) | f1;
        if self.has_octet2() {
            let octet2 = (self.coarse.wrapping_sub(c1) << 5)
                | (self.fine.wrapping_sub(f1) << 2)
                | self.mission;
            PField::build(first, &[octet2], self.tail)
        } else {
            PField::build(first, &[], PFieldTail::EMPTY)
        }
    }

    /// The format a CUC P-field describes; the caller has checked the time code ID.
    pub(crate) fn from_pfield(pfield: &PField) -> CucFormat {
        let first = pfield.first();
        let epoch = match pfield.id() {
            TimeCodeId::CucLevel1 => CucEpoch::Tai1958,
            _ => CucEpoch::Agency,
        };
        let coarse = ((first >> 2) & 0b11).wrapping_add(1);
        let fine = first & 0b11;
        match pfield.rest() {
            [] => CucFormat {
                epoch,
                coarse,
                fine,
                mission: 0,
                octet2: false,
                tail: PFieldTail::EMPTY,
            },
            [octet2, tail @ ..] => CucFormat {
                epoch,
                coarse: coarse.wrapping_add((octet2 >> 5) & 0b11),
                fine: fine.wrapping_add((octet2 >> 2) & 0b111),
                mission: octet2 & 0b11,
                octet2: true,
                tail: PFieldTail::from_chain(tail),
            },
        }
    }
}

/// A CUC value: coarse time and a left-aligned binary fraction of the basic time unit (301.0
/// §3.2.1; TIME-011, M1 D2). Fraction bit 127 is worth ½; a format with `n` fine octets uses the
/// top `8n` bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CucValue {
    coarse: u64,
    fine: u128,
}

impl CucValue {
    /// A value of `coarse` units and the left-aligned fraction `fine` (301.0 §3.2.1, TIME-011).
    #[must_use]
    pub const fn new(coarse: u64, fine: u128) -> CucValue {
        CucValue { coarse, fine }
    }

    /// Coarse time (301.0 §3.2.1, TIME-011).
    #[must_use]
    pub const fn coarse(&self) -> u64 {
        self.coarse
    }

    /// Left-aligned fraction (301.0 §3.2.1, TIME-011).
    #[must_use]
    pub const fn fine(&self) -> u128 {
        self.fine
    }

    /// The value as whole seconds and nanoseconds, truncated toward zero (301.0 §3.2.1, TIME-011).
    ///
    /// For 0–3 fine octets the round trip through [`Cuc::from_seconds_nanos`] is exact. With 4
    /// or more fine octets the fraction is finer than a nanosecond, so the round trip is exact
    /// only to the nanosecond.
    #[must_use]
    pub fn to_seconds_nanos(&self) -> (u64, u32) {
        // floor(fine × 10⁹ / 2¹²⁸) from the two 64-bit halves; each product is below 2⁹⁴.
        let hi = self.fine >> 64;
        let lo = self.fine & u128::from(u64::MAX);
        let nanos = u128::from(NANOS);
        let sum = hi
            .wrapping_mul(nanos)
            .wrapping_add(lo.wrapping_mul(nanos) >> 64);
        (self.coarse, u32::try_from(sum >> 64).unwrap_or(u32::MAX))
    }
}

/// `OutOfRange` (`coarse`) unless `coarse` fits the format's coarse octets (TIME-012).
fn check_coarse(format: &CucFormat, coarse: u64) -> Result<()> {
    let bits = u32::from(format.coarse).wrapping_mul(8);
    let max = u64::MAX
        .checked_shr(64_u32.wrapping_sub(bits))
        .unwrap_or(u64::MAX);
    if coarse > max {
        return Err(range("coarse", max, coarse));
    }
    Ok(())
}

/// A CUC time code: format and value (301.0 §3.2, TIME-010, TIME-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cuc {
    format: CucFormat,
    value: CucValue,
}

impl Cuc {
    /// A CUC code; the value must be representable in the format (301.0 §3.2.1; TIME-011,
    /// TIME-012).
    ///
    /// # Errors
    /// `OutOfRange` (`coarse`) if the coarse time needs more octets than the format has; it is
    /// never wrapped (TIME-012). `OutOfRange` (`fine`) if the fraction has bits below the
    /// format's resolution.
    pub fn new(format: CucFormat, value: CucValue) -> Result<Cuc> {
        check_coarse(&format, value.coarse)?;
        let below = u128::MAX
            .checked_shr(u32::from(format.fine).wrapping_mul(8))
            .unwrap_or(0);
        if value.fine & below != 0 {
            return Err(Error::new(ErrorKind::OutOfRange).with_field(FieldId::new("fine")));
        }
        Ok(Cuc { format, value })
    }

    /// The code for `seconds` and `nanos` in `format`: the fraction is rounded **up** to the next
    /// representable value (301.0 §3.2.1, TIME-011), and a carry adds one to the coarse time.
    /// Returns a [`Cuc`], so the fit to the format is established once (RS-020).
    ///
    /// # Errors
    /// `OutOfRange` (`nanoseconds`) if `nanos` ≥ 10⁹; `OutOfRange` (`coarse`) if the coarse time,
    /// after any carry, does not fit the format (TIME-012).
    pub fn from_seconds_nanos(format: CucFormat, seconds: u64, nanos: u32) -> Result<Cuc> {
        if nanos >= NANOS {
            return Err(range("nanoseconds", 999_999_999, u64::from(nanos)));
        }
        let bits = u32::from(format.fine).wrapping_mul(8);
        // nanos < 2³⁰ and bits ≤ 80, so the shifted value stays below 2¹¹⁰.
        let scaled = (u128::from(nanos) << bits).div_ceil(u128::from(NANOS));
        let (coarse, raw) = if scaled == 1_u128 << bits {
            let carried = seconds
                .checked_add(1)
                .ok_or_else(|| range("coarse", u64::MAX, u64::MAX))?;
            (carried, 0)
        } else {
            (seconds, scaled)
        };
        check_coarse(&format, coarse)?;
        // The rounded fraction has no bits below the format's resolution.
        Ok(Cuc {
            format,
            value: CucValue {
                coarse,
                fine: raw.checked_shl(128_u32.wrapping_sub(bits)).unwrap_or(0),
            },
        })
    }

    /// The code as whole seconds and nanoseconds, truncated toward zero; the counterpart of
    /// [`from_seconds_nanos`](Self::from_seconds_nanos) (301.0 §3.2.1, TIME-011). See
    /// [`CucValue::to_seconds_nanos`].
    #[must_use]
    pub fn to_seconds_nanos(&self) -> (u64, u32) {
        self.value.to_seconds_nanos()
    }

    /// The format (301.0 §3.2, TIME-010, TIME-011).
    #[must_use]
    pub const fn format(&self) -> CucFormat {
        self.format
    }

    /// The value (301.0 §3.2, TIME-010, TIME-011).
    #[must_use]
    pub const fn value(&self) -> CucValue {
        self.value
    }

    /// The T-field octets; the first [`CucFormat::t_len`] are used.
    pub(crate) fn t_octets(&self) -> [u8; 17] {
        let mut out = [0_u8; 17];
        let coarse = self.value.coarse.to_be_bytes();
        let fine = self.value.fine.to_be_bytes();
        let skip = 8_usize.wrapping_sub(usize::from(self.format.coarse));
        let octets = coarse
            .iter()
            .skip(skip)
            .chain(fine.iter().take(usize::from(self.format.fine)));
        for (d, s) in out.iter_mut().zip(octets) {
            *d = *s;
        }
        out
    }

    /// Decodes a T-field of exactly [`CucFormat::t_len`] octets.
    pub(crate) fn from_t(format: CucFormat, t: &[u8]) -> Cuc {
        let co = usize::from(format.coarse);
        let coarse = t
            .iter()
            .take(co)
            .fold(0_u64, |acc, &b| (acc << 8) | u64::from(b));
        let raw = t
            .iter()
            .skip(co)
            .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
        let fine = raw
            .checked_shl(128_u32.wrapping_sub(u32::from(format.fine).wrapping_mul(8)))
            .unwrap_or(0);
        Cuc {
            format,
            value: CucValue { coarse, fine },
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test models on bounded values (RS-018)"
)]
mod tests {
    use super::{Cuc, CucEpoch, CucFormat, CucValue};
    use crate::pfield::PFieldTail;
    use aldis_core::ErrorKind;
    use proptest::prelude::*;

    fn fmt(co: u8, fo: u8) -> CucFormat {
        CucFormat::new(CucEpoch::Tai1958, co, fo).unwrap()
    }

    #[test]
    fn format_limits_and_accessors() {
        // req: TIME-010, TIME-013
        for (co, fo) in [(0, 0), (8, 0), (1, 11)] {
            assert_eq!(
                CucFormat::new(CucEpoch::Agency, co, fo).unwrap_err().kind(),
                ErrorKind::OutOfRange
            );
        }
        assert_eq!(
            fmt(1, 0).with_mission_bits(4).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        let tail6 = PFieldTail::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]).unwrap();
        assert_eq!(
            fmt(1, 0).with_tail(tail6).map(|f| f.tail()),
            Ok(tail6),
            "6 octets fit"
        );
        let tail7 = PFieldTail::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00]).unwrap();
        assert_eq!(
            fmt(1, 0).with_tail(tail7).unwrap_err().kind(),
            ErrorKind::InvalidField
        );
        let f = fmt(3, 2).with_mission_bits(2).unwrap();
        assert_eq!(
            (
                f.epoch(),
                f.coarse_octets(),
                f.fine_octets(),
                f.mission_bits()
            ),
            (CucEpoch::Tai1958, 3, 2, 2)
        );
        assert!(f.has_octet2() && f.tail().is_empty());
        assert_eq!(f.t_len(), 5);
        assert!(!fmt(4, 3).has_octet2());
        assert!(
            fmt(5, 0).has_octet2()
                && fmt(1, 4).has_octet2()
                && fmt(1, 0).with_octet2().has_octet2()
        );
        let t = PFieldTail::new(&[0x01]).unwrap();
        let ft = fmt(1, 0).with_tail(t).unwrap();
        assert!(ft.has_octet2() && ft.tail() == t);
    }

    #[test]
    fn errors_report_the_violated_bound() {
        // req: TIME-010, GEN-041
        use aldis_core::ErrorDetail;
        for (co, fo, expected, found) in [(0, 0, 1, 0), (8, 0, 7, 8), (1, 11, 10, 11)] {
            let e = CucFormat::new(CucEpoch::Tai1958, co, fo).unwrap_err();
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected { expected, found },
                "{co} {fo}"
            );
        }
    }

    #[test]
    fn pfield_round_trip() {
        // req: TIME-001, TIME-010, TIME-013
        let tail = PFieldTail::new(&[0x80, 0x05]).unwrap();
        for epoch in [CucEpoch::Tai1958, CucEpoch::Agency] {
            for co in 1..=7 {
                for fo in 0..=10 {
                    let f = CucFormat::new(epoch, co, fo).unwrap();
                    for f in [
                        f,
                        f.with_octet2(),
                        f.with_mission_bits(3).unwrap(),
                        f.with_tail(tail).unwrap(),
                    ] {
                        assert_eq!(CucFormat::from_pfield(&f.pfield()), f, "{f:?}");
                    }
                }
            }
        }
        assert_eq!(fmt(4, 2).pfield().first(), 0x1E);
    }

    #[test]
    fn nanosecond_conversions() {
        // req: TIME-011, TIME-012
        assert_eq!(
            CucValue::new(7, 1 << 127).to_seconds_nanos(),
            (7, 500_000_000)
        );
        assert_eq!(
            CucValue::new(0, u128::MAX).to_seconds_nanos(),
            (0, 999_999_999)
        );
        assert_eq!(CucValue::default().to_seconds_nanos(), (0, 0));
        let c = Cuc::from_seconds_nanos(fmt(1, 1), 5, 999_999_999).unwrap();
        assert_eq!(
            (c.format(), c.value().coarse(), c.value().fine()),
            (fmt(1, 1), 6, 0)
        );
        let v = Cuc::from_seconds_nanos(fmt(1, 0), 5, 1).unwrap().value();
        assert_eq!(
            (v.coarse(), v.fine()),
            (6, 0),
            "no fine octets: any fraction rounds up"
        );
        assert_eq!(
            Cuc::from_seconds_nanos(fmt(1, 0), 5, 0)
                .unwrap()
                .value()
                .coarse(),
            5
        );
        let e = Cuc::from_seconds_nanos(fmt(7, 1), u64::MAX, 999_999_999).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        let e = Cuc::from_seconds_nanos(fmt(7, 1), 0, 1_000_000_000).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        let e = Cuc::from_seconds_nanos(fmt(1, 0), 256, 0).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        assert_eq!(
            Cuc::from_seconds_nanos(fmt(7, 0), 1 << 56, 0)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        assert!(Cuc::from_seconds_nanos(fmt(7, 0), (1 << 56) - 1, 0).is_ok());
    }

    #[test]
    fn cuc_round_trips_through_seconds_and_nanos() {
        // req: TIME-011
        let carried = Cuc::from_seconds_nanos(fmt(1, 1), 5, 999_999_999).unwrap();
        assert_eq!(carried.to_seconds_nanos(), (6, 0));
        let half = Cuc::from_seconds_nanos(fmt(1, 1), 5, 500_000_000).unwrap();
        assert_eq!(half.to_seconds_nanos(), (5, 500_000_000));
        assert_eq!(half.to_seconds_nanos(), half.value().to_seconds_nanos());
    }

    #[test]
    fn cuc_checks_the_value() {
        // req: TIME-012
        assert_eq!(
            Cuc::new(fmt(1, 0), CucValue::new(256, 0))
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            Cuc::new(fmt(1, 1), CucValue::new(0, 1 << 119))
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            Cuc::new(fmt(1, 0), CucValue::new(0, 1)).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        let c = Cuc::new(fmt(2, 1), CucValue::new(0x0102, 3 << 120)).unwrap();
        assert_eq!((c.format(), c.value().coarse()), (fmt(2, 1), 0x0102));
        assert_eq!(c.t_octets()[..3], [1, 2, 3]);
        assert_eq!(Cuc::from_t(fmt(2, 1), &[1, 2, 3]), c);
        let full = Cuc::new(fmt(7, 10), CucValue::new((1 << 56) - 1, u128::MAX << 48)).unwrap();
        assert_eq!(Cuc::from_t(fmt(7, 10), &full.t_octets()), full);
    }

    proptest! {
        #[test]
        fn ns_round_trip(co in 1_u8..=7, fo in 0_u8..=10, secs in any::<u64>(), ns in 0_u32..1_000_000_000) {
            // req: TIME-011, VER-023
            let f = fmt(co, fo);
            let secs = secs >> (64 - 8 * u32::from(co));
            let Ok(c) = Cuc::from_seconds_nanos(f, secs, ns) else { return Ok(()) };
            prop_assert_eq!(Cuc::new(f, c.value()), Ok(c), "rounded value is representable");
            let (s2, ns2) = c.value().to_seconds_nanos();
            // Rounding up never lowers the time, and moves it by less than one fine step.
            let before = u128::from(secs) * 1_000_000_000 + u128::from(ns);
            let after = u128::from(s2) * 1_000_000_000 + u128::from(ns2);
            prop_assert!(after + 1 >= before);
            let step = 1_000_000_000_u128 >> (8 * u32::from(fo)).min(30);
            prop_assert!(after <= before + step.max(1));
            if fo <= 3 {
                // Exact round trip of the value through nanoseconds (TIME-011).
                prop_assert_eq!(Cuc::from_seconds_nanos(f, s2, ns2), Ok(c));
            }
        }
    }
}
