//! The Fibonacci and Galois OID generators produce the same stream, whether read by bit,
//! octet, or `fill`, and an octet is eight bits MSB first (CMN-020…CMN-023).
#![no_main]

// req: CMN-020, CMN-021, CMN-022, CMN-023, VER-022

use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut fib = OidGenerator::new();
    let mut gal = OidGeneratorGalois::new();
    let mut fill = OidGenerator::new();
    let mut by_bit = OidGeneratorGalois::new();
    // Each input octet is one step: low bit 0 = one bit, 1 = `op >> 1` octets.
    for &op in data.iter().take(256) {
        if op & 1 == 0 {
            let b = fib.next_bit();
            assert!(b <= 1);
            assert_eq!(gal.next_bit(), b);
            assert_eq!(fill.next_bit(), b);
            assert_eq!(by_bit.next_bit(), b);
        } else {
            let mut octets = vec![0_u8; usize::from(op >> 1)];
            fill.fill(&mut octets);
            for &octet in &octets {
                assert_eq!(fib.next_octet(), octet);
                assert_eq!(gal.next_octet(), octet);
                let composed = (0..8).fold(0_u8, |acc, _| acc << 1 | by_bit.next_bit());
                assert_eq!(composed, octet);
            }
        }
    }
});
