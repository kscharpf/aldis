//! Helpers shared by the link engines of the C ABI: building an enum variant in
//! place (CABI-040, #240) and the C form of an output range (API-061).

use aldis::core::engine::OutRange;

use crate::range::aldis_range_t;

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
