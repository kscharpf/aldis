//! Scheduler, link model, and golden traces (docs/reference-application.md §6, §7; A0 WP-2).

use refapp_harness::scenario::{
    Call, Case, Direction, Execution, Fault, FaultEntry, Level, OpsEntry, Scenario,
    generated_files, refapp_dir,
};
use refapp_harness::sim;
use refapp_harness::trace::{Dir, FaultKind, NodeId, Record, check_canonical, compare};
use refapp_harness::{golden_path, run_case};
use refapp_scenario::{Apid, Instant, Node, Output, StepInput};

fn case(level: Level, end_us: u64) -> Case {
    Case {
        id: "test".to_owned(),
        level,
        end_us,
        trace_version: 1,
        requirements: vec![],
        ops: vec![],
        faults: vec![],
    }
}

/// A node driven by a closure of (`now`, delivered units, `tx_slot`).
struct Scripted<F>(F);

impl<F> Node for Scripted<F>
where
    F: FnMut(u64, &[&[u8]], bool, &mut dyn Output) -> Option<u64>,
{
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        (self.0)(input.now.as_micros(), input.delivered, input.tx_slot, out)
            .map(Instant::from_micros)
    }
}

const fn apid(v: u16) -> Apid {
    Apid::new(v).unwrap()
}

#[test]
fn empty_case_matches_its_golden_trace() {
    // req: APP-080, APP-081, APP-085, APP-087
    let trace = run_case("a0-empty").unwrap();
    check_canonical(&trace).unwrap();
    assert_eq!(trace.lines().count(), 1, "header only");
    let golden = std::fs::read_to_string(golden_path("a0-empty")).unwrap();
    compare(&golden, &trace).unwrap();
}

#[test]
fn every_golden_trace_is_canonical_and_reproduced() {
    // req: APP-080, APP-085, APP-087
    let scenario = Scenario::load().unwrap();
    for c in &scenario.cases {
        let golden = std::fs::read_to_string(golden_path(&c.id)).unwrap();
        check_canonical(&golden).unwrap();
        compare(&golden, &run_case(&c.id).unwrap()).unwrap();
    }
}

#[test]
fn generated_files_are_current() {
    // req: APP-107
    for (path, body) in generated_files().unwrap() {
        let on_disk = std::fs::read_to_string(refapp_dir().join(path)).unwrap();
        assert!(
            on_disk == body,
            "refapp/{path} is stale: run `cargo run -p refapp-harness -- gen-scenario`"
        );
    }
}

#[test]
fn packet_level_delay_and_tie_order() {
    // req: APP-071, APP-072, APP-073
    let scenario = Scenario::load().unwrap();
    // Both nodes send one unit at t = 0; both arrive at the same instant.
    let mut flight = Scripted(
        |t: u64, delivered: &[&[u8]], _tx: bool, out: &mut dyn Output| {
            if t == 0 {
                out.packet_out(apid(16), &[0xF0]);
                out.unit(&[0xF0]);
            }
            for d in delivered {
                out.packet_in(apid(128), d);
            }
            None
        },
    );
    let mut ground = Scripted(
        |t: u64, delivered: &[&[u8]], _tx: bool, out: &mut dyn Output| {
            if t == 0 {
                out.unit(&[0x0A]);
            }
            for d in delivered {
                out.packet_in(apid(16), d);
                out.app_event(1, &[16, 0]);
            }
            None
        },
    );
    let records = sim::records(
        &scenario,
        &case(Level::Packets, 1_000_000),
        &mut flight,
        &mut ground,
    )
    .unwrap();
    let delay = scenario.link.one_way_delay_us;
    let expected = vec![
        // t = 0: ground steps before flight (§6.2).
        Record::UnitSent {
            t_us: 0,
            dir: Dir::Up,
            index: 0,
            octets: vec![0x0A],
        },
        Record::PacketOut {
            t_us: 0,
            node: NodeId::Flight,
            apid: 16,
            octets: vec![0xF0],
        },
        Record::UnitSent {
            t_us: 0,
            dir: Dir::Down,
            index: 0,
            octets: vec![0xF0],
        },
        // Arrivals with equal send instants: downlink before uplink.
        Record::UnitDelivered {
            t_us: delay,
            dir: Dir::Down,
            index: 0,
            copy: 0,
        },
        Record::UnitDelivered {
            t_us: delay,
            dir: Dir::Up,
            index: 0,
            copy: 0,
        },
        Record::PacketIn {
            t_us: delay,
            node: NodeId::Ground,
            apid: 16,
            octets: vec![0xF0],
        },
        Record::AppEvent {
            t_us: delay,
            node: NodeId::Ground,
            code: 1,
            args: vec![16, 0],
        },
        Record::PacketIn {
            t_us: delay,
            node: NodeId::Flight,
            apid: 128,
            octets: vec![0x0A],
        },
    ];
    assert_eq!(records, expected);
}

#[test]
fn deadlines_wake_nodes() {
    // req: APP-073
    let scenario = Scenario::load().unwrap();
    let mut flight = Scripted(|t: u64, _d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
        out.packet_out(apid(16), &[u8::try_from(t / 250_000).unwrap()]);
        (t < 500_000).then_some(t + 250_000)
    });
    let mut ground = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _out: &mut dyn Output| None);
    let records = sim::records(
        &scenario,
        &case(Level::Packets, 1_000_000),
        &mut flight,
        &mut ground,
    )
    .unwrap();
    let times: Vec<u64> = records.iter().map(Record::t_us).collect();
    assert_eq!(times, [0, 250_000, 500_000]);
}

#[test]
fn frame_level_slots() {
    // req: APP-071, APP-090
    let scenario = Scenario::load().unwrap();
    let frame = scenario.link.frame_slot_us;
    let uplink = scenario.link.uplink_slot_us;
    let mut flight = Scripted(|_t: u64, _d: &[&[u8]], tx: bool, out: &mut dyn Output| {
        if tx {
            out.unit(&[0xAA]);
        }
        None
    });
    let mut ground = Scripted(|t: u64, _d: &[&[u8]], tx: bool, out: &mut dyn Output| {
        if tx && t == 0 {
            out.unit(&[0xBB]);
        }
        None
    });
    let end = 4 * frame;
    let records = sim::records(
        &scenario,
        &case(Level::Frames, end),
        &mut flight,
        &mut ground,
    )
    .unwrap();
    let down: Vec<u64> = records
        .iter()
        .filter_map(|r| match r {
            Record::UnitSent {
                t_us,
                dir: Dir::Down,
                ..
            } => Some(*t_us),
            _ => None,
        })
        .collect();
    assert_eq!(down, [0, frame, 2 * frame, 3 * frame]);
    let up = records
        .iter()
        .filter(|r| matches!(r, Record::UnitSent { dir: Dir::Up, .. }))
        .count();
    assert_eq!(up, 1);
    assert!(
        uplink.is_multiple_of(frame),
        "slot model assumes aligned slots"
    );
}

#[test]
fn step_contract_violations_fail_the_run() {
    // req: APP-090
    let scenario = Scenario::load().unwrap();
    // A flight node that sends nothing in a frame slot (LNK-030).
    let mut silent = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    let mut ground = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    assert!(sim::records(&scenario, &case(Level::Frames, 1), &mut silent, &mut ground).is_err());
    // A ground node that sends two units in one uplink slot.
    let mut flight = Scripted(|_t: u64, _d: &[&[u8]], tx: bool, out: &mut dyn Output| {
        if tx {
            out.unit(&[1]);
        }
        None
    });
    let mut chatty = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
        out.unit(&[1]);
        out.unit(&[2]);
        None
    });
    assert!(sim::records(&scenario, &case(Level::Frames, 1), &mut flight, &mut chatty).is_err());
    // A deadline that is not after now.
    let mut stuck = Scripted(|t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| Some(t));
    let mut ground = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    assert!(
        sim::records(
            &scenario,
            &case(Level::Packets, 10),
            &mut stuck,
            &mut ground
        )
        .is_err()
    );
}

/// A node that also records the ops indices it is given.
struct Ops<F>(F);

impl<F> Node for Ops<F>
where
    F: FnMut(u64, &[u16], &[&[u8]], &mut dyn Output) -> Option<u64>,
{
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        (self.0)(input.now.as_micros(), input.ops, input.delivered, out).map(Instant::from_micros)
    }
}

const fn noop_at(at_us: u64) -> OpsEntry {
    OpsEntry::Command {
        at_us,
        execution: Execution::Immediate {},
        call: Call::Noop {},
        extra_octets: None,
    }
}

const fn fault(dir: Direction, index: u64, fault: Fault) -> FaultEntry {
    FaultEntry { dir, index, fault }
}

#[test]
fn ops_entries_wake_the_ground_node_when_due() {
    // req: APP-073
    let scenario = Scenario::load().unwrap();
    let mut c = case(Level::Packets, 5_000_000);
    // Two entries at one instant, one later; the ground node asks for no deadline.
    c.ops = vec![noop_at(1_005_000), noop_at(1_005_000), noop_at(2_500_000)];
    let seen = std::cell::RefCell::new(Vec::new());
    let mut ground = Ops(|t: u64, ops: &[u16], _d: &[&[u8]], out: &mut dyn Output| {
        if !ops.is_empty() {
            seen.borrow_mut().push((t, ops.to_vec()));
            out.packet_out(apid(128), &[0]);
        }
        None
    });
    let mut flight = Ops(|_t: u64, ops: &[u16], _d: &[&[u8]], _o: &mut dyn Output| {
        assert!(ops.is_empty(), "only the ground node has an ops script");
        None
    });
    let records = sim::records(&scenario, &c, &mut flight, &mut ground).unwrap();
    assert_eq!(
        *seen.borrow(),
        [(1_005_000, vec![0, 1]), (2_500_000, vec![2])]
    );
    let times: Vec<u64> = records.iter().map(Record::t_us).collect();
    assert_eq!(times, [1_005_000, 2_500_000]);
}

#[test]
fn scripted_faults_reproduce_a_hand_written_trace() {
    // req: APP-072, APP-075, APP-082
    let scenario = Scenario::load().unwrap();
    let mut c = case(Level::Packets, 6_000_000);
    c.faults = vec![
        fault(Direction::Up, 0, Fault::Drop {}),
        fault(Direction::Down, 0, Fault::Drop {}),
        fault(Direction::Down, 1, Fault::Duplicate {}),
        fault(Direction::Down, 2, Fault::Delay { delay_us: 500_000 }),
        fault(Direction::Down, 3, Fault::Truncate { octets: 1 }),
    ];
    // The flight node sends [i, 0xAA] at i seconds, i = 0 to 4.
    let mut flight = Scripted(|t: u64, _d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
        if t.is_multiple_of(1_000_000) {
            out.unit(&[u8::try_from(t / 1_000_000).unwrap(), 0xAA]);
        }
        (t < 4_000_000).then(|| (t / 1_000_000 + 1) * 1_000_000)
    });
    let mut ground = Scripted(|t: u64, d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
        if t == 0 {
            out.unit(&[0x0A]);
        }
        for u in d {
            out.packet_in(apid(16), u);
        }
        None
    });
    let records = sim::records(&scenario, &c, &mut flight, &mut ground).unwrap();
    let delay = scenario.link.one_way_delay_us;
    let sent = |t_us: u64, dir: Dir, index: u64, octets: &[u8]| Record::UnitSent {
        t_us,
        dir,
        index,
        octets: octets.to_vec(),
    };
    let flt = |t_us: u64, dir: Dir, index: u64, fault: FaultKind, args: &[u64]| Record::Fault {
        t_us,
        dir,
        index,
        fault,
        args: args.to_vec(),
    };
    let deliv = |t_us: u64, index: u64, copy: u8| Record::UnitDelivered {
        t_us,
        dir: Dir::Down,
        index,
        copy,
    };
    let pkt = |t_us: u64, octets: &[u8]| Record::PacketIn {
        t_us,
        node: NodeId::Ground,
        apid: 16,
        octets: octets.to_vec(),
    };
    let s = 1_000_000;
    let expected = vec![
        // t = 0: ground first; both units dropped, so nothing arrives.
        flt(0, Dir::Up, 0, FaultKind::Drop, &[]),
        sent(0, Dir::Up, 0, &[0x0A]),
        flt(0, Dir::Down, 0, FaultKind::Drop, &[]),
        sent(0, Dir::Down, 0, &[0, 0xAA]),
        // Duplicate: two copies at the same instant.
        flt(s, Dir::Down, 1, FaultKind::Duplicate, &[]),
        sent(s, Dir::Down, 1, &[1, 0xAA]),
        deliv(s + delay, 1, 0),
        deliv(s + delay, 1, 1),
        pkt(s + delay, &[1, 0xAA]),
        pkt(s + delay, &[1, 0xAA]),
        // Delay: arrives 500 000 µs late.
        flt(2 * s, Dir::Down, 2, FaultKind::Delay, &[500_000]),
        sent(2 * s, Dir::Down, 2, &[2, 0xAA]),
        deliv(2 * s + delay + 500_000, 2, 0),
        pkt(2 * s + delay + 500_000, &[2, 0xAA]),
        // Truncate: unit_sent keeps the octets as sent, the delivery is cut.
        flt(3 * s, Dir::Down, 3, FaultKind::Truncate, &[1]),
        sent(3 * s, Dir::Down, 3, &[3, 0xAA]),
        deliv(3 * s + delay, 3, 0),
        pkt(3 * s + delay, &[3]),
        sent(4 * s, Dir::Down, 4, &[4, 0xAA]),
        deliv(4 * s + delay, 4, 0),
        pkt(4 * s + delay, &[4, 0xAA]),
    ];
    assert_eq!(records, expected);
    // The records are canonical lines in the key order of §7.3.
    let mut text = String::new();
    for r in &records {
        text.push_str(&r.to_line().unwrap());
    }
    assert!(
        text.contains(r#""kind":"fault","dir":"down","index":2,"fault":"delay","args":[500000]"#)
    );
}

#[test]
fn a_delay_reorders_units() {
    // req: APP-072, APP-073, APP-075
    let scenario = Scenario::load().unwrap();
    let mut c = case(Level::Packets, 3_000_000);
    c.faults = vec![fault(
        Direction::Down,
        0,
        Fault::Delay {
            delay_us: 1_000_000,
        },
    )];
    let mut flight = Scripted(|t: u64, _d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
        if t <= 100_000 {
            out.unit(&[u8::try_from(t / 100_000).unwrap()]);
        }
        (t < 100_000).then_some(t + 100_000)
    });
    let mut ground = Scripted(|_t: u64, d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
        for u in d {
            out.packet_in(apid(16), u);
        }
        None
    });
    let records = sim::records(&scenario, &c, &mut flight, &mut ground).unwrap();
    let arrivals: Vec<u8> = records
        .iter()
        .filter_map(|r| match r {
            Record::PacketIn { octets, .. } => octets.first().copied(),
            _ => None,
        })
        .collect();
    assert_eq!(arrivals, [1, 0]);
}

#[test]
fn faults_that_cannot_apply_fail_the_run() {
    // req: APP-075
    let scenario = Scenario::load().unwrap();
    let mut silent = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    let mut ground = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    // A unit that is never sent.
    let mut c = case(Level::Packets, 1_000_000);
    c.faults = vec![fault(Direction::Down, 3, Fault::Drop {})];
    assert!(sim::records(&scenario, &c, &mut silent, &mut ground).is_err());
    // `flip` before the frame levels.
    let mut sender = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, o: &mut dyn Output| {
        o.unit(&[1]);
        None
    });
    c.faults = vec![fault(Direction::Down, 0, Fault::Flip { octet: 0, bit: 0 })];
    assert!(sim::records(&scenario, &c, &mut sender, &mut ground).is_err());
}

#[test]
fn extra_wake_ups_do_not_change_the_trace() {
    // req: APP-074
    let scenario = Scenario::load().unwrap();
    let c = case(Level::Packets, 1_000_000);
    let run = |wake_every: Option<u64>| {
        // The flight node sends at t = 0; the ground node echoes what it receives. Both only
        // act on what is due, and may ask for extra wake-ups.
        let mut flight = Scripted(|t: u64, _d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
            if t == 0 {
                out.unit(&[7]);
            }
            wake_every.map(|p| (t / p + 1) * p)
        });
        let mut ground = Scripted(|t: u64, d: &[&[u8]], _tx: bool, out: &mut dyn Output| {
            for u in d {
                out.packet_in(apid(16), u);
                out.unit(u);
            }
            wake_every.map(|p| (t / p + 1) * p)
        });
        sim::records(&scenario, &c, &mut flight, &mut ground).unwrap()
    };
    let quiet = run(None);
    assert_eq!(quiet.len(), 5);
    assert_eq!(run(Some(50_000)), quiet);
}

#[test]
fn a_decreasing_release_instant_fails_the_run() {
    // req: APP-126
    let scenario = Scenario::load().unwrap();
    let mut c = case(Level::Packets, 5_000_000);
    c.ops = vec![noop_at(2_000_000), noop_at(1_000_000)];
    let mut flight = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    let mut ground = Scripted(|_t: u64, _d: &[&[u8]], _tx: bool, _o: &mut dyn Output| None);
    assert!(sim::records(&scenario, &c, &mut flight, &mut ground).is_err());
}
