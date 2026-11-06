//! The contract of the `CltuPacker` handle, called as a C caller calls it: storage and tags
//! (CABI-031, CABI-032), argument checks (CABI-022), the protocol chosen in the configuration
//! (D4, LNK-001), the step call and the read-only calls (CABI-040), and the capacities of the
//! header (CABI-030). The rules of the engine are checked by `crates/aldis-link/tests/
//! cltu_packer.rs`.
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::borrow_as_ptr,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "test-only harness: one long call sequence per contract, and the storage a C caller declares (RS-018)"
)]

use core::ptr::{null, null_mut};

use aldis_ffi::aldis_error_detail_t;
use aldis_ffi::capacities::ALDIS_LINK_MAX_FRAME_OCTETS;
use aldis_ffi::error::*;
use aldis_ffi::field::{
    ALDIS_FIELD_FRAME_LEN, ALDIS_FIELD_LINK_PROTOCOL, ALDIS_FIELD_MAX_FRAMES_PER_UNIT,
    ALDIS_FIELD_MAX_UNIT_LEN, ALDIS_FIELD_TRUNCATED_FRAME_LEN,
};
use aldis_ffi::link::*;
use aldis_ffi::link_cltu::*;

fn detail() -> aldis_error_detail_t {
    aldis_error_detail_t {
        kind: 999,
        ..aldis_error_detail_t::default()
    }
}

fn storage() -> Box<aldis_cltu_packer_t> {
    Box::new(aldis_cltu_packer_t {
        words: [0xA5A5_A5A5_A5A5_A5A5; ALDIS_CLTU_PACKER_WORDS],
    })
}

const fn config(protocol: u32) -> aldis_cltu_packer_config_t {
    aldis_cltu_packer_config_t {
        channel: aldis_link_channel_t {
            protocol,
            vc_id: 0,
            aos_edition: 0,
            uslp_upid: 0,
            uslp_map: 0,
            uslp_qos: 0,
        },
        max_frames: 2,
        max_unit_len: 32,
    }
}

const fn input(kind: u32) -> aldis_cltu_packer_input_t {
    aldis_cltu_packer_input_t {
        kind,
        data: null(),
        data_len: 0,
        qos: ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
        repetitions: 1,
        truncated: 0,
        config: config(ALDIS_LINK_PROTOCOL_TC),
    }
}

fn init(
    s: &mut aldis_cltu_packer_t,
    cfg: &aldis_cltu_packer_config_t,
    d: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_cltu_packer_init(s, cfg, d) }
}

fn step(
    h: *mut aldis_cltu_packer_t,
    input: &aldis_cltu_packer_input_t,
    out: &mut [u8],
    actions: &mut aldis_cltu_packer_actions_t,
    err: &mut aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_cltu_packer_step(h, input, out.as_mut_ptr(), out.len(), actions, err) }
}

/// An `ADD_FRAME` input of `frame`.
const fn add(frame: &[u8]) -> aldis_cltu_packer_input_t {
    let mut i = input(ALDIS_CLTU_PACKER_INPUT_ADD_FRAME);
    i.data = frame.as_ptr();
    i.data_len = frame.len();
    i
}

#[test]
fn capacities_are_published_and_match_the_engine() {
    // req: CABI-030, CABI-031, LNK-074
    assert_eq!(
        ALDIS_CLTU_PACKER_MAX_ACTIONS,
        aldis::link::cltu::CLTU_PACKER_MAX_ACTIONS
    );
    assert_eq!(
        size_of::<aldis_cltu_packer_t>(),
        ALDIS_CLTU_PACKER_WORDS * 8
    );
    assert_eq!(align_of::<aldis_cltu_packer_t>(), 8);
}

#[test]
fn the_storage_is_checked_and_tagged() {
    // req: CABI-031, CABI-032, CABI-022
    let cfg = config(ALDIS_LINK_PROTOCOL_TC);
    let mut d = detail();
    let mut s = storage();
    let mut status = aldis_cltu_packer_status_t::default();
    // SAFETY: every pointer is NULL, to a live local, or a deliberately misaligned address of one
    // that is rejected before any access.
    unsafe {
        assert_eq!(
            aldis_cltu_packer_init(null_mut(), &raw const cfg, &raw mut d),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(d.kind, ALDIS_ERR_INVALID_FIELD);
        let odd = (&raw mut *s).wrapping_byte_add(4);
        assert_eq!(
            aldis_cltu_packer_init(odd, &raw const cfg, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_cltu_packer_init(&raw mut *s, null(), null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_cltu_packer_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        assert_eq!(
            aldis_cltu_packer_status(null_mut(), &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        assert_eq!(
            aldis_cltu_packer_init(&raw mut *s, &raw const cfg, null_mut()),
            0
        );
        assert_eq!(
            aldis_cltu_packer_status(&raw mut *s, &raw mut status, null_mut()),
            0
        );
        assert_eq!(status, aldis_cltu_packer_status_t::default());
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
            aldis_cltu_packer_status(&raw mut *copy, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
        // The status output must not overlap the storage.
        let inside = (&raw mut *s).cast::<aldis_cltu_packer_status_t>();
        assert_eq!(
            aldis_cltu_packer_status(&raw mut *s, inside, null_mut()),
            ALDIS_ERR_INVALID_FIELD
        );
        aldis_cltu_packer_deinit(&raw mut *s);
        aldis_cltu_packer_deinit(null_mut());
        assert_eq!(
            aldis_cltu_packer_status(&raw mut *s, &raw mut status, null_mut()),
            ALDIS_ERR_INVALID_HANDLE
        );
    }
}

#[test]
fn the_protocol_is_chosen_in_the_configuration() {
    // req: LNK-001, CABI-040, LNK-044, LNK-078, API-040
    let mut s = storage();
    let mut d = detail();
    for protocol in [ALDIS_LINK_PROTOCOL_TC, ALDIS_LINK_PROTOCOL_USLP] {
        assert_eq!(init(&mut s, &config(protocol), &mut d), 0, "{protocol}");
    }
    for protocol in [ALDIS_LINK_PROTOCOL_TM, ALDIS_LINK_PROTOCOL_AOS] {
        assert_eq!(
            init(&mut s, &config(protocol), &mut d),
            ALDIS_ERR_INVALID_CONFIG
        );
        assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    }
    assert_eq!(init(&mut s, &config(9), &mut d), ALDIS_ERR_INVALID_FIELD);
    let mut cfg = config(ALDIS_LINK_PROTOCOL_TC);
    cfg.max_frames = 0;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAX_FRAMES_PER_UNIT);
    cfg = config(ALDIS_LINK_PROTOCOL_TC);
    cfg.max_unit_len = 0;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_INVALID_CONFIG);
    assert_eq!(d.field, ALDIS_FIELD_MAX_UNIT_LEN);
    cfg.max_unit_len = ALDIS_LINK_MAX_FRAME_OCTETS + 1;
    assert_eq!(init(&mut s, &cfg, &mut d), ALDIS_ERR_CAPACITY_EXCEEDED);
    assert_eq!(d.field, ALDIS_FIELD_MAX_UNIT_LEN);
    cfg.max_unit_len = ALDIS_LINK_MAX_FRAME_OCTETS;
    assert_eq!(init(&mut s, &cfg, &mut d), 0);
}

#[test]
fn the_packer_steps_and_reconfigures() {
    // req: CABI-040, LNK-003, LNK-044, LNK-075, API-058
    let mut s = storage();
    let mut d = detail();
    let mut actions = aldis_cltu_packer_actions_t::default();
    let mut out = [0_u8; 64];
    assert_eq!(init(&mut s, &config(ALDIS_LINK_PROTOCOL_TC), &mut d), 0);
    // Nothing held: a flush gets nothing.
    let flush = input(ALDIS_CLTU_PACKER_INPUT_FLUSH);
    assert_eq!(step(&raw mut *s, &flush, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 0);
    // One frame is held; the second fills the unit (two frames), which is returned at once.
    let a = [0xA1_u8; 10];
    let b = [0xB2_u8; 12];
    assert_eq!(
        step(&raw mut *s, &add(&a), &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 0);
    let mut status = aldis_cltu_packer_status_t::default();
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_cltu_packer_status(&mut *s, &mut status, &mut d) };
    assert_eq!(status.pending.current, 10);
    assert_eq!(
        step(&raw mut *s, &add(&b), &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    let u = actions.items[0];
    assert_eq!(u.kind, ALDIS_CLTU_PACKER_ACTION_DATA_UNIT);
    assert_eq!((u.frames, u.repetitions, u.truncated), (2, 1, 0));
    assert_eq!(&out[u.unit.offset..u.unit.offset + 10], a);
    assert_eq!(&out[u.unit.offset + 10..u.unit.offset + u.unit.len], b);
    // A frame of the expedited service asks for no repetition, whatever the caller says.
    let mut x = add(&a);
    x.qos = ALDIS_LINK_QOS_EXPEDITED;
    x.repetitions = 3;
    assert_eq!(step(&raw mut *s, &x, &mut out, &mut actions, &mut d), 0);
    assert_eq!(step(&raw mut *s, &flush, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.items[0].repetitions, 1);
    let mut r = add(&a);
    r.repetitions = 3;
    assert_eq!(step(&raw mut *s, &r, &mut out, &mut actions, &mut d), 0);
    assert_eq!(step(&raw mut *s, &flush, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.items[0].repetitions, 3);
    // Refusals leave the engine unchanged and the actions unwritten.
    actions = aldis_cltu_packer_actions_t {
        count: 99,
        ..aldis_cltu_packer_actions_t::default()
    };
    assert_eq!(
        step(&raw mut *s, &add(&[]), &mut out, &mut actions, &mut d),
        ALDIS_ERR_LENGTH_MISMATCH
    );
    assert_eq!(d.field, ALDIS_FIELD_FRAME_LEN);
    assert_eq!(
        step(&raw mut *s, &add(&[0; 33]), &mut out, &mut actions, &mut d),
        ALDIS_ERR_OUT_OF_RANGE
    );
    let mut t = add(&a);
    t.truncated = 1;
    assert_eq!(
        step(&raw mut *s, &t, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(d.field, ALDIS_FIELD_TRUNCATED_FRAME_LEN);
    t.truncated = 2;
    assert_eq!(
        step(&raw mut *s, &t, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    let mut bad = add(&a);
    bad.repetitions = 0;
    assert_eq!(
        step(&raw mut *s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    bad = add(&a);
    bad.qos = 5;
    assert_eq!(
        step(&raw mut *s, &bad, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        step(&raw mut *s, &input(99), &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    assert_eq!(
        step(&raw mut *s, &add(&a), &mut out[..40], &mut actions, &mut d),
        ALDIS_ERR_BUFFER_TOO_SMALL
    );
    assert_eq!(actions.count, 99);
    // The input data must not overlap the storage or `out`.
    let mut overlap = add(&a);
    overlap.data = (&raw const *s).cast::<u8>();
    assert_eq!(
        step(&raw mut *s, &overlap, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    // One raw pointer for both: a second borrow of `out` would end the first (Stacked Borrows).
    let out_ptr = out.as_mut_ptr();
    overlap.data = out_ptr.cast_const();
    // SAFETY: every pointer is to a live local; `data` is rejected before any access through it.
    let rc = unsafe {
        aldis_cltu_packer_step(
            &raw mut *s,
            &overlap,
            out_ptr,
            out.len(),
            &mut actions,
            &mut d,
        )
    };
    assert_eq!(rc, ALDIS_ERR_INVALID_FIELD);
    // A reconfiguration keeps the protocol and discards the unit in progress.
    assert_eq!(
        step(&raw mut *s, &add(&a), &mut out, &mut actions, &mut d),
        0
    );
    let mut reconf = input(ALDIS_CLTU_PACKER_INPUT_RECONFIGURE);
    reconf.config = config(ALDIS_LINK_PROTOCOL_USLP);
    assert_eq!(
        step(&raw mut *s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    reconf.config = config(ALDIS_LINK_PROTOCOL_TC);
    reconf.config.max_frames = 0;
    assert_eq!(
        step(&raw mut *s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_CONFIG
    );
    reconf.config = config(ALDIS_LINK_PROTOCOL_TC);
    reconf.config.channel.protocol = 9;
    assert_eq!(
        step(&raw mut *s, &reconf, &mut out, &mut actions, &mut d),
        ALDIS_ERR_INVALID_FIELD
    );
    reconf.config = config(ALDIS_LINK_PROTOCOL_TC);
    assert_eq!(
        step(&raw mut *s, &reconf, &mut out, &mut actions, &mut d),
        0
    );
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].kind, ALDIS_CLTU_PACKER_ACTION_DISCARDED);
    assert_eq!(actions.items[0].octets, 10);
    // SAFETY: every pointer is to a live local.
    unsafe { aldis_cltu_packer_status(&mut *s, &mut status, &mut d) };
    assert_eq!(status.discarded_octets, 10);
    assert_eq!(status.units_out, 3);
    assert!(status.refused > 0);
    assert_eq!(status.pending.current, 0);
}

#[test]
fn a_uslp_packer_keeps_truncated_frames_apart() {
    // req: LNK-044, CABI-040
    let mut s = storage();
    let mut d = detail();
    let mut actions = aldis_cltu_packer_actions_t::default();
    let mut out = [0_u8; 64];
    assert_eq!(init(&mut s, &config(ALDIS_LINK_PROTOCOL_USLP), &mut d), 0);
    let a = [0x11_u8; 8];
    assert_eq!(
        step(&raw mut *s, &add(&a), &mut out, &mut actions, &mut d),
        0
    );
    let mut t = add(&a);
    t.truncated = 1;
    // The truncated frame cannot join the unit: the unit in progress is returned first.
    assert_eq!(step(&raw mut *s, &t, &mut out, &mut actions, &mut d), 0);
    assert_eq!(actions.count, 1);
    assert_eq!(actions.items[0].truncated, 0);
    let flush = input(ALDIS_CLTU_PACKER_INPUT_FLUSH);
    assert_eq!(step(&raw mut *s, &flush, &mut out, &mut actions, &mut d), 0);
    assert_eq!(
        (actions.items[0].truncated, actions.items[0].frames),
        (1, 1)
    );
}

#[test]
fn the_members_of_the_channel_are_read_whatever_the_protocol() {
    // req: CABI-040, CABI-022, LNK-001
    type Set = fn(&mut aldis_link_channel_t);
    let mut s = storage();
    let mut d = detail();
    let sets: [(Set, u32); 4] = [
        (|c| c.uslp_qos = 99, ALDIS_ERR_INVALID_FIELD),
        (|c| c.aos_edition = 9, ALDIS_ERR_INVALID_FIELD),
        (|c| c.uslp_upid = 32, ALDIS_ERR_OUT_OF_RANGE),
        (|c| c.uslp_map = 16, ALDIS_ERR_OUT_OF_RANGE),
    ];
    for (set, want) in sets {
        for protocol in [ALDIS_LINK_PROTOCOL_TC, ALDIS_LINK_PROTOCOL_USLP] {
            let mut cfg = config(protocol);
            set(&mut cfg.channel);
            assert_eq!(init(&mut s, &cfg, &mut d), want);
        }
        assert_eq!(init(&mut s, &config(ALDIS_LINK_PROTOCOL_TC), &mut d), 0);
        let mut reconf = input(ALDIS_CLTU_PACKER_INPUT_RECONFIGURE);
        set(&mut reconf.config.channel);
        let mut actions = aldis_cltu_packer_actions_t::default();
        assert_eq!(
            step(&raw mut *s, &reconf, &mut [], &mut actions, &mut d),
            want
        );
    }
}
