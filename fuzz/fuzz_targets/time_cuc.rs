//! CUC formats and values from arbitrary input: encode/decode round trip with and without the
//! P-field, and the nanosecond conversions against a big-integer model (TIME-010…TIME-013,
//! API-020).
#![no_main]

// req: TIME-010, TIME-011, TIME-012, TIME-013, API-020, VER-022

use aldis_core::DecodeMode;
use aldis_time::{
    Cuc, CucEpoch, CucFormat, CucValue, TimeCode, TimeFormat, decode_implicit, decode_with_pfield,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let epoch = if u.arbitrary()? {
        CucEpoch::Tai1958
    } else {
        CucEpoch::Agency
    };
    let Ok(mut format) = CucFormat::new(epoch, u.int_in_range(0..=8)?, u.int_in_range(0..=11)?)
    else {
        return Ok(());
    };
    if u.arbitrary()? {
        format = format.with_mission_bits(u.int_in_range(0..=3)?).unwrap();
    }
    let coarse: u64 = u.arbitrary()?;
    let fine: u128 = u.arbitrary()?;
    let nanos: u32 = u.int_in_range(0..=999_999_999)?;

    // ns conversions (TIME-011, OI-40): both truncate, and nothing carries into the coarse time.
    let (s, ns) = CucValue::new(coarse, fine).to_seconds_nanos();
    assert_eq!(s, coarse);
    // Independent bound: the top 64 fraction bits alone give the same value or one less.
    let coarse_model = ((fine >> 64) * 1_000_000_000) >> 64;
    assert!(u128::from(ns) == coarse_model || u128::from(ns) == coarse_model + 1);
    assert!(ns < 1_000_000_000);
    let bits = 8 * u32::from(format.fine_octets());
    if let Ok(c) = Cuc::from_seconds_nanos(format, coarse, nanos) {
        let v = c.value();
        let floor = (u128::from(nanos) << bits) / 1_000_000_000;
        assert!(floor < 1_u128 << bits);
        assert_eq!(v.coarse(), coarse);
        assert_eq!(v.fine(), floor.checked_shl(128 - bits).unwrap_or(0));
        assert_eq!(Cuc::new(format, v), Ok(c));
        // Value → ns → value never rises, and is exact if and only if the value is a multiple
        // of 2⁻⁹ s; with 4 or more fine octets ns → value → ns loses one ns off those multiples.
        let (s2, ns2) = v.to_seconds_nanos();
        let back = Cuc::from_seconds_nanos(format, s2, ns2).unwrap().value();
        assert_eq!(back.coarse(), coarse);
        assert!(back.fine() <= v.fine());
        assert_eq!(back == v, v.fine() & (u128::MAX >> 9) == 0);
        if bits >= 32 {
            let expected = if nanos.is_multiple_of(1_953_125) {
                nanos
            } else {
                nanos - 1
            };
            assert_eq!((s2, ns2), (coarse, expected));
        }
    }

    // Round trip of a representable value.
    let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
    let coarse = coarse >> (64 - 8 * u32::from(format.coarse_octets()));
    let code = TimeCode::Cuc(Cuc::new(format, CucValue::new(coarse, fine & mask)).unwrap());
    let mut out = [0_u8; 32];
    let n = code.encode(true, &mut out).unwrap();
    assert_eq!(
        decode_with_pfield(&out[..n], DecodeMode::Strict)
            .unwrap()
            .code(),
        code
    );
    let m = code.encode(false, &mut out).unwrap();
    let implicit =
        decode_implicit(&out[..m], &TimeFormat::Cuc(format), DecodeMode::Strict).unwrap();
    assert_eq!(implicit.code(), code);
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
