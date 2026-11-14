//! The node protocol, harness side, over both transports (docs/reference-application.md §8.2,
//! §10.6; ACFS plan WP-2). The out-of-process node is `refapp-test-node`, which serves the Rust
//! flight node of the case named in HELLO.

use std::io::Cursor;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;

use refapp_harness::proto::{Message, NodeOutput, Role, read_message, write_message};
use refapp_harness::remote::{RemoteNode, run_remote};
use refapp_harness::scenario::Scenario;
use refapp_harness::{Transport, golden_path, launch, manifest, manifest_for, run_case};

const NODE: &str = env!("CARGO_BIN_EXE_refapp-test-node");

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stdio_run(case: &str) -> Result<String, String> {
    let log = scratch(&format!("stdio-{case}")).join("node.log");
    let mut command = Command::new(NODE);
    command.arg("--stdio");
    launch::run_stdio(case, command, &log)
}

fn tcp_run(case: &str) -> Result<String, String> {
    let log = scratch(&format!("tcp-{case}")).join("node.log");
    launch::run_tcp(
        case,
        0,
        |addr: SocketAddr| {
            let mut c = Command::new(NODE);
            c.arg("--connect").arg(addr.to_string());
            c
        },
        &log,
    )
}

fn golden(case: &str) -> String {
    std::fs::read_to_string(golden_path(case)).unwrap()
}

#[test]
fn a0_empty_over_stdio_and_tcp_is_the_golden_trace() {
    // req: APP-092, APP-118
    let expected = golden("a0-empty");
    assert_eq!(stdio_run("a0-empty").unwrap(), expected);
    assert_eq!(tcp_run("a0-empty").unwrap(), expected);
}

#[test]
fn out_of_process_trace_equals_in_process_trace() {
    // req: APP-092, APP-118
    // One case of each link level, so that the units, packets, events and deadlines of all
    // three kinds of node travel through the protocol.
    for case in ["a1-nominal", "a2-uplink-faults", "a3-uplink-loss"] {
        let in_process = run_case(case).unwrap();
        assert_eq!(in_process, golden(case), "{case}: in-process");
        assert_eq!(stdio_run(case).unwrap(), in_process, "{case}: stdio");
        assert_eq!(tcp_run(case).unwrap(), in_process, "{case}: tcp");
    }
}

fn frame(m: &Message) -> Vec<u8> {
    let mut v = Vec::new();
    write_message(&mut v, m).unwrap();
    v
}

/// Runs a case against a node that answers with the canned `reply` bytes.
fn canned(case: &str, reply: Vec<u8>) -> Result<String, String> {
    let version = Scenario::load().unwrap().scenario_version;
    let mut node =
        RemoteNode::connect(Cursor::new(reply), Vec::new(), Role::Flight, version, case)?;
    run_remote(case, &mut node)
}

fn ack() -> Vec<u8> {
    frame(&Message::HelloAck { version: 1 })
}

fn result(outputs: Vec<NodeOutput>) -> Vec<u8> {
    frame(&Message::StepResult {
        outputs,
        next_deadline: None,
    })
}

#[test]
fn a_well_behaved_canned_node_gives_the_trace() {
    // req: APP-092
    // `a0-empty` ends at 0 µs, before any step: HELLO, then END.
    let reply = [ack(), frame(&Message::EndAck)].concat();
    assert_eq!(canned("a0-empty", reply).unwrap(), golden("a0-empty"));
}

#[test]
fn a_protocol_error_ends_the_run_with_an_error_and_no_trace() {
    // req: APP-092
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("closed before HELLO_ACK", vec![], "stream closed"),
        ("closed after HELLO_ACK", ack(), "stream closed"),
        ("closed before END_ACK", ack(), "stream closed"),
        (
            "cut in the middle of a message",
            [ack(), vec![0, 0, 0, 20, 0x82, 0]].concat(),
            "stream closed",
        ),
        (
            "wrong protocol version",
            frame(&Message::HelloAck { version: 2 }),
            "protocol version 2",
        ),
        (
            "unexpected type for HELLO_ACK",
            frame(&Message::EndAck),
            "unexpected END_ACK",
        ),
        (
            "unexpected type for STEP_RESULT",
            [ack(), frame(&Message::EndAck)].concat(),
            "unexpected END_ACK",
        ),
        (
            "unexpected type for END_ACK",
            [ack(), result(vec![])].concat(),
            "unexpected STEP_RESULT",
        ),
        (
            "unknown type",
            [ack(), vec![0, 0, 0, 1, 0x7f]].concat(),
            "unknown message type 0x7f",
        ),
        (
            "zero length",
            [ack(), vec![0, 0, 0, 0]].concat(),
            "message length 0",
        ),
        (
            "length over the limit",
            [ack(), vec![0xff, 0xff, 0xff, 0xff]].concat(),
            "message length",
        ),
        (
            "malformed body",
            [ack(), vec![0, 0, 0, 3, 0x82, 0, 1]].concat(),
            "too short",
        ),
        (
            "trailing octets",
            [
                ack(),
                vec![
                    0, 0, 0, 12, 0x82, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 9,
                ],
            ]
            .concat(),
            "trailing",
        ),
        (
            "unknown output kind",
            [ack(), vec![0, 0, 0, 4, 0x82, 0, 1, 9]].concat(),
            "unknown output kind 9",
        ),
        (
            "APID out of range",
            [ack(), result(vec![NodeOutput::PacketOut(0x0800, vec![1])])].concat(),
            "APID 2048 out of range",
        ),
    ];
    // `a1-nominal` steps the flight node at 0 µs, so the replies after HELLO_ACK answer a STEP;
    // the two that act on END (`closed before END_ACK`, `unexpected type for END_ACK`) run
    // `a0-empty`, which has no step.
    for (what, reply, wanted) in cases {
        let case = if what.ends_with("END_ACK") && !what.contains("HELLO") {
            "a0-empty"
        } else {
            "a1-nominal"
        };
        let err = canned(case, reply).expect_err(what);
        assert!(err.contains(wanted), "{what}: `{err}` lacks `{wanted}`");
    }
}

#[test]
fn messages_round_trip_and_decode_strictly() {
    // req: APP-092
    let all = [
        Message::Hello {
            version: 1,
            scenario_version: 7,
            role: Role::Ground,
            case_id: "a1-nominal".to_owned(),
        },
        Message::Step {
            now: 125_000,
            tx_slot: true,
            units: vec![vec![1, 2, 3], vec![]],
            ops: vec![0, 65_535],
        },
        Message::End,
        Message::HelloAck { version: 1 },
        Message::StepResult {
            outputs: vec![
                NodeOutput::Unit(vec![9]),
                NodeOutput::PacketOut(100, vec![1, 2]),
                NodeOutput::PacketIn(101, vec![]),
                NodeOutput::Event(3, vec![1, u64::MAX - 1]),
            ],
            next_deadline: Some(250_000),
        },
        Message::StepResult {
            outputs: vec![],
            next_deadline: None,
        },
        Message::EndAck,
    ];
    for m in all {
        let wire = frame(&m);
        assert_eq!(read_message(&mut Cursor::new(&wire)).unwrap(), m);
        let body = m.encode().unwrap();
        // Every proper prefix of a body is refused; so is a body with an octet appended.
        for n in 1..body.len() {
            assert!(Message::decode(&body[..n]).is_err(), "{m:?} cut at {n}");
        }
        let mut longer = body;
        longer.push(0);
        assert!(
            Message::decode(&longer).is_err(),
            "{m:?} with a trailing octet"
        );
    }
    assert!(Message::decode(&[]).is_err());
    // tx_slot 2, role 2, a non-ASCII case ID.
    let step = Message::Step {
        now: 0,
        tx_slot: false,
        units: vec![],
        ops: vec![],
    };
    let mut body = step.encode().unwrap();
    body[9] = 2;
    assert!(Message::decode(&body).unwrap_err().contains("tx_slot"));
    assert!(
        Message::decode(&[1, 0, 1, 0, 1, 2, 0])
            .unwrap_err()
            .contains("role")
    );
    assert!(
        Message::decode(&[1, 0, 1, 0, 1, 0, 1, 0xc3])
            .unwrap_err()
            .contains("ASCII")
    );
    // Fields that do not fit are refused when encoding, not truncated.
    let big = Message::Step {
        now: 0,
        tx_slot: false,
        units: vec![vec![0; 65_536]],
        ops: vec![],
    };
    assert!(big.encode().is_err());
    let long_id = Message::Hello {
        version: 1,
        scenario_version: 1,
        role: Role::Flight,
        case_id: "x".repeat(256),
    };
    assert!(long_id.encode().is_err());
    let non_ascii = Message::Hello {
        version: 1,
        scenario_version: 1,
        role: Role::Flight,
        case_id: "é".to_owned(),
    };
    assert!(non_ascii.encode().is_err());
}

#[test]
fn a_node_that_ends_or_will_not_connect_leaves_no_trace() {
    // req: APP-092, APP-118
    let log = scratch("early-exit").join("node.log");
    let err = launch::run_tcp("a0-empty", 0, |_| Command::new("false"), &log).unwrap_err();
    assert!(err.contains("before it connected"), "{err}");
    let err = launch::run_stdio("a0-empty", Command::new("true"), &log).unwrap_err();
    // The child may end before or after the harness writes HELLO.
    assert!(
        err.contains("stream closed") || err.contains("Broken pipe"),
        "{err}"
    );
    let err = launch::run_stdio("a0-empty", Command::new("/nonexistent/node"), &log).unwrap_err();
    assert!(err.contains("starting the node"), "{err}");
    let err = launch::run_stdio("no-such-case", Command::new("true"), &log).unwrap_err();
    assert!(err.contains("no-such-case"), "{err}");
}

#[cfg(unix)]
#[test]
fn the_cfs_launcher_runs_core_cpu1_in_its_staging_directory() {
    // req: APP-118, APP-092
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch("cfs");
    let staging = dir.join("cpu1");
    std::fs::create_dir_all(&staging).unwrap();
    // The launcher refuses a directory without `core-cpu1`.
    let err = launch::run_cfs("a0-empty", &staging, 0, &dir.join("x.log")).unwrap_err();
    assert!(err.contains("core-cpu1"), "{err}");

    // A stand-in for cFS: it writes console output, notes its working directory, and connects
    // as the bridge does once started.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let exe = staging.join(launch::CFS_EXECUTABLE);
    std::fs::write(
        &exe,
        format!(
            "#!/bin/sh\necho 'console line'\npwd > cwd.txt\nexec {NODE} --connect 127.0.0.1:{port}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let log = dir.join("cfs.log");
    let trace = launch::run_cfs("a0-empty", &staging, port, &log).unwrap();
    assert_eq!(trace, golden("a0-empty"));
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("console line")
    );
    assert!(
        !trace.contains("console line"),
        "console output stays out of the trace"
    );
    let cwd = std::fs::read_to_string(staging.join("cwd.txt")).unwrap();
    assert_eq!(
        PathBuf::from(cwd.trim()).canonicalize().unwrap(),
        staging.canonicalize().unwrap()
    );
}

#[test]
fn the_manifest_names_transport_and_components() {
    // req: APP-084
    let m = manifest("a0-empty");
    assert!(
        m.contains(r#""nodes":"in-process""#) && m.contains(r#""transport":"none""#),
        "{m}"
    );
    assert!(!m.contains("components"));
    let comps = vec![
        ("cfe".to_owned(), "v7.0.1".to_owned()),
        ("aldis-ffi".to_owned(), "0.6.0".to_owned()),
    ];
    let m = manifest_for("a0-empty", "cfs", Transport::Tcp, &comps);
    let v: serde_json::Value = serde_json::from_str(&m).unwrap();
    assert_eq!(v["implementation"], "cfs");
    assert_eq!(v["nodes"], "out-of-process");
    assert_eq!(v["transport"], "tcp");
    assert_eq!(v["components"]["cfe"], "v7.0.1");
    assert_eq!(v["components"]["aldis-ffi"], "0.6.0");
    let m = manifest_for("a0-empty", "external", Transport::Stdio, &[]);
    assert!(m.contains(r#""transport":"stdio""#));
}

#[test]
fn the_harness_side_never_reads_the_clock() {
    // req: APP-092
    for src in [
        include_str!("../src/proto.rs"),
        include_str!("../src/remote.rs"),
        include_str!("../src/launch.rs"),
    ] {
        assert!(!src.contains("SystemTime") && !src.contains("Instant::now"));
    }
}
