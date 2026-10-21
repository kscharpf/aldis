//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082). Errors that
//! `aldis-time` returns through a time secondary header name the fields of
//! `aldis_time::field`.

pub use aldis_core::field::{
    APID, DATA_FIELD, MAX_PACKET_LEN, MAX_USER_LEN, PACKET_LEN, PACKET_VERSION, SDU,
    SECONDARY_HEADER, SECONDARY_HEADER_FLAG, USER_LEN,
};
/// Fields named only with the typed time secondary header (feature `time`, SPP-008).
#[cfg(feature = "time")]
pub use aldis_core::field::{TIME_CODE, TIME_CODE_ID};
