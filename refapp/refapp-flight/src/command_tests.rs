//! Tests of the command handler, the time-tag queue, the table store, and the memory dump
//! through the node interface (docs/reference-application.md §2.1, §2.3, §3.3.2, §3.3.4–3.3.6).

extern crate std;

use std::vec::Vec;

use aldis::core::ErrorKind;
use aldis::core::crc::crc16;
use aldis::core::ids::{Apid, SeqCount};
use aldis::spp::{
    PacketFields, PacketType, SequenceFlags, SpacePacket, SpacePacketWriter, SppConfig,
};
use refapp_scenario::generated::{apid, time};
use refapp_scenario::{Instant, Node, StepInput};

use crate::dump::memory_octet;
use crate::tests::{At, Out, Rec, packets};
use crate::{Event, FlightNode, PayloadMode};

/// Microseconds after the start of the run, as a CUC T-field of the scenario (§5): 4 coarse and
/// 2 fractional octets, T0 + `us`.
fn t_field(us: u64) -> [u8; 6] {
    let seconds = u32::try_from(us / 1_000_000)
        .unwrap()
        .wrapping_add(time::T0_COARSE_S);
    let fraction = u16::try_from((us % 1_000_000).wrapping_mul(65_536) / 1_000_000).unwrap();
    let [s3, s2, s1, s0] = seconds.to_be_bytes();
    let [f1, f0] = fraction.to_be_bytes();
    [s3, s2, s1, s0, f1, f0]
}

/// A telecommand packet of `apid_value` with `data` and sequence flags `flags`.
fn tc_with(apid_value: u16, flags: SequenceFlags, seq: u16, data: &[u8]) -> Vec<u8> {
    let mut buffer = [0_u8; 600];
    let fields = PacketFields {
        packet_type: PacketType::Telecommand,
        apid: Apid::new(apid_value).unwrap(),
        sequence_flags: flags,
        sequence_count: SeqCount::new(seq).unwrap(),
    };
    let mut writer =
        SpacePacketWriter::begin(&mut buffer, &SppConfig::DEFAULT, fields, 0, data.len()).unwrap();
    writer.user_data_mut().copy_from_slice(data);
    let total = writer.finish(data.len()).unwrap();
    buffer[..total].to_vec()
}

fn tc(apid_value: u16, seq: u16, data: &[u8]) -> Vec<u8> {
    tc_with(apid_value, SequenceFlags::Unsegmented, seq, data)
}

/// A command packet (§3.3.5): `at` is the execution time, µs after T0, or `None` for immediate.
fn command(seq: u16, function: u8, at: Option<u64>, args: &[u8]) -> Vec<u8> {
    let (mode, field) = at.map_or((0, [0; 6]), |us| (1, t_field(us)));
    let mut data = std::vec![function, mode];
    data.extend_from_slice(&field);
    data.extend_from_slice(args);
    tc(apid::COMMANDS, seq, &data)
}

fn noop(seq: u16, at: Option<u64>) -> Vec<u8> {
    command(seq, 1, at, &[])
}

fn dump_cmd(seq: u16, at: Option<u64>, address: u32, length: u16) -> Vec<u8> {
    let mut args = address.to_be_bytes().to_vec();
    args.extend_from_slice(&length.to_be_bytes());
    command(seq, 2, at, &args)
}

fn activate(seq: u16, at: Option<u64>, table_id: u8, crc: u16) -> Vec<u8> {
    let [c1, c0] = crc.to_be_bytes();
    command(seq, 3, at, &[table_id, c1, c0])
}

fn set_mode(seq: u16, at: Option<u64>, mode: u8) -> Vec<u8> {
    command(seq, 4, at, &[mode])
}

/// A table-upload packet (§3.3.6) with `actual` octets of the scenario pattern.
fn upload(seq: u16, table_id: u8, declared: u16, actual: usize) -> Vec<u8> {
    let mut data = std::vec![table_id];
    data.extend_from_slice(&declared.to_be_bytes());
    data.extend(
        (0..actual).map(|i| u8::try_from(i.wrapping_mul(3).wrapping_add(1) % 256).unwrap()),
    );
    tc(apid::TABLE_UPLOAD, seq, &data)
}

/// An event or an application event, in the order the node output them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Logged {
    /// An event packet: code and p1…p3.
    Event(u16, [u16; 3]),
    /// An application event: code and arguments.
    App(u16, Vec<u64>),
}

/// The events and application events of `rec`, with the instant of the step that output them.
fn log(rec: &Rec) -> Vec<(u64, Logged)> {
    rec.0
        .iter()
        .filter_map(|(t, o)| match o {
            Out::Packet(a, p) if *a == apid::EVENTS => {
                let user = p.get(13..21).unwrap();
                let w: Vec<u16> = user
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                Some((*t, Logged::Event(w[0], [w[1], w[2], w[3]])))
            }
            Out::Event(code, args) => Some((*t, Logged::App(*code, args.clone()))),
            Out::Packet(..) | Out::Unit(_) => None,
        })
        .collect()
}

fn events(rec: &Rec) -> Vec<Logged> {
    log(rec).into_iter().map(|(_, l)| l).collect()
}

/// A node with the output of every step.
#[derive(Default)]
struct Run {
    node: FlightNode,
    rec: Rec,
}

impl Run {
    fn step(&mut self, now: u64, delivered: &[Vec<u8>]) -> Option<u64> {
        let refs: Vec<&[u8]> = delivered.iter().map(Vec::as_slice).collect();
        let input = StepInput {
            now: Instant::from_micros(now),
            delivered: &refs,
            ops: &[],
            tx_slot: true,
        };
        self.node
            .step(&input, &mut At(now, &mut self.rec))
            .map(Instant::as_micros)
    }

    /// Forgets the output so far.
    fn clear(&mut self) {
        self.rec = Rec::default();
    }
}

fn executed(function: u16, seq: u16) -> Logged {
    Logged::Event(0x0001, [function, seq, 0])
}

fn rejected(function: u16, seq: u16, reason: u16) -> Logged {
    Logged::Event(0x0002, [function, seq, reason])
}

fn received(function: u64, seq: u64) -> Logged {
    Logged::App(20, std::vec![function, seq])
}

#[test]
fn an_immediate_command_executes_in_the_step_that_delivers_it() {
    // req: APP-012, APP-013, APP-129
    let mut run = Run::default();
    run.step(125_000, &[noop(6, None)]);
    assert_eq!(events(&run.rec), [received(1, 6), executed(1, 6)]);
    assert_eq!(run.node.status().commands_executed, 1);
    assert_eq!(run.node.status().commands_rejected, 0);
}

#[test]
fn a_time_tagged_command_executes_in_the_first_step_at_or_after_its_time() {
    // req: APP-012, APP-013
    let mut run = Run::default();
    run.step(
        1_010_000,
        &[noop(0, Some(6_000_000)), noop(1, Some(7_000_000))],
    );
    assert_eq!(events(&run.rec), [received(1, 0), received(1, 1)]);
    assert_eq!(run.node.status().queue_occupancy, 2);
    // The housekeeping packet of the same step reports it (§3.3.1 octet 8).
    let (_, hk) = packets(&run.rec, apid::HOUSEKEEPING).pop().unwrap();
    assert_eq!(hk[12 + 8], 2);
    run.clear();
    run.step(5_999_000, &[]);
    assert_eq!(events(&run.rec).len(), 0);
    // Exactly at the time of the first command.
    run.step(6_000_000, &[]);
    assert_eq!(log(&run.rec), [(6_000_000, executed(1, 0))]);
    assert_eq!(run.node.status().queue_occupancy, 1);
    // A late step: the second command was due at 7 s and executes at 8.5 s.
    run.clear();
    run.step(8_500_000, &[]);
    assert_eq!(log(&run.rec), [(8_500_000, executed(1, 1))]);
    assert_eq!(run.node.status().queue_occupancy, 0);
}

#[test]
fn a_command_tagged_for_the_delivery_instant_executes_in_that_step() {
    // req: APP-012
    let mut run = Run::default();
    run.step(3_000_000, &[noop(0, Some(3_000_000))]);
    assert_eq!(events(&run.rec), [received(1, 0), executed(1, 0)]);
}

#[test]
fn due_commands_execute_by_time_and_in_delivery_order_for_equal_times() {
    // req: APP-012
    let mut run = Run::default();
    run.step(
        1_000_000,
        &[
            noop(0, Some(10_000_000)),
            noop(1, Some(8_000_000)),
            noop(2, Some(10_000_000)),
        ],
    );
    run.clear();
    run.step(10_000_000, &[]);
    assert_eq!(
        events(&run.rec),
        [executed(1, 1), executed(1, 0), executed(1, 2)]
    );
}

#[test]
fn the_deadline_is_the_next_queued_command_or_dump_packet() {
    // req: APP-012, APP-016
    let mut run = Run::default();
    // After the last housekeeping and payload slot, only the queue and the dump remain.
    run.step(29_000_000, &[]);
    assert_eq!(
        run.step(29_010_000, &[noop(0, Some(31_000_000))]),
        Some(31_000_000)
    );
    run.step(31_000_000, &[dump_cmd(1, None, 0, 200)]);
    // The dump started at 31 s: its next packet is due 125 ms later.
    assert_eq!(run.step(31_000_000, &[]), Some(31_125_000));
    run.step(31_125_000, &[]);
    assert_eq!(run.step(31_125_000, &[]), None);
}

#[test]
fn rejection_reason_1_unknown_function_at_execution() {
    // req: APP-013, APP-129
    let mut run = Run::default();
    run.step(625_000, &[command(1, 127, None, &[])]);
    assert_eq!(events(&run.rec), [received(127, 1), rejected(127, 1, 1)]);
    // Time-tagged: reported at its execution time, not at delivery.
    run.clear();
    run.step(700_000, &[command(2, 99, Some(2_000_000), &[1, 2])]);
    assert_eq!(events(&run.rec), [received(99, 2)]);
    run.step(2_000_000, &[]);
    assert_eq!(log(&run.rec)[1], (2_000_000, rejected(99, 2, 1)));
    assert_eq!(run.node.status().commands_rejected, 2);
}

#[test]
fn rejection_reason_2_time_tag_in_the_past_at_delivery() {
    // req: APP-012, APP-013
    let mut run = Run::default();
    run.step(3_625_000, &[noop(3, Some(3_000_000))]);
    assert_eq!(events(&run.rec), [received(1, 3), rejected(1, 3, 2)]);
    assert_eq!(run.node.status().queue_occupancy, 0);
    // A time tag before T0 is in the past too.
    run.clear();
    let mut data = std::vec![1_u8, 1];
    data.extend_from_slice(&[0, 0, 0, 5, 0, 0]);
    run.step(4_000_000, &[tc(apid::COMMANDS, 4, &data)]);
    assert_eq!(events(&run.rec), [received(1, 4), rejected(1, 4, 2)]);
}

#[test]
fn rejection_reason_3_malformed_arguments_at_execution() {
    // req: APP-013, APP-024, APP-129
    let mut run = Run::default();
    // A NOOP with one extra octet and a time tag of 3 s.
    run.step(1_115_000, &[command(2, 1, Some(3_000_000), &[0])]);
    assert_eq!(events(&run.rec), [received(1, 2)]);
    run.step(3_000_000, &[]);
    assert_eq!(log(&run.rec)[1], (3_000_000, rejected(1, 2, 3)));
    // Wrong lengths for the other functions, immediate.
    run.clear();
    run.step(
        4_000_000,
        &[
            command(3, 2, None, &[0, 0, 0, 0, 0]),
            command(4, 3, None, &[1, 0]),
            command(5, 4, None, &[2]),
            command(6, 2, None, &[0, 0, 0, 0, 0x04, 0x01]),
        ],
    );
    assert_eq!(
        events(&run.rec)
            .into_iter()
            .filter(|e| matches!(e, Logged::Event(..)))
            .collect::<Vec<_>>(),
        [
            rejected(2, 3, 3),
            rejected(3, 4, 3),
            rejected(4, 5, 3),
            rejected(2, 6, 3)
        ]
    );
}

#[test]
fn a_command_without_a_valid_execution_instant_is_rejected_with_reason_3_at_delivery() {
    // req: APP-013, APP-024
    let mut run = Run::default();
    // Execution mode 2, and a non-zero T-field in an immediate command, with a valid function.
    let bad_mode = tc(apid::COMMANDS, 1, &[1, 2, 0, 0, 0, 0, 0, 0]);
    let immediate_with_time = tc(apid::COMMANDS, 2, &[1, 0, 0, 0, 0, 0, 0, 1]);
    run.step(1_000_000, &[bad_mode, immediate_with_time]);
    assert_eq!(
        log(&run.rec)
            .into_iter()
            .filter(|(_, l)| matches!(l, Logged::Event(..)))
            .collect::<Vec<_>>(),
        [
            (1_000_000, rejected(1, 1, 3)),
            (1_000_000, rejected(1, 2, 3))
        ]
    );
    assert_eq!(run.node.status().queue_occupancy, 0);
    assert_eq!(run.node.status().commands_rejected, 2);
}

#[test]
fn a_packet_too_short_to_be_a_command_is_malformed() {
    // req: APP-024
    let mut run = Run::default();
    // A Space Packet has at least one octet of data: here only the function code.
    run.step(500_000, &[tc(apid::COMMANDS, 9, &[1])]);
    assert_eq!(events(&run.rec), [received(1, 9), rejected(1, 9, 3)]);
}

#[test]
fn rejection_reason_4_time_tag_queue_full_at_delivery() {
    // req: APP-012, APP-013
    let mut run = Run::default();
    let nine: Vec<Vec<u8>> = (8..17).map(|seq| noop(seq, Some(20_000_000))).collect();
    run.step(10_115_000, &nine);
    let all = events(&run.rec);
    // Nine `cmd_received`, one rejection: seq 16, after its own `cmd_received`.
    assert_eq!(all.len(), 10);
    assert_eq!(all[8], received(1, 16));
    assert_eq!(all[9], rejected(1, 16, 4));
    assert_eq!(run.node.status().queue_occupancy, 8);
    // At 20 s the eight queued commands execute in queue order.
    run.clear();
    run.step(20_000_000, &[]);
    assert_eq!(
        events(&run.rec),
        (8..16).map(|seq| executed(1, seq)).collect::<Vec<_>>()
    );
    assert_eq!(run.node.status().queue_occupancy, 0);
    // A freed queue takes commands again.
    run.clear();
    run.step(20_100_000, &[noop(17, Some(21_000_000))]);
    assert_eq!(run.node.status().queue_occupancy, 1);
}

#[test]
fn every_command_gets_exactly_one_report() {
    // req: APP-013
    let mut run = Run::default();
    let delivered = [
        noop(0, None),
        command(1, 127, None, &[]),
        noop(2, Some(500_000)),
        noop(3, Some(3_000_000)),
        activate(4, None, 1, 0x0C4F),
        set_mode(5, None, 1),
        dump_cmd(6, None, 0x10000, 300),
        dump_cmd(7, None, 0x10000, 300),
        noop(8, Some(5_000_000)),
    ];
    run.step(1_000_000, &delivered);
    run.step(5_000_000, &[]);
    let reports: Vec<u16> = events(&run.rec)
        .into_iter()
        .filter_map(|e| match e {
            Logged::Event(0x0001 | 0x0002, [_, seq, _]) => Some(seq),
            Logged::Event(..) | Logged::App(..) => None,
        })
        .collect();
    let mut sorted = reports;
    sorted.sort_unstable();
    assert_eq!(sorted, (0..9).collect::<Vec<_>>());
    assert_eq!(
        run.node.status().commands_executed + run.node.status().commands_rejected,
        9
    );
}

#[test]
fn a_table_upload_is_stored_with_a_table_received_event_and_no_report() {
    // req: APP-017, APP-129
    let mut run = Run::default();
    run.step(2_010_000, &[upload(0, 1, 300, 300)]);
    assert_eq!(events(&run.rec), [Logged::App(21, std::vec![1, 300])]);
    assert_eq!(packets(&run.rec, apid::EVENTS).len(), 0);
}

#[test]
fn activating_the_stored_table_reports_executed_then_activated() {
    // req: APP-017, APP-023, APP-129
    let mut run = Run::default();
    run.step(2_010_000, &[upload(0, 1, 300, 300)]);
    run.clear();
    run.step(3_010_000, &[activate(1, Some(10_000_000), 1, 0x0C4F)]);
    run.step(10_000_000, &[]);
    assert_eq!(
        events(&run.rec),
        [
            received(3, 1),
            executed(3, 1),
            Logged::Event(0x0010, [1, 300, 0x0C4F])
        ]
    );
    assert_eq!(run.node.status().table_id, 1);
    assert_eq!(run.node.status().table_crc, 0x0C4F);
    // Housekeeping reports the table: ID at octet 10, CRC at octets 12 and 13.
    run.clear();
    run.step(11_000_000, &[]);
    let (_, hk) = packets(&run.rec, apid::HOUSEKEEPING).pop().unwrap();
    assert_eq!((hk[12 + 10], hk[12 + 12], hk[12 + 13]), (1, 0x0C, 0x4F));
}

#[test]
fn activation_without_a_matching_table_is_reason_1_and_a_wrong_crc_reason_2() {
    // req: APP-017, APP-129
    let mut run = Run::default();
    run.step(4_125_000, &[activate(4, None, 1, 0x0C4F)]);
    assert_eq!(
        events(&run.rec),
        [
            received(3, 4),
            executed(3, 4),
            Logged::Event(0x0011, [1, 1, 0])
        ]
    );
    run.clear();
    run.step(5_125_000, &[upload(0, 1, 300, 300)]);
    run.clear();
    // Another table ID: reason 1 again; the stored table is table 1.
    run.step(5_500_000, &[activate(5, None, 2, 0x0C4F)]);
    assert_eq!(events(&run.rec)[2], Logged::Event(0x0011, [2, 1, 0]));
    run.clear();
    run.step(6_125_000, &[activate(6, None, 1, 0x0C4E)]);
    assert_eq!(
        events(&run.rec),
        [
            received(3, 6),
            executed(3, 6),
            Logged::Event(0x0011, [1, 2, 0])
        ]
    );
    // Nothing was activated.
    assert_eq!(run.node.status().table_id, 0);
    assert_eq!(run.node.status().table_crc, 0);
    // An executed command each time (APP-129).
    assert_eq!(run.node.status().commands_executed, 3);
}

#[test]
fn a_table_of_503_octets_is_stored_and_a_longer_one_is_packet_rejected() {
    // req: APP-017
    let mut run = Run::default();
    run.step(1_000_000, &[upload(0, 7, 503, 503)]);
    assert_eq!(events(&run.rec), [Logged::App(21, std::vec![7, 503])]);
    run.clear();
    run.step(1_100_000, &[upload(1, 8, 504, 504)]);
    assert_eq!(
        events(&run.rec),
        [Logged::App(
            6,
            std::vec![u64::from(ErrorKind::OutOfRange.code())]
        )]
    );
    // The first table is still the stored one.
    run.clear();
    run.step(1_200_000, &[activate(0, None, 8, 0)]);
    assert_eq!(events(&run.rec)[2], Logged::Event(0x0011, [8, 1, 0]));
}

#[test]
fn a_malformed_table_upload_is_packet_rejected_and_keeps_the_stored_table() {
    // req: APP-017, APP-024
    let mut run = Run::default();
    run.step(1_000_000, &[upload(0, 1, 300, 300)]);
    run.clear();
    run.step(
        1_100_000,
        &[
            upload(1, 2, 10, 9),
            upload(2, 0, 4, 4),
            tc(apid::TABLE_UPLOAD, 3, &[1, 0]),
        ],
    );
    assert_eq!(
        events(&run.rec),
        [
            Logged::App(6, std::vec![u64::from(ErrorKind::LengthMismatch.code())]),
            Logged::App(6, std::vec![u64::from(ErrorKind::InvalidField.code())]),
            Logged::App(6, std::vec![u64::from(ErrorKind::Incomplete.code())]),
        ]
    );
    run.clear();
    run.step(1_200_000, &[activate(0, None, 1, 0x0C4F)]);
    assert_eq!(events(&run.rec)[2], Logged::Event(0x0010, [1, 300, 0x0C4F]));
}

#[test]
fn uplink_units_that_are_not_one_telecommand_packet_are_packet_rejected() {
    // req: APP-024
    let mut run = Run::default();
    let mut trailing = noop(0, None);
    trailing.push(0);
    let telemetry = {
        let mut buffer = [0_u8; 32];
        let fields = PacketFields {
            packet_type: PacketType::Telemetry,
            apid: Apid::new(apid::COMMANDS).unwrap(),
            sequence_flags: SequenceFlags::Unsegmented,
            sequence_count: SeqCount::new(0).unwrap(),
        };
        let mut w =
            SpacePacketWriter::begin(&mut buffer, &SppConfig::DEFAULT, fields, 0, 8).unwrap();
        w.user_data_mut().fill(1);
        let n = w.finish(8).unwrap();
        buffer[..n].to_vec()
    };
    let units = [
        std::vec![0_u8; 3],
        trailing,
        telemetry,
        tc(200, 0, &[1; 8]),
        tc_with(apid::COMMANDS, SequenceFlags::First, 0, &[1; 8]),
    ];
    run.step(1_000_000, &units);
    let kinds = [
        ErrorKind::Incomplete,
        ErrorKind::LengthMismatch,
        ErrorKind::InvalidField,
        ErrorKind::UnknownChannel,
        ErrorKind::InvalidField,
    ];
    assert_eq!(
        events(&run.rec),
        kinds
            .into_iter()
            .map(|k| Logged::App(6, std::vec![u64::from(k.code())]))
            .collect::<Vec<_>>()
    );
    // Not one of them is a command: no report, no status change.
    assert_eq!(run.node.status().commands_rejected, 0);
}

/// The dump packets of `rec`: (instant, flags, sequence count, user data).
fn dump_packets(rec: &Rec) -> Vec<(u64, SequenceFlags, u16, Vec<u8>)> {
    packets(rec, apid::DUMP)
        .into_iter()
        .map(|(t, bytes)| {
            let p = SpacePacket::decode(&bytes).unwrap();
            (
                t,
                p.sequence_flags(),
                p.sequence_count().get(),
                bytes[12..].to_vec(),
            )
        })
        .collect()
}

#[test]
fn the_nominal_dump_is_five_packets_one_every_125_ms() {
    // req: APP-016, APP-023, APP-021
    let mut run = Run::default();
    run.step(
        4_010_000,
        &[dump_cmd(2, Some(12_000_000), 0x0001_0000, 600)],
    );
    run.clear();
    // Step at each deadline from the execution instant on, as the harness does.
    let mut now = 12_000_000;
    let mut deadline = run.step(now, &[]);
    // bound: the dump has five packets, and the deadline is the next packet's slot.
    while let Some(next) = deadline.filter(|d| *d <= 12_500_000) {
        now = next;
        deadline = run.step(now, &[]);
    }
    let dump = dump_packets(&run.rec);
    let instants: Vec<u64> = dump.iter().map(|d| d.0).collect();
    assert_eq!(
        instants,
        [12_000_000, 12_125_000, 12_250_000, 12_375_000, 12_500_000]
    );
    assert_eq!(
        dump.iter().map(|d| d.1).collect::<Vec<_>>(),
        [
            SequenceFlags::First,
            SequenceFlags::Continuation,
            SequenceFlags::Continuation,
            SequenceFlags::Continuation,
            SequenceFlags::Last
        ]
    );
    assert_eq!(
        dump.iter().map(|d| d.2).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(
        dump.iter().map(|d| d.3.len()).collect::<Vec<_>>(),
        [128, 128, 128, 128, 94]
    );
    // The SDU: address, length, then the simulated memory (§3.3.4).
    let sdu: Vec<u8> = dump.iter().flat_map(|d| d.3.iter().copied()).collect();
    assert_eq!(sdu[..6], [0, 1, 0, 0, 0x02, 0x58]);
    assert!(
        sdu[6..]
            .iter()
            .enumerate()
            .all(|(i, o)| *o == memory_octet(0x0001_0000 + u32::try_from(i).unwrap()))
    );
    // The CRC-16 of the 600 dumped octets, hand-derived in §2.4.
    assert_eq!(crc16(&sdu[6..]), 0x80A8);
    // CMD_EXECUTED at 12.0 s, before the first packet; DUMP_COMPLETED at 12.5 s, after the last.
    let events = log(&run.rec);
    assert_eq!(
        events,
        [
            (12_000_000, executed(2, 2)),
            (12_500_000, Logged::Event(0x0020, [1, 0, 600]))
        ]
    );
    let last_packet = run
        .rec
        .0
        .iter()
        .rposition(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::DUMP))
        .unwrap();
    let completed = run
        .rec
        .0
        .iter()
        .rposition(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::EVENTS))
        .unwrap();
    assert!(completed > last_packet);
    let first_packet = run
        .rec
        .0
        .iter()
        .position(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::DUMP))
        .unwrap();
    let execution = run
        .rec
        .0
        .iter()
        .position(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::EVENTS))
        .unwrap();
    assert!(execution < first_packet);
    assert_eq!(run.node.status().dumps_completed, 1);
}

#[test]
fn dump_packets_decode_strictly_with_a_time_stamp_of_their_own_slot() {
    // req: APP-016, APP-022, APP-060
    let mut run = Run::default();
    run.step(1_000_000, &[dump_cmd(0, None, 0, 300)]);
    run.step(1_125_000, &[]);
    run.step(1_250_000, &[]);
    for (_, bytes) in packets(&run.rec, apid::DUMP) {
        let p = SpacePacket::decode(&bytes).unwrap();
        assert_eq!(p.packet_type(), PacketType::Telemetry);
        assert!(p.has_secondary_header());
        assert!(bytes.len() <= 12 + 128);
    }
    let stamps: Vec<(u8, u8)> = packets(&run.rec, apid::DUMP)
        .iter()
        .map(|(_, p)| (p[9], p[10]))
        .collect();
    // 1 s, 1.125 s, 1.250 s after T0: seconds 1, fraction 0x0000, 0x2000, 0x4000.
    assert_eq!(stamps, [(1, 0x00), (1, 0x20), (1, 0x40)]);
}

#[test]
fn a_late_step_sends_every_packet_due_each_with_its_own_slot_instant() {
    // req: APP-016, APP-060, APP-074
    let mut run = Run::default();
    run.step(1_000_000, &[dump_cmd(0, None, 0, 300)]);
    run.clear();
    run.step(1_400_000, &[]);
    let dump = dump_packets(&run.rec);
    assert_eq!(dump.len(), 2);
    assert_eq!(dump[0].1, SequenceFlags::Continuation);
    assert_eq!(dump[1].1, SequenceFlags::Last);
    assert_eq!(
        log(&run.rec),
        [(1_400_000, Logged::Event(0x0020, [0, 0, 300]))]
    );
}

#[test]
fn a_dump_command_while_a_dump_runs_is_rejected_with_reason_5_before_the_packet() {
    // req: APP-016, APP-013
    let mut run = Run::default();
    run.step(8_125_000, &[dump_cmd(6, None, 0x0001_0000, 300)]);
    run.clear();
    // The step of the second packet: the rejection comes first (§2.1 steps 1 and 3).
    run.step(8_250_000, &[dump_cmd(7, None, 0x0001_0000, 300)]);
    assert_eq!(events(&run.rec), [received(2, 7), rejected(2, 7, 5)]);
    assert_eq!(dump_packets(&run.rec).len(), 1);
    let first_report = run
        .rec
        .0
        .iter()
        .position(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::EVENTS))
        .unwrap();
    let first_dump = run
        .rec
        .0
        .iter()
        .position(|(_, o)| matches!(o, Out::Packet(a, _) if *a == apid::DUMP))
        .unwrap();
    assert!(first_report < first_dump);
    // Also in the step of the last packet, which handles commands first.
    run.clear();
    run.step(8_375_000, &[dump_cmd(8, None, 0, 1)]);
    assert_eq!(events(&run.rec)[..2], [received(2, 8), rejected(2, 8, 5)]);
    assert_eq!(run.node.status().dumps_completed, 1);
    // The dump is over: the next one is accepted.
    run.clear();
    run.step(8_500_000, &[dump_cmd(9, None, 0, 1)]);
    // The dump of one octet is a single packet, sent in the step that executes the command.
    assert_eq!(
        events(&run.rec),
        [
            received(2, 9),
            executed(2, 9),
            Logged::Event(0x0020, [0, 0, 1])
        ]
    );
    // A rejected command does not stop the dump in progress.
    assert_eq!(run.node.status().commands_rejected, 2);
}

#[test]
fn a_time_tagged_dump_command_is_rejected_at_execution_while_a_dump_runs() {
    // req: APP-016
    let mut run = Run::default();
    run.step(
        1_000_000,
        &[
            dump_cmd(0, None, 0, 600),
            dump_cmd(1, Some(1_125_000), 0, 8),
        ],
    );
    run.clear();
    run.step(1_125_000, &[]);
    assert_eq!(events(&run.rec), [rejected(2, 1, 5)]);
}

#[test]
fn the_dump_sdu_is_cut_into_chunks_of_128_octets() {
    // req: APP-016
    // The SDU is 6 + L octets: L = 1 and L = 122 give one packet (flags `11`), L = 123 two, and
    // L = 1 024 gives 1 030 octets in nine packets.
    for (length, chunks) in [
        (1_u16, std::vec![7_usize]),
        (122, std::vec![128]),
        (123, std::vec![128, 1]),
        (1_024, std::vec![128, 128, 128, 128, 128, 128, 128, 128, 6]),
    ] {
        let mut run = Run::default();
        run.step(0, &[dump_cmd(0, None, 0xFFFF_0000, length)]);
        let mut now = 0;
        // bound: at most nine packets, one per 125 ms slot.
        while let Some(next) = run.step(now, &[]).filter(|d| *d < 1_200_000) {
            now = next;
        }
        let dump = dump_packets(&run.rec);
        assert_eq!(
            dump.iter().map(|d| d.3.len()).collect::<Vec<_>>(),
            chunks,
            "L = {length}"
        );
        if chunks.len() == 1 {
            assert_eq!(dump[0].1, SequenceFlags::Unsegmented);
        }
        assert_eq!(run.node.status().dumps_completed, 1);
    }
}

#[test]
fn the_dump_keeps_its_sequence_count_across_dumps() {
    // req: APP-021, APP-016
    let mut run = Run::default();
    run.step(0, &[dump_cmd(0, None, 0, 1)]);
    run.step(1_000_000, &[dump_cmd(1, None, 0, 1)]);
    assert_eq!(
        dump_packets(&run.rec)
            .iter()
            .map(|d| d.2)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(run.node.status().dumps_completed, 2);
}

#[test]
fn set_payload_mode_changes_the_mode_at_its_execution_instant() {
    // req: APP-015, APP-012
    let mut run = Run::default();
    run.step(
        0,
        &[
            set_mode(0, Some(1_000_000), 0),
            set_mode(1, Some(3_000_000), 1),
        ],
    );
    run.step(500_000, &[]);
    run.step(1_000_000, &[]);
    assert_eq!(run.node.status().payload_mode, PayloadMode::Off);
    run.step(2_500_000, &[]);
    run.step(3_000_000, &[]);
    assert_eq!(run.node.status().payload_mode, PayloadMode::On);
    run.step(4_500_000, &[]);
    let payload: Vec<u64> = packets(&run.rec, apid::PAYLOAD)
        .iter()
        .map(|p| p.0)
        .collect();
    // The slot at 2.5 s fell in the off period; the numbering continues.
    assert_eq!(payload, [500_000, 4_500_000]);
    assert_eq!(
        events(&run.rec)
            .into_iter()
            .filter(|e| matches!(e, Logged::Event(0x0030, _)))
            .collect::<Vec<_>>(),
        [
            Logged::Event(0x0030, [0, 0, 0]),
            Logged::Event(0x0030, [1, 0, 0])
        ]
    );
}

/// Steps `node` at its own deadlines and at the delivery instants of `deliveries`, and also
/// at every multiple of `wake_every` µs when it is given, until `end_us`.
fn replay(deliveries: &[(u64, Vec<u8>)], end_us: u64, wake_every: Option<u64>) -> Run {
    let mut run = Run::default();
    let mut now = 0_u64;
    let mut deadline = None;
    loop {
        let due: Vec<Vec<u8>> = deliveries
            .iter()
            .filter(|(at, _)| *at == now)
            .map(|(_, p)| p.clone())
            .collect();
        deadline = run.step(now, &due).or(deadline.filter(|d| *d > now));
        let next_delivery = deliveries.iter().map(|d| d.0).filter(|at| *at > now).min();
        let next_wake =
            wake_every.map(|w| now.checked_div(w).unwrap().wrapping_add(1).wrapping_mul(w));
        let Some(next) = [deadline, next_delivery, next_wake]
            .into_iter()
            .flatten()
            .min()
        else {
            return run;
        };
        if next > end_us {
            return run;
        }
        now = next;
    }
}

/// The ops script of the nominal case (§2.4) as the flight node receives it: each entry 10 ms
/// after its release.
fn nominal_deliveries() -> Vec<(u64, Vec<u8>)> {
    std::vec![
        (1_010_000, noop(0, Some(6_000_000))),
        (2_010_000, upload(0, 1, 300, 300)),
        (3_010_000, activate(1, Some(10_000_000), 1, 0x0C4F)),
        (4_010_000, dump_cmd(2, Some(12_000_000), 0x0001_0000, 600)),
        (5_010_000, set_mode(3, Some(20_000_000), 0)),
        (5_010_000, noop(4, Some(20_000_000))),
        (6_010_000, set_mode(5, Some(24_000_000), 1)),
    ]
}

#[test]
fn the_nominal_timeline_produces_the_telemetry_of_the_scenario() {
    // req: APP-012, APP-013, APP-016, APP-017, APP-019
    let run = replay(&nominal_deliveries(), 35_000_000, None);
    let rec = &run.rec;
    assert_eq!(packets(rec, apid::HOUSEKEEPING).len(), 30);
    // Payload packets at 0.5…18.5 s and 24.5…28.5 s.
    let payload: Vec<u64> = packets(rec, apid::PAYLOAD)
        .iter()
        .map(|p| p.0 / 100_000)
        .collect();
    let expected: Vec<u64> = (0..10).chain(12..15).map(|k| 5 + 20 * k).collect();
    assert_eq!(payload, expected);
    assert_eq!(packets(rec, apid::DUMP).len(), 5);
    assert_eq!(packets(rec, apid::EVENTS).len(), 10);
    let reports: Vec<(u64, Logged)> = log(rec)
        .into_iter()
        .filter(|(_, l)| matches!(l, Logged::Event(..)))
        .collect();
    assert_eq!(
        reports,
        [
            (6_000_000, executed(1, 0)),
            (10_000_000, executed(3, 1)),
            (10_000_000, Logged::Event(0x0010, [1, 300, 0x0C4F])),
            (12_000_000, executed(2, 2)),
            (12_500_000, Logged::Event(0x0020, [1, 0, 600])),
            (20_000_000, executed(4, 3)),
            (20_000_000, Logged::Event(0x0030, [0, 0, 0])),
            (20_000_000, executed(1, 4)),
            (24_000_000, executed(4, 5)),
            (24_000_000, Logged::Event(0x0030, [1, 0, 0])),
        ]
    );
    // Application events: one `cmd_received` per command, one `table_received`.
    let app: Vec<Logged> = events(rec)
        .into_iter()
        .filter(|l| matches!(l, Logged::App(..)))
        .collect();
    assert_eq!(app.len(), 7);
    assert!(app.contains(&Logged::App(21, std::vec![1, 300])));
    assert_eq!(run.node.build_failures(), 0);
    // The status the housekeeping at 30 s would report.
    let status = run.node.status();
    assert_eq!(
        (
            status.commands_executed,
            status.commands_rejected,
            status.queue_occupancy
        ),
        (6, 0, 0)
    );
}

#[test]
fn extra_wakeups_change_nothing_with_commands_and_a_dump() {
    // req: APP-074
    let plain = replay(&nominal_deliveries(), 35_000_000, None);
    let busy = replay(&nominal_deliveries(), 35_000_000, Some(125_000));
    assert_eq!(plain.rec.0, busy.rec.0);
}

#[test]
fn a_packet_that_cannot_be_built_is_counted_and_raised_as_packet_rejected() {
    // req: APP-131
    let mut node = FlightNode::new();
    let mut rec = Rec::default();
    // 2 200 000 000 s after T0 does not fit four coarse octets (TIME-012).
    let far = Instant::from_micros(2_200_000_000_000_000);
    node.report(
        far,
        &Event::CmdExecuted {
            function: 1,
            seq: 0,
        },
        &mut At(7, &mut rec),
    );
    assert_eq!(node.build_failures(), 1);
    assert_eq!(
        rec.0,
        [(
            7,
            Out::Event(6, std::vec![u64::from(ErrorKind::OutOfRange.code())])
        )]
    );
    // The count of the events APID did not advance: the next packet is count 0.
    node.report(
        Instant::from_micros(0),
        &Event::CmdExecuted {
            function: 1,
            seq: 0,
        },
        &mut At(8, &mut rec),
    );
    let (_, bytes) = packets(&rec, apid::EVENTS).pop().unwrap();
    assert_eq!(
        SpacePacket::decode(&bytes).unwrap().sequence_count().get(),
        0
    );
    assert_eq!(node.build_failures(), 1);
}
