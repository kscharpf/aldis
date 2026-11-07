//! The ops script: the command and table-upload packets the ground node releases.
//!
//! Specification: `docs/reference-application.md` §2.2, §3.3.5, §3.3.6, §6.5; APP-011, APP-024,
//! APP-126.
//!
//! An entry holds parameters, not octets. [`Script::new`] builds every packet once, when the node
//! is made, so a script that cannot be encoded is an error there and a step cannot fail
//! (RS-020).

use aldis::core::ids::{Apid, SeqCount};
use aldis::core::{Error, ErrorKind};
use aldis::spp::{PacketFields, PacketType, SequenceCounter, SequenceFlags, SppConfig, encode};
use aldis::time::{Cuc, CucEpoch, CucFormat, TimeCode};
use refapp_scenario::generated::{apid, tc, time};
use refapp_scenario::{Crc16, TableId, TableLength};

/// Microseconds in a second.
const US_PER_S: u64 = 1_000_000;

/// Nanoseconds in a microsecond.
const NS_PER_US: u32 = 1_000;

/// Function code of `NOOP` (§3.3.5).
const FN_NOOP: u8 = 0x01;
/// Function code of `DUMP_MEMORY` (§3.3.5).
const FN_DUMP_MEMORY: u8 = 0x02;
/// Function code of `ACTIVATE_TABLE` (§3.3.5).
const FN_ACTIVATE_TABLE: u8 = 0x03;
/// Function code of `SET_PAYLOAD_MODE` (§3.3.5).
const FN_SET_PAYLOAD_MODE: u8 = 0x04;

/// Execution mode octet of an immediate command (§3.3.5).
const MODE_IMMEDIATE: u8 = 0x00;
/// Execution mode octet of a time-tagged command (§3.3.5).
const MODE_TIME_TAGGED: u8 = 0x01;

/// Octets of a CUC T-field of the scenario: 4 coarse and 2 fractional (§5).
const T_FIELD_LEN: usize = 6;

/// Execution mode and time of a command (§2.3, §3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    /// Executes when delivered; the execution-time field is zero.
    Immediate,
    /// Executes at the on-board time T0 + `exec_us`.
    TimeTagged {
        /// Execution instant, µs after T0.
        exec_us: u64,
    },
}

/// Function and arguments of a command (§3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `NOOP`.
    Noop,
    /// `DUMP_MEMORY`.
    DumpMemory {
        /// First address of the dump.
        address: u32,
        /// Length, octets.
        length: u16,
    },
    /// `ACTIVATE_TABLE`.
    ActivateTable {
        /// Table ID.
        table_id: u8,
        /// CRC-16 of the table that the command claims (CMN-001).
        crc: Crc16,
    },
    /// `SET_PAYLOAD_MODE`.
    SetPayloadMode {
        /// New mode: 0 off, 1 on.
        mode: u8,
    },
    /// A function code that no function has.
    Unknown {
        /// The code.
        code: u8,
    },
}

/// One entry of the ops script (§6.5), as the scenario file states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpsEntry {
    /// A command packet on the commands APID (§3.3.5).
    Command {
        /// Execution mode and time.
        execution: Execution,
        /// Function and arguments.
        call: Call,
        /// Zero octets appended after the arguments (APP-024); 0 for none.
        extra_octets: u8,
    },
    /// A table-upload packet on the table-upload APID (§3.3.6).
    TableUpload {
        /// Table ID.
        table_id: TableId,
        /// Table length, octets.
        length: TableLength,
    },
    /// A FOP-1 directive: issued from A3 on, ignored at A1 (APP-019).
    Fop1Directive,
}

/// A packet of the script, built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Built {
    /// The APID the packet is sent on.
    pub(crate) apid: Apid,
    /// The packet octets.
    pub(crate) octets: Vec<u8>,
}

/// Why the script cannot be built: the entry and the library's error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptError {
    /// Index of the entry in the script.
    pub index: usize,
    /// What the encoder reported.
    pub error: Error,
}

/// The ops script with its packets built, in script order (APP-024, APP-126).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Script {
    /// One slot per entry; `None` for a directive this link level ignores.
    packets: Vec<Option<Built>>,
}

impl Script {
    /// Builds the packet of every entry. Command sequence counts follow the release order of the
    /// `command` entries, and table uploads have their own count (§6.5, §3.1, APP-021).
    ///
    /// # Errors
    /// The first entry whose packet cannot be encoded (a time or length out of range).
    pub(crate) fn new(entries: &[OpsEntry]) -> Result<Script, ScriptError> {
        let mut commands = SequenceCounter::default();
        let mut tables = SequenceCounter::default();
        let mut packets = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let built = match *entry {
                OpsEntry::Command {
                    execution,
                    call,
                    extra_octets,
                } => command_data(execution, call, extra_octets)
                    .and_then(|data| build(apid::COMMANDS, commands.advance(), &data))
                    .map(Some),
                OpsEntry::TableUpload { table_id, length } => build(
                    apid::TABLE_UPLOAD,
                    tables.advance(),
                    &table_data(table_id, length),
                )
                .map(Some),
                OpsEntry::Fop1Directive => Ok(None),
            };
            packets.push(built.map_err(|error| ScriptError { index, error })?);
        }
        Ok(Script { packets })
    }

    /// The packet of entry `index`; `None` if the entry sends nothing at A1 or does not exist.
    pub(crate) fn packet(&self, index: usize) -> Option<&Built> {
        self.packets.get(index).and_then(Option::as_ref)
    }
}

/// The user data of a command packet (§3.3.5): function code, execution mode, execution time,
/// arguments, and `extra` zero octets.
fn command_data(execution: Execution, call: Call, extra: u8) -> Result<Vec<u8>, Error> {
    let (function, args): (u8, Vec<u8>) = match call {
        Call::Noop => (FN_NOOP, Vec::new()),
        Call::DumpMemory { address, length } => (
            FN_DUMP_MEMORY,
            address
                .to_be_bytes()
                .into_iter()
                .chain(length.to_be_bytes())
                .collect(),
        ),
        Call::ActivateTable { table_id, crc } => (
            FN_ACTIVATE_TABLE,
            core::iter::once(table_id)
                .chain(crc.get().to_be_bytes())
                .collect(),
        ),
        Call::SetPayloadMode { mode } => (FN_SET_PAYLOAD_MODE, vec![mode]),
        Call::Unknown { code } => (code, Vec::new()),
    };
    let (mode, time_field) = match execution {
        Execution::Immediate => (MODE_IMMEDIATE, [0; T_FIELD_LEN]),
        Execution::TimeTagged { exec_us } => (MODE_TIME_TAGGED, on_board_time(exec_us)?),
    };
    let mut data = vec![function, mode];
    data.extend_from_slice(&time_field);
    data.extend_from_slice(&args);
    data.resize(data.len().saturating_add(usize::from(extra)), 0);
    Ok(data)
}

/// The user data of a table-upload packet (§3.3.6): table ID, length, and table octet
/// `i = (3 · i + 1) mod 256`.
fn table_data(table_id: TableId, length: TableLength) -> Vec<u8> {
    let length = length.get();
    let mut data = vec![table_id.get()];
    data.extend_from_slice(&length.to_be_bytes());
    // Only `i mod 256` matters to `(3 · i + 1) mod 256`, and the low octet of `i` is that.
    data.extend((0..length).map(|i| i.to_be_bytes()[1].wrapping_mul(3).wrapping_add(1)));
    data
}

/// The CUC T-field of the on-board time T0 + `offset_us` (§5, APP-060, TIME-011).
///
/// # Errors
/// `OutOfRange` if the time does not fit 4 coarse octets.
fn on_board_time(offset_us: u64) -> Result<[u8; T_FIELD_LEN], Error> {
    let format = CucFormat::new(
        CucEpoch::Tai1958,
        time::CUC_COARSE_OCTETS,
        time::CUC_FINE_OCTETS,
    )?;
    let whole = offset_us / US_PER_S;
    let seconds = u64::from(time::T0_COARSE_S).saturating_add(whole);
    let micros =
        u32::try_from(offset_us % US_PER_S).map_err(|_| Error::new(ErrorKind::OutOfRange))?;
    let cuc = Cuc::from_seconds_nanos(format, seconds, micros.saturating_mul(NS_PER_US))?;
    let mut octets = [0_u8; T_FIELD_LEN];
    TimeCode::Cuc(cuc).encode(false, &mut octets)?;
    Ok(octets)
}

/// Encodes a telecommand packet of `apid` with `count` and the user data `data`, no secondary
/// header, Sequence Flags `11` (§3.1).
fn build(apid: u16, count: SeqCount, data: &[u8]) -> Result<Built, Error> {
    // A constant APID still needs an error path (api-feedback #304).
    let apid = Apid::new(apid).ok_or(Error::new(ErrorKind::InvalidField))?;
    let fields = PacketFields {
        packet_type: PacketType::Telecommand,
        apid,
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: count,
    };
    let packet = SppConfig::new(usize::from(tc::MAX_PACKET_LENGTH))?.packet(fields, &[], data)?;
    let mut octets = vec![0; packet.packet_len()];
    let len = encode(&packet, &mut octets)?;
    octets.truncate(len);
    Ok(Built { apid, octets })
}

#[cfg(test)]
mod tests {
    use super::{Call, Execution, OpsEntry, Script};
    use refapp_scenario::generated::apid;
    use refapp_scenario::{Crc16, TableId, TableLength};

    fn octets(entry: OpsEntry) -> Vec<u8> {
        let script = Script::new(&[entry]).unwrap();
        script.packet(0).unwrap().octets.clone()
    }

    #[test]
    fn time_tagged_noop_is_the_layout_of_section_3_3_5() {
        // req: APP-024
        // Execution 6.0 s after T0 = 2 100 000 006 s = 0x7D2B7506, fraction 0.
        let p = octets(OpsEntry::Command {
            execution: Execution::TimeTagged { exec_us: 6_000_000 },
            call: Call::Noop,
            extra_octets: 0,
        });
        assert_eq!(p.len(), 14, "§3.3.5: NOOP packet length 14");
        // Version 000, type 1 (TC), no secondary header, APID 128; flags 11, count 0;
        // data length 14 - 6 - 1 = 7.
        assert_eq!(p[..6], [0x10, 0x80, 0xC0, 0x00, 0x00, 0x07]);
        assert_eq!(p[6..], [0x01, 0x01, 0x7D, 0x2B, 0x75, 0x06, 0x00, 0x00]);
    }

    #[test]
    fn command_layouts_and_lengths() {
        // req: APP-024
        let immediate = |call| OpsEntry::Command {
            execution: Execution::Immediate,
            call,
            extra_octets: 0,
        };
        let dump = octets(immediate(Call::DumpMemory {
            address: 0x0001_0000,
            length: 600,
        }));
        assert_eq!(dump.len(), 20);
        assert_eq!(dump[6..8], [0x02, 0x00]);
        assert_eq!(dump[8..14], [0; 6], "immediate: zero execution time");
        assert_eq!(dump[14..], [0x00, 0x01, 0x00, 0x00, 0x02, 0x58]);
        let act = octets(immediate(Call::ActivateTable {
            table_id: 1,
            crc: Crc16::from_wire(0x0C4F),
        }));
        assert_eq!((act.len(), act[6]), (17, 0x03));
        assert_eq!(act[14..], [0x01, 0x0C, 0x4F]);
        let mode = octets(immediate(Call::SetPayloadMode { mode: 1 }));
        assert_eq!((mode.len(), mode[6], mode[14]), (15, 0x04, 1));
        let unknown = octets(immediate(Call::Unknown { code: 127 }));
        assert_eq!((unknown.len(), unknown[6]), (14, 127));
    }

    #[test]
    fn extra_octets_break_the_length_of_the_function() {
        // req: APP-024
        let p = octets(OpsEntry::Command {
            execution: Execution::Immediate,
            call: Call::Noop,
            extra_octets: 1,
        });
        assert_eq!(p.len(), 15, "NOOP is 14 octets; one extra octet");
        assert_eq!(p[14], 0);
    }

    #[test]
    fn table_upload_layout() {
        // req: APP-024
        let p = octets(OpsEntry::TableUpload {
            table_id: TableId::new(1).unwrap(),
            length: TableLength::new(300).unwrap(),
        });
        assert_eq!(p.len(), 309, "§3.3.6: the nominal table packet");
        // Type 1 (TC), APID 129 = 0x081; flags 11; count 0; data length 303 - 1 = 302.
        assert_eq!(p[..6], [0x10, 0x81, 0xC0, 0x00, 0x01, 0x2E]);
        assert_eq!(p[6..9], [1, 0x01, 0x2C]);
        // Octet i = (3 i + 1) mod 256: 1, 4, 7, ... and i = 255 wraps (766 mod 256 = 254).
        assert_eq!(p[9..12], [1, 4, 7]);
        assert_eq!(p[9 + 255], 254);
        assert_eq!(p[9 + 299], 130, "3 · 299 + 1 = 898, mod 256 = 130");
    }

    #[test]
    fn sequence_counts_follow_release_order_per_apid() {
        // req: APP-021
        let noop = OpsEntry::Command {
            execution: Execution::Immediate,
            call: Call::Noop,
            extra_octets: 0,
        };
        let table = OpsEntry::TableUpload {
            table_id: TableId::new(1).unwrap(),
            length: TableLength::new(1).unwrap(),
        };
        let script = Script::new(&[noop, table, noop, OpsEntry::Fop1Directive, table]).unwrap();
        let counts: Vec<_> = (0..5)
            .map(|i| {
                script.packet(i).map(|b| {
                    (
                        b.apid.get(),
                        u16::from(b.octets[2] & 0x3F) << 8 | u16::from(b.octets[3]),
                    )
                })
            })
            .collect();
        assert_eq!(
            counts,
            [
                Some((apid::COMMANDS, 0)),
                Some((apid::TABLE_UPLOAD, 0)),
                Some((apid::COMMANDS, 1)),
                None,
                Some((apid::TABLE_UPLOAD, 1)),
            ]
        );
        assert!(script.packet(9).is_none());
    }

    #[test]
    fn a_time_that_does_not_fit_is_an_error_naming_the_entry() {
        // req: APP-126
        let too_late = OpsEntry::Command {
            execution: Execution::TimeTagged {
                exec_us: u64::MAX / 2,
            },
            call: Call::Noop,
            extra_octets: 0,
        };
        let noop = OpsEntry::Command {
            execution: Execution::Immediate,
            call: Call::Noop,
            extra_octets: 0,
        };
        let e = Script::new(&[noop, too_late]).unwrap_err();
        assert_eq!(e.index, 1);
    }
}
