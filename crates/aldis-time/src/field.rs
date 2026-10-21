//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082).

pub use aldis_core::field::{
    AGENCY_EPOCH, CDS_DAY, COARSE, COARSE_OCTETS, DATE, DAY, DAY_OF_YEAR, FINE, FINE_OCTETS,
    FRACTION, FRACTION_DIGITS, HOUR, MAX_FRACTION_DIGITS, MINUTE, MISSION_BITS, MONTH, MS_OF_DAY,
    NANOSECONDS, PFIELD, PFIELD_TAIL, SECOND, SUBMS, SUBSECOND, SUBSECOND_OCTETS, T_LEN, TFIELD,
    TIME_CODE_ID, YEAR,
};
