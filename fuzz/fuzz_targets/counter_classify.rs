//! `ModCounter` arithmetic and continuity classification against a `u128` model (CMN-030,
//! CMN-031, GEN-024).
#![no_main]

// req: CMN-030, CMN-031, GEN-024, VER-022

use aldis_core::ErrorKind;
use aldis_core::counter::{Continuity, ModCounter};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn check(input: &mut Unstructured<'_>) -> Result<()> {
    let bits: u8 = input.int_in_range(0..=66)?;
    let prev: u64 = input.arbitrary()?;
    let later: u64 = input.arbitrary()?;
    let modulus = 1_u128 << bits.min(64);
    let fits = |value: u64| u128::from(value) < modulus;

    let counter = match ModCounter::new(bits, prev) {
        Ok(made) => {
            assert!((1..=64).contains(&bits) && fits(prev));
            made
        }
        Err(err) => {
            assert!(!(1..=64).contains(&bits) || !fits(prev));
            assert_eq!(err.kind(), ErrorKind::OutOfRange);
            return Ok(());
        }
    };
    assert_eq!((counter.bits(), counter.value()), (bits, prev));

    let next = counter.next();
    assert_eq!(u128::from(next.value()), (u128::from(prev) + 1) % modulus);
    let mut inc = counter;
    inc.increment();
    assert_eq!(inc, next);

    match (counter.distance_to(later), counter.classify(later)) {
        (Ok(dist), Ok(class)) => {
            assert!(fits(later));
            let model = (u128::from(later) + modulus - u128::from(prev)) % modulus;
            assert_eq!(u128::from(dist), model);
            let expect = match model {
                0 => Continuity::Repeat,
                1 => Continuity::InSequence,
                far if far <= modulus / 2 => Continuity::Gap {
                    missing: u64::try_from(far - 1).unwrap(),
                },
                _ => Continuity::Backwards,
            };
            assert_eq!(class, expect);
            // The count after `self` is always in sequence.
            assert_eq!(counter.classify(next.value()), Ok(Continuity::InSequence));
        }
        (Err(e1), Err(e2)) => {
            assert!(!fits(later));
            assert_eq!(
                (e1.kind(), e2.kind()),
                (ErrorKind::OutOfRange, ErrorKind::OutOfRange)
            );
        }
        other => panic!("distance_to and classify disagree: {other:?}"),
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = check(&mut Unstructured::new(data));
});
