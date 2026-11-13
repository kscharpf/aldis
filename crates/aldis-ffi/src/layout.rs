//! The sizes and protocol constants that the header publishes as literals, with the values they
//! must have (CABI-030, CABI-031; M6 plan WP-3d).
//!
//! `cbindgen` publishes a constant as the literal of its definition, and the storage structs
//! need one constant for an array length, so each `ALDIS_*_WORDS` and each protocol constant is
//! written once, in its module. This module lists them next to the value that the type or the
//! codec gives: the words of the slot of the engine state, and the constant of the codec crate.
//! `cargo xtask header` runs the `layout` example, which prints the lists, and fails when a
//! literal does not fit; `cargo xtask header --write` rewrites the literal.

use aldis::aos::header::{FHEC_LEN, PRIMARY_HEADER_LEN as AOS_HEADER_LEN};
use aldis::core::layout::OCF_LEN;
use aldis::epp::config::MAX_DATA_LEN as EPP_MAX_DATA_LEN;
use aldis::spp::header::{MAX_PACKET_LEN, MIN_PACKET_LEN, PRIMARY_HEADER_LEN as SPP_HEADER_LEN};
use aldis::tc::header::{
    MAX_FRAME_LEN as TC_MAX_FRAME_LEN, MIN_FRAME_LEN as TC_MIN_FRAME_LEN,
    PRIMARY_HEADER_LEN as TC_HEADER_LEN, SEGMENT_HEADER_LEN,
};
use aldis::time::ascii::MAX_ASCII_LEN;
use aldis::time::code::MAX_AGENCY_LEN;
use aldis::time::pfield::MAX_PFIELD_LEN;
use aldis::tm::header::PRIMARY_HEADER_LEN as TM_HEADER_LEN;
use aldis::uslp::header::{
    MAX_FRAME_LEN as USLP_MAX_FRAME_LEN, PRIMARY_HEADER_LEN as USLP_HEADER_LEN,
    TRUNCATED_HEADER_LEN,
};

use crate::farm1::{ALDIS_FARM1_WORDS, aldis_farm1_t};
use crate::fop1::{ALDIS_FOP1_WORDS, aldis_fop1_t};
use crate::handle::{Slot, Storage};
use crate::link::access::{ALDIS_ACCESS_GENERATOR_WORDS, aldis_access_generator_t};
use crate::link::bitstream::{ALDIS_BITSTREAM_GENERATOR_WORDS, aldis_bitstream_generator_t};
use crate::link::cltu::{ALDIS_CLTU_PACKER_WORDS, aldis_cltu_packer_t};
use crate::link::extractor::{ALDIS_PACKET_EXTRACTOR_WORDS, aldis_packet_extractor_t};
use crate::link::generator::{ALDIS_PACKET_GENERATOR_WORDS, aldis_packet_generator_t};
use crate::link::mux::{ALDIS_CHANNEL_MUX_WORDS, aldis_channel_mux_t};
use crate::link::reassembler::{ALDIS_MAP_REASSEMBLER_WORDS, aldis_map_reassembler_t};
use crate::link::receiver::{ALDIS_FRAME_RECEIVER_WORDS, aldis_frame_receiver_t};
use crate::link::segmenter::{ALDIS_MAP_SEGMENTER_WORDS, aldis_map_segmenter_t};
use crate::oid::{ALDIS_OID_GENERATOR_WORDS, aldis_oid_generator_t};
use crate::spp_state::{
    ALDIS_SPP_REASSEMBLER_WORDS, ALDIS_SPP_SEGMENTER_WORDS, ALDIS_SPP_TRACKER_WORDS,
    aldis_spp_reassembler_t, aldis_spp_segmenter_t, aldis_spp_tracker_t,
};
use crate::sync::{ALDIS_SYNC_SEARCH_WORDS, aldis_sync_search_t};
use crate::{aos, epp, ocf, spp, tc, time, tm, uslp};

/// How a literal relates to the value that the type or the codec gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The literal is the value. The words of an engine sized by `capacities.toml` and the
    /// protocol constants: a changed capacity or codec constant changes the literal.
    Exact,
    /// The literal is at least the value. The words of an engine whose state does not depend on
    /// a capacity: a literal above the need is room for the state to grow without a change of
    /// the C struct (CABI-031 asks only that the state fits).
    AtLeast,
}

/// A constant of the header: its name, the literal that the header publishes, the value that the
/// type or the codec gives, and how the two relate.
pub type Row = (&'static str, u64, u64, Fit);

/// The 64-bit words that the slot of the state of the handle `T` needs (CABI-031).
#[expect(
    clippy::as_conversions,
    reason = "const context: `usize` widens to u64 here, and `From` is not const"
)]
const fn words_of<T: Storage>() -> u64 {
    size_of::<Slot<T::State>>().div_ceil(8) as u64
}

/// One row of [`WORDS`]: the constant `$name` and the storage type `$ty` that it sizes.
macro_rules! words {
    ($fit:expr; $($name:ident: $ty:ty),+ $(,)?) => {
        [$((stringify!($name), $name as u64, words_of::<$ty>(), $fit)),+]
    };
}

/// One row of [`PROTOCOL`]: the constant `$name` of this crate and the constant `$source` of the
/// codec that it repeats.
macro_rules! protocol {
    ($($module:ident::$name:ident: $source:expr),+ $(,)?) => {
        &[$((stringify!($name), $module::$name as u64, $source as u64, Fit::Exact)),+]
    };
}

/// The `ALDIS_*_WORDS` constants of the engines that `capacities.toml` sizes: the declared words
/// and the words that the state needs. The build fails when a state needs more than is declared
/// (CABI-031).
pub const WORDS_SIZED: &[Row] = &words! { Fit::Exact;
    ALDIS_ACCESS_GENERATOR_WORDS: aldis_access_generator_t,
    ALDIS_BITSTREAM_GENERATOR_WORDS: aldis_bitstream_generator_t,
    ALDIS_CLTU_PACKER_WORDS: aldis_cltu_packer_t,
    ALDIS_PACKET_EXTRACTOR_WORDS: aldis_packet_extractor_t,
    ALDIS_PACKET_GENERATOR_WORDS: aldis_packet_generator_t,
    ALDIS_CHANNEL_MUX_WORDS: aldis_channel_mux_t,
    ALDIS_MAP_REASSEMBLER_WORDS: aldis_map_reassembler_t,
    ALDIS_FRAME_RECEIVER_WORDS: aldis_frame_receiver_t,
    ALDIS_MAP_SEGMENTER_WORDS: aldis_map_segmenter_t,
    ALDIS_SYNC_SEARCH_WORDS: aldis_sync_search_t,
};

/// The `ALDIS_*_WORDS` constants of the handles whose state is fixed or has room (see
/// [`Fit::AtLeast`]).
pub const WORDS_ROOMY: &[Row] = &words! { Fit::AtLeast;
    ALDIS_FARM1_WORDS: aldis_farm1_t,
    ALDIS_FOP1_WORDS: aldis_fop1_t,
    ALDIS_OID_GENERATOR_WORDS: aldis_oid_generator_t,
    ALDIS_SPP_TRACKER_WORDS: aldis_spp_tracker_t,
    ALDIS_SPP_SEGMENTER_WORDS: aldis_spp_segmenter_t,
    ALDIS_SPP_REASSEMBLER_WORDS: aldis_spp_reassembler_t,
};

/// The protocol constants that repeat a constant of a codec crate (the `ALDIS_*_LEN` constants).
#[expect(
    clippy::as_conversions,
    reason = "const context: the lengths widen to u64 here, and `From` is not const"
)]
pub const PROTOCOL: &[Row] = protocol! {
    aos::ALDIS_AOS_PRIMARY_HEADER_LEN: AOS_HEADER_LEN,
    aos::ALDIS_AOS_FHEC_LEN: FHEC_LEN,
    epp::ALDIS_EPP_MAX_DATA_LEN: EPP_MAX_DATA_LEN,
    ocf::ALDIS_OCF_LEN: OCF_LEN,
    spp::ALDIS_SPP_PRIMARY_HEADER_LEN: SPP_HEADER_LEN,
    spp::ALDIS_SPP_MIN_PACKET_LEN: MIN_PACKET_LEN,
    spp::ALDIS_SPP_MAX_PACKET_LEN: MAX_PACKET_LEN,
    tc::ALDIS_TC_PRIMARY_HEADER_LEN: TC_HEADER_LEN,
    tc::ALDIS_TC_SEGMENT_HEADER_LEN: SEGMENT_HEADER_LEN,
    tc::ALDIS_TC_MIN_FRAME_LEN: TC_MIN_FRAME_LEN,
    tc::ALDIS_TC_MAX_FRAME_LEN: TC_MAX_FRAME_LEN,
    time::ALDIS_TIME_MAX_PFIELD_LEN: MAX_PFIELD_LEN,
    time::ALDIS_TIME_MAX_AGENCY_LEN: MAX_AGENCY_LEN,
    time::ALDIS_TIME_MAX_ASCII_LEN: MAX_ASCII_LEN,
    tm::ALDIS_TM_PRIMARY_HEADER_LEN: TM_HEADER_LEN,
    uslp::ALDIS_USLP_PRIMARY_HEADER_LEN: USLP_HEADER_LEN,
    uslp::ALDIS_USLP_TRUNCATED_HEADER_LEN: TRUNCATED_HEADER_LEN,
    uslp::ALDIS_USLP_MAX_FRAME_LEN: USLP_MAX_FRAME_LEN,
};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Fit, PROTOCOL, WORDS_ROOMY, WORDS_SIZED};

    // req: CABI-031
    #[test]
    fn every_literal_fits_its_value() {
        for (name, literal, value, fit) in WORDS_SIZED.iter().chain(WORDS_ROOMY).chain(PROTOCOL) {
            match fit {
                Fit::Exact => assert_eq!(literal, value, "{name}"),
                Fit::AtLeast => assert!(literal >= value, "{name}"),
            }
        }
    }
}
