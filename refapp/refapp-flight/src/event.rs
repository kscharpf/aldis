//! The event reporter's vocabulary (docs/reference-application.md §3.3.2, APP-023).
//!
//! Event codes are never renumbered or reused (APP-025).

use crate::layout::{EVENT_LEN, PayloadMode};

/// Code of the application event `packet_rejected` (§7.4, APP-131): the argument is the error
/// code of the catalogue in docs/03-api-model.md §9.
pub const APP_PACKET_REJECTED: u16 = 6;
/// Code of the application event `cmd_received` (§7.4, APP-129): function code, command sequence
/// count.
pub const APP_CMD_RECEIVED: u16 = 20;
/// Code of the application event `table_received` (§7.4, APP-129): table ID, length.
pub const APP_TABLE_RECEIVED: u16 = 21;

/// An on-board event, reported as one packet on the event APID at the instant it is raised
/// (§2.1). The command handler, the table store, and the memory dump raise them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `CMD_EXECUTED` (`0x0001`).
    CmdExecuted {
        /// Function code.
        function: u16,
        /// Command sequence count.
        seq: u16,
    },
    /// `CMD_REJECTED` (`0x0002`).
    CmdRejected {
        /// Function code, 0 if unreadable.
        function: u16,
        /// Command sequence count.
        seq: u16,
        /// Reason: 1 unknown function, 2 time tag in the past, 3 malformed arguments, 4
        /// time-tag queue full, 5 dump in progress.
        reason: u16,
    },
    /// `TABLE_ACTIVATED` (`0x0010`).
    TableActivated {
        /// Table ID.
        table_id: u16,
        /// Table length.
        length: u16,
        /// CRC-16 of the table.
        crc: u16,
    },
    /// `TABLE_REJECTED` (`0x0011`).
    TableRejected {
        /// Table ID.
        table_id: u16,
        /// Reason: 1 no table, 2 CRC mismatch.
        reason: u16,
    },
    /// `DUMP_COMPLETED` (`0x0020`).
    DumpCompleted {
        /// Start address of the dump.
        address: u32,
        /// Length of the dumped memory in octets.
        length: u16,
    },
    /// `PAYLOAD_MODE` (`0x0030`).
    PayloadMode {
        /// The new mode.
        mode: PayloadMode,
    },
}

impl Event {
    /// The event code and the parameters p1…p3; unused parameters are 0 (§3.3.2).
    #[must_use]
    pub fn parts(&self) -> (u16, [u16; 3]) {
        match *self {
            Event::CmdExecuted { function, seq } => (0x0001, [function, seq, 0]),
            Event::CmdRejected {
                function,
                seq,
                reason,
            } => (0x0002, [function, seq, reason]),
            Event::TableActivated {
                table_id,
                length,
                crc,
            } => (0x0010, [table_id, length, crc]),
            Event::TableRejected { table_id, reason } => (0x0011, [table_id, reason, 0]),
            Event::DumpCompleted { address, length } => {
                let [a3, a2, a1, a0] = address.to_be_bytes();
                (
                    0x0020,
                    [
                        u16::from_be_bytes([a3, a2]),
                        u16::from_be_bytes([a1, a0]),
                        length,
                    ],
                )
            }
            Event::PayloadMode { mode } => (0x0030, [u16::from(mode.octet()), 0, 0]),
        }
    }

    /// The user data: event code ‖ p1 ‖ p2 ‖ p3, each a big-endian u16 (§3.3.2).
    #[must_use]
    pub fn user_data(&self) -> [u8; EVENT_LEN] {
        let (code, [p1, p2, p3]) = self.parts();
        let [c1, c0] = code.to_be_bytes();
        let [a1, a0] = p1.to_be_bytes();
        let [b1, b0] = p2.to_be_bytes();
        let [d1, d0] = p3.to_be_bytes();
        [c1, c0, a1, a0, b1, b0, d1, d0]
    }
}

#[cfg(test)]
mod tests {
    use super::Event;
    use crate::layout::PayloadMode;

    #[test]
    fn layouts_follow_the_event_table() {
        // req: APP-023
        let cases: [(Event, [u8; 8]); 6] = [
            (
                Event::CmdExecuted {
                    function: 1,
                    seq: 0,
                },
                [0, 1, 0, 1, 0, 0, 0, 0],
            ),
            (
                Event::CmdRejected {
                    function: 127,
                    seq: 1,
                    reason: 1,
                },
                [0, 2, 0, 127, 0, 1, 0, 1],
            ),
            (
                Event::TableActivated {
                    table_id: 1,
                    length: 300,
                    crc: 0x0C4F,
                },
                [0, 0x10, 0, 1, 0x01, 0x2C, 0x0C, 0x4F],
            ),
            (
                Event::TableRejected {
                    table_id: 1,
                    reason: 2,
                },
                [0, 0x11, 0, 1, 0, 2, 0, 0],
            ),
            (
                Event::DumpCompleted {
                    address: 0x0001_0000,
                    length: 600,
                },
                [0, 0x20, 0, 1, 0, 0, 0x02, 0x58],
            ),
            (
                Event::PayloadMode {
                    mode: PayloadMode::On,
                },
                [0, 0x30, 0, 1, 0, 0, 0, 0],
            ),
        ];
        for (event, octets) in cases {
            assert_eq!(event.user_data(), octets, "{event:?}");
        }
    }
}
