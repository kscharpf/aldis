//! The contract of the generation engine handles, called as a C caller calls them: storage and
//! tags (CABI-031, CABI-032), argument checks (CABI-022), the protocol chosen in the
//! configuration (D4, LNK-001), the step call and the read-only calls (CABI-040), and the
//! capacities of the header (CABI-030). The engines' behaviour is checked by the scenarios of
//! `test-vectors/link/send/` (`vectors.rs`).
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::too_many_lines,
    clippy::large_stack_arrays,
    clippy::unnecessary_box_returns,
    reason = "test-only harness: one long call sequence per contract, and the storage a C caller declares (RS-018)"
)]

use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::capacities::{ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_PACKET_QUEUE_OCTETS};
use aldis_ffi::error::*;
use aldis_ffi::field::{ALDIS_FIELD_LINK_PROTOCOL, ALDIS_FIELD_VC_ID};
use aldis_ffi::link::*;
use aldis_ffi::link_gen::*;

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

fn pg_storage() -> Box<aldis_packet_generator_t> {
    Box::new(aldis_packet_generator_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_PACKET_GENERATOR_WORDS],
    })
}

fn ag_storage() -> Box<aldis_access_generator_t> {
    Box::new(aldis_access_generator_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_ACCESS_GENERATOR_WORDS],
    })
}

fn bg_storage() -> Box<aldis_bitstream_generator_t> {
    Box::new(aldis_bitstream_generator_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_BITSTREAM_GENERATOR_WORDS],
    })
}

const fn channel(protocol: u32, vc_id: u8) -> aldis_link_channel_t {
    aldis_link_channel_t {
        protocol,
        vc_id,
        aos_edition: 0,
        uslp_upid: 0,
        uslp_map: 1,
        uslp_qos: ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
    }
}

const fn pg_config(protocol: u32, field_len: usize) -> aldis_packet_generator_config_t {
    aldis_packet_generator_config_t {
        channel: channel(protocol, 1),
        field_len,
        space_packets: 1,
        encapsulation_packets: 1,
        max_packet_len: 64,
        idle_policy: ALDIS_LINK_IDLE_POLICY_OID,
        idle_packet_kind: ALDIS_LINK_IDLE_PACKET_ENCAPSULATION,
        idle_pattern: 0,
        release_deadline_us: 0,
    }
}

const fn pg_input(kind: u32) -> aldis_packet_generator_input_t {
    aldis_packet_generator_input_t {
        kind,
        data: null(),
        data_len: 0,
        config: pg_config(ALDIS_LINK_PROTOCOL_TM, 16),
    }
}

/// A Space Packet of `total` octets (133.0 §4.1.3.2).
fn packet(total: usize) -> Vec<u8> {
    let mut p = vec![0x00, 0x05, 0xC0, 0x00];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, 0xAB);
    p
}

fn pg_step(
    h: &mut aldis_packet_generator_t,
    input: &aldis_packet_generator_input_t,
    out: &mut [u8],
    actions: &mut aldis_packet_generator_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_packet_generator_step(h, input, 10, out.as_mut_ptr(), out.len(), actions, err) }
}

#[test]
fn capacities_are_published_and_match_the_engines() {
    // req: CABI-030, CABI-031, LNK-074
    assert_eq!(
        ALDIS_PACKET_GENERATOR_MAX_ACTIONS,
        aldis::link::generator::PACKET_GENERATOR_MAX_ACTIONS
    );
    assert_eq!(
        ALDIS_ACCESS_GENERATOR_MAX_ACTIONS,
        aldis::link::access::ACCESS_GENERATOR_MAX_ACTIONS
    );
    assert_eq!(
        ALDIS_BITSTREAM_GENERATOR_MAX_ACTIONS,
        aldis::link::bitstream::BITSTREAM_GENERATOR_MAX_ACTIONS
    );
    assert_eq!(ALDIS_LINK_MAX_FRAME_OCTETS, 1024);
    assert_eq!(ALDIS_LINK_PACKET_QUEUE_OCTETS, 2048);
    assert_eq!(
        size_of::<aldis_packet_generator_t>(),
        ALDIS_PACKET_GENERATOR_WORDS * 8
    );
    assert_eq!(align_of::<aldis_access_generator_t>(), 8);
}

#[test]
fn storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = pg_config(ALDIS_LINK_PROTOCOL_TM, 16);
    let mut d = detail();
    let mut s = pg_storage();
    let mut status = aldis_packet_generator_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_packet_generator_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_packet_generator_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_generator_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_generator_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_packet_generator_status(null_mut(), &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_packet_generator_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_packet_generator_status_t::default());
        // A copy of live storage is foreign: the tag holds its own address.
        let mut copy = pg_storage();
        copy.words = s.words;
        assert_eq!(
            aldis_packet_generator_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // Storage of another engine: the magic differs.
        let foreign = (&raw mut *s).cast::<aldis_access_generator_t>();
        let mut access = aldis_access_generator_status_t::default();
        assert_eq!(
            aldis_access_generator_status(foreign, &raw mut access, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // The status output must not overlap the storage.
        let inside = (&raw mut *s).cast::<aldis_packet_generator_status_t>();
        assert_eq!(
            aldis_packet_generator_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_packet_generator_deinit(&raw mut *s);
        aldis_packet_generator_deinit(&raw mut *s);
        aldis_packet_generator_deinit(null_mut());
        assert_eq!(
            aldis_packet_generator_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_protocol_is_chosen_in_the_configuration() {
    // req: LNK-001, CABI-040, LNK-078, API-040
    let mut s = pg_storage();
    let mut d = detail();
    let uslp = |f: fn(&mut aldis_packet_generator_config_t)| {
        let mut c = pg_config(ALDIS_LINK_PROTOCOL_USLP, 27);
        f(&mut c);
        c
    };
    // SAFETY: every pointer is to a live local.
    unsafe {
        for (cfg, kind, field) in [
            (
                pg_config(ALDIS_LINK_PROTOCOL_TC, 16),
                ALDIS_ERR_INVALID_CONFIG,
                ALDIS_FIELD_LINK_PROTOCOL,
            ),
            (pg_config(9, 16), ALDIS_ERR_INVALID_FIELD, 0),
            (
                {
                    let mut c = pg_config(ALDIS_LINK_PROTOCOL_TM, 16);
                    c.channel.vc_id = 8;
                    c
                },
                ALDIS_ERR_OUT_OF_RANGE,
                ALDIS_FIELD_VC_ID,
            ),
            (
                {
                    let mut c = pg_config(ALDIS_LINK_PROTOCOL_TM, 16);
                    c.space_packets = 2;
                    c
                },
                ALDIS_ERR_INVALID_FIELD,
                0,
            ),
            (
                uslp(|c| c.channel.uslp_upid = 32),
                ALDIS_ERR_OUT_OF_RANGE,
                0,
            ),
            (
                uslp(|c| c.channel.uslp_upid = 31),
                ALDIS_ERR_INVALID_CONFIG,
                0,
            ),
            (uslp(|c| c.channel.uslp_map = 16), ALDIS_ERR_OUT_OF_RANGE, 0),
            (uslp(|c| c.channel.uslp_qos = 2), ALDIS_ERR_INVALID_FIELD, 0),
            (
                {
                    let mut c = pg_config(ALDIS_LINK_PROTOCOL_AOS, 16);
                    c.channel.aos_edition = 2;
                    c
                },
                ALDIS_ERR_INVALID_FIELD,
                0,
            ),
            (uslp(|c| c.idle_policy = 2), ALDIS_ERR_INVALID_FIELD, 0),
            (uslp(|c| c.idle_packet_kind = 2), ALDIS_ERR_INVALID_FIELD, 0),
            (uslp(|c| c.max_packet_len = 0), ALDIS_ERR_INVALID_CONFIG, 0),
            (
                uslp(|c| c.field_len = ALDIS_LINK_MAX_FRAME_OCTETS + 1),
                ALDIS_ERR_CAPACITY_EXCEEDED,
                0,
            ),
        ] {
            d = detail();
            let status = aldis_packet_generator_init(&raw mut *s, &raw const cfg, &raw mut d);
            assert_eq!((status, d.kind), (kind, kind), "{cfg:?}");
            if field != 0 {
                assert_eq!(d.field, field, "{cfg:?}");
            }
        }
        // Every protocol the engine serves, and AOS in both editions.
        for cfg in [
            pg_config(ALDIS_LINK_PROTOCOL_TM, 16),
            pg_config(ALDIS_LINK_PROTOCOL_AOS, 16),
            {
                let mut c = pg_config(ALDIS_LINK_PROTOCOL_AOS, 16);
                c.channel.aos_edition = 1;
                c
            },
            pg_config(ALDIS_LINK_PROTOCOL_USLP, 27),
        ] {
            assert_eq!(
                aldis_packet_generator_init(&raw mut *s, &raw const cfg, &raw mut d),
                0,
                "{cfg:?}"
            );
        }
    }
}

#[test]
fn a_packet_goes_in_and_a_field_comes_out_for_every_protocol() {
    // req: LNK-010, LNK-011, LNK-012, CABI-040, API-061
    for (protocol, field_len, pointer, route) in [
        (ALDIS_LINK_PROTOCOL_TM, 16, 0, 0),
        (ALDIS_LINK_PROTOCOL_AOS, 18, 0, 0),
        (ALDIS_LINK_PROTOCOL_USLP, 19, 0, 1),
    ] {
        let mut s = pg_storage();
        let cfg = pg_config(protocol, field_len);
        let mut d = detail();
        let mut out = [0_u8; 64];
        let mut actions = aldis_packet_generator_actions_t::default();
        let pkt = packet(16);
        let submit = aldis_packet_generator_input_t {
            kind: ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET,
            data: pkt.as_ptr(),
            data_len: pkt.len(),
            ..pg_input(0)
        };
        let release = pg_input(ALDIS_PACKET_GENERATOR_INPUT_RELEASE);
        // SAFETY: every pointer is to a live local.
        unsafe {
            assert_eq!(
                aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
                0
            );
        }
        assert_eq!(pg_step(&mut s, &submit, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 0);
        assert_eq!(pg_step(&mut s, &release, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 1);
        let a = actions.items[0];
        assert_eq!(a.kind, ALDIS_PACKET_GENERATOR_ACTION_DATA_FIELD);
        assert_eq!((a.vc_id, a.field.offset, a.field.len), (1, 0, field_len));
        assert_eq!(a.pointer, pointer, "{protocol}");
        assert_eq!((a.route_present, a.map_id), (route, route));
        // The packet is in the field, after its header.
        assert!(out[..field_len].windows(16).any(|w| w == pkt.as_slice()));
        let mut status = aldis_packet_generator_status_t::default();
        // SAFETY: every pointer is to a live local.
        unsafe {
            assert_eq!(
                aldis_packet_generator_status(&raw mut *s, &raw mut status, null_mut()),
                0
            );
        }
        assert_eq!((status.packets_in, status.fields_out), (1, 1));
        assert_eq!(status.queue.current, 0);
    }
}

#[test]
fn a_failed_step_changes_nothing_and_writes_no_actions() {
    // req: API-058, CABI-040, LNK-010, LNK-015, LNK-075
    let mut s = pg_storage();
    let cfg = pg_config(ALDIS_LINK_PROTOCOL_TM, 16);
    let mut d = detail();
    let mut out = [0_u8; 64];
    let mut actions = aldis_packet_generator_actions_t {
        count: 77,
        ..Default::default()
    };
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
    }
    let bad = [0x20_u8, 5, 0xC0, 0, 0, 0, 0];
    let submit = |data: &[u8]| aldis_packet_generator_input_t {
        kind: ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET,
        data: data.as_ptr(),
        data_len: data.len(),
        ..pg_input(0)
    };
    assert_eq!(
        pg_step(&mut s, &submit(&bad), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(actions.count, 77, "a failed call writes no actions");
    let long = packet(65);
    assert_eq!(
        pg_step(&mut s, &submit(&long), &mut out, &mut actions, &mut d),
        ALDIS_ERR_OUT_OF_RANGE
    );
    let short = packet(16);
    assert_eq!(
        pg_step(
            &mut s,
            &submit(&short[..15]),
            &mut out,
            &mut actions,
            &mut d
        ),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    // A release into an area shorter than the field.
    assert_eq!(
        pg_step(&mut s, &submit(&short), &mut out, &mut actions, &mut d),
        0
    );
    let release = pg_input(ALDIS_PACKET_GENERATOR_INPUT_RELEASE);
    assert_eq!(
        pg_step(&mut s, &release, &mut out[..8], &mut actions, &mut d),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert_eq!(pg_step(&mut s, &release, &mut out, &mut actions, &mut d), 0);
    // A kind that is none.
    assert_eq!(
        pg_step(&mut s, &pg_input(9), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // The queue fills: WouldBlock, and the status counts the refusals.
    let mut blocked = 0;
    for _ in 0..1000 {
        if pg_step(&mut s, &submit(&short), &mut out, &mut actions, &mut d) == ALDIS_ERR_WOULD_BLOCK
        {
            blocked += 1;
            break;
        }
    }
    assert_eq!(blocked, 1);
    let mut status = aldis_packet_generator_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_packet_generator_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
    }
    assert_eq!((status.refused, status.would_block), (3, 1));
    assert!(status.queue.current > 0 && status.queue.high_water >= status.queue.current);
}

#[test]
fn buffers_must_not_overlap_the_storage() {
    // req: CABI-022
    let mut s = pg_storage();
    let cfg = pg_config(ALDIS_LINK_PROTOCOL_TM, 16);
    let mut d = detail();
    let mut actions = aldis_packet_generator_actions_t::default();
    // SAFETY: every pointer is to a live local, or into the storage, which the call refuses
    // before it reads or writes.
    unsafe {
        assert_eq!(
            aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        let inside = (&raw mut *s).cast::<u8>().add(64);
        let release = pg_input(ALDIS_PACKET_GENERATOR_INPUT_RELEASE);
        assert_eq!(
            aldis_packet_generator_step(
                &raw mut *s,
                &release,
                0,
                inside,
                16,
                &raw mut actions,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        let submit = aldis_packet_generator_input_t {
            kind: ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET,
            data: inside.cast_const(),
            data_len: 16,
            ..pg_input(0)
        };
        assert_eq!(
            aldis_packet_generator_step(
                &raw mut *s,
                &submit,
                0,
                null_mut(),
                0,
                &raw mut actions,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_generator_step(
                &raw mut *s,
                &release,
                0,
                null_mut(),
                0,
                null_mut(),
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_packet_generator_step(
                &raw mut *s,
                null(),
                0,
                null_mut(),
                0,
                &raw mut actions,
                &raw mut d
            ),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn reconfiguration_discards_and_keeps_the_protocol() {
    // req: LNK-003, LNK-001, GEN-061
    let mut s = pg_storage();
    let cfg = pg_config(ALDIS_LINK_PROTOCOL_AOS, 18);
    let mut d = detail();
    let mut out = [0_u8; 64];
    let mut actions = aldis_packet_generator_actions_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
    }
    let pkt = packet(10);
    let submit = aldis_packet_generator_input_t {
        kind: ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET,
        data: pkt.as_ptr(),
        data_len: pkt.len(),
        ..pg_input(0)
    };
    assert_eq!(pg_step(&mut s, &submit, &mut out, &mut actions, &mut d), 0);
    let mut reconf = aldis_packet_generator_input_t {
        config: pg_config(ALDIS_LINK_PROTOCOL_TM, 16),
        ..pg_input(ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE)
    };
    // Another protocol is refused, and so is a bad configuration.
    assert_eq!(
        pg_step(&mut s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    reconf.config = pg_config(ALDIS_LINK_PROTOCOL_AOS, 18);
    reconf.config.max_packet_len = 0;
    assert_eq!(
        pg_step(&mut s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    reconf.config.max_packet_len = 32;
    reconf.config.release_deadline_us = 500;
    assert_eq!(pg_step(&mut s, &reconf, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(
        actions.items[0].kind,
        ALDIS_PACKET_GENERATOR_ACTION_DISCARDED
    );
    assert_eq!(actions.items[0].octets, 10);
    // The same for TM and USLP engines.
    for (protocol, field_len) in [(ALDIS_LINK_PROTOCOL_TM, 16), (ALDIS_LINK_PROTOCOL_USLP, 27)] {
        let cfg = pg_config(protocol, field_len);
        // SAFETY: every pointer is to a live local.
        unsafe {
            assert_eq!(
                aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
                0
            );
        }
        let other = pg_config(ALDIS_LINK_PROTOCOL_AOS, 18);
        let mut r = aldis_packet_generator_input_t {
            config: other,
            ..pg_input(ALDIS_PACKET_GENERATOR_INPUT_RECONFIGURE)
        };
        assert_eq!(
            pg_step(&mut s, &r, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
        r.config = cfg;
        assert_eq!(pg_step(&mut s, &r, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 0);
    }
}

#[test]
fn the_release_deadline_is_a_read_only_query() {
    // req: API-053, LNK-012, CABI-040
    let mut s = pg_storage();
    let mut cfg = pg_config(ALDIS_LINK_PROTOCOL_TM, 32);
    cfg.release_deadline_us = 500;
    let mut d = detail();
    let mut out = [0_u8; 64];
    let mut actions = aldis_packet_generator_actions_t::default();
    let (mut armed, mut at) = (9_u8, 9_u64);
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_packet_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_packet_generator_next_deadline(
                &raw mut *s,
                &raw mut armed,
                &raw mut at,
                null_mut()
            ),
            0
        );
        assert_eq!((armed, at), (0, 0));
    }
    let pkt = packet(10);
    let submit = aldis_packet_generator_input_t {
        kind: ALDIS_PACKET_GENERATOR_INPUT_SUBMIT_PACKET,
        data: pkt.as_ptr(),
        data_len: pkt.len(),
        ..pg_input(0)
    };
    assert_eq!(pg_step(&mut s, &submit, &mut out, &mut actions, &mut d), 0);
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_packet_generator_next_deadline(
                &raw mut *s,
                &raw mut armed,
                &raw mut at,
                null_mut()
            ),
            0
        );
        assert_eq!((armed, at), (1, 510));
        let inside = (&raw mut *s).cast::<u64>().add(8);
        assert_eq!(
            aldis_packet_generator_next_deadline(&raw mut *s, &raw mut armed, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

fn ag_config(protocol: u32, field_len: usize) -> aldis_access_generator_config_t {
    aldis_access_generator_config_t {
        channel: channel(protocol, 2),
        field_len,
        max_sdu_len: 0,
        idle_pattern: 0x55,
    }
}

fn ag_input(kind: u32) -> aldis_access_generator_input_t {
    aldis_access_generator_input_t {
        kind,
        data: null(),
        data_len: 0,
        config: ag_config(ALDIS_LINK_PROTOCOL_TM, 8),
    }
}

fn ag_step(
    h: &mut aldis_access_generator_t,
    input: &aldis_access_generator_input_t,
    out: &mut [u8],
    actions: &mut aldis_access_generator_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_access_generator_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

#[test]
fn the_access_generator_serves_tm_aos_and_uslp() {
    // req: LNK-020, LNK-001, LNK-003, LNK-078, CABI-040
    for (protocol, field_len, header, pointer, route) in [
        (ALDIS_LINK_PROTOCOL_TM, 8, 0, 0, 0),
        (ALDIS_LINK_PROTOCOL_AOS, 8, 0, 0, 0),
        (ALDIS_LINK_PROTOCOL_USLP, 11, 3, 1, 1),
    ] {
        let mut s = ag_storage();
        let mut cfg = ag_config(protocol, field_len);
        let mut d = detail();
        let mut out = [0_u8; 32];
        let mut actions = aldis_access_generator_actions_t::default();
        let mut status = aldis_access_generator_status_t::default();
        // SAFETY: every pointer is to a live local.
        unsafe {
            assert_eq!(
                aldis_access_generator_init(&raw mut *s, &raw const cfg, null_mut()),
                0
            );
        }
        let sdu = [1_u8, 2, 3, 4, 5, 6, 7, 8];
        let submit = aldis_access_generator_input_t {
            kind: ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU,
            data: sdu.as_ptr(),
            data_len: field_len - header,
            ..ag_input(0)
        };
        assert_eq!(ag_step(&mut s, &submit, &mut out, &mut actions, &mut d), 0);
        let release = ag_input(ALDIS_ACCESS_GENERATOR_INPUT_RELEASE);
        assert_eq!(ag_step(&mut s, &release, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 1);
        let a = actions.items[0];
        assert_eq!(a.kind, ALDIS_ACCESS_GENERATOR_ACTION_DATA_FIELD);
        assert_eq!((a.vc_id, a.field.offset, a.field.len), (2, 0, field_len));
        assert_eq!((a.pointer_present, a.route_present), (pointer, route));
        assert_eq!(&out[header..field_len], &sdu[..field_len - header]);
        // The queue is empty after the release, and the SDU was counted.
        // SAFETY: every pointer is to a live local.
        unsafe {
            assert_eq!(
                aldis_access_generator_status(&raw mut *s, &raw mut status, null_mut()),
                0
            );
        }
        assert_eq!((status.sdus_in, status.fields_out), (1, 1));
        // Reconfigure keeps the protocol; another one is refused.
        cfg.idle_pattern = 0xAA;
        let mut reconf = aldis_access_generator_input_t {
            config: cfg,
            ..ag_input(ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE)
        };
        assert_eq!(ag_step(&mut s, &reconf, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 0);
        reconf.config.channel.protocol = if protocol == ALDIS_LINK_PROTOCOL_TM {
            ALDIS_LINK_PROTOCOL_AOS
        } else {
            ALDIS_LINK_PROTOCOL_TM
        };
        assert_eq!(
            ag_step(&mut s, &reconf, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
    }
}

#[test]
fn the_access_generator_checks_its_input_and_configuration() {
    // req: LNK-020, LNK-015, LNK-078, API-058, CABI-022
    let mut s = ag_storage();
    let mut d = detail();
    let mut out = [0_u8; 32];
    let mut actions = aldis_access_generator_actions_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        for (cfg, kind) in [
            (
                ag_config(ALDIS_LINK_PROTOCOL_TC, 8),
                ALDIS_ERR_INVALID_CONFIG,
            ),
            (
                ag_config(ALDIS_LINK_PROTOCOL_TM, 0),
                ALDIS_ERR_INVALID_CONFIG,
            ),
            (
                aldis_access_generator_config_t {
                    max_sdu_len: 3,
                    ..ag_config(ALDIS_LINK_PROTOCOL_TM, 8)
                },
                ALDIS_ERR_INVALID_CONFIG,
            ),
            (
                ag_config(ALDIS_LINK_PROTOCOL_USLP, ALDIS_LINK_PACKET_QUEUE_OCTETS + 8),
                ALDIS_ERR_CAPACITY_EXCEEDED,
            ),
        ] {
            assert_eq!(
                aldis_access_generator_init(&raw mut *s, &raw const cfg, &raw mut d),
                kind,
                "{cfg:?}"
            );
        }
        let spanning = aldis_access_generator_config_t {
            max_sdu_len: 20,
            ..ag_config(ALDIS_LINK_PROTOCOL_USLP, 11)
        };
        assert_eq!(
            aldis_access_generator_init(&raw mut *s, &raw const spanning, &raw mut d),
            0
        );
        let cfg = ag_config(ALDIS_LINK_PROTOCOL_TM, 8);
        assert_eq!(
            aldis_access_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
    }
    let sdu = [0_u8; 7];
    let submit = aldis_access_generator_input_t {
        kind: ALDIS_ACCESS_GENERATOR_INPUT_SUBMIT_SDU,
        data: sdu.as_ptr(),
        data_len: sdu.len(),
        ..ag_input(0)
    };
    assert_eq!(
        ag_step(&mut s, &submit, &mut out, &mut actions, &mut d),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    assert_eq!(
        ag_step(&mut s, &ag_input(7), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // Nothing queued: a release has nothing to give.
    assert_eq!(
        ag_step(
            &mut s,
            &ag_input(ALDIS_ACCESS_GENERATOR_INPUT_RELEASE),
            &mut out,
            &mut actions,
            &mut d
        ),
        0
    );
    assert_eq!(actions.count, 0);
    let ok = [0_u8; 8];
    let submit = aldis_access_generator_input_t {
        data: ok.as_ptr(),
        data_len: ok.len(),
        ..submit
    };
    let mut blocked = false;
    for _ in 0..2000 {
        if ag_step(&mut s, &submit, &mut out, &mut actions, &mut d) == ALDIS_ERR_WOULD_BLOCK {
            blocked = true;
            break;
        }
    }
    assert!(blocked);
    assert_eq!(
        ag_step(
            &mut s,
            &ag_input(ALDIS_ACCESS_GENERATOR_INPUT_RELEASE),
            &mut out[..4],
            &mut actions,
            &mut d
        ),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    let mut r = aldis_access_generator_input_t {
        config: ag_config(ALDIS_LINK_PROTOCOL_TM, 8),
        ..ag_input(ALDIS_ACCESS_GENERATOR_INPUT_RECONFIGURE)
    };
    assert_eq!(ag_step(&mut s, &r, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(
        actions.items[0].kind,
        ALDIS_ACCESS_GENERATOR_ACTION_DISCARDED
    );
    r.config.channel.protocol = 9;
    assert_eq!(
        ag_step(&mut s, &r, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // SAFETY: every pointer is to a live local.
    unsafe {
        aldis_access_generator_deinit(&raw mut *s);
        let mut status = aldis_access_generator_status_t::default();
        assert_eq!(
            aldis_access_generator_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

fn bg_config(field_len: usize) -> aldis_bitstream_generator_config_t {
    aldis_bitstream_generator_config_t {
        channel: channel(ALDIS_LINK_PROTOCOL_AOS, 3),
        field_len,
        idle_pattern: 0,
    }
}

fn bg_input(kind: u32) -> aldis_bitstream_generator_input_t {
    aldis_bitstream_generator_input_t {
        kind,
        data: null(),
        data_len: 0,
        bit_len: 0,
        config: bg_config(6),
    }
}

fn bg_step(
    h: &mut aldis_bitstream_generator_t,
    input: &aldis_bitstream_generator_input_t,
    out: &mut [u8],
    actions: &mut aldis_bitstream_generator_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_bitstream_generator_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

#[test]
fn the_bitstream_generator_builds_b_pdus() {
    // req: LNK-021, LNK-003, LNK-078, CABI-040
    let mut s = bg_storage();
    let mut d = detail();
    let mut out = [0_u8; 16];
    let mut actions = aldis_bitstream_generator_actions_t::default();
    let mut status = aldis_bitstream_generator_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe {
        for (cfg, kind) in [
            (bg_config(2), ALDIS_ERR_INVALID_CONFIG),
            (
                aldis_bitstream_generator_config_t {
                    channel: channel(ALDIS_LINK_PROTOCOL_TM, 3),
                    ..bg_config(6)
                },
                ALDIS_ERR_INVALID_CONFIG,
            ),
            (
                aldis_bitstream_generator_config_t {
                    channel: channel(9, 3),
                    ..bg_config(6)
                },
                ALDIS_ERR_INVALID_FIELD,
            ),
            (
                aldis_bitstream_generator_config_t {
                    channel: channel(ALDIS_LINK_PROTOCOL_AOS, 64),
                    ..bg_config(6)
                },
                ALDIS_ERR_OUT_OF_RANGE,
            ),
        ] {
            assert_eq!(
                aldis_bitstream_generator_init(&raw mut *s, &raw const cfg, &raw mut d),
                kind,
                "{cfg:?}"
            );
        }
        let cfg = bg_config(6);
        assert_eq!(
            aldis_bitstream_generator_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
    }
    // An idle B_PDU: no bit queued (732.0 §4.1.4.3.3.4).
    let release = bg_input(ALDIS_BITSTREAM_GENERATOR_INPUT_RELEASE);
    assert_eq!(bg_step(&mut s, &release, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].pointer, 0x3FFE);
    let bits = [0x01_u8, 0x23, 0x45, 0x67, 0x89];
    let submit = aldis_bitstream_generator_input_t {
        kind: ALDIS_BITSTREAM_GENERATOR_INPUT_SUBMIT_BITS,
        data: bits.as_ptr(),
        data_len: bits.len(),
        bit_len: 40,
        ..bg_input(0)
    };
    assert_eq!(bg_step(&mut s, &submit, &mut out, &mut actions, &mut d), 0);
    assert_eq!(bg_step(&mut s, &release, &mut out, &mut actions, &mut d), 0);
    let a = actions.items[0];
    assert_eq!(a.kind, ALDIS_BITSTREAM_GENERATOR_ACTION_DATA_FIELD);
    assert_eq!((a.vc_id, a.field.len, a.pointer), (3, 6, 0x3FFF));
    assert_eq!(&out[2..6], &bits[..4]);
    // Bits that do not fit are kept for the next B_PDU; a reconfiguration discards them.
    let zero_bits = aldis_bitstream_generator_input_t {
        bit_len: 0,
        ..submit
    };
    assert_eq!(
        bg_step(&mut s, &zero_bits, &mut out, &mut actions, &mut d),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    assert_eq!(
        bg_step(&mut s, &bg_input(5), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        bg_step(&mut s, &release, &mut out[..3], &mut actions, &mut d),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    let mut r = aldis_bitstream_generator_input_t {
        config: bg_config(8),
        ..bg_input(ALDIS_BITSTREAM_GENERATOR_INPUT_RECONFIGURE)
    };
    assert_eq!(bg_step(&mut s, &r, &mut out, &mut actions, &mut d), 0);
    assert_eq!(
        actions.items[0].kind,
        ALDIS_BITSTREAM_GENERATOR_ACTION_DISCARDED
    );
    assert_eq!(actions.items[0].octets, 1);
    r.config.channel.protocol = ALDIS_LINK_PROTOCOL_USLP;
    assert_eq!(
        bg_step(&mut s, &r, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    // SAFETY: every pointer is to a live local.
    unsafe {
        assert_eq!(
            aldis_bitstream_generator_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(
            (
                status.submissions,
                status.fields_out,
                status.idle_fields,
                status.refused
            ),
            (1, 2, 1, 1)
        );
        aldis_bitstream_generator_deinit(&raw mut *s);
        assert_eq!(
            aldis_bitstream_generator_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}
