//! The `CltuPacker` against cases derived by hand from CCSDS 232.0-B-4 and 732.1-B-3 (LNK-044,
//! LNK-003, LNK-074, LNK-075, LNK-078, API-052, API-058, TV-020 hand-derived).
//!
//! **Derivation.** The rules are those of 232.0 §4.3.8.4 ("each data unit ... shall consist of one
//! or more Transfer Frames in accordance with parameters set by management": Table 5-1, Maximum
//! Number of Transfer Frames Given to the Coding Sublayer as a Single Data Unit, Maximum Length of
//! Data Unit), §4.3.8.5 (systematic retransmissions are not requested "unless the data unit
//! contains one or more frames carrying service data on the Type-B Service"), §2.4.2 (the unit is
//! transferred the number of times given by Repetitions), and of 732.1 §4.2.11.7–8, Table 5-1, and
//! Annex D, D1.2.5 ("Truncated and non-truncated USLP Transfer Frames shall not be mixed together
//! within the same CLTU"). Each case below states the frames given and the units the rules give.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_link::cltu::{
    CLTU_PACKER_MAX_ACTIONS, CltuInput, CltuOutput, CltuPacker, CltuPackerConfig, DataUnit,
    FrameIn, Repetitions,
};
use aldis_link::common::Qos;
use aldis_link::protocol::{Tc, Uslp};

const SC: Qos = Qos::SequenceControlled;
const EXP: Qos = Qos::Expedited;

/// At most 3 frames, 50 octets: the units are written to an area of 100 octets.
type TcPacker = CltuPacker<Tc, 64>;
type UslpPacker = CltuPacker<Uslp, 64>;

const fn rep(n: u16) -> Repetitions {
    Repetitions::new(n).unwrap()
}

fn config<P: aldis_link::cltu::CltuProtocol>(frames: usize, len: usize) -> CltuPackerConfig<P> {
    CltuPackerConfig::<P>::new(frames, len).unwrap()
}

fn frame(
    len: usize,
    fill: u8,
    qos: Qos,
    repetitions: u16,
    truncated: bool,
) -> (Vec<u8>, Qos, u16, bool) {
    (vec![fill; len], qos, repetitions, truncated)
}

/// The units a step returned, with their octets.
fn add<P: aldis_link::cltu::CltuProtocol, const U: usize>(
    p: &mut CltuPacker<P, U>,
    f: &(Vec<u8>, Qos, u16, bool),
) -> Result<Vec<(DataUnit, Vec<u8>)>, aldis_core::Error> {
    let mut out = [0xEE_u8; 128];
    let input = CltuInput::AddFrame(FrameIn {
        frame: &f.0,
        qos: f.1,
        repetitions: rep(f.2),
        truncated: f.3,
    });
    let actions = p.step(input, &mut out)?;
    Ok(actions
        .iter()
        .map(|o| match o {
            CltuOutput::DataUnit(u) => (*u, u.unit.slice(&out).unwrap().to_vec()),
            other @ CltuOutput::Discarded { .. } => panic!("{other:?}"),
        })
        .collect())
}

fn flush<P: aldis_link::cltu::CltuProtocol, const U: usize>(
    p: &mut CltuPacker<P, U>,
) -> Vec<(DataUnit, Vec<u8>)> {
    let mut out = [0xEE_u8; 128];
    p.step(CltuInput::Flush, &mut out)
        .unwrap()
        .iter()
        .map(|o| match o {
            CltuOutput::DataUnit(u) => (*u, u.unit.slice(&out).unwrap().to_vec()),
            other @ CltuOutput::Discarded { .. } => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_full_unit_is_returned_at_once() {
    // Given: Maximum Number of Transfer Frames 3, frames of 10, 20, and 15 octets, all Type-AD with
    // Repetitions 1 (232.0 Table 5-1). The first two stay in the unit in progress; the third
    // makes the third frame of the unit, which is full (§4.3.8.4): the unit of 45 octets is
    // returned by the same step. A fourth frame starts the next unit.
    // req: LNK-044
    let mut p = TcPacker::new(config(3, 50)).unwrap();
    assert_eq!(
        add(&mut p, &frame(10, 1, SC, 1, false)).unwrap(),
        Vec::new()
    );
    assert_eq!(
        add(&mut p, &frame(20, 2, SC, 1, false)).unwrap(),
        Vec::new()
    );
    assert_eq!(p.status().pending.current(), 30);
    let units = add(&mut p, &frame(15, 3, SC, 1, false)).unwrap();
    assert_eq!(units.len(), 1);
    let (u, octets) = &units[0];
    assert_eq!((u.frames, u.repetitions.get(), u.truncated), (3, 1, false));
    let want: Vec<u8> = [vec![1; 10], vec![2; 20], vec![3; 15]].concat();
    assert_eq!(octets, &want);
    assert_eq!((u.unit.start(), u.unit.len()), (0, 45));
    assert_eq!(p.status().pending.current(), 0);
    assert_eq!(add(&mut p, &frame(4, 4, SC, 1, false)).unwrap(), Vec::new());
    let units = flush(&mut p);
    assert_eq!(units[0].1, vec![4; 4]);
    assert_eq!(flush(&mut p), Vec::new());
    assert_eq!(p.status().units_out.get(), 2);
    assert_eq!(p.status().frames_in.get(), 4);
}

#[test]
fn a_frame_that_does_not_fit_starts_the_next_unit() {
    // Given: Maximum Length of Data Unit 50, frames of 30 and 30 octets. 30 + 30 = 60 > 50
    // (§4.3.8.4, Table 5-1): the second frame does not fit; the pending unit of 30 octets is
    // returned, and the second frame is pending in the next unit.
    // req: LNK-044
    let mut p = TcPacker::new(config(3, 50)).unwrap();
    add(&mut p, &frame(30, 1, SC, 1, false)).unwrap();
    let units = add(&mut p, &frame(30, 2, SC, 1, false)).unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!((units[0].0.frames, units[0].1.clone()), (1, vec![1; 30]));
    assert_eq!(p.status().pending.current(), 30);
    assert_eq!(flush(&mut p)[0].1, vec![2; 30]);
    // A frame of exactly the remaining room fits, and fills the unit: returned at once.
    add(&mut p, &frame(30, 3, SC, 1, false)).unwrap();
    let units = add(&mut p, &frame(20, 4, SC, 1, false)).unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!((units[0].0.frames, units[0].0.unit.len()), (2, 50));
}

#[test]
fn the_worst_case_is_two_units() {
    // Given: Maximum Number of Transfer Frames 2, Maximum Length of Data Unit 10, a pending frame
    // of 6 octets, and a frame of 10. 6 + 10 > 10: the pending unit is returned. The new frame is
    // the whole of a unit of 10 octets, which is full. Two units, 16 octets of 2·U = 20.
    // req: LNK-044, LNK-074, LNK-075
    let mut p = CltuPacker::<Uslp, 10>::new(config(2, 10)).unwrap();
    let mut out = [0_u8; 20];
    let one = FrameIn {
        frame: &[1; 6],
        qos: SC,
        repetitions: Repetitions::ONE,
        truncated: false,
    };
    assert!(
        p.step(CltuInput::AddFrame(one), &mut out)
            .unwrap()
            .is_empty()
    );
    let two = FrameIn {
        frame: &[2; 10],
        ..one
    };
    let actions = p.step(CltuInput::AddFrame(two), &mut out).unwrap();
    assert_eq!(CLTU_PACKER_MAX_ACTIONS, 2);
    assert_eq!(actions.len(), CLTU_PACKER_MAX_ACTIONS);
    let units: Vec<DataUnit> = actions
        .iter()
        .map(|o| match o {
            CltuOutput::DataUnit(u) => *u,
            other @ CltuOutput::Discarded { .. } => panic!("{other:?}"),
        })
        .collect();
    assert_eq!((units[0].unit.start(), units[0].unit.len()), (0, 6));
    assert_eq!((units[1].unit.start(), units[1].unit.len()), (6, 10));
    assert_eq!(units[0].unit.slice(&out), Some(&[1_u8; 6][..]));
    assert_eq!(units[1].unit.slice(&out), Some(&[2_u8; 10][..]));
    // The area is checked before anything changes: 19 octets are not 2·U.
    let mut short = [0_u8; 19];
    let r = p.step(CltuInput::AddFrame(one), &mut short);
    assert_eq!(r.err().map(|e| e.kind()), Some(ErrorKind::BufferTooSmall));
    assert_eq!(p.status().frames_in.get(), 2);
    // Flush needs U.
    p.step(CltuInput::AddFrame(one), &mut out).unwrap();
    let r = p.step(CltuInput::Flush, &mut short[..9]);
    assert_eq!(r.err().map(|e| e.kind()), Some(ErrorKind::BufferTooSmall));
    assert_eq!(p.step(CltuInput::Flush, &mut short[..10]).unwrap().len(), 1);
}

#[test]
fn frames_of_different_repetitions_do_not_share_a_unit() {
    // Given: Type-AD frames with Repetitions 2, 2, 3, 1. The unit is transferred as many times as
    // Repetitions says (232.0 §2.4.2), so a unit holds one value: [2, 2] [3] [1].
    // req: LNK-044
    let mut p = TcPacker::new(config(8, 60)).unwrap();
    assert_eq!(add(&mut p, &frame(5, 1, SC, 2, false)).unwrap(), Vec::new());
    assert_eq!(add(&mut p, &frame(5, 2, SC, 2, false)).unwrap(), Vec::new());
    let units = add(&mut p, &frame(5, 3, SC, 3, false)).unwrap();
    assert_eq!((units[0].0.frames, units[0].0.repetitions.get()), (2, 2));
    let units = add(&mut p, &frame(5, 4, SC, 1, false)).unwrap();
    assert_eq!((units[0].0.frames, units[0].0.repetitions.get()), (1, 3));
    let units = flush(&mut p);
    assert_eq!((units[0].0.frames, units[0].0.repetitions.get()), (1, 1));
}

#[test]
fn a_unit_with_a_type_bd_frame_asks_for_no_repetition() {
    // Given: a Type-BD frame with Repetitions 5 (a VC value for Type-AD), then a Type-AD frame with
    // Repetitions 5, then a Type-BD frame with Repetitions 1. 232.0 §4.3.8.5: no repetition for a
    // unit with a Type-BD frame, so the BD frame is Repetitions 1, whatever it was given: it
    // cannot share a unit with the AD frame of Repetitions 5, and it can with another BD frame.
    // req: LNK-044
    let mut p = TcPacker::new(config(8, 60)).unwrap();
    assert_eq!(
        add(&mut p, &frame(5, 1, EXP, 5, false)).unwrap(),
        Vec::new()
    );
    let units = add(&mut p, &frame(5, 2, SC, 5, false)).unwrap();
    assert_eq!((units[0].0.frames, units[0].0.repetitions.get()), (1, 1));
    let units = add(&mut p, &frame(5, 3, EXP, 1, false)).unwrap();
    assert_eq!((units[0].0.frames, units[0].0.repetitions.get()), (1, 5));
    let units = add(&mut p, &frame(5, 4, EXP, 9, false)).unwrap();
    assert_eq!(units, Vec::new());
    let units = flush(&mut p);
    assert_eq!((units[0].0.frames, units[0].0.repetitions.get()), (2, 1));
    assert_eq!(units[0].1, [vec![3; 5], vec![4; 5]].concat());
    // A Type-AD frame with Repetitions 1 shares a unit with a Type-BD frame.
    add(&mut p, &frame(5, 5, SC, 1, false)).unwrap();
    assert_eq!(
        add(&mut p, &frame(5, 6, EXP, 3, false)).unwrap(),
        Vec::new()
    );
    assert_eq!(flush(&mut p)[0].0.frames, 2);
}

#[test]
fn truncated_and_non_truncated_usl_frames_do_not_share_a_unit() {
    // Given: a truncated frame, a truncated frame, a non-truncated frame, a truncated frame.
    // 732.1 Annex D, D1.2.5: not mixed within a CLTU: [t, t] [n] [t].
    // req: LNK-044
    let mut p = UslpPacker::new(config(8, 60)).unwrap();
    add(&mut p, &frame(4, 1, SC, 1, true)).unwrap();
    add(&mut p, &frame(4, 2, SC, 1, true)).unwrap();
    let units = add(&mut p, &frame(20, 3, SC, 1, false)).unwrap();
    assert_eq!((units[0].0.frames, units[0].0.truncated), (2, true));
    let units = add(&mut p, &frame(4, 4, SC, 1, true)).unwrap();
    assert_eq!((units[0].0.frames, units[0].0.truncated), (1, false));
    let units = flush(&mut p);
    assert_eq!((units[0].0.frames, units[0].0.truncated), (1, true));
    // TC has no truncated frames.
    let mut p = TcPacker::new(config(8, 60)).unwrap();
    let e = add(&mut p, &frame(4, 1, SC, 1, true)).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidField, Some(field::TRUNCATED_FRAME_LEN))
    );
    assert_eq!(p.status().refused.get(), 1);
}

#[test]
fn frames_and_configurations_are_checked() {
    // req: LNK-078, LNK-044, API-058
    let e = CltuPackerConfig::<Tc>::new(0, 10).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::MAX_FRAMES_PER_UNIT))
    );
    let e = CltuPackerConfig::<Uslp>::new(1, 0).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::MAX_UNIT_LEN))
    );
    let c = config::<Tc>(2, 65);
    assert_eq!((c.max_frames(), c.max_unit_len()), (2, 65));
    let e = TcPacker::new(c).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::MAX_UNIT_LEN))
    );
    assert_eq!(Repetitions::new(0), None);
    assert_eq!(Repetitions::ONE.get(), 1);
    let mut p = TcPacker::new(config(2, 20)).unwrap();
    assert_eq!(
        add(&mut p, &frame(0, 0, SC, 1, false)).map_err(|e| (e.kind(), e.field())),
        Err((ErrorKind::LengthMismatch, Some(field::FRAME_LEN)))
    );
    assert_eq!(
        add(&mut p, &frame(21, 0, SC, 1, false)).map_err(|e| (e.kind(), e.field())),
        Err((ErrorKind::OutOfRange, Some(field::MAX_UNIT_LEN)))
    );
    assert_eq!(p.status().refused.get(), 2);
    assert_eq!(p.status().frames_in.get(), 0);
    assert_eq!(add(&mut p, &frame(20, 1, SC, 1, false)).unwrap().len(), 1);
    assert_eq!(p.config().max_unit_len(), 20);
}

#[test]
fn reconfigure_discards_the_unit_in_progress() {
    // req: LNK-003, LNK-002
    let mut p = TcPacker::new(config(4, 40)).unwrap();
    add(&mut p, &frame(7, 1, SC, 1, false)).unwrap();
    add(&mut p, &frame(8, 2, SC, 1, false)).unwrap();
    let mut out = [0_u8; 128];
    let new = CltuInput::Reconfigure(config(2, 20));
    let a = p.step(new, &mut out).unwrap();
    assert_eq!(a.get(0), Some(&CltuOutput::Discarded { octets: 15 }));
    assert_eq!(p.status().discarded_octets.get(), 15);
    assert_eq!(p.status().pending.current(), 0);
    assert_eq!(p.status().pending.high_water(), 15);
    assert!(p.step(new, &mut out).unwrap().is_empty());
    assert_eq!(flush(&mut p), Vec::new());
    // A configuration that does not fit changes nothing.
    add(&mut p, &frame(3, 3, SC, 1, false)).unwrap();
    let r = p.step(CltuInput::Reconfigure(config(2, 65)), &mut out);
    assert_eq!(r.err().map(|e| e.kind()), Some(ErrorKind::CapacityExceeded));
    assert_eq!(p.status().pending.current(), 3);
}
