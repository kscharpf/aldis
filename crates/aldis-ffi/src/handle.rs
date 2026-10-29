//! Caller-owned storage for the stateful helpers (c-abi.md §5; CABI-031, CABI-032; M2 plan D5).
//!
//! A helper's state lives in a `Slot` at the start of the typed storage struct the caller
//! declares (`aldis_x_t`, a `uint64_t words[N]`), so the C compiler supplies its size and
//! alignment and no length crosses the ABI; a compile-time check proves that the slot fits the
//! struct (CABI-031). `_init` writes the slot's tag: a magic number per helper, the generation
//! of the slot layout, and the slot's own address. Every later call checks the tag and fails
//! with `ALDIS_ERR_INVALID_HANDLE` on uninitialized, deinitialized, foreign, or copied storage
//! (CABI-032). `_deinit` clears the magic.
//!
//! A call reaches the state in one of two ways. A small helper reads it by value
//! (`Handle::load`) and writes it back (`Handle::store`). An engine works **in place**
//! (CABI-040; c-abi.md §12 item 6): `Handle::read` and `Handle::update` lend the state in
//! the caller's storage to a closure as `&T` or `&mut T` for the duration of the call, so no
//! copy of the state is made on the stack (FOP-1's state is 16 KiB, the stack budget 10 240
//! octets; OI-45). No Rust reference outlives the closure, and the call's buffers are checked
//! not to overlap the storage (`Handle::range`), so the reference aliases nothing (GEN-071).
//! M3 and M4 use `update` for FOP-1, FARM-1, and the link engines.

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

    /// Lends the state to `f` as `&T` for the duration of the call; the state is not copied.
    #[expect(
        unsafe_code,
        reason = "borrows caller storage behind a checked tag (GEN-071)"
    )]
    pub(crate) fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        // SAFETY: as for `load`. The reference lives only inside `f`, which cannot keep it
        // (`R` does not borrow from the argument) and cannot reach the storage another way:
        // the caller serializes calls on a handle (CABI-050) and the call's buffers do not
        // overlap the storage (`range`).
        f(unsafe { &self.slot.as_ref().state })
    }

    /// Lends the state to `f` as `&mut T` for the duration of the call; the state is not copied
    /// (CABI-040).
    ///
    /// `f` is the whole engine step. For the `decide`/`commit` form of API-058 it calls the pure
    /// `decide` on the state, and, only if that succeeds, the infallible `commit` with the plan,
    /// so an error leaves the state unchanged. `&mut self` keeps a second reference to the state
    /// out of `f`.
    #[expect(
        unsafe_code,
        reason = "borrows caller storage behind a checked tag (GEN-071)"
    )]
    pub(crate) fn update<R>(&mut self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: as for `read`; in addition no other reference to the state exists, since
        // `&mut self` is the handle's only one and `f` receives the only reference it makes.
        f(unsafe { &mut self.slot.as_mut().state })
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

    /// The size of FOP-1's state at its default capacity (`capacities.toml` `[fop1]`): 16
    /// frames of 1 024 octets, their lengths, and a head counter.
    #[repr(C)]
    struct Window {
        frames: [[u8; 1024]; 16],
        lens: [u16; 16],
        head: u32,
    }

    /// Its storage struct, as the header declares it: 2 056 words.
    #[repr(C)]
    struct Big {
        words: [u64; 2056],
    }

    impl super::Storage for Big {
        type State = Window;
        const MAGIC: u32 = MAGIC;
    }

    impl Window {
        /// The pure part of a step: the plan, or an error that changes nothing (API-058).
        fn decide(&self, len: u16) -> Result<usize> {
            let at = usize::try_from(self.head).map_err(|_| invalid_argument())?;
            if usize::from(len) > 1024 || at >= self.lens.len() {
                return Err(invalid_argument());
            }
            Ok(at)
        }

        /// The infallible part: applies the plan.
        fn commit(&mut self, at: usize, len: u16) {
            if let Some(l) = self.lens.get_mut(at) {
                *l = len;
            }
            self.head = self.head.wrapping_add(1);
        }
    }

    /// Zeroed words for a `Big`, on the heap (the tests may allocate).
    fn big_words() -> Vec<u64> {
        vec![0; 2056]
    }

    const fn window() -> Window {
        Window {
            frames: [[0; 1024]; 16],
            lens: [0; 16],
            head: 0,
        }
    }

    #[test]
    fn a_large_state_is_borrowed_in_place() {
        // req: CABI-040
        const { assert!(size_of::<Window>() >= 16 * 1024) };
        let mut words = big_words();
        let big = words.as_mut_ptr().cast::<Big>();
        let base = (big).addr();
        let at = core::mem::offset_of!(Slot<Window>, state);
        // SAFETY: every pointer is to `big`.
        unsafe {
            init(big, window()).unwrap();
            let mut h = get(big).unwrap();
            // The closure sees the state where it lives in the caller's storage: no copy.
            h.read(|w| assert_eq!((&raw const *w).addr(), base.wrapping_add(at)));
            h.update(|w| assert_eq!((&raw const *w).addr(), base.wrapping_add(at)));
            h.update(|w| w.commit(0, 10));
            assert_eq!(h.read(|w| (w.lens[0], w.head)), (10, 1));
            // A second handle sees the update: it was made in the storage.
            assert_eq!(get(big).unwrap().read(|w| w.head), 1);
        }
    }

    #[test]
    fn a_failed_step_leaves_the_state() {
        // req: CABI-040
        let mut words = big_words();
        let big = words.as_mut_ptr().cast::<Big>();
        // SAFETY: every pointer is to `big`.
        unsafe {
            init(big, window()).unwrap();
            let mut h = get(big).unwrap();
            let step = |h: &mut Handle<Window>, len: u16| {
                h.update(|w| {
                    let at = w.decide(len)?;
                    w.commit(at, len);
                    Ok(())
                })
            };
            assert_eq!(step(&mut h, 5), Ok(()));
            assert_eq!(step(&mut h, 2000), Err(invalid_argument()));
            assert_eq!(h.read(|w| (w.lens[0], w.lens[1], w.head)), (5, 0, 1));
        }
    }

    #[test]
    fn engine_storage_is_tagged() {
        // req: CABI-032
        let mut words = big_words();
        let big = words.as_mut_ptr().cast::<Big>();
        // SAFETY: every pointer is to `big`.
        unsafe {
            assert_eq!(
                get(big).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
            init(big, window()).unwrap();
            deinit(big);
            assert_eq!(
                get(big).map(|_| ()),
                Err(Error::new(ErrorKind::InvalidHandle))
            );
        }
    }
}
