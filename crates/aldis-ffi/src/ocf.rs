//! C ABI of the Operational Control Field of `aldis-core` (132.0-B-3 §4.1.5, 732.0-B-5 §4.1.5,
//! 732.1-B-3 §4.1.5; CMN-010, CMN-011; M2 plan D14).
//!
//! [`aldis_ocf_classify`] tells which report four OCF octets hold, by bits 0–1, as
//! `Ocf::decode` does; [`aldis_ocf_check`] checks that they hold a given report, as the checked
//! constructors of `Type1Report`, `ProjectSpecificReport`, and `FsrReport` do. The OCF is a
//! four-octet input buffer (CABI-011): a frame view gives its offset and length in the frame
//! (CABI-012), and any other length is a CABI-022 violation. The CLCW stays opaque here: the
//! TC C ABI decodes it (CMN-011).

use aldis::core::Result;
use aldis::core::ocf::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};

use crate::abi::{self, invalid_argument};
use crate::mirror::{mirror, parse};

/// Length of the Operational Control Field in octets (132.0 §4.1.5.1, 732.0 §4.1.5.1,
/// 732.1 §4.1.5.2; CMN-010).
pub const ALDIS_OCF_LEN: u32 = 4;

/// The report an Operational Control Field holds (CMN-010).
///
/// A `uint32_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_ocf_report {
    /// Report: Type Flag (bit 0) `0`, a Type-1 Report, the CLCW, or on USLP a CLCW or a PLCW
    /// (132.0 §4.1.5.4 a, 732.0 §4.1.5.4 a, 732.1 §4.1.5.2.1 a; CMN-010, CMN-011).
    ALDIS_OCF_CLCW = 0,
    /// Report: bits 0–1 `10`, a project-specific Type-2 Report (132.0 §4.1.5.5 a, 732.0 §4.1.5.5 a,
    /// 732.1 §4.1.5.2.2 a; CMN-010).
    ALDIS_OCF_PROJECT_SPECIFIC = 1,
    /// Report: bits 0–1 `11`, a Type-2 Report holding an SDLS Frame Security Report
    /// (132.0 §4.1.5.5 b, 732.0 §4.1.5.5 b, 732.1 §4.1.5.2.2 b; CMN-010).
    ALDIS_OCF_SDLS_FSR = 2,
}
mirror!(pub aldis_ocf_report: u32 { ALDIS_OCF_CLCW, ALDIS_OCF_PROJECT_SPECIFIC, ALDIS_OCF_SDLS_FSR });

const _: () = assert!(ALDIS_OCF_LEN == 4 && OCF_LEN == 4);

/// The four octets of an OCF buffer (CABI-011, CABI-022).
///
/// # Safety
/// As for `abi::bytes`.
#[expect(unsafe_code, reason = "checked access to caller memory (CABI-022)")]
unsafe fn octets(ocf: *const u8, ocf_len: usize) -> Result<[u8; OCF_LEN]> {
    // SAFETY: forwarded from the caller.
    let bytes = unsafe { abi::bytes(ocf, ocf_len)? };
    <[u8; OCF_LEN]>::try_from(bytes).map_err(|_| invalid_argument())
}

/// The `ALDIS_OCF_*` constant of a report.
const fn report(ocf: Ocf) -> u32 {
    match ocf {
        Ocf::Clcw(_) => ALDIS_OCF_CLCW,
        Ocf::ProjectSpecific(_) => ALDIS_OCF_PROJECT_SPECIFIC,
        Ocf::SdlsFsr(_) => ALDIS_OCF_SDLS_FSR,
    }
}

/// Classifies the four OCF octets at `ocf` by bits 0–1 (CMN-010).
///
/// `report` receives `ALDIS_OCF_CLCW`, `ALDIS_OCF_PROJECT_SPECIFIC`, or `ALDIS_OCF_SDLS_FSR`
/// (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2). Every four octets are an OCF, and
/// the CLCW is not decoded (CMN-011).
///
/// # Errors
/// CABI-022 violations only: `ocf_len` other than `ALDIS_OCF_LEN`, `ocf` `NULL`, or `report`
/// `NULL` or misaligned.
///
/// # Safety
/// `ocf` is valid for reads of `ocf_len` octets; `report` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_ocf_classify(
    ocf: *const u8,
    ocf_len: usize,
    report: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let octets = octets(ocf, ocf_len)?;
            abi::out(report)?.write(self::report(Ocf::decode(octets)));
            Ok(())
        })
    }
}

/// Checks that the four OCF octets at `ocf` hold the report `report` (CMN-010).
///
/// `report` is an `ALDIS_OCF_*` constant. A writer checks its report so before it places it in
/// a frame (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` naming `ALDIS_FIELD_OCF_TYPE_FLAG` when the Type Flag is not the
/// report's, or `ALDIS_FIELD_OCF_REPORT_USE` when bit 1 of a Type-2 Report is not, with the
/// expected and found bit; CABI-022 violations (no field): `ocf_len` other than
/// `ALDIS_OCF_LEN`, `ocf` `NULL`, or `report` not an `ALDIS_OCF_*` constant.
///
/// # Safety
/// `ocf` is valid for reads of `ocf_len` octets; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_ocf_check(
    ocf: *const u8,
    ocf_len: usize,
    report: u32,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let octets = octets(ocf, ocf_len)?;
            match parse::<aldis_ocf_report>(report)? {
                aldis_ocf_report::ALDIS_OCF_CLCW => Type1Report::try_from(octets).map(drop),
                aldis_ocf_report::ALDIS_OCF_PROJECT_SPECIFIC => {
                    ProjectSpecificReport::try_from(octets).map(drop)
                }
                aldis_ocf_report::ALDIS_OCF_SDLS_FSR => FsrReport::try_from(octets).map(drop),
            }
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(unsafe_code, reason = "the tests call the C entry points")]
mod tests {
    use super::*;
    use crate::aldis_error_detail_t;
    use crate::error::{ALDIS_DETAIL_EXPECTED, ALDIS_ERR_INVALID_FIELD, ALDIS_OK};
    use crate::field::{ALDIS_FIELD_OCF_REPORT_USE, ALDIS_FIELD_OCF_TYPE_FLAG};

    fn classify(octets: &[u8]) -> (u32, u32) {
        let mut r = u32::MAX;
        // SAFETY: `octets` and `r` are live locals.
        let status = unsafe {
            aldis_ocf_classify(
                octets.as_ptr(),
                octets.len(),
                &raw mut r,
                core::ptr::null_mut(),
            )
        };
        (status, r)
    }

    fn check(octets: &[u8], report: u32) -> (u32, aldis_error_detail_t) {
        let mut d = aldis_error_detail_t {
            struct_size: u32::try_from(size_of::<aldis_error_detail_t>()).unwrap_or(0),
            ..aldis_error_detail_t::default()
        };
        // SAFETY: `octets` and `d` are live locals.
        let status = unsafe { aldis_ocf_check(octets.as_ptr(), octets.len(), report, &raw mut d) };
        (status, d)
    }

    #[test]
    fn bits_0_and_1_select_the_report() {
        // req: CMN-010, CMN-011
        assert_eq!(
            classify(&[0x01, 0x14, 0x0C, 0x2A]),
            (ALDIS_OK, ALDIS_OCF_CLCW)
        );
        assert_eq!(
            classify(&[0x7F, 0xFF, 0xFF, 0xFF]),
            (ALDIS_OK, ALDIS_OCF_CLCW)
        );
        assert_eq!(
            classify(&[0x80, 0, 0, 0]),
            (ALDIS_OK, ALDIS_OCF_PROJECT_SPECIFIC)
        );
        assert_eq!(classify(&[0xC0, 0, 0, 0]), (ALDIS_OK, ALDIS_OCF_SDLS_FSR));
    }

    #[test]
    fn the_ocf_is_four_octets() {
        // req: CABI-022, CABI-011
        assert_eq!(classify(&[0; 3]), (ALDIS_ERR_INVALID_FIELD, u32::MAX));
        assert_eq!(classify(&[0; 5]), (ALDIS_ERR_INVALID_FIELD, u32::MAX));
        assert_eq!(classify(&[]), (ALDIS_ERR_INVALID_FIELD, u32::MAX));
        let (status, d) = check(&[0; 3], ALDIS_OCF_CLCW);
        assert_eq!((status, d.field), (ALDIS_ERR_INVALID_FIELD, 0));
        let mut r = 0;
        // SAFETY: NULL with a length, and a NULL output, are rejected before any access.
        unsafe {
            let null = core::ptr::null_mut();
            assert_eq!(
                aldis_ocf_classify(core::ptr::null(), 4, &raw mut r, null),
                ALDIS_ERR_INVALID_FIELD
            );
            assert_eq!(
                aldis_ocf_classify([0_u8; 4].as_ptr(), 4, null.cast(), null),
                ALDIS_ERR_INVALID_FIELD
            );
        }
    }

    #[test]
    fn check_names_the_bit_that_differs() {
        // req: CMN-010, CABI-021
        let (clcw, ps, fsr) = (
            [0x01, 0x14, 0x0C, 0x2A],
            [0x80, 0x12, 0x34, 0x56],
            [0xC1, 0, 0, 0],
        );
        assert_eq!(check(&clcw, ALDIS_OCF_CLCW).0, ALDIS_OK);
        assert_eq!(check(&ps, ALDIS_OCF_PROJECT_SPECIFIC).0, ALDIS_OK);
        assert_eq!(check(&fsr, ALDIS_OCF_SDLS_FSR).0, ALDIS_OK);
        for (octets, report, field, expected, found) in [
            (ps, ALDIS_OCF_CLCW, ALDIS_FIELD_OCF_TYPE_FLAG, 0, 1),
            (
                clcw,
                ALDIS_OCF_PROJECT_SPECIFIC,
                ALDIS_FIELD_OCF_TYPE_FLAG,
                1,
                0,
            ),
            (
                fsr,
                ALDIS_OCF_PROJECT_SPECIFIC,
                ALDIS_FIELD_OCF_REPORT_USE,
                0,
                1,
            ),
            (clcw, ALDIS_OCF_SDLS_FSR, ALDIS_FIELD_OCF_TYPE_FLAG, 1, 0),
            (ps, ALDIS_OCF_SDLS_FSR, ALDIS_FIELD_OCF_REPORT_USE, 1, 0),
        ] {
            let (status, d) = check(&octets, report);
            assert_eq!(status, ALDIS_ERR_INVALID_FIELD);
            assert_eq!(
                (d.field, d.detail, d.expected, d.found),
                (field, ALDIS_DETAIL_EXPECTED, expected, found)
            );
        }
        let (status, d) = check(&clcw, 3);
        assert_eq!((status, d.field), (ALDIS_ERR_INVALID_FIELD, 0));
    }
}
