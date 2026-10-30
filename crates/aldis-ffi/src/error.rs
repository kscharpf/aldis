//! Status codes and error detail (c-abi.md §4; CABI-020, CABI-021, GEN-040, GEN-041).
//!
//! Every fallible function returns a `uint32_t`: [`ALDIS_OK`] (0), or the code of the error
//! catalogue (03-api-model.md §9), one `ALDIS_ERR_*` constant per kind, declared by
//! [`aldis_status`], which `cargo xtask status --write` generates from that table (the module
//! `status`, re-exported here). A code is `ErrorKind::code()` and is never renumbered; the build
//! checks each constant against its `ErrorKind`. An optional [`aldis_error_detail_t`] receives the kind, the field code,
//! and the detail of the error.

use aldis::core::{Error, ErrorDetail};

pub use crate::status::*;

/// [`aldis_error_detail_t::detail`]: the error carries no further detail (GEN-041, CABI-021).
pub const ALDIS_DETAIL_NONE: u16 = 0;
/// [`aldis_error_detail_t::detail`]: `needed` holds the octets required (GEN-041, CABI-021).
pub const ALDIS_DETAIL_NEEDED: u16 = 1;
/// [`aldis_error_detail_t::detail`]: `expected` and `found` hold the limit and the value
/// (GEN-041, CABI-021).
pub const ALDIS_DETAIL_EXPECTED: u16 = 2;

/// Detail of a failed call, written only when the call fails (CABI-021, GEN-041).
///
/// Every function takes an optional `aldis_error_detail_t *` as its last parameter; `NULL`
/// is allowed. The library writes the whole struct when the call fails. A misaligned pointer
/// fails the call with `ALDIS_ERR_INVALID_FIELD` before it does anything, and the detail is not
/// written.
///
/// A violation of CABI-022 (a `NULL` or misaligned pointer, an inconsistent length,
/// overlapping buffers, or an enumeration value with no constant) is
/// `ALDIS_ERR_INVALID_FIELD` with field 0 and no detail.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_error_detail_t {
    /// The error kind: its catalogue code, the returned status (03 §9).
    pub kind: u32,
    /// The field code of the field catalogue (03 §9.1, API-082), 0 for none.
    pub field: u16,
    /// `ALDIS_DETAIL_NONE`, `ALDIS_DETAIL_NEEDED`, or `ALDIS_DETAIL_EXPECTED`.
    pub detail: u16,
    /// Octets (or bits, where documented) required, for `ALDIS_DETAIL_NEEDED`.
    pub needed: u64,
    /// The value or limit the standard or configuration requires, for `ALDIS_DETAIL_EXPECTED`.
    pub expected: u64,
    /// The value found, for `ALDIS_DETAIL_EXPECTED`.
    pub found: u64,
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
        // req: CABI-021
        use core::mem::offset_of;
        assert_eq!(size_of::<aldis_error_detail_t>(), 32);
        assert_eq!(offset_of!(aldis_error_detail_t, kind), 0);
        assert_eq!(offset_of!(aldis_error_detail_t, field), 4);
        assert_eq!(offset_of!(aldis_error_detail_t, detail), 6);
        assert_eq!(offset_of!(aldis_error_detail_t, needed), 8);
        assert_eq!(aldis_error_detail_t::default().detail, ALDIS_DETAIL_NONE);
    }
}
