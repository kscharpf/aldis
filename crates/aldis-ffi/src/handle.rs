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
//! The storage holds uninitialized padding bytes (the slot is smaller than the struct, and its
//! own fields have padding), so a Rust caller must copy it as bytes (`copy_nonoverlapping`),
//! never by value (`a.words = b.words`), which reads uninitialized memory (Miri, VER-026).
//!
//! A call reaches the state in one of two ways. A small helper reads it by value
//! (`Handle::load`) and writes it back (`Handle::store`). An engine works **in place**
//! (CABI-040; c-abi.md §12 item 6): `Handle::read` and `Handle::update` lend the state in
//! the caller's storage to a closure as `&T` or `&mut T` for the duration of the call, so no
//! copy of the state is made on the stack (FOP-1's state is at least 16 KiB, the stack budget
//! 10 240 octets; OI-45). No Rust reference outlives the closure. That the reference aliases
//! nothing rests on the caller serializing calls on a handle (CABI-050) and on the call's
//! buffers not overlapping the storage (`Handle::range`); the handle does not enforce either
//! (c-abi.md §12 item 3 lists the input-buffer check the first engine must add) (GEN-071).
//! M3 and M4 use `update` for FOP-1, FARM-1, and the link engines.

use core::ptr::NonNull;

use aldis::core::time::Instant;
use aldis::core::{Error, ErrorKind, Result};

use crate::abi::{self, Out, invalid_argument};
use crate::error::aldis_error_detail_t;

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
    /// `f` is the whole engine step. `update` has no rollback: for the `decide`/`commit` form of
    /// API-058 `f` must call the pure `decide` on the state and, only if that succeeds, the
    /// infallible `commit` with the plan, so that an error leaves the state unchanged. `&mut self`
    /// keeps a second reference to the state *through this handle* out of `f` (no `read` inside
    /// it); that no other handle or pointer reaches the storage is the caller's obligation
    /// (CABI-050).
    #[expect(
        unsafe_code,
        reason = "borrows caller storage behind a checked tag (GEN-071)"
    )]
    pub(crate) fn update<R>(&mut self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: as for `read`; in addition `&mut self` means no reference to the state is made
        // through this handle while `f` runs. Another `Handle` on the same storage is excluded
        // only by CABI-050 (the caller serializes calls), not by the type.
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

/// Places a helper in `storage` and builds its state in place, without a copy of the state on the
/// stack (CABI-031, CABI-040; the state of FOP-1 is about 18 KB).
///
/// `build` writes the whole state into the slot it is given, after the tag. As for [`init`], a
/// live helper in `storage` is overwritten.
///
/// # Errors
/// `InvalidField` (no field) when `storage` is `NULL` or misaligned for `S` (CABI-022).
///
/// # Safety
/// A non-null, aligned `storage` must be valid for writes of one `S`. `build` must initialize
/// every octet of the state it is given.
#[expect(
    unsafe_code,
    reason = "initializes caller storage in place (CABI-031, CABI-040, GEN-071)"
)]
pub(crate) unsafe fn init_with<S: Storage>(
    storage: *mut S,
    build: impl FnOnce(*mut S::State),
) -> Result<()> {
    const { assert!(fits::<S, S::State>()) };
    if storage.is_null() || !storage.is_aligned() {
        return Err(invalid_argument());
    }
    let slot = storage.cast::<Slot<S::State>>();
    // SAFETY: non-null and aligned for `S`, hence for `Slot<T>`, and valid for
    // `size_of::<Slot<T>>()` ≤ `size_of::<S>()` octets (the caller's contract and `fits`). The
    // tag fields are written through raw field pointers, so no reference to the not yet
    // initialized state exists.
    unsafe {
        (&raw mut (*slot).magic).write(S::MAGIC);
        (&raw mut (*slot).generation).write(GENERATION);
        (&raw mut (*slot).addr).write(storage.addr());
        build(&raw mut (*slot).state);
    }
    Ok(())
}

/// An engine that [`place`] builds in caller storage: a plain Rust struct of a **head** and
/// **buffers** (plus, at most, fields valid when zero), whose offsets the `unsafe impl` takes from `offset_of!`
/// (CABI-040, GEN-012; OI-91 (b); M6 plan WP-3a).
///
/// The head is everything small, built by value from the configuration. The buffers are the
/// large octet arrays, never built by value: `place` zeroes them. The engines keep no `repr(C)`
/// layout constant and no `unsafe` (GEN-070); this trait and its impls are the one place that
/// knows the layout.
///
/// # Safety
/// An impl guarantees all of:
/// - `HEAD_OFFSET` and `BUFFERS_OFFSET` are the offsets of the fields of type `Head` and
///   `Buffers` in `Self`, taken with `core::mem::offset_of!`, never written by hand;
/// - every field of `Self` other than the head is valid when all of its bytes are zero (the
///   buffers, and any other field, are not written);
/// - `Buffers` is valid when all of its bytes are zero.
///
/// Then the slot of `Self` with every byte zero and a valid `Head` written at `HEAD_OFFSET` is a
/// valid `Self`, on which `finish_place` may run (the compile-time assertion of `place` keeps
/// the two fields disjoint and inside `Self`).
#[expect(
    unsafe_code,
    reason = "an impl asserts the layout of an engine (GEN-071)"
)]
pub(crate) unsafe trait InPlace: Sized {
    /// The small part, built from the configuration.
    type Head;
    /// The octet buffers, valid when all zero.
    type Buffers;
    /// The offset of the head field.
    const HEAD_OFFSET: usize;
    /// The offset of the buffers field.
    const BUFFERS_OFFSET: usize;

    /// Sets up what the zeroed buffers cannot express, once the engine is placed (for the
    /// `ChannelMux`, the Insert Zone slot at the idle pattern, LNK-034). Safe: it runs on a
    /// valid engine.
    fn finish_place(&mut self) {}
}

/// Whether `Head` and `Buffers` are disjoint and inside `E` (the assertion of [`place`]).
const fn layout_fits<E: InPlace>() -> bool {
    let head_end = E::HEAD_OFFSET.saturating_add(size_of::<E::Head>());
    let bufs_end = E::BUFFERS_OFFSET.saturating_add(size_of::<E::Buffers>());
    let disjoint = head_end <= E::BUFFERS_OFFSET || bufs_end <= E::HEAD_OFFSET;
    disjoint && head_end <= size_of::<E>() && bufs_end <= size_of::<E>()
}

/// Builds an engine at `engine` from its head, without an engine on the stack (CABI-040; the
/// engine is 18 KB for FOP-1): zeroes the whole slot (the buffers, the padding, which makes every
/// byte initialized for Miri, VER-026), writes `head` at its offset, and runs `finish_place`.
///
/// # Safety
/// `engine` is non-null, aligned for `E`, and valid for writes of one `E`.
#[expect(unsafe_code, reason = "initializes an engine in place (GEN-071)")]
pub(crate) unsafe fn place<E: InPlace>(engine: *mut E, head: E::Head) {
    const { assert!(layout_fits::<E>()) };
    // SAFETY: by the contract `engine` is valid for writes of `size_of::<E>()` octets. The
    // assertion above keeps the head in the engine, and `HEAD_OFFSET` is its offset (the
    // trait's contract), so the write is in bounds and aligned. After the zeroing and the head
    // every octet is initialized; `Buffers` is valid when zero and `Head` and `Buffers` are the
    // other fields valid when zero (the trait's contract), so the engine is valid and the reference is sound.
    // Zeroing and writing go through raw pointers, so no reference to uninitialized memory is
    // made before the engine is valid.
    unsafe {
        engine.cast::<u8>().write_bytes(0, size_of::<E>());
        engine
            .byte_add(E::HEAD_OFFSET)
            .cast::<E::Head>()
            .write(head);
        (*engine).finish_place();
    }
}

/// Builds the engine of the single-engine helper `S` in `storage` from its head, in place
/// (CABI-031, CABI-040): [`init_with`] and [`place`].
///
/// # Errors
/// As for [`init_with`].
///
/// # Safety
/// As for [`init_with`].
#[expect(unsafe_code, reason = "initializes caller storage in place (GEN-071)")]
pub(crate) unsafe fn init_engine<S: Storage>(
    storage: *mut S,
    head: <S::State as InPlace>::Head,
) -> Result<()>
where
    S::State: InPlace,
{
    // SAFETY: `init_with` passes a pointer to the state in the slot, valid for writes of one
    // engine and aligned for it; the contract of `init_with` is the caller's.
    unsafe { init_with(storage, |state| place(state, head)) }
}

/// The `repr(u8)` layout of an enum variant with one field (RFC 2195): the tag, then the field.
#[repr(C)]
struct Tagged<T> {
    tag: u8,
    payload: T,
}

/// A sum of engines: a `repr(u8)` enum whose variant tagged [`TAG`](Variant::TAG) has the one
/// field `E` (the engine of one frame protocol). [`engine_enum!`] declares the enum and the impls
/// from one list, so that the tag of the variant and the discriminant it is written for are one
/// expression.
///
/// # Safety
/// `Self` is a `repr(u8)` enum with a variant whose discriminant is `TAG` and whose only field
/// is an `E`.
#[expect(
    unsafe_code,
    reason = "an impl asserts the layout of an enum (GEN-071)"
)]
pub(crate) unsafe trait Variant<E>: Sized {
    /// The discriminant of the variant that holds an `E`.
    const TAG: u8;
}

/// Declares a `repr(u8)` sum of engines and its [`Variant`] impls. The discriminant of each variant
/// is its `tag` expression (a `u8` constant), written once.
macro_rules! engine_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident($engine:ty) = $tag:expr),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u8)]
        $vis enum $name {
            $($(#[$vmeta])* $variant($engine) = $tag),+
        }
        $(
            #[expect(unsafe_code, reason = "asserts the layout of the enum (GEN-071)")]
            // SAFETY: `$name` is the `repr(u8)` enum just declared, and the discriminant of the
            // variant holding the `$engine` is `$tag`, the same expression.
            unsafe impl $crate::handle::Variant<$engine> for $name {
                const TAG: u8 = $tag;
            }
        )+
    };
}
pub(crate) use engine_enum;

/// Builds the engine `E` as the variant of `S` that holds it, in place: writes the tag, then
/// [`place`]s the engine in the field (CABI-040).
///
/// # Safety
/// `state` is non-null, aligned for `S`, and valid for writes of one `S`.
#[expect(unsafe_code, reason = "initializes an enum variant in place (GEN-071)")]
unsafe fn place_variant<S: Variant<E>, E: InPlace>(state: *mut S, head: E::Head) {
    const { assert!(size_of::<Tagged<E>>() <= size_of::<S>()) };
    let tagged = state.cast::<Tagged<E>>();
    // SAFETY: by the contract of `Variant`, `S` has the layout of a union of `repr(C)` structs of
    // tag and field (RFC 2195), so `Tagged<E>` is the layout of this variant and in bounds
    // (the assertion above). The tag is written through a raw field pointer, so no reference to
    // the not yet initialized engine exists; `place` then initializes the engine in the field,
    // which is aligned and valid for writes of one `E`.
    unsafe {
        (&raw mut (*tagged).tag).write(S::TAG);
        place(&raw mut (*tagged).payload, head);
    }
}

/// Builds the engine `E` as the variant of the sum helper `S` that holds it, in `storage`
/// (CABI-031, CABI-040): [`init_with`] and [`place_variant`].
///
/// # Errors
/// As for [`init_with`].
///
/// # Safety
/// As for [`init_with`].
#[expect(unsafe_code, reason = "initializes caller storage in place (GEN-071)")]
pub(crate) unsafe fn init_variant<S: Storage, E: InPlace>(
    storage: *mut S,
    head: E::Head,
) -> Result<()>
where
    S::State: Variant<E>,
{
    // SAFETY: `init_with` passes a pointer to the state in the slot, valid for writes of one
    // `S::State` and aligned for it.
    unsafe { init_with(storage, |state| place_variant::<S::State, E>(state, head)) }
}

/// The C types of an engine's `_step` call and how a call's octets and result are read and
/// written (API-057, CABI-040). The engine's state is that of its [`Storage`].
pub(crate) trait StepIo: Storage {
    /// The C input struct.
    type Input: Copy;
    /// The C actions struct.
    type Actions;
    /// The octets the input carries: the pointer and the length of its `data` (API-060).
    fn data(input: &Self::Input) -> (*const u8, usize);
}

/// Declares the [`StepIo`] of the C handle `$storage`: its input and actions structs, whose `data`
/// and `data_len` members name the octets the input carries (API-060).
macro_rules! step_io {
    ($storage:ty, $input:ty, $actions:ty) => {
        impl $crate::handle::StepIo for $storage {
            type Input = $input;
            type Actions = $actions;

            fn data(input: &$input) -> (*const u8, usize) {
                (input.data, input.data_len)
            }
        }
    };
}
pub(crate) use step_io;

/// An engine without a clock: one input of `_step`, with the octets of `out` (API-057).
pub(crate) trait Step: StepIo {
    /// Applies `input` to `state`, with `data` its octets and `area` the octets of `out`, and
    /// writes the actions to `actions` only on success: an error leaves `state` unchanged and
    /// `actions` unwritten (API-058).
    ///
    /// # Errors
    /// The engine's.
    fn step(
        state: &mut Self::State,
        input: &Self::Input,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, Self::Actions>,
    ) -> Result<()>;
}

/// An engine with a clock: as [`Step`], at the instant `now`.
pub(crate) trait TimedStep: StepIo {
    /// As [`Step::step`], at `now`.
    ///
    /// # Errors
    /// The engine's.
    fn step(
        state: &mut Self::State,
        input: &Self::Input,
        now: Instant,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, Self::Actions>,
    ) -> Result<()>;
}

/// The checks every `_step` makes, then `run` on the state in place (CABI-022, CABI-040;
/// c-abi.md §12.2): the handle, the input struct (copied), `data` disjoint from the storage, `out`
/// disjoint from `data` and the storage, and `actions` disjoint from all of them.
///
/// # Safety
/// As for [`get`]; `input`, `out`, `out_cap`, and `actions` as in the `# Safety` of the C
/// entry point.
#[expect(
    unsafe_code,
    reason = "checks the buffers of a C call (CABI-022, GEN-071)"
)]
unsafe fn dispatch<S: StepIo>(
    h: *mut S,
    input: *const S::Input,
    out: *mut u8,
    out_cap: usize,
    actions: *mut S::Actions,
    run: impl FnOnce(&mut S::State, &S::Input, &[u8], &mut [u8], Out<'_, S::Actions>) -> Result<()>,
) -> Result<()> {
    // SAFETY: forwarded from the caller; each check below is of a caller pointer, valid as the
    // `# Safety` of the entry point states.
    unsafe {
        let mut h = get(h)?;
        let value = abi::read(input)?;
        let (data_ptr, data_len) = S::data(&value);
        let data = abi::bytes_disjoint(data_ptr, data_len, &[h.range()])?;
        let area = abi::bytes_mut(out, out_cap, &[(data_ptr, data_len), h.range()])?;
        let actions = abi::out_disjoint(
            actions,
            &[h.range(), (data_ptr, data_len), (out.cast_const(), out_cap)],
        )?;
        h.update(|state| run(state, &value, data, area, actions))
    }
}

/// `_step` of an engine without a clock (API-057, CABI-040): the checks of [`dispatch`], then
/// [`Step::step`]. Returns the status code of the call (CABI-020).
///
/// # Safety
/// As for the C entry point: `h` a handle of `S` or detectably invalid storage; `input` valid
/// for a read and its `data` for its length; `out` valid for `out_cap` octets of writes (`NULL`
/// when `out_cap` is 0); `actions` valid for one write; `err` `NULL` or valid for one write; no
/// other thread uses the storage (CABI-050).
#[expect(unsafe_code, reason = "dispatches a C entry point (GEN-071)")]
pub(crate) unsafe fn step<S: Step>(
    h: *mut S,
    input: *const S::Input,
    out: *mut u8,
    out_cap: usize,
    actions: *mut S::Actions,
    err: *mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: forwarded from the caller; `abi::call` and `dispatch` check them.
    unsafe {
        abi::call(err, || {
            dispatch::<S>(h, input, out, out_cap, actions, S::step)
        })
    }
}

/// `_step` of an engine with a clock (API-053, API-057, CABI-040): as [`step`], at the instant
/// `now_us` microseconds.
///
/// # Safety
/// As for [`step`].
#[expect(unsafe_code, reason = "dispatches a C entry point (GEN-071)")]
pub(crate) unsafe fn step_timed<S: TimedStep>(
    h: *mut S,
    input: *const S::Input,
    now_us: u64,
    out: *mut u8,
    out_cap: usize,
    actions: *mut S::Actions,
    err: *mut aldis_error_detail_t,
) -> u32 {
    let now = Instant::from_micros(now_us);
    // SAFETY: forwarded from the caller; `abi::call` and `dispatch` check them.
    unsafe {
        abi::call(err, || {
            dispatch::<S>(
                h,
                input,
                out,
                out_cap,
                actions,
                |state, value, data, area, actions| S::step(state, value, now, data, area, actions),
            )
        })
    }
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

    /// A plain Rust engine, with a layout the compiler chooses: a head, a padding hole, buffers.
    struct Toy {
        bufs: Bufs,
        head: ToyHead,
        marked: bool,
    }

    struct ToyHead {
        count: u64,
        flag: u8,
    }

    struct Bufs {
        octets: [u8; 37],
        counts: [u16; 3],
    }

    // SAFETY: `marked` is the only field besides `head` and `bufs`, and is valid when zero
    // (`false`); the offsets are `offset_of!`; `Bufs` is valid when zeroed.
    unsafe impl InPlace for Toy {
        type Head = ToyHead;
        type Buffers = Bufs;
        const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
        const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

        fn finish_place(&mut self) {
            self.marked = self.bufs.counts == [0; 3] && self.bufs.octets.iter().all(|b| *b == 0);
        }
    }

    #[test]
    fn place_zeroes_the_slot_writes_the_head_and_finishes() {
        // req: CABI-040
        // Storage that holds garbage, to show that every octet of the engine is overwritten.
        let mut slot = core::mem::MaybeUninit::<Toy>::uninit();
        // SAFETY: `slot` is valid for writes of one `Toy`; reads follow `place`, which
        // initializes all of it.
        unsafe {
            slot.as_mut_ptr()
                .cast::<u8>()
                .write_bytes(0xFF, size_of::<Toy>());
            place(slot.as_mut_ptr(), ToyHead { count: 7, flag: 3 });
            let toy = slot.assume_init_ref();
            assert_eq!((toy.head.count, toy.head.flag), (7, 3));
            assert!(toy.bufs.octets.iter().all(|b| *b == 0));
            assert_eq!(toy.bufs.counts, [0; 3]);
            // `finish_place` saw the zeroed buffers.
            assert!(toy.marked);
        }
    }

    /// A second engine, whose head is a plain integer.
    struct Toy2 {
        bufs: [u8; 5],
        head: u32,
    }

    // SAFETY: `head` and `bufs` are the only fields; the offsets are `offset_of!`; `[u8; 5]` is
    // valid when zeroed.
    unsafe impl InPlace for Toy2 {
        type Head = u32;
        type Buffers = [u8; 5];
        const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
        const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);
    }

    engine_enum! {
        #[expect(dead_code, reason = "the variants are written in place, never constructed")]
        enum Sum {
            One(Toy) = 7,
            Two(Toy2) = 9,
        }
    }

    /// The typed storage of a `Sum`: words enough for the slot and the larger engine.
    #[repr(C)]
    struct SumStorage {
        words: [u64; 12],
    }

    impl super::Storage for SumStorage {
        type State = Sum;
        const MAGIC: u32 = MAGIC;
    }

    #[test]
    fn a_variant_is_placed_under_its_tag() {
        // req: CABI-040
        assert_eq!(<Sum as Variant<Toy>>::TAG, 7);
        assert_eq!(<Sum as Variant<Toy2>>::TAG, 9);
        let mut storage = SumStorage { words: [!0; 12] };
        // SAFETY: `storage` is valid for writes of one `SumStorage`.
        unsafe {
            init_variant::<SumStorage, Toy2>(&raw mut storage, 41).unwrap();
            let h = get(&raw mut storage).unwrap();
            h.read(|sum| match sum {
                Sum::Two(e) => assert_eq!((e.head, e.bufs), (41, [0; 5])),
                Sum::One(_) => panic!("the tag of `Two` was not written"),
            });
            init_variant::<SumStorage, Toy>(&raw mut storage, ToyHead { count: 5, flag: 1 })
                .unwrap();
            let h = get(&raw mut storage).unwrap();
            h.read(|sum| match sum {
                Sum::One(e) => assert_eq!((e.head.count, e.head.flag, e.marked), (5, 1, true)),
                Sum::Two(_) => panic!("the tag of `One` was not written"),
            });
        }
    }

    #[test]
    fn the_layout_check_rejects_overlap_and_overrun() {
        // req: CABI-040
        #[expect(dead_code, reason = "only its size is used")]
        struct Bad<const H: usize, const B: usize>([u8; 16]);
        // SAFETY: only `layout_fits` reads the constants; `place` is never called on `Bad`.
        unsafe impl<const H: usize, const B: usize> InPlace for Bad<H, B> {
            type Head = [u8; 8];
            type Buffers = [u8; 8];
            const HEAD_OFFSET: usize = H;
            const BUFFERS_OFFSET: usize = B;
        }
        // The fields are 8 octets each in an engine of 16.
        const { assert!(layout_fits::<Bad<0, 8>>()) };
        const { assert!(layout_fits::<Bad<8, 0>>()) };
        // Overlap, in both orders.
        const { assert!(!layout_fits::<Bad<0, 4>>()) };
        const { assert!(!layout_fits::<Bad<4, 0>>()) };
        // Beyond the engine.
        const { assert!(!layout_fits::<Bad<0, 12>>()) };
        const { assert!(!layout_fits::<Bad<12, 0>>()) };
        const { assert!(layout_fits::<Toy>()) };
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
