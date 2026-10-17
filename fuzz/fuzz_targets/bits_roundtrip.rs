//! `bits::get`/`set` and `read_u16`/`write_u16` against a bit-by-bit model (GEN-050, GEN-051,
//! GEN-042).
#![no_main]

// req: GEN-050, GEN-051, GEN-042, VER-022

use aldis_core::ErrorKind;
use aldis_core::bits::{get, read_u16, set, write_u16};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// Bit `i` of `buf`, MSB first.
fn bit(buf: &[u8], i: usize) -> u64 {
    u64::from(buf[i / 8] >> (7 - i % 8) & 1)
}

fn check(u: &mut Unstructured<'_>) -> Result<()> {
    let mut buf: Vec<u8> = u.arbitrary()?;
    buf.truncate(16);
    let offset = usize::from(u.int_in_range(0_u8..=140)?);
    let width: u8 = u.int_in_range(0..=66)?;
    let value: u64 = u.arbitrary()?;

    let in_range = (1..=64).contains(&width) && offset + usize::from(width) <= buf.len() * 8;
    match get(&buf, offset, width) {
        Ok(v) => {
            assert!(in_range);
            let model = (0..usize::from(width)).fold(0, |acc, i| acc << 1 | bit(&buf, offset + i));
            assert_eq!(v, model);
        }
        Err(e) => {
            assert!(!in_range);
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
        }
    }

    let before = buf.clone();
    let fits = width == 64 || (width < 64 && value >> width == 0);
    match set(&mut buf, offset, width, value) {
        Ok(()) => {
            assert!(in_range && fits);
            assert_eq!(get(&buf, offset, width), Ok(value));
            let field = offset..offset + usize::from(width);
            for i in (0..buf.len() * 8).filter(|i| !field.contains(i)) {
                assert_eq!(
                    bit(&buf, i),
                    bit(&before, i),
                    "bit {i} outside the field changed"
                );
            }
        }
        Err(e) => {
            assert!(!(in_range && fits));
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert_eq!(buf, before, "GEN-042: buffer changed on error");
        }
    }

    let offset = offset / 8;
    let [.., hi, lo] = value.to_be_bytes();
    let word = u16::from_be_bytes([hi, lo]);
    let before = buf.clone();
    if write_u16(&mut buf, offset, word).is_ok() {
        assert_eq!(read_u16(&buf, offset), Ok(word));
        assert_eq!(get(&buf, offset * 8, 16), Ok(u64::from(word)));
    } else {
        assert!(offset + 2 > buf.len());
        assert_eq!(buf, before, "GEN-042: buffer changed on error");
        assert!(read_u16(&buf, offset).is_err());
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = check(&mut Unstructured::new(data));
});
