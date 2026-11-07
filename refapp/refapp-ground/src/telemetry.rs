//! Telemetry reception: strict decoding, sequence tracking, time stamps, and dump reassembly.
//!
//! Specification: `docs/reference-application.md` §2.2, §3.1, §3.2, §3.3.4, §6.6, §7.4; APP-011,
//! APP-018, APP-022, APP-130.

use aldis::core::counter::Continuity;
use aldis::core::crc::crc16;
use aldis::core::ids::Apid;
use aldis::core::{Error, ErrorKind};
use aldis::spp::{
    PacketType, ReassemblerState, SecondaryHeaderLayout, SequenceFlags, SequenceTracker,
    SpacePacket, SpacePacketHeader, Status, TimeHeaderLayout, Tracked,
};
use aldis::time::{CucEpoch, CucFormat, TimeCode, TimeFormat};
use refapp_scenario::Output;
use refapp_scenario::generated::{apid, time};

/// Event code `seq_gap` (§7.4).
pub const SEQ_GAP: u16 = 1;
/// Event code `seq_repeat` (§7.4).
pub const SEQ_REPEAT: u16 = 2;
/// Event code `seq_reset` (§7.4).
pub const SEQ_RESET: u16 = 3;
/// Event code `dump_reassembled` (§7.4).
pub const DUMP_REASSEMBLED: u16 = 4;
/// Event code `dump_discarded` (§7.4).
pub const DUMP_DISCARDED: u16 = 5;
/// Event code `packet_rejected` (§7.4).
pub const PACKET_REJECTED: u16 = 6;

/// APIDs the tracker follows: the four telemetry APIDs of §3.1, with room for the Idle Packet
/// APID that A2 adds.
const TRACKED_APIDS: usize = 8;

/// Header of the dump SDU: address (u32) and length (u16) (§3.3.4).
const SDU_HEADER_LEN: usize = 6;

/// Largest memory length of a dump (§3.3.4).
const MAX_DUMP_LENGTH: usize = 1024;

/// A time stamp decoded from a packet's secondary header (301.0 §3.2.1; APP-011, APP-062).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeStamp {
    /// The APID of the packet that carried it.
    pub apid: Apid,
    /// Whole seconds since the epoch of the code, 1958 January 1 TAI.
    pub seconds: u64,
    /// Nanoseconds after `seconds`, truncated (TIME-011).
    pub nanos: u32,
}

/// A link unit that passed every strict decoder.
struct Accepted<'a> {
    header: SpacePacketHeader,
    user_data: &'a [u8],
    stamp: TimeStamp,
}

/// The telemetry applications of the ground node.
#[derive(Debug, Clone)]
pub(crate) struct Telemetry {
    /// Sequence classification per APID (APP-018).
    tracker: SequenceTracker<TRACKED_APIDS>,
    /// The reassembler of the dump APID, without its buffer.
    dump: ReassemblerState,
    /// The buffer of the reassembler: the largest dump SDU.
    dump_buf: Vec<u8>,
    /// Octets of the partial SDU the reassembler holds. The reassembler reports why it threw a
    /// partial SDU away, not how much (api-feedback #302), so the node counts.
    dump_held: usize,
    /// The time stamp of the last packet that carried one.
    last_time: Option<TimeStamp>,
}

impl Telemetry {
    /// A ground node that has received nothing.
    ///
    /// # Errors
    /// `InvalidField` if the dump APID of the scenario is not an APID (cannot happen).
    pub(crate) fn new() -> Result<Telemetry, Error> {
        // A constant APID still needs an error path (api-feedback #304).
        let dump = Apid::new(apid::DUMP).ok_or(Error::new(ErrorKind::InvalidField))?;
        Ok(Telemetry {
            tracker: SequenceTracker::new(),
            dump: ReassemblerState::new(dump),
            dump_buf: vec![0; SDU_HEADER_LEN.saturating_add(MAX_DUMP_LENGTH)],
            dump_held: 0,
            last_time: None,
        })
    }

    /// The time stamp of the last accepted packet.
    pub(crate) const fn last_time(&self) -> Option<TimeStamp> {
        self.last_time
    }

    /// Handles one delivered link unit, outputting in the order of APP-130: `packet_rejected` and
    /// nothing further if a strict decoder fails; otherwise `packet_in`, the sequence event, and
    /// for the dump APID `dump_discarded` and `dump_reassembled`.
    pub(crate) fn receive(&mut self, unit: &[u8], out: &mut dyn Output) {
        let accepted = match decode(unit) {
            Ok(accepted) => accepted,
            Err(e) => return reject(out, e.kind()),
        };
        let header = accepted.header;
        let apid = header.apid();
        let tracked = match self.tracker.track(apid, header.sequence_count()) {
            Ok(tracked) => tracked,
            Err(e) => return reject(out, e.kind()),
        };
        self.last_time = Some(accepted.stamp);
        out.packet_in(apid, unit);
        let apid_value = u64::from(apid.get());
        match tracked {
            Tracked::First | Tracked::Next(Continuity::InSequence) => {}
            Tracked::Next(Continuity::Gap { missing }) => {
                out.app_event(SEQ_GAP, &[apid_value, missing]);
            }
            Tracked::Next(Continuity::Repeat) => {
                out.app_event(
                    SEQ_REPEAT,
                    &[apid_value, u64::from(header.sequence_count().get())],
                );
            }
            Tracked::Next(Continuity::Backwards) => {
                out.app_event(
                    SEQ_RESET,
                    &[apid_value, u64::from(header.sequence_count().get())],
                );
            }
        }
        if apid.get() == apid::DUMP {
            self.reassemble(&header, accepted.user_data, out);
        }
    }

    /// Pushes a dump packet into the reassembler and reports a discarded or complete SDU.
    fn reassemble(&mut self, header: &SpacePacketHeader, user_data: &[u8], out: &mut dyn Output) {
        let held_before = self.dump_held;
        let mut reassembler = match self.dump.resume(&mut self.dump_buf) {
            Ok(r) => r,
            Err(e) => return reject(out, e.kind()),
        };
        let outcome = match reassembler.push(header, user_data) {
            Ok(outcome) => outcome,
            Err(e) => return reject(out, e.kind()),
        };
        let complete = match outcome.status {
            Status::Complete(_) => Some(parse_sdu(reassembler.sdu())),
            Status::InProgress | Status::Dropped(_) => None,
        };
        self.dump = reassembler.suspend();
        self.dump_held = match (outcome.status, header.sequence_flags()) {
            (Status::InProgress, SequenceFlags::First) => user_data.len(),
            (Status::InProgress, _) => held_before.saturating_add(user_data.len()),
            (Status::Complete(_) | Status::Dropped(_), _) => 0,
        };
        // Only a discarded partial SDU is an event, with the octets it held; a packet dropped
        // with nothing held (an orphan, an oversized first segment) raises none (APP-130).
        if outcome.discarded.is_some() {
            let Ok(lost) = u64::try_from(held_before) else {
                return reject(out, ErrorKind::OutOfRange);
            };
            out.app_event(DUMP_DISCARDED, &[u64::from(header.apid().get()), lost]);
        }
        match complete {
            Some(Ok((address, length, crc))) => {
                out.app_event(
                    DUMP_REASSEMBLED,
                    &[u64::from(address), u64::from(length), u64::from(crc)],
                );
            }
            Some(Err(e)) => reject(out, e.kind()),
            None => {}
        }
    }
}

/// `packet_rejected` with the catalogue code of `kind` (§7.4).
fn reject(out: &mut dyn Output, kind: ErrorKind) {
    out.app_event(PACKET_REJECTED, &[u64::from(kind.code())]);
}

/// The CUC format of every time stamp of the scenario (§5).
const fn cuc_format() -> Result<CucFormat, Error> {
    CucFormat::new(
        CucEpoch::Tai1958,
        time::CUC_COARSE_OCTETS,
        time::CUC_FINE_OCTETS,
    )
}

/// The secondary header of a telemetry APID: a CUC time code, with the explicit P-field of
/// `0x1E` for the event APID and the implicit one for the others (§3.1, §3.2, APP-022).
///
/// # Errors
/// `UnknownChannel` for an APID the scenario does not use for telemetry.
fn layout(apid: Apid) -> Result<SecondaryHeaderLayout, Error> {
    let explicit_pfield = match apid.get() {
        apid::HOUSEKEEPING | apid::PAYLOAD | apid::DUMP => false,
        apid::EVENTS => true,
        _ => return Err(Error::new(ErrorKind::UnknownChannel)),
    };
    let format = TimeFormat::Cuc(cuc_format()?);
    Ok(SecondaryHeaderLayout::Time(TimeHeaderLayout::new(
        format,
        explicit_pfield,
        0,
    )?))
}

/// Decodes a link unit with the strict codecs: one Space Packet, nothing after it, a known
/// telemetry APID with its Packet Type, Sequence Flags, and secondary header (§3.1, §3.2;
/// API-030, APP-011).
///
/// # Errors
/// The decoder's error, or `LengthMismatch` for octets after the packet, `UnknownChannel` for an
/// APID that is not telemetry, `InvalidField` for another Packet Type, Sequence Flags that the
/// APID does not use, or a time stamp that is not the CUC code of §5.
fn decode(unit: &[u8]) -> Result<Accepted<'_>, Error> {
    let packet = SpacePacket::decode(unit)?;
    // The strict decoder ignores octets after the packet (api-feedback #303).
    if packet.len() != unit.len() {
        return Err(Error::new(ErrorKind::LengthMismatch));
    }
    let layout = layout(packet.apid())?;
    if packet.packet_type() != PacketType::Telemetry {
        return Err(Error::new(ErrorKind::InvalidField));
    }
    // Only the dump is segmented (§3.1).
    if packet.apid().get() != apid::DUMP && packet.sequence_flags() != SequenceFlags::Unsegmented {
        return Err(Error::new(ErrorKind::InvalidField));
    }
    let split = packet.split(&layout)?;
    let Some(TimeCode::Cuc(cuc)) = split.time_code().map(|d| d.code()) else {
        return Err(Error::new(ErrorKind::InvalidField));
    };
    let format = cuc.format();
    let expected = cuc_format()?;
    if format.epoch() != expected.epoch()
        || format.coarse_octets() != expected.coarse_octets()
        || format.fine_octets() != expected.fine_octets()
    {
        return Err(Error::new(ErrorKind::InvalidField));
    }
    let (seconds, nanos) = cuc.to_seconds_nanos();
    Ok(Accepted {
        header: packet.header(),
        user_data: split.user_data(),
        stamp: TimeStamp {
            apid: packet.apid(),
            seconds,
            nanos,
        },
    })
}

/// Reads a reassembled dump SDU (§3.3.4): the address, the length, and the CRC-16 (CMN-001) of
/// the memory octets.
///
/// # Errors
/// `Incomplete` if the SDU is shorter than its header, `OutOfRange` for a length outside 1 to
/// 1024, `LengthMismatch` if the memory octets are not as many as the length says.
fn parse_sdu(sdu: &[u8]) -> Result<(u32, u16, u16), Error> {
    let Some((head, memory)) = sdu.split_first_chunk::<SDU_HEADER_LEN>() else {
        return Err(Error::incomplete(SDU_HEADER_LEN));
    };
    let [a0, a1, a2, a3, l0, l1] = *head;
    let address = u32::from_be_bytes([a0, a1, a2, a3]);
    let length = u16::from_be_bytes([l0, l1]);
    if length == 0 || usize::from(length) > MAX_DUMP_LENGTH {
        return Err(Error::new(ErrorKind::OutOfRange));
    }
    if memory.len() != usize::from(length) {
        return Err(Error::new(ErrorKind::LengthMismatch));
    }
    Ok((address, length, crc16(memory)))
}

#[cfg(test)]
mod tests {
    use super::{decode, parse_sdu};
    use aldis::core::ErrorKind;

    #[test]
    fn sdu_of_the_nominal_dump() {
        // req: APP-011, APP-018
        // Simulated memory (§3.3.4): the octet at address a is (a XOR (a >> 8)) mod 256.
        let mut sdu = vec![0x00, 0x01, 0x00, 0x00, 0x02, 0x58];
        sdu.extend((0_u32..600).map(|i| {
            let a = 0x0001_0000 + i;
            u8::try_from((a ^ (a >> 8)) & 0xFF).unwrap()
        }));
        assert_eq!(parse_sdu(&sdu).unwrap(), (65_536, 600, 0x80A8));
    }

    #[test]
    fn a_malformed_sdu_is_an_error() {
        // req: APP-011
        let kind = |sdu: &[u8]| parse_sdu(sdu).unwrap_err().kind();
        assert_eq!(kind(&[0; 5]), ErrorKind::Incomplete);
        assert_eq!(kind(&[0, 0, 0, 0, 0, 0]), ErrorKind::OutOfRange);
        assert_eq!(kind(&[0, 0, 0, 0, 0x04, 0x01, 1]), ErrorKind::OutOfRange);
        assert_eq!(kind(&[0, 0, 0, 0, 0, 2, 1]), ErrorKind::LengthMismatch);
        assert_eq!(kind(&[0, 0, 0, 0, 0, 1, 1, 2]), ErrorKind::LengthMismatch);
    }

    #[test]
    fn decode_rejects_what_the_strict_codec_rejects() {
        // req: APP-011, API-030
        let kind = |unit: &[u8]| decode(unit).err().map(|e| e.kind());
        assert_eq!(kind(&[0x08]), Some(ErrorKind::Incomplete));
        // Version 001.
        assert_eq!(
            kind(&[0x28, 0x10, 0xC0, 0, 0, 0, 0]),
            Some(ErrorKind::UnsupportedVersion)
        );
    }
}
