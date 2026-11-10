//! The contract of the `ChannelMux` handle, called as a C caller calls it: storage and tags
//! (CABI-031, CABI-032), argument checks (CABI-022), the protocol chosen in the configuration
//! (D4, LNK-001), every member of the configuration read once and refused when invalid
//! (CABI-040), the step call and the read-only calls, and the capacities of the header
//! (CABI-030). The engine's behaviour is checked by the scenarios of
//! `test-vectors/link/send/channel_mux_*.json` (`vectors.rs`).
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::borrow_as_ptr,
    clippy::too_many_lines,
    clippy::large_stack_arrays,
    reason = "test-only harness: one long call sequence per contract, and the storage a C caller declares (RS-018)"
)]

use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::aos::{ALDIS_AOS_EDITION_ISSUE4, ALDIS_AOS_EDITION_ISSUE5};
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_CAPACITY, ALDIS_FIELD_COP_IN_EFFECT, ALDIS_FIELD_DATA_FIELD_CONTENT,
    ALDIS_FIELD_FIELD_LEN, ALDIS_FIELD_FIRST_HEADER_POINTER, ALDIS_FIELD_FRAME_LEN,
    ALDIS_FIELD_FSH_LEN, ALDIS_FIELD_INSERT_ZONE_LEN, ALDIS_FIELD_LINK_PROTOCOL,
    ALDIS_FIELD_MAP_ID, ALDIS_FIELD_PROTOCOL_CONTROL_COMMAND_FLAG, ALDIS_FIELD_SECURITY_HEADER_LEN,
    ALDIS_FIELD_SPACECRAFT_ID, ALDIS_FIELD_VC_ID, ALDIS_FIELD_VC_PRIORITY, ALDIS_FIELD_VC_WEIGHT,
    ALDIS_FIELD_VCF_COUNT_LEN,
};
use aldis_ffi::link::*;
use aldis_ffi::link_mux::*;
use aldis_ffi::link_rx::*;

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

fn storage() -> Box<aldis_channel_mux_t> {
    Box::new(aldis_channel_mux_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_CHANNEL_MUX_WORDS],
    })
}

fn vc(id: u8, priority: u8) -> aldis_channel_mux_vc_t {
    aldis_channel_mux_vc_t {
        vc_id: id,
        priority,
        role: ALDIS_LINK_CONTENT_PACKETS,
        ..aldis_channel_mux_vc_t::default()
    }
}

/// The TM channel of the vectors: VCs 1, 2, 3, and the OID VC 7; frames of 24 octets, 16 of them
/// the data field.
fn tm_config() -> aldis_channel_mux_config_t {
    let mut c = aldis_channel_mux_config_t {
        protocol: ALDIS_LINK_PROTOCOL_TM,
        scid: 165,
        frame_len: 24,
        fecf: 1,
        tm_oid_vc: 7,
        vc_count: 4,
        ..aldis_channel_mux_config_t::default()
    };
    c.vcs[0] = vc(1, 1);
    c.vcs[1] = vc(2, 3);
    c.vcs[2] = vc(3, 2);
    c.vcs[3] = vc(7, 0);
    c
}

/// The AOS channel: VCs 1 and 2, an Insert Zone of 3 octets; 17 octets of data field.
fn aos_config() -> aldis_channel_mux_config_t {
    let mut c = aldis_channel_mux_config_t {
        protocol: ALDIS_LINK_PROTOCOL_AOS,
        scid: 421,
        frame_len: 28,
        fecf: 1,
        insert_zone_len: 3,
        insert_idle_pattern: 0x5A,
        vc_count: 2,
        ..aldis_channel_mux_config_t::default()
    };
    c.vcs[0] = vc(1, 1);
    c.vcs[1] = vc(2, 2);
    c
}

/// The USLP channel: VC 1 with COP-1 and a 2-octet VCF Count; 21 octets of data field.
fn uslp_config() -> aldis_channel_mux_config_t {
    let mut c = aldis_channel_mux_config_t {
        protocol: ALDIS_LINK_PROTOCOL_USLP,
        scid: 4660,
        frame_len: 32,
        fecf: 1,
        vc_count: 1,
        ..aldis_channel_mux_config_t::default()
    };
    c.vcs[0] = vc(1, 1);
    c.vcs[0].uslp_map = 3;
    c.vcs[0].uslp_vcf_len_sc = 2;
    c.vcs[0].uslp_vcf_len_exp = 1;
    c.vcs[0].uslp_cop = 1;
    c
}

fn input(kind: u32) -> aldis_channel_mux_input_t {
    aldis_channel_mux_input_t {
        kind,
        data: null(),
        data_len: 0,
        vc_id: 0,
        scope: ALDIS_LINK_SCOPE_MASTER_CHANNEL,
        ocf: 0,
        pointer_present: 0,
        pointer: 0,
        route_present: 0,
        map_id: 0,
        qos: 0,
        cop_present: 0,
        cop_vcf_count: 0,
        cop_bypass: 0,
        cop_protocol_control: 0,
        config: tm_config(),
    }
}

fn init(
    s: &mut aldis_channel_mux_t,
    cfg: &aldis_channel_mux_config_t,
    d: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_channel_mux_init(s, cfg, d) }
}

fn step(
    h: &mut aldis_channel_mux_t,
    input: &aldis_channel_mux_input_t,
    out: &mut [u8],
    actions: &mut aldis_channel_mux_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_channel_mux_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

fn status(h: &mut aldis_channel_mux_t) -> aldis_channel_mux_status_t {
    let mut s = aldis_channel_mux_status_t::default();
    let mut d = detail();
    // SAFETY: every pointer is to a live local.
    assert_eq!(unsafe { aldis_channel_mux_status(h, &mut s, &mut d) }, 0);
    s
}

/// The kind and the field of the error of `init` on `cfg`.
fn refused(cfg: &aldis_channel_mux_config_t) -> (u32, u16) {
    let mut s = storage();
    let mut d = detail();
    let status = init(&mut s, cfg, &mut d);
    assert_ne!(status, 0);
    assert_eq!(status, d.kind);
    (status, d.field)
}

#[test]
fn capacities_are_published_and_match_the_engine() {
    // req: CABI-030, CABI-031, LNK-074
    assert_eq!(
        ALDIS_CHANNEL_MUX_MAX_ACTIONS,
        aldis::link::mux::CHANNEL_MUX_MAX_ACTIONS
    );
    assert_eq!(
        ALDIS_CHANNEL_MUX_MAX_FSH_DATA,
        aldis::link::mux::MAX_FSH_DATA
    );
    assert_eq!(
        size_of::<aldis_channel_mux_t>(),
        ALDIS_CHANNEL_MUX_WORDS * 8
    );
    assert_eq!(align_of::<aldis_channel_mux_t>(), 8);
}

#[test]
fn the_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = tm_config();
    let mut d = detail();
    let mut s = storage();
    let mut st = aldis_channel_mux_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_channel_mux_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_channel_mux_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_channel_mux_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_channel_mux_status(&raw mut *s, &raw mut st, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_channel_mux_status(null_mut(), &raw mut st, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_channel_mux_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_channel_mux_status(&raw mut *s, &raw mut st, null_mut()),
            0
        );
        assert_eq!(st, aldis_channel_mux_status_t::default());
        // A copy of live storage is foreign: the tag holds its own address.
        let mut copy = storage();
        // A byte copy: the engine inside the words has padding, which a typed copy would read
        // uninitialized (Miri, VER-026).
        core::ptr::copy_nonoverlapping(
            (&raw const *s).cast::<u8>(),
            (&raw mut *copy).cast::<u8>(),
            size_of_val(&*s),
        );
        assert_eq!(
            aldis_channel_mux_status(&raw mut *copy, &raw mut st, null_mut()),
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
        let inside = (&raw mut *s).cast::<aldis_channel_mux_status_t>();
        assert_eq!(
            aldis_channel_mux_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_channel_mux_deinit(&raw mut *s);
        aldis_channel_mux_deinit(&raw mut *s);
        aldis_channel_mux_deinit(null_mut());
        assert_eq!(
            aldis_channel_mux_status(&raw mut *s, &raw mut st, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_protocol_is_chosen_in_the_configuration() {
    // req: LNK-001, CABI-040
    for cfg in [tm_config(), aos_config(), uslp_config()] {
        let mut s = storage();
        let mut d = detail();
        assert_eq!(init(&mut s, &cfg, &mut d), 0);
    }
    // TC has no multiplexer here, and a value that is no protocol is not read as one.
    let mut tc = tm_config();
    tc.protocol = ALDIS_LINK_PROTOCOL_TC;
    assert_eq!(
        refused(&tc),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_LINK_PROTOCOL)
    );
    tc.protocol = 9;
    assert_eq!(refused(&tc).0, ALDIS_ERR_INVALID_FIELD);
}

#[test]
fn every_member_of_the_configuration_is_read_and_checked() {
    // req: CABI-040, CABI-022, LNK-078, LNK-031, LNK-033, LNK-034, LNK-035
    let invalid = |f: &dyn Fn(&mut aldis_channel_mux_config_t)| {
        let mut c = tm_config();
        f(&mut c);
        refused(&c)
    };
    // The members common to every protocol.
    assert_eq!(invalid(&|c| c.fecf = 2).0, ALDIS_ERR_INVALID_FIELD);
    assert_eq!(invalid(&|c| c.policy = 9).0, ALDIS_ERR_INVALID_FIELD);
    assert_eq!(
        invalid(&|c| c.frame_len = 2048 + 1),
        (ALDIS_ERR_CAPACITY_EXCEEDED, ALDIS_FIELD_FRAME_LEN)
    );
    // The capacity is the engine's, whatever the protocol (LNK-075).
    for mut c in [aos_config(), uslp_config()] {
        c.frame_len = 2048 + 1;
        assert_eq!(
            refused(&c),
            (ALDIS_ERR_CAPACITY_EXCEEDED, ALDIS_FIELD_FRAME_LEN)
        );
    }
    assert_eq!(
        invalid(&|c| c.frame_len = 8),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_FRAME_LEN)
    );
    assert_eq!(
        invalid(&|c| c.vc_count = 9),
        (ALDIS_ERR_CAPACITY_EXCEEDED, ALDIS_FIELD_CAPACITY)
    );
    assert_eq!(
        invalid(&|c| c.vcs[1].priority = 1),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_PRIORITY)
    );
    assert_eq!(
        invalid(&|c| c.vcs[1].vc_id = 1),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID)
    );
    assert_eq!(
        invalid(&|c| c.vcs[1].vc_id = 8),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_VC_ID)
    );
    assert_eq!(
        invalid(&|c| c.vcs[0].weight = 0),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_WEIGHT)
    );
    assert_eq!(
        invalid(&|c| c.vcs[0].sec_header_len = 65),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_SECURITY_HEADER_LEN)
    );
    assert_eq!(
        invalid(&|c| c.vcs[0].sec_trailer_len = 65_536).0,
        ALDIS_ERR_INVALID_CONFIG
    );
    // The Insert Zone of a protocol without one.
    assert_eq!(
        invalid(&|c| c.insert_zone_len = 2),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_INSERT_ZONE_LEN)
    );
    // TM members.
    assert_eq!(
        invalid(&|c| c.scid = 1024),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_SPACECRAFT_ID)
    );
    assert_eq!(
        invalid(&|c| c.tm_oid_vc = 8),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_VC_ID)
    );
    assert_eq!(invalid(&|c| c.mc_ocf = 2).0, ALDIS_ERR_INVALID_FIELD);
    assert_eq!(invalid(&|c| c.vcs[0].ocf = 2).0, ALDIS_ERR_INVALID_FIELD);
    assert_eq!(
        invalid(&|c| c.mc_fsh_len = 64),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_FSH_LEN)
    );
    assert_eq!(
        invalid(&|c| c.vcs[0].fsh_len = 64),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_FSH_LEN)
    );
    assert_eq!(invalid(&|c| c.vcs[0].role = 9).0, ALDIS_ERR_INVALID_FIELD);
    for role in [ALDIS_LINK_CONTENT_BITSTREAM, ALDIS_LINK_CONTENT_UNSERVED] {
        assert_eq!(
            invalid(&|c| c.vcs[0].role = role),
            (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_DATA_FIELD_CONTENT)
        );
    }
    // An MC and a VC that both have an OCF (132.0 §2.2.4 b).
    assert_eq!(
        invalid(&|c| {
            c.mc_ocf = 1;
            c.vcs[0].ocf = 1;
        })
        .0,
        ALDIS_ERR_INVALID_CONFIG
    );
    // The members of AOS.
    let bad_aos = |f: &dyn Fn(&mut aldis_channel_mux_config_t)| {
        let mut c = aos_config();
        f(&mut c);
        refused(&c)
    };
    assert_eq!(bad_aos(&|c| c.aos_edition = 2).0, ALDIS_ERR_INVALID_FIELD);
    assert_eq!(bad_aos(&|c| c.aos_fhec = 2).0, ALDIS_ERR_INVALID_FIELD);
    assert_eq!(
        bad_aos(&|c| c.aos_cycle_in_use = 2).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        bad_aos(&|c| c.scid = 1024),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_SPACECRAFT_ID)
    );
    // The members of USLP.
    let bad_uslp = |f: &dyn Fn(&mut aldis_channel_mux_config_t)| {
        let mut c = uslp_config();
        f(&mut c);
        refused(&c)
    };
    assert_eq!(
        bad_uslp(&|c| c.uslp_destination = 2).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        bad_uslp(&|c| c.uslp_oid_vcf_len = 8),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VCF_COUNT_LEN)
    );
    assert_eq!(
        bad_uslp(&|c| c.vcs[0].uslp_map = 16),
        (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_MAP_ID)
    );
    assert_eq!(
        bad_uslp(&|c| c.vcs[0].uslp_qos = 9).0,
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        bad_uslp(&|c| c.vcs[0].uslp_vcf_len_sc = 8),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VCF_COUNT_LEN)
    );
    assert_eq!(
        bad_uslp(&|c| c.vcs[0].uslp_vcf_len_exp = 8),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VCF_COUNT_LEN)
    );
    assert_eq!(
        bad_uslp(&|c| c.vcs[0].uslp_cop = 2).0,
        ALDIS_ERR_INVALID_FIELD
    );
    // COP-1 in effect needs a count field for the Frame Sequence Number.
    assert_eq!(
        bad_uslp(&|c| c.vcs[0].uslp_vcf_len_sc = 0),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VCF_COUNT_LEN)
    );
}

#[test]
fn the_aos_edition_sets_the_scid_width() {
    // req: AOS-001, AOS-002, CABI-040
    let mut cfg = aos_config();
    cfg.aos_edition = ALDIS_AOS_EDITION_ISSUE4;
    cfg.scid = 255;
    let mut s = storage();
    let mut d = detail();
    assert_eq!(init(&mut s, &cfg, &mut d), 0);
    // Issue 4 has an 8-bit SCID: the edition is read, not ignored.
    cfg.scid = 256;
    assert_eq!(
        refused(&cfg),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_SPACECRAFT_ID)
    );
    cfg.aos_edition = ALDIS_AOS_EDITION_ISSUE5;
    let mut s = storage();
    assert_eq!(init(&mut s, &cfg, &mut d), 0);
}

#[test]
fn the_step_builds_frames_and_changes_the_slots() {
    // req: CABI-040, LNK-030, LNK-033, LNK-034, LNK-076, API-057, API-058
    let mut s = storage();
    let mut d = detail();
    let mut actions = aldis_channel_mux_actions_t::default();
    let mut out = [0xEE_u8; 64];
    assert_eq!(init(&mut s, &tm_config(), &mut d), 0);
    // With nothing offered, an OID frame on the dedicated VC.
    assert_eq!(
        step(
            &mut s,
            &input(ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY),
            &mut out,
            &mut actions,
            &mut d
        ),
        0
    );
    assert_eq!(actions.count, 1);
    let a = actions.items[0];
    assert_eq!(a.kind, ALDIS_CHANNEL_MUX_ACTION_TRANSMIT);
    assert_eq!(a.class, ALDIS_LINK_FRAME_CLASS_OID);
    assert_eq!((a.vc_id, a.frame.offset, a.frame.len), (7, 0, 24));
    // The area must hold the frame.
    let mut small = [0_u8; 8];
    assert_eq!(
        step(
            &mut s,
            &input(ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY),
            &mut small,
            &mut actions,
            &mut d
        ),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    // An offer of the wrong length, then the right one, which is held, and a second is refused.
    let field = [0x11_u8; 16];
    let mut offer = input(ALDIS_CHANNEL_MUX_INPUT_OFFER);
    offer.vc_id = 1;
    offer.data = field.as_ptr();
    offer.data_len = 15;
    offer.pointer_present = 1;
    offer.pointer = 0;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    assert_eq!(d.field, ALDIS_FIELD_FIELD_LEN);
    offer.data_len = 16;
    assert_eq!(step(&mut s, &offer, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 0);
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_WOULD_BLOCK
    );
    // A pointer is required on a TM VC of Packets.
    offer.vc_id = 2;
    offer.pointer_present = 0;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_FIRST_HEADER_POINTER);
    // Every flag of an offer is 0 or 1.
    for which in 0..3 {
        let mut bad = offer;
        match which {
            0 => bad.pointer_present = 2,
            1 => bad.route_present = 2,
            _ => bad.cop_present = 2,
        }
        assert_eq!(
            step(&mut s, &bad, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // A route on TM, a COP block on TM.
    let mut bad = offer;
    bad.route_present = 1;
    assert_eq!(
        step(&mut s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    bad.route_present = 0;
    bad.cop_present = 1;
    assert_eq!(
        step(&mut s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_COP_IN_EFFECT);
    // The offered field goes out, with the Security ranges in the action.
    assert_eq!(
        step(
            &mut s,
            &input(ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY),
            &mut out,
            &mut actions,
            &mut d
        ),
        0
    );
    let a = actions.items[0];
    assert_eq!((a.class, a.vc_id), (ALDIS_LINK_FRAME_CLASS_DATA, 1));
    assert_eq!((a.data.offset, a.data.len), (6, 16));
    assert_eq!(&out[6..22], &field);
    // The OCF and Secondary Header slots: the MC has neither, a VC not configured is unknown.
    let mut ocf = input(ALDIS_CHANNEL_MUX_INPUT_SET_OCF);
    ocf.ocf = 0x0102_0304;
    assert_eq!(
        step(&mut s, &ocf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    ocf.scope = ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL;
    ocf.vc_id = 5;
    assert_eq!(
        step(&mut s, &ocf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_UNKNOWN_CHANNEL
    );
    ocf.scope = 9;
    assert_eq!(
        step(&mut s, &ocf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    let fsh = [1_u8, 2];
    let mut set = input(ALDIS_CHANNEL_MUX_INPUT_SET_FSH);
    set.data = fsh.as_ptr();
    set.data_len = fsh.len();
    assert_eq!(
        step(&mut s, &set, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    set.kind = ALDIS_CHANNEL_MUX_INPUT_SET_INSERT;
    assert_eq!(
        step(&mut s, &set, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_INSERT_ZONE_LEN);
    set.kind = 99;
    assert_eq!(
        step(&mut s, &set, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    let st = status(&mut s);
    assert_eq!((st.frames_out, st.oid_frames, st.offers), (2, 1, 1));
    assert_eq!((st.would_block, st.slots.current), (1, 0));
    assert!(st.refused >= 3);
}

#[test]
fn the_slots_of_tm_aos_and_uslp_take_their_values() {
    // req: CABI-040, LNK-033, LNK-034, LNK-032, LNK-076
    let mut d = detail();
    let mut actions = aldis_channel_mux_actions_t::default();
    let mut out = [0xEE_u8; 64];

    // TM: a VC with an OCF and a Secondary Header takes new values, of the configured length.
    let mut cfg = tm_config();
    cfg.vcs[0].ocf = 1;
    cfg.vcs[0].ocf_init = 0x0A0B_0C0D;
    cfg.vcs[0].fsh_len = 2;
    cfg.vcs[0].fsh_init = [0x0F; ALDIS_CHANNEL_MUX_MAX_FSH_DATA];
    cfg.frame_len = 32;
    let mut s = storage();
    assert_eq!(init(&mut s, &cfg, &mut d), 0);
    let mut ocf = input(ALDIS_CHANNEL_MUX_INPUT_SET_OCF);
    ocf.scope = ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL;
    ocf.vc_id = 1;
    ocf.ocf = 0x0102_0304;
    assert_eq!(step(&mut s, &ocf, &mut out, &mut actions, &mut d), 0);
    let new_fsh = [0xC1_u8, 0xC2];
    let mut set = input(ALDIS_CHANNEL_MUX_INPUT_SET_FSH);
    set.scope = ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL;
    set.vc_id = 1;
    set.data = new_fsh.as_ptr();
    set.data_len = new_fsh.len();
    assert_eq!(step(&mut s, &set, &mut out, &mut actions, &mut d), 0);
    set.data_len = 1;
    assert_eq!(
        step(&mut s, &set, &mut out, &mut actions, &mut d),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    let field = [0x22_u8; 32 - 6 - 2 - 4 - 3];
    let mut offer = input(ALDIS_CHANNEL_MUX_INPUT_OFFER);
    offer.vc_id = 1;
    offer.data = field.as_ptr();
    offer.data_len = field.len();
    offer.pointer_present = 1;
    assert_eq!(step(&mut s, &offer, &mut out, &mut actions, &mut d), 0);
    assert_eq!(
        step(
            &mut s,
            &input(ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY),
            &mut out,
            &mut actions,
            &mut d
        ),
        0
    );
    assert_eq!(actions.items[0].class, ALDIS_LINK_FRAME_CLASS_DATA);
    // The frame holds the Secondary Header, then the data field, then the OCF and the FECF.
    assert_eq!(&out[6..9], &[0x02, 0xC1, 0xC2]);
    assert_eq!(&out[26..30], &[0x01, 0x02, 0x03, 0x04]);

    // AOS: the Insert Zone repeats its value until a new one is set.
    let mut s = storage();
    assert_eq!(init(&mut s, &aos_config(), &mut d), 0);
    let mut opportunity = input(ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY);
    opportunity.config = aos_config();
    assert_eq!(
        step(&mut s, &opportunity, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 2);
    assert_eq!(
        actions.items[1].kind,
        ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED
    );
    assert_eq!(&out[6..9], &[0x5A; 3]);
    let iz = [1_u8, 2, 3];
    let mut set = input(ALDIS_CHANNEL_MUX_INPUT_SET_INSERT);
    set.data = iz.as_ptr();
    set.data_len = 3;
    assert_eq!(step(&mut s, &set, &mut out, &mut actions, &mut d), 0);
    assert_eq!(
        step(&mut s, &opportunity, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    assert_eq!(&out[6..9], &iz);
    set.data_len = 2;
    assert_eq!(
        step(&mut s, &set, &mut out, &mut actions, &mut d),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    assert_eq!(d.field, ALDIS_FIELD_INSERT_ZONE_LEN);

    // USLP: a route, a COP block of each kind, and the refusals of the block.
    let mut s = storage();
    assert_eq!(init(&mut s, &uslp_config(), &mut d), 0);
    let field = [0x33_u8; 21];
    let mut offer = input(ALDIS_CHANNEL_MUX_INPUT_OFFER);
    offer.config = uslp_config();
    offer.vc_id = 1;
    offer.data = field.as_ptr();
    offer.data_len = field.len();
    offer.route_present = 1;
    offer.map_id = 3;
    offer.qos = ALDIS_LINK_QOS_SEQUENCE_CONTROLLED;
    offer.cop_present = 1;
    offer.cop_vcf_count = 70_000;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_OUT_OF_RANGE
    );
    offer.cop_vcf_count = 5;
    offer.cop_protocol_control = 1;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_PROTOCOL_CONTROL_COMMAND_FLAG);
    offer.cop_protocol_control = 0;
    offer.cop_bypass = 2;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    offer.cop_bypass = 0;
    offer.map_id = 4;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_UNKNOWN_CHANNEL
    );
    assert_eq!(d.field, ALDIS_FIELD_MAP_ID);
    offer.map_id = 16;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_OUT_OF_RANGE
    );
    offer.map_id = 3;
    offer.qos = 9;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    offer.qos = ALDIS_LINK_QOS_EXPEDITED;
    assert_eq!(
        step(&mut s, &offer, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    offer.qos = ALDIS_LINK_QOS_SEQUENCE_CONTROLLED;
    assert_eq!(step(&mut s, &offer, &mut out, &mut actions, &mut d), 0);
    assert_eq!(status(&mut s).offers, 1);
}

#[test]
fn a_reconfiguration_keeps_the_protocol_and_reports_what_it_discards() {
    // req: LNK-003, CABI-040, API-058
    let mut d = detail();
    let mut actions = aldis_channel_mux_actions_t::default();
    let mut out = [0_u8; 64];
    for (cfg, other) in [
        (tm_config(), aos_config()),
        (aos_config(), uslp_config()),
        (uslp_config(), tm_config()),
    ] {
        let mut s = storage();
        assert_eq!(init(&mut s, &cfg, &mut d), 0);
        let mut re = input(ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE);
        re.config = cfg;
        assert_eq!(step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
        assert_eq!(actions.count, 0);
        re.config = other;
        assert_eq!(
            step(&mut s, &re, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
        assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
        // A configuration that is refused changes nothing.
        re.config = cfg;
        re.config.policy = 9;
        assert_eq!(
            step(&mut s, &re, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        re.config.protocol = 9;
        assert_eq!(
            step(&mut s, &re, &mut out, &mut actions, &mut d),
            ALDIS_ERR_INVALID_FIELD
        );
    }
    // A reconfiguration is checked against the capacity too, and changes nothing.
    for cfg in [tm_config(), aos_config(), uslp_config()] {
        let mut s = storage();
        assert_eq!(init(&mut s, &cfg, &mut d), 0);
        let mut re = input(ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE);
        re.config = cfg;
        re.config.frame_len = 2048 + 1;
        assert_eq!(
            step(&mut s, &re, &mut out, &mut actions, &mut d),
            ALDIS_ERR_CAPACITY_EXCEEDED
        );
        assert_eq!(d.field, ALDIS_FIELD_FRAME_LEN);
    }
    // The offered fields a reconfiguration drops are reported in octets.
    let mut s = storage();
    assert_eq!(init(&mut s, &tm_config(), &mut d), 0);
    let field = [0x44_u8; 16];
    let mut offer = input(ALDIS_CHANNEL_MUX_INPUT_OFFER);
    offer.vc_id = 1;
    offer.data = field.as_ptr();
    offer.data_len = field.len();
    offer.pointer_present = 1;
    assert_eq!(step(&mut s, &offer, &mut out, &mut actions, &mut d), 0);
    let re = input(ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE);
    assert_eq!(step(&mut s, &re, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].kind, ALDIS_CHANNEL_MUX_ACTION_DISCARDED);
    assert_eq!(actions.items[0].octets, 16);
    assert_eq!(status(&mut s).discarded_octets, 16);
}

#[test]
fn the_step_checks_its_pointers() {
    // req: CABI-022, CABI-040
    let mut s = storage();
    let mut d = detail();
    assert_eq!(init(&mut s, &tm_config(), &mut d), 0);
    let mut actions = aldis_channel_mux_actions_t::default();
    let mut out = [0_u8; 64];
    let mut field = input(ALDIS_CHANNEL_MUX_INPUT_OFFER);
    // SAFETY: every pointer is NULL, to a live local, or an address inside the storage that is
    // rejected before any access.
    unsafe {
        assert_eq!(
            aldis_channel_mux_step(
                &raw mut *s,
                null(),
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_channel_mux_step(
                &raw mut *s,
                &raw const field,
                out.as_mut_ptr(),
                out.len(),
                null_mut(),
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // The data of an input must not overlap the storage.
        field.data = (&raw const *s).cast::<u8>();
        field.data_len = 4;
        assert_eq!(
            aldis_channel_mux_step(
                &raw mut *s,
                &raw const field,
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
        // `NULL` data only with length 0.
        field.data = null();
        assert_eq!(
            aldis_channel_mux_step(
                &raw mut *s,
                &raw const field,
                out.as_mut_ptr(),
                out.len(),
                &raw mut actions,
                null_mut()
            ),
            ALDIS_ERR_INVALID_FIELD
        );
    }
}

#[test]
fn a_member_of_another_protocol_is_read_and_refused_when_invalid() {
    // req: CABI-040, CABI-022
    type Edit = fn(&mut aldis_channel_mux_config_t);
    // Flags and constants: InvalidField from any base configuration.
    let invalid_field: [Edit; 12] = [
        |c| c.fecf = 7,
        |c| c.policy = 7,
        |c| c.mc_ocf = 7,
        |c| c.aos_edition = 7,
        |c| c.aos_fhec = 7,
        |c| c.aos_cycle_in_use = 7,
        |c| c.uslp_destination = 7,
        |c| c.vcs[0].ocf = 7,
        |c| c.vcs[0].role = 7,
        |c| c.vcs[0].uslp_qos = 7,
        |c| c.vcs[0].uslp_cop = 7,
        |c| c.vcs[1].uslp_cop = 7,
    ];
    // Ranges of the members of another protocol: refused with their own kind.
    let ranges: [(Edit, u32, u16); 8] = [
        (
            |c| c.tm_oid_vc = 8,
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_VC_ID,
        ),
        (
            |c| c.uslp_oid_vcf_len = 8,
            ALDIS_ERR_INVALID_CONFIG,
            ALDIS_FIELD_VCF_COUNT_LEN,
        ),
        (
            |c| c.mc_fsh_len = 64,
            ALDIS_ERR_INVALID_CONFIG,
            ALDIS_FIELD_FSH_LEN,
        ),
        (
            |c| c.vcs[0].fsh_len = 64,
            ALDIS_ERR_INVALID_CONFIG,
            ALDIS_FIELD_FSH_LEN,
        ),
        (
            |c| c.vcs[0].uslp_vcf_len_sc = 8,
            ALDIS_ERR_INVALID_CONFIG,
            ALDIS_FIELD_VCF_COUNT_LEN,
        ),
        (
            |c| c.vcs[0].uslp_vcf_len_exp = 8,
            ALDIS_ERR_INVALID_CONFIG,
            ALDIS_FIELD_VCF_COUNT_LEN,
        ),
        (
            |c| {
                c.vc_count = 2;
                c.vcs[1] = vc(2, 2);
                c.vcs[1].uslp_map = 16;
            },
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_MAP_ID,
        ),
        (
            |c| c.vcs[0].uslp_map = 16,
            ALDIS_ERR_OUT_OF_RANGE,
            ALDIS_FIELD_MAP_ID,
        ),
    ];
    for base in [tm_config(), aos_config(), uslp_config()] {
        let mut s = storage();
        let mut d = detail();
        assert_eq!(init(&mut s, &base, &mut d), 0);
        for (i, edit) in invalid_field.iter().enumerate() {
            let mut c = base;
            c.vc_count = 2;
            c.vcs[1] = vc(2, 2);
            edit(&mut c);
            assert_eq!(
                refused(&c).0,
                ALDIS_ERR_INVALID_FIELD,
                "protocol {} edit {i}",
                base.protocol
            );
            // The same through a reconfiguration, which changes nothing.
            let mut s = storage();
            assert_eq!(init(&mut s, &base, &mut d), 0);
            let mut re = input(ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE);
            re.config = c;
            let mut actions = aldis_channel_mux_actions_t::default();
            assert_eq!(
                step(&mut s, &re, &mut [0; 64], &mut actions, &mut d),
                ALDIS_ERR_INVALID_FIELD
            );
        }
        for (i, (edit, kind, field)) in ranges.iter().enumerate() {
            let mut c = base;
            edit(&mut c);
            assert_eq!(
                refused(&c),
                (*kind, *field),
                "protocol {} range {i}",
                base.protocol
            );
        }
    }
    // An entry past `vc_count` is not read.
    let mut c = tm_config();
    c.vcs[5].ocf = 7;
    let mut s = storage();
    let mut d = detail();
    assert_eq!(init(&mut s, &c, &mut d), 0);
}

#[test]
fn every_protocol_refuses_a_bad_member_and_reports_its_status() {
    // req: CABI-022, CABI-040, LNK-001, LNK-002, API-056
    for cfg in [tm_config(), aos_config(), uslp_config()] {
        // A VC of no valid identifier, whatever the protocol.
        let mut bad = cfg;
        bad.vcs[0].vc_id = 200;
        let (code, field) = refused(&bad);
        assert_eq!((code, field), (ALDIS_ERR_OUT_OF_RANGE, ALDIS_FIELD_VC_ID));
        // More VCs than the array holds.
        let mut bad = cfg;
        bad.vc_count = aldis_ffi::capacities::ALDIS_LINK_MAX_VIRTUAL_CHANNELS + 1;
        assert_eq!(refused(&bad).0, ALDIS_ERR_CAPACITY_EXCEEDED);
        // A fresh engine reports an empty status.
        let mut s = storage();
        assert_eq!(init(&mut s, &cfg, &mut detail()), 0);
        assert_eq!(status(&mut s), aldis_channel_mux_status_t::default());
    }
}

#[test]
fn the_frame_checks_report_the_same_field_and_order_as_before_the_physical_sets() {
    // req: LNK-078, CABI-040, API-040
    let refuse = |c: &aldis_channel_mux_config_t| {
        let mut s = storage();
        let mut d = detail();
        let status = init(&mut s, c, &mut d);
        (status, d.field)
    };
    let invalid_frame_len = (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_FRAME_LEN);
    // TM: a frame with a FECF that leaves no data field.
    let mut c = tm_config();
    c.frame_len = 8;
    assert_eq!(refuse(&c), invalid_frame_len);
    // USLP: a frame above the 16-bit Frame Length field.
    let mut c = uslp_config();
    c.frame_len = 65_537;
    assert_eq!(refuse(&c), invalid_frame_len);
    // AOS: an Issue 4 SCID above 255 is refused before the frame length is, and before the OID
    // VCID.
    let mut c = aos_config();
    c.aos_edition = ALDIS_AOS_EDITION_ISSUE4;
    c.scid = 256;
    c.frame_len = 4;
    assert_eq!(
        refuse(&c),
        (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_SPACECRAFT_ID)
    );
    // AOS and USLP: VCID 63 is refused before the frame length is.
    let mut c = aos_config();
    c.frame_len = 4;
    c.vcs[0].vc_id = 63;
    assert_eq!(refuse(&c), (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID));
    let mut c = uslp_config();
    c.frame_len = 65_537;
    c.vcs[0].vc_id = 63;
    assert_eq!(refuse(&c), (ALDIS_ERR_INVALID_CONFIG, ALDIS_FIELD_VC_ID));
    // An Insert Zone that takes the whole data field.
    let mut c = aos_config();
    c.insert_zone_len = 20;
    assert_eq!(refuse(&c), invalid_frame_len);
}
