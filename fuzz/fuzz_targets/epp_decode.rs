//! `EncapsulationPacket::decode` and `peek_length` on arbitrary input in both modes: no panic,
//! every view slice inside the input, strict ⊆ lenient, and an accepted strict packet
//! re-encodes to the same octets (GEN-020, API-010, API-020, API-030, GEN-042).
#![no_main]

// req: GEN-020, API-010, API-020, API-030, API-032, GEN-042, VER-022

use aldis_core::{DecodeMode, ErrorKind};
use aldis_epp::{EncapsulationFields, EncapsulationPacket, HeaderSize, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let strict = EncapsulationPacket::decode(data, DecodeMode::Strict);
    let lenient = EncapsulationPacket::decode(data, DecodeMode::Lenient);
    let peek = EncapsulationPacket::peek_length(data);
    match (&strict, &lenient) {
        (Ok(s), Ok(l)) => assert_eq!(s, l),
        (Ok(_), Err(e)) => panic!("lenient rejected a strict packet: {e}"),
        (Err(s), Ok(l)) => {
            assert_eq!(s.kind(), ErrorKind::ReservedValue);
            assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => assert_eq!(s, l),
    }
    if let Ok(p) = lenient {
        assert!(p.len() <= data.len());
        assert_eq!(peek, Ok(p.len()));
        let start = p.data().as_ptr() as usize - data.as_ptr() as usize;
        assert_eq!(start, p.header_len().octets());
        assert_eq!(start + p.data().len(), p.len());
    }
    if let Ok(p) = strict {
        assert!(p.anomalies().is_empty());
        let fields = EncapsulationFields {
            epi: p.epi(),
            extension: p.extension().unwrap_or_default(),
            user_defined: p.user_defined().unwrap_or_default(),
        };
        let mut out = vec![0_u8; p.len()];
        let n = encode(
            fields,
            HeaderSize::Fixed(p.header_len()),
            p.data(),
            &mut out,
        )
        .unwrap();
        assert_eq!(&out[..n], &data[..p.len()]);
    }
});
