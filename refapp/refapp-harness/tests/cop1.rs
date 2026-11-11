//! The harness at the link level `cop1`: scripted node pairs, the frame rules at the new level,
//! the decoding of Type-AD and Type-BC frames, and the `fop1_directive` entry (A3 WP-2;
//! docs/reference-application.md §4.5, §6.1, §6.5).

use std::cell::RefCell;

use refapp_harness::frame::{FramedPackets, decode_frame};
use refapp_harness::micros::Micros;
use refapp_harness::ranged::Ranged;
use refapp_harness::scenario::{
    Call, Case, Direction, Execution, Fault, FaultEntry, Fop1Directive, Level, OpsEntry, Scenario,
};
use refapp_harness::sim;
use refapp_harness::trace::{Dir, FaultKind, NodeId, Record};
use refapp_scenario::{Delivered, Instant, Node, Output, StepInput};

use aldis::tc::{ControlCommand, FrameType};

fn case(level: Level, end_us: u64, ops: Vec<OpsEntry>, faults: Vec<FaultEntry>) -> Case {
    Case {
        id: "test".to_owned(),
        level,
        end_us: Micros::new(end_us),
        trace_version: Ranged::new(1).unwrap(),
        requirements: vec![],
        ops,
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

/// CRC-16 of the FECF (232.0 Annex A): polynomial 0x1021, initial value 0xFFFF.
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

/// A TC frame of the scenario on VC 0: `flags` are the Bypass and Control Command Flags in
/// the top bits of the first octet (232.0 Table 4-1), `body` the octets after N(S).
fn tc(flags: u8, sequence: u8, body: &[u8]) -> Vec<u8> {
    let len = u16::try_from(5 + body.len() + 2 - 1).unwrap();
    let mut f = vec![flags | 0x01, 0x55];
    f.extend_from_slice(&len.to_be_bytes());
    f.push(sequence);
    f.extend_from_slice(body);
    fecf(&mut f);
    f
}

/// Type-AD: the Segment Header (unsegmented, MAP 0) and `data` (232.0 §4.1.3.2).
fn ad(sequence: u8, data: &[u8]) -> Vec<u8> {
    tc(0x00, sequence, &[&[0xC0], data].concat())
}

/// Type-BD with the same body.
fn bd(sequence: u8, data: &[u8]) -> Vec<u8> {
    tc(0x20, sequence, &[&[0xC0], data].concat())
}

/// Type-BC: a Control Command (232.0 §4.1.3.3).
fn bc(command: ControlCommand) -> Vec<u8> {
    tc(0x30, 0, command.to_octets().as_slice())
}

/// A Space Packet of the commands APID with one data octet.
const PACKET: [u8; 7] = [0x18, 0x80, 0xC0, 0x00, 0x00, 0x00, 0xAA];

const fn unit(index: u8) -> [u8; 4] {
    [0x10 + index, 0xAA, 0xBB, 0xCC]
}

#[expect(
    clippy::too_many_lines,
    reason = "one hand-written expected trace, easier to review in one piece"
)]
#[test]
fn a_node_pair_at_cop1_with_every_fault_kind_reproduces_a_hand_written_trace() {
    // req: APP-071, APP-075, APP-090
    let scenario = Scenario::load().unwrap();
    let down = RefCell::new(Vec::new());
    let up = RefCell::new(Vec::new());
    // The uplink: an AD frame at each of the first four slots (0, 250, 500, 750 ms), then the
    // Unlock of a Type-BC frame at 1 s, as a ground node with FOP-1 sends them.
    let uplink = [
        ad(0, &PACKET),
        ad(1, &PACKET),
        ad(2, &PACKET),
        ad(3, &PACKET),
        ad(4, &PACKET),
        bc(ControlCommand::Unlock),
    ];
    let mut slot = 0u8;
    let mut flight = Scripted(
        |t: u64, d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            up.borrow_mut()
                .extend(d.into_iter().map(|u| (t, u.to_vec())));
            if tx {
                out.unit(&unit(slot));
                slot += 1;
            }
        },
    );
    let mut sent = 0usize;
    let mut ground = Scripted(|t: u64, d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
        down.borrow_mut()
            .extend(d.into_iter().map(|u| (t, u.to_vec())));
        if tx && let Some(frame) = uplink.get(sent) {
            out.unit(frame);
            sent += 1;
        }
    });
    let bit = |b| Ranged::new(b).unwrap();
    let faults = vec![
        fault(Direction::Up, 0, Fault::Drop {}),
        fault(Direction::Up, 1, Fault::Duplicate {}),
        fault(
            Direction::Up,
            2,
            Fault::Delay {
                delay_us: Micros::new(30_000),
            },
        ),
        fault(
            Direction::Up,
            3,
            Fault::Flip {
                octet: 0,
                bit: bit(7),
            },
        ),
        fault(Direction::Up, 4, Fault::Truncate { octets: 3 }),
        fault(Direction::Down, 0, Fault::Drop {}),
        fault(Direction::Down, 3, Fault::Duplicate {}),
    ];
    let c = case(Level::Cop1, 1_375_000, vec![], faults);
    let records = sim::records(&scenario, &c, &mut flight, &mut ground).unwrap();
    let sent_rec = |t_us: u64, dir, index, octets: &[u8]| Record::UnitSent {
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
        fault_rec(0, Dir::Up, 0, FaultKind::Drop, &[]),
        sent_rec(0, Dir::Up, 0, &uplink[0]),
        fault_rec(0, Dir::Down, 0, FaultKind::Drop, &[]),
        sent_rec(0, Dir::Down, 0, &unit(0)),
        sent_rec(125_000, Dir::Down, 1, &unit(1)),
        delivered(135_000, Dir::Down, 1, 0),
        fault_rec(250_000, Dir::Up, 1, FaultKind::Duplicate, &[]),
        sent_rec(250_000, Dir::Up, 1, &uplink[1]),
        sent_rec(250_000, Dir::Down, 2, &unit(2)),
        // Equal arrival instants: the downlink unit first (§6.2).
        delivered(260_000, Dir::Down, 2, 0),
        delivered(260_000, Dir::Up, 1, 0),
        delivered(260_000, Dir::Up, 1, 1),
        fault_rec(375_000, Dir::Down, 3, FaultKind::Duplicate, &[]),
        sent_rec(375_000, Dir::Down, 3, &unit(3)),
        delivered(385_000, Dir::Down, 3, 0),
        delivered(385_000, Dir::Down, 3, 1),
        fault_rec(500_000, Dir::Up, 2, FaultKind::Delay, &[30_000]),
        sent_rec(500_000, Dir::Up, 2, &uplink[2]),
        sent_rec(500_000, Dir::Down, 4, &unit(4)),
        delivered(510_000, Dir::Down, 4, 0),
        delivered(540_000, Dir::Up, 2, 0),
        sent_rec(625_000, Dir::Down, 5, &unit(5)),
        delivered(635_000, Dir::Down, 5, 0),
        fault_rec(750_000, Dir::Up, 3, FaultKind::Flip, &[0, 7]),
        sent_rec(750_000, Dir::Up, 3, &uplink[3]),
        sent_rec(750_000, Dir::Down, 6, &unit(6)),
        delivered(760_000, Dir::Down, 6, 0),
        delivered(760_000, Dir::Up, 3, 0),
        sent_rec(875_000, Dir::Down, 7, &unit(7)),
        delivered(885_000, Dir::Down, 7, 0),
        fault_rec(1_000_000, Dir::Up, 4, FaultKind::Truncate, &[3]),
        sent_rec(1_000_000, Dir::Up, 4, &uplink[4]),
        sent_rec(1_000_000, Dir::Down, 8, &unit(8)),
        delivered(1_010_000, Dir::Down, 8, 0),
        delivered(1_010_000, Dir::Up, 4, 0),
        sent_rec(1_125_000, Dir::Down, 9, &unit(9)),
        delivered(1_135_000, Dir::Down, 9, 0),
        sent_rec(1_250_000, Dir::Up, 5, &uplink[5]),
        sent_rec(1_250_000, Dir::Down, 10, &unit(10)),
        delivered(1_260_000, Dir::Down, 10, 0),
        delivered(1_260_000, Dir::Up, 5, 0),
    ];
    assert_eq!(records, expected);
    // What the flight node received: the duplicate twice, the delayed frame 30 ms late, the
    // flip inverts the least significant bit of octet 0 (GEN-050: bit 7), the truncation keeps
    // three octets; the dropped frame never arrives.
    let mut flipped = uplink[3].clone();
    flipped[0] ^= 0x01;
    assert_eq!(
        up.into_inner(),
        [
            (260_000, uplink[1].clone()),
            (260_000, uplink[1].clone()),
            (540_000, uplink[2].clone()),
            (760_000, flipped),
            (1_010_000, uplink[4][..3].to_vec()),
            (1_260_000, uplink[5].clone()),
        ]
    );
    let got = down.into_inner();
    assert_eq!(got.len(), 11);
    assert_eq!(got[2], (385_000, unit(3).to_vec()));
    assert_eq!(got[3], (385_000, unit(3).to_vec()));
}

/// A pair that breaks a `tx_slot` rule: the ground node sends `ground_units` units at t = 0 and
/// the flight node `flight_units` at every slot.
fn run_units(
    level: Level,
    ground_units: usize,
    flight_units: usize,
    end_us: u64,
) -> Result<Vec<Record>, String> {
    let scenario = Scenario::load().unwrap();
    let mut flight = Scripted(
        move |_t: u64, _d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            if tx {
                for _ in 0..flight_units {
                    out.unit(&[1]);
                }
            }
        },
    );
    let mut ground = Scripted(
        move |t: u64, _d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            if tx && t == 0 {
                for _ in 0..ground_units {
                    out.unit(&[2]);
                }
            }
        },
    );
    sim::records(
        &scenario,
        &case(level, end_us, vec![], vec![]),
        &mut flight,
        &mut ground,
    )
}

#[test]
fn one_frame_per_slot_holds_at_cop1_as_at_frames() {
    // req: APP-071, APP-090
    for level in [Level::Frames, Level::Cop1] {
        assert!(run_units(level, 1, 1, 500_000).is_ok());
        // The ground node may send nothing at a slot, never two frames.
        assert!(run_units(level, 0, 1, 500_000).is_ok());
        let e = run_units(level, 2, 1, 500_000).unwrap_err();
        assert!(e.contains("tx_slot"), "{e}");
        // The flight node sends exactly one frame at every slot (LNK-030).
        assert!(run_units(level, 1, 0, 500_000).is_err());
        assert!(run_units(level, 1, 2, 500_000).is_err());
    }
}

#[test]
fn a_node_sees_tx_slot_only_on_its_own_slot_at_cop1() {
    // req: APP-071, APP-090
    let scenario = Scenario::load().unwrap();
    let ground_slots = RefCell::new(Vec::new());
    let flight_slots = RefCell::new(Vec::new());
    let mut flight = Scripted(
        |t: u64, _d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            if tx {
                flight_slots.borrow_mut().push(t);
                out.unit(&[1]);
            }
        },
    );
    let mut ground = Scripted(|t: u64, _d: Delivered<'_>, tx: bool, _out: &mut dyn Output| {
        if tx {
            ground_slots.borrow_mut().push(t);
        }
    });
    let c = case(Level::Cop1, 600_000, vec![], vec![]);
    sim::records(&scenario, &c, &mut flight, &mut ground).unwrap();
    assert_eq!(
        flight_slots.into_inner(),
        [0, 125_000, 250_000, 375_000, 500_000]
    );
    assert_eq!(ground_slots.into_inner(), [0, 250_000, 500_000]);
}

#[test]
fn a_unit_sent_off_its_slot_fails_at_cop1() {
    // req: APP-090
    let scenario = Scenario::load().unwrap();
    // A ground node that sends when a unit arrives, 10 ms after a flight slot: off the slots.
    let mut flight = Scripted(
        |_t: u64, _d: Delivered<'_>, tx: bool, out: &mut dyn Output| {
            if tx {
                out.unit(&[1]);
            }
        },
    );
    let mut ground = Scripted(|_t: u64, d: Delivered<'_>, _tx: bool, out: &mut dyn Output| {
        if d.into_iter().next().is_some() {
            out.unit(&[2]);
        }
    });
    let c = case(Level::Cop1, 600_000, vec![], vec![]);
    let e = sim::records(&scenario, &c, &mut flight, &mut ground).unwrap_err();
    assert!(e.contains("tx_slot"), "{e}");
}

#[test]
fn the_ad_and_bc_frames_decode_by_level() {
    // req: APP-086, APP-090
    let s = Scenario::load().unwrap();
    let d = decode_frame(&s, Level::Cop1, Dir::Up, &ad(5, &PACKET)).unwrap();
    assert_eq!(d.frame_type, Some(FrameType::Ad));
    assert_eq!((d.sequence, d.vcid, d.is_control()), (Some(5), 0, false));
    assert_eq!(d.segment_header, Some(0xC0));
    assert_eq!(d.data, PACKET);
    assert_eq!(d.control_command, None);
    let d = decode_frame(&s, Level::Cop1, Dir::Up, &bd(7, &PACKET)).unwrap();
    assert_eq!((d.frame_type, d.sequence), (Some(FrameType::Bd), Some(7)));
    let d = decode_frame(&s, Level::Cop1, Dir::Up, &bc(ControlCommand::Unlock)).unwrap();
    assert_eq!(d.frame_type, Some(FrameType::Bc));
    assert!(d.is_control());
    assert_eq!(d.control_command, Some(ControlCommand::Unlock));
    assert_eq!(d.segment_header, None);
    let set_vr = ControlCommand::SetVr(aldis::core::ids::FrameSeqNum::new(9));
    let d = decode_frame(&s, Level::Cop1, Dir::Up, &bc(set_vr)).unwrap();
    assert_eq!(d.control_command, Some(set_vr));
    // A frame-level case has Type-BD frames only (APP-043).
    assert!(decode_frame(&s, Level::Frames, Dir::Up, &bd(0, &PACKET)).is_ok());
    assert!(decode_frame(&s, Level::Frames, Dir::Up, &ad(0, &PACKET)).is_err());
    assert!(decode_frame(&s, Level::Frames, Dir::Up, &bc(ControlCommand::Unlock)).is_err());
    // The strict decoding still catches a wrong FECF on each type.
    for mut frame in [ad(0, &PACKET), bc(ControlCommand::Unlock)] {
        let last = frame.len() - 1;
        frame[last] ^= 1;
        assert!(decode_frame(&s, Level::Cop1, Dir::Up, &frame).is_err());
    }
}

#[test]
fn the_uplink_reassembly_area_is_kept_across_frames() {
    // req: APP-086
    // #484: the area belongs to the struct; a packet in each of several frames, and a Type-BC
    // frame between them, are all handed out.
    let s = Scenario::load().unwrap();
    let mut framed = FramedPackets::new(&s).unwrap();
    let mut packets = 0;
    for frame in [
        ad(0, &PACKET),
        bc(ControlCommand::Unlock),
        ad(1, &PACKET),
        ad(2, &PACKET),
    ] {
        let d = decode_frame(&s, Level::Cop1, Dir::Up, &frame).unwrap();
        packets += framed.take(Dir::Up, &d).unwrap();
    }
    assert_eq!(packets, 3);
}

const fn directive(at_us: u64, directive: Fop1Directive) -> OpsEntry {
    OpsEntry::Fop1Directive {
        at_us: Micros::new(at_us),
        directive,
    }
}

#[test]
fn the_directives_of_the_scenario_parse_with_their_codes() {
    // req: APP-148
    let parse = |text: &str| serde_json::from_str::<Fop1Directive>(text);
    let vs = |n| Fop1Directive::SetVs(Ranged::new(n).unwrap());
    let table = [
        (
            "\"initiate_ad_without_clcw\"",
            Fop1Directive::InitiateAdWithoutClcw,
            1,
            0,
        ),
        (
            "\"initiate_ad_with_clcw\"",
            Fop1Directive::InitiateAdWithClcw,
            2,
            0,
        ),
        (
            "\"initiate_ad_with_unlock\"",
            Fop1Directive::InitiateAdWithUnlock,
            3,
            0,
        ),
        ("\"terminate_ad\"", Fop1Directive::TerminateAd, 5, 0),
        ("{\"set_vs\":0}", vs(0), 7, 0),
        ("{\"set_vs\":255}", vs(255), 7, 255),
    ];
    for (text, expected, code, qualifier) in table {
        let d = parse(text).unwrap();
        assert_eq!(d, expected, "{text}");
        assert_eq!((d.code(), d.qualifier()), (code, qualifier), "{text}");
        assert_eq!(serde_json::to_string(&d).unwrap(), text);
    }
    // A value the scenario does not know, or out of its range, is not a directive.
    for text in [
        "\"resume_ad\"",
        "\"initiate_ad_with_set_vr\"",
        "\"set_vs\"",
        "{\"set_vs\":256}",
        "{\"set_vs\":-1}",
        "{\"set_vs\":1.5}",
        "{\"set_vs\":1,\"extra\":2}",
        "{\"set_t1\":1}",
        "5",
    ] {
        assert!(parse(text).is_err(), "{text}");
    }
}

/// The scenario with one more case, `a3-test`, a copy of `a2-nominal` at `level` with `ops`
/// and no faults, as JSON text.
fn text_with(level: &str, ops: &serde_json::Value) -> String {
    let mut value: serde_json::Value =
        serde_json::from_str(refapp_harness::scenario::SCENARIO_JSON).unwrap();
    let cases = value["cases"].as_array_mut().unwrap();
    let mut c = cases
        .iter()
        .find(|c| c["id"] == "a2-nominal")
        .unwrap()
        .clone();
    c["id"] = "a3-test".into();
    c["level"] = level.into();
    c["faults"] = serde_json::json!([]);
    c["ops"] = ops.clone();
    cases.push(c);
    value.to_string()
}

#[test]
fn the_file_takes_the_level_cop1_and_the_directive_entries() {
    // req: APP-107, APP-148
    let entry = |directive: serde_json::Value| {
        serde_json::json!([{"kind": "fop1_directive", "at_us": 0, "directive": directive},
            {"kind": "fop1_directive", "at_us": 0, "directive": {"set_vs": 3}}])
    };
    let parsed = Scenario::parse(&text_with("cop1", &entry("terminate_ad".into()))).unwrap();
    let c = parsed.case("a3-test").unwrap();
    assert_eq!(c.level, Level::Cop1);
    assert_eq!(
        c.ops,
        [
            directive(0, Fop1Directive::TerminateAd),
            directive(0, Fop1Directive::SetVs(Ranged::new(3).unwrap())),
        ]
    );
    // An unknown level, an unknown directive and a Qualifier out of range are refused when the
    // file is parsed.
    assert!(Scenario::parse(&text_with("cop2", &entry("terminate_ad".into()))).is_err());
    for bad in [
        serde_json::json!("resume_ad"),
        serde_json::json!({"set_vs": 256}),
    ] {
        let e = Scenario::parse(&text_with("cop1", &entry(bad.clone()))).unwrap_err();
        assert!(e.contains("ops[0]"), "{bad}: {e}");
    }
    // The generated schema lists the level and the directive entry.
    let schema = refapp_harness::scenario::schema().unwrap();
    assert!(schema.contains("\"cop1\""));
    assert!(schema.contains("set_vs"));
}

const fn noop(at_us: u64, execution: Execution) -> OpsEntry {
    OpsEntry::Command {
        at_us: Micros::new(at_us),
        execution,
        call: Call::Noop {},
        extra_octets: None,
    }
}

/// The error of the rules of §6.5 for a script at `level`, or an empty string.
fn check_of(level: &str, ops: &[OpsEntry]) -> String {
    let ops = serde_json::to_value(ops).unwrap();
    Scenario::parse(&text_with(level, &ops))
        .err()
        .unwrap_or_default()
}

#[test]
fn a_cop1_case_is_checked_against_the_nominal_uplink_schedule() {
    // req: APP-141, APP-152
    let tagged = |exec_us| Execution::TimeTagged {
        exec_us: Micros::new(exec_us),
    };
    // An immediate command is refused at the new level as at `frames`.
    let e = check_of("cop1", &[noop(0, Execution::Immediate {})]);
    assert!(e.contains("APP-141"), "{e}");
    // A command whose execution time is before its nominal arrival is refused: released at
    // 0.5 s it arrives at 0.51 s.
    let e = check_of("cop1", &[noop(500_000, tagged(500_000))]);
    assert!(e.contains("APP-141"), "{e}");
    // A command that arrives in time is accepted, with a directive entry or without one: the
    // directive takes no uplink slot in the nominal schedule.
    let command = noop(0, tagged(125_000));
    assert_eq!(check_of("cop1", std::slice::from_ref(&command)), "");
    let with = [directive(0, Fop1Directive::TerminateAd), command];
    assert_eq!(check_of("cop1", &with), "");
    assert_eq!(check_of("frames", &with), "");
}

#[test]
fn a_directive_below_cop1_is_ignored() {
    // req: APP-019
    // At the frame level the entry parses, wakes the ground node at its instant, and sends
    // nothing: the ground node of A2 gives a directive no frame and no packet.
    let ops = serde_json::json!([{"kind": "fop1_directive", "at_us": 0,
        "directive": "terminate_ad"}]);
    let scenario = Scenario::parse(&text_with("frames", &ops)).unwrap();
    let case = scenario.case("a3-test").unwrap();
    let script: Vec<_> = case.ops.iter().map(|e| e.to_node().unwrap()).collect();
    let mut flight = refapp_flight::FlightNode::new_frames().unwrap();
    let mut ground = refapp_ground::GroundNode::new_frames(&script).unwrap();
    let records = sim::records(&scenario, case, &mut flight, &mut ground).unwrap();
    assert!(records.iter().all(|r| !matches!(
        r,
        Record::UnitSent { dir: Dir::Up, .. }
            | Record::PacketOut {
                node: NodeId::Ground,
                ..
            }
    )));
    assert!(
        records
            .iter()
            .any(|r| matches!(r, Record::UnitSent { dir: Dir::Down, .. }))
    );
}
