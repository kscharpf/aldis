//! Command packets, parsed (docs/reference-application.md §2.3, §3.3.5; APP-012, APP-013,
//! APP-024).
//!
//! [`parse`] turns the user data of a command packet into a [`Command`] once, where it enters the
//! node (RS-020). A command whose function code is unknown or whose arguments do not fit its
//! function is still a command: it carries the [`Reject`] that its execution reports, because
//! the checks of function code and arguments happen at execution (§2.3).

use aldis::core::ids::SeqCount;
use aldis::time::{CucEpoch, CucFormat, TimeCode, TimeFormat, decode_implicit};
use refapp_scenario::generated::time;

use crate::layout::PayloadMode;

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

/// Octets before the arguments: function code, execution mode, and the 6-octet T-field (§3.3.5).
const HEADER_LEN: usize = 8;

/// Longest dump a command may ask for, octets (§3.3.4, §3.3.5).
pub const DUMP_MAX_LENGTH: u16 = 1_024;

/// Why a command is rejected: the `CMD_REJECTED` reason of §3.3.2 (APP-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// 1: no function has this code.
    UnknownFunction,
    /// 2: the time tag is earlier than the delivery instant (§2.3).
    TimeTagPast,
    /// 3: the arguments or the length do not match the function code (APP-024).
    Malformed,
    /// 4: the time-tag queue is full (§2.3).
    QueueFull,
    /// 5: a dump is in progress (APP-016).
    DumpInProgress,
}

impl Reject {
    /// The value of `p3` of `CMD_REJECTED` (§3.3.2).
    #[must_use]
    pub const fn reason(self) -> u16 {
        match self {
            Reject::UnknownFunction => 1,
            Reject::TimeTagPast => 2,
            Reject::Malformed => 3,
            Reject::QueueFull => 4,
            Reject::DumpInProgress => 5,
        }
    }
}

/// The memory a `DUMP_MEMORY` command names: at least 1 and at most [`DUMP_MAX_LENGTH`] octets,
/// none beyond address 2³² − 1 (§3.3.4, §3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumpRegion {
    address: u32,
    length: u16,
}

impl DumpRegion {
    /// The region, or `None` for a length outside 1…1 024 or one that runs past the end of the
    /// address space.
    #[must_use]
    pub fn new(address: u32, length: u16) -> Option<DumpRegion> {
        let last = u32::from(length).checked_sub(1)?;
        if length > DUMP_MAX_LENGTH {
            return None;
        }
        address.checked_add(last)?;
        Some(DumpRegion { address, length })
    }

    /// First address.
    #[must_use]
    pub const fn address(self) -> u32 {
        self.address
    }

    /// Length, octets.
    #[must_use]
    pub const fn length(self) -> u16 {
        self.length
    }
}

/// A function of §3.3.5 with arguments that fit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `NOOP`.
    Noop,
    /// `DUMP_MEMORY`.
    DumpMemory(DumpRegion),
    /// `ACTIVATE_TABLE`.
    ActivateTable {
        /// Table ID.
        table_id: u8,
        /// CRC-16 of the table that the command claims (CMN-001).
        crc: u16,
    },
    /// `SET_PAYLOAD_MODE`.
    SetPayloadMode(PayloadMode),
}

/// When a command executes (§2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// When it is delivered.
    Immediate,
    /// At the instant that many µs after the start of the run (T0 + `at`, §5).
    At(u64),
    /// A time tag before T0, earlier than any instant of the run.
    BeforeRun,
}

/// A command packet of §3.3.5, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// Function code, 0 if the packet has none (§3.3.2).
    pub function: u16,
    /// Command sequence count (§3.3.5).
    pub seq: u16,
    /// When it executes.
    pub timing: Timing,
    /// The function, or the reason that executing it is rejected.
    pub call: Result<Call, Reject>,
}

/// Parses the user data of the command packet with sequence count `seq` (§3.3.5; APP-024).
///
/// Nothing here fails: a command the node cannot understand is a command that executes as a
/// rejection (§2.3). User data without a function code, execution mode, and T-field, an
/// execution mode other than 0 and 1, a non-zero time for an immediate command, and an
/// undecodable time tag are `Malformed`, and such a command executes when delivered because
/// the node cannot tell when it was meant to execute.
#[must_use]
pub fn parse(seq: SeqCount, data: &[u8]) -> Command {
    let seq = seq.get();
    let (Some(&code), Some(&mode), Some(t_field), Some(args)) = (
        data.first(),
        data.get(1),
        data.get(2..HEADER_LEN),
        data.get(HEADER_LEN..),
    ) else {
        return Command {
            function: data.first().copied().map_or(0, u16::from),
            seq,
            timing: Timing::Immediate,
            call: Err(Reject::Malformed),
        };
    };
    let function = u16::from(code);
    match timing(mode, t_field) {
        Some(timing) => Command {
            function,
            seq,
            timing,
            call: call(code, args),
        },
        None => Command {
            function,
            seq,
            timing: Timing::Immediate,
            call: Err(Reject::Malformed),
        },
    }
}

/// The execution mode and T-field as a [`Timing`]; `None` if they are not valid (§3.3.5).
fn timing(mode: u8, t_field: &[u8]) -> Option<Timing> {
    match mode {
        MODE_IMMEDIATE => t_field.iter().all(|o| *o == 0).then_some(Timing::Immediate),
        MODE_TIME_TAGGED => time_tag(t_field),
        _ => None,
    }
}

/// The time tag in `t_field`, a CUC T-field of §5.
fn time_tag(t_field: &[u8]) -> Option<Timing> {
    let format = CucFormat::new(
        CucEpoch::Tai1958,
        time::CUC_COARSE_OCTETS,
        time::CUC_FINE_OCTETS,
    )
    .ok()?;
    let decoded = decode_implicit(t_field, &TimeFormat::Cuc(format)).ok()?;
    let TimeCode::Cuc(cuc) = decoded.code() else {
        return None;
    };
    let (seconds, nanos) = cuc.to_seconds_nanos();
    let Some(after_t0) = seconds.checked_sub(u64::from(time::T0_COARSE_S)) else {
        return Some(Timing::BeforeRun);
    };
    // The fraction is truncated to whole µs; every execution time of the scenario is a multiple
    // of 125 000 µs, which two fractional octets hold exactly (APP-061).
    let micros = after_t0
        .checked_mul(1_000_000)?
        .checked_add(u64::from(nanos.checked_div(1_000)?))?;
    Some(Timing::At(micros))
}

/// The function with code `code` and its `args` (§3.3.5).
fn call(code: u8, args: &[u8]) -> Result<Call, Reject> {
    let malformed = Reject::Malformed;
    match code {
        FN_NOOP => args.is_empty().then_some(Call::Noop).ok_or(malformed),
        FN_DUMP_MEMORY => {
            let &[a3, a2, a1, a0, l1, l0] = <&[u8; 6]>::try_from(args).map_err(|_| malformed)?;
            DumpRegion::new(
                u32::from_be_bytes([a3, a2, a1, a0]),
                u16::from_be_bytes([l1, l0]),
            )
            .map(Call::DumpMemory)
            .ok_or(malformed)
        }
        FN_ACTIVATE_TABLE => {
            let &[table_id, c1, c0] = <&[u8; 3]>::try_from(args).map_err(|_| malformed)?;
            Ok(Call::ActivateTable {
                table_id,
                crc: u16::from_be_bytes([c1, c0]),
            })
        }
        FN_SET_PAYLOAD_MODE => match args {
            [0] => Ok(Call::SetPayloadMode(PayloadMode::Off)),
            [1] => Ok(Call::SetPayloadMode(PayloadMode::On)),
            _ => Err(malformed),
        },
        _ => Err(Reject::UnknownFunction),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use aldis::core::ids::SeqCount;

    use super::{Call, Command, DumpRegion, Reject, Timing, parse};
    use crate::layout::PayloadMode;

    /// A command's user data: function, mode, T-field, arguments.
    fn data(function: u8, mode: u8, t_field: [u8; 6], args: &[u8]) -> std::vec::Vec<u8> {
        let mut v = std::vec![function, mode];
        v.extend_from_slice(&t_field);
        v.extend_from_slice(args);
        v
    }

    fn seq(n: u16) -> SeqCount {
        SeqCount::new(n).unwrap()
    }

    fn call_of(function: u8, args: &[u8]) -> Result<Call, Reject> {
        parse(seq(0), &data(function, 0, [0; 6], args)).call
    }

    #[test]
    fn functions_and_arguments_of_the_command_table() {
        // req: APP-024
        assert_eq!(call_of(1, &[]), Ok(Call::Noop));
        assert_eq!(
            call_of(2, &[0, 1, 0, 0, 0x02, 0x58]),
            Ok(Call::DumpMemory(DumpRegion::new(0x0001_0000, 600).unwrap()))
        );
        assert_eq!(
            call_of(3, &[1, 0x0C, 0x4F]),
            Ok(Call::ActivateTable {
                table_id: 1,
                crc: 0x0C4F
            })
        );
        assert_eq!(call_of(4, &[0]), Ok(Call::SetPayloadMode(PayloadMode::Off)));
        assert_eq!(call_of(4, &[1]), Ok(Call::SetPayloadMode(PayloadMode::On)));
    }

    #[test]
    fn a_length_that_does_not_match_the_function_is_malformed() {
        // req: APP-024
        for (function, args) in [
            (1, &[0_u8][..]),
            (2, &[0, 0, 0, 0, 0]),
            (2, &[0, 0, 0, 0, 0, 1, 0]),
            (2, &[0, 0, 0, 0, 0, 0]),
            (3, &[1, 0]),
            (3, &[]),
            (4, &[]),
            (4, &[1, 0]),
            (4, &[2]),
        ] {
            assert_eq!(
                call_of(function, args),
                Err(Reject::Malformed),
                "{function} {args:?}"
            );
        }
    }

    #[test]
    fn an_unknown_function_code_is_rejected_whatever_its_arguments() {
        // req: APP-013, APP-129
        for code in [0, 5, 127, 255] {
            assert_eq!(call_of(code, &[]), Err(Reject::UnknownFunction));
            assert_eq!(call_of(code, &[9, 9]), Err(Reject::UnknownFunction));
        }
    }

    #[test]
    fn dump_regions_are_one_to_1024_octets_inside_the_address_space() {
        // req: APP-016
        assert!(DumpRegion::new(0, 1).is_some());
        assert!(DumpRegion::new(0, 1_024).is_some());
        assert!(DumpRegion::new(0, 0).is_none());
        assert!(DumpRegion::new(0, 1_025).is_none());
        assert!(DumpRegion::new(u32::MAX, 1).is_some());
        assert!(DumpRegion::new(u32::MAX, 2).is_none());
        assert!(DumpRegion::new(u32::MAX - 1_023, 1_024).is_some());
        assert!(DumpRegion::new(u32::MAX - 1_022, 1_024).is_none());
    }

    #[test]
    fn time_tags_are_micros_after_t0() {
        // req: APP-012, APP-060
        // T0 = 2 100 000 000 s = 0x7D2B7500; 12 s, fraction 0.
        let at = |t_field| parse(seq(0), &data(1, 1, t_field, &[])).timing;
        assert_eq!(at([0x7D, 0x2B, 0x75, 0x0C, 0, 0]), Timing::At(12_000_000));
        // 12.125 s: fraction 0x2000 / 65 536 = 0.125.
        assert_eq!(
            at([0x7D, 0x2B, 0x75, 0x0C, 0x20, 0]),
            Timing::At(12_125_000)
        );
        assert_eq!(at([0x7D, 0x2B, 0x75, 0, 0, 0]), Timing::At(0));
        // Before T0 is before the run.
        assert_eq!(at([0x7D, 0x2B, 0x74, 0xFF, 0, 0]), Timing::BeforeRun);
        assert_eq!(at([0, 0, 0, 0, 0, 0]), Timing::BeforeRun);
    }

    #[test]
    fn a_command_without_a_valid_timing_is_malformed_and_immediate() {
        // req: APP-024
        let rejected = |c: Command| (c.timing, c.call);
        let bad = (Timing::Immediate, Err(Reject::Malformed));
        // Too short for the header, with and without a function code.
        assert_eq!(rejected(parse(seq(3), &[])), bad);
        assert_eq!(parse(seq(3), &[]).function, 0);
        assert_eq!(rejected(parse(seq(3), &[1, 0, 0])), bad);
        assert_eq!(parse(seq(3), &[1, 0, 0]).function, 1);
        // An execution mode that is neither 0 nor 1.
        assert_eq!(rejected(parse(seq(3), &data(1, 2, [0; 6], &[]))), bad);
        // A non-zero time for an immediate command.
        assert_eq!(
            rejected(parse(seq(3), &data(1, 0, [0, 0, 0, 0, 0, 1], &[]))),
            bad
        );
        assert_eq!(parse(seq(3), &data(1, 0, [0; 6], &[])).seq, 3);
    }
}
