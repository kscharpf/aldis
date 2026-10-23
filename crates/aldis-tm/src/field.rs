//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082).

pub use aldis_core::field::{
    FIRST_HEADER_POINTER, FRAME_DATA_FIELD, FRAME_LEN, FRAME_VERSION, FSH_FLAG, FSH_LEN,
    FSH_VERSION, OCF_FLAG, PACKET_ORDER_FLAG, SECURITY_HEADER_LEN, SECURITY_TRAILER_LEN,
    SEGMENT_LENGTH_ID, SPACECRAFT_ID, SYNC_FLAG, VC_ID,
};
