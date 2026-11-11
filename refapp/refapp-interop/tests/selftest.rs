//! The self-test of the transport driver (plan D8 a, APP-106): both Rust nodes, each stepped by
//! the driver through a UDP socket on loopback in virtual time.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use refapp_harness::golden_path;
use refapp_harness::micros::Micros;
use refapp_harness::scenario::Scenario;
use refapp_harness::trace::{
    Dir, NodeId, PacketRecord, Projection, Record, hex, packet_projection,
};
use refapp_interop::clock::Monotonic;
use refapp_interop::log::{Datagram, DatagramKind, Event, Header, RunLog, Sink};
use refapp_interop::loopback::run_pair;
use refapp_interop::loss::LossScript;
use refapp_interop::port::UdpPort;
use refapp_interop::run::{flight_node, run_flight};

/// The packet projection of a run's events, in the form of a golden trace's (APP-003).
fn projection(events: &[Event]) -> Projection {
    let mut p = Projection::new();
    for e in events {
        let (kind, node, apid, octets) = match e {
            Event::Node(Record::PacketOut {
                node, apid, octets, ..
            }) => (PacketRecord::Out, *node, *apid, octets),
            Event::Node(Record::PacketIn {
                node, apid, octets, ..
            }) => (PacketRecord::In, *node, *apid, octets),
            _ => continue,
        };
        p.entry((kind, node, apid)).or_default().push(hex(octets));
    }
    p
}

fn golden_projection(case: &str) -> Projection {
    packet_projection(&std::fs::read_to_string(golden_path(case)).unwrap()).unwrap()
}

fn datagrams(events: &[Event], dir: Dir, kind: DatagramKind) -> Vec<&Datagram> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Datagram(d) if d.dir == dir && d.kind == kind => Some(d),
            _ => None,
        })
        .collect()
}

#[test]
fn nominal_case_over_loopback_gives_the_packet_projection_of_a3_nominal() {
    // req: APP-003, APP-157, APP-158, APP-159, APP-160
    let mut events: Vec<Event> = Vec::new();
    run_pair("a3-nominal", &LossScript::default(), &mut events).unwrap();
    let got = projection(&events);
    assert!(!got.is_empty());
    assert_eq!(got, golden_projection("a3-nominal"), "APP-003");

    // One frame per datagram, one datagram per 125 000 µs slot, and nothing dropped.
    let sent = datagrams(&events, Dir::Down, DatagramKind::Sent);
    let case_end = Scenario::load().unwrap().case("a3-nominal").unwrap().end_us;
    assert_eq!(sent.len() as u64, case_end.get() / 125_000);
    for (i, d) in sent.iter().enumerate() {
        assert_eq!(d.index, i as u64, "indexes count in order");
        assert_eq!(
            d.t_us,
            Micros::new(125_000 * i as u64),
            "tx_slot at every slot"
        );
        assert_eq!(d.octets.len(), 223, "one 223-octet TM frame (§4.1)");
    }
    assert_eq!(
        datagrams(&events, Dir::Down, DatagramKind::Dropped).len(),
        0
    );
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, Event::Node(Record::Fault { .. })))
    );
}

#[test]
fn lossy_case_over_loopback_applies_the_script_and_the_commands_still_arrive() {
    // req: APP-163, APP-162, APP-003
    let scenario = Scenario::load().unwrap();
    let loss = LossScript::from_case(scenario.case("a3-uplink-loss").unwrap()).unwrap();
    let mut events: Vec<Event> = Vec::new();
    run_pair("a3-nominal", &loss, &mut events).unwrap();

    // Every scripted drop is recorded as a fault, before its datagram, at the right index.
    let faults: Vec<(Dir, u64)> = events
        .iter()
        .filter_map(|e| match e {
            Event::Node(Record::Fault { dir, index, .. }) => Some((*dir, *index)),
            _ => None,
        })
        .collect();
    let mut want = loss.entries().to_vec();
    let mut got = faults;
    want.sort();
    got.sort();
    assert_eq!(got, want);
    assert_eq!(
        datagrams(&events, Dir::Down, DatagramKind::Dropped).len(),
        7
    );
    assert_eq!(datagrams(&events, Dir::Up, DatagramKind::Dropped).len(), 2);

    // The commands and table segments reach the flight application exactly once, in order,
    // as in the nominal case: FOP-1 retransmits what the script lost (APP-165).
    let nominal = golden_projection("a3-nominal");
    let got = projection(&events);
    let uplink_side = |p: &Projection| -> BTreeMap<(PacketRecord, NodeId, u16), Vec<String>> {
        p.iter()
            .filter(|((kind, node, _), _)| {
                matches!(
                    (kind, node),
                    (PacketRecord::In, NodeId::Flight) | (PacketRecord::Out, NodeId::Ground)
                )
            })
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    };
    let want_up = uplink_side(&nominal);
    assert!(!want_up.is_empty());
    assert_eq!(
        uplink_side(&got),
        want_up,
        "commands equal those of a3-nominal"
    );

    // The link has the scenario's delay, so the schedule is the harness's: the whole projection
    // is that of the golden trace of the lossy case.
    assert_eq!(got, golden_projection("a3-uplink-loss"));

    // The run log has a header, the datagrams, and the faults (APP-162).
    let header = Header {
        case: "a3-nominal".to_owned(),
        ground_version: None,
        image_digest: None,
        node_commit: None,
        loss_script: loss,
    };
    let mut log = RunLog::new(Vec::new(), &header).unwrap();
    for e in events {
        log.record(e).unwrap();
    }
    let text = String::from_utf8(log.finish().unwrap()).unwrap();
    let count = |kind: &str| {
        text.lines()
            .filter(|l| l.contains(&format!("\"kind\":\"{kind}\"")))
            .count()
    };
    assert_eq!(count("header"), 1);
    assert_eq!(count("fault"), 9);
    assert!(count("datagram") > 9);
    assert!(count("app_event") > 0);
}

#[test]
fn flight_node_over_a_real_clock_sends_one_frame_per_datagram() {
    // req: APP-157, APP-158, APP-159, APP-160
    let tm = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let tc = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let to_tm = tm.local_addr().unwrap();
    tm.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut port = UdpPort::new(tc, to_tm);
    let scenario = Scenario::load().unwrap();
    let case = scenario.case("a3-nominal").unwrap();
    let mut flight = flight_node(case.level).unwrap();
    let mut events: Vec<Event> = Vec::new();
    let mut clock = Monotonic::start();
    // How many slots fit in the run depends on the wall clock; the count is checked against the
    // frames the driver itself sent.
    run_flight(
        &mut flight,
        NonZeroU64::new(125_000).unwrap(),
        Micros::new(300_000),
        &LossScript::default(),
        &mut clock,
        &mut port,
        &mut events,
    )
    .unwrap();
    let mut lengths = Vec::new();
    let mut buf = vec![0_u8; 65_536];
    tm.set_nonblocking(true).unwrap();
    while let Ok(n) = tm.recv(&mut buf) {
        lengths.push(n);
    }
    let sent = datagrams(&events, Dir::Down, DatagramKind::Sent);
    assert!(!sent.is_empty(), "slot 0 is always served");
    assert_eq!(lengths.len(), sent.len(), "one datagram per frame sent");
    assert!(lengths.iter().all(|n| *n == 223), "{lengths:?}");
    for (i, d) in sent.iter().enumerate() {
        assert_eq!(d.index, i as u64, "indexes count in order");
        assert_eq!(d.octets.len(), 223);
    }
}
