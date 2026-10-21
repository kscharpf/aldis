//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082).

pub use aldis_core::field::{
    CCSDS_DEFINED, DATA_LEN, EPI, EPI_EXTENSION, HEADER_SIZE, MAX_DATA_LEN, MIN_DATA_LEN,
    PACKET_LEN, PACKET_VERSION, USER_DEFINED,
};
