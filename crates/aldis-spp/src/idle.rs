//! Idle Packets (133.0 §4.1.3.3.4.4, §4.1.4.3.4; SPP-010).

use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{Error, Result};

use crate::config::SppConfig;
use crate::header::{
    MIN_PACKET_LEN, PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags, SpacePacketHeader,
    len_u64,
};

/// Default Idle Data pattern, the single octet `0x00` (SPP-010).
const DEFAULT_PATTERN: &[u8] = &[0x00];

/// Builder for an Idle Packet (SPP-010).
///
/// The packet has APID 2047, no secondary header, Sequence Flags `11`, and a User Data Field
/// filled by repeating a pattern. CCSDS leaves the Idle Data pattern to the mission (133.0
/// §4.1.4.3.4 note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdlePacket<'p> {
    total_len: usize,
    sequence_count: SeqCount,
    pattern: &'p [u8],
}

impl<'p> IdlePacket<'p> {
    /// An Idle Packet of `total_len` octets with count 0 and the pattern `0x00` (133.0
    /// §4.1.3.3.4.4, SPP-010).
    ///
    /// # Errors
    /// `OutOfRange` (`packet_len`) below 7 octets or above `config`'s maximum (SPP-006).
    pub fn new(config: &SppConfig, total_len: usize) -> Result<IdlePacket<'p>> {
        if total_len < MIN_PACKET_LEN {
            return Err(Error::out_of_range(
                "packet_len",
                len_u64(MIN_PACKET_LEN),
                len_u64(total_len),
            ));
        }
        config.check_len(total_len)?;
        Ok(IdlePacket {
            total_len,
            sequence_count: SeqCount::default(),
            pattern: DEFAULT_PATTERN,
        })
    }

    /// The same packet with Packet Sequence Count `count` (133.0 §4.1.3.4.3; SPP-010, SPP-011).
    #[must_use]
    pub const fn with_sequence_count(self, count: SeqCount) -> IdlePacket<'p> {
        IdlePacket {
            sequence_count: count,
            ..self
        }
    }

    /// The same packet with Idle Data repeating `pattern`; an empty pattern means the default
    /// `0x00` (133.0 §4.1.4.3.4, SPP-010).
    #[must_use]
    pub const fn with_pattern(self, pattern: &'p [u8]) -> IdlePacket<'p> {
        IdlePacket {
            pattern: if pattern.is_empty() {
                DEFAULT_PATTERN
            } else {
                pattern
            },
            ..self
        }
    }

    /// Total packet length in octets (133.0 §4.1.2.2, SPP-010).
    #[must_use]
    pub const fn total_len(&self) -> usize {
        self.total_len
    }

    /// Encodes the packet into the start of `out`; returns the total length (133.0 §4.1.3.3.4.4,
    /// §4.1.4.3.4, SPP-010).
    ///
    /// # Errors
    /// `BufferTooSmall` (`needed` = total length) if `out` is too short; `out` is then unchanged
    /// (API-022).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        let fields = PacketFields {
            packet_type: PacketType::Telemetry,
            apid: Apid::IDLE,
            sequence_flags: SequenceFlags::Unsegmented,
            sequence_count: self.sequence_count,
        };
        let data_len = self.total_len.saturating_sub(PRIMARY_HEADER_LEN);
        let header = SpacePacketHeader::new(fields, false, data_len);
        let dst = out
            .get_mut(..self.total_len)
            .ok_or(Error::buffer_too_small(self.total_len))?;
        let fill = self.pattern.iter().cycle().take(data_len);
        for (d, s) in dst.iter_mut().zip(header.octets().iter().chain(fill)) {
            *d = *s;
        }
        Ok(self.total_len)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::IdlePacket;
    use crate::config::SppConfig;
    use crate::view::SpacePacket;
    use aldis_core::ids::SeqCount;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind};

    #[test]
    fn idle_packets() {
        // req: SPP-010, SPP-006, API-022
        let cfg = SppConfig::new(20).unwrap();
        let idle = IdlePacket::new(&cfg, 11)
            .unwrap()
            .with_sequence_count(SeqCount::new(3).unwrap())
            .with_pattern(&[1, 2]);
        assert_eq!(idle.total_len(), 11);
        let mut out = [0_u8; 11];
        assert_eq!(idle.encode(&mut out), Ok(11));
        assert_eq!(out, [0x07, 0xFF, 0xC0, 0x03, 0x00, 0x04, 1, 2, 1, 2, 1]);
        let p = SpacePacket::decode(&out, DecodeMode::Strict).unwrap();
        assert!(p.is_idle() && !p.has_secondary_header());
        let default = IdlePacket::new(&cfg, 7).unwrap().with_pattern(&[]);
        let mut short = [0xEE_u8; 6];
        let e = default.encode(&mut short).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail(), short),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(7), [0xEE; 6])
        );
        let mut seven = [0xEE_u8; 7];
        assert_eq!(default.encode(&mut seven), Ok(7));
        assert_eq!(seven, [0x07, 0xFF, 0xC0, 0, 0, 0, 0]);
        let e = IdlePacket::new(&cfg, 6).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (
                ErrorKind::OutOfRange,
                ErrorDetail::Expected {
                    expected: 7,
                    found: 6
                }
            )
        );
        assert_eq!(
            IdlePacket::new(&cfg, 21).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
    }
}
