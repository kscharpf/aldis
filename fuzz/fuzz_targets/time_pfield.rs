//! `decode_with_pfield` on arbitrary input in both modes: no panic, strict ⊆ lenient, the
//! consumed length within the input, and a re-encode that decodes to the same code (GEN-020,
//! API-010, API-020, API-030, TIME-001…TIME-006).
#![no_main]

// req: GEN-020, API-010, API-020, API-030, TIME-001, TIME-003, TIME-005, VER-022

use aldis_core::{DecodeMode, ErrorKind};
use aldis_time::decode_with_pfield;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let strict = decode_with_pfield(data, DecodeMode::Strict);
    let lenient = decode_with_pfield(data, DecodeMode::Lenient);
    match (&strict, &lenient) {
        (Ok(s), Ok(l)) => assert_eq!(s, l),
        (Ok(_), Err(e)) => panic!("lenient rejected a strict code: {e}"),
        (Err(s), Ok(l)) => {
            assert_eq!(s.kind(), ErrorKind::ReservedValue);
            assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => assert!(s == l || s.kind() == ErrorKind::ReservedValue),
    }
    if let Ok(d) = lenient {
        assert!(d.len() <= data.len());
        assert!(d.pfield().is_some_and(|p| p.len() <= d.len()));
        let mut out = [0_u8; 32];
        let n = d.code().encode(true, &mut out).unwrap();
        let again = decode_with_pfield(&out[..n], DecodeMode::Lenient).unwrap();
        assert_eq!((again.code(), again.len()), (d.code(), n));
    }
    if let Ok(d) = strict {
        assert!(d.anomalies().is_empty());
    }
});
