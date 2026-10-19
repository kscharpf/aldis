//! Scheduler, link model, and golden traces (docs/reference-application.md §6, §7; A0 WP-2).

use refapp_harness::scenario::{Case, Level, Scenario, generated_files, refapp_dir};
use refapp_harness::sim;
use refapp_harness::trace::{Dir, NodeId, Record, check_canonical, compare};
use refapp_harness::{golden_path, run_case};
use refapp_scenario::{Apid, Instant, Node, Output, StepInput};

fn case(level: Level, end_us: u64) -> Case {
    Case {
        id: "test".to_owned(),
        level,
        end_us,
        trace_version: 1,
        requirements: vec![],
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
