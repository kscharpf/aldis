//! ASCII time codes and conversions from arbitrary input: neither parser panics, an accepted
//! complete code formats back to the same text and is also an accepted subset, and the
//! conversions round-trip exactly through CDS (picoseconds), CCS (six sub-second octets) and
//! ASCII (twelve digits) (TIME-031, TIME-032, TIME-033).
#![no_main]

// req: TIME-031, TIME-032, TIME-033, GEN-020, VER-022

use aldis_time::{
    AsciiFormat, AsciiOptions, AsciiSubset, AsciiTime, CcsFormat, CdsDayLen, CdsEpoch, CdsFormat,
    CdsSubMs, MAX_ASCII_LEN, UtcInstant,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, text)) = data.split_first() else {
        return;
    };
    let options = AsciiOptions::new(selector % 13).unwrap();
    let subset = AsciiSubset::parse(text, &options);
    let Ok(t) = AsciiTime::parse(text, &options) else {
        return;
    };
    assert!(subset.is_ok(), "a complete code is a subset");
    let mut out = [0_u8; MAX_ASCII_LEN];
    let n = t.encode(&mut out).unwrap();
    assert_eq!((&out[..n], t.encoded_len()), (text, text.len()));

    let instant = UtcInstant::from_ascii(&t);
    let variation = t.date.variation();
    let ascii = instant
        .to_ascii(AsciiFormat::new(variation, 12, t.utc).unwrap())
        .unwrap();
    assert_eq!(UtcInstant::from_ascii(&ascii), instant);
    let ccs = instant
        .to_ccs(CcsFormat::new(variation, 6).unwrap())
        .unwrap();
    assert_eq!(UtcInstant::from_ccs(&ccs), instant);
    let cds = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits24, CdsSubMs::Picos);
    match instant.to_cds(cds, None) {
        Ok(c) => assert_eq!(UtcInstant::from_cds(&c, None), Ok(instant)),
        // Before 1958, or a second 60 that is not 23:59:60 (OI-29).
        Err(_) => assert!(t.date.year().get() < 1958 || t.second.get() == 60),
    }
});
