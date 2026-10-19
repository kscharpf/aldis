//! Trace writer, canonical-form check, and comparator against a hand-written sample trace
//! (docs/reference-application.md §7; A0 WP-2).

use refapp_harness::trace::{
    Dir, FaultKind, Header, Mismatch, NodeId, Record, check_canonical, compare,
};

const SAMPLE: &str = include_str!("data/sample.jsonl");

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn sample_is_canonical() {
    // req: APP-080, APP-081, APP-082, APP-083
    check_canonical(SAMPLE).unwrap();
}

#[test]
fn writer_reproduces_the_sample() {
    // req: APP-080, APP-081, APP-082, APP-083
    let tm = unhex("0810C00000057D2B75000000");
    let tc = unhex("1880C000000701017D2B75060000");
    let header = Header {
        scenario: "aldis-refapp".to_owned(),
        scenario_version: 1,
        case: "sample".to_owned(),
        trace_version: 1,
        requirements: vec!["APP-080".to_owned(), "APP-082".to_owned()],
    };
    let records = [
        Record::PacketOut {
            t_us: 0,
            node: NodeId::Flight,
            apid: 16,
            octets: tm.clone(),
        },
        Record::UnitSent {
            t_us: 0,
            dir: Dir::Down,
            index: 0,
            octets: tm.clone(),
        },
        Record::UnitDelivered {
            t_us: 10_000,
            dir: Dir::Down,
            index: 0,
            copy: 0,
        },
        Record::PacketIn {
            t_us: 10_000,
            node: NodeId::Ground,
            apid: 16,
            octets: tm,
        },
        Record::AppEvent {
            t_us: 10_000,
            node: NodeId::Ground,
            code: 1,
            args: vec![16, 2],
        },
        Record::PacketOut {
            t_us: 1_000_000,
            node: NodeId::Ground,
            apid: 128,
            octets: tc.clone(),
        },
        Record::Fault {
            t_us: 1_000_000,
            dir: Dir::Up,
            index: 0,
            fault: FaultKind::Duplicate,
            args: vec![],
        },
        Record::UnitSent {
            t_us: 1_000_000,
            dir: Dir::Up,
            index: 0,
            octets: tc.clone(),
        },
        Record::UnitDelivered {
            t_us: 1_010_000,
            dir: Dir::Up,
            index: 0,
            copy: 0,
        },
        Record::UnitDelivered {
            t_us: 1_010_000,
            dir: Dir::Up,
            index: 0,
            copy: 1,
        },
        Record::PacketIn {
            t_us: 1_010_000,
            node: NodeId::Flight,
            apid: 128,
            octets: tc,
        },
        Record::AppEvent {
            t_us: 1_010_000,
            node: NodeId::Flight,
            code: 20,
            args: vec![1, 0],
        },
    ];
    let mut text = header.to_line();
    for r in &records {
        text.push_str(&r.to_line().unwrap());
    }
    assert_eq!(text, SAMPLE);
}

#[test]
fn writer_rejects_unknown_event_codes() {
    // req: APP-083
    let r = Record::AppEvent {
        t_us: 0,
        node: NodeId::Ground,
        code: 999,
        args: vec![],
    };
    assert!(r.to_line().is_err());
}

#[test]
fn non_canonical_traces_are_rejected() {
    // req: APP-080, APP-081, APP-082, APP-083
    let cases: &[(&str, &str, &str)] = &[
        ("space after colon", "\"t_us\":0,", "\"t_us\": 0,"),
        (
            "key order",
            "\"kind\":\"unit_delivered\",\"dir\":\"down\",\"index\":0,\"copy\":0",
            "\"kind\":\"unit_delivered\",\"index\":0,\"dir\":\"down\",\"copy\":0",
        ),
        (
            "lower-case hex",
            "\"hex\":\"0810C00000057D2B75000000\"}\n{\"t_us\":0,\"kind\":\"unit_sent\"",
            "\"hex\":\"0810c00000057d2b75000000\"}\n{\"t_us\":0,\"kind\":\"unit_sent\"",
        ),
        (
            "float instant",
            "{\"t_us\":10000,\"kind\":\"unit_delivered\"",
            "{\"t_us\":10000.0,\"kind\":\"unit_delivered\"",
        ),
        (
            "unknown kind",
            "\"kind\":\"packet_in\",\"node\":\"ground\"",
            "\"kind\":\"packet_seen\",\"node\":\"ground\"",
        ),
        (
            "event name",
            "\"name\":\"seq_gap\"",
            "\"name\":\"seq_repeat\"",
        ),
        ("fault arity", "\"args\":[]}", "\"args\":[1]}"),
        (
            "unknown node",
            "\"node\":\"flight\",\"apid\":16",
            "\"node\":\"relay\",\"apid\":16",
        ),
        ("copy above 1", "\"copy\":1", "\"copy\":2"),
        (
            "escaped string",
            "\"case\":\"sample\"",
            "\"case\":\"s\\u0061mple\"",
        ),
        (
            "time going back",
            "{\"t_us\":1010000,\"kind\":\"app_event\"",
            "{\"t_us\":5,\"kind\":\"app_event\"",
        ),
        (
            "extra key",
            "\"trace_version\":1,",
            "\"trace_version\":1,\"x\":1,",
        ),
        (
            "odd hex",
            "\"hex\":\"1880C000000701017D2B75060000\"}\n{\"t_us\":1000000,\"kind\":\"fault\"",
            "\"hex\":\"1880C000000701017D2B7506000\"}\n{\"t_us\":1000000,\"kind\":\"fault\"",
        ),
    ];
    for (what, from, to) in cases {
        assert!(SAMPLE.contains(from), "{what}: pattern not in sample");
        let bad = SAMPLE.replacen(from, to, 1);
        assert!(check_canonical(&bad).is_err(), "{what} was accepted");
    }
    assert!(
        check_canonical(SAMPLE.trim_end()).is_err(),
        "missing final LF"
    );
    assert!(check_canonical("").is_err(), "empty trace");
    let no_header: String = SAMPLE.split_inclusive('\n').skip(1).collect();
    assert!(check_canonical(&no_header).is_err(), "missing header");
    let crlf = SAMPLE.replace('\n', "\r\n");
    assert!(check_canonical(&crlf).is_err(), "CRLF line ends");
}

#[test]
fn comparator_reports_the_first_difference() {
    // req: APP-085
    assert_eq!(compare(SAMPLE, SAMPLE), Ok(()));
    let changed = SAMPLE.replacen("\"copy\":1", "\"copy\":0", 1);
    let m = compare(SAMPLE, &changed).unwrap_err();
    assert_eq!(m.line, 11);
    assert!(m.expected.unwrap().contains("\"copy\":1"));
    let shorter: String = SAMPLE.split_inclusive('\n').take(3).collect();
    let m: Mismatch = compare(SAMPLE, &shorter).unwrap_err();
    assert_eq!(m.line, 4);
    assert_eq!(m.actual, None);
    assert!(m.to_string().contains("<end of trace>"));
}
