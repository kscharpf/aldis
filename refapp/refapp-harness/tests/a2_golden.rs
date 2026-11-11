//! The invariants of the A2 cases, checked on their golden traces (A2 WP-5; D5 and D7 of
//! docs/milestones/a2-frames.md; docs/reference-application.md §4.4.4, §6.7).
//!
//! The expected schedule and the expected events are written out here from the tables of the
//! document, not read back from the traces.

use std::collections::BTreeMap;

use refapp_harness::golden_path;
use refapp_harness::trace::{check_canonical, packet_projection};
use serde_json::Value;

/// The records of a case's golden trace, after the header.
fn records(case: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(golden_path(case)).unwrap();
    check_canonical(&text).unwrap();
    text.lines()
        .skip(1)
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// A unit sent: its send instant and octets, by index.
fn units(case: &str, dir: &str) -> Vec<(u64, Vec<u8>)> {
    let mut sent: Vec<(u64, u64, Vec<u8>)> = records(case)
        .iter()
        .filter(|r| r["kind"] == "unit_sent" && r["dir"] == dir)
        .map(|r| {
            (
                r["index"].as_u64().unwrap(),
                r["t_us"].as_u64().unwrap(),
                unhex(r["hex"].as_str().unwrap()),
            )
        })
        .collect();
    sent.sort_by_key(|u| u.0);
    for (want, u) in sent.iter().enumerate() {
        assert_eq!(
            u.0, want as u64,
            "{case}: {dir} unit indexes are 0, 1, 2, …"
        );
    }
    sent.into_iter().map(|u| (u.1, u.2)).collect()
}

/// One application event: instant, name, arguments.
type Event = (u64, String, Vec<u64>);

fn app_events(case: &str, node: &str) -> Vec<Event> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "app_event" && r["node"] == node)
        .map(|r| {
            (
                r["t_us"].as_u64().unwrap(),
                r["name"].as_str().unwrap().to_owned(),
                r["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_u64().unwrap())
                    .collect(),
            )
        })
        .collect()
}

fn event(t: u64, name: &str, args: &[u64]) -> Event {
    (t, name.to_owned(), args.to_vec())
}

/// The ground events of `case` other than `clcw_changed`, and the `clcw_changed` ones.
fn ground_split(case: &str) -> (Vec<Event>, Vec<Event>) {
    app_events(case, "ground")
        .into_iter()
        .partition(|e| e.1 != "clcw_changed")
}

/// The fault records of `case`: direction, index, kind, arguments.
fn faults(case: &str) -> Vec<(String, u64, String, Vec<u64>)> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "fault")
        .map(|r| {
            (
                r["dir"].as_str().unwrap().to_owned(),
                r["index"].as_u64().unwrap(),
                r["fault"].as_str().unwrap().to_owned(),
                r["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_u64().unwrap())
                    .collect(),
            )
        })
        .collect()
}

fn fault(dir: &str, index: u64, kind: &str, args: &[u64]) -> (String, u64, String, Vec<u64>) {
    (dir.to_owned(), index, kind.to_owned(), args.to_vec())
}

/// The arrival instants of the units delivered, in trace order: direction, index, instant.
fn deliveries(case: &str) -> Vec<(String, u64, u64)> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "unit_delivered")
        .map(|r| {
            (
                r["dir"].as_str().unwrap().to_owned(),
                r["index"].as_u64().unwrap(),
                r["t_us"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// The records that the flight node's own behaviour makes in a case with downlink faults only:
/// the frames and packets it sends (the faults change only what the ground node receives).
fn flight_output(case: &str) -> Vec<String> {
    records(case)
        .iter()
        .filter(|r| {
            (r["kind"] == "unit_sent" && r["dir"] == "down")
                || (r["kind"] == "packet_out" && r["node"] == "flight")
        })
        .map(Value::to_string)
        .collect()
}

const CMD_EXECUTED: u64 = 1;
const TABLE_REJECTED: u64 = 0x11;
const DUMP_COMPLETED: u64 = 0x20;
const PAYLOAD_MODE: u64 = 0x30;
const SLOT: u64 = 125_000;
const DELAY: u64 = 10_000;

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF), the FECF of 132.0 §4.1.6 and 232.0 §4.1.4.
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    crc
}

/// The packets of a frame's data field (octets 6 to 216) from the First Header Pointer: the
/// list of (APID, octets) from the first packet that starts there to the end of the field.
fn packets_from(field: &[u8], fhp: usize) -> Vec<(u16, usize)> {
    let mut out = Vec::new();
    let mut at = fhp;
    // bound: each pass moves `at` forward by at least the 7 octets of a packet.
    while at + 6 <= field.len() {
        let apid = u16::from_be_bytes([field[at], field[at + 1]]) & 0x07FF;
        let len = usize::from(u16::from_be_bytes([field[at + 4], field[at + 5]])) + 7;
        out.push((apid, len.min(field.len() - at)));
        at += len;
    }
    out
}

const HK: usize = 44;
const EVENT: usize = 21;
const IDLE: u16 = 2047;

/// Slots of the VC 1 frames of `a2-nominal` (§4.4.4): payload fields, and the dump stream.
fn vc1_slots() -> BTreeMap<u64, (u8, usize)> {
    // slot -> (unused, First Header Pointer of the field)
    let mut m = BTreeMap::new();
    for n in (0..10).chain(10..13) {
        if n == 6 {
            continue;
        }
        let t = if n < 10 {
            500_000 + 2_000_000 * n
        } else {
            24_500_000 + 2_000_000 * (n - 10)
        };
        let slot = t / SLOT;
        m.insert(slot, (0, 0));
        m.insert(slot + 8, (0, 201));
    }
    m
}

#[test]
fn nominal_downlink_follows_the_hand_derived_schedule() {
    // req: APP-137, APP-134, APP-135, APP-132, APP-071, APP-082
    let down = units("a2-nominal", "down");
    assert_eq!(down.len(), 280);
    // The kinds of frames by slot (§4.4.4).
    let mut vc0: Vec<u64> = (0..30).map(|k| 8 * k + 2).collect();
    vc0.push(102);
    let mut vc1: BTreeMap<u64, u8> = vc1_slots()
        .into_iter()
        .map(|(s, (_, f))| (s, u8::try_from(f).unwrap()))
        .collect();
    // the dump stream and payload packet 6: frame -> VC count 12..17 are checked below.
    for s in [97, 99, 100, 101, 103, 110] {
        vc1.insert(s, 0);
    }
    assert_eq!(vc0.len(), 31);
    assert_eq!(vc1.len(), 30);
    let mut counts = [0_u64; 8];
    for (i, (t, f)) in down.iter().enumerate() {
        let i = i as u64;
        assert_eq!(*t, i * SLOT, "frame {i} is sent at its slot");
        assert_eq!(f.len(), 223, "frame {i}: length (§4.4.3)");
        let fecf = u16::from_be_bytes([f[221], f[222]]);
        assert_eq!(fecf, crc16(&f[..221]), "frame {i}: FECF (132.0 §4.1.6)");
        let vc = if vc0.contains(&i) {
            0
        } else if vc1.contains_key(&i) {
            1
        } else {
            7
        };
        // Master and virtual channel counts.
        assert_eq!(u64::from(f[2]), i % 256, "frame {i}: MC Frame Count");
        assert_eq!(
            u64::from(f[3]),
            counts[vc] % 256,
            "frame {i}: VC Frame Count"
        );
        counts[vc] += 1;
        let first_two = match vc {
            0 => [0x15, 0x51],
            1 => [0x15, 0x53],
            _ => [0x15, 0x5F],
        };
        assert_eq!(
            f[..2],
            first_two,
            "frame {i}: version, SCID, VCID, OCF flag"
        );
        // Data Field Status 0x18: OCF flag 1 set elsewhere; FHP is the next 11 bits.
        // FHP 0 and 201 give 0x18; the FHP of an OID frame (0x7FE) and `no packet starts`
        // (0x7FF, frame 103) give 0x1F.
        assert_eq!(
            f[4],
            if vc == 7 || i == 103 { 0x1F } else { 0x18 },
            "frame {i}: Data Field Status"
        );
    }
    // Totals (§4.4.4): 61 data frames, 219 OID frames; the last counts.
    assert_eq!(counts[0], 31);
    assert_eq!(counts[1], 30);
    assert_eq!(counts[7], 219);
    assert_eq!(down[279].1[2], 23, "MC count after the wrap at 32 s");
    // The first frames, §4.4.4.
    assert_eq!(down[0].1[..6], [0x15, 0x5F, 0x00, 0x00, 0x1F, 0xFE]);
    assert_eq!(down[2].1[..6], [0x15, 0x51, 0x02, 0x00, 0x18, 0x00]);
    assert_eq!(down[4].1[..6], [0x15, 0x53, 0x04, 0x00, 0x18, 0x00]);
    assert_eq!(
        down[0].1[6..16],
        [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6, 0xD8, 0x61, 0x45, 0x1F],
        "the OID generator's first octets (132.0 §4.1.4.6.2 note)"
    );
    // OID frames: FHP 0x7FE.
    for (i, (_, f)) in down.iter().enumerate() {
        let vc = f[1] >> 1 & 7;
        if vc == 7 {
            assert_eq!(f[4..6], [0x1F, 0xFE], "frame {i}: OID First Header Pointer");
        }
    }
}

#[test]
fn nominal_fields_hold_the_packets_of_the_schedule() {
    // req: APP-137, APP-135, APP-132
    let down = units("a2-nominal", "down");
    let field = |i: usize| &down[i].1[6..217];
    // VC 0: events of the second, then HK, then an idle packet that fills the field.
    let events_at: BTreeMap<u64, usize> = [(6, 1), (10, 2), (12, 1), (20, 3), (24, 2)]
        .into_iter()
        .collect();
    for k in 0..30_u64 {
        let i = usize::try_from(8 * k + 2).unwrap();
        assert_eq!(down[i].1[4..6], [0x18, 0x00], "frame {i}: FHP 0");
        let n = events_at.get(&k).copied().unwrap_or(0);
        let mut expected = vec![(17, EVENT); n];
        expected.push((16, HK));
        let used: usize = expected.iter().map(|p| p.1).sum();
        expected.push((IDLE, 211 - used));
        assert_eq!(packets_from(field(i), 0), expected, "frame {i} (t = {k} s)");
    }
    // Idle packets of 146, 125, 146, 104, 125 and 167 octets (§4.4.4).
    assert_eq!(packets_from(field(8 * 6 + 2), 0).last(), Some(&(IDLE, 146)));
    assert_eq!(
        packets_from(field(8 * 10 + 2), 0).last(),
        Some(&(IDLE, 125))
    );
    assert_eq!(
        packets_from(field(8 * 20 + 2), 0).last(),
        Some(&(IDLE, 104))
    );
    // The idle packet of 167 octets: header `07 FF C0 00 00 A0`, then idle data 0x00.
    assert_eq!(field(2)[44..50], [0x07, 0xFF, 0xC0, 0x00, 0x00, 0xA0]);
    assert!(field(2)[50..].iter().all(|&b| b == 0));
    // Frame 102: the DUMP_COMPLETED event packet and an idle packet of 190 octets.
    assert_eq!(down[102].1[4..6], [0x18, 0x00]);
    assert_eq!(packets_from(field(102), 0), vec![(17, EVENT), (IDLE, 190)]);
    // VC 1, payload packets: first field at T(n) (FHP 0), the other 201 octets and an idle
    // packet of 10 octets at T(n) + 1 s (FHP 201).
    for (slot, (_, fhp)) in vc1_slots() {
        let i = usize::try_from(slot).unwrap();
        let f = &down[i].1;
        assert_eq!(f[1], 0x53, "frame {i} is on VC 1");
        if fhp == 0 {
            assert_eq!(f[4..6], [0x18, 0x00], "frame {i}: first field");
            assert_eq!(
                packets_from(field(i), 0)[0].0,
                32,
                "frame {i}: payload packet"
            );
        } else {
            assert_eq!(f[4..6], [0x18, 0xC9], "frame {i}: FHP 201");
            assert_eq!(packets_from(field(i), 201), vec![(IDLE, 10)]);
            assert_eq!(
                field(i)[201..211],
                [0x07, 0xFF, 0xC0, 0x00, 0x00, 0x03, 0, 0, 0, 0]
            );
        }
    }
    // VC 1, the dump stream and payload packet 6: VC count, FHP, and first packets (§4.4.4).
    let stream: [(usize, u8, usize); 6] = [
        (97, 12, 0),
        (99, 13, 69),
        (100, 14, 138),
        (101, 15, 33),
        (103, 16, 0x7FF),
        (110, 17, 23),
    ];
    for (i, vc_count, fhp) in stream {
        assert_eq!(down[i].1[1], 0x53, "frame {i} is on VC 1");
        assert_eq!(down[i].1[3], vc_count, "frame {i}: VC Frame Count");
        assert_eq!(
            u16::from_be_bytes([down[i].1[4], down[i].1[5]]) & 0x07FF,
            u16::try_from(fhp).unwrap(),
            "frame {i}: First Header Pointer"
        );
    }
    assert_eq!(packets_from(field(97), 0)[0], (33, 140));
    assert_eq!(packets_from(field(99), 69)[0], (33, 140));
    assert_eq!(
        packets_from(field(100), 138)[0],
        (33, 73),
        "73 octets of dump 4 start here"
    );
    assert_eq!(packets_from(field(110), 23), vec![(IDLE, 188)]);
    // Frames 98 and 102 are VC 0; at slot 102 the full VC 1 field waits for slot 103.
    assert_eq!(down[98].1[1], 0x51);
    assert_eq!(down[102].1[1], 0x51);
    assert_eq!(down[103].1[1], 0x53);
}

#[test]
fn nominal_ocf_follows_the_farm_b_counter() {
    // req: APP-134, APP-137
    let down = units("a2-nominal", "down");
    let changes: [(usize, u32); 8] = [
        (9, 0x0100_0200),
        (17, 0x0100_0400),
        (19, 0x0100_0600),
        (21, 0x0100_0000),
        (25, 0x0100_0200),
        (33, 0x0100_0400),
        (41, 0x0100_0600),
        (49, 0x0100_0000),
    ];
    let mut word = 0x0100_0000_u32;
    for (i, (_, f)) in down.iter().enumerate() {
        if let Some((_, w)) = changes.iter().find(|c| c.0 == i) {
            word = *w;
        }
        let ocf = u32::from_be_bytes([f[217], f[218], f[219], f[220]]);
        assert_eq!(ocf, word, "frame {i}: OCF (232.0 §4.2.1)");
    }
    let (other, clcw) = ground_split("a2-nominal");
    let expected: Vec<Event> = changes
        .iter()
        .map(|(i, w)| event(*i as u64 * SLOT + DELAY, "clcw_changed", &[u64::from(*w)]))
        .collect();
    assert_eq!(clcw, expected);
    assert_eq!(
        other,
        vec![event(
            12_635_000,
            "dump_reassembled",
            &[65_536, 600, 32_936]
        )]
    );
    assert!(
        app_events("a2-nominal", "flight")
            .iter()
            .all(|e| e.1 == "cmd_received" || e.1 == "table_received"),
        "the flight node raises no frame-level event"
    );
}

#[test]
fn nominal_uplink_has_the_eight_frames_of_the_schedule() {
    // req: APP-137, APP-090, APP-071
    let up = units("a2-nominal", "up");
    // (send instant, frame octets, first five octets, Segment Header)
    let expected: [(u64, usize, [u8; 5], u8); 8] = [
        (1_000_000, 22, [0x21, 0x55, 0x00, 0x15, 0x00], 0xC0),
        (2_000_000, 128, [0x21, 0x55, 0x00, 0x7F, 0x00], 0x41),
        (2_250_000, 128, [0x21, 0x55, 0x00, 0x7F, 0x00], 0x01),
        (2_500_000, 77, [0x21, 0x55, 0x00, 0x4C, 0x00], 0x81),
        (3_000_000, 25, [0x21, 0x55, 0x00, 0x18, 0x00], 0xC0),
        (4_000_000, 28, [0x21, 0x55, 0x00, 0x1B, 0x00], 0xC0),
        (5_000_000, 37, [0x21, 0x55, 0x00, 0x24, 0x00], 0xC0),
        (6_000_000, 23, [0x21, 0x55, 0x00, 0x16, 0x00], 0xC0),
    ];
    assert_eq!(up.len(), expected.len());
    for (i, ((t, f), (et, len, head, seg))) in up.iter().zip(expected).enumerate() {
        assert_eq!(*t, et, "uplink frame {i}: send instant");
        assert_eq!(f.len(), len, "uplink frame {i}: length");
        assert_eq!(f[..5], head, "uplink frame {i}: first five octets");
        assert_eq!(f[5], seg, "uplink frame {i}: Segment Header");
        let fecf = u16::from_be_bytes([f[len - 2], f[len - 1]]);
        assert_eq!(
            fecf,
            crc16(&f[..len - 2]),
            "uplink frame {i}: FECF (232.0 §4.1.4)"
        );
        // The frame arrives 10 000 µs after it is sent, and counts once.
        assert!(deliveries("a2-nominal").contains(&("up".to_owned(), i as u64, t + DELAY)));
    }
}

#[test]
fn nominal_packet_projection_equals_the_a1_one_and_the_instants_agree() {
    // req: APP-003
    let a1 = std::fs::read_to_string(golden_path("a1-nominal")).unwrap();
    let a2 = std::fs::read_to_string(golden_path("a2-nominal")).unwrap();
    assert_eq!(
        packet_projection(&a2).unwrap(),
        packet_projection(&a1).unwrap(),
        "APP-003: the packet projection of the nominal case is the same at every link level"
    );
    // The stronger fact of D7: each node's packet_out records are equal in instant, APID, and
    // octets, since the commands reach the flight node 10 000 µs after their release as at A1.
    let outputs = |text: &str, node: &str| -> Vec<(u64, u64, String)> {
        text.lines()
            .skip(1)
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|r| r["kind"] == "packet_out" && r["node"] == node)
            .map(|r| {
                (
                    r["t_us"].as_u64().unwrap(),
                    r["apid"].as_u64().unwrap(),
                    r["hex"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    for node in ["flight", "ground"] {
        let (x, y) = (outputs(&a1, node), outputs(&a2, node));
        assert!(!x.is_empty(), "{node} outputs packets");
        assert_eq!(y, x, "{node}: packet_out instants and octets");
    }
}

#[test]
fn downlink_loss_events_follow_the_table() {
    // req: APP-140, APP-138, APP-075
    let case = "a2-downlink-loss";
    assert_eq!(
        faults(case),
        vec![
            fault("down", 18, "duplicate", &[]),
            fault("down", 36, "drop", &[]),
            fault("down", 92, "drop", &[]),
            fault("down", 99, "drop", &[]),
        ]
    );
    let (other, clcw) = ground_split(case);
    let expected = vec![
        event(2_260_000, "frame_discontinuity", &[0, 0]),
        event(2_260_000, "frame_discontinuity", &[1, 0]),
        event(2_260_000, "seq_repeat", &[16, 2]),
        event(4_635_000, "frame_gap", &[0, 0, 1]),
        event(5_510_000, "frame_gap", &[1, 1, 1]),
        event(7_510_000, "seq_gap", &[32, 1]),
        event(11_635_000, "frame_gap", &[0, 0, 1]),
        event(12_135_000, "frame_gap", &[1, 1, 1]),
        event(12_135_000, "partial_discarded", &[1, 211]),
        event(12_510_000, "frame_gap", &[0, 0, 1]),
        event(12_510_000, "frame_gap", &[1, 1, 1]),
        event(12_510_000, "partial_discarded", &[1, 71]),
        event(12_635_000, "seq_gap", &[33, 3]),
        event(12_635_000, "dump_discarded", &[33, 128]),
        event(13_760_000, "seq_gap", &[32, 1]),
    ];
    assert_eq!(other, expected);
    // No fault changes a CLCW change: the same eight as in the nominal case, and no dump.
    assert_eq!(clcw, ground_split("a2-nominal").1);
    assert!(other.iter().all(|e| e.1 != "dump_reassembled"));
    // The flight node's output is that of the nominal case; the faults change only what the
    // ground node receives.
    assert_eq!(flight_output(case), flight_output("a2-nominal"));
    // Frame 18 arrives twice at one instant, the lost frames never arrive.
    let d = deliveries(case);
    let copies = |i: u64| d.iter().filter(|x| x.0 == "down" && x.1 == i).count();
    assert_eq!(copies(18), 2);
    for i in [36, 92, 99] {
        assert_eq!(copies(i), 0, "frame {i} is dropped");
    }
}

#[test]
fn downlink_corrupt_events_follow_the_table() {
    // req: APP-140, APP-138, APP-075
    let case = "a2-downlink-corrupt";
    assert_eq!(
        faults(case),
        vec![
            fault("down", 5, "flip", &[1, 6]),
            fault("down", 20, "delay", &[1_200_000]),
            fault("down", 66, "flip", &[100, 3]),
            fault("down", 148, "truncate", &[100]),
        ]
    );
    let (other, clcw) = ground_split(case);
    let expected = vec![
        event(635_000, "frame_rejected", &[8]),
        event(760_000, "frame_gap", &[0, 0, 1]),
        event(760_000, "frame_gap", &[1, 7, 1]),
        event(2_635_000, "frame_gap", &[0, 0, 1]),
        event(3_510_000, "frame_gap", &[1, 1, 1]),
        event(3_710_000, "frame_discontinuity", &[0, 0]),
        event(3_710_000, "frame_discontinuity", &[1, 1]),
        event(3_760_000, "frame_gap", &[0, 0, 9]),
        event(4_510_000, "frame_gap", &[1, 1, 1]),
        event(4_510_000, "partial_discarded", &[1, 211]),
        event(5_510_000, "seq_gap", &[32, 1]),
        event(8_260_000, "frame_rejected", &[8]),
        event(8_385_000, "frame_gap", &[0, 0, 1]),
        event(9_260_000, "frame_gap", &[1, 0, 1]),
        event(9_260_000, "seq_gap", &[16, 1]),
        // The dump is not touched by a fault.
        event(12_635_000, "dump_reassembled", &[65_536, 600, 32_936]),
        event(18_510_000, "frame_rejected", &[3]),
        event(18_635_000, "frame_gap", &[0, 0, 1]),
        event(19_510_000, "frame_gap", &[1, 1, 1]),
        event(25_510_000, "seq_gap", &[32, 1]),
    ];
    assert_eq!(other, expected);
    // The CLCW changes of the nominal case, and two more: the late frame 20 carries the CLCW
    // of its own slot (counter 3), and frame 30 then changes it back.
    let mut expected_clcw = ground_split("a2-nominal").1;
    expected_clcw.push(event(3_710_000, "clcw_changed", &[16_778_752]));
    expected_clcw.push(event(3_760_000, "clcw_changed", &[16_777_728]));
    expected_clcw.sort_by_key(|e| e.0);
    assert_eq!(clcw, expected_clcw);
    assert_eq!(flight_output(case), flight_output("a2-nominal"));
    // The delay is more than one slot: frame 20 arrives after frame 29 and before frame 30.
    let order: Vec<u64> = deliveries(case)
        .into_iter()
        .filter(|d| d.0 == "down")
        .map(|d| d.1)
        .collect();
    let pos = |i: u64| order.iter().position(|&x| x == i).unwrap();
    assert!(pos(29) < pos(20) && pos(20) < pos(30));
    // Each flipped frame fails the FECF, and the truncated one is 100 octets.
    let down = units(case, "down");
    assert_eq!(
        u16::from_be_bytes([down[5].1[221], down[5].1[222]]),
        crc16(&down[5].1[..221]),
        "the sent frame 5 is the unflipped one: a fault applies to the delivered octets"
    );
}

#[test]
fn uplink_faults_have_no_recovery() {
    // req: APP-140, APP-139, APP-043, APP-075
    let case = "a2-uplink-faults";
    assert_eq!(
        faults(case),
        vec![
            fault("up", 0, "flip", &[12, 7]),
            fault("up", 2, "drop", &[]),
            fault("up", 6, "duplicate", &[]),
        ]
    );
    let flight = app_events(case, "flight");
    let expected = vec![
        event(1_010_000, "frame_rejected", &[8]),
        event(2_510_000, "packet_rejected", &[3]),
        event(3_010_000, "cmd_received", &[3, 1]),
        event(4_010_000, "cmd_received", &[2, 2]),
        event(5_010_000, "cmd_received", &[4, 3]),
        event(5_010_000, "cmd_received", &[1, 4]),
        event(5_010_000, "cmd_received", &[4, 3]),
        event(5_010_000, "cmd_received", &[1, 4]),
        event(6_010_000, "cmd_received", &[4, 5]),
    ];
    assert_eq!(flight, expected);
    // The CLCW: FARM-1 counts every frame that passes the validation checks and the invalid
    // one not at all (§4.4.1).
    let (other, clcw) = ground_split(case);
    let expected_clcw: Vec<Event> = [
        (2_135_000, 16_777_728),
        (2_635_000, 16_778_240),
        (3_135_000, 16_778_752),
        (4_135_000, 16_777_216),
        (5_135_000, 16_778_240),
        (6_135_000, 16_778_752),
    ]
    .iter()
    .map(|&(t, w)| event(t, "clcw_changed", &[w]))
    .collect();
    assert_eq!(clcw, expected_clcw);
    assert_eq!(
        other,
        vec![event(
            12_635_000,
            "dump_reassembled",
            &[65_536, 600, 32_936]
        )]
    );
    // On board: no execution at 6.0 s, the activation finds no table at 10.0 s, the dump runs,
    // the two commands of the duplicated frame run twice at 20.0 s.
    let reports: Vec<(u64, u64, Vec<u64>)> = records(case)
        .iter()
        .filter(|r| r["kind"] == "packet_out" && r["apid"] == 17)
        .map(|r| {
            let p = unhex(r["hex"].as_str().unwrap());
            let field = |i: usize| u64::from(u16::from_be_bytes([p[13 + 2 * i], p[14 + 2 * i]]));
            (
                r["t_us"].as_u64().unwrap(),
                field(0),
                vec![field(1), field(2), field(3)],
            )
        })
        .collect();
    let expected_reports = vec![
        (10_000_000, CMD_EXECUTED, vec![3, 1, 0]),
        (10_000_000, TABLE_REJECTED, vec![1, 1, 0]),
        (12_000_000, CMD_EXECUTED, vec![2, 2, 0]),
        (12_500_000, DUMP_COMPLETED, vec![1, 0, 600]),
        (20_000_000, CMD_EXECUTED, vec![4, 3, 0]),
        (20_000_000, PAYLOAD_MODE, vec![0, 0, 0]),
        (20_000_000, CMD_EXECUTED, vec![1, 4, 0]),
        (20_000_000, CMD_EXECUTED, vec![4, 3, 0]),
        (20_000_000, PAYLOAD_MODE, vec![0, 0, 0]),
        (20_000_000, CMD_EXECUTED, vec![1, 4, 0]),
        (24_000_000, CMD_EXECUTED, vec![4, 5, 0]),
        (24_000_000, PAYLOAD_MODE, vec![1, 0, 0]),
    ];
    assert_eq!(reports, expected_reports);
    // The downlink has the frame kinds of the nominal case: the same headers, 280 frames.
    let (a, b) = (units(case, "down"), units("a2-nominal", "down"));
    assert_eq!(a.len(), 280);
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(x.1[..6], y.1[..6], "downlink frame {i}: header and FHP");
    }
    // The dropped frame never arrives, the duplicate arrives twice.
    let d = deliveries(case);
    let copies = |i: u64| d.iter().filter(|x| x.0 == "up" && x.1 == i).count();
    assert_eq!((copies(2), copies(6), copies(0)), (0, 2, 1));
}
