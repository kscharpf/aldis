//! CDS T-fields from arbitrary input in every format: decode never panics, accepted values are
//! in range, and they re-encode to the same octets (TIME-020, TIME-021, API-020).
#![no_main]

// req: TIME-020, TIME-021, GEN-020, API-020, VER-022

use aldis_core::DecodeMode;
use aldis_time::{
    CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, MsOfDay, TimeCode, TimeFormat, decode_implicit,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, t)) = data.split_first() else {
        return;
    };
    let day = if selector & 1 == 0 {
        CdsDayLen::Bits16
    } else {
        CdsDayLen::Bits24
    };
    let subms = match (selector >> 1) % 3 {
        0 => CdsSubMs::None,
        1 => CdsSubMs::Micros,
        _ => CdsSubMs::Picos,
    };
    let format = CdsFormat::new(CdsEpoch::Ccsds1958, day, subms);
    let Ok(d) = decode_implicit(t, &TimeFormat::Cds(format), DecodeMode::Lenient) else {
        return;
    };
    let TimeCode::Cds(c) = d.code() else {
        panic!("not CDS")
    };
    let v = c.value();
    assert!(v.ms_of_day.get() <= MsOfDay::MAX && v.subms.get() <= subms.max());
    assert_eq!(v.subms.resolution(), subms);
    let mut out = [0_u8; 16];
    let n = d.code().encode(false, &mut out).unwrap();
    assert_eq!(&out[..n], &t[..d.len()]);
});
