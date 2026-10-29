//! Checks at the C boundary: every pointer and length a function receives (CABI-022), and the
//! status and detail it returns (CABI-020, CABI-021).
//!
//! Every access to caller memory goes through this module, except the handle storage of
//! [`crate::handle`]. A pointer is rejected when it is `NULL` (unless its length is 0),
//! misaligned for its type, or when its range wraps the address space; an output buffer is
//! rejected when it overlaps an input buffer of the same call, so that no shared and mutable
//! Rust references alias (GEN-071). A configuration struct is rejected when its `struct_size`
//! is smaller than the struct this library knows (CABI-033). Each rejection is
//! `ALDIS_ERR_INVALID_FIELD` with field 0 and no detail. What cannot be checked, that a
//! non-`NULL` pointer is valid for the access, is the caller's part of the contract, stated in
//! every function's `# Safety` section.

use core::marker::PhantomData;
use core::ptr::NonNull;

use aldis::core::{DecodeMode, Error, ErrorKind, Result};

use crate::mirror::{mirror, parse};

use crate::error::{
    ALDIS_ERR_INVALID_FIELD, ALDIS_ERROR_DETAIL_MIN_SIZE, ALDIS_OK, aldis_error_detail_t, status,
};

/// The decode mode argument (API-030; OI-57 convention 2). The argument is a `uint32_t`: an
/// enumerator of this type, which no field of the header uses (CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_mode {
    /// Strict decoding, the default (API-030, API-031).
    ALDIS_MODE_STRICT = 0,
    /// Lenient decoding, with anomaly flags (API-030, API-032).
    ALDIS_MODE_LENIENT = 1,
}
mirror!(pub aldis_mode: u32 { ALDIS_MODE_STRICT, ALDIS_MODE_LENIENT });

/// The error of a CABI-022 violation: `InvalidField` naming no field.
pub(crate) const fn invalid_argument() -> Error {
    Error::new(ErrorKind::InvalidField)
}

/// The decode mode of a `uint32_t` mode argument (API-030; CABI-010, CABI-022).
pub(crate) fn mode(mode: u32) -> Result<DecodeMode> {
    match parse::<aldis_mode>(mode)? {
        aldis_mode::ALDIS_MODE_STRICT => Ok(DecodeMode::Strict),
        aldis_mode::ALDIS_MODE_LENIENT => Ok(DecodeMode::Lenient),
    }
}

/// A `bool` from a `uint8_t` flag: 0 or 1 (CABI-010, CABI-022).
pub(crate) const fn flag(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid_argument()),
    }
}

/// A checked location for one `T`, written once (CABI-022).
#[derive(Debug)]
pub(crate) struct Out<'a, T> {
    ptr: NonNull<T>,
    life: PhantomData<&'a mut T>,
}

impl<T> Out<'_, T> {
    /// Writes `value`.
    #[expect(
        unsafe_code,
        reason = "write through a checked pointer (CABI-022, GEN-071)"
    )]
    pub(crate) const fn write(self, value: T) {
        // SAFETY: `out` checked that `ptr` is non-null and aligned for `T`; its caller
        // guaranteed that it is valid for a write of `T`.
        unsafe { self.ptr.as_ptr().write(value) }
    }
}

/// `ptr` as an output location (CABI-022).
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` or misaligned.
///
/// # Safety
/// A non-null, aligned `ptr` must be valid for one write of `T` for `'a`.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn out<'a, T>(ptr: *mut T) -> Result<Out<'a, T>> {
    match NonNull::new(ptr) {
        Some(ptr) if ptr.as_ptr().is_aligned() => Ok(Out {
            ptr,
            life: PhantomData,
        }),
        _ => Err(invalid_argument()),
    }
}

/// The `T` that `ptr` points to, read by value (CABI-022).
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` or misaligned.
///
/// # Safety
/// A non-null, aligned `ptr` must be valid for a read of an initialized `T`.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn read<T: Copy>(ptr: *const T) -> Result<T> {
    if ptr.is_null() || !ptr.is_aligned() {
        return Err(invalid_argument());
    }
    // SAFETY: non-null and aligned, checked above; valid for a read by the caller's contract.
    Ok(unsafe { ptr.read() })
}

/// `ptr` as an in-out location, disjoint from `inputs` (CABI-022).
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` or misaligned, the range wraps, or it
/// overlaps one of `inputs`.
///
/// # Safety
/// A non-null, aligned `ptr` must be valid for reads and writes of `T` during `'a`.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn inout<'a, T: Copy>(
    ptr: *mut T,
    inputs: &[(*const u8, usize)],
) -> Result<&'a mut T> {
    if ptr.is_null() || !ptr.is_aligned() {
        return Err(invalid_argument());
    }
    let size = core::mem::size_of::<T>();
    let Some((start, end)) = range(ptr.cast::<u8>(), size) else {
        return Err(invalid_argument());
    };
    let overlaps = inputs
        .iter()
        .any(|&(p, len)| len != 0 && range(p, len).is_none_or(|(s, e)| s < end && start < e));
    if overlaps {
        return Err(invalid_argument());
    }
    // SAFETY: non-null, aligned, and disjoint from inputs, checked above; valid for reads and writes by caller contract.
    Ok(unsafe { &mut *ptr })
}

/// `ptr` as an output location disjoint from `inputs` (CABI-022).
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` or misaligned, the range wraps, or it
/// overlaps one of `inputs`.
///
/// # Safety
/// A non-null, aligned `ptr` must be valid for one write of `T` for `'a`.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn out_disjoint<'a, T>(
    ptr: *mut T,
    inputs: &[(*const u8, usize)],
) -> Result<Out<'a, T>> {
    // SAFETY: caller guaranteed ptr is valid for one write of T.
    let out = unsafe { out(ptr) }?;
    let size = core::mem::size_of::<T>();
    let Some((start, end)) = range(ptr.cast::<u8>(), size) else {
        return Err(invalid_argument());
    };
    let overlaps = inputs
        .iter()
        .any(|&(p, len)| len != 0 && range(p, len).is_none_or(|(s, e)| s < end && start < e));
    if overlaps {
        return Err(invalid_argument());
    }
    Ok(out)
}

/// The configuration struct that `ptr` points to, whose first field is `uint32_t struct_size`
/// (CABI-033, CABI-022).
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` or misaligned, or `struct_size` is smaller
/// than `T`.
///
/// # Safety
/// A non-null, aligned `ptr` must be valid for reads of `struct_size` initialized octets, of
/// which the first four hold `struct_size`. `T` must be `#[repr(C)]` with a `u32` first field.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn config<T: Copy>(ptr: *const T) -> Result<T> {
    if ptr.is_null() || !ptr.is_aligned() {
        return Err(invalid_argument());
    }
    let size = ptr.cast::<u32>();
    if !size.is_aligned() {
        return Err(invalid_argument());
    }
    // SAFETY: non-null and aligned for `u32`, checked above; the first four octets of the
    // struct are its `struct_size` by the caller's contract.
    let struct_size = unsafe { size.read() };
    if usize::try_from(struct_size).map_or(true, |n| n < size_of::<T>()) {
        return Err(invalid_argument());
    }
    // SAFETY: aligned for `T`, and valid for `struct_size` ≥ `size_of::<T>()` octets.
    Ok(unsafe { ptr.read() })
}

/// An array of configuration structs in caller memory, each starting with `uint32_t
/// struct_size` (CABI-033, CABI-022; M2 plan D14): `len` structs, `stride` octets apart.
///
/// The stride is the `struct_size` of the first struct, which every struct repeats, so that a
/// caller built against a later header, with a longer struct, still passes an array this
/// library can step through; the library reads the struct it knows from each element.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Configs<'a, T> {
    ptr: *const T,
    stride: usize,
    len: usize,
    life: PhantomData<&'a T>,
}

impl<T: Copy> Configs<'_, T> {
    /// The number of structs.
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// The struct at index `i`, read by value.
    ///
    /// # Errors
    /// `InvalidField` (no field) when `i` is not below [`len`](Self::len), or the struct's
    /// `struct_size` is not the array's stride.
    #[expect(
        unsafe_code,
        reason = "reads caller memory that `configs` checked (CABI-022, GEN-071)"
    )]
    pub(crate) fn get(&self, i: usize) -> Result<T> {
        if i >= self.len {
            return Err(invalid_argument());
        }
        let offset = i.checked_mul(self.stride).ok_or_else(invalid_argument)?;
        // SAFETY: `configs` checked that the `len × stride` octets at `ptr` form one range that
        // neither wraps nor exceeds `isize::MAX`, and its caller made them valid for reads; with
        // `i < len`, the struct at `offset` lies inside that range.
        let element = unsafe { self.ptr.cast::<u8>().add(offset) }.cast::<T>();
        // `ptr` is aligned for `T` and for `u32`, and `stride` a multiple of both alignments, so
        // `element` is too.
        let size = element.cast::<u32>();
        // SAFETY: inside the checked range and aligned for `u32`; the first four octets of
        // every struct are its `struct_size` by the caller's contract.
        let struct_size = unsafe { size.read() };
        if usize::try_from(struct_size) != Ok(self.stride) {
            return Err(invalid_argument());
        }
        // SAFETY: aligned for `T`, and `stride` ≥ `size_of::<T>()` octets of it lie inside the
        // checked range and hold an initialized `T` by the caller's contract.
        Ok(unsafe { element.read() })
    }
}

/// The `len` configuration structs at `ptr` as an array (CABI-033, CABI-022; M2 plan D14).
/// `NULL` is allowed for an empty array.
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` and `len` is not 0, `ptr` is misaligned, the
/// first `struct_size` (the stride) is smaller than `T` or not a multiple of its alignment, or
/// the `len × stride` octets wrap or exceed `isize::MAX`.
///
/// # Safety
/// For `len` > 0, `ptr` must be valid for reads of `len × struct_size` initialized octets, of
/// which each struct's first four hold its `struct_size`, not written during `'a`. `T` must be
/// `#[repr(C)]` with a `u32` first field.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn configs<'a, T: Copy>(ptr: *const T, len: usize) -> Result<Configs<'a, T>> {
    let empty = Configs {
        ptr,
        stride: 0,
        len: 0,
        life: PhantomData,
    };
    if len == 0 {
        return Ok(empty);
    }
    if ptr.is_null() || !ptr.is_aligned() {
        return Err(invalid_argument());
    }
    let size = ptr.cast::<u32>();
    if !size.is_aligned() {
        return Err(invalid_argument());
    }
    // SAFETY: non-null and aligned for `u32`, checked above; the first four octets of the
    // first struct are its `struct_size` by the caller's contract.
    let stride = usize::try_from(unsafe { size.read() }).map_err(|_| invalid_argument())?;
    // A multiple of both alignments keeps every element aligned for `T` and for the `u32`
    // `struct_size` that `Configs::get` reads first, whatever `T` is.
    if stride < size_of::<T>()
        || !stride.is_multiple_of(align_of::<T>())
        || !stride.is_multiple_of(align_of::<u32>())
    {
        return Err(invalid_argument());
    }
    let total = stride.checked_mul(len).ok_or_else(invalid_argument)?;
    if range(ptr.cast::<u8>(), total).is_none() || isize::try_from(total).is_err() {
        return Err(invalid_argument());
    }
    Ok(Configs {
        stride,
        len,
        ..empty
    })
}

/// The address range `[start, end)` of `len` octets at `ptr`, if it does not wrap.
fn range(ptr: *const u8, len: usize) -> Option<(usize, usize)> {
    let start = ptr.addr();
    start.checked_add(len).map(|end| (start, end))
}

/// `len` octets at `ptr` as an input buffer: `(const uint8_t *ptr, size_t len)` (CABI-011,
/// CABI-022). `NULL` is allowed for an empty buffer.
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` and `len` is not 0, or the range wraps or
/// exceeds `isize::MAX`.
///
/// # Safety
/// For `len` > 0, `ptr` must be valid for reads of `len` initialized octets, not written
/// during `'a`.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8]> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() || range(ptr, len).is_none() || isize::try_from(len).is_err() {
        return Err(invalid_argument());
    }
    // SAFETY: non-null, a range that does not wrap and fits `isize`, checked above; valid for
    // reads of `len` octets by the caller's contract. `u8` has no alignment requirement.
    Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// `cap` octets at `ptr` as an output buffer: `(uint8_t *ptr, size_t cap)` (CABI-011,
/// CABI-022). `NULL` is allowed for an empty buffer. `inputs` are the call's input buffers,
/// which it must not overlap.
///
/// # Errors
/// `InvalidField` (no field) when `ptr` is `NULL` and `cap` is not 0, the range wraps or
/// exceeds `isize::MAX`, or it overlaps one of `inputs`.
///
/// # Safety
/// For `cap` > 0, `ptr` must be valid for reads and writes of `cap` octets, accessed through
/// nothing else during `'a`.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn bytes_mut<'a>(
    ptr: *mut u8,
    cap: usize,
    inputs: &[(*const u8, usize)],
) -> Result<&'a mut [u8]> {
    if cap == 0 {
        return Ok(&mut []);
    }
    let Some((start, end)) = range(ptr, cap) else {
        return Err(invalid_argument());
    };
    if ptr.is_null() || isize::try_from(cap).is_err() {
        return Err(invalid_argument());
    }
    let overlaps = inputs
        .iter()
        .any(|&(p, len)| len != 0 && range(p, len).is_none_or(|(s, e)| s < end && start < e));
    if overlaps {
        return Err(invalid_argument());
    }
    // SAFETY: non-null, a range that does not wrap and fits `isize`, disjoint from every input
    // buffer, checked above; valid and exclusive by the caller's contract. `u8` has no
    // alignment requirement, and any octet value is initialized.
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr, cap) })
}

/// Writes the members of `value` that lie wholly inside the caller's `struct_size` octets at
/// `detail`, except `struct_size` itself (CABI-021, CABI-033).
///
/// # Safety
/// `detail` is aligned and valid for writes of `struct_size` octets.
#[expect(
    unsafe_code,
    reason = "writes caller memory within its struct_size (CABI-021, GEN-071)"
)]
unsafe fn write_detail(
    detail: NonNull<aldis_error_detail_t>,
    struct_size: usize,
    value: &aldis_error_detail_t,
) {
    use core::mem::offset_of;
    let fits = |offset: usize, len: usize| {
        offset
            .checked_add(len)
            .is_some_and(|end| end <= struct_size)
    };
    let d = detail.as_ptr();
    // SAFETY: each member is written only when it lies wholly inside the `struct_size` octets
    // the caller made writable (`fits`); a field projection of the aligned struct is aligned
    // for the field's type, and no member outside `struct_size` is projected.
    unsafe {
        if fits(offset_of!(aldis_error_detail_t, kind), 4) {
            (&raw mut (*d).kind).write(value.kind);
        }
        if fits(offset_of!(aldis_error_detail_t, field), 2) {
            (&raw mut (*d).field).write(value.field);
        }
        if fits(offset_of!(aldis_error_detail_t, detail), 2) {
            (&raw mut (*d).detail).write(value.detail);
        }
        if fits(offset_of!(aldis_error_detail_t, reserved), 4) {
            (&raw mut (*d).reserved).write(value.reserved);
        }
        if fits(offset_of!(aldis_error_detail_t, needed), 8) {
            (&raw mut (*d).needed).write(value.needed);
        }
        if fits(offset_of!(aldis_error_detail_t, expected), 8) {
            (&raw mut (*d).expected).write(value.expected);
        }
        if fits(offset_of!(aldis_error_detail_t, found), 8) {
            (&raw mut (*d).found).write(value.found);
        }
    }
}

/// Runs `f` and reports its result: [`ALDIS_OK`], or the catalogue code with the
/// detail written to `detail` when it is not `NULL` (CABI-020, CABI-021).
///
/// A misaligned `detail`, or one whose `struct_size` is below
/// [`ALDIS_ERROR_DETAIL_MIN_SIZE`], is rejected before `f` runs, with
/// `ALDIS_ERR_INVALID_FIELD`; the detail is then not written.
///
/// # Safety
/// A non-null, aligned `detail` must be valid for a read of its first four octets, the
/// `struct_size` the caller set, and for writes of `struct_size` octets.
#[expect(
    unsafe_code,
    reason = "checked access to caller memory (CABI-022, GEN-071)"
)]
pub(crate) unsafe fn call(
    detail: *mut aldis_error_detail_t,
    f: impl FnOnce() -> Result<()>,
) -> u32 {
    let target = match NonNull::new(detail) {
        None => None,
        Some(d) => {
            if !d.as_ptr().is_aligned() {
                return ALDIS_ERR_INVALID_FIELD;
            }
            // SAFETY: `d` is NonNull and aligned; the caller set `struct_size`.
            let struct_size = unsafe { d.cast::<u32>().as_ptr().read() };
            if struct_size < ALDIS_ERROR_DETAIL_MIN_SIZE {
                return ALDIS_ERR_INVALID_FIELD;
            }
            Some((d, usize::try_from(struct_size).unwrap_or(usize::MAX)))
        }
    };
    match f() {
        Ok(()) => ALDIS_OK,
        Err(e) => {
            if let Some((d, struct_size)) = target {
                // SAFETY: `d` is NonNull and aligned, checked above; valid for writes of
                // `struct_size` octets by the caller's contract.
                unsafe { write_detail(d, struct_size, &aldis_error_detail_t::from(e)) };
            }
            status(e)
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    unsafe_code,
    reason = "the tests call the unsafe checks with valid and invalid pointers"
)]
mod tests {
    use super::*;
    use crate::error::{ALDIS_DETAIL_NONE, ALDIS_ERR_INCOMPLETE};

    #[test]
    fn buffers_are_checked() {
        // req: CABI-022, CABI-011
        let data = [1_u8, 2, 3];
        // SAFETY: `data` is valid for 3 octets; the other pointers are rejected or unused.
        unsafe {
            assert_eq!(bytes(data.as_ptr(), 3), Ok(&data[..]));
            assert_eq!(bytes(core::ptr::null(), 0), Ok(&[][..]));
            assert_eq!(bytes(core::ptr::null(), 1), Err(invalid_argument()));
            let wrap = core::ptr::without_provenance::<u8>(usize::MAX);
            assert_eq!(bytes(wrap, 2), Err(invalid_argument()));
        }
    }

    #[test]
    fn output_buffers_must_not_overlap_inputs() {
        // req: CABI-022
        let mut buf = [0_u8; 8];
        let base = buf.as_mut_ptr();
        // SAFETY: every range lies in `buf`; overlapping ones are rejected before any slice
        // is made.
        unsafe {
            let inside = base.add(4).cast_const();
            assert!(bytes_mut(base, 4, &[(inside, 4)]).is_ok());
            assert_eq!(
                bytes_mut(base, 5, &[(inside, 4)]).map(|b| b.len()),
                Err(invalid_argument())
            );
            assert_eq!(
                bytes_mut(base.add(4), 4, &[(base.cast_const(), 5)]).map(|b| b.len()),
                Err(invalid_argument())
            );
            assert!(bytes_mut(base, 8, &[(core::ptr::null(), 0)]).is_ok());
            assert_eq!(
                bytes_mut(core::ptr::null_mut(), 0, &[]).map(|b| b.len()),
                Ok(0)
            );
            assert_eq!(
                bytes_mut(core::ptr::null_mut(), 1, &[]).map(|b| b.len()),
                Err(invalid_argument())
            );
            let wrap = core::ptr::without_provenance_mut::<u8>(usize::MAX);
            assert_eq!(
                bytes_mut(wrap, 2, &[]).map(|b| b.len()),
                Err(invalid_argument())
            );
            let wrapped_input = core::ptr::without_provenance::<u8>(usize::MAX);
            assert_eq!(
                bytes_mut(base, 1, &[(wrapped_input, 2)]).map(|b| b.len()),
                Err(invalid_argument())
            );
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Cfg {
        struct_size: u32,
        value: u32,
    }

    #[test]
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "a misaligned pointer, rejected before any access"
    )]
    fn configurations_need_their_struct_size() {
        // req: CABI-033, CABI-022
        let ok = Cfg {
            struct_size: 8,
            value: 7,
        };
        let small = Cfg {
            struct_size: 4,
            value: 7,
        };
        // SAFETY: each pointer is NULL or points to a live `Cfg`.
        unsafe {
            assert_eq!(config(&raw const ok), Ok(ok));
            assert_eq!(config(&raw const small), Err(invalid_argument()));
            assert_eq!(config::<Cfg>(core::ptr::null()), Err(invalid_argument()));
            assert_eq!(read(&raw const ok), Ok(ok));
            assert_eq!(read::<Cfg>(core::ptr::null()), Err(invalid_argument()));
        }
        let words = [0_u32; 4];
        let misaligned = words.as_ptr().cast::<u8>().wrapping_add(1).cast::<Cfg>();
        // SAFETY: misaligned pointers are rejected before any access.
        unsafe {
            assert_eq!(config(misaligned), Err(invalid_argument()));
            assert_eq!(read(misaligned), Err(invalid_argument()));
            assert!(out(misaligned.cast_mut()).is_err());
            assert!(out::<Cfg>(core::ptr::null_mut()).is_err());
        }
    }

    #[test]
    fn configuration_arrays_step_by_their_struct_size() {
        // req: CABI-033, CABI-022
        let cfgs = [
            Cfg {
                struct_size: 8,
                value: 1,
            },
            Cfg {
                struct_size: 8,
                value: 2,
            },
        ];
        // SAFETY: each pointer is NULL with length 0 or points to `len` live `Cfg`s.
        unsafe {
            let array = configs(cfgs.as_ptr(), 2).unwrap();
            assert_eq!(array.len(), 2);
            assert_eq!(array.get(1), Ok(cfgs[1]));
            assert_eq!(array.get(2), Err(invalid_argument()));
            assert_eq!(configs::<Cfg>(core::ptr::null(), 0).map(|a| a.len()), Ok(0));
            assert_eq!(
                configs::<Cfg>(core::ptr::null(), 1).map(|a| a.len()),
                Err(invalid_argument())
            );
            assert_eq!(
                configs(&raw const cfgs[0], usize::MAX).map(|a| a.len()),
                Err(invalid_argument())
            );
        }
    }

    #[test]
    fn modes_and_flags_take_only_their_constants() {
        // req: API-030, CABI-010, CABI-022
        assert_eq!(mode(ALDIS_MODE_STRICT), Ok(DecodeMode::Strict));
        assert_eq!(mode(ALDIS_MODE_LENIENT), Ok(DecodeMode::Lenient));
        assert_eq!(mode(2), Err(invalid_argument()));
        assert_eq!((flag(0), flag(1)), (Ok(false), Ok(true)));
        assert_eq!(flag(2), Err(invalid_argument()));
    }

    #[test]
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "a misaligned pointer, rejected before any access"
    )]
    fn call_reports_status_and_detail() {
        // req: CABI-020, CABI-021
        let mut d = aldis_error_detail_t::default();
        // SAFETY: `d` is a live, aligned detail; NULL is allowed.
        unsafe {
            assert_eq!(call(&raw mut d, || Ok(())), ALDIS_OK);
            assert_eq!(d, aldis_error_detail_t::default());
            assert_eq!(
                call(&raw mut d, || Err(Error::incomplete(4))),
                ALDIS_ERR_INCOMPLETE
            );
            assert_eq!((d.kind, d.needed), (2, 4));
            assert_eq!(
                call(core::ptr::null_mut(), || Err(invalid_argument())),
                ALDIS_ERR_INVALID_FIELD
            );
            assert_eq!(call(&raw mut d, || Err(invalid_argument())), 6);
            assert_eq!((d.kind, d.field, d.detail), (6, 0, ALDIS_DETAIL_NONE));
            let words = [0_u64; 8];
            let misaligned = words
                .as_ptr()
                .cast::<u8>()
                .wrapping_add(1)
                .cast::<aldis_error_detail_t>()
                .cast_mut();
            let mut ran = false;
            assert_eq!(
                call(misaligned, || {
                    ran = true;
                    Ok(())
                }),
                ALDIS_ERR_INVALID_FIELD
            );
            assert!(!ran);
        }
    }
}
