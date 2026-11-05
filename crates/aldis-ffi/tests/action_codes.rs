//! The numeric codes of the actions that every engine returns (API-080). A caller switches on
//! `kind`; the code of a kind never changes, and one code space serves every protocol of an
//! engine, since one handle type serves them all (CABI-032). The values here are the ones
//! released in `include/aldis/aldis.h`; a change of one fails this test.

use aldis_ffi::farm1::aldis_farm1_action::*;
use aldis_ffi::fop1::aldis_fop1_action::*;
use aldis_ffi::link_cltu::aldis_cltu_packer_action::*;
use aldis_ffi::link_gen::aldis_access_generator_action::*;
use aldis_ffi::link_gen::aldis_bitstream_generator_action::*;
use aldis_ffi::link_gen::aldis_packet_generator_action::*;
use aldis_ffi::link_map::aldis_map_reassembler_action::*;
use aldis_ffi::link_map::aldis_map_segmenter_action::*;
use aldis_ffi::link_mux::aldis_channel_mux_action::*;
use aldis_ffi::link_rx::aldis_frame_receiver_action::*;
use aldis_ffi::link_rx::aldis_packet_extractor_action::*;

#[test]
#[expect(
    clippy::as_conversions,
    reason = "the point of the test is the numeric value of each `repr(u32)` variant"
)]
fn the_action_codes_are_stable() {
    // req: API-080
    assert_eq!(
        [
            ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD as u32,
            ALDIS_PACKET_GENERATOR_ACTION_DISCARDED as u32,
            ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD as u32,
            ALDIS_ACCESS_GENERATOR_ACTION_DISCARDED as u32,
            ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD as u32,
            ALDIS_BITSTREAM_GENERATOR_ACTION_DISCARDED as u32,
            ALDIS_CHANNEL_MUX_ACTION_TRANSMIT as u32,
            ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED as u32,
            ALDIS_CHANNEL_MUX_ACTION_DISCARDED as u32,
            ALDIS_MAP_SEGMENTER_ACTION_FDU as u32,
            ALDIS_MAP_SEGMENTER_ACTION_DISCARDED as u32,
            ALDIS_CLTU_PACKER_ACTION_DATA_UNIT as u32,
            ALDIS_CLTU_PACKER_ACTION_DISCARDED as u32,
        ],
        [0, 1, 0, 1, 0, 1, 0, 1, 2, 0, 1, 0, 1]
    );
    assert_eq!(
        [
            ALDIS_FRAME_RECEIVER_ACTION_FRAME_GAP as u32,
            ALDIS_FRAME_RECEIVER_ACTION_COUNT_DISCONTINUITY as u32,
            ALDIS_FRAME_RECEIVER_ACTION_FRAME_DISCARDED as u32,
            ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG as u32,
            ALDIS_FRAME_RECEIVER_ACTION_DATA_FIELD as u32,
            ALDIS_FRAME_RECEIVER_ACTION_OCF as u32,
            ALDIS_FRAME_RECEIVER_ACTION_FSH_SDU as u32,
            ALDIS_FRAME_RECEIVER_ACTION_INSERT_SDU as u32,
            ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME as u32,
            ALDIS_FRAME_RECEIVER_ACTION_PROTOCOL_CONTROL as u32,
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(
        [
            ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS as u32,
            ALDIS_PACKET_EXTRACTOR_ACTION_INCOMPLETE_PACKET as u32,
            ALDIS_PACKET_EXTRACTOR_ACTION_DISCARDED as u32,
            ALDIS_PACKET_EXTRACTOR_ACTION_ERROR as u32,
            ALDIS_MAP_REASSEMBLER_ACTION_EVENT as u32,
            ALDIS_MAP_REASSEMBLER_ACTION_SDU as u32,
            ALDIS_MAP_REASSEMBLER_ACTION_PACKETS as u32,
            ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA as u32,
            ALDIS_MAP_REASSEMBLER_ACTION_DISCARDED as u32,
        ],
        [0, 1, 2, 3, 0, 1, 2, 3, 4]
    );
    assert_eq!(
        [
            ALDIS_FARM1_ACTION_DELIVER as u32,
            ALDIS_FARM1_ACTION_FRAME_DISCARDED as u32,
            ALDIS_FARM1_ACTION_STATE_CHANGED as u32,
            ALDIS_FARM1_ACTION_UNLOCKED as u32,
            ALDIS_FOP1_ACTION_TRANSMIT as u32,
            ALDIS_FOP1_ACTION_ABORT as u32,
            ALDIS_FOP1_ACTION_TRANSFER_NOTIFY as u32,
            ALDIS_FOP1_ACTION_DIRECTIVE_NOTIFY as u32,
            ALDIS_FOP1_ACTION_ALERT as u32,
            ALDIS_FOP1_ACTION_SUSPEND as u32,
        ],
        [0, 1, 2, 3, 0, 1, 2, 3, 4, 5]
    );
}
