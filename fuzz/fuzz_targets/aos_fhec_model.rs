//! The FHEC decoder against the table model of `aldis-testkit` (M2 plan D10): for an arbitrary
//! received header the decoder corrects exactly what the model corrects and fails exactly where
//! the model has no entry; an encoded header decodes with zero corrections; and one or two
//! symbol errors on it are corrected back (AOS-013, AOS-014, GEN-020).
#![no_main]

// req: AOS-013, AOS-014, GEN-020, VER-022

use aldis_aos::fhec::{self, CorrectedSymbols};
use aldis_core::ErrorKind;
use aldis_testkit::oracle::fhec as model;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((received, rest)) = data.split_first_chunk::<8>() else {
        return;
    };
    match (fhec::decode(received), model::decode(received)) {
        (Ok(c), Some((octets, weight))) => {
            assert_eq!(c.octets(), octets);
            assert_eq!(c.corrected_symbols().get(), weight);
            assert_eq!(c.octets()[2..5], received[2..5], "frame count altered");
        }
        (Err(e), None) => assert_eq!(e.kind(), ErrorKind::HeaderUncorrectable),
        (d, m) => panic!("{received:02X?}: decoder {d:?}, model {m:?}"),
    }

    // encode, then decode: a codeword, which the model encodes the same way.
    let mut codeword = *received;
    fhec::write(&mut codeword);
    let six = [
        codeword[0],
        codeword[1],
        codeword[2],
        codeword[3],
        codeword[4],
        codeword[5],
    ];
    assert_eq!(fhec::encode(&six), model::encode(&six));
    let clean = fhec::decode(&codeword).unwrap();
    assert_eq!(
        (clean.octets(), clean.corrected_symbols()),
        (codeword, CorrectedSymbols::Zero)
    );

    // Up to two symbol errors drawn from the rest of the input are corrected back.
    let mut errors = [0_u8; model::SYMBOLS];
    for &[position, value] in rest.as_chunks::<2>().0.iter().take(2) {
        errors[usize::from(position) % model::SYMBOLS] = value & 0x0F;
    }
    let weight = errors.iter().filter(|&&e| e != 0).count();
    let damaged = model::with_errors(&codeword, &errors);
    let fixed = fhec::decode(&damaged).unwrap();
    assert_eq!(fixed.octets(), codeword);
    assert_eq!(usize::from(fixed.corrected_symbols().get()), weight);
});
