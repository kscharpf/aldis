//! The ground node on scripted inputs (docs/reference-application.md §2.2, §6.5, §6.6, §7.4;
//! A1 WP-4).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic on small literals"
)]

use crate::{Call, Execution, GroundNode, OpsEntry, telemetry};
use aldis::core::ids::{Apid, SeqCount};
use aldis::spp::{PacketFields, PacketType, SequenceFlags, SppConfig, encode};
use refapp_scenario::{
    Delivered, Duration, Instant, Node, Output, StepInput, TableId, TableLength,
};

/// One output of a step, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rec {
    Unit(Vec<u8>),
    PacketOut(u16, Vec<u8>),
    PacketIn(u16, Vec<u8>),
    Event(u16, Vec<u64>),
}

#[derive(Default)]
struct Recorder(Vec<Rec>);

impl Output for Recorder {
    fn unit(&mut self, octets: &[u8]) {
        self.0.push(Rec::Unit(octets.to_vec()));
    }
    fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
        self.0.push(Rec::PacketOut(apid.get(), octets.to_vec()));
    }
    fn packet_in(&mut self, apid: Apid, octets: &[u8]) {
        self.0.push(Rec::PacketIn(apid.get(), octets.to_vec()));
    }
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.0.push(Rec::Event(code, args.to_vec()));
    }
}

const HK: u16 = 16;
const EVENTS: u16 = 17;
const PAYLOAD: u16 = 32;
const DUMP: u16 = 33;

/// Catalogue codes of docs/03-api-model.md §9.
const INCOMPLETE: u64 = 2;
const LENGTH_MISMATCH: u64 = 3;
const UNSUPPORTED_VERSION: u64 = 4;
const INVALID_FIELD: u64 = 6;
const UNKNOWN_CHANNEL: u64 = 10;

/// The CUC T-field of T0 + `s` seconds with the 16-bit `fraction` (§5).
fn t_field(s: u32, fraction: u16) -> Vec<u8> {
    let mut t = (2_100_000_000 + s).to_be_bytes().to_vec();
    t.extend_from_slice(&fraction.to_be_bytes());
    t
}

/// A telemetry packet with a CUC secondary header, implicit or explicit P-field (§3.2).
fn tm(apid: u16, flags: SequenceFlags, count: u16, explicit: bool, user: &[u8]) -> Vec<u8> {
    let mut secondary = if explicit { vec![0x1E] } else { vec![] };
    secondary.extend(t_field(u32::from(count), 0));
    packet(PacketType::Telemetry, apid, flags, count, &secondary, user)
}

fn packet(
    packet_type: PacketType,
    apid: u16,
    flags: SequenceFlags,
    count: u16,
    secondary: &[u8],
    user: &[u8],
) -> Vec<u8> {
    let fields = PacketFields {
        packet_type,
        apid: Apid::new(apid).unwrap(),
        sequence_flags: flags,
        sequence_count: SeqCount::new(count).unwrap(),
    };
    let data = SppConfig::DEFAULT.packet(fields, secondary, user).unwrap();
    let mut out = vec![0; data.packet_len()];
    let n = encode(&data, &mut out).unwrap();
    out.truncate(n);
    out
}

/// A housekeeping packet (any user data will do for the ground node).
fn hk(count: u16) -> Vec<u8> {
    tm(HK, SequenceFlags::Unsegmented, count, false, &[0; 4])
}

fn payload(count: u16) -> Vec<u8> {
    tm(PAYLOAD, SequenceFlags::Unsegmented, count, false, &[0; 4])
}

/// Delivers `units`, one step per instant `10 000 · (i + 1)` µs, and returns all outputs.
fn deliver(node: &mut GroundNode, units: &[Vec<u8>]) -> Vec<Rec> {
    let mut out = Recorder::default();
    for (i, unit) in units.iter().enumerate() {
        let now = Instant::from_micros(10_000 * (u64::try_from(i).unwrap() + 1));
        let delivered: [&[u8]; 1] = [unit];
        let input = StepInput {
            now,
            delivered: Delivered::new(&delivered),
            ops: &[],
            tx_slot: true,
        };
        assert_eq!(node.step(&input, &mut out), None);
    }
    out.0
}

fn node() -> GroundNode {
    GroundNode::new(&[]).unwrap()
}

fn events(recs: &[Rec]) -> Vec<(u16, Vec<u64>)> {
    recs.iter()
        .filter_map(|r| match r {
            Rec::Event(c, a) => Some((*c, a.clone())),
            Rec::Unit(_) | Rec::PacketOut(..) | Rec::PacketIn(..) => None,
        })
        .collect()
}

/// The dump SDU of §3.3.4 for `length` octets from `address`, cut into 128-octet chunks.
fn dump_chunks(address: u32, length: u16) -> Vec<Vec<u8>> {
    let mut sdu = address.to_be_bytes().to_vec();
    sdu.extend_from_slice(&length.to_be_bytes());
    sdu.extend((0..u32::from(length)).map(|i| {
        let a = address + i;
        u8::try_from((a ^ (a >> 8)) & 0xFF).unwrap()
    }));
    sdu.chunks(128).map(<[u8]>::to_vec).collect()
}

/// Dump packets for the chunks, counts from `first_count`: flags `01`, `00`, `10` (or `11`).
fn dump_packets(chunks: &[Vec<u8>], first_count: u16) -> Vec<Vec<u8>> {
    let last = chunks.len() - 1;
    chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let flags = match (i, i == last) {
                (0, true) => SequenceFlags::Unsegmented,
                (0, false) => SequenceFlags::First,
                (_, true) => SequenceFlags::Last,
                _ => SequenceFlags::Continuation,
            };
            let count = first_count + u16::try_from(i).unwrap();
            tm(DUMP, flags, count, false, chunk)
        })
        .collect()
}

#[test]
fn every_accepted_packet_is_delivered_without_events_in_sequence() {
    // req: APP-011, APP-130
    let units = [hk(0), hk(1), payload(0), hk(2)];
    let recs = deliver(&mut node(), &units);
    let expected: Vec<Rec> = [(HK, 0), (HK, 1), (PAYLOAD, 2), (HK, 3)]
        .iter()
        .map(|&(apid, i)| Rec::PacketIn(apid, units[i].clone()))
        .collect();
    assert_eq!(recs, expected);
}

#[test]
fn time_stamps_decode_with_implicit_and_explicit_p_fields() {
    // req: APP-011, APP-022
    let mut n = node();
    assert_eq!(n.last_time_stamp(), None);
    // Implicit (housekeeping): T0 + 7 s plus a fraction of 0x8000 / 65 536 = 1/2 s.
    let mut secondary = t_field(7, 0x8000);
    let unit = packet(
        PacketType::Telemetry,
        HK,
        SequenceFlags::Unsegmented,
        0,
        &secondary,
        &[0; 4],
    );
    deliver(&mut n, &[unit]);
    let stamp = n.last_time_stamp().unwrap();
    assert_eq!(
        (stamp.apid.get(), stamp.seconds, stamp.nanos),
        (HK, 2_100_000_007, 500_000_000)
    );
    // Explicit (events): P-field 0x1E, then the T-field.
    secondary = vec![0x1E];
    secondary.extend(t_field(9, 0x4000));
    let unit = packet(
        PacketType::Telemetry,
        EVENTS,
        SequenceFlags::Unsegmented,
        0,
        &secondary,
        &[0; 8],
    );
    let recs = deliver(&mut n, std::slice::from_ref(&unit));
    assert_eq!(recs, [Rec::PacketIn(EVENTS, unit)]);
    let stamp = n.last_time_stamp().unwrap();
    assert_eq!(
        (stamp.apid.get(), stamp.seconds, stamp.nanos),
        (EVENTS, 2_100_000_009, 250_000_000)
    );
}

#[test]
fn a_gap_a_repeat_and_a_reset_are_events_after_packet_in() {
    // req: APP-018, APP-130
    // The sequences of §6.6, `a1-downlink-faults`: HK 0, (1 dropped), 2: gap of 1.
    let recs = deliver(&mut node(), &[hk(0), hk(2)]);
    assert!(matches!(recs[0], Rec::PacketIn(HK, _)));
    assert!(matches!(recs[1], Rec::PacketIn(HK, _)));
    assert_eq!(events(&recs), [(telemetry::SEQ_GAP, vec![16, 1])]);
    assert_eq!(recs.len(), 3, "packet_in, packet_in, then the event");
    assert!(matches!(recs[2], Rec::Event(..)));

    // A duplicate: packet_in twice, the second raises seq_repeat (32, 1).
    let recs = deliver(&mut node(), &[payload(0), payload(1), payload(1)]);
    assert_eq!(
        recs.iter()
            .map(|r| matches!(r, Rec::PacketIn(..)))
            .collect::<Vec<_>>(),
        [true, true, true, false]
    );
    assert_eq!(events(&recs), [(telemetry::SEQ_REPEAT, vec![32, 1])]);

    // A delayed packet: counts 2, 4, 3, 5 (§6.6): gap (16, 1), reset (16, 3), gap (16, 1).
    let recs = deliver(&mut node(), &[hk(2), hk(4), hk(3), hk(5)]);
    assert_eq!(
        events(&recs),
        [
            (telemetry::SEQ_GAP, vec![16, 1]),
            (telemetry::SEQ_RESET, vec![16, 3]),
            (telemetry::SEQ_GAP, vec![16, 1]),
        ]
    );
}

#[test]
fn counts_are_tracked_per_apid_and_wrap_modulo_16384() {
    // req: APP-018, APP-021
    let recs = deliver(
        &mut node(),
        &[hk(16_383), payload(5), hk(0), payload(6), hk(1)],
    );
    assert_eq!(events(&recs), []);
}

#[test]
fn a_strict_decoder_error_is_packet_rejected_and_nothing_else() {
    // req: APP-018, APP-130, API-030
    let mut n = node();
    // Truncated to 20 octets (§6.6): Incomplete.
    let mut cut = payload(3);
    cut.truncate(10);
    let recs = deliver(&mut n, &[hk(0), cut]);
    assert_eq!(
        events(&recs),
        [(telemetry::PACKET_REJECTED, vec![INCOMPLETE])]
    );
    assert_eq!(recs.len(), 2, "one packet_in, then the rejection alone");

    let check = |unit: Vec<u8>, code: u64| {
        let recs = deliver(&mut node(), &[unit]);
        assert_eq!(
            recs,
            [Rec::Event(telemetry::PACKET_REJECTED, vec![code])],
            "code {code}"
        );
    };
    // Version 001.
    let mut bad_version = hk(0);
    bad_version[0] |= 0x20;
    check(bad_version, UNSUPPORTED_VERSION);
    // An octet after the packet.
    let mut long = hk(0);
    long.push(0);
    check(long, LENGTH_MISMATCH);
    // An APID that is not telemetry of the scenario (the commands APID, and another one).
    check(
        packet(
            PacketType::Telemetry,
            128,
            SequenceFlags::Unsegmented,
            0,
            &[],
            &[1],
        ),
        UNKNOWN_CHANNEL,
    );
    check(
        packet(
            PacketType::Telemetry,
            5,
            SequenceFlags::Unsegmented,
            0,
            &[],
            &[1],
        ),
        UNKNOWN_CHANNEL,
    );
    // A telecommand packet on a telemetry APID.
    check(
        packet(
            PacketType::Telecommand,
            HK,
            SequenceFlags::Unsegmented,
            0,
            &t_field(0, 0),
            &[0; 4],
        ),
        INVALID_FIELD,
    );
    // Sequence flags `01` on an APID that is not segmented.
    check(
        tm(HK, SequenceFlags::First, 0, false, &[0; 4]),
        INVALID_FIELD,
    );
    // No secondary header on an APID that has one.
    check(
        packet(
            PacketType::Telemetry,
            HK,
            SequenceFlags::Unsegmented,
            0,
            &[],
            &[0; 4],
        ),
        INVALID_FIELD,
    );
    // The events APID without its P-field: the T-field is read as a P-field.
    check(
        tm(EVENTS, SequenceFlags::Unsegmented, 0, false, &[0; 8]),
        INVALID_FIELD,
    );
    // A secondary header cut off inside the time code.
    check(
        packet(
            PacketType::Telemetry,
            HK,
            SequenceFlags::Unsegmented,
            0,
            &[0x7D, 0x2B],
            &[],
        ),
        LENGTH_MISMATCH,
    );
    // The explicit P-field of another format (3 coarse octets, 2 fine): not the code of §5.
    let mut secondary = vec![0x1D];
    secondary.extend_from_slice(&[0x7D, 0x2B, 0x75, 0, 0]);
    check(
        packet(
            PacketType::Telemetry,
            EVENTS,
            SequenceFlags::Unsegmented,
            0,
            &secondary,
            &[0; 8],
        ),
        INVALID_FIELD,
    );
}

#[test]
fn a_rejected_packet_does_not_move_the_sequence_tracker() {
    // req: APP-130
    // Count 3 is cut to a stub, so count 4 follows count 2 (§6.6, unit 11).
    let mut cut = payload(3);
    cut.truncate(10);
    let recs = deliver(&mut node(), &[payload(2), cut, payload(4)]);
    assert_eq!(
        events(&recs),
        [
            (telemetry::PACKET_REJECTED, vec![INCOMPLETE]),
            (telemetry::SEQ_GAP, vec![32, 1]),
        ]
    );
}

#[test]
fn the_nominal_dump_is_reassembled() {
    // req: APP-011, APP-018, APP-130
    let chunks = dump_chunks(0x0001_0000, 600);
    assert_eq!(
        chunks.iter().map(Vec::len).collect::<Vec<_>>(),
        [128, 128, 128, 128, 94]
    );
    let packets = dump_packets(&chunks, 0);
    let recs = deliver(&mut node(), &packets);
    assert_eq!(
        events(&recs),
        [(telemetry::DUMP_REASSEMBLED, vec![65_536, 600, 32_936])]
    );
    // The event comes after the last packet's packet_in and nothing follows it.
    assert!(matches!(recs[recs.len() - 2], Rec::PacketIn(DUMP, _)));
    assert!(matches!(recs[recs.len() - 1], Rec::Event(..)));
}

#[test]
fn the_300_octet_dump_of_a1_rejects() {
    // req: APP-018
    let recs = deliver(
        &mut node(),
        &dump_packets(&dump_chunks(0x0001_0000, 300), 0),
    );
    assert_eq!(
        events(&recs),
        [(telemetry::DUMP_REASSEMBLED, vec![65_536, 300, 32_736])]
    );
}

#[test]
fn an_unsegmented_dump_is_reassembled() {
    // req: APP-011, APP-130
    let chunks = dump_chunks(0x0000_0010, 4);
    assert_eq!(chunks.len(), 1);
    let recs = deliver(&mut node(), &dump_packets(&chunks, 7));
    assert_eq!(events(&recs).len(), 1);
    assert_eq!(events(&recs)[0].1[..2], [16, 4]);
}

#[test]
fn a_dropped_dump_packet_discards_the_partial_sdu() {
    // req: APP-018, APP-130
    // §6.6: the third packet (count 2) is dropped; count 3 arrives after count 1.
    let packets = dump_packets(&dump_chunks(0x0001_0000, 600), 0);
    let delivered = [
        packets[0].clone(),
        packets[1].clone(),
        packets[3].clone(),
        packets[4].clone(),
    ];
    let recs = deliver(&mut node(), &delivered);
    // First two: no events. Count 3: packet_in, seq_gap (33, 1), dump_discarded (33, 256).
    // Count 4 (`10`) is an orphan: packet_in only, and no dump_reassembled.
    let kinds: Vec<_> = recs
        .iter()
        .map(|r| match r {
            Rec::PacketIn(..) => "in".to_owned(),
            Rec::Event(c, a) => format!("event {c} {a:?}"),
            other @ (Rec::Unit(_) | Rec::PacketOut(..)) => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "in",
            "in",
            "in",
            "event 1 [33, 1]",
            "event 5 [33, 256]",
            "in"
        ]
    );
}

#[test]
fn a_restarted_dump_discards_what_was_held() {
    // req: APP-018
    let packets = dump_packets(&dump_chunks(0x0001_0000, 600), 0);
    // Counts 0, 1 (`01`, `00`), then a new first segment with count 2: in sequence, so the
    // reassembler restarts and throws away the 256 octets it held.
    let restart = tm(DUMP, SequenceFlags::First, 2, false, &[0; 10]);
    let recs = deliver(
        &mut node(),
        &[packets[0].clone(), packets[1].clone(), restart],
    );
    assert_eq!(events(&recs), [(telemetry::DUMP_DISCARDED, vec![33, 256])]);
}

#[test]
fn a_dump_that_does_not_fit_is_discarded_with_its_octets() {
    // req: APP-018
    // Eight 128-octet segments are 1 024 octets, then 128 more overflow the buffer of 6 + 1 024 octets (`SDU_HEADER_LEN` and
    // `MAX_DUMP_LENGTH` in telemetry.rs).
    let mut units = vec![tm(DUMP, SequenceFlags::First, 0, false, &[0; 128])];
    for count in 1..=8 {
        units.push(tm(
            DUMP,
            SequenceFlags::Continuation,
            count,
            false,
            &[0; 128],
        ));
    }
    let recs = deliver(&mut node(), &units);
    // The ninth segment overflows: the event counts the 1 024 octets held, not its own 128.
    assert_eq!(
        events(&recs),
        [(telemetry::DUMP_DISCARDED, vec![33, 1_024])]
    );
}

#[test]
fn an_oversized_segment_with_nothing_held_raises_no_event() {
    // req: APP-130
    for flags in [SequenceFlags::First, SequenceFlags::Unsegmented] {
        let recs = deliver(&mut node(), &[tm(DUMP, flags, 0, false, &[0; 1_100])]);
        assert!(
            matches!(recs[..], [Rec::PacketIn(DUMP, _)]),
            "{flags:?}: {recs:?}"
        );
    }
}

#[test]
fn a_malformed_dump_sdu_is_a_rejection() {
    // req: APP-011, APP-130
    // A complete SDU of 3 octets has no header.
    let recs = deliver(
        &mut node(),
        &[tm(DUMP, SequenceFlags::Unsegmented, 0, false, &[1, 2, 3])],
    );
    // The rejection follows packet_in: the failure is found only once the SDU is complete.
    assert!(matches!(recs[..], [Rec::PacketIn(DUMP, _), Rec::Event(..)]));
    assert_eq!(
        events(&recs),
        [(telemetry::PACKET_REJECTED, vec![INCOMPLETE])]
    );
}

#[test]
fn several_units_in_one_step_are_handled_in_delivery_order() {
    // req: APP-130
    let (a, b) = (hk(0), hk(2));
    let delivered: [&[u8]; 2] = [&a, &b];
    let input = StepInput {
        now: Instant::from_micros(1),
        delivered: Delivered::new(&delivered),
        ops: &[],
        tx_slot: true,
    };
    let mut out = Recorder::default();
    node().step(&input, &mut out);
    assert_eq!(
        out.0,
        [
            Rec::PacketIn(HK, a),
            Rec::PacketIn(HK, b),
            Rec::Event(telemetry::SEQ_GAP, vec![16, 1]),
        ]
    );
}

const fn noop() -> OpsEntry {
    OpsEntry::Command {
        execution: Execution::TimeTagged {
            after_t0: Duration::from_secs(6),
        },
        call: Call::Noop,
        extra_octets: 0,
    }
}

#[test]
fn due_ops_entries_are_output_as_packet_then_unit_after_deliveries() {
    // req: APP-011, APP-024
    let script = [
        noop(),
        OpsEntry::TableUpload {
            table_id: TableId::new(1).unwrap(),
            length: TableLength::new(300).unwrap(),
        },
        OpsEntry::Fop1Directive,
        noop(),
    ];
    let mut n = GroundNode::new(&script).unwrap();
    let unit = hk(0);
    let delivered: [&[u8]; 1] = [&unit];
    let input = StepInput {
        now: Instant::from_micros(1_000_000),
        delivered: Delivered::new(&delivered),
        ops: &[0, 1, 2],
        tx_slot: true,
    };
    let mut out = Recorder::default();
    assert_eq!(n.step(&input, &mut out), None);
    let [
        Rec::PacketIn(HK, _),
        Rec::PacketOut(128, cmd),
        Rec::Unit(cmd_unit),
        Rec::PacketOut(129, table),
        Rec::Unit(table_unit),
    ] = &out.0[..]
    else {
        panic!("{:?}", out.0);
    };
    assert_eq!((cmd.len(), table.len()), (14, 309));
    assert_eq!(
        (cmd, table),
        (cmd_unit, table_unit),
        "the unit is the packet"
    );

    // A later entry takes the next command count; a stale or unknown index sends nothing.
    let input = StepInput {
        now: Instant::from_micros(3_000_000),
        delivered: Delivered::new(&[]),
        ops: &[3, 9],
        tx_slot: true,
    };
    let mut out = Recorder::default();
    n.step(&input, &mut out);
    let [Rec::PacketOut(128, second), Rec::Unit(_)] = &out.0[..] else {
        panic!("{:?}", out.0);
    };
    assert_eq!(second[2..4], [0xC0, 0x01], "flags 11, count 1");
}

#[test]
fn stepping_with_nothing_due_changes_nothing() {
    // req: APP-074
    let mut n = node();
    let input = StepInput {
        now: Instant::from_micros(5),
        delivered: Delivered::new(&[]),
        ops: &[],
        tx_slot: true,
    };
    let mut out = Recorder::default();
    assert_eq!(n.step(&input, &mut out), None);
    assert_eq!(out.0, []);
}

#[test]
fn a_script_that_cannot_be_built_is_an_error_naming_the_entry() {
    // req: APP-126
    let e = GroundNode::new(&[
        noop(),
        OpsEntry::Command {
            execution: Execution::TimeTagged {
                // 2 200 000 000 s after T0 does not fit four coarse octets.
                after_t0: Duration::from_secs(2_200_000_000),
            },
            call: Call::Noop,
            extra_octets: 0,
        },
    ])
    .unwrap_err();
    assert!(e.to_string().starts_with("ops[1]: "), "{e}");
}
