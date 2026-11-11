//! The invariants of the A3 cases, checked on their golden traces (A3 WP-5; D5 and D8 of
//! docs/milestones/a3-cop1.md; docs/reference-application.md §4.5.5, §6.8).
//!
//! The expected schedules and events are written out here from the tables of the document, not
//! read back from the traces.

use refapp_harness::golden_path;
use refapp_harness::trace::{check_canonical, packet_projection};
use serde_json::Value;

const SLOT: u64 = 125_000;
const DELAY: u64 = 10_000;

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
        assert_eq!(u.0, want as u64, "{case}: {dir} indexes are 0, 1, 2, …");
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

/// The arrival instants of the uplink units delivered, in trace order: index, instant.
fn up_arrivals(case: &str) -> Vec<(u64, u64)> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "unit_delivered" && r["dir"] == "up")
        .map(|r| (r["index"].as_u64().unwrap(), r["t_us"].as_u64().unwrap()))
        .collect()
}

/// CRC-16/CCITT-FALSE, the FECF of 232.0 §4.1.4.
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

/// Length and Segment Header of the frame that carries FDU `n` (§4.4.4, §4.5.5).
const FDU: [(usize, u8); 8] = [
    (22, 0xC0),
    (128, 0x41),
    (128, 0x01),
    (77, 0x81),
    (25, 0xC0),
    (28, 0xC0),
    (37, 0xC0),
    (23, 0xC0),
];

/// One expected uplink unit of a case's table: index, send instant, frame kind, N(S) (or the
/// Unlock command), FDU.
enum Up {
    Ad(u64, u64, u8, usize),
    Unlock(u64, u64),
}

/// Checks the uplink units of `case` against its table: instant, octets, FECF (232.0 §4.1.4).
fn check_uplink(case: &str, table: &[Up]) {
    let up = units(case, "up");
    assert_eq!(up.len(), table.len(), "{case}: uplink units");
    for entry in table {
        match *entry {
            Up::Ad(i, t, ns, fdu) => {
                let (len, seg) = FDU[fdu];
                let f = &up[ix(i)].1;
                assert_eq!(up[ix(i)].0, t, "{case}: unit {i}: send instant");
                assert_eq!(f.len(), len, "{case}: unit {i}: length");
                // Version 00, Bypass 0, Control Command 0 (Type-AD), SCID 0x155, VC 0.
                assert_eq!(f[..3], [0x01, 0x55, 0x00], "{case}: unit {i}: header");
                assert_eq!(usize::from(f[3]), len - 1, "{case}: unit {i}: length field");
                assert_eq!(f[4], ns, "{case}: unit {i}: N(S) (232.1 §5.1.3)");
                assert_eq!(f[5], seg, "{case}: unit {i}: Segment Header");
                check_fecf(case, i, f);
            }
            Up::Unlock(i, t) => {
                let f = &up[ix(i)].1;
                assert_eq!(up[ix(i)].0, t, "{case}: unit {i}: send instant");
                // Type-BC (Bypass 1, Control Command 1), SCID 0x155, VC 0, length 8, N(S) 0,
                // the Unlock command `00` (232.0 §4.1.2, §4.1.3.3; 232.1 §6.3.2.2).
                assert_eq!(
                    f[..6],
                    [0x31, 0x55, 0x00, 0x07, 0x00, 0x00],
                    "{case}: Unlock"
                );
                assert_eq!(f.len(), 8);
                check_fecf(case, i, f);
            }
        }
    }
}

fn check_fecf(case: &str, i: u64, f: &[u8]) {
    let n = f.len();
    assert_eq!(
        u16::from_be_bytes([f[n - 2], f[n - 1]]),
        crc16(&f[..n - 2]),
        "{case}: unit {i}: FECF"
    );
}

/// The ground events of the nominal case (§4.5.5), for the FDUs and acknowledgement instants
/// given: the directive of 0.0 s, then per row `clcw_changed` and `fop1_transfer`.
fn ack(t_us: u64, word: u64, fdu: u64) -> [Event; 2] {
    [
        event(t_us, "clcw_changed", &[word]),
        event(t_us, "fop1_transfer", &[3, fdu]),
    ]
}

fn directive0() -> Vec<Event> {
    vec![
        event(0, "fop1_directive", &[1, 1, 0]),
        event(0, "fop1_directive", &[3, 1, 0]),
        event(0, "fop1_state", &[1]),
    ]
}

const DUMP: (u64, &str, [u64; 3]) = (12_635_000, "dump_reassembled", [65_536, 600, 32_936]);

fn dump() -> Event {
    event(DUMP.0, DUMP.1, &DUMP.2)
}

/// The `packet_out` and `packet_in` records of one node: instant, APID, octets.
fn packets(case: &str, kind: &str, node: &str) -> Vec<(u64, u64, String)> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == kind && r["node"] == node)
        .map(|r| {
            (
                r["t_us"].as_u64().unwrap(),
                r["apid"].as_u64().unwrap(),
                r["hex"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The event reports of the flight node (APID 17): instant, event ID, three arguments.
fn reports(case: &str) -> Vec<(u64, u64, Vec<u64>)> {
    packets(case, "packet_out", "flight")
        .into_iter()
        .filter(|p| p.1 == 17)
        .map(|p| {
            let b = unhex(&p.2);
            let field = |i: usize| u64::from(u16::from_be_bytes([b[13 + 2 * i], b[14 + 2 * i]]));
            (p.0, field(0), vec![field(1), field(2), field(3)])
        })
        .collect()
}

const TABLE_REJECTED: u64 = 0x11;

#[test]
fn nominal_uplink_has_the_eight_frames_of_the_schedule() {
    // req: APP-150, APP-044, APP-090, APP-071
    check_uplink(
        "a3-nominal",
        &[
            Up::Ad(0, 1_000_000, 0, 0),
            Up::Ad(1, 2_000_000, 1, 1),
            Up::Ad(2, 2_250_000, 2, 2),
            Up::Ad(3, 2_500_000, 3, 3),
            Up::Ad(4, 3_000_000, 4, 4),
            Up::Ad(5, 4_000_000, 5, 5),
            Up::Ad(6, 5_000_000, 6, 6),
            Up::Ad(7, 6_000_000, 7, 7),
        ],
    );
    let arrivals: Vec<(u64, u64)> = [1_000_000, 2_000_000, 2_250_000, 2_500_000]
        .into_iter()
        .chain([3_000_000, 4_000_000, 5_000_000, 6_000_000])
        .enumerate()
        .map(|(i, t)| (i as u64, t + DELAY))
        .collect();
    assert_eq!(up_arrivals("a3-nominal"), arrivals);
    assert_eq!(faults("a3-nominal").len(), 0);
}

#[test]
fn nominal_ocf_and_events_follow_the_table() {
    // req: APP-150, APP-147, APP-149, APP-146
    let case = "a3-nominal";
    // downlink frame, N(R), clcw_changed instant, FDU acknowledged (§4.5.5)
    let rows: [(usize, u64, u64); 8] = [
        (9, 1_135_000, 0),
        (17, 2_135_000, 1),
        (19, 2_385_000, 2),
        (21, 2_635_000, 3),
        (25, 3_135_000, 4),
        (33, 4_135_000, 5),
        (41, 5_135_000, 6),
        (49, 6_135_000, 7),
    ];
    let down = units(case, "down");
    let a2 = units("a2-nominal", "down");
    assert_eq!(down.len(), 280);
    let mut word = 0x0100_0000_u32;
    for (i, (_, f)) in down.iter().enumerate() {
        if let Some(r) = rows.iter().find(|r| r.0 == i) {
            word = 0x0100_0000 + u32::try_from(r.2 + 1).unwrap();
        }
        let ocf = u32::from_be_bytes([f[217], f[218], f[219], f[220]]);
        assert_eq!(ocf, word, "frame {i}: OCF (232.0 §4.2.1)");
        // The downlink is that of `a2-nominal` except for the OCF and the FECF.
        assert_eq!(f[..217], a2[i].1[..217], "frame {i}: as in a2-nominal");
        assert_eq!(f.len(), 223);
        check_fecf(case, i as u64, f);
    }
    let mut expected = directive0();
    let mut flights = vec![];
    for (i, t, fdu) in rows {
        let word = 0x0100_0000 + fdu + 1;
        assert_eq!(down[i].0, i as u64 * SLOT);
        assert_eq!(t, i as u64 * SLOT + DELAY);
        expected.extend(ack(t, word, fdu));
    }
    expected.push(dump());
    expected.sort_by_key(|e| e.0);
    assert_eq!(app_events(case, "ground"), expected);
    flights.extend([
        event(1_010_000, "cmd_received", &[1, 0]),
        event(2_510_000, "table_received", &[1, 300]),
        event(3_010_000, "cmd_received", &[3, 1]),
        event(4_010_000, "cmd_received", &[2, 2]),
        event(5_010_000, "cmd_received", &[4, 3]),
        event(5_010_000, "cmd_received", &[1, 4]),
        event(6_010_000, "cmd_received", &[4, 5]),
    ]);
    assert_eq!(app_events(case, "flight"), flights, "no FARM-1 event");
}

#[test]
fn nominal_packet_projection_equals_the_a1_one_and_the_instants_agree() {
    // req: APP-003, APP-150
    let a1 = std::fs::read_to_string(golden_path("a1-nominal")).unwrap();
    let a3 = std::fs::read_to_string(golden_path("a3-nominal")).unwrap();
    assert_eq!(
        packet_projection(&a3).unwrap(),
        packet_projection(&a1).unwrap(),
        "APP-003: the packet projection of the nominal case is the same at every link level"
    );
    for node in ["flight", "ground"] {
        let x = packets("a1-nominal", "packet_out", node);
        let y = packets("a3-nominal", "packet_out", node);
        assert!(!x.is_empty(), "{node} outputs packets");
        assert_eq!(y, x, "{node}: packet_out instants and octets");
    }
}

#[test]
fn uplink_loss_follows_the_table() {
    // req: APP-151, APP-145, APP-075, APP-044
    let case = "a3-uplink-loss";
    let mut expected_faults = vec![fault("up", 2, "drop", &[])];
    expected_faults.extend((33..40).map(|i| fault("down", i, "drop", &[])));
    expected_faults.push(fault("up", 10, "drop", &[]));
    assert_eq!(faults(case), expected_faults);
    check_uplink(
        case,
        &[
            Up::Ad(0, 1_000_000, 0, 0),
            Up::Ad(1, 2_000_000, 1, 1),
            Up::Ad(2, 2_250_000, 2, 2),
            Up::Ad(3, 2_500_000, 3, 3),
            Up::Ad(4, 2_750_000, 2, 2),
            Up::Ad(5, 3_000_000, 3, 3),
            Up::Ad(6, 3_250_000, 4, 4),
            Up::Ad(7, 4_000_000, 5, 5),
            Up::Ad(8, 5_000_000, 5, 5),
            Up::Ad(9, 5_250_000, 6, 6),
            Up::Ad(10, 6_000_000, 7, 7),
            Up::Ad(11, 7_000_000, 7, 7),
        ],
    );
    // A retransmission has the octets of its first transmission (COP-011).
    let up = units(case, "up");
    for (a, b) in [(2, 4), (3, 5), (7, 8), (10, 11)] {
        assert_eq!(up[a].1, up[b].1, "units {a} and {b}: same octets");
    }
    let arrived: Vec<u64> = up_arrivals(case).iter().map(|a| a.0).collect();
    assert_eq!(arrived, [0, 1, 3, 4, 5, 6, 7, 8, 9, 11]);
    for (i, t) in up_arrivals(case) {
        assert_eq!(t, up[ix(i)].0 + DELAY);
    }
    let mut expected = directive0();
    expected.extend(ack(1_135_000, 16_777_217, 0));
    expected.extend(ack(2_135_000, 16_777_218, 1));
    expected.extend([
        event(2_635_000, "clcw_changed", &[16_779_266]),
        event(2_635_000, "fop1_state", &[2]),
        event(2_885_000, "clcw_changed", &[16_777_219]),
        event(2_885_000, "fop1_transfer", &[3, 2]),
        event(2_885_000, "fop1_state", &[1]),
    ]);
    expected.extend(ack(3_135_000, 16_777_220, 3));
    expected.extend(ack(3_385_000, 16_777_221, 4));
    expected.extend([
        event(5_010_000, "frame_gap", &[0, 0, 7]),
        event(5_010_000, "frame_gap", &[1, 7, 5]),
    ]);
    expected.extend(ack(5_010_000, 16_777_222, 5));
    expected.extend([
        event(5_260_000, "frame_gap", &[1, 0, 1]),
        event(5_260_000, "seq_gap", &[16, 1]),
    ]);
    expected.extend(ack(5_385_000, 16_777_223, 6));
    expected.push(event(5_510_000, "frame_gap", &[1, 1, 1]));
    expected.extend(ack(7_135_000, 16_777_224, 7));
    expected.push(event(7_510_000, "seq_gap", &[32, 1]));
    expected.push(dump());
    assert_eq!(app_events(case, "ground"), expected);
    assert_eq!(
        app_events(case, "flight"),
        vec![
            event(1_010_000, "cmd_received", &[1, 0]),
            event(2_510_000, "frame_discarded", &[2, 3]),
            event(3_010_000, "table_received", &[1, 300]),
            event(3_260_000, "cmd_received", &[3, 1]),
            event(4_010_000, "cmd_received", &[2, 2]),
            event(5_010_000, "frame_discarded", &[2, 4]),
            event(5_260_000, "cmd_received", &[4, 3]),
            event(5_260_000, "cmd_received", &[1, 4]),
            event(7_010_000, "cmd_received", &[4, 5]),
        ]
    );
    // The first CLCW that shows the Retransmit flag (232.0 §4.2.1.9): slot 21, N(R) = 2.
    let down = units(case, "down");
    let ocf = |i: usize| {
        u32::from_be_bytes([
            down[i].1[217],
            down[i].1[218],
            down[i].1[219],
            down[i].1[220],
        ])
    };
    assert_eq!(ocf(20), 0x0100_0002);
    assert_eq!(ocf(21), 0x0100_0802, "Retransmit, N(R) = 2");
    assert_eq!(ocf(22), 0x0100_0802);
    assert_eq!(ocf(23), 0x0100_0003, "N(R) = 3, Retransmit reset");
    // Every command executes as in the nominal case, and the flight node's packets are the
    // same but for the housekeeping packet of 7.0 s, which counts the last command one second
    // later (it arrives at 7.010 s instead of 6.010 s). Two downlink packets are lost.
    assert_eq!(reports(case), reports("a3-nominal"));
    assert_eq!(
        without_hk_of_7s(packets(case, "packet_out", "flight")),
        without_hk_of_7s(packets("a3-nominal", "packet_out", "flight"))
    );
    let received = |c: &str| packets(c, "packet_in", "ground");
    assert_eq!(received("a3-nominal").len() - received(case).len(), 2);
    let seen: Vec<(u64, u64)> = received(case).iter().map(|p| (p.0, p.1)).collect();
    assert!(
        !seen.contains(&(4_260_000, 16)) && !seen.contains(&(4_510_000, 32)),
        "housekeeping packet 4 and payload packet 2 are lost"
    );
}

/// A node's packets without the housekeeping packet of the second 7 s (generated at 7.0 s, received at 7.26 s).
fn without_hk_of_7s(v: Vec<(u64, u64, String)>) -> Vec<(u64, u64, String)> {
    v.into_iter()
        .filter(|p| !(p.0 / 1_000_000 == 7 && p.1 == 16))
        .collect()
}

#[test]
fn uplink_disorder_follows_the_table() {
    // req: APP-151, APP-075, APP-044
    let case = "a3-uplink-disorder";
    assert_eq!(
        faults(case),
        vec![
            fault("up", 0, "duplicate", &[]),
            fault("up", 2, "delay", &[400_000]),
            fault("up", 9, "flip", &[12, 7]),
        ]
    );
    check_uplink(
        case,
        &[
            Up::Ad(0, 1_000_000, 0, 0),
            Up::Ad(1, 2_000_000, 1, 1),
            Up::Ad(2, 2_250_000, 2, 2),
            Up::Ad(3, 2_500_000, 3, 3),
            Up::Ad(4, 2_750_000, 2, 2),
            Up::Ad(5, 3_000_000, 3, 3),
            Up::Ad(6, 3_250_000, 4, 4),
            Up::Ad(7, 4_000_000, 5, 5),
            Up::Ad(8, 5_000_000, 6, 6),
            // the sent frame is the unflipped one: a fault applies to the delivered octets
            Up::Ad(9, 6_000_000, 7, 7),
            Up::Ad(10, 7_000_000, 7, 7),
        ],
    );
    let mut arrivals = up_arrivals(case);
    arrivals.sort_by_key(|a| (a.1, a.0));
    let expected: Vec<(u64, u64)> = vec![
        (0, 1_010_000),
        (0, 1_010_000),
        (1, 2_010_000),
        (3, 2_510_000),
        (2, 2_660_000),
        (4, 2_760_000),
        (5, 3_010_000),
        (6, 3_260_000),
        (7, 4_010_000),
        (8, 5_010_000),
        (9, 6_010_000),
        (10, 7_010_000),
    ];
    assert_eq!(arrivals, expected);
    let mut expected = directive0();
    expected.extend(ack(1_135_000, 16_777_217, 0));
    expected.extend(ack(2_135_000, 16_777_218, 1));
    expected.extend([
        event(2_635_000, "clcw_changed", &[16_779_266]),
        event(2_635_000, "fop1_state", &[2]),
        event(2_760_000, "clcw_changed", &[16_777_219]),
        event(2_760_000, "fop1_transfer", &[3, 2]),
        event(2_760_000, "fop1_state", &[1]),
    ]);
    expected.extend(ack(3_135_000, 16_777_220, 3));
    expected.extend(ack(3_385_000, 16_777_221, 4));
    expected.extend(ack(4_135_000, 16_777_222, 5));
    expected.extend(ack(5_135_000, 16_777_223, 6));
    expected.extend(ack(7_135_000, 16_777_224, 7));
    expected.push(dump());
    assert_eq!(app_events(case, "ground"), expected);
    assert_eq!(
        app_events(case, "flight"),
        vec![
            event(1_010_000, "cmd_received", &[1, 0]),
            event(1_010_000, "frame_discarded", &[2, 4]),
            event(2_510_000, "frame_discarded", &[2, 3]),
            event(2_760_000, "frame_discarded", &[2, 4]),
            event(3_010_000, "table_received", &[1, 300]),
            event(3_260_000, "cmd_received", &[3, 1]),
            event(4_010_000, "cmd_received", &[2, 2]),
            event(5_010_000, "cmd_received", &[4, 3]),
            event(5_010_000, "cmd_received", &[1, 4]),
            event(6_010_000, "frame_rejected", &[8]),
            event(7_010_000, "cmd_received", &[4, 5]),
        ]
    );
    assert_eq!(reports(case), reports("a3-nominal"));
    assert_eq!(
        without_hk_of_7s(packets(case, "packet_in", "ground")),
        without_hk_of_7s(packets("a3-nominal", "packet_in", "ground")),
        "the downlink has no loss; the housekeeping of 7.0 s counts the last command later"
    );
}

#[test]
fn uplink_lockout_follows_the_table() {
    // req: APP-151, APP-145, APP-148, APP-146, APP-075
    let case = "a3-uplink-lockout";
    assert_eq!(faults(case), vec![fault("down", 16, "drop", &[])]);
    check_uplink(
        case,
        &[
            Up::Ad(0, 1_000_000, 0, 0),
            Up::Ad(1, 2_000_000, 128, 1),
            Up::Unlock(2, 2_500_000),
            Up::Ad(3, 3_000_000, 1, 4),
            Up::Ad(4, 4_000_000, 2, 5),
            Up::Ad(5, 5_000_000, 3, 6),
            Up::Ad(6, 6_000_000, 4, 7),
        ],
    );
    let mut expected = directive0();
    expected.extend(ack(1_135_000, 16_777_217, 0));
    expected.extend([
        event(2_000_000, "fop1_directive", &[1, 5, 0]),
        event(2_000_000, "fop1_alert", &[8, 29, 1]),
        event(2_000_000, "fop1_directive", &[3, 5, 0]),
        event(2_000_000, "fop1_state", &[6]),
        event(2_000_000, "fop1_directive", &[1, 7, 128]),
        event(2_000_000, "fop1_directive", &[3, 7, 128]),
        event(2_000_000, "fop1_directive", &[1, 1, 0]),
        event(2_000_000, "fop1_directive", &[3, 1, 0]),
        event(2_000_000, "fop1_state", &[1]),
        event(2_135_000, "frame_gap", &[0, 0, 1]),
        event(2_135_000, "frame_gap", &[1, 7, 1]),
        event(2_135_000, "clcw_changed", &[16_785_409]),
        event(2_135_000, "fop1_transfer", &[4, 1]),
        event(2_135_000, "fop1_alert", &[3, 14, 1]),
        event(2_135_000, "fop1_state", &[6]),
        event(2_250_000, "fop1_directive", &[1, 5, 0]),
        event(2_250_000, "fop1_directive", &[3, 5, 0]),
        event(2_250_000, "fop1_transfer", &[2, 2]),
        event(2_375_000, "fop1_directive", &[1, 7, 1]),
        event(2_375_000, "fop1_directive", &[3, 7, 1]),
        event(2_500_000, "fop1_directive", &[1, 3, 0]),
        event(2_500_000, "fop1_state", &[5]),
        event(2_500_000, "fop1_transfer", &[2, 3]),
        event(2_635_000, "clcw_changed", &[16_777_729]),
        event(2_635_000, "fop1_directive", &[3, 3, 0]),
        event(2_635_000, "fop1_state", &[1]),
    ]);
    expected.extend(ack(3_135_000, 16_777_730, 4));
    expected.extend(ack(4_135_000, 16_777_731, 5));
    expected.extend(ack(5_135_000, 16_777_732, 6));
    expected.extend(ack(6_135_000, 16_777_733, 7));
    expected.push(dump());
    assert_eq!(app_events(case, "ground"), expected);
    assert_eq!(
        app_events(case, "flight"),
        vec![
            event(1_010_000, "cmd_received", &[1, 0]),
            event(2_010_000, "frame_discarded", &[2, 5]),
            event(2_010_000, "farm1_state", &[3]),
            event(2_510_000, "farm1_unlock", &[]),
            event(2_510_000, "farm1_state", &[1]),
            event(3_010_000, "cmd_received", &[3, 1]),
            event(4_010_000, "cmd_received", &[2, 2]),
            event(5_010_000, "cmd_received", &[4, 3]),
            event(5_010_000, "cmd_received", &[1, 4]),
            event(6_010_000, "cmd_received", &[4, 5]),
        ]
    );
    // The CLCW words of the slots: Lockout (0x2000) from the slot of 2.0 s' arrival until the
    // Unlock; N(R) = 1; the FARM-B Counter 1 after the Unlock (232.0 §4.2.1; 232.1 §6.1.3).
    let down = units(case, "down");
    let ocf = |i: usize| {
        u32::from_be_bytes([
            down[i].1[217],
            down[i].1[218],
            down[i].1[219],
            down[i].1[220],
        ])
    };
    assert_eq!(ocf(17), 0x0100_2001, "slot 2.125 s: Lockout, N(R) = 1");
    assert_eq!(
        ocf(20),
        0x0100_2001,
        "slot 2.5 s: before the Unlock arrives"
    );
    assert_eq!(
        ocf(21),
        0x0100_0201,
        "slot 2.625 s: clean, FARM-B Counter 1"
    );
    // On board the table never arrives: the activation finds none (§6.8).
    let nominal = reports("a3-nominal");
    let lost = reports(case);
    assert_eq!(nominal.len(), lost.len());
    let mut changed = 0;
    for (n, l) in nominal.iter().zip(&lost) {
        if n != l {
            changed += 1;
            assert_eq!(l, &(10_000_000, TABLE_REJECTED, vec![1, 1, 0]));
            assert_eq!(n.0, 10_000_000);
        }
    }
    assert_eq!(changed, 1, "only the activation differs");
}

#[test]
fn uplink_limit_follows_the_table() {
    // req: APP-151, APP-145, APP-148, APP-075, APP-044
    let case = "a3-uplink-limit";
    assert_eq!(
        faults(case),
        vec![
            fault("up", 7, "drop", &[]),
            fault("up", 8, "drop", &[]),
            fault("up", 9, "drop", &[]),
        ]
    );
    check_uplink(
        case,
        &[
            Up::Ad(0, 1_000_000, 0, 0),
            Up::Ad(1, 2_000_000, 1, 1),
            Up::Ad(2, 2_250_000, 2, 2),
            Up::Ad(3, 2_500_000, 3, 3),
            Up::Ad(4, 3_000_000, 4, 4),
            Up::Ad(5, 4_000_000, 5, 5),
            Up::Ad(6, 5_000_000, 6, 6),
            Up::Ad(7, 6_000_000, 7, 7),
            Up::Ad(8, 7_000_000, 7, 7),
            Up::Ad(9, 8_000_000, 7, 7),
        ],
    );
    // Three transmissions, 1 000 000 µs apart, the last timer expiring at 9.0 s (E17).
    let mut expected = directive0();
    for (t, w, n) in [
        (1_135_000, 16_777_217, 0),
        (2_135_000, 16_777_218, 1),
        (2_385_000, 16_777_219, 2),
        (2_635_000, 16_777_220, 3),
        (3_135_000, 16_777_221, 4),
        (4_135_000, 16_777_222, 5),
        (5_135_000, 16_777_223, 6),
    ] {
        expected.extend(ack(t, w, n));
    }
    expected.extend([
        event(9_000_000, "fop1_transfer", &[4, 7]),
        event(9_000_000, "fop1_alert", &[2, 17, 1]),
        event(9_000_000, "fop1_state", &[6]),
        event(10_000_000, "fop1_directive", &[1, 7, 7]),
        event(10_000_000, "fop1_directive", &[3, 7, 7]),
        event(11_000_000, "fop1_directive", &[1, 2, 0]),
        event(11_000_000, "fop1_state", &[4]),
        event(11_010_000, "fop1_directive", &[3, 2, 0]),
        event(11_010_000, "fop1_state", &[1]),
        dump(),
    ]);
    expected.sort_by_key(|e| e.0);
    assert_eq!(app_events(case, "ground"), expected);
    assert_eq!(
        app_events(case, "flight"),
        vec![
            event(1_010_000, "cmd_received", &[1, 0]),
            event(2_510_000, "table_received", &[1, 300]),
            event(3_010_000, "cmd_received", &[3, 1]),
            event(4_010_000, "cmd_received", &[2, 2]),
            event(5_010_000, "cmd_received", &[4, 3]),
            event(5_010_000, "cmd_received", &[1, 4]),
        ]
    );
    // The CLCW words stay at N(R) = 7 after 5.125 s, and FARM-1 raises no event.
    let down = units(case, "down");
    for (i, (_, f)) in down.iter().enumerate().skip(41) {
        let ocf = u32::from_be_bytes([f[217], f[218], f[219], f[220]]);
        assert_eq!(ocf, 0x0100_0007, "frame {i}: OCF");
    }
    // `SET_PAYLOAD_MODE` (on) never arrives: 8 event packets (no CMD_EXECUTED and
    // PAYLOAD_MODE at 24.0 s), 10 payload packets.
    let count = |case: &str, apid: u64| {
        packets(case, "packet_out", "flight")
            .iter()
            .filter(|p| p.1 == apid)
            .count()
    };
    assert_eq!((count(case, 17), count(case, 32)), (8, 10));
    assert_eq!((count("a3-nominal", 17), count("a3-nominal", 32)), (10, 13));
    assert!(reports(case).iter().all(|r| r.0 < 24_000_000));
}

fn ix(i: u64) -> usize {
    usize::try_from(i).unwrap()
}

/// `n` bits of `word` from bit `first` (bit 0 is the most significant of `width` bits).
const fn field(word: u32, width: u32, first: u32, n: u32) -> u32 {
    (word >> (width - first - n)) & ((1 << n) - 1)
}

/// The Transfer Frame Primary Header of a TC frame, field by field (232.0 §4.1.2): version,
/// bypass, control command, spare, SCID, VCID, frame length, N(S).
fn tc_header(f: &[u8]) -> [u32; 8] {
    let w = u32::from_be_bytes([f[0], f[1], f[2], f[3]]);
    [
        field(w, 32, 0, 2),
        field(w, 32, 2, 1),
        field(w, 32, 3, 1),
        field(w, 32, 4, 2),
        field(w, 32, 6, 10),
        field(w, 32, 16, 6),
        field(w, 32, 22, 10),
        u32::from(f[4]),
    ]
}

/// A CLCW, field by field (232.0 §4.2.1): type, version, status, COP in effect, VCID, spare,
/// No RF, No Bit Lock, Lockout, Wait, Retransmit, FARM-B Counter, spare, Report Value.
const fn clcw(w: u32) -> [u32; 14] {
    [
        field(w, 32, 0, 1),
        field(w, 32, 1, 2),
        field(w, 32, 3, 3),
        field(w, 32, 6, 2),
        field(w, 32, 8, 6),
        field(w, 32, 14, 2),
        field(w, 32, 16, 1),
        field(w, 32, 17, 1),
        field(w, 32, 18, 1),
        field(w, 32, 19, 1),
        field(w, 32, 20, 1),
        field(w, 32, 21, 2),
        field(w, 32, 23, 1),
        field(w, 32, 24, 8),
    ]
}

#[test]
fn first_frame_of_each_kind_by_hand() {
    // req: APP-150, APP-151, APP-090, APP-147
    // D8(b): the first frame of each kind decoded field by field, against 232.0-B-4 and
    // 232.1-B-2 (the clauses are in the pull request).
    let nominal = units("a3-nominal", "up");
    let loss = units("a3-uplink-loss", "up");
    // The AD frame with N(S) = 0: Type-AD (Table 4-1), SCID 0x155, VC 0, 22 octets, N(S) 0,
    // Segment Header `C0` (no segmentation, MAP 0; Table 4-2), one 14-octet telecommand packet.
    let f = &nominal[0].1;
    assert_eq!(tc_header(f), [0, 0, 0, 0, 0x155, 0, 21, 0]);
    assert_eq!(f[5], 0xC0);
    assert_eq!(f.len(), 22);
    assert_eq!(f[6..20], unhex("1080C000000701017D2B75060000")[..]);
    check_fecf("a3-nominal", 0, f);
    // The retransmitted AD frame: unit 4 of `a3-uplink-loss` repeats unit 2, N(S) 2, with the
    // Segment Header `01` (first portion, MAP 1), 128 octets, at 2.75 s (232.1 §5.2.4 a, §5.2.7 d).
    assert_eq!(loss[2].1, loss[4].1);
    assert_eq!((loss[2].0, loss[4].0), (2_250_000, 2_750_000));
    let f = &loss[4].1;
    assert_eq!(tc_header(f), [0, 0, 0, 0, 0x155, 0, 127, 2]);
    assert_eq!(f[5], 0x01);
    assert_eq!(f.len(), 128);
    check_fecf("a3-uplink-loss", 4, f);
    // The Unlock frame: Type-BC (bypass 1, control command 1), length 7 (8 octets), N(S) 0, no
    // Segment Header (232.0 §4.1.3.2.2.1.3), the data octet 0x00 (§4.1.3.3.2).
    let lockout = units("a3-uplink-lockout", "up");
    assert_eq!(lockout[2].0, 2_500_000);
    let f = &lockout[2].1;
    assert_eq!(tc_header(f), [0, 1, 1, 0, 0x155, 0, 7, 0]);
    assert_eq!(f[5], 0x00);
    assert_eq!(f.len(), 8);
    check_fecf("a3-uplink-lockout", 2, f);
    // The CLCW with the Retransmit flag (232.0 §4.2.1.8.6): 0x01000802, VC 0.
    // The OCF is the four octets before the two-octet FECF that ends each
    // 223-octet downlink frame, so octets 217 to 220.
    let ocf = |case: &str, i: usize| {
        let d = units(case, "down");
        u32::from_be_bytes([d[i].1[217], d[i].1[218], d[i].1[219], d[i].1[220]])
    };
    let w = ocf("a3-uplink-loss", 21);
    assert_eq!(w, 0x0100_0802);
    assert_eq!(clcw(w), [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 2]);
    // The CLCW with the Lockout flag (§4.2.1.8.4): 0x01002001, N(R) 1.
    let w = ocf("a3-uplink-lockout", 17);
    assert_eq!(w, 0x0100_2001);
    assert_eq!(clcw(w), [0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
}
