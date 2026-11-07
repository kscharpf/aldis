//! The invariants of the A1 cases, checked on their golden traces (A1 WP-5;
//! docs/reference-application.md §2.4, §3.3.2, §6.6).
//!
//! Each case's expected reports and ground events are written out here from the tables of the
//! document, not read back from the trace.

use std::collections::BTreeMap;

use refapp_harness::scenario::{OpsEntry, Scenario};
use refapp_harness::trace::{NodeId, PacketRecord, Projection, check_canonical, packet_projection};
use refapp_harness::{golden_path, run_case};
use serde_json::Value;

const FLIGHT: NodeId = NodeId::Flight;
const GROUND: NodeId = NodeId::Ground;

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

/// How many packets of each (node, APID) the trace generated.
fn generated(case: &str) -> BTreeMap<(&'static str, u64), usize> {
    let mut counts = BTreeMap::new();
    for r in records(case) {
        if r["kind"] == "packet_out" {
            let node = if r["node"] == "flight" {
                "flight"
            } else {
                "ground"
            };
            *counts
                .entry((node, r["apid"].as_u64().unwrap()))
                .or_default() += 1;
        }
    }
    counts
}

/// One event report of §3.3.2: instant, code, p1, p2, p3.
type Report = (u64, u16, u16, u16, u16);

/// The event packets (APID `0x011`, 21 octets) of the flight node, in trace order.
fn reports(case: &str) -> Vec<Report> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "packet_out" && r["apid"] == 17)
        .map(|r| {
            let packet = unhex(r["hex"].as_str().unwrap());
            assert_eq!(packet.len(), 21, "event packet length (§3.3.2)");
            let field = |i: usize| u16::from_be_bytes([packet[13 + 2 * i], packet[14 + 2 * i]]);
            (
                r["t_us"].as_u64().unwrap(),
                field(0),
                field(1),
                field(2),
                field(3),
            )
        })
        .collect()
}

const CMD_EXECUTED: u16 = 0x0001;
const CMD_REJECTED: u16 = 0x0002;
const TABLE_ACTIVATED: u16 = 0x0010;
const TABLE_REJECTED: u16 = 0x0011;
const DUMP_COMPLETED: u16 = 0x0020;
const PAYLOAD_MODE: u16 = 0x0030;

/// The application events of `node` as (instant, name, args), in trace order.
fn app_events(case: &str, node: &str) -> Vec<(u64, String, Vec<u64>)> {
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

fn ground_events(case: &str) -> Vec<(u64, String, Vec<u64>)> {
    app_events(case, "ground")
}

fn event(t: u64, name: &str, args: &[u64]) -> (u64, String, Vec<u64>) {
    (t, name.to_owned(), args.to_vec())
}

fn count_of(c: &BTreeMap<(&'static str, u64), usize>, node: &'static str, apid: u64) -> usize {
    c.get(&(node, apid)).copied().unwrap_or(0)
}

/// The packet counts of §2.4 for the nominal timeline, plus the extra payload packet and
/// command packets of the case.
fn assert_counts(case: &str, hk: usize, payload: usize, dump: usize, events: usize, cmds: usize) {
    let c = generated(case);
    assert_eq!(count_of(&c, "flight", 16), hk, "{case}: housekeeping");
    assert_eq!(count_of(&c, "flight", 32), payload, "{case}: payload");
    assert_eq!(count_of(&c, "flight", 33), dump, "{case}: dump");
    assert_eq!(count_of(&c, "flight", 17), events, "{case}: events");
    assert_eq!(count_of(&c, "ground", 128), cmds, "{case}: commands");
}

#[test]
fn nominal_counts_and_reports_follow_the_timeline() {
    // req: APP-019, APP-082, APP-083
    // §2.4: 30 HK, 13 payload, 5 dump, 10 event packets; six commands and one table upload.
    assert_counts("a1-nominal", 30, 13, 5, 10, 6);
    assert_eq!(count_of(&generated("a1-nominal"), "ground", 129), 1);
    let expected: Vec<Report> = vec![
        (6_000_000, CMD_EXECUTED, 1, 0, 0),
        (10_000_000, CMD_EXECUTED, 3, 1, 0),
        (10_000_000, TABLE_ACTIVATED, 1, 300, 0x0C4F),
        (12_000_000, CMD_EXECUTED, 2, 2, 0),
        (12_500_000, DUMP_COMPLETED, 1, 0, 600),
        (20_000_000, CMD_EXECUTED, 4, 3, 0),
        (20_000_000, PAYLOAD_MODE, 0, 0, 0),
        (20_000_000, CMD_EXECUTED, 1, 4, 0),
        (24_000_000, CMD_EXECUTED, 4, 5, 0),
        (24_000_000, PAYLOAD_MODE, 1, 0, 0),
    ];
    assert_eq!(reports("a1-nominal"), expected);
    // The payload stops at 20 s and resumes at 24 s: 0.5…18.5 s and 24.5…28.5 s.
    let payload_times: Vec<u64> = records("a1-nominal")
        .iter()
        .filter(|r| r["kind"] == "packet_out" && r["apid"] == 32)
        .map(|r| r["t_us"].as_u64().unwrap())
        .collect();
    let expected_times: Vec<u64> = (0..10)
        .chain(12..15)
        .map(|k| 500_000 + 2_000_000 * k)
        .collect();
    assert_eq!(payload_times, expected_times);
}

#[test]
fn nominal_has_no_gap_and_the_dump_arrives_whole() {
    // req: APP-019, APP-011, APP-018
    // No fault is injected, so the ground node raises only the reassembled dump (§2.4: CRC
    // 0x80A8 = 32 936).
    assert_eq!(
        ground_events("a1-nominal"),
        [event(
            12_510_000,
            "dump_reassembled",
            &[65_536, 600, 0x80A8]
        )]
    );
    assert!(records("a1-nominal").iter().all(|r| r["kind"] != "fault"));
    // Every packet the flight node generated is delivered, in order.
    let p =
        packet_projection(&std::fs::read_to_string(golden_path("a1-nominal")).unwrap()).unwrap();
    for ((record, node, apid), packets) in &p {
        if *record == PacketRecord::In && *node == GROUND {
            assert_eq!(
                p.get(&(PacketRecord::Out, FLIGHT, *apid)),
                Some(packets),
                "apid {apid}"
            );
        }
    }
}

/// Every command of the case's script gets exactly one `cmd_received` and one report, in
/// non-decreasing time and, within an instant, in command order (APP-012, APP-013).
fn assert_every_command_once_and_in_order(case: &str) {
    let scenario = Scenario::load().unwrap();
    let commands = scenario
        .case(case)
        .unwrap()
        .ops
        .iter()
        .filter(|e| matches!(e, OpsEntry::Command { .. }))
        .count();
    let received: Vec<u64> = app_events(case, "flight")
        .iter()
        .filter(|(_, name, _)| name == "cmd_received")
        .map(|(_, _, args)| args[1])
        .collect();
    let seqs: Vec<u64> = (0..commands as u64).collect();
    assert_eq!(
        received, seqs,
        "{case}: commands delivered in release order"
    );
    let verdicts: Vec<(u64, u16)> = reports(case)
        .iter()
        .filter(|r| r.1 == CMD_EXECUTED || r.1 == CMD_REJECTED)
        .map(|r| (r.0, r.3))
        .collect();
    let mut once: Vec<u16> = verdicts.iter().map(|v| v.1).collect();
    once.sort_unstable();
    assert_eq!(
        once,
        (0..u16::try_from(commands).unwrap()).collect::<Vec<_>>(),
        "{case}: exactly one report per command"
    );
    for pair in verdicts.windows(2) {
        assert!(pair[0].0 <= pair[1].0, "{case}: reports in time order");
        if pair[0].0 == pair[1].0 {
            assert!(
                pair[0].1 < pair[1].1,
                "{case}: one instant in command order"
            );
        }
    }
}

#[test]
fn every_command_executes_once_and_in_order() {
    // req: APP-012, APP-013, APP-129
    for case in [
        "a1-nominal",
        "a1-immediate",
        "a1-rejects",
        "a1-downlink-faults",
    ] {
        assert_every_command_once_and_in_order(case);
    }
}

#[test]
fn immediate_commands_execute_on_delivery() {
    // req: APP-012, APP-013, APP-127, APP-128
    // The nominal case plus two immediate commands: two more command packets, one more payload
    // packet (22.5 s), and three more event packets.
    assert_counts("a1-immediate", 30, 14, 5, 13, 8);
    let r = reports("a1-immediate");
    let nominal = reports("a1-nominal");
    // The nominal reports stay, in order, and the new ones are inserted at their instants.
    let kept: Vec<Report> = r
        .iter()
        .copied()
        .filter(|x| x.0 != 14_125_000 && x.0 != 21_125_000)
        .collect();
    assert_eq!(kept, nominal);
    let at = |t: u64| -> Vec<Report> { r.iter().copied().filter(|x| x.0 == t).collect() };
    assert_eq!(at(14_125_000), [(14_125_000, CMD_EXECUTED, 1, 6, 0)]);
    assert_eq!(
        at(21_125_000),
        [
            (21_125_000, CMD_EXECUTED, 4, 7, 0),
            (21_125_000, PAYLOAD_MODE, 1, 0, 0)
        ]
    );
    // The payload at 22.5 s exists only because the immediate command switched the mode on.
    assert!(
        records("a1-immediate")
            .iter()
            .any(|x| x["kind"] == "packet_out" && x["apid"] == 32 && x["t_us"] == 22_500_000)
    );
    assert_eq!(
        ground_events("a1-immediate"),
        [event(
            12_510_000,
            "dump_reassembled",
            &[65_536, 600, 0x80A8]
        )]
    );
}

#[test]
fn every_rejection_reason_reports_as_specified() {
    // req: APP-013, APP-016, APP-017, APP-024, APP-127, APP-129
    // §6.6: (p1, p2, p3) of each report, at its instant. The nine NOOP at 10.115 s give seq
    // 16 the queue-full rejection and run seqs 8…15 at 20 s.
    let mut expected: Vec<Report> = vec![
        (125_000, CMD_EXECUTED, 1, 0, 0),
        (625_000, CMD_REJECTED, 127, 1, 1),
        (3_000_000, CMD_REJECTED, 1, 2, 3),
        (3_625_000, CMD_REJECTED, 1, 3, 2),
        (4_125_000, CMD_EXECUTED, 3, 4, 0),
        (4_125_000, TABLE_REJECTED, 1, 1, 0),
        (6_125_000, CMD_EXECUTED, 3, 5, 0),
        (6_125_000, TABLE_REJECTED, 1, 2, 0),
        (8_125_000, CMD_EXECUTED, 2, 6, 0),
        (8_250_000, CMD_REJECTED, 2, 7, 5),
        (8_375_000, DUMP_COMPLETED, 1, 0, 300),
        (10_125_000, CMD_REJECTED, 1, 16, 4),
    ];
    expected.extend((8..16).map(|n| (20_000_000, CMD_EXECUTED, 1, n, 0)));
    assert_eq!(reports("a1-rejects"), expected);
    // 21 HK (t < 21 s), 11 payload (0.5…20.5 s), 3 dump, 20 event packets; 17 commands.
    assert_counts("a1-rejects", 21, 11, 3, 20, 17);
    // TABLE_ACTIVATED never occurs: both activations are rejected.
    assert!(reports("a1-rejects").iter().all(|r| r.1 != TABLE_ACTIVATED));
    // One application event of the ground node; no sequence or rejected-packet event.
    assert_eq!(
        ground_events("a1-rejects"),
        [event(8_385_000, "dump_reassembled", &[65_536, 300, 0x7FE0])]
    );
}

#[test]
fn rejected_commands_raise_cmd_received_before_their_report() {
    // req: APP-129
    // The command of the rejection at delivery (seq 3, 3.625 s) is received and reported in
    // one step: cmd_received first (APP-129), then CMD_REJECTED.
    let recs = records("a1-rejects");
    let received = recs
        .iter()
        .position(|r| r["kind"] == "app_event" && r["name"] == "cmd_received" && r["args"][1] == 3)
        .unwrap();
    let report = recs
        .iter()
        .position(|r| r["kind"] == "packet_out" && r["t_us"] == 3_625_000 && r["apid"] == 17)
        .unwrap();
    assert!(received < report);
    let uploads = app_events("a1-rejects", "flight")
        .into_iter()
        .filter(|(_, name, _)| name == "table_received")
        .collect::<Vec<_>>();
    assert_eq!(uploads, [event(5_125_000, "table_received", &[1, 300])]);
}

#[test]
fn faults_change_only_what_the_ground_node_receives() {
    // req: APP-011, APP-018, APP-072, APP-075, APP-127, APP-130
    let faults = records("a1-downlink-faults");
    let injected: Vec<(u64, &str)> = faults
        .iter()
        .filter(|r| r["kind"] == "fault")
        .map(|r| (r["index"].as_u64().unwrap(), r["fault"].as_str().unwrap()))
        .collect();
    assert_eq!(
        injected,
        [
            (2, "drop"),
            (4, "duplicate"),
            (5, "delay"),
            (11, "truncate"),
            (25, "drop")
        ]
    );
    // The flight node's output is that of a1-nominal.
    let flight_out = |case: &str| -> Vec<String> {
        records(case)
            .iter()
            .filter(|r| r["kind"] == "packet_out" && r["node"] == "flight")
            .map(Value::to_string)
            .collect()
    };
    assert_eq!(flight_out("a1-downlink-faults"), flight_out("a1-nominal"));
    assert_eq!(reports("a1-downlink-faults"), reports("a1-nominal"));
    // Gaps only where a fault was injected: these events, and no others, in this order.
    let expected = [
        event(2_010_000, "seq_gap", &[16, 1]),
        event(2_510_000, "seq_repeat", &[32, 1]),
        event(4_010_000, "seq_gap", &[16, 1]),
        event(4_510_000, "seq_reset", &[16, 3]),
        event(5_010_000, "seq_gap", &[16, 1]),
        event(6_510_000, "packet_rejected", &[2]),
        event(8_510_000, "seq_gap", &[32, 1]),
        event(12_385_000, "seq_gap", &[33, 1]),
        event(12_385_000, "dump_discarded", &[33, 256]),
    ];
    assert_eq!(ground_events("a1-downlink-faults"), expected);
    // 58 packets sent, two dropped, one duplicated: 57 units arrive; the truncated one is
    // rejected without a packet_in.
    let delivered = faults
        .iter()
        .filter(|r| r["kind"] == "unit_delivered" && r["dir"] == "down")
        .count();
    assert_eq!(delivered, 57);
    let packet_in = faults
        .iter()
        .filter(|r| r["kind"] == "packet_in" && r["node"] == "ground")
        .count();
    assert_eq!(packet_in, 56);
    // No dump_reassembled: the lost packet leaves the SDU incomplete.
    assert!(
        ground_events("a1-downlink-faults")
            .iter()
            .all(|e| e.1 != "dump_reassembled")
    );
}

#[test]
fn nominal_packet_projection_is_the_baseline_of_every_link_level() {
    // req: APP-003
    let text = std::fs::read_to_string(golden_path("a1-nominal")).unwrap();
    let projection: Projection = packet_projection(&text).unwrap();
    // The projection of a fresh run equals that of the golden trace.
    assert_eq!(
        packet_projection(&run_case("a1-nominal").unwrap()).unwrap(),
        projection
    );
    let sizes: Vec<(PacketRecord, NodeId, u16, usize)> = projection
        .iter()
        .map(|((r, n, a), v)| (*r, *n, *a, v.len()))
        .collect();
    let (o, i) = (PacketRecord::Out, PacketRecord::In);
    assert_eq!(
        sizes,
        [
            (o, FLIGHT, 16, 30),
            (o, FLIGHT, 17, 10),
            (o, FLIGHT, 32, 13),
            (o, FLIGHT, 33, 5),
            (o, GROUND, 128, 6),
            (o, GROUND, 129, 1),
            (i, GROUND, 16, 30),
            (i, GROUND, 17, 10),
            (i, GROUND, 32, 13),
            (i, GROUND, 33, 5),
        ]
    );
    // The flight node raises `cmd_received` and `table_received` instead of `packet_in`.
    // A non-canonical text has no projection.
    assert!(packet_projection("{}\n").is_err());
}
