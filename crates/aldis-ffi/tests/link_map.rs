//! The contract of the `MapSegmenter` and `MapReassembler` handles, called as a C caller calls
//! them: storage and tags (CABI-031, CABI-032), argument checks (CABI-022), the protocol chosen in
//! the configuration (D4, LNK-001), the step call and the read-only calls (CABI-040), and the
//! capacities of the header (CABI-030). The engines' behaviour is checked by the scenarios of
//! `test-vectors/link/send/` and `test-vectors/link/recv/` (`vectors.rs`).
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::borrow_as_ptr,
    clippy::too_many_lines,
    clippy::large_stack_arrays,
    reason = "test-only harness: one long call sequence per contract, and the storage a C caller declares (RS-018)"
)]

use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::capacities::{
    ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_MAX_MAP_CHANNELS, ALDIS_LINK_PACKET_QUEUE_OCTETS,
};
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_BLOCKING, ALDIS_FIELD_DATA_FIELD_CONTENT, ALDIS_FIELD_FRAME_DATA_FIELD,
    ALDIS_FIELD_LINK_PROTOCOL, ALDIS_FIELD_MAP_ID, ALDIS_FIELD_MAX_FDU_LEN,
    ALDIS_FIELD_MAX_SDU_LEN, ALDIS_FIELD_SEGMENTATION, ALDIS_FIELD_UPID, ALDIS_FIELD_VCF_COUNT_LEN,
};
use aldis_ffi::link::*;
use aldis_ffi::link_map::*;

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

fn sg_storage() -> Box<aldis_map_segmenter_t> {
    Box::new(aldis_map_segmenter_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_MAP_SEGMENTER_WORDS],
    })
}

fn ra_storage() -> Box<aldis_map_reassembler_t> {
    Box::new(aldis_map_reassembler_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_MAP_REASSEMBLER_WORDS],
    })
}

/// A TC VC with a Segment Header and one MAP (1) that segments SDUs of up to 60 octets.
fn tc_sg() -> aldis_map_segmenter_config_t {
    let mut c = aldis_map_segmenter_config_t {
        channel: aldis_link_channel_t {
            protocol: ALDIS_LINK_PROTOCOL_TC,
            vc_id: 1,
            ..aldis_link_channel_t::default()
        },
        max_fdu_len: 16,
        tc_segment_header: 1,
        map_count: 1,
        ..aldis_map_segmenter_config_t::default()
    };
    c.maps[0] = aldis_map_segmenter_map_t {
        map_id: 1,
        service: ALDIS_MAP_SERVICE_ANY,
        segmentation: 1,
        max_sdu_len: 60,
        ..aldis_map_segmenter_map_t::default()
    };
    c
}

/// A USLP VC with a packet MAP (1) and a stream MAP (4, deadline 500 us).
fn uslp_sg() -> aldis_map_segmenter_config_t {
    let mut c = aldis_map_segmenter_config_t {
        channel: aldis_link_channel_t {
            protocol: ALDIS_LINK_PROTOCOL_USLP,
            vc_id: 2,
            ..aldis_link_channel_t::default()
        },
        max_fdu_len: 12,
        uslp_vcf_len_sc: 2,
        uslp_vcf_len_exp: 2,
        map_count: 2,
        ..aldis_map_segmenter_config_t::default()
    };
    c.maps[0] = aldis_map_segmenter_map_t {
        map_id: 1,
        service: ALDIS_MAP_SERVICE_PACKETS,
        segmentation: 1,
        max_sdu_len: 40,
        ..aldis_map_segmenter_map_t::default()
    };
    c.maps[1] = aldis_map_segmenter_map_t {
        map_id: 4,
        service: ALDIS_MAP_SERVICE_OCTET_STREAM,
        uslp_upid: 4,
        release_deadline_us: 500,
        ..aldis_map_segmenter_map_t::default()
    };
    c
}

fn sg_input(kind: u32) -> aldis_map_segmenter_input_t {
    aldis_map_segmenter_input_t {
        kind,
        data: null(),
        data_len: 0,
        map_id: 1,
        handle: 0,
        qos: ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
        config: tc_sg(),
    }
}

fn sg_init(
    s: &mut aldis_map_segmenter_t,
    cfg: &aldis_map_segmenter_config_t,
    d: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_init(s, cfg, d) }
}

fn sg_step(
    h: &mut aldis_map_segmenter_t,
    input: &aldis_map_segmenter_input_t,
    now_us: u64,
    out: &mut [u8],
    actions: &mut aldis_map_segmenter_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_step(h, input, now_us, out.as_mut_ptr(), out.len(), actions, err) }
}

/// Submits `data` as the kind `kind` for the MAP `map`.
fn sg_submit(h: &mut aldis_map_segmenter_t, kind: u32, map: u8, data: &[u8]) -> u32 {
    let mut input = sg_input(kind);
    input.map_id = map;
    input.data = data.as_ptr();
    input.data_len = data.len();
    let mut actions = aldis_map_segmenter_actions_t::default();
    let mut d = detail();
    sg_step(h, &input, 0, &mut [], &mut actions, &mut d)
}

#[test]
fn capacities_are_published_and_match_the_engines() {
    // req: CABI-030, CABI-031, LNK-074
    assert_eq!(
        ALDIS_MAP_SEGMENTER_MAX_ACTIONS,
        aldis::link::segmenter::MAP_SEGMENTER_MAX_ACTIONS
    );
    assert_eq!(
        ALDIS_MAP_REASSEMBLER_MAX_ACTIONS,
        aldis::link::reassembler::MAP_REASSEMBLER_MAX_ACTIONS
    );
    assert_eq!(
        size_of::<aldis_map_segmenter_t>(),
        ALDIS_MAP_SEGMENTER_WORDS * 8
    );
    assert_eq!(
        size_of::<aldis_map_reassembler_t>(),
        ALDIS_MAP_REASSEMBLER_WORDS * 8
    );
    assert_eq!(align_of::<aldis_map_segmenter_t>(), 8);
    assert_eq!(ALDIS_LINK_MAX_MAP_CHANNELS, 16);
}

#[test]
fn the_segmenter_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = tc_sg();
    let mut d = detail();
    let mut s = sg_storage();
    let mut status = aldis_map_segmenter_status_t::default();
    let (mut armed, mut at) = (9_u8, 9_u64);
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_map_segmenter_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_map_segmenter_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_map_segmenter_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_map_segmenter_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_map_segmenter_next_deadline(&raw mut *s, &raw mut armed, &raw mut at, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_map_segmenter_status(null_mut(), &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_map_segmenter_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_map_segmenter_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_map_segmenter_status_t::default());
        assert_eq!(
            aldis_map_segmenter_next_deadline(&raw mut *s, &raw mut armed, &raw mut at, null_mut()),
            0
        );
        assert_eq!((armed, at), (0, 0));
        // A copy of live storage is foreign: the tag holds its own address.
        let mut copy = sg_storage();
        copy.words = s.words;
        assert_eq!(
            aldis_map_segmenter_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // Storage of another engine: the magic differs.
        let foreign = (&raw mut *s).cast::<aldis_map_reassembler_t>();
        let mut rx = aldis_map_reassembler_status_t::default();
        assert_eq!(
            aldis_map_reassembler_status(foreign, &raw mut rx, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // The status output must not overlap the storage.
        let inside = (&raw mut *s).cast::<aldis_map_segmenter_status_t>();
        assert_eq!(
            aldis_map_segmenter_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_map_segmenter_deinit(&raw mut *s);
        aldis_map_segmenter_deinit(&raw mut *s);
        aldis_map_segmenter_deinit(null_mut());
        assert_eq!(
            aldis_map_segmenter_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_segmenter_protocol_is_chosen_in_the_configuration() {
    // req: LNK-001, CABI-040, LNK-040, LNK-078, API-040
    let mut s = sg_storage();
    let mut d = detail();
    assert_eq!(sg_init(&mut s, &tc_sg(), &mut d), 0);
    assert_eq!(sg_init(&mut s, &uslp_sg(), &mut d), 0);
    for protocol in [ALDIS_LINK_PROTOCOL_TM, ALDIS_LINK_PROTOCOL_AOS] {
        let mut cfg = tc_sg();
        cfg.channel.protocol = protocol;
        assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
        assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    }
    let mut cfg = tc_sg();
    cfg.channel.protocol = 9;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    // The flags and constants are read whatever the protocol.
    let sets: [fn(&mut aldis_map_segmenter_config_t); 6] = [
        |c| c.tc_segment_header = 2,
        |c| c.tc_fecf = 2,
        |c| c.tc_blocking = 2,
        |c| c.maps[0].segmentation = 2,
        |c| c.maps[0].blocking = 2,
        |c| c.maps[0].service = 9,
    ];
    for set in sets {
        for mut cfg in [tc_sg(), uslp_sg()] {
            set(&mut cfg);
            assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
        }
    }
    // Ranges of members of the other protocol.
    let mut cfg = tc_sg();
    cfg.uslp_vcf_len_sc = 8;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_VCF_COUNT_LEN);
    cfg = tc_sg();
    cfg.uslp_vcf_len_exp = 8;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    cfg = tc_sg();
    cfg.maps[0].uslp_upid = 32;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    assert_eq!(d.field, ALDIS_FIELD_UPID);
    // The protocol's own conversions.
    cfg = tc_sg();
    cfg.channel.vc_id = 64;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = tc_sg();
    cfg.maps[0].map_id = 64;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = uslp_sg();
    cfg.maps[0].map_id = 16;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = tc_sg();
    cfg.maps[0].service = ALDIS_MAP_SERVICE_PACKETS;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_DATA_FIELD_CONTENT);
    cfg = uslp_sg();
    cfg.maps[0].service = ALDIS_MAP_SERVICE_ANY;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_DATA_FIELD_CONTENT);
    cfg = uslp_sg();
    cfg.maps[0].uslp_upid = 5;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), 0);
    // The relations of the configuration.
    cfg = tc_sg();
    cfg.max_fdu_len = 0;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAX_FDU_LEN);
    cfg = tc_sg();
    cfg.map_count = 0;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    cfg = tc_sg();
    cfg.map_count = ALDIS_LINK_MAX_MAP_CHANNELS + 1;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    cfg = tc_sg();
    cfg.map_count = 2;
    cfg.maps[1] = cfg.maps[0];
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    cfg = tc_sg();
    cfg.maps[0].max_sdu_len = 0;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAX_SDU_LEN);
    cfg = tc_sg();
    cfg.maps[0].max_sdu_len = ALDIS_LINK_PACKET_QUEUE_OCTETS;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_MAX_SDU_LEN);
    cfg = uslp_sg();
    cfg.maps[1].blocking = 1;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_BLOCKING);
    cfg = uslp_sg();
    cfg.maps[1].segmentation = 1;
    cfg.maps[1].max_sdu_len = 10;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_SEGMENTATION);
    // A TC VC without a Segment Header has the implicit MAP 0 and takes no MAP.
    cfg = tc_sg();
    cfg.tc_segment_header = 0;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    cfg.map_count = 0;
    cfg.tc_blocking = 1;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), 0);
}

#[test]
fn the_segmenter_steps_and_reconfigures() {
    // req: CABI-040, LNK-003, LNK-040, LNK-041, LNK-043, LNK-075, API-058
    let mut s = sg_storage();
    let mut d = detail();
    let mut actions = aldis_map_segmenter_actions_t::default();
    let mut out = [0_u8; 16];
    assert_eq!(sg_init(&mut s, &tc_sg(), &mut d), 0);
    // Nothing queued: a request gets nothing.
    let mut next = sg_input(ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU);
    next.handle = 7;
    assert_eq!(sg_step(&mut s, &next, 0, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 0);
    // A 20-octet SDU is segmented into three FDUs of one header and at most 15 octets.
    let sdu: Vec<u8> = (0..20).collect();
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 1, &sdu),
        0
    );
    let mut status = aldis_map_segmenter_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_status(&mut *s, &mut status, &mut d) };
    assert_eq!((status.submitted, status.queue.current), (1, 20));
    let mut framings = Vec::new();
    let mut octets = Vec::new();
    for handle in 1..=2 {
        next.handle = handle;
        assert_eq!(sg_step(&mut s, &next, 0, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 1);
        let a = actions.items[0];
        assert_eq!(a.kind, ALDIS_MAP_SEGMENTER_ACTION_FDU);
        assert_eq!(
            (a.handle, a.vc_id, a.map_present, a.map_id, a.qos),
            (handle, 1, 1, 1, ALDIS_LINK_QOS_SEQUENCE_CONTROLLED)
        );
        framings.push(a.framing);
        octets.extend_from_slice(&out[a.fdu.offset + 1..a.fdu.offset + a.fdu.len]);
    }
    assert_eq!(framings, [ALDIS_MAP_FRAMING_FIRST, ALDIS_MAP_FRAMING_LAST]);
    assert_eq!(octets, sdu);
    // The expedited service of a request is the request's.
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 1, &[1, 2]),
        0
    );
    next.qos = ALDIS_LINK_QOS_EXPEDITED;
    assert_eq!(sg_step(&mut s, &next, 0, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.items[0].qos, ALDIS_LINK_QOS_EXPEDITED);
    assert_eq!(actions.items[0].framing, ALDIS_MAP_FRAMING_WHOLE);
    // Refusals leave the engine unchanged and the actions unwritten.
    actions = aldis_map_segmenter_actions_t {
        count: 99,
        ..aldis_map_segmenter_actions_t::default()
    };
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 5, &[1]),
        ALDIS_ERR_UNKNOWN_CHANNEL
    );
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 64, &[1]),
        ALDIS_ERR_OUT_OF_RANGE
    );
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 1, &[]),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS, 1, &[1]),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        sg_submit(
            &mut s,
            ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_PACKET,
            1,
            &[0x20, 0, 0]
        ),
        ALDIS_ERR_INVALID_FIELD
    );
    let mut bad = sg_input(99);
    assert_eq!(
        sg_step(&mut s, &bad, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    bad = next;
    bad.qos = 5;
    assert_eq!(
        sg_step(&mut s, &bad, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(actions.count, 99);
    // An area shorter than the FDU: `BufferTooSmall`, even though a request is pending.
    assert_eq!(
        sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 1, &[1, 2]),
        0
    );
    assert_eq!(
        sg_step(&mut s, &next, 0, &mut out[..8], &mut actions, &mut d),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    // The input data must not overlap the storage or `out`.
    let mut overlap = sg_input(ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU);
    overlap.data = (&raw const *s).cast::<u8>();
    overlap.data_len = 8;
    assert_eq!(
        sg_step(&mut s, &overlap, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    overlap.data = out.as_ptr();
    assert_eq!(
        sg_step(&mut s, &overlap, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // A reconfiguration keeps the protocol, discards the queue, and is refused when invalid.
    let mut reconf = sg_input(ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE);
    reconf.config = uslp_sg();
    assert_eq!(
        sg_step(&mut s, &reconf, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    reconf.config = tc_sg();
    reconf.config.max_fdu_len = 0;
    assert_eq!(
        sg_step(&mut s, &reconf, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    reconf.config = tc_sg();
    reconf.config.channel.protocol = 9;
    assert_eq!(
        sg_step(&mut s, &reconf, 0, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    reconf.config = tc_sg();
    assert_eq!(
        sg_step(&mut s, &reconf, 0, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].kind, ALDIS_MAP_SEGMENTER_ACTION_DISCARDED);
    assert_eq!(actions.items[0].octets, 2);
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_status(&mut *s, &mut status, &mut d) };
    assert_eq!(status.discarded_octets, 2);
    assert!(status.refused > 0);
    assert_eq!(status.queue.current, 0);
}

#[test]
fn the_segmenter_queue_refuses_what_does_not_fit_whole() {
    // req: LNK-015, CABI-030, API-052
    let mut s = sg_storage();
    let mut d = detail();
    let mut cfg = tc_sg();
    cfg.maps[0].segmentation = 0;
    assert_eq!(sg_init(&mut s, &cfg, &mut d), 0);
    let sdu = [0x55_u8; 15];
    let mut taken = 0;
    // bound: each SDU takes 19 octets of the queue, so it is full after a fixed number.
    let refused = loop {
        let status = sg_submit(&mut s, ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_SDU, 1, &sdu);
        if status != 0 {
            break status;
        }
        taken += 1;
    };
    assert_eq!(refused, ALDIS_ERR_WOULD_BLOCK);
    assert_eq!(taken, ALDIS_LINK_PACKET_QUEUE_OCTETS / 19);
    let mut status = aldis_map_segmenter_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_status(&mut *s, &mut status, &mut d) };
    assert_eq!(status.would_block, 1);
    assert_eq!(status.queue.current, taken * 15);
}

#[test]
fn the_segmenter_serves_a_stream_on_uslp_with_a_deadline() {
    // req: LNK-012, LNK-042, API-053, CABI-040
    let mut s = sg_storage();
    let mut d = detail();
    let mut actions = aldis_map_segmenter_actions_t::default();
    let mut out = [0_u8; 12];
    assert_eq!(sg_init(&mut s, &uslp_sg(), &mut d), 0);
    let mut submit = sg_input(ALDIS_MAP_SEGMENTER_INPUT_SUBMIT_OCTETS);
    submit.map_id = 4;
    let data = [1_u8, 2, 3];
    submit.data = data.as_ptr();
    submit.data_len = data.len();
    assert_eq!(
        sg_step(&mut s, &submit, 100, &mut out, &mut actions, &mut d),
        0
    );
    let (mut armed, mut at) = (0_u8, 0_u64);
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_next_deadline(&mut *s, &mut armed, &mut at, &mut d) };
    assert_eq!((armed, at), (1, 600));
    // The TFDZ is not releasable before `TIMEOUT` says the deadline passed.
    let mut next = sg_input(ALDIS_MAP_SEGMENTER_INPUT_NEXT_FDU);
    next.handle = 1;
    assert_eq!(
        sg_step(&mut s, &next, 600, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 0);
    let timeout = sg_input(ALDIS_MAP_SEGMENTER_INPUT_TIMEOUT);
    assert_eq!(
        sg_step(&mut s, &timeout, 600, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(
        sg_step(&mut s, &next, 600, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    let a = actions.items[0];
    assert_eq!((a.map_id, a.framing), (4, ALDIS_MAP_FRAMING_STREAM));
    assert_eq!(
        &out[a.fdu.offset..a.fdu.offset + a.fdu.len],
        [0x64, 1, 2, 3]
    );
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_segmenter_next_deadline(&mut *s, &mut armed, &mut at, &mut d) };
    assert_eq!((armed, at), (0, 0));
}

// ---- MapReassembler

fn tc_ra() -> aldis_map_reassembler_config_t {
    let mut c = aldis_map_reassembler_config_t {
        channel: aldis_link_channel_t {
            protocol: ALDIS_LINK_PROTOCOL_TC,
            ..aldis_link_channel_t::default()
        },
        tc_segment_header: 1,
        map_count: 2,
        ..aldis_map_reassembler_config_t::default()
    };
    c.maps[0] = aldis_map_reassembler_map_t {
        vc_id: 1,
        map_id: 0,
        service: ALDIS_MAP_SERVICE_PACKETS,
        max_sdu_len: 64,
    };
    c.maps[1] = aldis_map_reassembler_map_t {
        vc_id: 1,
        map_id: 1,
        service: ALDIS_MAP_SERVICE_SDU,
        max_sdu_len: 10,
    };
    c
}

fn uslp_ra() -> aldis_map_reassembler_config_t {
    let mut c = aldis_map_reassembler_config_t {
        channel: aldis_link_channel_t {
            protocol: ALDIS_LINK_PROTOCOL_USLP,
            ..aldis_link_channel_t::default()
        },
        map_count: 2,
        ..aldis_map_reassembler_config_t::default()
    };
    c.maps[0] = aldis_map_reassembler_map_t {
        vc_id: 1,
        map_id: 0,
        service: ALDIS_MAP_SERVICE_SDU,
        max_sdu_len: 64,
    };
    c.maps[1] = aldis_map_reassembler_map_t {
        vc_id: 1,
        map_id: 1,
        service: ALDIS_MAP_SERVICE_OCTET_STREAM,
        max_sdu_len: 0,
    };
    c
}

fn ra_input(kind: u32) -> aldis_map_reassembler_input_t {
    aldis_map_reassembler_input_t {
        kind,
        data: null(),
        data_len: 0,
        vc_id: 1,
        map_id: 0,
        gap: 0,
        config: tc_ra(),
    }
}

fn ra_init(
    s: &mut aldis_map_reassembler_t,
    cfg: &aldis_map_reassembler_config_t,
    d: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_reassembler_init(s, cfg, d) }
}

fn ra_step(
    h: &mut aldis_map_reassembler_t,
    input: &aldis_map_reassembler_input_t,
    out: &mut [u8],
    actions: &mut aldis_map_reassembler_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_reassembler_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

/// An FDU `data` of the VC 1 and MAP `map`.
fn fdu(data: &[u8], map: u8, gap: u8) -> aldis_map_reassembler_input_t {
    let mut input = ra_input(ALDIS_MAP_REASSEMBLER_INPUT_FDU);
    input.data = data.as_ptr();
    input.data_len = data.len();
    input.map_id = map;
    input.gap = gap;
    input
}

#[test]
fn the_reassembler_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = tc_ra();
    let mut d = detail();
    let mut s = ra_storage();
    let mut status = aldis_map_reassembler_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_map_reassembler_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_map_reassembler_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_map_reassembler_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_map_reassembler_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_map_reassembler_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_map_reassembler_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_map_reassembler_status_t::default());
        let mut copy = ra_storage();
        copy.words = s.words;
        assert_eq!(
            aldis_map_reassembler_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        let inside = (&raw mut *s).cast::<aldis_map_reassembler_status_t>();
        assert_eq!(
            aldis_map_reassembler_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_map_reassembler_deinit(&raw mut *s);
        aldis_map_reassembler_deinit(null_mut());
        assert_eq!(
            aldis_map_reassembler_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_reassembler_configuration_is_checked() {
    // req: LNK-001, CABI-040, LNK-070, LNK-073, API-040
    let mut s = ra_storage();
    let mut d = detail();
    assert_eq!(ra_init(&mut s, &tc_ra(), &mut d), 0);
    assert_eq!(ra_init(&mut s, &uslp_ra(), &mut d), 0);
    for protocol in [ALDIS_LINK_PROTOCOL_TM, ALDIS_LINK_PROTOCOL_AOS] {
        let mut cfg = tc_ra();
        cfg.channel.protocol = protocol;
        assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
        assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    }
    let mut cfg = tc_ra();
    cfg.channel.protocol = 9;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    for mut cfg in [tc_ra(), uslp_ra()] {
        cfg.tc_segment_header = 2;
        assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    }
    cfg = tc_ra();
    cfg.maps[0].service = 9;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_FIELD);
    cfg = tc_ra();
    cfg.maps[0].vc_id = 64;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = tc_ra();
    cfg.maps[0].map_id = 64;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = uslp_ra();
    cfg.maps[0].map_id = 16;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_OUT_OF_RANGE);
    cfg = tc_ra();
    cfg.maps[0].service = ALDIS_MAP_SERVICE_ANY;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_DATA_FIELD_CONTENT);
    cfg = tc_ra();
    cfg.map_count = 0;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    cfg = tc_ra();
    cfg.map_count = ALDIS_LINK_MAX_MAP_CHANNELS + 1;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    cfg = tc_ra();
    cfg.maps[1] = cfg.maps[0];
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    cfg = tc_ra();
    cfg.maps[0].max_sdu_len = 0;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAX_SDU_LEN);
    cfg.maps[0].max_sdu_len = ALDIS_LINK_MAX_FRAME_OCTETS + 1;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_MAX_SDU_LEN);
    cfg.maps[0].max_sdu_len = ALDIS_LINK_MAX_FRAME_OCTETS;
    assert_eq!(ra_init(&mut s, &cfg, &mut d), 0);
}

#[test]
fn the_reassembler_steps_and_reconfigures_on_tc() {
    // req: CABI-040, LNK-003, LNK-070, LNK-073, LNK-075, API-058
    let mut s = ra_storage();
    let mut d = detail();
    let mut actions = aldis_map_reassembler_actions_t::default();
    let mut out = [0_u8; 128];
    assert_eq!(ra_init(&mut s, &tc_ra(), &mut d), 0);
    // An SDU in two segments (flags 01 and 10) of the MAP 1 is delivered at the last one.
    let first = [0x41, 1, 2, 3];
    assert_eq!(
        ra_step(&mut s, &fdu(&first, 0, 0), &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 0);
    let mut status = aldis_map_reassembler_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_reassembler_status(&mut *s, &mut status, &mut d) };
    assert_eq!(status.assembly.current, 3);
    let last = [0x81, 4, 5];
    assert_eq!(
        ra_step(&mut s, &fdu(&last, 0, 0), &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    let a = actions.items[0];
    assert_eq!(
        (a.kind, a.vc_id, a.map_id),
        (ALDIS_MAP_REASSEMBLER_ACTION_SDU, 1, 1)
    );
    assert_eq!(
        &out[a.data.offset..a.data.offset + a.data.len],
        [1, 2, 3, 4, 5]
    );
    // A run of two packets on the packet MAP 0 (flags 11).
    let packets = [
        0xC0, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0xAA, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0xBB,
    ];
    assert_eq!(
        ra_step(&mut s, &fdu(&packets, 0, 0), &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    let a = actions.items[0];
    assert_eq!(a.kind, ALDIS_MAP_REASSEMBLER_ACTION_PACKETS);
    assert_eq!((a.count, a.data.len), (2, 14));
    // An FDU of a MAP that is not configured is an event.
    let unknown = [0xC5, 1];
    assert_eq!(
        ra_step(&mut s, &fdu(&unknown, 0, 0), &mut out, &mut actions, &mut d),
        0
    );
    let a = actions.items[0];
    assert_eq!(
        (a.kind, a.event, a.map_id),
        (
            ALDIS_MAP_REASSEMBLER_ACTION_EVENT,
            ALDIS_MAP_REASSEMBLY_EVENT_UNKNOWN_MAP,
            5
        )
    );
    // A continuing segment without a first one is a sequence violation; one too long is
    // discarded.
    let orphan = [0x01, 1];
    ra_step(&mut s, &fdu(&orphan, 0, 0), &mut out, &mut actions, &mut d);
    assert_eq!(
        actions.items[0].event,
        ALDIS_MAP_REASSEMBLY_EVENT_SEQUENCE_VIOLATION
    );
    let long = [0x41, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    ra_step(&mut s, &fdu(&long, 0, 0), &mut out, &mut actions, &mut d);
    assert_eq!(
        actions.items[0].event,
        ALDIS_MAP_REASSEMBLY_EVENT_SDU_TOO_LONG
    );
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_map_reassembler_status(&mut *s, &mut status, &mut d) };
    assert_eq!(
        (
            status.fdus_in,
            status.delivered,
            status.unknown_maps,
            status.sequence_violations,
            status.sdus_too_long
        ),
        (6, 3, 1, 1, 1)
    );
    // Refusals leave the engine unchanged and the actions unwritten.
    actions = aldis_map_reassembler_actions_t {
        count: 99,
        ..aldis_map_reassembler_actions_t::default()
    };
    assert_eq!(
        ra_step(&mut s, &fdu(&[], 0, 0), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_FRAME_DATA_FIELD);
    assert_eq!(
        ra_step(&mut s, &fdu(&first, 0, 2), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    let mut bad = fdu(&first, 0, 0);
    bad.vc_id = 64;
    assert_eq!(
        ra_step(&mut s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_OUT_OF_RANGE
    );
    assert_eq!(
        ra_step(&mut s, &ra_input(99), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        ra_step(
            &mut s,
            &fdu(&first, 0, 0),
            &mut out[..2],
            &mut actions,
            &mut d
        ),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert_eq!(actions.count, 99);
    // The input data must not overlap the storage or `out`.
    let mut overlap = fdu(&first, 0, 0);
    overlap.data = (&raw const *s).cast::<u8>();
    assert_eq!(
        ra_step(&mut s, &overlap, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // A reconfiguration keeps the protocol and discards the SDUs in progress.
    ra_step(&mut s, &fdu(&first, 0, 0), &mut out, &mut actions, &mut d);
    let mut reconf = ra_input(ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE);
    reconf.config = uslp_ra();
    assert_eq!(
        ra_step(&mut s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    reconf.config = tc_ra();
    reconf.config.map_count = 0;
    assert_eq!(
        ra_step(&mut s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    reconf.config = tc_ra();
    assert_eq!(ra_step(&mut s, &reconf, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(
        actions.items[0].kind,
        ALDIS_MAP_REASSEMBLER_ACTION_DISCARDED
    );
    assert_eq!(actions.items[0].octets, 3);
}

#[test]
fn the_reassembler_serves_uslp_streams_and_gaps() {
    // req: CABI-040, LNK-051, LNK-070, LNK-072
    let mut s = ra_storage();
    let mut d = detail();
    let mut actions = aldis_map_reassembler_actions_t::default();
    let mut out = [0_u8; 128];
    assert_eq!(ra_init(&mut s, &uslp_ra(), &mut d), 0);
    // A whole SDU (rule 111, UPID 5) of the MAP 0.
    let whole = [0xE5, 0x20, 0x21];
    assert_eq!(
        ra_step(&mut s, &fdu(&whole, 0, 0), &mut out, &mut actions, &mut d),
        0
    );
    let a = actions.items[0];
    assert_eq!(a.kind, ALDIS_MAP_REASSEMBLER_ACTION_SDU);
    assert_eq!(
        &out[a.data.offset..a.data.offset + a.data.len],
        [0x20, 0x21]
    );
    // Octets of the stream MAP 1 (rule 011, UPID 4), the first after a gap.
    let stream = [0x64, 7, 8];
    assert_eq!(
        ra_step(&mut s, &fdu(&stream, 1, 1), &mut out, &mut actions, &mut d),
        0
    );
    let a = actions.items[0];
    assert_eq!(a.kind, ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA);
    assert_eq!((a.after_gap, a.map_id), (1, 1));
    assert_eq!(&out[a.data.offset..a.data.offset + a.data.len], [7, 8]);
    // A gap discards the SDUs in progress.
    let first = [0x85, 1, 2, 3];
    ra_step(&mut s, &fdu(&first, 0, 0), &mut out, &mut actions, &mut d);
    let next = [0xA5, 4];
    ra_step(&mut s, &fdu(&next, 0, 1), &mut out, &mut actions, &mut d);
    assert_eq!(
        actions.items[0].event,
        ALDIS_MAP_REASSEMBLY_EVENT_GAP_DISCARD
    );
    let mut bad = fdu(&whole, 16, 0);
    assert_eq!(
        ra_step(&mut s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_OUT_OF_RANGE
    );
    bad = fdu(&whole, 0, 3);
    assert_eq!(
        ra_step(&mut s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
}

#[test]
fn the_members_of_the_channel_are_read_whatever_the_protocol() {
    // req: CABI-040, CABI-022, LNK-001
    type Set = fn(&mut aldis_link_channel_t);
    let mut s = sg_storage();
    let mut r = ra_storage();
    let mut d = detail();
    let sets: [(Set, u32); 4] = [
        (|c| c.uslp_qos = 99, ALDIS_ERR_INVALID_FIELD),
        (|c| c.aos_edition = 9, ALDIS_ERR_INVALID_FIELD),
        (|c| c.uslp_upid = 32, ALDIS_ERR_OUT_OF_RANGE),
        (|c| c.uslp_map = 16, ALDIS_ERR_OUT_OF_RANGE),
    ];
    for (set, want) in sets {
        for mut cfg in [tc_sg(), uslp_sg()] {
            set(&mut cfg.channel);
            assert_eq!(sg_init(&mut s, &cfg, &mut d), want);
        }
        for mut cfg in [tc_ra(), uslp_ra()] {
            set(&mut cfg.channel);
            assert_eq!(ra_init(&mut r, &cfg, &mut d), want);
        }
        // A reconfiguration reads them too.
        assert_eq!(sg_init(&mut s, &tc_sg(), &mut d), 0);
        let mut reconf = sg_input(ALDIS_MAP_SEGMENTER_INPUT_RECONFIGURE);
        set(&mut reconf.config.channel);
        let mut actions = aldis_map_segmenter_actions_t::default();
        assert_eq!(
            sg_step(&mut s, &reconf, 0, &mut [], &mut actions, &mut d),
            want
        );
    }
}
