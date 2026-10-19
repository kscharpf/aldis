//! CCS T-fields from arbitrary input in every format: decode never panics, accepted values are
//! valid calendar dates and times, and they re-encode to the same octets except a flagged
//! day-of-year high nibble (TIME-030, TIME-034, API-020).
#![no_main]

// req: TIME-030, TIME-034, GEN-020, API-020, VER-022

use aldis_core::DecodeMode;
use aldis_time::{
    CalendarDate, CcsFormat, CcsVariation, Hour, Minute, Second, TimeAnomaly, TimeCode, TimeFormat,
    decode_implicit,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, t)) = data.split_first() else {
        return;
    };
    let variation = if selector & 1 == 0 {
        CcsVariation::MonthDay
    } else {
        CcsVariation::DayOfYear
    };
    let format = CcsFormat::new(variation, (selector >> 1) % 7).unwrap();
    let strict = decode_implicit(t, &TimeFormat::Ccs(format), DecodeMode::Strict);
    let Ok(d) = decode_implicit(t, &TimeFormat::Ccs(format), DecodeMode::Lenient) else {
        assert!(strict.is_err());
        return;
    };
    let TimeCode::Ccs(c) = d.code() else {
        panic!("not CCS")
    };
    // The decoded fields are typed; the date must rebuild from its own parts (TIME-030).
    let v = c.value();
    let year = v.date.year();
    let rebuilt = match (v.date.month_day(), v.date.day_of_year()) {
        (Some((month, day)), _) => CalendarDate::from_month_day(year, month, day),
        (None, Some(doy)) => CalendarDate::from_day_of_year(year, doy),
        (None, None) => panic!("a date has a month and day or a day of year"),
    };
    assert_eq!(rebuilt, Ok(v.date));
    assert_eq!(v.date.variation(), variation);
    assert!(
        v.hour.get() <= Hour::MAX && v.minute.get() <= Minute::MAX && v.second.get() <= Second::MAX
    );
    let mut out = [0_u8; 16];
    let n = d.code().encode(false, &mut out).unwrap();
    if d.anomalies().contains(TimeAnomaly::NonZeroDoyHighNibble) {
        assert!(strict.is_err());
        assert_eq!(out[2], t[2] & 0x0F);
    } else {
        assert_eq!(strict.map(|s| s.code()), Ok(d.code()));
        assert_eq!(&out[..n], &t[..d.len()]);
    }
});
