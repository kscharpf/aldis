//! The sizes and protocol constants that the header publishes as literals, with the values they
//! must have (CABI-030, CABI-031; M6 plan WP-3d; GEN-009).
//!
//! `cbindgen` publishes a constant as the literal of its definition, and the storage structs
//! need one constant for an array length, so each `ALDIS_*_WORDS` and each protocol constant is
//! written once, in its module. This module lists them next to the value that the type or the
//! codec gives: the words of the slot of the engine state, and the constant of the codec crate.
//! `cargo xtask header` runs the `layout` example, which prints the lists, and fails when a
//! literal does not fit; `cargo xtask header --write` rewrites the literal.
//!
//! **Subsets (GEN-009).** The header is whole and every storage struct keeps the size of the
//! full build in every subset library, so that a caller's `aldis_x_t` has one size for any
//! library it links (CABI-031). The `words` literals are those of the full build. A subset has
//! fewer protocols in the enum of an engine's state, so its state can need fewer words than the
//! literal: the rows of the engines that `capacities.toml` sizes are [`Fit::Exact`] only with
//! the feature `full`, and [`Fit::AtLeast`] in a subset, where the build still fails when a
//! state needs more than the literal. A subset has the rows of the engines and codecs it has.

#[cfg(feature = "aos")]
use aldis::aos::header::{FHEC_LEN, PRIMARY_HEADER_LEN as AOS_HEADER_LEN};
#[cfg(feature = "core")]
use aldis::core::layout::OCF_LEN;
#[cfg(feature = "epp")]
use aldis::epp::config::MAX_DATA_LEN as EPP_MAX_DATA_LEN;
#[cfg(feature = "spp")]
use aldis::spp::header::{MAX_PACKET_LEN, MIN_PACKET_LEN, PRIMARY_HEADER_LEN as SPP_HEADER_LEN};
#[cfg(feature = "tc")]
use aldis::tc::header::{
    MAX_FRAME_LEN as TC_MAX_FRAME_LEN, MIN_FRAME_LEN as TC_MIN_FRAME_LEN,
    PRIMARY_HEADER_LEN as TC_HEADER_LEN, SEGMENT_HEADER_LEN,
};
#[cfg(feature = "time")]
use aldis::time::ascii::MAX_ASCII_LEN;
#[cfg(feature = "time")]
use aldis::time::code::MAX_AGENCY_LEN;
#[cfg(feature = "time")]
use aldis::time::pfield::MAX_PFIELD_LEN;
#[cfg(feature = "tm")]
use aldis::tm::header::PRIMARY_HEADER_LEN as TM_HEADER_LEN;
#[cfg(feature = "uslp")]
use aldis::uslp::header::{
    MAX_FRAME_LEN as USLP_MAX_FRAME_LEN, PRIMARY_HEADER_LEN as USLP_HEADER_LEN,
    TRUNCATED_HEADER_LEN,
};

#[cfg(feature = "aos")]
use crate::aos;
#[cfg(feature = "epp")]
use crate::epp;
#[cfg(all(feature = "cop1", feature = "tc"))]
use crate::farm1::{ALDIS_FARM1_WORDS, aldis_farm1_t};
#[cfg(all(feature = "cop1", feature = "tc"))]
use crate::fop1::{ALDIS_FOP1_WORDS, aldis_fop1_t};
use crate::handle::{Slot, Storage};
#[cfg(all(
    feature = "link",
    any(feature = "tm", feature = "aos", feature = "uslp")
))]
use crate::link::access::{ALDIS_ACCESS_GENERATOR_WORDS, aldis_access_generator_t};
#[cfg(all(feature = "link", feature = "aos"))]
use crate::link::bitstream::{ALDIS_BITSTREAM_GENERATOR_WORDS, aldis_bitstream_generator_t};
#[cfg(all(feature = "link", any(feature = "tc", feature = "uslp")))]
use crate::link::cltu::{ALDIS_CLTU_PACKER_WORDS, aldis_cltu_packer_t};
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(
        feature = "tm",
        feature = "aos",
        all(feature = "uslp", feature = "epp")
    )
))]
use crate::link::extractor::{ALDIS_PACKET_EXTRACTOR_WORDS, aldis_packet_extractor_t};
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(
        feature = "tm",
        feature = "aos",
        all(feature = "uslp", feature = "epp")
    )
))]
use crate::link::generator::{ALDIS_PACKET_GENERATOR_WORDS, aldis_packet_generator_t};
#[cfg(all(
    feature = "link",
    any(feature = "tm", feature = "aos", feature = "uslp")
))]
use crate::link::mux::{ALDIS_CHANNEL_MUX_WORDS, aldis_channel_mux_t};
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(feature = "tc", feature = "uslp")
))]
use crate::link::reassembler::{ALDIS_MAP_REASSEMBLER_WORDS, aldis_map_reassembler_t};
#[cfg(all(
    feature = "link",
    any(feature = "tm", feature = "tc", feature = "aos", feature = "uslp")
))]
use crate::link::receiver::{ALDIS_FRAME_RECEIVER_WORDS, aldis_frame_receiver_t};
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(feature = "tc", feature = "uslp")
))]
use crate::link::segmenter::{ALDIS_MAP_SEGMENTER_WORDS, aldis_map_segmenter_t};
#[cfg(feature = "core")]
use crate::ocf;
#[cfg(feature = "core")]
use crate::oid::{ALDIS_OID_GENERATOR_WORDS, aldis_oid_generator_t};
#[cfg(feature = "spp")]
use crate::spp;
#[cfg(feature = "spp")]
use crate::spp_state::{
    ALDIS_SPP_REASSEMBLER_WORDS, ALDIS_SPP_SEGMENTER_WORDS, ALDIS_SPP_TRACKER_WORDS,
    aldis_spp_reassembler_t, aldis_spp_segmenter_t, aldis_spp_tracker_t,
};
#[cfg(feature = "sync")]
use crate::sync::{ALDIS_SYNC_SEARCH_WORDS, aldis_sync_search_t};
#[cfg(feature = "tc")]
use crate::tc;
#[cfg(feature = "time")]
use crate::time;
#[cfg(feature = "tm")]
use crate::tm;
#[cfg(feature = "uslp")]
use crate::uslp;

/// How a literal relates to the value that the type or the codec gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The literal is the value. The words of an engine sized by `capacities.toml` and the
    /// protocol constants: a changed capacity or codec constant changes the literal.
    Exact,
    /// The literal is at least the value. The words of an engine whose state does not depend on
    /// a capacity: a literal above the need is room for the state to grow without a change of
    /// the C struct (CABI-031 asks only that the state fits). The spare words are deliberate
    /// (maintainer, 2026-11-13).
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

/// One row of [`WORDS_SIZED`] or [`WORDS_ROOMY`]: the constant `$name` and the storage type `$ty`
/// that it sizes, with the `cfg` of the feature that builds them.
macro_rules! words {
    ($fit:expr; $($(#[$cfg:meta])* $name:ident: $ty:ty),+ $(,)?) => {
        [$($(#[$cfg])* (stringify!($name), $name as u64, words_of::<$ty>(), $fit)),+]
    };
}

/// One row of [`PROTOCOL`]: the constant `$name` of this crate and the constant `$source` of the
/// codec that it repeats.
macro_rules! protocol {
    ($($(#[$cfg:meta])* $module:ident::$name:ident: $source:expr),+ $(,)?) => {
        &[$($(#[$cfg])* (stringify!($name), $module::$name as u64, $source as u64, Fit::Exact)),+]
    };
}

/// How the words of an engine sized by `capacities.toml` relate to what its state needs.
///
/// The literal is that of the full build: it is the size of the largest engine of every
/// protocol, and a header and a storage struct are one for every library (the header is whole,
/// CABI-031, c-abi.md after CABI-060). In the full build the literal is the value, so
/// `cargo xtask header` rewrites it. A subset library has fewer protocols in its engine enums,
/// so a state needs fewer words than the literal gives: the literal is at least the value, and
/// the storage struct keeps its size.
const SIZED_FIT: Fit = if cfg!(feature = "full") {
    Fit::Exact
} else {
    Fit::AtLeast
};

/// The `ALDIS_*_WORDS` constants of the engines that `capacities.toml` sizes.
///
/// A row is the declared words and the words that the state needs. The build fails when a state
/// needs more than is declared (CABI-031). A subset build has the rows of the engines it has.
pub const WORDS_SIZED: &[Row] = &words! { SIZED_FIT;
    #[cfg(all(feature = "link", any(feature = "tm", feature = "aos", feature = "uslp")))]
    ALDIS_ACCESS_GENERATOR_WORDS: aldis_access_generator_t,
    #[cfg(all(feature = "link", feature = "aos"))]
    ALDIS_BITSTREAM_GENERATOR_WORDS: aldis_bitstream_generator_t,
    #[cfg(all(feature = "link", any(feature = "tc", feature = "uslp")))]
    ALDIS_CLTU_PACKER_WORDS: aldis_cltu_packer_t,
    #[cfg(all(
        feature = "link",
        any(feature = "spp", feature = "epp"),
        any(feature = "tm", feature = "aos", all(feature = "uslp", feature = "epp"))
    ))]
    ALDIS_PACKET_EXTRACTOR_WORDS: aldis_packet_extractor_t,
    #[cfg(all(
        feature = "link",
        any(feature = "spp", feature = "epp"),
        any(feature = "tm", feature = "aos", all(feature = "uslp", feature = "epp"))
    ))]
    ALDIS_PACKET_GENERATOR_WORDS: aldis_packet_generator_t,
    #[cfg(all(feature = "link", any(feature = "tm", feature = "aos", feature = "uslp")))]
    ALDIS_CHANNEL_MUX_WORDS: aldis_channel_mux_t,
    #[cfg(all(
        feature = "link",
        any(feature = "spp", feature = "epp"),
        any(feature = "tc", feature = "uslp")
    ))]
    ALDIS_MAP_REASSEMBLER_WORDS: aldis_map_reassembler_t,
    #[cfg(all(
        feature = "link",
        any(feature = "tm", feature = "tc", feature = "aos", feature = "uslp")
    ))]
    ALDIS_FRAME_RECEIVER_WORDS: aldis_frame_receiver_t,
    #[cfg(all(
        feature = "link",
        any(feature = "spp", feature = "epp"),
        any(feature = "tc", feature = "uslp")
    ))]
    ALDIS_MAP_SEGMENTER_WORDS: aldis_map_segmenter_t,
    #[cfg(feature = "sync")]
    ALDIS_SYNC_SEARCH_WORDS: aldis_sync_search_t,
};

/// The `ALDIS_*_WORDS` constants of the handles whose state is fixed or has room (see
/// [`Fit::AtLeast`]).
pub const WORDS_ROOMY: &[Row] = &words! { Fit::AtLeast;
    #[cfg(all(feature = "cop1", feature = "tc"))]
    ALDIS_FARM1_WORDS: aldis_farm1_t,
    #[cfg(all(feature = "cop1", feature = "tc"))]
    ALDIS_FOP1_WORDS: aldis_fop1_t,
    #[cfg(feature = "core")]
    ALDIS_OID_GENERATOR_WORDS: aldis_oid_generator_t,
    #[cfg(feature = "spp")]
    ALDIS_SPP_TRACKER_WORDS: aldis_spp_tracker_t,
    #[cfg(feature = "spp")]
    ALDIS_SPP_SEGMENTER_WORDS: aldis_spp_segmenter_t,
    #[cfg(feature = "spp")]
    ALDIS_SPP_REASSEMBLER_WORDS: aldis_spp_reassembler_t,
};

/// The protocol constants that repeat a constant of a codec crate (the `ALDIS_*_LEN` constants).
#[expect(
    clippy::as_conversions,
    reason = "const context: the lengths widen to u64 here, and `From` is not const"
)]
pub const PROTOCOL: &[Row] = protocol! {
    #[cfg(feature = "aos")]
    aos::ALDIS_AOS_PRIMARY_HEADER_LEN: AOS_HEADER_LEN,
    #[cfg(feature = "aos")]
    aos::ALDIS_AOS_FHEC_LEN: FHEC_LEN,
    #[cfg(feature = "epp")]
    epp::ALDIS_EPP_MAX_DATA_LEN: EPP_MAX_DATA_LEN,
    #[cfg(feature = "core")]
    ocf::ALDIS_OCF_LEN: OCF_LEN,
    #[cfg(feature = "spp")]
    spp::ALDIS_SPP_PRIMARY_HEADER_LEN: SPP_HEADER_LEN,
    #[cfg(feature = "spp")]
    spp::ALDIS_SPP_MIN_PACKET_LEN: MIN_PACKET_LEN,
    #[cfg(feature = "spp")]
    spp::ALDIS_SPP_MAX_PACKET_LEN: MAX_PACKET_LEN,
    #[cfg(feature = "tc")]
    tc::ALDIS_TC_PRIMARY_HEADER_LEN: TC_HEADER_LEN,
    #[cfg(feature = "tc")]
    tc::ALDIS_TC_SEGMENT_HEADER_LEN: SEGMENT_HEADER_LEN,
    #[cfg(feature = "tc")]
    tc::ALDIS_TC_MIN_FRAME_LEN: TC_MIN_FRAME_LEN,
    #[cfg(feature = "tc")]
    tc::ALDIS_TC_MAX_FRAME_LEN: TC_MAX_FRAME_LEN,
    #[cfg(feature = "time")]
    time::ALDIS_TIME_MAX_PFIELD_LEN: MAX_PFIELD_LEN,
    #[cfg(feature = "time")]
    time::ALDIS_TIME_MAX_AGENCY_LEN: MAX_AGENCY_LEN,
    #[cfg(feature = "time")]
    time::ALDIS_TIME_MAX_ASCII_LEN: MAX_ASCII_LEN,
    #[cfg(feature = "tm")]
    tm::ALDIS_TM_PRIMARY_HEADER_LEN: TM_HEADER_LEN,
    #[cfg(feature = "uslp")]
    uslp::ALDIS_USLP_PRIMARY_HEADER_LEN: USLP_HEADER_LEN,
    #[cfg(feature = "uslp")]
    uslp::ALDIS_USLP_TRUNCATED_HEADER_LEN: TRUNCATED_HEADER_LEN,
    #[cfg(feature = "uslp")]
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
