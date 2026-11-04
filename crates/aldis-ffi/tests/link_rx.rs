//! The contract of the receiving engine handles, called as a C caller calls them: storage and
//! tags (CABI-031, CABI-032), argument checks (CABI-022), the protocol chosen in the
//! configuration (D4, LNK-001), the step call and the read-only calls (CABI-040), and the
//! capacities of the header (CABI-030). The engines' behaviour is checked by the scenarios of
//! `test-vectors/link/recv/` (`vectors.rs`).
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::borrow_as_ptr,
    clippy::too_many_lines,
    clippy::undocumented_unsafe_blocks,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test-only harness: one long call sequence per contract, and the storage a C caller declares (RS-018)"
)]

use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::capacities::ALDIS_LINK_MAX_VIRTUAL_CHANNELS;
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_DATA_FIELD_CONTENT, ALDIS_FIELD_FSH_LEN, ALDIS_FIELD_LINK_PROTOCOL,
    ALDIS_FIELD_MAX_PACKET_LEN, ALDIS_FIELD_RAW_FRAMES, ALDIS_FIELD_VC_ID,
};
use aldis_ffi::link::*;
use aldis_ffi::link_rx::*;

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

fn pe_storage() -> Box<aldis_packet_extractor_t> {
    Box::new(aldis_packet_extractor_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_PACKET_EXTRACTOR_WORDS],
    })
}

fn fr_storage() -> Box<aldis_frame_receiver_t> {
    Box::new(aldis_frame_receiver_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_FRAME_RECEIVER_WORDS],
    })
}

const fn channel(protocol: u32) -> aldis_link_channel_t {
    aldis_link_channel_t {
        protocol,
        vc_id: 0,
        aos_edition: 0,
        uslp_upid: 0,
        uslp_map: 1,
        uslp_qos: ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
    }
}

const fn pe_config(protocol: u32) -> aldis_packet_extractor_config_t {
    aldis_packet_extractor_config_t {
        channel: channel(protocol),
        space_packets: 1,
        encapsulation_packets: 1,
        max_packet_len: 64,
        deliver_incomplete: 0,
        deliver_idle: 0,
    }
}

const fn pe_input(kind: u32) -> aldis_packet_extractor_input_t {
    aldis_packet_extractor_input_t {
        kind,
        data: null(),
        data_len: 0,
        header_pointer_present: 0,
        header_pointer: 0,
        gap: 0,
        config: pe_config(ALDIS_LINK_PROTOCOL_TM),
    }
}

fn pe_step(
    h: &mut aldis_packet_extractor_t,
    input: &aldis_packet_extractor_input_t,
    out: &mut [u8],
    actions: &mut aldis_packet_extractor_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_packet_extractor_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

fn fr_config(protocol: u32) -> aldis_frame_receiver_config_t {
    let mut c = aldis_frame_receiver_config_t {
        protocol,
        frame_len: 32,
        fecf: 0,
        raw_frames: 0,
        ocf_service: ALDIS_LINK_SERVICE_NONE,
        fsh_service: ALDIS_LINK_SERVICE_NONE,
        insert_zone_len: 0,
        aos_fhec: 0,
        aos_edition: 0,
        uslp_variable: 0,
        vc_count: 1,
        vcs: [aldis_frame_receiver_vc_t::default(); ALDIS_LINK_MAX_VIRTUAL_CHANNELS],
    };
    c.vcs[0].vc_id = 1;
    c
}

fn fr_input(kind: u32) -> aldis_frame_receiver_input_t {
    aldis_frame_receiver_input_t {
        kind,
        data: null(),
        data_len: 0,
        reason: 0,
        config: fr_config(ALDIS_LINK_PROTOCOL_TM),
    }
}

fn fr_step(
    h: &mut aldis_frame_receiver_t,
    input: &aldis_frame_receiver_input_t,
    out: &mut [u8],
    actions: &mut aldis_frame_receiver_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_frame_receiver_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

fn fr_init(
    s: &mut aldis_frame_receiver_t,
    cfg: &aldis_frame_receiver_config_t,
    d: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_frame_receiver_init(s, cfg, d) }
}

/// A Space Packet of `total` octets (133.0 §4.1.3.2).
fn packet(total: usize) -> Vec<u8> {
    let mut p = vec![0x00, 0x05, 0xC0, 0x00];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, 0xAB);
    p
}

#[test]
fn capacities_are_published_and_match_the_engines() {
    // req: CABI-030, CABI-031, LNK-074
    assert_eq!(
        ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS,
        aldis::link::extractor::EXTRACTOR_MAX_ACTIONS
    );
    assert_eq!(
        ALDIS_FRAME_RECEIVER_MAX_ACTIONS,
        aldis::link::receiver::FRAME_RECEIVER_MAX_ACTIONS
    );
    assert_eq!(
        size_of::<aldis_packet_extractor_t>(),
        ALDIS_PACKET_EXTRACTOR_WORDS * 8
    );
    assert_eq!(align_of::<aldis_frame_receiver_t>(), 8);
}

#[test]
fn the_extractor_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = pe_config(ALDIS_LINK_PROTOCOL_TM);
    let mut d = detail();
    let mut s = pe_storage();
    let mut status = aldis_packet_extractor_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_packet_extractor_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_packet_extractor_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_extractor_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_extractor_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_packet_extractor_status(null_mut(), &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_extractor_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_packet_extractor_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_packet_extractor_status_t::default());
        // A copy of live storage is foreign: the tag holds its own address.
        let mut copy = pe_storage();
        copy.words = s.words;
        assert_eq!(
            aldis_packet_extractor_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // Storage of another engine: the magic differs.
        let foreign = (&raw mut *s).cast::<aldis_frame_receiver_t>();
        let mut rx = aldis_frame_receiver_status_t::default();
        assert_eq!(
            aldis_frame_receiver_status(foreign, &raw mut rx, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // The status output must not overlap the storage.
        let inside = (&raw mut *s).cast::<aldis_packet_extractor_status_t>();
        assert_eq!(
            aldis_packet_extractor_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_packet_extractor_deinit(&raw mut *s);
        aldis_packet_extractor_deinit(&raw mut *s);
        aldis_packet_extractor_deinit(null_mut());
        assert_eq!(
            aldis_packet_extractor_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_extractor_protocol_is_chosen_in_the_configuration() {
    // req: LNK-001, CABI-040, LNK-062, API-040
    let mut s = pe_storage();
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    let init =
        |s: &mut aldis_packet_extractor_t,
         cfg: &aldis_packet_extractor_config_t,
         d: &mut aldis_error_detail_t| unsafe { aldis_packet_extractor_init(s, cfg, d) };
    for protocol in [
        ALDIS_LINK_PROTOCOL_TM,
        ALDIS_LINK_PROTOCOL_AOS,
        ALDIS_LINK_PROTOCOL_USLP,
    ] {
        assert_eq!(init(&mut s, &pe_config(protocol), &mut d), 0, "{protocol}");
    }
    let tc = pe_config(ALDIS_LINK_PROTOCOL_TC);
    assert_eq!(init(&mut s, &tc, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    let mut cfg = pe_config(9);
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    cfg = pe_config(ALDIS_LINK_PROTOCOL_AOS);
    cfg.channel.aos_edition = 7;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    cfg = pe_config(ALDIS_LINK_PROTOCOL_USLP);
    cfg.channel.uslp_upid = 32;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = pe_config(ALDIS_LINK_PROTOCOL_TM);
    cfg.space_packets = 2;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    cfg = pe_config(ALDIS_LINK_PROTOCOL_TM);
    cfg.space_packets = 0;
    cfg.encapsulation_packets = 0;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    cfg = pe_config(ALDIS_LINK_PROTOCOL_TM);
    cfg.max_packet_len = 0;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    cfg.max_packet_len = ALDIS_LINK_PACKET_QUEUE_OCTETS_FOR_TESTS + 1;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_MAX_PACKET_LEN);
    cfg = pe_config(ALDIS_LINK_PROTOCOL_TM);
    cfg.deliver_idle = 3;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    cfg.deliver_idle = 0;
    cfg.deliver_incomplete = 3;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
}

const ALDIS_LINK_PACKET_QUEUE_OCTETS_FOR_TESTS: usize =
    aldis_ffi::capacities::ALDIS_LINK_PACKET_QUEUE_OCTETS;

#[test]
fn the_extractor_steps_and_reconfigures() {
    // req: CABI-040, LNK-003, LNK-060, LNK-061, LNK-075, API-058
    let mut s = pe_storage();
    let mut d = detail();
    let mut actions = aldis_packet_extractor_actions_t::default();
    let mut out = [0_u8; 256];
    // SAFETY: every pointer is to a live local.
    assert_eq!(
        unsafe { aldis_packet_extractor_init(&mut *s, &pe_config(ALDIS_LINK_PROTOCOL_TM), &mut d) },
        0
    );
    // A TM data field with the pointer at its start: one packet.
    let field = packet(10);
    let mut input = pe_input(ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD);
    input.data = field.as_ptr();
    input.data_len = field.len();
    input.header_pointer_present = 1;
    assert_eq!(pe_step(&mut s, &input, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].kind, ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS);
    assert_eq!(
        (
            actions.items[0].packets.offset,
            actions.items[0].packets.len
        ),
        (0, 10)
    );
    assert_eq!(&out[..10], field.as_slice());
    let mut status = aldis_packet_extractor_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_packet_extractor_status(&mut *s, &mut status, &mut d) };
    assert_eq!((status.fields_in, status.packets_out), (1, 1));
    // A TM field without a pointer, a flag that is no flag, and a short area change nothing.
    input.header_pointer_present = 0;
    assert_eq!(
        pe_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    input.header_pointer_present = 2;
    assert_eq!(
        pe_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    input.header_pointer_present = 1;
    input.gap = 2;
    assert_eq!(
        pe_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    input.gap = 0;
    assert_eq!(
        pe_step(&mut s, &input, &mut out[..8], &mut actions, &mut d),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    // A data field above the build capacity.
    let big = vec![0_u8; aldis_ffi::capacities::ALDIS_LINK_MAX_FRAME_OCTETS + 1];
    input.data = big.as_ptr();
    input.data_len = big.len();
    assert_eq!(
        pe_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_CAPACITY_EXCEEDED
    );
    // An input kind that is none.
    assert_eq!(
        pe_step(&mut s, &pe_input(9), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // The data must not overlap the storage.
    input.data = (&raw const *s).cast::<u8>();
    input.data_len = 8;
    assert_eq!(
        pe_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // A reconfiguration of the same protocol, and of another one.
    let mut re = pe_input(ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE);
    assert_eq!(pe_step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 0);
    re.config = pe_config(ALDIS_LINK_PROTOCOL_AOS);
    assert_eq!(
        pe_step(&mut s, &re, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    re.config = pe_config(9);
    assert_eq!(
        pe_step(&mut s, &re, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
}

#[test]
fn the_extractor_serves_aos_and_uslp() {
    // req: LNK-001, LNK-003, LNK-060, CABI-040
    let mut d = detail();
    let mut actions = aldis_packet_extractor_actions_t::default();
    let mut out = [0_u8; 256];
    for protocol in [ALDIS_LINK_PROTOCOL_AOS, ALDIS_LINK_PROTOCOL_USLP] {
        let mut s = pe_storage();
        let cfg = pe_config(protocol);
        // SAFETY: every pointer is to a live local.
        assert_eq!(
            unsafe { aldis_packet_extractor_init(&mut *s, &cfg, &mut d) },
            0
        );
        let mut re = pe_input(ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE);
        re.config = cfg;
        assert_eq!(pe_step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
        re.config = pe_config(ALDIS_LINK_PROTOCOL_TM);
        assert_eq!(
            pe_step(&mut s, &re, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
        // A data field without its header is the codec's refusal of the engine.
        let mut input = pe_input(ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD);
        let field = [0_u8; 1];
        input.data = field.as_ptr();
        input.data_len = 1;
        assert_ne!(pe_step(&mut s, &input, &mut out, &mut actions, &mut d), 0);
    }
}

#[test]
fn the_receiver_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = fr_config(ALDIS_LINK_PROTOCOL_TM);
    let mut d = detail();
    let mut s = fr_storage();
    let mut status = aldis_frame_receiver_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_frame_receiver_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_frame_receiver_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_frame_receiver_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_frame_receiver_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_frame_receiver_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_frame_receiver_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_frame_receiver_status_t::default());
        let mut copy = fr_storage();
        copy.words = s.words;
        assert_eq!(
            aldis_frame_receiver_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        let inside = (&raw mut *s).cast::<aldis_frame_receiver_status_t>();
        assert_eq!(
            aldis_frame_receiver_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_frame_receiver_deinit(&raw mut *s);
        aldis_frame_receiver_deinit(&raw mut *s);
        assert_eq!(
            aldis_frame_receiver_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_receiver_configuration_is_checked() {
    // req: LNK-001, LNK-050, LNK-079, API-040, CABI-022
    let mut s = fr_storage();
    let mut d = detail();
    for protocol in [
        ALDIS_LINK_PROTOCOL_TM,
        ALDIS_LINK_PROTOCOL_TC,
        ALDIS_LINK_PROTOCOL_AOS,
        ALDIS_LINK_PROTOCOL_USLP,
    ] {
        assert_eq!(
            fr_init(&mut s, &fr_config(protocol), &mut d),
            0,
            "{protocol}"
        );
    }
    assert_eq!(
        fr_init(&mut s, &fr_config(9), &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // More VCs than the build holds, and the VC table cannot be addressed past its end.
    let mut c = fr_config(ALDIS_LINK_PROTOCOL_TC);
    c.vc_count = ALDIS_LINK_MAX_VIRTUAL_CHANNELS + 1;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_VC_ID);
    // No VC at all.
    c.vc_count = 0;
    assert_eq!(fr_init(&mut s, &c, &mut d), 0);
    // A VC twice, and a VCID above the protocol's.
    c = fr_config(ALDIS_LINK_PROTOCOL_TC);
    c.vc_count = 2;
    c.vcs[1] = c.vcs[0];
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_VC_ID);
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.vcs[0].vc_id = 8;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    // A frame that leaves no data field octet.
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.frame_len = 6;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_CONFIG);
    // Flags and constants that are none.
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.fecf = 2;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.raw_frames = 2;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.ocf_service = 3;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.vcs[0].content = 9;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    // TM: a B_PDU VC does not exist, and the Secondary Header has 2 to 64 octets.
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.vcs[0].content = ALDIS_LINK_CONTENT_BITSTREAM;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_DATA_FIELD_CONTENT);
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.vcs[0].fsh_len = 1;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_FSH_LEN);
    c.vcs[0].fsh_len = 2;
    c.vcs[0].ocf = 1;
    c.ocf_service = ALDIS_LINK_SERVICE_VC;
    c.fsh_service = ALDIS_LINK_SERVICE_MC;
    c.frame_len = 40;
    assert_eq!(fr_init(&mut s, &c, &mut d), 0);
    // AOS: an edition that is none is refused, at init and at a reconfiguration.
    c = fr_config(ALDIS_LINK_PROTOCOL_AOS);
    c.aos_edition = 7;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    c.aos_edition = 1;
    assert_eq!(fr_init(&mut s, &c, &mut d), 0);
    let mut re = fr_input(ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE);
    re.config = c;
    let mut actions = aldis_frame_receiver_actions_t::default();
    let mut out = [0_u8; 64];
    assert_eq!(fr_step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
    re.config.aos_edition = 7;
    assert_eq!(
        fr_step(&mut s, &re, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // Raw frames need an Insert Zone.
    c = fr_config(ALDIS_LINK_PROTOCOL_TM);
    c.raw_frames = 1;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_RAW_FRAMES);
    c = fr_config(ALDIS_LINK_PROTOCOL_AOS);
    c.raw_frames = 1;
    c.insert_zone_len = 3;
    c.aos_fhec = 1;
    c.vcs[0].content = ALDIS_LINK_CONTENT_BITSTREAM;
    c.vcs[0].ocf = 1;
    assert_eq!(fr_init(&mut s, &c, &mut d), 0);
    c.aos_fhec = 2;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    // USLP: variable length, a MAP mask, and a VC that is not served.
    c = fr_config(ALDIS_LINK_PROTOCOL_USLP);
    c.uslp_variable = 1;
    c.vcs[0].maps = 0x8001;
    c.vcs[0].cop = 1;
    assert_eq!(fr_init(&mut s, &c, &mut d), 0);
    c.vcs[0].cop = 2;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
    c.vcs[0].cop = 0;
    c.vcs[0].maps = 0;
    c.vcs[0].content = ALDIS_LINK_CONTENT_UNSERVED;
    c.uslp_variable = 2;
    assert_eq!(fr_init(&mut s, &c, &mut d), ALDIS_ERR_INVALID_FIELD);
}

#[test]
fn the_receiver_steps_and_reconfigures() {
    // req: CABI-040, LNK-003, LNK-050, LNK-053, LNK-079, LNK-080, API-058
    let mut s = fr_storage();
    let mut d = detail();
    let mut actions = aldis_frame_receiver_actions_t::default();
    let mut out = [0_u8; 64];
    assert_eq!(
        fr_init(&mut s, &fr_config(ALDIS_LINK_PROTOCOL_TM), &mut d),
        0
    );
    // Frame loss is a flag for every VC.
    assert_eq!(
        fr_step(
            &mut s,
            &fr_input(ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS),
            &mut out,
            &mut actions,
            &mut d
        ),
        0
    );
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].kind, ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG);
    // A caller's discard: the three reasons it may report, and the two it may not.
    for reason in [
        ALDIS_LINK_DISCARD_CRC_MISMATCH,
        ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE,
        ALDIS_LINK_DISCARD_SECURITY,
    ] {
        let mut input = fr_input(ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD);
        input.reason = reason;
        assert_eq!(fr_step(&mut s, &input, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.items[0].reason, reason);
        assert_eq!(actions.items[0].vc_present, 0);
    }
    for reason in [
        ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL,
        ALDIS_LINK_DISCARD_NOT_SERVED,
        99,
    ] {
        let mut input = fr_input(ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD);
        input.reason = reason;
        assert_eq!(
            fr_step(&mut s, &input, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // A frame the codec refuses (too short), and the same frame as a raw frame while the option
    // is off.
    let short = [0_u8; 4];
    let mut input = fr_input(ALDIS_FRAME_RECEIVER_INPUT_FRAME);
    input.data = short.as_ptr();
    input.data_len = short.len();
    assert_ne!(fr_step(&mut s, &input, &mut out, &mut actions, &mut d), 0);
    input.kind = ALDIS_FRAME_RECEIVER_INPUT_RAW_FRAME;
    assert_eq!(
        fr_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_RAW_FRAMES);
    input.kind = 99;
    assert_eq!(
        fr_step(&mut s, &input, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    let mut status = aldis_frame_receiver_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_frame_receiver_status(&mut *s, &mut status, &mut d) };
    assert_eq!(
        (status.frame_loss, status.crc_mismatch, status.refused),
        (1, 1, 1)
    );
    // A reconfiguration of the same protocol, and of another one.
    let mut re = fr_input(ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE);
    assert_eq!(fr_step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 0);
    re.config = fr_config(ALDIS_LINK_PROTOCOL_AOS);
    assert_eq!(
        fr_step(&mut s, &re, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    re.config = fr_config(9);
    assert_eq!(
        fr_step(&mut s, &re, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // The same for each other protocol.
    for protocol in [
        ALDIS_LINK_PROTOCOL_TC,
        ALDIS_LINK_PROTOCOL_AOS,
        ALDIS_LINK_PROTOCOL_USLP,
    ] {
        let mut s = fr_storage();
        assert_eq!(fr_init(&mut s, &fr_config(protocol), &mut d), 0);
        let mut re = fr_input(ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE);
        re.config = fr_config(protocol);
        assert_eq!(fr_step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
        re.config = fr_config(ALDIS_LINK_PROTOCOL_TM);
        assert_eq!(
            fr_step(&mut s, &re, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
    }
    // The area must be as long as the frame.
    let frame = packet(32);
    let mut input = fr_input(ALDIS_FRAME_RECEIVER_INPUT_FRAME);
    input.data = frame.as_ptr();
    input.data_len = frame.len();
    let mut small = [0_u8; 8];
    assert_ne!(fr_step(&mut s, &input, &mut small, &mut actions, &mut d), 0);
}

#[test]
fn every_protocol_refuses_a_bad_extractor_configuration_and_serves_loss_and_discard() {
    // req: LNK-001, CABI-040, CABI-022, LNK-003, LNK-053, API-058
    let mut d = detail();
    let mut out = [0_u8; 64];
    let mut actions = aldis_frame_receiver_actions_t::default();
    for protocol in [
        ALDIS_LINK_PROTOCOL_TM,
        ALDIS_LINK_PROTOCOL_AOS,
        ALDIS_LINK_PROTOCOL_USLP,
    ] {
        // A packet length of no octet is refused in the engine's own configuration, whatever
        // the protocol.
        let mut s = pe_storage();
        let mut cfg = pe_config(protocol);
        cfg.max_packet_len = 0;
        // SAFETY: every pointer is to a live local.
        let status = unsafe { aldis_packet_extractor_init(&mut *s, &cfg, &mut d) };
        assert_eq!(status, ALDIS_ERR_INVALID_CONFIG, "{protocol}");
    }
    for protocol in [
        ALDIS_LINK_PROTOCOL_TM,
        ALDIS_LINK_PROTOCOL_TC,
        ALDIS_LINK_PROTOCOL_AOS,
        ALDIS_LINK_PROTOCOL_USLP,
    ] {
        let mut s = fr_storage();
        let mut cfg = fr_config(protocol);
        assert_eq!(fr_init(&mut s, &cfg, &mut d), 0, "{protocol}");
        let mut loss = fr_input(ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS);
        loss.config = cfg;
        assert_eq!(fr_step(&mut s, &loss, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.items[0].kind, ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG);
        let mut discard = fr_input(ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD);
        discard.config = cfg;
        discard.reason = ALDIS_LINK_DISCARD_CRC_MISMATCH;
        assert_eq!(fr_step(&mut s, &discard, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.items[0].reason, ALDIS_LINK_DISCARD_CRC_MISMATCH);
        discard.reason = ALDIS_LINK_DISCARD_NOT_SERVED;
        assert_eq!(
            fr_step(&mut s, &discard, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        // A reconfiguration with a configuration the engine refuses changes nothing.
        cfg.frame_len = 0;
        let mut re = fr_input(ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE);
        re.config = cfg;
        assert_ne!(fr_step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
        assert_ne!(fr_init(&mut s, &cfg, &mut d), 0);
    }
}
