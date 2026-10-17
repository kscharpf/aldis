//! The bitwise and table CRC-16 agree on every input, and FECF write/verify/syndrome are
//! consistent (CMN-001…CMN-004).
#![no_main]

// req: CMN-001, CMN-002, CMN-003, CMN-004, VER-022

use aldis_core::ErrorKind;
use aldis_core::crc::{crc16, crc16_bitwise, crc16_table, syndrome, verify_fecf, write_fecf};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let crc = crc16_bitwise(data);
    assert_eq!(crc16_table(data), crc);
    assert_eq!(crc16(data), crc);

    let mut frame = data.to_vec();
    frame.extend_from_slice(&[0, 0]);
    write_fecf(&mut frame).unwrap();
    assert_eq!(frame[data.len()..], crc.to_be_bytes());
    assert_eq!(verify_fecf(&frame), Ok(()));
    assert_eq!(syndrome(&frame), 0);

    // Any single-bit error is detected by both methods.
    if !frame.is_empty() {
        let i = usize::from(data.first().copied().unwrap_or(0)) % (frame.len() * 8);
        frame[i / 8] ^= 0x80 >> (i % 8);
        assert_eq!(
            verify_fecf(&frame).map_err(|e| e.kind()),
            Err(ErrorKind::CrcMismatch)
        );
        assert_ne!(syndrome(&frame), 0);
    }

    // Arbitrary input: verify and syndrome agree.
    if data.len() >= 2 {
        assert_eq!(verify_fecf(data).is_ok(), syndrome(data) == 0);
    } else {
        assert_eq!(
            verify_fecf(data).map_err(|e| e.kind()),
            Err(ErrorKind::Incomplete)
        );
    }
});
