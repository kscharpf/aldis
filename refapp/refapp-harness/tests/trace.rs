//! Trace writer, canonical-form check, and comparator against a hand-written sample trace
//! (docs/reference-application.md §7; A0 WP-2).

use std::fmt::Write as _;

use refapp_harness::micros::Micros;
use refapp_harness::scenario::Scenario;
use refapp_harness::trace::{
    Dir, FaultKind, Header, Mismatch, NodeId, Record, check_canonical, compare, decode_strict,
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
            t_us: Micros::new(0),
            node: NodeId::Flight,
            apid: 16,
            octets: tm.clone(),
        },
        Record::UnitSent {
            t_us: Micros::new(0),
            dir: Dir::Down,
            index: 0,
            octets: tm.clone(),
        },
        Record::UnitDelivered {
            t_us: Micros::new(10_000),
            dir: Dir::Down,
            index: 0,
            copy: 0,
        },
        Record::PacketIn {
            t_us: Micros::new(10_000),
            node: NodeId::Ground,
            apid: 16,
            octets: tm,
        },
        Record::AppEvent {
            t_us: Micros::new(10_000),
            node: NodeId::Ground,
            code: 1,
            args: vec![16, 2],
        },
        Record::PacketOut {
            t_us: Micros::new(1_000_000),
            node: NodeId::Ground,
            apid: 128,
            octets: tc.clone(),
        },
        Record::Fault {
            t_us: Micros::new(1_000_000),
            dir: Dir::Up,
            index: 0,
            fault: FaultKind::Duplicate,
            args: vec![],
        },
        Record::UnitSent {
            t_us: Micros::new(1_000_000),
            dir: Dir::Up,
            index: 0,
            octets: tc.clone(),
        },
        Record::UnitDelivered {
            t_us: Micros::new(1_010_000),
            dir: Dir::Up,
            index: 0,
            copy: 0,
        },
        Record::UnitDelivered {
            t_us: Micros::new(1_010_000),
            dir: Dir::Up,
            index: 0,
            copy: 1,
        },
        Record::PacketIn {
            t_us: Micros::new(1_010_000),
            node: NodeId::Flight,
            apid: 128,
            octets: tc,
        },
        Record::AppEvent {
            t_us: Micros::new(1_010_000),
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
        t_us: Micros::new(0),
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

#[test]
fn golden_traces_decode_with_the_strict_codecs() {
    // req: APP-086
    let dir = refapp_harness::golden_path("a1-nominal");
    let dir = dir.parent().unwrap();
    let scenario = Scenario::load().unwrap();
    let mut traces = 0;
    let mut frame_traces = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|x| x == "jsonl") {
            let text = std::fs::read_to_string(&path).unwrap();
            let d = decode_strict(&scenario, &text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            traces += 1;
            if d.frames > 0 {
                // A frame-level trace: its frames and the packets inside were decoded.
                assert!(d.framed_packets > 0, "{}", path.display());
                frame_traces += 1;
            }
        }
    }
    assert!(traces >= 5);
    assert_eq!(
        frame_traces, 9,
        "the four a2-* and the five a3-* traces are decoded as frames"
    );
}

#[test]
fn a_planted_malformed_packet_fails_the_strict_decode() {
    // req: APP-086
    let line = |kind: &str, hex: &str| {
        format!(
            "{{\"t_us\":0,\"kind\":\"{kind}\",\"node\":\"flight\",\"apid\":16,\"hex\":\"{hex}\"}}\n"
        )
    };
    let scenario = Scenario::load().unwrap();
    let header = SAMPLE
        .lines()
        .next()
        .unwrap()
        .replace("\"sample\"", "\"a1-nominal\"");
    let good = "0810C00000057D2B75000000";
    let ok = format!("{header}\n{}", line("packet_out", good));
    assert_eq!(decode_strict(&scenario, &ok).unwrap().records, 1);
    // Version 1 (SPP-001), truncated, and trailing octets after the packet.
    for bad in [
        "2810C00000057D2B75000000",
        "0810C00000057D2B750000",
        "0810C00000057D2B75000000FF",
    ] {
        let text = format!("{header}\n{}", line("packet_out", bad));
        let e = decode_strict(&scenario, &text).unwrap_err();
        assert!(e.starts_with("line 2:"), "{e}");
    }
    let unit = format!(
        "{header}\n{{\"t_us\":0,\"kind\":\"unit_sent\",\"dir\":\"down\",\"index\":0,\"hex\":\"2810C00000057D2B75000000\"}}\n"
    );
    assert!(decode_strict(&scenario, &unit).is_err());
}

/// The golden trace of `case_id` with one `unit_sent` of direction `dir` changed by `f`.
fn with_unit_changed(case_id: &str, dir: &str, f: impl Fn(&mut Vec<u8>) -> bool) -> String {
    let path = refapp_harness::golden_path(case_id);
    let text = std::fs::read_to_string(path).unwrap();
    let mark = format!("\"kind\":\"unit_sent\",\"dir\":\"{dir}\",\"index\":");
    let mut out = String::new();
    let mut done = false;
    for l in text.split_inclusive('\n') {
        let picked = if !done && l.contains(&mark) {
            let (head, rest) = l.split_once("\"hex\":\"").unwrap();
            let hex = rest.trim_end().trim_end_matches("\"}");
            let mut octets = unhex(hex);
            f(&mut octets).then_some((head, octets))
        } else {
            None
        };
        if let Some((head, octets)) = picked {
            let hex = octets.iter().fold(String::new(), |mut acc, b| {
                write!(acc, "{b:02X}").unwrap();
                acc
            });
            writeln!(out, "{head}\"hex\":\"{hex}\"}}").unwrap();
            done = true;
        } else {
            out.push_str(l);
        }
    }
    assert!(done, "no {dir} unit_sent in {case_id}");
    out
}

#[test]
fn a_planted_wrong_fecf_fails_the_strict_decode_of_a_frame_trace() {
    // req: APP-086
    let scenario = Scenario::load().unwrap();
    let good = std::fs::read_to_string(refapp_harness::golden_path("a2-nominal")).unwrap();
    assert!(decode_strict(&scenario, &good).is_ok());
    for dir in ["down", "up"] {
        let bad = with_unit_changed("a2-nominal", dir, |o| {
            if let Some(last) = o.last_mut() {
                *last ^= 1;
            }
            true
        });
        let e = decode_strict(&scenario, &bad).unwrap_err();
        assert!(e.contains("not a strict"), "{dir}: {e}");
    }
}

#[test]
fn the_level_of_the_case_chooses_the_decoder_not_its_name() {
    // req: APP-086
    let scenario = Scenario::load().unwrap();
    // A frame under a packet-level case fails as a packet; an unknown case fails.
    let text = std::fs::read_to_string(refapp_harness::golden_path("a2-nominal")).unwrap();
    let as_packets = text.replacen("\"a2-nominal\"", "\"a1-nominal\"", 1);
    let e = decode_strict(&scenario, &as_packets).unwrap_err();
    assert!(e.contains("is not a strict Space Packet"), "{e}");
    let unknown = text.replacen("\"a2-nominal\"", "\"a2-unknown\"", 1);
    let e = decode_strict(&scenario, &unknown).unwrap_err();
    assert_eq!(e, "no case `a2-unknown` in scenario.json");
}

/// Appends the FECF of 132.0 Annex B and 232.0 Annex A (polynomial 0x1021, initial 0xFFFF).
fn with_fecf(mut frame: Vec<u8>) -> Vec<u8> {
    let mut crc = 0xFFFFu16;
    for b in &frame {
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
    frame
}

#[test]
fn a_corrupt_packet_inside_a_frame_with_a_valid_fecf_fails_the_strict_decode() {
    // req: APP-086
    let scenario = Scenario::load().unwrap();
    // The first octet of the first packet that starts in a TM frame (the First Header Pointer
    // is the offset into the data field after the 6 octets of the header), and of the first FDU
    // of the uplink (after the 5 octets of the header and the Segment Header). The Packet
    // Version Number is set to 1, which the codec refuses (SPP-001); the FECF is recomputed.
    let plant = |frame: &mut Vec<u8>, at: usize| {
        frame.truncate(frame.len() - 2);
        frame[at] = (frame[at] & 0x1F) | 0x20;
        *frame = with_fecf(std::mem::take(frame));
    };
    let down = with_unit_changed("a2-nominal", "down", |o| {
        let fhp = usize::from(o[4] & 0x07) << 8 | usize::from(o[5]);
        if fhp >= 0x7FE {
            return false;
        }
        plant(o, 6 + fhp);
        true
    });
    let e = decode_strict(&scenario, &down).unwrap_err();
    assert!(
        e.contains("extraction error") || e.contains("not a strict Space Packet"),
        "{e}"
    );
    assert!(
        !e.contains("not a strict TM frame"),
        "the frame itself is valid: {e}"
    );
    let up = with_unit_changed("a2-nominal", "up", |o| {
        plant(o, 6);
        true
    });
    let e = decode_strict(&scenario, &up).unwrap_err();
    assert!(
        !e.contains("not a strict TC frame"),
        "the frame itself is valid: {e}"
    );
}

#[test]
fn a_planted_invalid_clcw_fails_the_strict_decode_of_a_downlink_frame() {
    // req: APP-086, APP-134
    // The CLCW is the last four octets before the FECF; its Reserved Spare of bits 14 to 15
    // is set (TC-030, strict mode), the FECF recomputed, so only the CLCW is wrong.
    let scenario = Scenario::load().unwrap();
    for case in ["a3-nominal", "a3-uplink-loss"] {
        assert!(
            decode_strict(
                &scenario,
                &std::fs::read_to_string(refapp_harness::golden_path(case)).unwrap()
            )
            .is_ok()
        );
        let bad = with_unit_changed(case, "down", |o| {
            o.truncate(o.len() - 2);
            let at = o.len() - 3;
            o[at] |= 0x01;
            *o = with_fecf(std::mem::take(o));
            true
        });
        let e = decode_strict(&scenario, &bad).unwrap_err();
        assert!(e.contains("not a strict CLCW"), "{case}: {e}");
    }
}

#[test]
fn the_flight_nodes_records_must_match_the_packets_reassembled_from_the_uplink() {
    // req: APP-086, APP-129, APP-146
    let scenario = Scenario::load().unwrap();
    let text = std::fs::read_to_string(refapp_harness::golden_path("a3-uplink-loss")).unwrap();
    // A command the node never received.
    let dropped: String = text
        .split_inclusive('\n')
        .filter(|l| !(l.contains("\"cmd_received\"") && l.contains("\"args\":[3,1]")))
        .collect();
    assert_ne!(dropped, text);
    let e = decode_strict(&scenario, &dropped).unwrap_err();
    assert!(e.contains("cmd_received"), "{e}");
    // A command with another sequence count.
    let wrong = text.replacen(
        "\"name\":\"cmd_received\",\"args\":[3,1]",
        "\"name\":\"cmd_received\",\"args\":[3,9]",
        1,
    );
    assert_ne!(wrong, text);
    let e = decode_strict(&scenario, &wrong).unwrap_err();
    assert!(e.contains("cmd_received"), "{e}");
}

#[test]
fn an_uplink_fault_without_an_index_fails_the_strict_decode() {
    // req: APP-086
    // The unit a fault hit must be known: a record without it is refused, not read as no fault.
    let scenario = Scenario::load().unwrap();
    let text = std::fs::read_to_string(refapp_harness::golden_path("a3-uplink-loss")).unwrap();
    assert!(decode_strict(&scenario, &text).is_ok());
    let planted: String = text
        .split_inclusive('\n')
        .map(|l| {
            if l.contains("\"kind\":\"fault\",\"dir\":\"up\"") {
                let (a, b) = l.split_once("\"index\":").unwrap();
                format!("{a}{}", b.split_once(',').unwrap().1)
            } else {
                l.to_owned()
            }
        })
        .collect();
    assert_ne!(planted, text);
    let e = decode_strict(&scenario, &planted).unwrap_err();
    // The canonical check refuses the record first; the decoder's own guard for a missing index
    // (a second guard in `trace.rs`) is not reached.
    assert!(e.contains("keys of `fault` must be exactly"), "{e}");
}
