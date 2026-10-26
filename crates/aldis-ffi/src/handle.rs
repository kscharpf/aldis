//! Caller-owned storage for the stateful helpers (c-abi.md §5; CABI-031, CABI-032; M2 plan D5).
//!
//! A helper's state lives in a `Slot` at the start of storage the caller provides, of at
//! least the helper's `ALDIS_*_STORAGE_SIZE` octets and aligned to its `ALDIS_*_STORAGE_ALIGN`.
//! `_init` checks both (CABI-031) and writes the slot's tag: a magic number per helper, the
//! generation of the slot layout, and the slot's own address. Every later call checks the
//! tag and fails with `ALDIS_ERR_INVALID_HANDLE` on uninitialized, deinitialized, foreign, or
//! copied storage (CABI-032). `_deinit` clears the magic. The state is read by value at the
//! start of a call and written back at its end, so no Rust reference to caller storage lives
//! across the call or aliases the call's buffers (GEN-071). M3 and M4 reuse this pattern for
//! FOP-1, FARM-1, and the link engines.

use core::ptr::NonNull;

use aldis::core::{Error, ErrorKind, Result};

use crate::abi::invalid_argument;

/// Alignment, in octets, of every helper's storage: a C caller gets it with a `uint64_t`
/// array (CABI-031).
pub const ALDIS_STORAGE_ALIGN: usize = 8;

/// The slot layout version, part of every tag: storage that another layout initialized is
/// foreign (CABI-032).
const GENERATION: u32 = 1;

/// The magic of storage that `_deinit` released.
const DEAD: u32 = 0;

/// The storage of one helper: its tag, then its state (CABI-032).
#[repr(C)]
#[derive(Debug)]
pub(crate) struct Slot<T> {
    magic: u32,
    generation: u32,
    /// The address of the slot when it was initialized: a copy elsewhere is foreign.
    addr: usize,
    state: T,
}

/// A checked handle: the slot it points to carries the tag of its helper.
#[derive(Debug)]
pub(crate) struct Handle<T> {
    slot: NonNull<Slot<T>>,
}

impl<T> Handle<T> {
    /// The helper's state, by value.
    #[expect(
        unsafe_code,
        reason = "reads caller storage behind a checked tag (GEN-071)"
    )]
    pub(crate) fn load(&self) -> T {
        let slot = self.slot.as_ptr();
        // SAFETY: `get` checked that the slot is non-null, aligned, and tagged by `init`, which
        // wrote an initialized `T`; the caller's contract keeps the storage valid for the call.
        unsafe { (&raw const (*slot).state).read() }
    }

    /// Writes the helper's state back.
    #[expect(
        unsafe_code,
        reason = "writes caller storage behind a checked tag (GEN-071)"
    )]
    pub(crate) fn store(&self, state: T) {
        let slot = self.slot.as_ptr();
        // SAFETY: as for `load`; the old state has no drop glue to run (`T: Copy` or plain
        // data), so overwriting it is sound.
        unsafe { (&raw mut (*slot).state).write(state) }
    }

    /// The storage's address range, which the call's output buffers must not overlap.
    pub(crate) const fn range(&self) -> (*const u8, usize) {
        (
            self.slot.as_ptr().cast_const().cast::<u8>(),
            size_of::<Slot<T>>(),
        )
    }
}

/// Places `state` in `storage` and returns the handle (CABI-031, CABI-032).
///
/// # Errors
/// `InvalidField` (no field) when `storage` is `NULL` or not aligned to
/// [`ALDIS_STORAGE_ALIGN`]; `BufferTooSmall` (`needed` = `size`) when `storage_len` is below
/// `size`, the published storage size.
///
/// # Safety
/// A non-null `storage` must be valid for writes of `storage_len` octets.
#[expect(unsafe_code, reason = "initializes caller storage (CABI-031, GEN-071)")]
pub(crate) unsafe fn init<T>(
    storage: *mut core::ffi::c_void,
    storage_len: usize,
    size: usize,
    magic: u32,
    state: T,
) -> Result<*mut Slot<T>> {
    if storage.is_null() || !storage.addr().is_multiple_of(ALDIS_STORAGE_ALIGN) {
        return Err(invalid_argument());
    }
    let slot = storage.cast::<Slot<T>>();
    if !slot.is_aligned() {
        return Err(invalid_argument());
    }
    if storage_len < size || storage_len < size_of::<Slot<T>>() {
        return Err(Error::buffer_too_small(size));
    }
    // SAFETY: non-null and aligned, checked above, and valid for `size_of::<Slot<T>>()` ≤
    // `storage_len` octets by the caller's contract.
    unsafe {
        slot.write(Slot {
            magic,
            generation: GENERATION,
            addr: storage.addr(),
            state,
        });
    }
    Ok(slot)
}

/// The handle `slot` checked against `magic` (CABI-032).
///
/// # Errors
/// `InvalidField` (no field) for `NULL` or a misaligned pointer (CABI-022); `InvalidHandle`
/// when the tag is not the one `init` wrote for this helper at this address.
///
/// # Safety
/// A non-null, aligned `slot` must be valid for reads of the tag (16 octets) and, when the tag
/// matches, for reads and writes of the whole slot during the call.
#[expect(
    unsafe_code,
    reason = "reads the tag of caller storage (CABI-032, GEN-071)"
)]
pub(crate) unsafe fn get<T>(slot: *mut Slot<T>, magic: u32) -> Result<Handle<T>> {
    let slot = match NonNull::new(slot) {
        Some(s) if s.as_ptr().is_aligned() => s,
        _ => return Err(invalid_argument()),
    };
    let p = slot.as_ptr();
    // SAFETY: `slot` is NonNull and aligned, checked above; the caller's contract makes the
    // tag readable. Only the tag's plain integers are read before it is checked.
    let (m, g, a) = unsafe {
        (
            (&raw const (*p).magic).read(),
            (&raw const (*p).generation).read(),
            (&raw const (*p).addr).read(),
        )
    };
    if m != magic || m == DEAD || g != GENERATION || a != p.addr() {
        return Err(Error::new(ErrorKind::InvalidHandle));
    }
    Ok(Handle { slot })
}

/// Releases the storage of a valid handle: later calls fail with `InvalidHandle` (CABI-032).
/// Anything else is left alone.
///
/// # Safety
/// As for [`get`].
#[expect(
    unsafe_code,
    reason = "clears the tag of caller storage (CABI-032, GEN-071)"
)]
pub(crate) unsafe fn deinit<T>(slot: *mut Slot<T>, magic: u32) {
    // SAFETY: as for `get`, whose checks pass before the tag is written.
    unsafe {
        if let Ok(h) = get(slot, magic) {
            (&raw mut (*h.slot.as_ptr()).magic).write(DEAD);
        }
    }
}

/// The size of a slot for `T`, for the compile-time checks of the published sizes.
pub(crate) const fn slot_size<T>() -> usize {
    size_of::<Slot<T>>()
}

/// The alignment of a slot for `T`, for the compile-time checks of the published alignment.
pub(crate) const fn slot_align<T>() -> usize {
    align_of::<Slot<T>>()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(unsafe_code, reason = "the tests place slots in local storage")]
mod tests {
    use super::*;

    const MAGIC: u32 = 0x5445_5354;

    #[test]
    fn init_checks_size_and_alignment() {
        // req: CABI-031
        let mut words = [0_u64; 4];
        let base = words.as_mut_ptr().cast::<core::ffi::c_void>();
        // SAFETY: every pointer is NULL or into `words`, which has 32 octets.
        unsafe {
            assert_eq!(
                init(base, 31, 32, MAGIC, 7_u32).map(|_| ()),
                Err(Error::buffer_too_small(32))
            );
            assert_eq!(
                init(core::ptr::null_mut(), 32, 32, MAGIC, 7_u32).map(|_| ()),
                Err(invalid_argument())
            );
            let odd = base
                .cast::<u8>()
                .wrapping_add(4)
                .cast::<core::ffi::c_void>();
            assert_eq!(
                init(odd, 28, 16, MAGIC, 7_u32).map(|_| ()),
                Err(invalid_argument())
            );
            let slot = init(base, 32, 32, MAGIC, 7_u32).unwrap();
            let h = get(slot, MAGIC).unwrap();
            assert_eq!(h.load(), 7);
            h.store(9);
            assert_eq!(get(slot, MAGIC).unwrap().load(), 9);
            assert_eq!(
                h.range(),
                (base.cast_const().cast::<u8>(), slot_size::<u32>())
            );
        }
    }

    #[test]
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "offset 32 of a `u64` array is aligned"
    )]
    fn handles_carry_a_tag() {
        // req: CABI-032
        let mut words = [0_u64; 8];
        let base = words.as_mut_ptr().cast::<core::ffi::c_void>();
        // SAFETY: every pointer is NULL or into `words`.
        unsafe {
            let slot = base.cast::<Slot<u32>>();
            // Never initialized: zeroed storage.
            assert_eq!(
                get(slot, MAGIC).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            let slot = init(base, 64, 16, MAGIC, 1_u32).unwrap();
            // Foreign: another helper's magic.
            assert_eq!(
                get(slot, MAGIC + 1).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            // Copied elsewhere: the address no longer matches.
            let copy = base.cast::<u8>().wrapping_add(32).cast::<Slot<u32>>();
            core::ptr::copy_nonoverlapping(slot, copy, 1);
            assert_eq!(
                get(copy, MAGIC).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            // Another generation.
            (&raw mut (*slot).generation).write(GENERATION + 1);
            assert!(get(slot, MAGIC).is_err());
            (&raw mut (*slot).generation).write(GENERATION);
            assert!(get(slot, MAGIC).is_ok());
            // Deinitialized.
            deinit(slot, MAGIC);
            assert_eq!(
                get(slot, MAGIC).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            deinit(slot, MAGIC);
            assert_eq!(
                get(slot, DEAD).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            assert_eq!(
                get::<u32>(core::ptr::null_mut(), MAGIC).map(|_| ()),
                Err(invalid_argument())
            );
            assert_eq!(slot_align::<u32>(), 8);
        }
    }
}
