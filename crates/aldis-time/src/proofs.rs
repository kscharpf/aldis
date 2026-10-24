//! Kani proof harnesses (spike, docs/verification/formal-proofs.md, OI-42).
//!
//! Compiled only by `cargo kani` (`cfg(kani)`); not part of the library, the tests, or the
//! coverage run. The properties are those TIME-011 states (docs/standards/time-codes.md).
//!
//! The number of fine octets is fixed per harness and the coarse octets, epoch, and values are
//! symbolic. With a symbolic fine count the 128-bit shifts and divisions take symbolic amounts,
//! and CBMC did not finish one such harness in 10 minutes (formal-proofs.md §6); with a fixed
//! count of 1 or 2 each finishes in seconds or minutes, and 3 only for `to_seconds_nanos`. No
//! harness has a loop.

use aldis_core::ErrorKind;

use crate::cuc::{Cuc, CucEpoch, CucFormat, CucValue};
use crate::field;

const NANOS: u128 = 1_000_000_000;

/// A CUC format with 1–7 symbolic coarse octets, `fine` fine octets, and either epoch.
fn any_format(fine: u8) -> CucFormat {
    let coarse: u8 = kani::any();
    kani::assume((1..=7).contains(&coarse));
    let epoch = if kani::any() {
        CucEpoch::Tai1958
    } else {
        CucEpoch::Agency
    };
    let format = CucFormat::new(epoch, coarse, fine);
    assert!(format.is_ok());
    format.unwrap_or_else(|_| unreachable!())
}

/// The fraction of `value` as an integer count of units of the last fine octet.
fn units(format: &CucFormat, value: &CucValue) -> u128 {
    value.fine() >> (128 - 8 * u32::from(format.fine_octets()))
}

/// `Cuc::from_seconds_nanos` for every format with `fine` fine octets and every `seconds` and
/// `nanos`: it fails exactly for `nanos` ≥ 10⁹ or `seconds` too wide for the coarse octets
/// (TIME-012), keeps `seconds` as the coarse time (nothing carries), and rounds the fraction
/// down: the result is the largest value of the format at or below `nanos` (OI-40). The
/// value is representable, and converting back never gives a later time.
fn from_seconds_nanos_floor(fine: u8) {
    let format = any_format(fine);
    let seconds: u64 = kani::any();
    let nanos: u32 = kani::any();
    let coarse_bits = 8 * u32::from(format.coarse_octets());
    let coarse_fits = coarse_bits == 64 || seconds >> coarse_bits == 0;
    match Cuc::from_seconds_nanos(format, seconds, nanos) {
        Err(e) => {
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            if u128::from(nanos) >= NANOS {
                assert_eq!(e.field(), Some(field::NANOSECONDS));
            } else {
                assert!(!coarse_fits);
                assert_eq!(e.field(), Some(field::COARSE));
            }
        }
        Ok(cuc) => {
            assert!(u128::from(nanos) < NANOS && coarse_fits);
            let value = cuc.value();
            assert_eq!(value.coarse(), seconds);
            assert_eq!(Cuc::new(format, value), Ok(cuc));
            // r units of 2^-8f s with r·10⁹ ≤ nanos·2^8f < (r + 1)·10⁹.
            let r = units(&format, &value);
            let scaled = u128::from(nanos) << (8 * u32::from(fine));
            assert!(r * NANOS <= scaled);
            assert!(scaled < (r + 1) * NANOS);
            let (s, n) = cuc.to_seconds_nanos();
            assert!(s == seconds && n <= nanos);
        }
    }
}

/// `to_seconds_nanos` for every value of every format with `fine` fine octets: the coarse time
/// is the seconds and the nanoseconds are the fraction truncated toward zero.
fn to_seconds_nanos_truncates(fine: u8) {
    let bits = 8 * u32::from(fine);
    let coarse: u64 = kani::any();
    let r: u128 = kani::any();
    kani::assume(r >> bits == 0);
    let value = CucValue::new(coarse, r << (128 - bits));
    let (s, n) = value.to_seconds_nanos();
    assert_eq!(s, coarse);
    // n = floor(r·10⁹ / 2^bits): n·2^bits ≤ r·10⁹ < (n + 1)·2^bits.
    let n = u128::from(n);
    assert!(n < NANOS);
    assert!(n << bits <= r * NANOS);
    assert!(r * NANOS < (n + 1) << bits);
}

/// The round trip value → nanoseconds → value that TIME-011 documents, for every value of every
/// format with `fine` fine octets: exact if and only if the value is a multiple of 2⁻⁹ s
/// (always with 1 fine octet), and otherwise one unit of the last fine octet lower.
fn round_trip_value(fine: u8) {
    let format = any_format(fine);
    let bits = 8 * u32::from(fine);
    let coarse_bits = 8 * u32::from(format.coarse_octets());
    let coarse: u64 = kani::any();
    kani::assume(coarse_bits == 64 || coarse >> coarse_bits == 0);
    let r: u128 = kani::any();
    kani::assume(r >> bits == 0);
    let cuc = Cuc::new(format, CucValue::new(coarse, r << (128 - bits)));
    assert!(cuc.is_ok());
    let Ok(cuc) = cuc else { return };
    let (s, n) = cuc.to_seconds_nanos();
    let back = Cuc::from_seconds_nanos(format, s, n);
    assert!(back.is_ok());
    let Ok(back) = back else { return };
    assert_eq!(back.value().coarse(), coarse);
    let r_back = units(&format, &back.value());
    // A multiple of 2⁻⁹ s is a multiple of 2^(bits − 9) units; with 8 bits every value is.
    let multiple = bits <= 9 || r % (1 << (bits - 9)) == 0;
    if multiple {
        assert_eq!(r_back, r);
    } else {
        assert_eq!(r_back + 1, r);
    }
    assert_eq!(back == cuc, multiple);
}

// One harness per fine-octet count that CBMC finishes within 30 minutes; the counts that did not
// (3 for the floor and the value round trip, 4 for the nanoseconds round trip) have none
// (formal-proofs.md §6).

macro_rules! from_ns_harnesses {
    ($($fine:literal: $name:ident;)*) => {$(
        #[kani::proof]
        fn $name() {
            // req: TIME-011, TIME-012, GEN-020
            from_seconds_nanos_floor($fine);
        }
    )*};
}

from_ns_harnesses! {
    1: cuc_from_ns_floor_fine1;
    2: cuc_from_ns_floor_fine2;
}

macro_rules! to_ns_harnesses {
    ($($fine:literal: $name:ident;)*) => {$(
        #[kani::proof]
        fn $name() {
            // req: TIME-011, GEN-020
            to_seconds_nanos_truncates($fine);
        }
    )*};
}

to_ns_harnesses! {
    1: cuc_to_ns_truncates_fine1;
    2: cuc_to_ns_truncates_fine2;
    3: cuc_to_ns_truncates_fine3;
}

macro_rules! value_harnesses {
    ($($fine:literal: $name:ident;)*) => {$(
        #[kani::proof]
        fn $name() {
            // req: TIME-011, GEN-020
            round_trip_value($fine);
        }
    )*};
}

value_harnesses! {
    1: cuc_round_trip_value_fine1;
    2: cuc_round_trip_value_fine2;
}
