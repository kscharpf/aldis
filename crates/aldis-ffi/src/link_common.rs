//! Helpers shared by the link engines of the C ABI: building an engine or an enum variant in
//! place (CABI-040, #240) and the C form of an output range (API-061).

use aldis::core::engine::OutRange;

use crate::range::aldis_range_t;

/// Builds an engine at `engine` from its head, without an engine on the stack (CABI-040, #240;
/// the pattern of `aldis::link::inplace`): the head is written at the offset of its field and the
/// octet buffers are zeroed at theirs, which is their empty state. Every field of the engine is
/// then initialized. The arguments are the engine type `$e`, its head type `$h` and buffers type
/// `$b`, the pointer, and the head.
macro_rules! engine_in_place {
    ($e:ty, $h:ty, $b:ty, $ptr:expr, $head:expr) => {{
        const {
            assert!(
                <$e>::HEAD_OFFSET.saturating_add(size_of::<$h>()) <= <$e>::BUFFERS_OFFSET
                    && <$e>::BUFFERS_OFFSET.saturating_add(size_of::<$b>()) <= size_of::<$e>()
            );
        };
        let engine: *mut $e = $ptr;
        // SAFETY: `$ptr` is valid for writes of one `$e` (the macro's caller). `$e` is
        // `repr(C)` with the fields head (`$h`) and buffers (`$b`) in that order, at the
        // offsets `HEAD_OFFSET` and `BUFFERS_OFFSET` (`offset_of!`), and the assertion above
        // keeps both in the engine and apart. The buffers are plain arrays of octets, valid
        // when zeroed. The writes go through raw pointers, so no reference to uninitialized
        // memory is made.
        unsafe {
            engine.byte_add(<$e>::HEAD_OFFSET).cast::<$h>().write($head);
            engine
                .byte_add(<$e>::BUFFERS_OFFSET)
                .cast::<u8>()
                .write_bytes(0, size_of::<$b>());
        }
    }};
}

pub(crate) use engine_in_place;

/// The `repr(u8)` layout of an enum variant with one field (RFC 2195): the tag, then the field.
#[repr(C)]
struct Tagged<T> {
    tag: u8,
    payload: T,
}

/// Writes the tag `tag` of the variant of the `repr(u8)` enum `S` whose field is a `T`, and
/// returns a pointer to the field, which the caller then initializes in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `S`; `S` is a `repr(u8)` enum, `tag` the discriminant of
/// the variant, and `T` the type of that variant's only field.
#[expect(unsafe_code, reason = "initializes an enum variant in place (GEN-071)")]
pub(crate) unsafe fn variant_payload<S, T>(state: *mut S, tag: u8) -> *mut T {
    const { assert!(size_of::<Tagged<T>>() <= size_of::<S>()) };
    let tagged = state.cast::<Tagged<T>>();
    // SAFETY: by the contract, `S` has the layout of a union of `repr(C)` structs of tag and
    // field, so `Tagged<T>` is the layout of this variant and in bounds. The writes are through
    // raw field pointers.
    unsafe {
        (&raw mut (*tagged).tag).write(tag);
        &raw mut (*tagged).payload
    }
}

/// The C form of a range of `out`.
pub(crate) const fn range(r: OutRange) -> aldis_range_t {
    aldis_range_t {
        offset: r.start(),
        len: r.len(),
    }
}
