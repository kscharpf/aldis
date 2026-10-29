//! Status codes and error detail (c-abi.md §4; CABI-020, CABI-021, GEN-040, GEN-041).
//!
//! Every fallible function returns a `uint32_t`: [`ALDIS_OK`] (0), or the code of the error
//! catalogue (03-api-model.md §9), one `ALDIS_ERR_*` constant per kind, declared by
//! [`aldis_status`]. A code is `ErrorKind::code()` and is never renumbered; there is no second
//! list to keep in step. An optional [`aldis_error_detail_t`] receives the kind, the field code,
//! and the detail of the error.

use aldis::core::{Error, ErrorDetail};

use crate::mirror::mirror;

/// The status a fallible function returns (03-api-model.md §9; CABI-020, GEN-040).
///
/// [`ALDIS_OK`], or the catalogue code of the error. Every `ALDIS_ERR_*` has the value of
/// `ErrorKind::code()` of the kind it is named for, and a test checks each one against
/// `ErrorKind::ALL` (OI-57 convention 3).
///
/// A function returns a `uint32_t`; the type itself is not a field or return type of the header
/// (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_status {
    /// Success (CABI-020).
    ALDIS_OK = 0,
    /// Output buffer too small; the detail carries `needed` (03-api-model.md §9 code 1; CABI-020).
    ALDIS_ERR_BUFFER_TOO_SMALL = 1,
    /// Input shorter than required; the detail carries `needed` (code 2; CABI-020).
    ALDIS_ERR_INCOMPLETE = 2,
    /// Length field, configured length, and buffer length disagree (code 3; CABI-020).
    ALDIS_ERR_LENGTH_MISMATCH = 3,
    /// Version number field is not the one this codec handles (code 4; CABI-020).
    ALDIS_ERR_UNSUPPORTED_VERSION = 4,
    /// A value exceeds its field range or a configured limit (code 5; CABI-020).
    ALDIS_ERR_OUT_OF_RANGE = 5,
    /// A field holds a value the Blue Book forbids, or an argument violates CABI-022 (code 6;
    /// CABI-020, CABI-022).
    ALDIS_ERR_INVALID_FIELD = 6,
    /// A field holds a value CCSDS reserves, in strict mode (code 7; CABI-020).
    ALDIS_ERR_RESERVED_VALUE = 7,
    /// FECF check failed (code 8; CABI-020).
    ALDIS_ERR_CRC_MISMATCH = 8,
    /// AOS Frame Header Error Control detected an uncorrectable error (code 9; CABI-020).
    ALDIS_ERR_HEADER_UNCORRECTABLE = 9,
    /// SCID, VCID, MAP ID, APID, or EPI not in the configuration (code 10; CABI-020).
    ALDIS_ERR_UNKNOWN_CHANNEL = 10,
    /// Configuration violates a Blue Book constraint (code 11; CABI-020).
    ALDIS_ERR_INVALID_CONFIG = 11,
    /// Engine queue full; input rejected without state change (code 12; CABI-020).
    ALDIS_ERR_WOULD_BLOCK = 12,
    /// Operation not permitted in the engine's current state (code 13; CABI-020).
    ALDIS_ERR_INVALID_STATE = 13,
    /// Security hook reported failure (code 14; CABI-020).
    ALDIS_ERR_SECURITY = 14,
    /// Configuration exceeds a compile-time capacity (code 15; CABI-020).
    ALDIS_ERR_CAPACITY_EXCEEDED = 15,
    /// Unknown, uninitialized, or stale handle (code 16; CABI-020, CABI-032).
    ALDIS_ERR_INVALID_HANDLE = 16,
    /// Structurally impossible input (code 17; CABI-020).
    ALDIS_ERR_MALFORMED = 17,
    /// An internal invariant or consistency check failed (code 18; CABI-020).
    ALDIS_ERR_INTERNAL_ERROR = 18,
}
mirror!(pub aldis_status: u32 {
    ALDIS_OK,
    ALDIS_ERR_BUFFER_TOO_SMALL,
    ALDIS_ERR_INCOMPLETE,
    ALDIS_ERR_LENGTH_MISMATCH,
    ALDIS_ERR_UNSUPPORTED_VERSION,
    ALDIS_ERR_OUT_OF_RANGE,
    ALDIS_ERR_INVALID_FIELD,
    ALDIS_ERR_RESERVED_VALUE,
    ALDIS_ERR_CRC_MISMATCH,
    ALDIS_ERR_HEADER_UNCORRECTABLE,
    ALDIS_ERR_UNKNOWN_CHANNEL,
    ALDIS_ERR_INVALID_CONFIG,
    ALDIS_ERR_WOULD_BLOCK,
    ALDIS_ERR_INVALID_STATE,
    ALDIS_ERR_SECURITY,
    ALDIS_ERR_CAPACITY_EXCEEDED,
    ALDIS_ERR_INVALID_HANDLE,
    ALDIS_ERR_MALFORMED,
    ALDIS_ERR_INTERNAL_ERROR,
});

/// [`aldis_error_detail_t::detail`]: the error carries no further detail (GEN-041, CABI-021).
pub const ALDIS_DETAIL_NONE: u16 = 0;
/// [`aldis_error_detail_t::detail`]: `needed` holds the octets required (GEN-041, CABI-021).
pub const ALDIS_DETAIL_NEEDED: u16 = 1;
/// [`aldis_error_detail_t::detail`]: `expected` and `found` hold the limit and the value
/// (GEN-041, CABI-021).
pub const ALDIS_DETAIL_EXPECTED: u16 = 2;

/// The smallest `struct_size` of an [`aldis_error_detail_t`]: `struct_size`, `kind`, `field`,
/// and `detail`, 12 octets (CABI-021, CABI-033).
pub const ALDIS_ERROR_DETAIL_MIN_SIZE: u32 = 12;

/// Detail of a failed call, written only when the call fails (CABI-021, GEN-041).
///
/// Every function takes an optional `aldis_error_detail_t *` as its last parameter; `NULL`
/// is allowed. The caller sets `struct_size`, at least `ALDIS_ERROR_DETAIL_MIN_SIZE`; the
/// library writes only the members that lie wholly inside it, never `struct_size` itself, so
/// a later ABI version can append members (CABI-033). A smaller `struct_size`, or a
/// misaligned pointer, fails the call with `ALDIS_ERR_INVALID_FIELD` before it does anything,
/// and the detail is not written.
///
/// A violation of CABI-022 (a `NULL` or misaligned pointer, an inconsistent length,
/// overlapping buffers, a `struct_size` too small, or an enumeration value with no constant)
/// is `ALDIS_ERR_INVALID_FIELD` with field 0 and no detail.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_error_detail_t {
    /// `sizeof(aldis_error_detail_t)`, set by the caller before the call (CABI-033).
    pub struct_size: u32,
    /// The error kind: its catalogue code, the returned status (03 §9).
    pub kind: u32,
    /// The field code of the field catalogue (03 §9.1, API-082), 0 for none.
    pub field: u16,
    /// `ALDIS_DETAIL_NONE`, `ALDIS_DETAIL_NEEDED`, or `ALDIS_DETAIL_EXPECTED`.
    pub detail: u16,
    /// Reserved, written as 0.
    pub reserved: u32,
    /// Octets (or bits, where documented) required, for `ALDIS_DETAIL_NEEDED`.
    pub needed: u64,
    /// The value or limit the standard or configuration requires, for `ALDIS_DETAIL_EXPECTED`.
    pub expected: u64,
    /// The value found, for `ALDIS_DETAIL_EXPECTED`.
    pub found: u64,
}

/// `sizeof(aldis_error_detail_t)`.
const DETAIL_SIZE: u32 = {
    let n = size_of::<aldis_error_detail_t>();
    assert!(n <= 0xFFFF, "the detail fits its struct_size");
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "checked above to fit"
    )]
    let size = n as u32;
    size
};

impl Default for aldis_error_detail_t {
    /// An empty detail whose `struct_size` is the size of this struct.
    fn default() -> aldis_error_detail_t {
        aldis_error_detail_t {
            struct_size: DETAIL_SIZE,
            kind: 0,
            field: 0,
            detail: ALDIS_DETAIL_NONE,
            reserved: 0,
            needed: 0,
            expected: 0,
            found: 0,
        }
    }
}

impl From<Error> for aldis_error_detail_t {
    fn from(e: Error) -> aldis_error_detail_t {
        let base = aldis_error_detail_t {
            kind: u32::from(e.kind().code()),
            field: e.field().map_or(0, |f| f.code().get()),
            ..aldis_error_detail_t::default()
        };
        match e.detail() {
            ErrorDetail::Needed(n) => aldis_error_detail_t {
                detail: ALDIS_DETAIL_NEEDED,
                needed: u64::try_from(n).unwrap_or(u64::MAX),
                ..base
            },
            ErrorDetail::Expected { expected, found } => aldis_error_detail_t {
                detail: ALDIS_DETAIL_EXPECTED,
                expected,
                found,
                ..base
            },
            // `ErrorDetail::None`, and any detail a later version adds (GEN-081).
            _ => base,
        }
    }
}

/// The status a failed call returns: the catalogue code (CABI-020).
pub(crate) fn status(e: Error) -> u32 {
    u32::from(e.kind().code())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::mirror::Mirror;
    use aldis::core::{ErrorKind, field};

    #[test]
    fn status_codes_are_the_catalogue() {
        // req: CABI-020, GEN-040
        assert_eq!(ALDIS_OK, 0);
        assert_eq!(aldis_status::ALL[0], aldis_status::ALDIS_OK);
        let errors = &aldis_status::ALL[1..];
        assert_eq!(errors.len(), ErrorKind::ALL.len());
        for (kind, constant) in ErrorKind::ALL.into_iter().zip(errors) {
            assert_eq!(status(Error::new(kind)), constant.code(), "{kind:?}");
            assert_eq!(constant.code(), u32::from(kind.code()));
            // The constant is named for the kind: `ALDIS_ERR_` and the kind in upper snake case.
            let name = kind.name();
            let mut snake = String::from("ALDIS_ERR_");
            for (i, c) in name.chars().enumerate() {
                if c.is_ascii_uppercase() && i > 0 {
                    snake.push('_');
                }
                snake.push(c.to_ascii_uppercase());
            }
            assert_eq!(format!("{constant:?}"), snake, "{kind:?}");
        }
        assert_eq!(ALDIS_ERR_BUFFER_TOO_SMALL, 1);
    }

    #[test]
    fn detail_carries_kind_field_and_values() {
        // req: CABI-021, GEN-041, API-082
        let d = aldis_error_detail_t::from(Error::out_of_range(field::APID, 2047, 4096));
        assert_eq!(
            d,
            aldis_error_detail_t {
                struct_size: 40,
                reserved: 0,
                kind: 5,
                field: 5,
                detail: ALDIS_DETAIL_EXPECTED,
                needed: 0,
                expected: 2047,
                found: 4096,
            }
        );
        let d = aldis_error_detail_t::from(Error::incomplete(6));
        assert_eq!(
            (d.kind, d.field, d.detail, d.needed),
            (2, 0, ALDIS_DETAIL_NEEDED, 6)
        );
        let d = aldis_error_detail_t::from(Error::new(ErrorKind::Malformed));
        assert_eq!((d.kind, d.field, d.detail), (17, 0, ALDIS_DETAIL_NONE));
    }

    #[test]
    fn the_detail_layout_is_fixed() {
        // req: CABI-021, CABI-033
        use core::mem::offset_of;
        assert_eq!(size_of::<aldis_error_detail_t>(), 40);
        assert_eq!(offset_of!(aldis_error_detail_t, struct_size), 0);
        let min = usize::try_from(ALDIS_ERROR_DETAIL_MIN_SIZE).unwrap();
        assert_eq!(
            offset_of!(aldis_error_detail_t, detail).checked_add(2),
            Some(min)
        );
        assert_eq!(offset_of!(aldis_error_detail_t, needed), 16);
        assert_eq!(aldis_error_detail_t::default().struct_size, 40);
    }
}
