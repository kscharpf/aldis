//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082).

pub use aldis_core::field::{
    AOS_HEADER_SPARE, BITSTREAM_DATA_POINTER, BPDU_SPARE, DATA_FIELD_CONTENT, FIRST_HEADER_POINTER,
    FRAME_DATA_FIELD, FRAME_LEN, FRAME_VERSION, INSERT_ZONE_LEN, MPDU_SPARE, OCF,
    SECURITY_HEADER_LEN, SECURITY_TRAILER_LEN, SPACECRAFT_ID, VC_FRAME_COUNT_CYCLE, VC_ID,
    VCF_COUNT,
};
