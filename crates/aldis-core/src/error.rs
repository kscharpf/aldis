//! Error kinds and details shared by all crates.
//!
//! Implements the stable error catalogue of `docs/03-api-model.md` §9 (GEN-040), errors with
//! field and value detail without allocation (GEN-041), and extensible enums (GEN-081, RS-011).

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::num::NonZeroU16;

/// Error kind with a stable numeric code shared by all profiles (03-api-model.md §9).
///
/// The C ABI returns the negated code (CABI-020). Codes are never renumbered.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum ErrorKind {
    /// Output buffer too small (detail: `Needed`).
    BufferTooSmall = 1,
    /// Input shorter than required (detail: `Needed`).
    Incomplete = 2,
    /// Length field, configured length, and buffer length disagree.
    LengthMismatch = 3,
    /// Version number field is not the one this codec handles.
    UnsupportedVersion = 4,
    /// A value exceeds its field range or a configured limit.
    OutOfRange = 5,
    /// A field holds a value the Blue Book forbids.
    InvalidField = 6,
    /// A field holds a value CCSDS reserves (strict mode).
    ReservedValue = 7,
    /// FECF check failed.
    CrcMismatch = 8,
    /// AOS Frame Header Error Control detected an uncorrectable error.
    HeaderUncorrectable = 9,
    /// SCID, VCID, MAP ID, or APID not in the configuration.
    UnknownChannel = 10,
    /// Configuration violates a Blue Book constraint.
    InvalidConfig = 11,
    /// Engine queue full; input rejected without state change.
    WouldBlock = 12,
    /// Operation not permitted in the engine's current state.
    InvalidState = 13,
    /// Security hook reported failure.
    Security = 14,
    /// Configuration exceeds a compile-time capacity.
    CapacityExceeded = 15,
    /// Unknown or stale transmit or request handle.
    InvalidHandle = 16,
    /// Structurally impossible input.
    Malformed = 17,
    /// An internal invariant or consistency check failed.
    InternalError = 18,
}

impl ErrorKind {
    /// Every kind, in code order.
    pub const ALL: [ErrorKind; 18] = [
        ErrorKind::BufferTooSmall,
        ErrorKind::Incomplete,
        ErrorKind::LengthMismatch,
        ErrorKind::UnsupportedVersion,
        ErrorKind::OutOfRange,
        ErrorKind::InvalidField,
        ErrorKind::ReservedValue,
        ErrorKind::CrcMismatch,
        ErrorKind::HeaderUncorrectable,
        ErrorKind::UnknownChannel,
        ErrorKind::InvalidConfig,
        ErrorKind::WouldBlock,
        ErrorKind::InvalidState,
        ErrorKind::Security,
        ErrorKind::CapacityExceeded,
        ErrorKind::InvalidHandle,
        ErrorKind::Malformed,
        ErrorKind::InternalError,
    ];

    /// The stable numeric code (GEN-040).
    #[must_use]
    #[expect(
        clippy::as_conversions,
        reason = "#[repr(u16)] discriminant to its u16 code"
    )]
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// The kind for a numeric code, if defined.
    #[must_use]
    pub fn from_code(code: u16) -> Option<ErrorKind> {
        ErrorKind::ALL.into_iter().find(|k| k.code() == code)
    }

    /// The catalogue name, e.g. `"CrcMismatch"`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ErrorKind::BufferTooSmall => "BufferTooSmall",
            ErrorKind::Incomplete => "Incomplete",
            ErrorKind::LengthMismatch => "LengthMismatch",
            ErrorKind::UnsupportedVersion => "UnsupportedVersion",
            ErrorKind::OutOfRange => "OutOfRange",
            ErrorKind::InvalidField => "InvalidField",
            ErrorKind::ReservedValue => "ReservedValue",
            ErrorKind::CrcMismatch => "CrcMismatch",
            ErrorKind::HeaderUncorrectable => "HeaderUncorrectable",
            ErrorKind::UnknownChannel => "UnknownChannel",
            ErrorKind::InvalidConfig => "InvalidConfig",
            ErrorKind::WouldBlock => "WouldBlock",
            ErrorKind::InvalidState => "InvalidState",
            ErrorKind::Security => "Security",
            ErrorKind::CapacityExceeded => "CapacityExceeded",
            ErrorKind::InvalidHandle => "InvalidHandle",
            ErrorKind::Malformed => "Malformed",
            ErrorKind::InternalError => "InternalError",
        }
    }

    /// The kind with a catalogue name, if defined.
    #[must_use]
    pub fn from_name(name: &str) -> Option<ErrorKind> {
        ErrorKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// Identifies the field an error refers to by its code in the field catalogue, e.g.
/// [`field::APID`](crate::field::APID) (03-api-model.md §9.1; GEN-041, API-082).
///
/// The only values are the constants of [`crate::field`], generated from the catalogue, so no
/// code can be made up at a call site; [`FieldId::from_code`] finds the constant of a code that
/// crossed the C ABI. Equality, ordering, and hashing use the code alone: the name is for
/// display only and is never compared (API-082). There is no `Display`: [`FieldId::name`] is the
/// display name, and [`Error`]'s `Display` prints it.
///
/// ```
/// use aldis_core::field;
/// use aldis_core::ids::Apid;
///
/// let e = Apid::try_from(4096).unwrap_err();
/// assert_eq!(e.field(), Some(field::APID));
/// assert_eq!(field::APID.code().get(), 5); // what the C ABI carries
/// assert_eq!(e.to_string(), "OutOfRange (apid): expected 2047, found 4096");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FieldId {
    code: NonZeroU16,
    name: &'static str,
}

impl FieldId {
    /// The catalogued field `CODE` with its display name. Only the generated module
    /// [`crate::field`] calls it; `CODE` 0 ("no field" in the C ABI) fails to compile.
    pub(crate) const fn catalogued<const CODE: u16>(name: &'static str) -> FieldId {
        // Evaluated at compile time, after the assertion: with `CODE` ≥ 1, `1 + (CODE - 1)` is
        // `CODE`, so the expression adds no run-time branch and no panic path, which
        // `NonZeroU16::new(CODE).unwrap()` would (GEN-020).
        let code = const {
            assert!(CODE != 0, "field code 0 means no field (API-082)");
            NonZeroU16::MIN.saturating_add(CODE.wrapping_sub(1))
        };
        FieldId { code, name }
    }

    /// The field code, shared by all profiles and carried by the C ABI (API-082, CABI-021).
    #[must_use]
    pub const fn code(self) -> NonZeroU16 {
        self.code
    }

    /// The field name, for display only (API-082).
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// The catalogued field with code `code`, or `None` for 0 and codes the catalogue does not
    /// have (API-082, CABI-021): the counterpart of [`ErrorKind::from_code`] for a code that
    /// crossed the C ABI.
    #[must_use]
    pub fn from_code(code: u16) -> Option<FieldId> {
        let code = NonZeroU16::new(code)?;
        crate::field::ALL
            .binary_search_by_key(&code, |f| f.code())
            .ok()
            .and_then(|index| crate::field::ALL.get(index).copied())
    }
}

impl PartialEq for FieldId {
    fn eq(&self, other: &FieldId) -> bool {
        self.code == other.code
    }
}

impl Eq for FieldId {}

impl Hash for FieldId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.code.hash(state);
    }
}

// Written by hand rather than derived, so that the name never enters the comparison (API-082).
impl PartialOrd for FieldId {
    fn partial_cmp(&self, other: &FieldId) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FieldId {
    fn cmp(&self, other: &FieldId) -> Ordering {
        self.code.cmp(&other.code)
    }
}

/// Widens `u8` to `u16` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> u16")]
pub(crate) const fn u8_to_u16(n: u8) -> u16 {
    n as u16
}

/// Widens `u8` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> u64")]
pub(crate) const fn u8_to_u64(n: u8) -> u64 {
    n as u64
}

/// Widens `u16` to `u64` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u16 -> u64")]
pub(crate) const fn u16_to_u64(n: u16) -> u64 {
    n as u64
}

/// Widens `u8` to `usize` in `const` context (`From` is not yet `const`).
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> usize")]
pub(crate) const fn u8_to_usize(n: u8) -> usize {
    n as usize
}

/// Widens `u16` to `usize` in `const` context (`From` is not yet `const`).
#[expect(
    clippy::as_conversions,
    reason = "lossless widening u16 -> usize on 16-bit+ targets"
)]
pub(crate) const fn u16_to_usize(n: u16) -> usize {
    n as usize
}

/// Narrows `n` to `u8` in `const` context if `n <= max` and `n <= u8::MAX`, so a `max` above
/// `u8::MAX` can never truncate (GEN-020).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "checked against u8::MAX before narrowing"
)]
pub(crate) const fn usize_to_u8(n: usize, max: usize) -> Option<u8> {
    if n <= max && n <= u8_to_usize(u8::MAX) {
        Some(n as u8)
    } else {
        None
    }
}

/// Narrows `n` to `u16` in `const` context if `n <= max` and `n <= u16::MAX`, so a `max` above
/// `u16::MAX` can never truncate (GEN-020).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "checked against u16::MAX before narrowing"
)]
pub(crate) const fn usize_to_u16(n: usize, max: usize) -> Option<u16> {
    if n <= max && n <= u16_to_usize(u16::MAX) {
        Some(n as u16)
    } else {
        None
    }
}

/// Converts a `usize` count or offset to `u64` for [`ErrorDetail::Expected`].
#[expect(
    clippy::as_conversions,
    reason = "supported targets are 32- or 64-bit, so usize fits in u64"
)]
pub(crate) const fn as_u64(n: usize) -> u64 {
    n as u64
}

/// Additional detail carried by an error, without allocation (GEN-041).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorDetail {
    /// No further detail.
    None,
    /// Octets (or bits, where documented) required.
    Needed(usize),
    /// The value found, and the value or limit expected.
    Expected {
        /// What the standard or configuration requires.
        expected: u64,
        /// What was found.
        found: u64,
    },
}

/// An error: kind, optional field, and detail (RS-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Error {
    kind: ErrorKind,
    field: Option<FieldId>,
    detail: ErrorDetail,
}

impl Error {
    /// An error of `kind` with no field or detail.
    #[must_use]
    pub const fn new(kind: ErrorKind) -> Error {
        Error {
            kind,
            field: None,
            detail: ErrorDetail::None,
        }
    }

    /// `BufferTooSmall` with the number of octets needed (API-022, API-061).
    #[must_use]
    pub const fn buffer_too_small(needed: usize) -> Error {
        Error::new(ErrorKind::BufferTooSmall).with_detail(ErrorDetail::Needed(needed))
    }

    /// `Incomplete` with the number of octets needed (API-013).
    #[must_use]
    pub const fn incomplete(needed: usize) -> Error {
        Error::new(ErrorKind::Incomplete).with_detail(ErrorDetail::Needed(needed))
    }

    /// `OutOfRange` for `field`, with the limit and the value found (GEN-041).
    #[must_use]
    pub const fn out_of_range(field: FieldId, limit: u64, found: u64) -> Error {
        Error::new(ErrorKind::OutOfRange)
            .with_field(field)
            .with_detail(ErrorDetail::Expected {
                expected: limit,
                found,
            })
    }

    /// The same error, naming `field` (GEN-041, API-082).
    #[must_use]
    pub const fn with_field(self, field: FieldId) -> Error {
        Error {
            field: Some(field),
            ..self
        }
    }

    /// The same error, with `detail`.
    #[must_use]
    pub const fn with_detail(self, detail: ErrorDetail) -> Error {
        Error { detail, ..self }
    }

    /// The error kind.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The field, if known.
    #[must_use]
    pub const fn field(&self) -> Option<FieldId> {
        self.field
    }

    /// The detail.
    #[must_use]
    pub const fn detail(&self) -> ErrorDetail {
        self.detail
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error::new(kind)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = self.kind.name();
        // One write per case: no intermediate `?` paths (formatting into `f` is infallible
        // for every sink used in practice, and untestable when it is not).
        let (open, field, close) = self.field.map_or(("", "", ""), |id| (" (", id.name(), ")"));
        match self.detail {
            ErrorDetail::None => write!(f, "{kind}{open}{field}{close}"),
            ErrorDetail::Needed(n) => write!(f, "{kind}{open}{field}{close}: {n} needed"),
            ErrorDetail::Expected { expected, found } => {
                write!(
                    f,
                    "{kind}{open}{field}{close}: expected {expected}, found {found}"
                )
            }
        }
    }
}

impl core::error::Error for Error {}

/// Result with this crate's [`Error`].
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Error, ErrorDetail, ErrorKind, FieldId, usize_to_u8, usize_to_u16};
    use crate::field;

    #[test]
    fn narrowing_never_truncates() {
        // req: GEN-020
        assert_eq!(usize_to_u8(7, 8), Some(7));
        assert_eq!(usize_to_u8(8, 8), Some(8));
        assert_eq!(usize_to_u8(9, 8), None);
        assert_eq!(usize_to_u8(255, 300), Some(255));
        assert_eq!(usize_to_u8(256, 300), None);
        assert_eq!(usize_to_u8(300, 300), None);
        assert_eq!(usize_to_u8(usize::MAX, usize::MAX), None);
        assert_eq!(usize_to_u16(512, 1024), Some(512));
        assert_eq!(usize_to_u16(1025, 1024), None);
        assert_eq!(usize_to_u16(65_535, 70_000), Some(65_535));
        assert_eq!(usize_to_u16(65_536, 70_000), None);
        assert_eq!(usize_to_u16(70_000, 70_000), None);
        assert_eq!(usize_to_u16(usize::MAX, usize::MAX), None);
    }

    #[test]
    fn codes_are_dense_and_round_trip() {
        // req: GEN-040
        for (i, kind) in ErrorKind::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(kind.code()), i + 1);
            assert_eq!(ErrorKind::from_code(kind.code()), Some(kind));
            assert_eq!(ErrorKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(ErrorKind::from_code(0), None);
        assert_eq!(ErrorKind::from_code(19), None);
        assert_eq!(ErrorKind::from_name("NoSuchKind"), None);
    }

    #[test]
    fn detail_and_display() {
        // req: GEN-041, RS-011
        extern crate std;
        use std::string::ToString;
        let e = Error::out_of_range(field::APID, 2047, 4096);
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        assert_eq!(e.field(), Some(field::APID));
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 2047,
                found: 4096
            }
        );
        assert_eq!(
            e.to_string(),
            "OutOfRange (apid): expected 2047, found 4096"
        );
        assert_eq!(
            Error::buffer_too_small(9).to_string(),
            "BufferTooSmall: 9 needed"
        );
        assert_eq!(Error::incomplete(3).detail(), ErrorDetail::Needed(3));
        assert_eq!(Error::from(ErrorKind::Malformed).to_string(), "Malformed");
        assert_eq!(
            Error::new(ErrorKind::InvalidField)
                .with_field(field::EPI)
                .with_detail(ErrorDetail::Needed(2))
                .to_string(),
            "InvalidField (epi): 2 needed"
        );
        let _: &dyn core::error::Error = &e;
    }

    #[test]
    fn field_codes_are_the_catalogue() {
        // req: API-082, GEN-041
        // Codes run in strictly increasing order, as `cargo xtask fields` checks the table.
        for (i, f) in field::ALL.into_iter().enumerate() {
            if i > 0 {
                assert!(field::ALL[i - 1].code() < f.code());
            }
        }
        assert_eq!(field::SPACECRAFT_ID.code().get(), 1);
        assert_eq!(field::APID.code().get(), 5);
        assert_eq!(field::MAX_DATA_LEN.code().get(), 64);
        assert_eq!(field::APID.name(), "apid");
        // Equality is by code: each constant equals only itself.
        for (i, a) in field::ALL.into_iter().enumerate() {
            for (j, b) in field::ALL.into_iter().enumerate() {
                assert_eq!(a == b, i == j);
            }
        }
    }

    #[test]
    fn field_codes_find_their_constant() {
        // req: API-082, CABI-021
        for f in field::ALL {
            assert_eq!(FieldId::from_code(f.code().get()), Some(f));
        }
        assert_eq!(FieldId::from_code(5).map(FieldId::name), Some("apid"));
        assert_eq!(FieldId::from_code(0), None);
        // Code 107 is in the common block and unassigned
        assert_eq!(FieldId::from_code(107), None);
        assert_eq!(FieldId::from_code(u16::MAX), None);
    }

    #[test]
    fn fields_are_ordered_by_code_only() {
        // req: API-082
        use core::cmp::Ordering;
        assert!(field::SPACECRAFT_ID < field::APID && field::APID < field::MAX_DATA_LEN);
        assert_eq!(field::APID.cmp(&field::APID), Ordering::Equal);
        assert_eq!(
            field::EPI.partial_cmp(&field::APID),
            Some(Ordering::Greater)
        );
        // A name that sorts first does not move a field: the order is the codes' order.
        let renamed = FieldId::catalogued::<64>("aaa");
        assert_eq!(renamed.cmp(&field::MAX_DATA_LEN), Ordering::Equal);
        assert!(renamed > field::SPACECRAFT_ID);
        let mut all = field::ALL;
        all.reverse();
        all.sort();
        assert_eq!(all, field::ALL);
    }

    #[test]
    fn field_equality_and_hash_use_the_code_only() {
        // req: API-082
        extern crate std;
        use core::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        fn hash(f: FieldId) -> u64 {
            let mut h = DefaultHasher::new();
            f.hash(&mut h);
            h.finish()
        }
        // The same code under another display name is the same field.
        let renamed = FieldId::catalogued::<5>("application_process_id");
        assert_eq!(renamed, field::APID);
        assert_eq!(hash(renamed), hash(field::APID));
        assert_ne!(field::APID, field::EPI);
        assert_ne!(hash(field::APID), hash(field::EPI));
        let mut h = DefaultHasher::new();
        field::APID.code().hash(&mut h);
        assert_eq!(hash(field::APID), h.finish());
    }
}
