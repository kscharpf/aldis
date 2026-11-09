//! The command handler, the table store, and the memory dump as steps of the flight node
//! (docs/reference-application.md §2.1 steps 1–3, §2.3, §3.3.5, §3.3.6; APP-012, APP-013,
//! APP-016, APP-017, APP-024, APP-129).

use aldis::core::ErrorKind;
use aldis::spp::{PacketType, SequenceFlags, SpacePacket};
use refapp_scenario::generated::apid;
use refapp_scenario::{Delivered, Instant, Output};

use crate::FlightNode;
use crate::command::{Call, Command, Reject, Timing, parse};
use crate::dump::Dump;
use crate::event::{APP_CMD_RECEIVED, APP_PACKET_REJECTED, APP_TABLE_RECEIVED, Event};
use crate::table::Activation;

impl FlightNode {
    /// Step 1 of §2.1: handles the delivered link units in delivery order. An immediate command
    /// executes here.
    pub(crate) fn handle_delivered(
        &mut self,
        now: Instant,
        units: Delivered<'_>,
        out: &mut dyn Output,
    ) {
        for unit in units {
            self.handle_unit(now, unit, out);
        }
    }

    /// One uplink unit. At packet level it is a Space Packet ([`FlightNode::handle_packet`]); at
    /// frame level (A2 on) it is a TC frame, and the packets it delivers are handled in the same
    /// way (APP-139).
    fn handle_unit(&mut self, now: Instant, unit: &[u8], out: &mut dyn Output) {
        let Some(mut uplink) = self.uplink.take() else {
            return self.handle_packet(now, unit, out);
        };
        uplink.accept(now, unit, out, &mut |packet, out| {
            self.handle_packet(now, packet, out);
        });
        self.uplink = Some(uplink);
    }

    /// One packet: exactly one Space Packet of Packet Type 1 on the commands or the
    /// table-upload APID, unsegmented (§3.1). Anything else is `packet_rejected` (APP-131).
    fn handle_packet(&mut self, now: Instant, unit: &[u8], out: &mut dyn Output) {
        let packet = match SpacePacket::decode_exact(unit) {
            Ok(packet) => packet,
            Err(e) => return reject_unit(out, e.kind()),
        };
        if packet.packet_type() != PacketType::Telecommand
            || packet.sequence_flags() != SequenceFlags::Unsegmented
            || packet.has_secondary_header()
        {
            return reject_unit(out, ErrorKind::InvalidField);
        }
        let data = packet.data_field();
        match packet.apid().get() {
            apid::COMMANDS => self.deliver_command(now, parse(packet.sequence_count(), data), out),
            apid::TABLE_UPLOAD => self.deliver_table(data, out),
            _ => reject_unit(out, ErrorKind::UnknownChannel),
        }
    }

    /// A command packet is delivered: `cmd_received`, then an immediate command executes, a
    /// time tag in the past or a full queue rejects it, and any other time-tagged command is
    /// queued (§2.3, APP-012, APP-013, APP-129).
    fn deliver_command(&mut self, now: Instant, command: Command, out: &mut dyn Output) {
        out.app_event(
            APP_CMD_RECEIVED,
            &[u64::from(command.function), u64::from(command.seq)],
        );
        match command.timing {
            Timing::Immediate => self.execute(now, command, out),
            Timing::BeforeRun => self.reject(now, &command, Reject::TimeTagPast, out),
            Timing::At(at) if at < now => {
                self.reject(now, &command, Reject::TimeTagPast, out);
            }
            Timing::At(at) => {
                if let Err(reason) = self.queue.push(at, command) {
                    self.reject(now, &command, reason, out);
                }
            }
        }
    }

    /// A table-upload packet is delivered: the table is stored and `table_received` raised, or
    /// the upload is `packet_rejected` (§3.3.6, APP-017, APP-129).
    fn deliver_table(&mut self, data: &[u8], out: &mut dyn Output) {
        match self.table.store(data) {
            Ok((table_id, length)) => {
                out.app_event(
                    APP_TABLE_RECEIVED,
                    &[u64::from(table_id.get()), u64::from(length.get())],
                );
            }
            Err(e) => reject_unit(out, e.kind()),
        }
    }

    /// Step 2 of §2.1: executes the time-tagged commands due at `now`, in queue order, and
    /// brings the queue occupancy of the housekeeping status up to date (APP-012).
    pub(crate) fn execute_due(&mut self, now: Instant, out: &mut dyn Output) {
        // bound: each pass removes one command from a queue of at most 8.
        while let Some(command) = self.queue.pop_due(now) {
            self.execute(now, command, out);
        }
        self.status.queue_occupancy = self.queue.occupancy();
    }

    /// Executes `command` at `now`: one verification report, `CMD_EXECUTED` followed by the
    /// events of its effect, or `CMD_REJECTED` (APP-013, §3.3.2).
    fn execute(&mut self, now: Instant, command: Command, out: &mut dyn Output) {
        let call = match command.call {
            Ok(call) => call,
            Err(reason) => return self.reject(now, &command, reason, out),
        };
        // A dump already running rejects the command before it counts as executed (APP-016).
        if matches!(call, Call::DumpMemory(_)) && self.dump.is_some() {
            return self.reject(now, &command, Reject::DumpInProgress, out);
        }
        self.status.commands_executed = self.status.commands_executed.wrapping_add(1);
        self.report(
            now,
            &Event::CmdExecuted {
                function: command.function,
                seq: command.seq,
            },
            out,
        );
        match call {
            Call::Noop => {}
            Call::DumpMemory(region) => match Dump::start(region, now) {
                Ok(dump) => self.dump = Some(dump),
                Err(e) => self.build_failed(e.kind(), out),
            },
            Call::ActivateTable { table_id, crc } => {
                let event = match self.table.activate(table_id, crc) {
                    Activation::NoTable => Event::TableRejected {
                        table_id: u16::from(table_id),
                        reason: 1,
                    },
                    Activation::CrcMismatch => Event::TableRejected {
                        table_id: u16::from(table_id),
                        reason: 2,
                    },
                    Activation::Activated { length, crc } => {
                        self.status.table_id = table_id;
                        self.status.table_crc = crc.get();
                        Event::TableActivated {
                            table_id: u16::from(table_id),
                            length: length.get(),
                            crc: crc.get(),
                        }
                    }
                };
                self.report(now, &event, out);
            }
            Call::SetPayloadMode(mode) => {
                self.status.payload_mode = mode;
                self.report(now, &Event::PayloadMode { mode }, out);
            }
        }
    }

    /// Reports `command` as `CMD_REJECTED` for `reason` at `now` (APP-013).
    fn reject(&mut self, now: Instant, command: &Command, reason: Reject, out: &mut dyn Output) {
        self.status.commands_rejected = self.status.commands_rejected.wrapping_add(1);
        self.report(
            now,
            &Event::CmdRejected {
                function: command.function,
                seq: command.seq,
                reason: reason.reason(),
            },
            out,
        );
    }

    /// Step 3 of §2.1: sends the dump packets due at `now`, one per 125 000 µs slot, each stamped
    /// with its slot instant, and raises `DUMP_COMPLETED` after the last one (APP-016).
    pub(crate) fn dump_segments(&mut self, now: Instant, out: &mut dyn Output) {
        let Some(mut dump) = self.dump.take() else {
            return;
        };
        // bound: each pass sends one of the at most 9 packets of the dump and moves its next slot
        // on; the pass ends the loop on the last packet, on an error, and at a slot after `now`.
        loop {
            if dump.due() > now {
                self.dump = Some(dump);
                return;
            }
            match dump.send(&mut self.downlink, out) {
                Ok(segment) if segment.last => {
                    self.status.dumps_completed = self.status.dumps_completed.wrapping_add(1);
                    let region = dump.region();
                    self.report(
                        segment.at,
                        &Event::DumpCompleted {
                            address: region.address(),
                            length: region.length(),
                        },
                        out,
                    );
                    return;
                }
                Ok(_) => {}
                // The dump is abandoned: a packet that cannot be built cannot be built again.
                Err(e) => return self.build_failed(e.kind(), out),
            }
        }
    }

    /// The instant of the next queued command or dump packet, µs.
    pub(crate) fn next_command_deadline(&self) -> [Option<Instant>; 2] {
        [self.queue.next_due(), self.dump.as_ref().map(Dump::due)]
    }

    /// A packet could not be built: counted, and raised as `packet_rejected` with the error code
    /// (APP-131). No packet is output, so the event cannot raise itself.
    pub(crate) fn build_failed(&mut self, kind: ErrorKind, out: &mut dyn Output) {
        self.build_failures = self.build_failures.saturating_add(1);
        reject_unit(out, kind);
    }
}

/// `packet_rejected` with the catalogue code of `kind` (§7.4, APP-131).
fn reject_unit(out: &mut dyn Output, kind: ErrorKind) {
    out.app_event(APP_PACKET_REJECTED, &[u64::from(kind.code())]);
}
