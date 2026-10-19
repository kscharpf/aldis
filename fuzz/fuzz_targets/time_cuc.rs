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

    // ns conversions (TIME-011): truncation down, ceiling up, carry into coarse.
    let (s, ns) = CucValue::new(coarse, fine).to_seconds_nanos();
    assert_eq!(s, coarse);
    // Independent bound: the top 64 fraction bits alone give the same value or one less.
    let coarse_model = ((fine >> 64) * 1_000_000_000) >> 64;
    assert!(u128::from(ns) == coarse_model || u128::from(ns) == coarse_model + 1);
    assert!(ns < 1_000_000_000);
    let bits = 8 * u32::from(format.fine_octets());
    if let Ok(v) = CucValue::from_seconds_nanos(coarse, nanos, &format) {
        let ceil = (u128::from(nanos) << bits).div_ceil(1_000_000_000);
        let expect_coarse = if ceil == 1 << bits {
            coarse + 1
        } else {
            coarse
        };
        assert_eq!(v.coarse(), expect_coarse);
        assert!(Cuc::new(format, v).is_ok());
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
