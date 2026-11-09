//! The harness at the frame level: scripted node pairs, the frame-level faults, and the strict
//! decoding of frames (docs/reference-application.md §6.1, §6.3; A2 WP-2).

use std::cell::RefCell;

use refapp_harness::frame::decode_frame;
use refapp_harness::micros::Micros;
use refapp_harness::ranged::Ranged;
use refapp_harness::scenario::{Case, Direction, Fault, FaultEntry, Level, Scenario};
use refapp_harness::sim;
use refapp_harness::trace::{Dir, FaultKind, Record};
use refapp_scenario::{Delivered, Instant, Node, Output, StepInput};

fn case(end_us: u64, faults: Vec<FaultEntry>) -> Case {
    Case {
        id: "test".to_owned(),
        level: Level::Frames,
        end_us: Micros::new(end_us),
        trace_version: Ranged::new(1).unwrap(),
        requirements: vec![],
        ops: vec![],
        faults,
    }
}

/// A node driven by a closure of (`now`, delivered units, `tx_slot`).
struct Scripted<F>(F);

impl<F> Node for Scripted<F>
where
    F: FnMut(u64, Delivered<'_>, bool, &mut dyn Output),
{
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        (self.0)(input.now.as_micros(), input.delivered, input.tx_slot, out);
        None
    }
}

const fn fault(dir: Direction, index: u64, fault: Fault) -> FaultEntry {
    FaultEntry { dir, index, fault }
}

/// The unit `index` of the downlink script: four octets.
const fn unit(index: u8) -> [u8; 4] {
    [0x10 + index, 0xAA, 0xBB, 0xCC]
}

#[expect(
    clippy::too_many_lines,
    reason = "one hand-written expected trace, easier to review in one piece"
)]
#[test]
fn a_node_pair_with_every_fault_kind_reproduces_a_hand_written_trace() {
    // req: APP-071, APP-075, APP-090
    let scenario = Scenario::load().unwrap();
    assert_eq!(scenario.link.frame_slot_us.get().get(), 125_000);
    assert_eq!(scenario.link.uplink_slot_us.get().get(), 250_000);
    assert_eq!(scenario.link.one_way_delay_us.get(), 10_000);
    let down = RefCell::new(Vec::new());
    let up = RefCell::new(Vec::new());
    let mut slot = 0u8;
    let mut flight = Scripted(
        |_t: u64, d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            up.borrow_mut().extend(d.into_iter().map(<[u8]>::to_vec));
            if tx {
                out.unit(&unit(slot));
                slot += 1;
            }
        },
    );
    let mut ground = Scripted(|t: u64, d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
        down.borrow_mut()
            .extend(d.into_iter().map(|u| (t, u.to_vec())));
        if tx && t == 0 {
            out.unit(&[0xC0]);
        }
    });
    let bit = |b| Ranged::new(b).unwrap();
    let faults = vec![
        fault(
            Direction::Up,
            0,
            Fault::Flip {
                octet: 0,
                bit: bit(7),
            },
        ),
        fault(Direction::Down, 0, Fault::Drop {}),
        fault(Direction::Down, 1, Fault::Duplicate {}),
        fault(
            Direction::Down,
            2,
            Fault::Delay {
                delay_us: Micros::new(30_000),
            },
        ),
        fault(
            Direction::Down,
            3,
            Fault::Flip {
                octet: 1,
                bit: bit(0),
            },
        ),
        fault(Direction::Down, 4, Fault::Truncate { octets: 2 }),
    ];
    let records =
        sim::records(&scenario, &case(700_000, faults), &mut flight, &mut ground).unwrap();
    let sent = |t_us: u64, dir, index, octets: &[u8]| Record::UnitSent {
        t_us: Micros::new(t_us),
        dir,
        index,
        octets: octets.to_vec(),
    };
    let fault_rec = |t_us: u64, dir, index, fault, args: &[u64]| Record::Fault {
        t_us: Micros::new(t_us),
        dir,
        index,
        fault,
        args: args.to_vec(),
    };
    let delivered = |t_us: u64, dir, index, copy| Record::UnitDelivered {
        t_us: Micros::new(t_us),
        dir,
        index,
        copy,
    };
    let expected = vec![
        // t = 0: the ground node steps first; each fault precedes its `unit_sent` (§7.3).
        fault_rec(0, Dir::Up, 0, FaultKind::Flip, &[0, 7]),
        sent(0, Dir::Up, 0, &[0xC0]),
        fault_rec(0, Dir::Down, 0, FaultKind::Drop, &[]),
        sent(0, Dir::Down, 0, &unit(0)),
        delivered(10_000, Dir::Up, 0, 0),
        fault_rec(125_000, Dir::Down, 1, FaultKind::Duplicate, &[]),
        sent(125_000, Dir::Down, 1, &unit(1)),
        delivered(135_000, Dir::Down, 1, 0),
        delivered(135_000, Dir::Down, 1, 1),
        fault_rec(250_000, Dir::Down, 2, FaultKind::Delay, &[30_000]),
        sent(250_000, Dir::Down, 2, &unit(2)),
        delivered(290_000, Dir::Down, 2, 0),
        fault_rec(375_000, Dir::Down, 3, FaultKind::Flip, &[1, 0]),
        sent(375_000, Dir::Down, 3, &unit(3)),
        delivered(385_000, Dir::Down, 3, 0),
        fault_rec(500_000, Dir::Down, 4, FaultKind::Truncate, &[2]),
        sent(500_000, Dir::Down, 4, &unit(4)),
        delivered(510_000, Dir::Down, 4, 0),
        sent(625_000, Dir::Down, 5, &unit(5)),
        delivered(635_000, Dir::Down, 5, 0),
    ];
    assert_eq!(records, expected);
    // The octets the ground node received: the flip inverts the most significant bit of
    // octet 1 (GEN-050), the truncation keeps two octets, the duplicate arrives twice.
    let mut flipped = unit(3);
    flipped[1] = 0x2A;
    let got: Vec<(u64, Vec<u8>)> = down.into_inner();
    assert_eq!(
        got,
        [
            (135_000, unit(1).to_vec()),
            (135_000, unit(1).to_vec()),
            (290_000, unit(2).to_vec()),
            (385_000, flipped.to_vec()),
            (510_000, vec![0x14, 0xAA]),
            (635_000, unit(5).to_vec()),
        ]
    );
    assert_eq!(up.into_inner(), [vec![0xC1]]);
}

#[test]
fn a_flip_outside_the_unit_fails_the_run() {
    // req: APP-075
    let scenario = Scenario::load().unwrap();
    let mut flight = Scripted(
        |_t: u64, _d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            if tx {
                out.unit(&[1, 2]);
            }
        },
    );
    let mut ground = Scripted(|_t: u64, _d: Delivered<'_>, _tx: bool, _o: &mut dyn Output| {});
    let flip = |octet| {
        fault(
            Direction::Down,
            0,
            Fault::Flip {
                octet,
                bit: Ranged::new(0).unwrap(),
            },
        )
    };
    let c = case(125_000, vec![flip(1)]);
    sim::records(&scenario, &c, &mut flight, &mut ground).unwrap();
    let c = case(125_000, vec![flip(2)]);
    let e = sim::records(&scenario, &c, &mut flight, &mut ground).unwrap_err();
    assert!(e.contains("flip"), "{e}");
}

/// CRC-16 of the FECF (132.0 Annex B, 232.0 Annex A): polynomial 0x1021, initial value 0xFFFF.
fn fecf(frame: &mut Vec<u8>) {
    let mut crc = 0xFFFFu16;
    for b in frame.iter() {
        crc ^= u16::from(*b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    frame.extend_from_slice(&crc.to_be_bytes());
}

/// An Only Idle Data TM frame of the scenario on virtual channel `vcid` (132.0 §4.1.2).
fn tm_oid_frame(vcid: u8) -> Vec<u8> {
    let mut f = vec![0x15, 0x51 | (vcid << 1), 0, 0, 0x1F, 0xFE];
    f.extend_from_slice(&[0x55; 211]);
    f.extend_from_slice(&[1, 0, 0, 0]);
    fecf(&mut f);
    f
}

/// A Type-BD TC frame with a Segment Header (232.0 §4.1.2, §4.1.3).
fn tc_frame(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(5 + 1 + data.len() + 2 - 1).unwrap();
    let mut f = vec![0x21, 0x55];
    f.extend_from_slice(&len.to_be_bytes());
    f.push(0);
    f.push(0xC0);
    f.extend_from_slice(data);
    fecf(&mut f);
    f
}

#[test]
fn frames_decode_by_direction_with_the_strict_codecs() {
    // req: APP-086
    let s = Scenario::load().unwrap();
    let tm = tm_oid_frame(7);
    let d = decode_frame(&s, Level::Frames, Dir::Down, &tm).unwrap();
    assert_eq!((d.vcid, d.data.len()), (7, 211));
    let tc = tc_frame(&[0x1A; 9]);
    let d = decode_frame(&s, Level::Frames, Dir::Up, &tc).unwrap();
    assert_eq!(d.vcid, 0);
    assert_eq!(d.data.last(), Some(&0x1A));
    // A wrong FECF, a wrong length, and a unit of the other direction fail.
    let mut bad = tm.clone();
    bad[100] ^= 1;
    assert!(decode_frame(&s, Level::Frames, Dir::Down, &bad).is_err());
    assert!(decode_frame(&s, Level::Frames, Dir::Down, &tm[..222]).is_err());
    let mut bad = tc;
    bad[8] ^= 1;
    assert!(decode_frame(&s, Level::Frames, Dir::Up, &bad).is_err());
    assert!(decode_frame(&s, Level::Frames, Dir::Up, &tm).is_err());
    // Another Spacecraft ID fails (APP-026).
    let mut other = tm.clone();
    other[1] ^= 0x10;
    other.truncate(221);
    fecf(&mut other);
    assert!(decode_frame(&s, Level::Frames, Dir::Down, &other).is_err());
    // A unit of a packet-level case is no frame.
    assert!(decode_frame(&s, Level::Packets, Dir::Down, &tm).is_err());
}

#[test]
fn a_tc_vc_without_a_segment_header_decodes_without_maps() {
    // req: APP-086
    let mut s = Scenario::load().unwrap();
    s.tc.segment_header = false;
    let d = decode_frame(&s, Level::Frames, Dir::Up, &tc_frame(&[0x1A; 9])).unwrap();
    // The Segment Header octet is data now.
    assert_eq!((d.vcid, d.data.len()), (0, 10));
}
