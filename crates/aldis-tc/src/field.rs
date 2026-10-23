//! The field codes that this crate's errors name (03-api-model.md §9.1; API-082, GEN-041).
//!
//! Re-exported from [`aldis_core::field`], where every constant is generated from the field
//! catalogue, so a constant never moves when another crate starts to use its field (M2 plan D2).
//! Compare an error's field with these constants, never by name (API-082).

pub use aldis_core::field::{
    CLCW_SPARE_14_15, CLCW_SPARE_23, CLCW_STATUS, CLCW_VERSION, CONTROL_COMMAND,
    CONTROL_COMMAND_FLAG, CONTROL_WORD_TYPE, COP_IN_EFFECT, FARM_B_COUNTER, FRAME_DATA_FIELD,
    FRAME_LEN, FRAME_VERSION, MAP_ID, MAX_FRAME_LEN, SEGMENT_HEADER, SPACECRAFT_ID,
    TC_HEADER_SPARE, USER_LEN, VC_ID,
};
