//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082).

pub use aldis_core::field::{
    END_OF_FRAME_PRIMARY_HEADER_FLAG, FHP_LVO_POINTER, FRAME_DATA_FIELD, FRAME_LEN, FRAME_VERSION,
    INSERT_ZONE_LEN, MAP_ID, MAX_FRAME_LEN, OCF_FLAG, PROTOCOL_CONTROL_COMMAND_FLAG,
    SECURITY_HEADER_LEN, SECURITY_TRAILER_LEN, SPACECRAFT_ID, TFDZ_CONSTRUCTION_RULE,
    TRANSFER_FRAME_TYPE, TRUNCATED_FRAME_LEN, UPID, USER_LEN, USLP_HEADER_SPARE, VC_ID, VCF_COUNT,
    VCF_COUNT_LEN,
};
