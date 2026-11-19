//! The storage structs have the size and alignment of the full build in every library that
//! has them, so that a caller's `aldis_x_t` is one size for any library it links (CABI-031,
//! GEN-009; c-abi.md, after CABI-060).
//!
//! The header is whole and is generated from the full build. A subset library (`--no-default-features
//! --features …`) has the structs of the features it has: each one is `ALDIS_X_WORDS` 64-bit
//! words of that header, 8-aligned, whatever the engines of the subset need. This test has no
//! `required-features`, so every subset of the matrix of `cargo xtask features` runs it with the
//! structs it has.
#![expect(clippy::unwrap_used, reason = "test-only harness (RS-018)")]

/// The header of the full build, whose literals every subset library must keep.
const HEADER: &str = include_str!("../../../include/aldis/aldis.h");

/// The value of `#define <name> <n>` in the header.
fn header_words(name: &str) -> usize {
    let prefix = format!("#define {name} ");
    let value = HEADER.lines().find_map(|l| l.strip_prefix(&prefix));
    assert!(value.is_some(), "{name} is not in the header");
    value.unwrap().trim().parse().unwrap()
}

/// Checks one storage struct: its literal is the header's, its size is that many words, and it is
/// 8-aligned.
macro_rules! storage {
    ($name:ident, $words:path, $ty:path) => {
        #[test]
        fn $name() {
            // req: CABI-031, GEN-009
            let words = header_words(stringify!($words).rsplit("::").next().unwrap());
            assert_eq!($words, words, "the literal is the header's");
            assert_eq!(size_of::<$ty>(), words * 8);
            assert_eq!(align_of::<$ty>(), 8);
        }
    };
}

#[cfg(feature = "core")]
storage!(
    oid_generator,
    aldis_ffi::oid::ALDIS_OID_GENERATOR_WORDS,
    aldis_ffi::oid::aldis_oid_generator_t
);
#[cfg(feature = "spp")]
storage!(
    spp_tracker,
    aldis_ffi::spp_state::ALDIS_SPP_TRACKER_WORDS,
    aldis_ffi::spp_state::aldis_spp_tracker_t
);
#[cfg(feature = "spp")]
storage!(
    spp_segmenter,
    aldis_ffi::spp_state::ALDIS_SPP_SEGMENTER_WORDS,
    aldis_ffi::spp_state::aldis_spp_segmenter_t
);
#[cfg(feature = "spp")]
storage!(
    spp_reassembler,
    aldis_ffi::spp_state::ALDIS_SPP_REASSEMBLER_WORDS,
    aldis_ffi::spp_state::aldis_spp_reassembler_t
);
#[cfg(feature = "sync")]
storage!(
    sync_search,
    aldis_ffi::sync::ALDIS_SYNC_SEARCH_WORDS,
    aldis_ffi::sync::aldis_sync_search_t
);
#[cfg(all(feature = "cop1", feature = "tc"))]
storage!(
    farm1,
    aldis_ffi::farm1::ALDIS_FARM1_WORDS,
    aldis_ffi::farm1::aldis_farm1_t
);
#[cfg(all(feature = "cop1", feature = "tc"))]
storage!(
    fop1,
    aldis_ffi::fop1::ALDIS_FOP1_WORDS,
    aldis_ffi::fop1::aldis_fop1_t
);
#[cfg(all(
    feature = "link",
    any(feature = "tm", feature = "aos", feature = "uslp")
))]
storage!(
    access_generator,
    aldis_ffi::link::access::ALDIS_ACCESS_GENERATOR_WORDS,
    aldis_ffi::link::access::aldis_access_generator_t
);
#[cfg(all(feature = "link", feature = "aos"))]
storage!(
    bitstream_generator,
    aldis_ffi::link::bitstream::ALDIS_BITSTREAM_GENERATOR_WORDS,
    aldis_ffi::link::bitstream::aldis_bitstream_generator_t
);
#[cfg(all(feature = "link", any(feature = "tc", feature = "uslp")))]
storage!(
    cltu_packer,
    aldis_ffi::link::cltu::ALDIS_CLTU_PACKER_WORDS,
    aldis_ffi::link::cltu::aldis_cltu_packer_t
);
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(
        feature = "tm",
        feature = "aos",
        all(feature = "uslp", feature = "epp")
    )
))]
storage!(
    packet_extractor,
    aldis_ffi::link::extractor::ALDIS_PACKET_EXTRACTOR_WORDS,
    aldis_ffi::link::extractor::aldis_packet_extractor_t
);
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(
        feature = "tm",
        feature = "aos",
        all(feature = "uslp", feature = "epp")
    )
))]
storage!(
    packet_generator,
    aldis_ffi::link::generator::ALDIS_PACKET_GENERATOR_WORDS,
    aldis_ffi::link::generator::aldis_packet_generator_t
);
#[cfg(all(
    feature = "link",
    any(feature = "tm", feature = "aos", feature = "uslp")
))]
storage!(
    channel_mux,
    aldis_ffi::link::mux::ALDIS_CHANNEL_MUX_WORDS,
    aldis_ffi::link::mux::aldis_channel_mux_t
);
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(feature = "tc", feature = "uslp")
))]
storage!(
    map_reassembler,
    aldis_ffi::link::reassembler::ALDIS_MAP_REASSEMBLER_WORDS,
    aldis_ffi::link::reassembler::aldis_map_reassembler_t
);
#[cfg(all(
    feature = "link",
    any(feature = "spp", feature = "epp"),
    any(feature = "tc", feature = "uslp")
))]
storage!(
    map_segmenter,
    aldis_ffi::link::segmenter::ALDIS_MAP_SEGMENTER_WORDS,
    aldis_ffi::link::segmenter::aldis_map_segmenter_t
);
#[cfg(all(
    feature = "link",
    any(feature = "tm", feature = "tc", feature = "aos", feature = "uslp")
))]
storage!(
    frame_receiver,
    aldis_ffi::link::receiver::ALDIS_FRAME_RECEIVER_WORDS,
    aldis_ffi::link::receiver::aldis_frame_receiver_t
);
