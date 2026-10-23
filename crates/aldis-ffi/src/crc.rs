//! C ABI of the Frame Error Control Field CRC-16 of `aldis-core` (132.0-B-3 §4.1.6,
//! 232.0-B-4 §4.1.4, 732.0-B-5 §4.1.6, 732.1-B-3 Annex B; CMN-001, CMN-004; M2 plan D14).
//!
//! [`aldis_crc16`] computes the CRC with the implementation `aldis-core` was built with (bitwise
//! by default, the table with its feature `crc-table`; both give the same result, CMN-004). The
//! frame codecs place and check the FECF themselves; this function is for callers that compute
//! it over their own octets, such as the frame differential tests of track C (M2 plan D14).

use aldis::core::crc::crc16;

use crate::abi;

/// The CRC-16 of the `len` octets at `data` (132.0 §4.1.6.2; CMN-001, CMN-004).
///
/// The register is preset to all ones: `0x29B1` for the nine octets of `"123456789"`
/// (CMN-002), `0xFFFF` for none. Over a whole frame including its FECF it is the syndrome, 0
/// exactly when no error is detected (132.0 §4.1.6.3; CMN-003).
///
/// # Errors
/// CABI-022 violations: `data` `NULL` with `len` above 0, a range that wraps, or `crc` `NULL`
/// or misaligned.
///
/// # Safety
/// `data` is valid for reads of `len` octets (or `NULL` with `len` 0); `crc` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_crc16(
    data: *const u8,
    len: usize,
    crc: *mut u16,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let data = abi::bytes(data, len)?;
            abi::out(crc)?.write(crc16(data));
            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(unsafe_code, reason = "the tests call the C entry point")]
mod tests {
    use core::ptr::{null, null_mut};

    use super::aldis_crc16;
    use crate::error::{ALDIS_ERR_INVALID_FIELD, ALDIS_OK};

    #[test]
    fn crc16_of_a_buffer() {
        // req: CMN-001, CMN-002, CMN-003, CABI-022
        let mut crc = 0_u16;
        let frame = *b"123456789\x29\xB1";
        // SAFETY: every pointer is to a live local, or NULL with length 0, or rejected.
        unsafe {
            assert_eq!(
                aldis_crc16(frame.as_ptr(), 9, &raw mut crc, null_mut()),
                ALDIS_OK
            );
            assert_eq!(crc, 0x29B1);
            assert_eq!(aldis_crc16(null(), 0, &raw mut crc, null_mut()), ALDIS_OK);
            assert_eq!(crc, 0xFFFF);
            assert_eq!(
                aldis_crc16(frame.as_ptr(), 11, &raw mut crc, null_mut()),
                ALDIS_OK
            );
            assert_eq!(crc, 0, "the syndrome of a frame with its FECF");
            assert_eq!(
                aldis_crc16(null(), 1, &raw mut crc, null_mut()),
                ALDIS_ERR_INVALID_FIELD
            );
            assert_eq!(
                aldis_crc16(frame.as_ptr(), 1, null_mut(), null_mut()),
                ALDIS_ERR_INVALID_FIELD
            );
        }
    }
}
