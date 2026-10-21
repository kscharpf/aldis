//! Status codes and error detail (c-abi.md §4; CABI-020, CABI-021, GEN-040, GEN-041).
//!
//! Every fallible function returns `int32_t`: [`ALDIS_OK`], or the negated code of the error
//! catalogue (03-api-model.md §9), one `ALDIS_ERR_*` constant per kind. The constants are
//! negative, as CABI-020 gives them (`ALDIS_ERR_BUFFER_TOO_SMALL` is −1). An optional
//! [`aldis_error_detail_t`] receives the kind, the field code, and the detail of the error.

use aldis::core::{Error, ErrorDetail};

/// Success (CABI-020).
pub const ALDIS_OK: i32 = 0;
/// Output buffer too small; the detail carries `needed` (03-api-model.md §9 code 1; CABI-020).
pub const ALDIS_ERR_BUFFER_TOO_SMALL: i32 = -1;
/// Input shorter than required; the detail carries `needed` (code 2; CABI-020).
pub const ALDIS_ERR_INCOMPLETE: i32 = -2;
/// Length field, configured length, and buffer length disagree (code 3; CABI-020).
pub const ALDIS_ERR_LENGTH_MISMATCH: i32 = -3;
/// Version number field is not the one this codec handles (code 4; CABI-020).
pub const ALDIS_ERR_UNSUPPORTED_VERSION: i32 = -4;
/// A value exceeds its field range or a configured limit (code 5; CABI-020).
pub const ALDIS_ERR_OUT_OF_RANGE: i32 = -5;
/// A field holds a value the Blue Book forbids, or an argument violates CABI-022 (code 6;
/// CABI-020, CABI-022).
pub const ALDIS_ERR_INVALID_FIELD: i32 = -6;
/// A field holds a value CCSDS reserves, in strict mode (code 7; CABI-020).
pub const ALDIS_ERR_RESERVED_VALUE: i32 = -7;
/// FECF check failed (code 8; CABI-020).
pub const ALDIS_ERR_CRC_MISMATCH: i32 = -8;
/// AOS Frame Header Error Control detected an uncorrectable error (code 9; CABI-020).
pub const ALDIS_ERR_HEADER_UNCORRECTABLE: i32 = -9;
/// SCID, VCID, MAP ID, APID, or EPI not in the configuration (code 10; CABI-020).
pub const ALDIS_ERR_UNKNOWN_CHANNEL: i32 = -10;
/// Configuration violates a Blue Book constraint (code 11; CABI-020).
pub const ALDIS_ERR_INVALID_CONFIG: i32 = -11;
/// Engine queue full; input rejected without state change (code 12; CABI-020).
pub const ALDIS_ERR_WOULD_BLOCK: i32 = -12;
/// Operation not permitted in the engine's current state (code 13; CABI-020).
pub const ALDIS_ERR_INVALID_STATE: i32 = -13;
/// Security hook reported failure (code 14; CABI-020).
pub const ALDIS_ERR_SECURITY: i32 = -14;
/// Configuration exceeds a compile-time capacity (code 15; CABI-020).
pub const ALDIS_ERR_CAPACITY_EXCEEDED: i32 = -15;
/// Unknown, uninitialized, or stale handle (code 16; CABI-020, CABI-032).
pub const ALDIS_ERR_INVALID_HANDLE: i32 = -16;
/// Structurally impossible input (code 17; CABI-020).
pub const ALDIS_ERR_MALFORMED: i32 = -17;

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
/// is allowed. A violation of CABI-022 (a `NULL` or misaligned pointer, an inconsistent
/// length, overlapping buffers, a `struct_size` too small, or an enumeration value with no
/// constant) is `ALDIS_ERR_INVALID_FIELD` with field 0 and no detail.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_error_detail_t {
    /// The error kind: its catalogue code, the negation of the returned status (03 §9).
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

/// The status a failed call returns: the negated catalogue code (CABI-020).
pub(crate) fn status(e: Error) -> i32 {
    i32::from(e.kind().code()).wrapping_neg()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::{ErrorKind, field};

    #[test]
    fn status_codes_are_the_negated_catalogue() {
        // req: CABI-020, GEN-040
        let constants = [
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
        ];
        assert_eq!(constants.len(), ErrorKind::ALL.len());
        for (kind, constant) in ErrorKind::ALL.into_iter().zip(constants) {
            assert_eq!(status(Error::new(kind)), constant, "{kind:?}");
            assert_eq!(-constant, i32::from(kind.code()));
        }
        assert_eq!(ALDIS_ERR_BUFFER_TOO_SMALL, -1);
        assert_eq!(ALDIS_OK, 0);
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
}
