//! C ABI of the Only-Idle-Data generator of `aldis-core` in caller-owned storage (CMN-020…023).
//!
//! The generator is that of 132.0-B-3 §4.1.4.6.2 and Annex D, 732.0-B-5 §4.1.4.1.5.2, and
//! 732.1-B-3 §4.1.4.1.10 (M2 plan D5, D14; CABI-030…032).
//!
//! An `aldis_oid_generator_t` follows the handle pattern of the stateful SPP helpers
//! ([`crate::spp_state`]): the caller declares the typed storage struct, `_init` places the
//! generator in it (the pointer to it is the handle), [`aldis_oid_generator_fill`] uses it, and
//! `_deinit` releases it. `_init` chooses the Fibonacci form, the reference (CMN-020), or the Galois form
//! (CMN-022); both give the same sequence. The generator is in its device start-up state after
//! `_init`, and every fill continues the sequence where the last one stopped (CMN-021,
//! CMN-023): one generator per physical channel, never reinitialized by reconfiguration
//! (CMN-021). It has no build-time capacity, so `aldis_limits` reports nothing for it.

use aldis::core::Result;
use aldis::core::oid::{OidGenerator, OidGeneratorGalois};

use crate::abi::{self, invalid_argument};
use crate::handle::{self, Storage};

/// [`aldis_oid_generator_init`] `form`: the Fibonacci form, the reference implementation
/// (132.0 Annex D; CMN-020).
pub const ALDIS_OID_FIBONACCI: u32 = 0;
/// [`aldis_oid_generator_init`] `form`: the Galois form, with the seed of 132.0 Annex D
/// (CMN-022).
pub const ALDIS_OID_GALOIS: u32 = 1;

/// 64-bit words of one `aldis_oid_generator_t` (CABI-031; M2 plan D5, D14).
pub const ALDIS_OID_GENERATOR_WORDS: usize = 4;

/// The state of an `aldis_oid_generator_t`: the generator in the form `_init` chose.
#[derive(Debug, Clone)]
pub(crate) enum GeneratorState {
    /// The Fibonacci form (CMN-020).
    Fibonacci(OidGenerator),
    /// The Galois form (CMN-022).
    Galois(OidGeneratorGalois),
}

impl GeneratorState {
    /// Fills `buf` with the next `buf.len()` octets of the sequence (CMN-023).
    fn fill(&mut self, buf: &mut [u8]) {
        match self {
            GeneratorState::Fibonacci(g) => g.fill(buf),
            GeneratorState::Galois(g) => g.fill(buf),
        }
    }
}

/// An OID generator in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (CMN-020…023, CABI-031, CABI-032).
///
/// The words are the library's: a C caller never reads or writes them.
#[repr(C)]
pub struct aldis_oid_generator_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_OID_GENERATOR_WORDS],
}

impl Storage for aldis_oid_generator_t {
    type State = GeneratorState;
    const MAGIC: u32 = 0x414C_4F47; // "ALOG"
}

/// Places an OID generator of the form `form` (`ALDIS_OID_FIBONACCI` or `ALDIS_OID_GALOIS`) in
/// `storage`, in its device start-up state (132.0 §4.1.4.6.2, Annex D; CMN-020, CMN-022).
///
/// `storage` is the handle (CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned storage, or a `form` that is no
/// `ALDIS_OID_*` constant; CABI-022 violations.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_oid_generator_t` and outlives its use; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_oid_generator_init(
    storage: *mut aldis_oid_generator_t,
    form: u32,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let state = match form {
                ALDIS_OID_FIBONACCI => GeneratorState::Fibonacci(OidGenerator::new()),
                ALDIS_OID_GALOIS => GeneratorState::Galois(OidGeneratorGalois::new()),
                _ => return Err(invalid_argument()),
            };
            handle::init(storage, state)
        })
    }
}

/// The generator behind `h`; also used by the OID frame writers of the frame codecs
/// ([`crate::tm`]).
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
pub(crate) unsafe fn generator(
    h: *mut aldis_oid_generator_t,
) -> Result<handle::Handle<GeneratorState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Fills the `len` octets at `out` with the next `len` octets of the OID sequence (CMN-023).
///
/// The octets are MSB first, and the generator advances by exactly `len` octets (132.0
/// §4.1.4.6.2; CMN-020, CMN-021). The first 20 octets after `_init` are those of 132.0 Annex D.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations, including `out` overlapping the storage. On
/// error the generator is unchanged and `out` unwritten.
///
/// # Safety
/// `h` is a handle from `aldis_oid_generator_init` whose storage is still valid, or detectably
/// invalid storage; `out` is valid for writes of `len` octets (or `NULL` with `len` 0); `err`
/// is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_oid_generator_fill(
    h: *mut aldis_oid_generator_t,
    out: *mut u8,
    len: usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = generator(h)?;
            let out = abi::bytes_mut(out, len, &[h.range()])?;
            let mut state = h.load();
            state.fill(out);
            h.store(state);
            Ok(())
        })
    }
}

/// Releases an OID generator: later calls with `h` fail with `ALDIS_ERR_INVALID_HANDLE`
/// (CABI-032). An invalid handle is ignored.
///
/// # Safety
/// `h` is `NULL`, a handle whose storage is still valid, or detectably invalid storage.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_oid_generator_deinit(h: *mut aldis_oid_generator_t) {
    // SAFETY: as `# Safety` states; `handle` checks the tag before writing.
    unsafe { handle::deinit(h) }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(unsafe_code, reason = "the tests call the C entry points")]
mod tests {
    use core::ptr::null_mut;

    use super::*;
    use crate::error::{ALDIS_ERR_INVALID_FIELD, ALDIS_ERR_INVALID_HANDLE, ALDIS_OK};

    /// The first 20 octets of the sequence, as 132.0 Annex D prints them.
    const ANNEX_D: [u8; 20] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6, 0xD8, 0x61, 0x45, 0x1F, 0x11, 0xF1, 0x97, 0x16, 0x72,
        0x3C, 0xBE, 0x7E, 0x00, 0xB1,
    ];

    fn init(g: &mut aldis_oid_generator_t, form: u32) -> i32 {
        // SAFETY: `g` is live storage of the published type.
        unsafe { aldis_oid_generator_init(g, form, null_mut()) }
    }

    const fn storage() -> aldis_oid_generator_t {
        aldis_oid_generator_t {
            words: [0; ALDIS_OID_GENERATOR_WORDS],
        }
    }

    #[test]
    fn both_forms_continue_the_annex_d_sequence() {
        // req: CMN-020, CMN-021, CMN-022, CMN-023
        for form in [ALDIS_OID_FIBONACCI, ALDIS_OID_GALOIS] {
            let mut g = storage();
            assert_eq!(init(&mut g, form), ALDIS_OK);
            let h = &raw mut g;
            let (mut a, mut b) = ([0_u8; 7], [0_u8; 13]);
            // SAFETY: `h` is the handle of `g`; the buffers are live locals; NULL with 0.
            unsafe {
                assert_eq!(
                    aldis_oid_generator_fill(h, a.as_mut_ptr(), 7, null_mut()),
                    ALDIS_OK
                );
                assert_eq!(
                    aldis_oid_generator_fill(h, null_mut(), 0, null_mut()),
                    ALDIS_OK
                );
                assert_eq!(
                    aldis_oid_generator_fill(h, b.as_mut_ptr(), 13, null_mut()),
                    ALDIS_OK
                );
            }
            assert_eq!((&a[..], &b[..]), (&ANNEX_D[..7], &ANNEX_D[7..]));
        }
    }

    #[test]
    fn init_checks_the_form_and_the_storage() {
        // req: CABI-031, CABI-022
        let mut g = storage();
        assert_eq!(init(&mut g, 2), ALDIS_ERR_INVALID_FIELD);
        // SAFETY: NULL is rejected before any write.
        let status =
            unsafe { aldis_oid_generator_init(null_mut(), ALDIS_OID_FIBONACCI, null_mut()) };
        assert_eq!(status, ALDIS_ERR_INVALID_FIELD);
    }

    #[test]
    fn handles_are_tagged_and_buffers_checked() {
        // req: CABI-032, CABI-022
        let mut g = storage();
        assert_eq!(init(&mut g, ALDIS_OID_GALOIS), ALDIS_OK);
        let h = &raw mut g;
        let mut buf = [0_u8; 4];
        // SAFETY: `h` is the handle of `g`; the buffers are live locals or rejected.
        unsafe {
            assert_eq!(
                aldis_oid_generator_fill(h, null_mut(), 1, null_mut()),
                ALDIS_ERR_INVALID_FIELD
            );
            // Derived from the handle, so that `h` stays valid (no new borrow of `g`).
            let inside = h.cast::<u8>();
            assert_eq!(
                aldis_oid_generator_fill(h, inside, 4, null_mut()),
                ALDIS_ERR_INVALID_FIELD
            );
            aldis_oid_generator_deinit(h);
            assert_eq!(
                aldis_oid_generator_fill(h, buf.as_mut_ptr(), 4, null_mut()),
                ALDIS_ERR_INVALID_HANDLE
            );
            assert_eq!(buf, [0; 4]);
        }
    }
}
