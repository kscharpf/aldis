//! Optional SDU segmentation and reassembly helpers (133.0 §4.1.3.4.2 note; SPP-013, OI-30).
//!
//! 133.0 does not mandate segmentation: these are utilities on top of the codec. The rules are
//! in `docs/standards/space-packet.md` §5.4.

use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{Error, ErrorDetail, Result};

use crate::config::SppConfig;
use crate::field;
use crate::header::{PacketFields, PacketType, SequenceFlags, SpacePacketHeader, invalid};
use crate::sequence::SequenceCounter;
use crate::writer::encode;

/// Splits one SDU across consecutive packets of one APID (133.0 §4.1.3.4.2, SPP-013).
///
/// A single segment is sent with Sequence Flags `11`; otherwise the flags are `01`, `00`…,
/// `10`. Each packet takes the next count of the caller's [`SequenceCounter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segmenter<'s> {
    config: SppConfig,
    packet_type: PacketType,
    apid: Apid,
    sdu: &'s [u8],
    max_user_len: usize,
    offset: usize,
}

impl<'s> Segmenter<'s> {
    /// A segmenter for `sdu` with at most `max_user_len` user data octets per packet (133.0
    /// §4.1.3.4.2, SPP-013).
    ///
    /// # Errors
    /// - `InvalidField` (`sdu`) for an empty SDU.
    /// - `OutOfRange` (`max_user_len`) for a maximum of 0.
    /// - `InvalidField` (`apid`) for APID 2047: Idle Packets carry Idle Data (133.0
    ///   §4.1.4.3.4).
    pub const fn new(
        config: &SppConfig,
        packet_type: PacketType,
        apid: Apid,
        sdu: &'s [u8],
        max_user_len: usize,
    ) -> Result<Segmenter<'s>> {
        if sdu.is_empty() {
            return Err(invalid(field::SDU).with_detail(ErrorDetail::Expected {
                expected: 1,
                found: 0,
            }));
        }
        if max_user_len == 0 {
            return Err(Error::out_of_range(field::MAX_USER_LEN, 1, 0));
        }
        if apid.is_idle() {
            return Err(invalid(field::APID));
        }
        Ok(Segmenter {
            config: *config,
            packet_type,
            apid,
            sdu,
            max_user_len,
            offset: 0,
        })
    }

    /// Encodes the next segment into the start of `out` with `secondary_header` (empty for
    /// none) and the next count of `counter`; returns its length, or `None` when the SDU is
    /// done (133.0 §4.1.3.4.2, SPP-013).
    ///
    /// # Errors
    /// `OutOfRange` (`packet_len`) if the segment exceeds the configured maximum (SPP-006);
    /// `BufferTooSmall` if `out` is too short. On error nothing changes: `out`, `counter`, and
    /// the position in the SDU are as before.
    pub fn next_packet(
        &mut self,
        counter: &mut SequenceCounter,
        secondary_header: &[u8],
        out: &mut [u8],
    ) -> Result<Option<usize>> {
        let rest = self.sdu.get(self.offset..).unwrap_or_default();
        if rest.is_empty() {
            return Ok(None);
        }
        let take = rest.len().min(self.max_user_len);
        let chunk = rest.get(..take).unwrap_or_default();
        let first = self.offset == 0;
        let last = take == rest.len();
        let sequence_flags = match (first, last) {
            (true, true) => SequenceFlags::Unsegmented,
            (true, false) => SequenceFlags::First,
            (false, true) => SequenceFlags::Last,
            (false, false) => SequenceFlags::Continuation,
        };
        let fields = PacketFields {
            packet_type: self.packet_type,
            apid: self.apid,
            sequence_flags,
            sequence_count: counter.peek(),
        };
        let packet = self.config.packet(fields, secondary_header, chunk)?;
        let len = encode(&packet, out)?;
        counter.advance();
        self.offset = self.offset.saturating_add(take);
        Ok(Some(len))
    }

    /// Whether every segment has been written (133.0 §4.1.3.4.2, SPP-013).
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.offset >= self.sdu.len()
    }

    /// SDU octets not yet written (133.0 §4.1.3.4.2, SPP-013).
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.sdu.len().saturating_sub(self.offset)
    }
}

/// Why the reassembler threw data away (133.0 §4.1.3.4.2; SPP-013, OI-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Discard {
    /// A sequence gap (any continuity but in-sequence) while assembling.
    Gap,
    /// A first segment (`01`) arrived while assembling.
    Restarted,
    /// An unsegmented packet (`11`) arrived while assembling.
    Interrupted,
    /// A continuation or last segment (`00`, `10`) with nothing in progress.
    Orphan,
    /// The SDU would exceed the buffer.
    Overflow,
}

/// What happened to the packet just pushed (133.0 §4.1.3.4.2, SPP-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// The packet's segment was stored; the SDU is not complete yet.
    InProgress,
    /// The SDU is complete: this many octets, readable with [`Reassembler::sdu`] until the
    /// next push.
    Complete(usize),
    /// The packet was dropped, for the reason given: `Orphan` or `Overflow`.
    Dropped(Discard),
}

/// The outcome of [`Reassembler::push`] (133.0 §4.1.3.4.2; SPP-013,
/// `docs/standards/space-packet.md` §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Outcome {
    /// Why a partial SDU was thrown away, if one was: `Gap`, `Restarted`, `Interrupted`, or
    /// `Overflow` (133.0 §4.1.3.4.2, SPP-013).
    pub discarded: Option<Discard>,
    /// What happened to the packet itself (133.0 §4.1.3.4.2, SPP-013).
    pub status: Status,
}

/// Rejoins the segments of one APID into the caller's buffer, detecting sequence gaps
/// (133.0 §4.1.3.4.2, SPP-013).
///
/// It never fails or gets stuck on segment order: a partial SDU that cannot be completed is
/// discarded, the [`Outcome`] says why, and the packet is then handled as if nothing were in
/// progress (OI-30).
#[derive(Debug)]
pub struct Reassembler<'b> {
    apid: Apid,
    buf: &'b mut [u8],
    filled: usize,
    assembling: bool,
    complete: usize,
    last: Option<SeqCount>,
}

impl<'b> Reassembler<'b> {
    /// A reassembler for `apid` that assembles SDUs of up to `buf.len()` octets (133.0 §4.1.3.4.2,
    /// SPP-013).
    #[must_use]
    pub const fn new(apid: Apid, buf: &'b mut [u8]) -> Reassembler<'b> {
        Reassembler {
            apid,
            buf,
            filled: 0,
            assembling: false,
            complete: 0,
            last: None,
        }
    }

    /// Adds the user data of one packet, whose primary header is `header` (133.0 §4.1.3.4.2,
    /// SPP-013).
    ///
    /// # Errors
    /// `InvalidField` (`apid`) for a packet of another APID; nothing changes.
    pub fn push(&mut self, header: &SpacePacketHeader, user_data: &[u8]) -> Result<Outcome> {
        if header.apid() != self.apid {
            return Err(invalid(field::APID).with_detail(ErrorDetail::Expected {
                expected: u64::from(self.apid.get()),
                found: u64::from(header.apid().get()),
            }));
        }
        self.complete = 0;
        let count = header.sequence_count();
        let in_sequence = self
            .last
            .is_some_and(|last| last.classify(count) == Continuity::InSequence);
        self.last = Some(count);
        let mut discarded = None;
        if self.assembling && !in_sequence {
            self.assembling = false;
            discarded = Some(Discard::Gap);
        }
        let status = match header.sequence_flags() {
            SequenceFlags::First => {
                if self.assembling {
                    discarded = Some(Discard::Restarted);
                }
                self.filled = 0;
                self.assembling = self.append(user_data);
                if self.assembling {
                    Status::InProgress
                } else {
                    Status::Dropped(Discard::Overflow)
                }
            }
            SequenceFlags::Unsegmented => {
                if self.assembling {
                    discarded = Some(Discard::Interrupted);
                }
                self.assembling = false;
                self.filled = 0;
                if self.append(user_data) {
                    self.complete = self.filled;
                    Status::Complete(self.filled)
                } else {
                    Status::Dropped(Discard::Overflow)
                }
            }
            SequenceFlags::Continuation | SequenceFlags::Last if !self.assembling => {
                Status::Dropped(Discard::Orphan)
            }
            flags @ (SequenceFlags::Continuation | SequenceFlags::Last) => {
                if !self.append(user_data) {
                    self.assembling = false;
                    discarded = Some(Discard::Overflow);
                    Status::Dropped(Discard::Overflow)
                } else if flags == SequenceFlags::Last {
                    self.assembling = false;
                    self.complete = self.filled;
                    Status::Complete(self.filled)
                } else {
                    Status::InProgress
                }
            }
        };
        Ok(Outcome { discarded, status })
    }

    /// Appends `data` after the stored octets; `false`, storing nothing, if it does not fit.
    fn append(&mut self, data: &[u8]) -> bool {
        let end = self.filled.saturating_add(data.len());
        let Some(dst) = self.buf.get_mut(self.filled..end) else {
            return false;
        };
        for (d, s) in dst.iter_mut().zip(data) {
            *d = *s;
        }
        self.filled = end;
        true
    }

    /// The SDU completed by the last push; empty after any other outcome (133.0 §4.1.3.4.2,
    /// SPP-013).
    #[must_use]
    pub fn sdu(&self) -> &[u8] {
        self.buf.get(..self.complete).unwrap_or_default()
    }

    /// Whether a partial SDU is in progress (133.0 §4.1.3.4.2, SPP-013).
    #[must_use]
    pub const fn is_assembling(&self) -> bool {
        self.assembling
    }

    /// The APID this reassembler accepts (133.0 §4.1.3.4.2, SPP-013).
    #[must_use]
    pub const fn apid(&self) -> Apid {
        self.apid
    }

    /// The largest SDU the buffer holds (133.0 §4.1.3.4.2, SPP-013).
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Discards any partial SDU and forgets the last count (133.0 §4.1.3.4.2, SPP-013).
    pub const fn reset(&mut self) {
        self.filled = 0;
        self.assembling = false;
        self.complete = 0;
        self.last = None;
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::field;

    use super::{Discard, Outcome, Reassembler, Segmenter, Status};
    use crate::config::SppConfig;
    use crate::header::{PacketFields, PacketType, SequenceFlags, SpacePacketHeader};
    use crate::sequence::SequenceCounter;
    use crate::view::SpacePacket;
    use SequenceFlags::{Continuation as C, First as F, Last as L, Unsegmented as U};
    use aldis_core::ids::{Apid, SeqCount};
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind};

    fn apid(n: u16) -> Apid {
        Apid::new(n).unwrap()
    }

    fn header(flags: SequenceFlags, count: u16) -> SpacePacketHeader {
        let fields = PacketFields {
            packet_type: PacketType::Telemetry,
            apid: apid(5),
            sequence_flags: flags,
            sequence_count: SeqCount::new(count).unwrap(),
        };
        SpacePacketHeader::new(fields, false, 1)
    }

    const fn outcome(discarded: Option<Discard>, status: Status) -> Outcome {
        Outcome { discarded, status }
    }

    #[test]
    fn segmenter_checks() {
        // req: SPP-013, SPP-005, SPP-006
        let cfg = SppConfig::new(10).unwrap();
        let tm = PacketType::Telemetry;
        let e = Segmenter::new(&cfg, tm, apid(5), &[], 4).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::SDU))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 1,
                found: 0
            }
        );
        let e = Segmenter::new(&cfg, tm, apid(5), &[1], 0).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::MAX_USER_LEN))
        );
        let e = Segmenter::new(&cfg, tm, Apid::IDLE, &[1], 4).unwrap_err();
        assert_eq!(e.field(), Some(field::APID));
        // Errors change nothing: a short buffer, then a too-long packet.
        let mut seg = Segmenter::new(&cfg, tm, apid(5), &[1, 2, 3, 4, 5, 6], 4).unwrap();
        let mut counter = SequenceCounter::new(SeqCount::new(9).unwrap());
        let mut out = [0_u8; 16];
        assert_eq!(
            seg.next_packet(&mut counter, &[], &mut out[..5])
                .unwrap_err()
                .kind(),
            ErrorKind::BufferTooSmall
        );
        assert_eq!(
            seg.next_packet(&mut counter, &[0xA0], &mut out)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            (seg.remaining(), seg.is_done(), counter.peek().get()),
            (6, false, 9)
        );
        assert_eq!(seg.next_packet(&mut counter, &[], &mut out), Ok(Some(10)));
        let p = SpacePacket::decode(&out, DecodeMode::Strict).unwrap();
        assert_eq!(
            (p.sequence_flags(), p.sequence_count().get(), p.data_field()),
            (SequenceFlags::First, 9, &[1, 2, 3, 4][..])
        );
        assert_eq!(seg.next_packet(&mut counter, &[], &mut out), Ok(Some(8)));
        assert_eq!(
            SpacePacket::decode(&out, DecodeMode::Strict)
                .unwrap()
                .sequence_flags(),
            SequenceFlags::Last
        );
        assert!(seg.is_done() && seg.remaining() == 0);
        assert_eq!(seg.next_packet(&mut counter, &[], &mut out), Ok(None));
        assert_eq!(counter.peek().get(), 11);
    }

    #[test]
    fn reassembler_rules() {
        // req: SPP-013, OI-30
        let mut buf = [0_u8; 4];
        let mut ra = Reassembler::new(apid(5), &mut buf);
        assert_eq!(
            (ra.apid(), ra.capacity(), ra.is_assembling()),
            (apid(5), 4, false)
        );
        let other = SpacePacketHeader::new(
            PacketFields {
                apid: apid(6),
                ..header(SequenceFlags::First, 0).fields()
            },
            false,
            1,
        );
        let e = ra.push(&other, &[1]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::APID))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 5,
                found: 6
            }
        );
        let steps: [(SequenceFlags, u16, &[u8], Outcome); 12] = [
            (F, 1, &[1], outcome(None, Status::InProgress)),
            (C, 2, &[2], outcome(None, Status::InProgress)),
            (L, 3, &[3], outcome(None, Status::Complete(3))),
            (C, 4, &[4], outcome(None, Status::Dropped(Discard::Orphan))),
            (F, 5, &[5], outcome(None, Status::InProgress)),
            (
                L,
                7,
                &[7],
                outcome(Some(Discard::Gap), Status::Dropped(Discard::Orphan)),
            ),
            (F, 8, &[8], outcome(None, Status::InProgress)),
            (
                F,
                9,
                &[9, 9],
                outcome(Some(Discard::Restarted), Status::InProgress),
            ),
            (
                C,
                10,
                &[1, 2, 3],
                outcome(Some(Discard::Overflow), Status::Dropped(Discard::Overflow)),
            ),
            (F, 11, &[1], outcome(None, Status::InProgress)),
            (
                U,
                12,
                &[6, 6],
                outcome(Some(Discard::Interrupted), Status::Complete(2)),
            ),
            (
                U,
                13,
                &[1, 2, 3, 4, 5],
                outcome(None, Status::Dropped(Discard::Overflow)),
            ),
        ];
        for (flags, count, data, expected) in steps {
            assert_eq!(
                ra.push(&header(flags, count), data),
                Ok(expected),
                "{flags:?} {count}"
            );
            if let Status::Complete(n) = expected.status {
                assert_eq!(ra.sdu().len(), n);
            } else {
                assert_eq!(ra.sdu(), <&[u8]>::default());
            }
        }
        assert_eq!(
            ra.push(&header(F, 0), &[1, 2, 3, 4, 5]),
            Ok(outcome(None, Status::Dropped(Discard::Overflow)))
        );
        assert!(!ra.is_assembling());
        assert_eq!(
            ra.push(&header(F, 1), &[1]),
            Ok(outcome(None, Status::InProgress))
        );
        assert!(ra.is_assembling());
        ra.reset();
        assert!(!ra.is_assembling() && ra.sdu().is_empty());
        // After a reset there is no previous count: a continuation is an orphan, not a gap.
        assert_eq!(
            ra.push(&header(C, 2), &[1]),
            Ok(outcome(None, Status::Dropped(Discard::Orphan)))
        );
        assert_eq!(
            ra.push(&header(U, 3), &[7]),
            Ok(outcome(None, Status::Complete(1)))
        );
        assert_eq!(ra.sdu(), &[7]);
    }
}
