//! Times as GitHub, git, and the evidence records write them, and the durations of the report.

/// Days from 1970-01-01 to the civil date `y-m-d` (H. Hinnant's `days_from_civil`).
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The number in `s[range]`, if it is all ASCII digits.
fn digits(s: &str, range: std::ops::Range<usize>) -> Option<i64> {
    let part = s.get(range)?;
    if part.bytes().all(|b| b.is_ascii_digit()) {
        part.parse().ok()
    } else {
        None
    }
}

/// Seconds since the Unix epoch of a UTC time `YYYY-MM-DDTHH:MM:SSZ` (the form of the GitHub API,
/// of git dates as GitHub reports them, and of the evidence records), with optional fractional
/// seconds, or of a date `YYYY-MM-DD` at midnight UTC.
pub fn parse(text: &str) -> Option<i64> {
    let sep = |i: usize, c: u8| text.as_bytes().get(i) == Some(&c);
    if !(sep(4, b'-') && sep(7, b'-')) {
        return None;
    }
    let year = digits(text, 0..4)?;
    let month = digits(text, 5..7)?;
    let day = digits(text, 8..10)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let midnight = days_from_civil(year, month, day) * 86_400;
    if text.len() == 10 {
        return Some(midnight);
    }
    if !(sep(10, b'T') && sep(13, b':') && sep(16, b':') && text.ends_with('Z')) {
        return None;
    }
    let hour = digits(text, 11..13)?;
    let minute = digits(text, 14..16)?;
    let second = digits(text, 17..19)?;
    let frac = text.get(19..text.len() - 1)?;
    let frac_ok = frac.is_empty()
        || frac
            .strip_prefix('.')
            .is_some_and(|f| f.bytes().all(|b| b.is_ascii_digit()));
    if !frac_ok || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(midnight + hour * 3600 + minute * 60 + second)
}

/// The UTC date `YYYY-MM-DD` of a Unix time.
pub fn date(secs: i64) -> String {
    let full = crate::verify::utc(u64::try_from(secs).unwrap_or(0));
    full.get(..10).unwrap_or(&full).to_owned()
}

/// The current Unix time.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// A duration for a reader: `45 s`, `12 min`, `3 h 05 min`, `2 d 4 h`.
pub fn duration(secs: i64) -> String {
    let sign = if secs < 0 { "-" } else { "" };
    let s = secs.unsigned_abs();
    let body = match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h {:02} min", s / 3600, s / 60 % 60),
        _ => format!("{} d {} h", s / 86_400, s / 3600 % 24),
    };
    format!("{sign}{body}")
}

/// The median of `values` (the lower middle one for an even count), if there are any.
pub fn median(values: &[i64]) -> Option<i64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len().checked_sub(1)? / 2;
    sorted.get(mid).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-063
    #[test]
    fn parses_api_times_and_dates() {
        assert_eq!(parse("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse("2026-10-19T16:02:34Z"), Some(1_792_425_754));
        assert_eq!(parse("2026-10-19T16:02:34.250Z"), Some(1_792_425_754));
        assert_eq!(parse("2000-02-29"), Some(951_782_400));
        for bad in [
            "",
            "2026-10-19 16:02:34Z",
            "2026-13-01",
            "2026-10-19T16:02:34",
            "2026-10-19T24:00:00Z",
            "2026-1x-19",
            "2026-10-19T16:02:34.x5Z",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(date(1_792_425_754), "2026-10-19");
    }

    // req: VER-063
    #[test]
    fn durations_and_medians() {
        assert_eq!(duration(45), "45 s");
        assert_eq!(duration(12 * 60 + 5), "12 min");
        assert_eq!(duration(3 * 3600 + 5 * 60), "3 h 05 min");
        assert_eq!(duration(2 * 86_400 + 4 * 3600 + 10), "2 d 4 h");
        assert_eq!(duration(-90), "-1 min");
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[5]), Some(5));
        assert_eq!(median(&[9, 1, 5]), Some(5));
        assert_eq!(median(&[4, 1, 3, 2]), Some(2));
    }
}
