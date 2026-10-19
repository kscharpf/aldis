//! CCSDS Day Segmented Code, CDS (301.0 §3.3, Annex A; TIME-020…TIME-022).
//!
//! CDS is UTC-based: the milliseconds-of-day segment runs to 86 400 999 on a day with a
//! positive leap second. Leap-second handling is the caller's responsibility; Aldis checks
//! only the counter ranges (TIME-022, `docs/standards/time-codes.md` §6).

use aldis_core::{Error, ErrorKind, FieldId, Result};

use crate::pfield::{PField, PFieldTail, TimeCodeId};
use crate::util::wide;

/// Milliseconds of day allow one positive leap second: 0 to 86 400 999 (301.0 Annex A).
pub const MAX_MS_OF_DAY: u32 = 86_400_999;

/// The epoch of a CDS code (301.0 §3.3.2 bit 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CdsEpoch {
    /// 1958 January 1 (Level 1) (301.0 §3.3.2).
    Ccsds1958,
    /// Agency-defined epoch (Level 2) (301.0 §3.3.2).
    Agency,
}

/// Length of the day segment (301.0 §3.3.2 bit 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CdsDayLen {
    /// 16-bit day count (301.0 §3.3.2, TIME-020).
    Bits16,
    /// 24-bit day count (301.0 §3.3.2, TIME-020).
    Bits24,
}

/// Resolution of the submillisecond segment (301.0 §3.3.2 bits 6–7). Code `11` is reserved and
/// is an error in both modes (TIME-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CdsSubMs {
    /// No submillisecond segment (301.0 §3.3.2, TIME-020).
    None,
    /// 16-bit microseconds of millisecond, 0–999 (301.0 Annex A, TIME-020).
    Micros,
    /// 32-bit picoseconds of millisecond, 0–999 999 999 (301.0 Annex A, TIME-020).
    Picos,
}

impl CdsSubMs {
    /// Segment length in octets.
    const fn octets(self) -> usize {
        match self {
            CdsSubMs::None => 0,
            CdsSubMs::Micros => 2,
            CdsSubMs::Picos => 4,
        }
    }

    /// Largest valid segment value (301.0 Annex A).
    #[must_use]
    pub const fn max(self) -> u32 {
        match self {
            CdsSubMs::None => 0,
            CdsSubMs::Micros => 999,
            CdsSubMs::Picos => 999_999_999,
        }
    }
}

/// CDS layout (TIME-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CdsFormat {
    epoch: CdsEpoch,
    day: CdsDayLen,
    subms: CdsSubMs,
    tail: PFieldTail,
}

impl CdsFormat {
    /// A format; every combination of day length and resolution is valid (TIME-020).
    #[must_use]
    pub const fn new(epoch: CdsEpoch, day: CdsDayLen, subms: CdsSubMs) -> CdsFormat {
        CdsFormat {
            epoch,
            day,
            subms,
            tail: PFieldTail::EMPTY,
        }
    }

    /// The same format with undefined P-field octets after octet 1 (TIME-005).
    #[must_use]
    pub const fn with_tail(self, tail: PFieldTail) -> CdsFormat {
        CdsFormat { tail, ..self }
    }

    /// The epoch (301.0 §3.3.2, TIME-020).
    #[must_use]
    pub const fn epoch(&self) -> CdsEpoch {
        self.epoch
    }

    /// The day segment length (301.0 §3.3.2, TIME-020).
    #[must_use]
    pub const fn day_len(&self) -> CdsDayLen {
        self.day
    }

    /// The submillisecond resolution (301.0 §3.3.2, TIME-020).
    #[must_use]
    pub const fn subms(&self) -> CdsSubMs {
        self.subms
    }

    /// Undefined P-field octets (TIME-005).
    #[must_use]
    pub const fn tail(&self) -> PFieldTail {
        self.tail
    }

    /// Day segment length in octets.
    const fn day_octets(&self) -> usize {
        match self.day {
            CdsDayLen::Bits16 => 2,
            CdsDayLen::Bits24 => 3,
        }
    }

    /// T-field length in octets (301.0 §3.3.1, TIME-003).
    #[must_use]
    pub fn t_len(&self) -> usize {
        self.day_octets()
            .wrapping_add(4)
            .wrapping_add(self.subms.octets())
    }

    /// The P-field for this format (301.0 §3.3.2).
    #[must_use]
    pub fn pfield(&self) -> PField {
        let epoch = match self.epoch {
            CdsEpoch::Ccsds1958 => 0,
            CdsEpoch::Agency => 0b1000,
        };
        let day = match self.day {
            CdsDayLen::Bits16 => 0,
            CdsDayLen::Bits24 => 0b0100,
        };
        let subms = match self.subms {
            CdsSubMs::None => 0b00,
            CdsSubMs::Micros => 0b01,
            CdsSubMs::Picos => 0b10,
        };
        PField::build(
            (TimeCodeId::Cds.bits() << 4) | epoch | day | subms,
            &[],
            self.tail,
        )
    }

    /// The format a CDS P-field describes.
    ///
    /// # Errors
    /// `ReservedValue` (`subms`) for submillisecond code `11`, in both modes (TIME-020).
    pub(crate) fn from_pfield(pfield: &PField) -> Result<CdsFormat> {
        let first = pfield.first();
        let subms = match first & 0b11 {
            0b00 => CdsSubMs::None,
            0b01 => CdsSubMs::Micros,
            0b10 => CdsSubMs::Picos,
            _ => {
                return Err(Error::new(ErrorKind::ReservedValue).with_field(FieldId::new("subms")));
            }
        };
        Ok(CdsFormat {
            epoch: if first & 0b1000 == 0 {
                CdsEpoch::Ccsds1958
            } else {
                CdsEpoch::Agency
            },
            day: if first & 0b0100 == 0 {
                CdsDayLen::Bits16
            } else {
                CdsDayLen::Bits24
            },
            subms,
            tail: PFieldTail::from_chain(pfield.rest()),
        })
    }
}

/// A CDS value: day count, milliseconds of day, and submilliseconds (0 when the format has (301.0
/// §3.3.1)
/// none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CdsValue {
    /// Days since the epoch (301.0 §3.3.1, Annex A).
    pub day: u32,
    /// Milliseconds of the day, 0 to [`MAX_MS_OF_DAY`] (301.0 Annex A, TIME-021).
    pub ms_of_day: u32,
    /// Microseconds or picoseconds of the millisecond, per the format (301.0 Annex A, TIME-021).
    pub subms: u32,
}

/// A CDS time code: format and a value that fits it (301.0 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cds {
    format: CdsFormat,
    value: CdsValue,
}

impl Cds {
    /// A CDS code.
    ///
    /// # Errors
    /// `OutOfRange` (`day`) if the day does not fit the day segment; `OutOfRange`
    /// (`ms_of_day`) above [`MAX_MS_OF_DAY`]; `OutOfRange` (`subms`) above the resolution's
    /// maximum (TIME-021).
    pub const fn new(format: CdsFormat, value: CdsValue) -> Result<Cds> {
        let max_day = match format.day {
            CdsDayLen::Bits16 => 0xFFFF,
            CdsDayLen::Bits24 => 0xFF_FFFF,
        };
        if value.day > max_day {
            return Err(Error::out_of_range("day", wide(max_day), wide(value.day)));
        }
        if value.ms_of_day > MAX_MS_OF_DAY {
            return Err(Error::out_of_range(
                "ms_of_day",
                wide(MAX_MS_OF_DAY),
                wide(value.ms_of_day),
            ));
        }
        if value.subms > format.subms.max() {
            return Err(Error::out_of_range(
                "subms",
                wide(format.subms.max()),
                wide(value.subms),
            ));
        }
        Ok(Cds { format, value })
    }

    /// The format (301.0 §3.3).
    #[must_use]
    pub const fn format(&self) -> CdsFormat {
        self.format
    }

    /// The value (301.0 §3.3).
    #[must_use]
    pub const fn value(&self) -> CdsValue {
        self.value
    }

    /// The T-field octets; the first [`CdsFormat::t_len`] are used.
    pub(crate) fn t_octets(&self) -> [u8; 11] {
        let mut out = [0_u8; 11];
        let day = self.value.day.to_be_bytes();
        let ms = self.value.ms_of_day.to_be_bytes();
        let sub = self.value.subms.to_be_bytes();
        let octets = day
            .iter()
            .skip(4_usize.wrapping_sub(self.format.day_octets()))
            .chain(ms.iter())
            .chain(
                sub.iter()
                    .skip(4_usize.wrapping_sub(self.format.subms.octets())),
            );
        for (d, s) in out.iter_mut().zip(octets) {
            *d = *s;
        }
        out
    }

    /// Decodes a T-field of exactly [`CdsFormat::t_len`] octets and checks its ranges in both
    /// modes (TIME-021).
    pub(crate) fn from_t(format: CdsFormat, t: &[u8]) -> Result<Cds> {
        let mut octets = t.iter();
        let mut take = |n: usize| {
            octets
                .by_ref()
                .take(n)
                .fold(0_u32, |acc, &b| (acc << 8) | u32::from(b))
        };
        let day = take(format.day_octets());
        let ms_of_day = take(4);
        let subms = take(format.subms.octets());
        Cds::new(
            format,
            CdsValue {
                day,
                ms_of_day,
                subms,
            },
        )
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, MAX_MS_OF_DAY};
    use crate::pfield::{PField, PFieldTail};
    use aldis_core::{ErrorKind, FieldId};

    const SUBMS: [CdsSubMs; 3] = [CdsSubMs::None, CdsSubMs::Micros, CdsSubMs::Picos];

    #[test]
    fn formats_round_trip_through_the_pfield() {
        // req: TIME-020, TIME-001
        let tail = PFieldTail::new(&[0x7F]).unwrap();
        for epoch in [CdsEpoch::Ccsds1958, CdsEpoch::Agency] {
            for day in [CdsDayLen::Bits16, CdsDayLen::Bits24] {
                for subms in SUBMS {
                    let f = CdsFormat::new(epoch, day, subms);
                    assert_eq!((f.epoch(), f.day_len(), f.subms()), (epoch, day, subms));
                    for f in [f, f.with_tail(tail)] {
                        assert_eq!(CdsFormat::from_pfield(&f.pfield()), Ok(f));
                    }
                }
            }
        }
        let f = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits24, CdsSubMs::Picos);
        assert_eq!(
            (f.t_len(), f.tail(), f.pfield().first()),
            (11, PFieldTail::EMPTY, 0x46)
        );
        let e = CdsFormat::from_pfield(&PField::parse(&[0x43]).unwrap()).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::ReservedValue, Some("subms"))
        );
        assert_eq!(SUBMS.map(CdsSubMs::max), [0, 999, 999_999_999]);
    }

    #[test]
    fn values_are_range_checked() {
        // req: TIME-021
        let f16 = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits16, CdsSubMs::Micros);
        let f24 = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits24, CdsSubMs::None);
        let ok = CdsValue {
            day: 0xFFFF,
            ms_of_day: MAX_MS_OF_DAY,
            subms: 999,
        };
        let c = Cds::new(f16, ok).unwrap();
        assert_eq!((c.format(), c.value()), (f16, ok));
        for (format, value, field) in [
            (
                f16,
                CdsValue {
                    day: 0x1_0000,
                    ..ok
                },
                "day",
            ),
            (
                f24,
                CdsValue {
                    day: 0x100_0000,
                    ms_of_day: 0,
                    subms: 0,
                },
                "day",
            ),
            (
                f16,
                CdsValue {
                    ms_of_day: MAX_MS_OF_DAY + 1,
                    ..ok
                },
                "ms_of_day",
            ),
            (f16, CdsValue { subms: 1000, ..ok }, "subms"),
            (
                f24,
                CdsValue {
                    day: 1,
                    ms_of_day: 0,
                    subms: 1,
                },
                "subms",
            ),
        ] {
            let e = Cds::new(format, value).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::OutOfRange, Some(field))
            );
        }
        assert!(
            Cds::new(
                f24,
                CdsValue {
                    day: 0xFF_FFFF,
                    ms_of_day: 0,
                    subms: 0
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn t_field_round_trip() {
        // req: TIME-020, API-020
        for day in [CdsDayLen::Bits16, CdsDayLen::Bits24] {
            for subms in SUBMS {
                let f = CdsFormat::new(CdsEpoch::Agency, day, subms);
                let v = CdsValue {
                    day: 0xBEEF,
                    ms_of_day: 86_399_999,
                    subms: subms.max(),
                };
                let c = Cds::new(f, v).unwrap();
                let t = c.t_octets();
                assert_eq!(Cds::from_t(f, &t[..f.t_len()]), Ok(c));
            }
        }
    }
}
