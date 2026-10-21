//! Property tests of `aldis-epp` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "property helpers outside #[test] fail loudly; test buffer sizing on small bounded lengths (RS-018)"
)]

use aldis_core::{DecodeMode, ErrorKind};
use aldis_epp::{EncapsulationFields, EncapsulationPacket, HeaderSize, encode};
use aldis_testkit::strategies::epp::{
    accept_all, encapsulation_packet, encapsulation_packet_decoder_input,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::TestCaseError;
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

/// A strict decode re-encodes to exactly the octets decoded, and lenient decodes it the same
/// (API-020, API-010, GEN-020).
fn strict_re_encode_property(bytes: &[u8]) -> Result<(), TestCaseError> {
    let Ok(p) = EncapsulationPacket::decode(bytes, DecodeMode::Strict) else {
        return Ok(());
    };
    let fields = EncapsulationFields::from_parts(
        p.epi(),
        p.extension().unwrap_or_default(),
        p.user_defined().unwrap_or_default(),
    )
    .unwrap();
    let mut out = vec![0_u8; p.len()];
    let unit = accept_all().data_unit(fields, p.data()).unwrap();
    let n = encode(&unit, HeaderSize::Fixed(p.header_len()), &mut out).unwrap();
    prop_assert_eq!(&out[..n], &bytes[..p.len()]);
    let lenient = EncapsulationPacket::decode(bytes, DecodeMode::Lenient).unwrap();
    prop_assert_eq!(lenient, p);
    Ok(())
}

/// Lenient accepts whatever strict accepts, and more only with an anomaly flag (API-030,
/// API-032, GEN-020).
fn lenient_superset_property(bytes: &[u8]) -> Result<(), TestCaseError> {
    let strict = EncapsulationPacket::decode(bytes, DecodeMode::Strict);
    let lenient = EncapsulationPacket::decode(bytes, DecodeMode::Lenient);
    match (strict, lenient) {
        (Ok(s), Ok(l)) => prop_assert_eq!(s, l),
        (Ok(_), Err(e)) => prop_assert!(false, "lenient rejected a strict packet: {e}"),
        (Err(s), Ok(l)) => {
            prop_assert_eq!(s.kind(), ErrorKind::ReservedValue);
            prop_assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => prop_assert_eq!(s, l),
    }
    Ok(())
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn encode_then_decode_round_trips(p in encapsulation_packet(), tail in octets(0..4)) {
        // req: API-020, API-011, EPP-002, VER-023
        let mut out = vec![0_u8; p.data.len() + 8];
        let n = encode(&p.data_unit(), p.size, &mut out).unwrap();
        out.truncate(n);
        out.extend_from_slice(&tail);
        let d = EncapsulationPacket::decode(&out, DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.epi(), p.fields.epi());
        prop_assert_eq!(d.data(), p.data.as_slice());
        prop_assert_eq!(d.len(), n);
        prop_assert_eq!(EncapsulationPacket::peek_length(&out), Ok(n));
        if d.header_len().has_extension() {
            prop_assert_eq!(d.extension(), Some(p.fields.extension()));
            prop_assert_eq!(d.user_defined(), Some(p.fields.user_defined()));
        }
    }

    #[test]
    fn a_rejected_encode_leaves_the_buffer_unchanged(p in encapsulation_packet(), fill in any::<u8>()) {
        // req: API-022
        let mut short = vec![fill; p.octets.len() - 1];
        let e = encode(&p.data_unit(), p.size, &mut short).unwrap_err();
        prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        prop_assert!(short.iter().all(|&b| b == fill), "no partial packet written");
    }
}

proptest! {
    #![proptest_config(strategies::config(1024, REGRESSIONS))]

    #[test]
    fn strict_decode_re_encodes_exactly(bytes in encapsulation_packet_decoder_input()) {
        // req: API-020, API-010, GEN-020, VER-023
        strict_re_encode_property(&bytes)?;
    }

    #[test]
    fn lenient_accepts_whatever_strict_accepts_and_more(bytes in encapsulation_packet_decoder_input()) {
        // req: API-030, API-032, GEN-020, VER-023
        lenient_superset_property(&bytes)?;
    }
}

#[test]
fn saved_regressions() {
    // req: API-020, API-030, GEN-020, VER-023
    // Shrunk failures of the property tests' earlier unit-test form, replayed with their
    // values: a proptest seed draws a different value once the strategy changes.
    let bytes = [249, 3, 0, 0, 0, 0, 0, 0];
    strict_re_encode_property(&bytes).unwrap();
    lenient_superset_property(&bytes).unwrap();
}
