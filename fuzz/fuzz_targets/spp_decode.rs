//! Space Packets from arbitrary input: decode never panics, strict ⊆ lenient, `peek_length`
//! agrees with `decode`, the data field lies inside the input, and a strict decode re-encodes
//! to the same octets (SPP-001…SPP-004, API-010, API-011, API-013, GEN-020).
#![no_main]

// req: SPP-001, SPP-002, SPP-003, SPP-004, API-010, API-011, API-013, GEN-020, VER-022

use aldis_core::DecodeMode;
use aldis_spp::{SpacePacket, SppAnomaly, SppConfig, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let strict = SpacePacket::decode(data, DecodeMode::Strict);
    let Ok(lenient) = SpacePacket::decode(data, DecodeMode::Lenient) else {
        assert!(strict.is_err());
        return;
    };
    assert_eq!(SpacePacket::peek_length(data), Ok(lenient.len()));
    assert!(lenient.len() <= data.len() && lenient.data_field().len() + 6 == lenient.len());
    let flagged = lenient
        .anomalies()
        .contains(SppAnomaly::IdleWithSecondaryHeader);
    assert_eq!(flagged, lenient.is_idle() && lenient.has_secondary_header());
    match strict {
        Err(_) => assert!(flagged),
        Ok(s) => {
            assert_eq!(s, lenient);
            // The data field is opaque here: put it all in the user data or, with the flag
            // set, all in the secondary header. Either way the octets must come back.
            let field = s.data_field();
            let (sec, user) = if s.has_secondary_header() {
                (field, &[][..])
            } else {
                (&[][..], field)
            };
            let packet = SppConfig::DEFAULT
                .packet(s.header().fields(), sec, user)
                .unwrap();
            let mut out = [0_u8; 70_000];
            let n = encode(&packet, &mut out).unwrap();
            assert_eq!(&out[..n], &data[..s.len()]);
        }
    }
});
