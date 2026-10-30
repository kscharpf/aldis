//! Arbitrary fields, header-size policy, and data through `encode` and the two-phase writer,
//! then `decode`: `decode(encode(x)) == x`, both encoders agree, and a rejected encode leaves
//! the output buffer unchanged (API-020…API-022, EPP-008, GEN-042).
#![no_main]

// req: API-020, API-021, API-022, API-023, EPP-008, EPP-010, GEN-042, VER-022

use aldis_epp::{
    EncapsulationFields, EncapsulationPacket, EncapsulationWriter, Epi, EpiExtension, EpiSet,
    EppConfig, ExtendedEpiSet, HeaderLen, HeaderSize, UserDefined, encode,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    // Raw parts go through `from_parts`; an extension without EPI 110 cannot exist (RS-020).
    let epi = Epi::new(u.int_in_range(0..=7)?).unwrap();
    let extension = EpiExtension::new(u.int_in_range(0..=15)?).unwrap();
    let user_defined = UserDefined::new(u.int_in_range(0..=15)?).unwrap();
    let Ok(fields) = EncapsulationFields::from_parts(epi, extension, user_defined) else {
        assert!(epi != Epi::EXTENDED && extension.get() != 0);
        return Ok(());
    };
    // Managed parameters drawn from the input; the check happens once, in `data_unit`.
    let max = u32::from(u.int_in_range(0_u16..=320)?);
    let min = u32::from(u.int_in_range(0_u16..=8)?).min(max);
    let config = EppConfig::new(
        min,
        max,
        EpiSet::from_bits(u.arbitrary()?),
        ExtendedEpiSet::from_bits(u.arbitrary()?),
    )
    .unwrap();
    let size = match u.int_in_range(0..=4)? {
        0 => HeaderSize::Auto,
        1 => HeaderSize::Fixed(HeaderLen::One),
        2 => HeaderSize::Fixed(HeaderLen::Two),
        3 => HeaderSize::Fixed(HeaderLen::Four),
        _ => HeaderSize::Fixed(HeaderLen::Eight),
    };
    let len = usize::from(u.int_in_range(0_u16..=300)?);
    let data = u.bytes(len.min(u.len()))?.to_vec();
    let cap = usize::from(u.int_in_range(0_u16..=320)?);
    let mut out = vec![0xA5_u8; cap];
    let Ok(unit) = config.data_unit(fields, &data) else {
        assert!(
            config.check(epi, Some(extension), data.len()).is_err()
                || (data.is_empty() && !epi.is_idle())
        );
        return Ok(());
    };
    match encode(&unit, size, &mut out) {
        Err(_) => assert!(
            out.iter().all(|&b| b == 0xA5),
            "rejected encode wrote output"
        ),
        Ok(n) => {
            let p = EncapsulationPacket::decode(&out).unwrap();
            assert_eq!(
                (p.len(), p.epi(), p.data()),
                (n, fields.epi(), data.as_slice())
            );
            if p.header_len().has_extension() {
                assert_eq!(p.extension(), Some(fields.extension()));
                assert_eq!(p.user_defined(), Some(fields.user_defined()));
                assert!(p.validate(&config).is_ok());
            }
            let mut two = vec![0x5A_u8; cap];
            let mut w =
                EncapsulationWriter::begin(&mut two, &config, fields, size, data.len()).unwrap();
            w.data_mut().copy_from_slice(&data);
            assert_eq!(w.finish(data.len()), Ok(n));
            assert_eq!(two[..n], out[..n]);
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
