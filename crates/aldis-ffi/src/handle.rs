//! Caller-owned storage for the stateful helpers (c-abi.md §5; CABI-031, CABI-032; M2 plan D5).
//!
//! A helper's state lives in a `Slot` at the start of the typed storage struct the caller
//! declares (`aldis_x_t`, a `uint64_t words[N]`), so the C compiler supplies its size and
//! alignment and no length crosses the ABI; a compile-time check proves that the slot fits the
//! struct (CABI-031). `_init` writes the slot's tag: a magic number per helper, the generation
//! of the slot layout, and the slot's own address. Every later call checks the tag and fails
//! with `ALDIS_ERR_INVALID_HANDLE` on uninitialized, deinitialized, foreign, or copied storage
//! (CABI-032). `_deinit` clears the magic. The state is read by value at the
//! start of a call and written back at its end, so no Rust reference to caller storage lives
//! across the call or aliases the call's buffers (GEN-071). M3 and M4 reuse this pattern for
//! FOP-1, FARM-1, and the link engines.

use core::ptr::NonNull;

use aldis::core::{Error, ErrorKind, Result};

use crate::abi::invalid_argument;

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

/// The typed storage struct of one helper (`aldis_x_t`), bound to the state it holds and the
/// magic of its tag, so that no call pairs a storage struct with another helper's state
/// (RS-020; CABI-031, CABI-032).
pub(crate) trait Storage {
    /// The state that `_init` places in the storage.
    type State;
    /// The magic of the helper's tag: one per helper (CABI-032).
    const MAGIC: u32;
}

/// A checked handle: the slot it points to carries the tag of its helper.
#[derive(Debug)]
pub(crate) struct Handle<T> {
    slot: NonNull<Slot<T>>,
    /// The size of the whole storage struct, which the call's output buffers must not overlap.
    size: usize,
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
        (self.slot.as_ptr().cast_const().cast::<u8>(), self.size)
    }
}

/// Places `state` in `storage` and returns the handle (CABI-031, CABI-032).
///
/// The build fails when the slot of the helper's state does not fit `S` (CABI-031). A live
/// helper in `storage` is overwritten: its tag cannot tell live storage from the garbage of an
/// uninitialized object (CABI-031).
///
/// # Errors
/// `InvalidField` (no field) when `storage` is `NULL` or misaligned for `S` (CABI-022).
///
/// # Safety
/// A non-null, aligned `storage` must be valid for writes of one `S`.
#[expect(unsafe_code, reason = "initializes caller storage (CABI-031, GEN-071)")]
pub(crate) unsafe fn init<S: Storage>(storage: *mut S, state: S::State) -> Result<()> {
    const { assert!(fits::<S, S::State>()) };
    if storage.is_null() || !storage.is_aligned() {
        return Err(invalid_argument());
    }
    let slot = storage.cast::<Slot<S::State>>();
    // SAFETY: non-null and aligned for `S`, checked above, hence for `Slot<T>` (its alignment is
    // at most that of `S`); valid for `size_of::<Slot<T>>()` ≤ `size_of::<S>()` octets by the
    // caller's contract and the check of `fits`.
    unsafe {
        slot.write(Slot {
            magic: S::MAGIC,
            generation: GENERATION,
            addr: storage.addr(),
            state,
        });
    }
    Ok(())
}

/// The handle `storage` checked against the magic of `S` (CABI-032).
///
/// # Errors
/// `InvalidField` (no field) for `NULL` or a misaligned pointer (CABI-022); `InvalidHandle`
/// when the tag is not the one `init` wrote for this helper at this address.
///
/// # Safety
/// A non-null, aligned `storage` must be valid for reads of the tag (16 octets) and, when the tag
/// matches, for reads and writes of the whole `S` during the call.
#[expect(
    unsafe_code,
    reason = "reads the tag of caller storage (CABI-032, GEN-071)"
)]
pub(crate) unsafe fn get<S: Storage>(storage: *mut S) -> Result<Handle<S::State>> {
    const { assert!(fits::<S, S::State>()) };
    let storage = match NonNull::new(storage) {
        Some(s) if s.as_ptr().is_aligned() => s,
        _ => return Err(invalid_argument()),
    };
    let slot = storage.cast::<Slot<S::State>>();
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
    if m != S::MAGIC || m == DEAD || g != GENERATION || a != p.addr() {
        return Err(Error::new(ErrorKind::InvalidHandle));
    }
    Ok(Handle {
        slot,
        size: size_of::<S>(),
    })
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
pub(crate) unsafe fn deinit<S: Storage>(storage: *mut S) {
    // SAFETY: as for `get`, whose checks pass before the tag is written.
    unsafe {
        if let Ok(h) = get(storage) {
            (&raw mut (*h.slot.as_ptr()).magic).write(DEAD);
        }
    }
}

/// Whether the slot of `T` fits, in size and alignment, in the storage struct `S`.
const fn fits<S, T>() -> bool {
    size_of::<Slot<T>>() <= size_of::<S>() && align_of::<Slot<T>>() <= align_of::<S>()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(unsafe_code, reason = "the tests place slots in local storage")]
mod tests {
    use super::*;

    const MAGIC: u32 = 0x5445_5354;

    /// A typed storage struct as the header declares them: four words.
    #[repr(C)]
    struct Words {
        words: [u64; 4],
    }

    /// Storage of a `u32` state.
    #[repr(C)]
    struct Storage(Words);

    /// Storage of another helper, with another magic.
    #[repr(C)]
    struct Other(Words);

    impl super::Storage for Storage {
        type State = u32;
        const MAGIC: u32 = MAGIC;
    }

    impl super::Storage for Other {
        type State = u32;
        const MAGIC: u32 = MAGIC + 1;
    }

    /// Storage whose magic is the one of released storage: it never matches.
    #[repr(C)]
    struct Released(Words);

    impl super::Storage for Released {
        type State = u32;
        const MAGIC: u32 = DEAD;
    }

    const fn storage() -> Storage {
        Storage(Words { words: [0; 4] })
    }

    #[test]
    fn init_checks_the_pointer() {
        // req: CABI-031
        let mut s = storage();
        // SAFETY: every pointer is NULL or to `s`, or an unaligned address that is never read.
        unsafe {
            assert_eq!(
                init(core::ptr::null_mut::<Storage>(), 7_u32),
                Err(invalid_argument())
            );
            let odd = (&raw mut s).wrapping_byte_add(4);
            assert_eq!(init(odd, 7_u32), Err(invalid_argument()));
            init(&raw mut s, 7_u32).unwrap();
            let h = get(&raw mut s).unwrap();
            assert_eq!(h.load(), 7);
            h.store(9);
            assert_eq!(get(&raw mut s).unwrap().load(), 9);
            assert_eq!(
                h.range(),
                ((&raw const s).cast::<u8>(), size_of::<Storage>())
            );
        }
    }

    #[test]
    fn init_replaces_live_storage() {
        // req: CABI-031
        let mut s = storage();
        // SAFETY: every pointer is to `s`.
        unsafe {
            init(&raw mut s, 7_u32).unwrap();
            init(&raw mut s, 3_u32).unwrap();
            assert_eq!(get(&raw mut s).unwrap().load(), 3);
        }
    }

    #[test]
    fn a_slot_fits_its_storage() {
        // req: CABI-031
        assert!(fits::<Words, u32>());
        assert!(fits::<Words, [u64; 2]>());
        assert!(!fits::<Words, [u64; 3]>());
        assert!(!fits::<[u32; 8], u32>());
    }

    #[test]
    fn handles_carry_a_tag() {
        // req: CABI-032
        let mut two = [storage(), storage()];
        let base = two.as_mut_ptr();
        // SAFETY: every pointer is NULL or into `two`.
        unsafe {
            let first = base;
            // Never initialized: zeroed storage.
            assert_eq!(
                get(first).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            init(first, 1_u32).unwrap();
            // Foreign: another helper's magic.
            assert_eq!(
                get(first.cast::<Other>()).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            // Copied elsewhere: the address no longer matches.
            let copy = base.add(1);
            core::ptr::copy_nonoverlapping(first, copy, 1);
            assert_eq!(
                get(copy).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            // Another generation.
            (&raw mut (*first.cast::<Slot<u32>>()).generation).write(GENERATION + 1);
            assert!(get(first).is_err());
            (&raw mut (*first.cast::<Slot<u32>>()).generation).write(GENERATION);
            assert!(get(first).is_ok());
            // Deinitialized.
            deinit(first);
            assert_eq!(
                get(first).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            deinit(first);
            assert_eq!(
                get(first.cast::<Released>()).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            assert_eq!(
                get(core::ptr::null_mut::<Storage>()).map(|_| ()),
                Err(invalid_argument())
            );
        }
    }
}
