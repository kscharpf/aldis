//! Calls every public `aldis-aos` API (VER-010, M2 D16) and runs every `test-vectors/aos/` case
//! (`vector`). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_aos::fhec::{self, CorrectedSymbols};
use aldis_core::ErrorKind;

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Out, Run, need};

/// The FHEC group: encode, write, and decode with zero, one, and two corrections and an
/// uncorrectable header (AOS-013, AOS-014).
fn fhec_group(t: &mut Tally) {
    let six = [0x40, 0x01, 0, 0, 0, 0];
    t.check(fhec::encode(bb(&six)) == [0x3E, 0xDF]);
    let mut header = [0x40, 0x01, 0, 0, 0, 0, 0xFF, 0xFF];
    fhec::write(bb(&mut header));
    t.check(header == [0x40, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]);
    let clean = fhec::decode(bb(&header));
    t.check(
        clean
            .is_ok_and(|c| c.octets() == header && c.corrected_symbols() == CorrectedSymbols::Zero),
    );
    let one = fhec::decode(bb(&[0x50, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]));
    t.check(one.is_ok_and(|c| c.octets() == header && c.corrected_symbols().get() == 1));
    let two = fhec::decode(bb(&[0xBF, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]));
    t.check(two.is_ok_and(|c| c.corrected_symbols() == CorrectedSymbols::Two));
    let bad = fhec::decode(bb(&[0x51, 0x11, 0, 0, 0, 0, 0x3E, 0xDF]));
    t.check(bad.is_err_and(|e| e.kind() == ErrorKind::HeaderUncorrectable));
}

/// Runs the `aldis-aos` group.
pub fn run(t: &mut Tally) {
    fhec_group(t);
}

/// The input of a case as an `N`-octet array.
fn array<const N: usize>(input: &[u8]) -> Result<[u8; N], Fail> {
    need(
        input
            .first_chunk::<N>()
            .filter(|_| input.len() == N)
            .copied(),
    )
}

/// One `test-vectors/aos/` case, as `crates/aldis-aos/tests/vectors.rs` runs it.
pub fn vector(case: &Case, out: &mut Out) -> Run {
    match case.op {
        "fhec_encode" => {
            let six: [u8; 6] = array(case.input)?;
            let mut header = [0xA5; 8];
            for (h, s) in header.iter_mut().zip(six) {
                *h = s;
            }
            fhec::write(bb(&mut header));
            out.bytes("fhec_hex", &fhec::encode(bb(&six)));
            out.bytes("output_hex", &header);
        }
        "fhec_decode" => {
            let corrected = fhec::decode(bb(&array(case.input)?))?;
            out.bytes("header_hex", &corrected.octets());
            out.num("corrected_symbols", corrected.corrected_symbols().get());
        }
        _ => return Err(Fail::Vector),
    }
    Ok(())
}
