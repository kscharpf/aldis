//! Run-time report of the build-time capacities (CABI-030, GEN-011; M2 plan D5).

use crate::abi;
use crate::capacities::{ALDIS_SPP_TRACKER_MAX_APIDS, ALDIS_TC_MAX_MAPS};
use crate::error::{ALDIS_OK, status};

/// The capacities this library was built with: the `ALDIS_*_MAX_*` constants (CABI-030).
///
/// Later ABI versions append fields; `struct_size` tells the library how many the caller's
/// struct has (CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_limits_t {
    /// `sizeof(aldis_limits_t)`, set by the caller before the call (CABI-033).
    pub struct_size: u32,
    /// `ALDIS_SPP_TRACKER_MAX_APIDS`: APIDs one `aldis_spp_tracker_t` tracks (SPP-011).
    pub spp_tracker_max_apids: u32,
    /// `ALDIS_TC_MAX_MAPS`: MAPs one `aldis_tc_*` call takes with a master channel (232.0
    /// Table 5-4; M2 plan D14).
    pub tc_max_maps: u32,
}

/// Fills `limits` with the capacities of this build (CABI-030).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` when `limits` is `NULL` or misaligned, or its `struct_size` is
/// smaller than `aldis_limits_t` (CABI-022).
///
/// # Safety
/// `limits` is valid for a read of its `struct_size` and one write of `aldis_limits_t`.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_limits(limits: *mut aldis_limits_t) -> i32 {
    // SAFETY: `limits` is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // `struct_size`.
    let checked = unsafe {
        abi::config(limits.cast_const()).and_then(|caller| {
            let out = abi::out(limits)?;
            out.write(aldis_limits_t {
                struct_size: caller.struct_size,
                spp_tracker_max_apids: u32::try_from(ALDIS_SPP_TRACKER_MAX_APIDS)
                    .unwrap_or(u32::MAX),
                tc_max_maps: u32::try_from(ALDIS_TC_MAX_MAPS).unwrap_or(u32::MAX),
            });
            Ok(())
        })
    };
    checked.map_or_else(status, |()| ALDIS_OK)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(unsafe_code, reason = "the tests call the C entry point")]
mod tests {
    use super::*;
    use crate::error::ALDIS_ERR_INVALID_FIELD;

    #[test]
    fn limits_report_the_build_capacities() {
        // req: CABI-030, CABI-033
        let mut l = aldis_limits_t {
            struct_size: u32::try_from(size_of::<aldis_limits_t>()).unwrap(),
            ..aldis_limits_t::default()
        };
        // SAFETY: `l` is a live, aligned struct.
        assert_eq!(unsafe { aldis_limits(&raw mut l) }, ALDIS_OK);
        assert_eq!(
            usize::try_from(l.spp_tracker_max_apids).ok(),
            Some(ALDIS_SPP_TRACKER_MAX_APIDS)
        );
        assert_eq!(usize::try_from(l.tc_max_maps).ok(), Some(ALDIS_TC_MAX_MAPS));
        let mut small = aldis_limits_t {
            struct_size: 4,
            ..aldis_limits_t::default()
        };
        // SAFETY: as above; NULL is rejected.
        unsafe {
            assert_eq!(aldis_limits(&raw mut small), ALDIS_ERR_INVALID_FIELD);
            assert_eq!(small.spp_tracker_max_apids, 0);
            assert_eq!(aldis_limits(core::ptr::null_mut()), ALDIS_ERR_INVALID_FIELD);
        }
    }
}
