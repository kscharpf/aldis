//! Octet String service encoder (133.0 §3.4, §4.2.2; SPP-012).

use aldis_core::Result;
use aldis_core::ids::Apid;

use crate::config::{SppConfig, check_parts};
use crate::field;
use crate::header::{
    PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags, SpacePacketHeader, invalid,
};
use crate::sequence::SequenceCounter;
use crate::writer::write_packet;

/// The Octet String service's Packet Assembly Function for one APID (133.0 §4.2.2, SPP-012).
///
/// It builds the primary header for each Octet String, takes the count from a
/// [`SequenceCounter`], and always sets the Sequence Flags to `11` (§4.1.3.4.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OctetStringEncoder {
    config: SppConfig,
    packet_type: PacketType,
    apid: Apid,
    counter: SequenceCounter,
}

impl OctetStringEncoder {
    /// An encoder for `apid`, whose packets take counts from `counter` (133.0 §4.2.2, SPP-012).
    ///
    /// # Errors
    /// `InvalidField` (`apid`) for APID 2047: Idle Packets come from
    /// [`IdlePacket`](crate::idle::IdlePacket).
    pub const fn new(
        config: &SppConfig,
        packet_type: PacketType,
        apid: Apid,
        counter: SequenceCounter,
    ) -> Result<OctetStringEncoder> {
        if apid.is_idle() {
            return Err(invalid(field::APID));
        }
        Ok(OctetStringEncoder {
            config: *config,
            packet_type,
            apid,
            counter,
        })
    }

    /// Encodes `octets` as the data field of one packet into the start of `out`; the Secondary
    /// Header Flag is `secondary_header_indicator` (133.0 §3.4.2.3, SPP-012). Returns the total
    /// length. The counter advances only when a packet is written.
    ///
    /// # Errors
    /// `InvalidField` (`data_field`) for an empty Octet String (SPP-005); `OutOfRange`
    /// (`packet_len`) above the configured maximum (SPP-006); `BufferTooSmall` if `out` is too
    /// short. On error `out` and the counter are unchanged.
    pub fn encode(
        &mut self,
        secondary_header_indicator: bool,
        octets: &[u8],
        out: &mut [u8],
    ) -> Result<usize> {
        let fields = PacketFields {
            packet_type: self.packet_type,
            apid: self.apid,
            sequence_flags: SequenceFlags::Unsegmented,
            sequence_count: self.counter.peek(),
        };
        check_parts(fields, 0, octets.len())?;
        self.config
            .check_len(octets.len().saturating_add(PRIMARY_HEADER_LEN))?;
        let header = SpacePacketHeader::new(fields, secondary_header_indicator, octets.len());
        let total = write_packet(out, &header, [octets, &[]])?;
        self.counter.advance();
        Ok(total)
    }

    /// The counter, whose next count the next packet takes (133.0 §4.1.3.4.3.4; SPP-011, SPP-012).
    #[must_use]
    pub const fn counter(&self) -> SequenceCounter {
        self.counter
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::OctetStringEncoder;
    use crate::config::SppConfig;
    use crate::field;
    use crate::header::{PacketType, SequenceFlags};
    use crate::sequence::SequenceCounter;
    use crate::view::SpacePacket;
    use aldis_core::ErrorKind;
    use aldis_core::ids::{Apid, SeqCount};

    #[test]
    fn octet_strings() {
        // req: SPP-012, SPP-005, SPP-006
        let cfg = SppConfig::new(10).unwrap();
        let apid = Apid::new(0x42).unwrap();
        let counter = SequenceCounter::new(SeqCount::new(0x3FFF).unwrap());
        let e =
            OctetStringEncoder::new(&cfg, PacketType::Telemetry, Apid::IDLE, counter).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::APID))
        );
        let mut enc =
            OctetStringEncoder::new(&cfg, PacketType::Telecommand, apid, counter).unwrap();
        let mut out = [0_u8; 10];
        assert_eq!(enc.encode(true, &[1, 2], &mut out), Ok(8));
        let p = SpacePacket::decode(&out).unwrap();
        assert_eq!(
            (p.apid(), p.sequence_flags(), p.sequence_count().get()),
            (apid, SequenceFlags::Unsegmented, 0x3FFF)
        );
        assert!(p.has_secondary_header() && p.packet_type() == PacketType::Telecommand);
        assert_eq!(enc.counter().peek().get(), 0);
        for (octets, kind) in [
            (&[][..], ErrorKind::InvalidField),
            (&[0; 5][..], ErrorKind::OutOfRange),
        ] {
            assert_eq!(
                enc.encode(false, octets, &mut out).unwrap_err().kind(),
                kind
            );
        }
        assert_eq!(
            enc.encode(false, &[1], &mut out[..6]).unwrap_err().kind(),
            ErrorKind::BufferTooSmall
        );
        assert_eq!(
            enc.counter().peek().get(),
            0,
            "errors do not advance the counter"
        );
        assert_eq!(enc.encode(false, &[9], &mut out), Ok(7));
        assert_eq!(
            (out[1], out[3], enc.counter().peek().get()),
            (0x42, 0x00, 1)
        );
    }
}
