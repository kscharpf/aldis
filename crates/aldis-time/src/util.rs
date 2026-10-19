//! Crate-internal helpers: `const` widening and Gregorian calendar rules.

/// Widens `u32` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u32 -> u64")]
pub(crate) const fn wide(n: u32) -> u64 {
    n as u64
}

/// Widens `u8` to `u64` in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> u64")]
pub(crate) const fn wide8(n: u8) -> u64 {
    n as u64
}

/// Gregorian leap year: divisible by 4, except centuries not divisible by 400 (301.0 Annex A).
pub(crate) const fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Days in `month` (1–12) of `year`; 0 for an invalid month.
pub(crate) const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days in `year`.
pub(crate) const fn days_in_year(year: u16) -> u16 {
    if is_leap(year) { 366 } else { 365 }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{days_in_month, days_in_year, is_leap, wide, wide8};

    #[test]
    fn gregorian_rules() {
        // req: TIME-030
        assert!(is_leap(2024) && is_leap(2000) && !is_leap(1900) && !is_leap(2023));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 12), 31);
        assert_eq!(days_in_month(2023, 0), 0);
        assert_eq!(days_in_month(2023, 13), 0);
        assert_eq!((days_in_year(2024), days_in_year(2023)), (366, 365));
        assert_eq!((wide(u32::MAX), wide8(255)), (u64::from(u32::MAX), 255));
    }
}
