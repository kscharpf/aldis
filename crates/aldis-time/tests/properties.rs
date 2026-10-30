//! Property tests of `aldis-time` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "property helpers outside #[test] fail loudly; test models on bounded values (RS-018)"
)]

use aldis_core::ErrorKind;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::TestCaseError;
use aldis_testkit::strategies::time::{
    ascii_parser_input, ccs, cds_in, cuc_coarse, cuc_format, nanos, time_code_decoder_input,
};
use aldis_testkit::strategies::{self};
use aldis_time::{
    AsciiFormat, AsciiOptions, AsciiSubset, AsciiTime, Ccs, CcsFormat, CcsVariation, CdsDayLen,
    CdsEpoch, CdsFormat, CdsSubMs, Cuc, MAX_ASCII_LEN, TimeCode, TimeFormat, UtcInstant,
    decode_implicit, decode_implicit_lenient, decode_with_pfield, decode_with_pfield_lenient,
};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

/// Neither ASCII parser panics; an accepted complete code formats back to the same text and is
/// an accepted subset (GEN-020, TIME-031, TIME-032).
fn ascii_text_property(bytes: &[u8]) -> Result<(), TestCaseError> {
    if let Ok(t) = AsciiTime::parse(bytes, &AsciiOptions::DEFAULT) {
        let mut out = [0_u8; MAX_ASCII_LEN];
        let n = t.encode(&mut out).unwrap();
        prop_assert_eq!(&out[..n], bytes);
        prop_assert!(AsciiSubset::parse(bytes, &AsciiOptions::DEFAULT).is_ok());
    }
    let _ = AsciiSubset::parse(bytes, &AsciiOptions::DEFAULT);
    Ok(())
}

/// Strict accepts a subset of what lenient accepts, and a lenient decode re-encodes to a code
/// that decodes the same (GEN-020, API-010, API-020, API-030).
fn pfield_decode_property(bytes: &[u8]) -> Result<(), TestCaseError> {
    let strict = decode_with_pfield(bytes);
    let lenient = decode_with_pfield_lenient(bytes);
    match (&strict, &lenient) {
        (Ok(s), Ok(l)) => {
            prop_assert_eq!(s, &**l);
            prop_assert!(l.anomalies().is_empty());
        }
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
        let again = decode_with_pfield_lenient(&out[..n]).unwrap();
        prop_assert_eq!(again.code(), d.code());
        prop_assert_eq!(again.len(), n);
    }
    Ok(())
}

proptest! {
    #![proptest_config(strategies::config(1024, REGRESSIONS))]

    #[test]
    fn arbitrary_text_never_panics(bytes in ascii_parser_input()) {
        // req: GEN-020, TIME-031, TIME-032
        ascii_text_property(&bytes)?;
    }

    #[test]
    fn arbitrary_input(bytes in time_code_decoder_input()) {
        // req: GEN-020, API-010, API-020, API-030, VER-023
        pfield_decode_property(&bytes)?;
    }
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn ns_round_trip(
        (f, secs, ns) in cuc_format().prop_flat_map(|f| (Just(f), cuc_coarse(f), nanos()))
    ) {
        // req: TIME-011, VER-023
        let fo = f.fine_octets();
        let Ok(c) = Cuc::from_seconds_nanos(f, secs, ns) else { return Ok(()) };
        prop_assert_eq!(Cuc::new(f, c.value()), Ok(c), "rounded value is representable");
        let (s2, ns2) = c.value().to_seconds_nanos();
        // Rounding down never raises the time, and lowers it by less than one fine step plus the
        // nanosecond truncated on the way back (TIME-011, OI-40).
        let before = u128::from(secs) * 1_000_000_000 + u128::from(ns);
        let after = u128::from(s2) * 1_000_000_000 + u128::from(ns2);
        prop_assert!(after <= before);
        let step = 1_000_000_000_u128 >> (8 * u32::from(fo)).min(30);
        prop_assert!(before <= after + step + 1);
        // CucValue → ns → CucValue is exact if and only if the value is a multiple of 2⁻⁹ s,
        // and otherwise one unit of the last fine octet lower for 2 or 3 fine octets (TIME-011).
        let back = Cuc::from_seconds_nanos(f, s2, ns2).unwrap().value();
        let value = c.value();
        prop_assert_eq!(back.coarse(), value.coarse());
        if value.fine() & (u128::MAX >> 9) == 0 {
            prop_assert_eq!(back, value);
        } else {
            prop_assert!(back.fine() < value.fine());
            if fo <= 3 {
                let unit = 1_u128 << (128 - 8 * u32::from(fo));
                prop_assert_eq!(back.fine() + unit, value.fine());
            }
        }
        if fo >= 4 {
            // ns → CucValue → ns gives the nanoseconds back if they are a multiple of 1 953 125
            // (2⁻⁹ s), and one fewer otherwise (TIME-011).
            let expected = if ns.is_multiple_of(1_953_125) { ns } else { ns - 1 };
            prop_assert_eq!((s2, ns2), (secs, expected));
        }
    }

    #[test]
    fn t_field_round_trip(c in ccs()) {
        // req: TIME-030, API-020, VER-023
        let format = c.format();
        let mut t = [0_u8; 32];
        let n = TimeCode::Ccs(c).encode(false, &mut t).unwrap();
        prop_assert_eq!(n, format.t_len());
        let d = decode_implicit(&t[..n], &TimeFormat::Ccs(format)).unwrap();
        prop_assert_eq!(d.code(), TimeCode::Ccs(c));
        let lenient = decode_implicit_lenient(&t[..n], &TimeFormat::Ccs(format)).unwrap();
        prop_assert!(lenient.anomalies().is_empty());
        prop_assert_eq!(t[2] >> 4 == 0 || format.variation() == CcsVariation::MonthDay, true);
    }

    #[test]
    fn cds_round_trips(
        code in cds_in(CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits16, CdsSubMs::Picos))
    ) {
        // req: TIME-033, VER-023
        let fmt = code.format();
        let inst = UtcInstant::from_cds(&code, None).unwrap();
        prop_assert_eq!(inst.to_cds(fmt, None), Ok(code));
        // Through CCS with 12 sub-second digits and ASCII with 12 digits: exact.
        let ccs = inst.to_ccs(CcsFormat::new(CcsVariation::DayOfYear, 6).unwrap()).unwrap();
        prop_assert_eq!(UtcInstant::from_ccs(&ccs), inst);
        let agency_cds = inst.to_ascii(AsciiFormat::new(CcsVariation::MonthDay, 12, true).unwrap()).unwrap();
        prop_assert_eq!(UtcInstant::from_ascii(&agency_cds), inst);
        prop_assert_eq!(Ccs::new(ccs.format(), ccs.value()), Ok(ccs));
    }
}

#[test]
fn saved_regressions() {
    // req: GEN-020, API-030, VER-023
    // Shrunk failures of the property tests' earlier unit-test form, replayed with their
    // values: a proptest seed draws a different value once the strategy changes.
    pfield_decode_property(&[144, 128, 0]).unwrap();
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn strict_equals_lenient_without_anomalies(bytes in time_code_decoder_input()) {
        // req: API-030, API-031, API-032, TIME-002
        let strict = decode_with_pfield(&bytes);
        let lenient = decode_with_pfield_lenient(&bytes);
        if let Ok(s) = strict {
            let l = lenient.expect("lenient accepts what strict accepts");
            prop_assert!(l.anomalies().is_empty());
            prop_assert_eq!(s, l.into_view());
        }
    }
}
