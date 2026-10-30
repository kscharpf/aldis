//! Encoders: one-shot [`encode`] and the two-phase [`SpacePacketWriter`] (133.0 §4.1;
//! SPP-005, SPP-006, API-020…API-023, GEN-014).

use aldis_core::{Error, Result};

use crate::config::{PacketData, SppConfig, check_parts};
use crate::field;
use crate::header::{
    PRIMARY_HEADER_LEN, PacketFields, SpacePacketHeader, UNFINISHED_VERSION, len_u64,
};

/// Writes a header and the concatenated `parts` into the start of `out`; returns the total
/// length. The caller has checked the parts (SPP-005) and the length (SPP-006).
///
/// # Errors
/// `BufferTooSmall` (`needed` = total length) if `out` is too short; `out` is then unchanged
/// (API-022).
pub(crate) fn write_packet(
    out: &mut [u8],
    header: &SpacePacketHeader,
    parts: [&[u8]; 2],
) -> Result<usize> {
    let total = header.packet_len();
    let dst = out.get_mut(..total).ok_or(Error::buffer_too_small(total))?;
    let octets = header.octets();
    let body = parts.iter().flat_map(|p| p.iter());
    for (d, s) in dst.iter_mut().zip(octets.iter().chain(body)) {
        *d = *s;
    }
    Ok(total)
}

/// Encodes a checked packet into the start of `out`; returns the total length (133.0 §4.1; SPP-005,
/// SPP-006, API-020, API-021). The Secondary Header Flag and Packet Data Length are derived
/// from the parts.
///
/// # Errors
/// `BufferTooSmall` (`needed` = total length) if `out` is too short. On error `out` is
/// unchanged (API-022).
pub fn encode(packet: &PacketData<'_>, out: &mut [u8]) -> Result<usize> {
    let secondary = packet.secondary_header();
    let user = packet.user_data();
    let data_len = secondary.len().saturating_add(user.len());
    let header = SpacePacketHeader::new(packet.fields(), !secondary.is_empty(), data_len);
    write_packet(out, &header, [secondary, user])
}

/// Two-phase writer: [`begin`](Self::begin) reserves the header, the secondary header, and up
/// to a maximum of user data; the caller fills both in place; [`finish`](Self::finish) writes
/// the header (133.0 §4.1.3.5; API-020…API-023).
///
/// Until `finish`, the buffer holds a header whose version field is `111` and whose Packet Data
/// Length is 0, which every decoder rejects with `UnsupportedVersion`, so an unfinished packet
/// cannot be mistaken for a valid one (API-023).
#[derive(Debug)]
pub struct SpacePacketWriter<'b> {
    out: &'b mut [u8],
    fields: PacketFields,
    secondary_len: usize,
    max_user_len: usize,
}

impl<'b> SpacePacketWriter<'b> {
    /// Reserves a packet with a secondary header of `secondary_len` octets (0 for none) and room
    /// for up to `max_user_len` user data octets (133.0 §4.1; SPP-005, SPP-006, API-020).
    ///
    /// # Errors
    /// - `InvalidField` (`data_field`) if both lengths are 0, since such a packet could never
    ///   finish (SPP-005); `InvalidField` (`secondary_header_flag`) for an Idle Packet with a
    ///   secondary header.
    /// - `OutOfRange` (`packet_len`) if the largest packet exceeds `config` (SPP-006).
    /// - `BufferTooSmall` if `out` cannot hold the largest packet.
    ///
    /// On error `out` is unchanged.
    pub fn begin(
        out: &'b mut [u8],
        config: &SppConfig,
        fields: PacketFields,
        secondary_len: usize,
        max_user_len: usize,
    ) -> Result<SpacePacketWriter<'b>> {
        let max_data = secondary_len.saturating_add(max_user_len);
        check_parts(fields, secondary_len, max_data)?;
        let total = max_data.saturating_add(PRIMARY_HEADER_LEN);
        config.check_len(total)?;
        let buf = out.get_mut(..total).ok_or(Error::buffer_too_small(total))?;
        // Version `111` and Packet Data Length 0 until `finish` (API-023).
        let unfinished = SpacePacketHeader::new(fields, secondary_len != 0, 1);
        for (d, s) in buf
            .iter_mut()
            .zip(unfinished.octets_with_version(UNFINISHED_VERSION))
        {
            *d = s;
        }
        Ok(SpacePacketWriter {
            out,
            fields,
            secondary_len,
            max_user_len,
        })
    }

    /// The secondary header to fill in place, `secondary_len` octets (133.0 §4.1.4.2, GEN-014).
    pub fn secondary_header_mut(&mut self) -> &mut [u8] {
        let end = PRIMARY_HEADER_LEN.saturating_add(self.secondary_len);
        self.out
            .get_mut(PRIMARY_HEADER_LEN..end)
            .unwrap_or_default()
    }

    /// The user data field to fill in place, `max_user_len` octets (133.0 §4.1.4.3, GEN-014).
    pub fn user_data_mut(&mut self) -> &mut [u8] {
        let start = PRIMARY_HEADER_LEN.saturating_add(self.secondary_len);
        let end = start.saturating_add(self.max_user_len);
        self.out.get_mut(start..end).unwrap_or_default()
    }

    /// Completes the packet with `user_len` octets of user data; returns the total length (133.0
    /// §4.1.3.5; SPP-005, API-020, API-023).
    ///
    /// The packet is the first `total` octets of the buffer. When `user_len` is below the
    /// reservation, the reserved octets after the packet are left as they are; use only
    /// `out[..total]`.
    ///
    /// # Errors
    /// `OutOfRange` (`user_len`) above the reservation; `InvalidField` (`data_field`) if the
    /// packet would have no data field (SPP-005). On error the buffer keeps its unfinished
    /// state.
    pub fn finish(self, user_len: usize) -> Result<usize> {
        if user_len > self.max_user_len {
            return Err(Error::out_of_range(
                field::USER_LEN,
                len_u64(self.max_user_len),
                len_u64(user_len),
            ));
        }
        let data_len = self.secondary_len.saturating_add(user_len);
        check_parts(self.fields, self.secondary_len, data_len)?;
        let header = SpacePacketHeader::new(self.fields, self.secondary_len != 0, data_len);
        for (d, s) in self.out.iter_mut().zip(header.octets()) {
            *d = s;
        }
        Ok(header.packet_len())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{SpacePacketWriter, encode};
    use crate::config::SppConfig;
    use crate::field;
    use crate::header::{PacketFields, PacketType, SequenceFlags};
    use crate::view::SpacePacket;
    use aldis_core::ids::{Apid, SeqCount};
    use aldis_core::{ErrorDetail, ErrorKind};

    fn fields(apid: u16, count: u16) -> PacketFields {
        PacketFields {
            packet_type: PacketType::Telecommand,
            apid: Apid::new(apid).unwrap(),
            sequence_flags: SequenceFlags::First,
            sequence_count: SeqCount::new(count).unwrap(),
        }
    }

    #[test]
    fn too_small_buffers_are_untouched() {
        // req: API-022
        let cfg = SppConfig::DEFAULT;
        let p = cfg.packet(fields(5, 9), &[0xA0], &[1, 2]).unwrap();
        let mut out = [0xEE_u8; 8];
        let e = encode(&p, &mut out).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(9))
        );
        assert_eq!(out, [0xEE; 8]);
        let e = SpacePacketWriter::begin(&mut out, &cfg, fields(5, 9), 1, 2).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(out, [0xEE; 8]);
    }

    #[test]
    fn unfinished_writer_leaves_an_invalid_packet() {
        // req: API-023, SPP-001
        let mut out = [0_u8; 16];
        let mut w =
            SpacePacketWriter::begin(&mut out, &SppConfig::DEFAULT, fields(5, 9), 2, 4).unwrap();
        w.secondary_header_mut().copy_from_slice(&[0xA0, 0xA1]);
        w.user_data_mut()[..2].copy_from_slice(&[1, 2]);
        let e = SpacePacket::decode_lenient(&out).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
        // Version 111, the chosen fields and flag, and Packet Data Length 0 (API-023).
        assert_eq!(out[..6], [0xF8, 0x05, 0x40, 0x09, 0x00, 0x00]);
    }

    #[test]
    fn writer_checks() {
        // req: SPP-005, SPP-006, API-021
        let cfg = SppConfig::new(12).unwrap();
        let mut out = [0_u8; 16];
        let e = SpacePacketWriter::begin(&mut out, &cfg, fields(5, 0), 0, 0).unwrap_err();
        assert_eq!(e.field(), Some(field::DATA_FIELD));
        let e = SpacePacketWriter::begin(&mut out, &cfg, fields(0x7FF, 0), 1, 1).unwrap_err();
        assert_eq!(e.field(), Some(field::SECONDARY_HEADER_FLAG));
        let e = SpacePacketWriter::begin(&mut out, &cfg, fields(5, 0), 3, 4).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::PACKET_LEN))
        );
        let w = SpacePacketWriter::begin(&mut out, &cfg, fields(5, 0), 0, 3).unwrap();
        let e = w.finish(0).unwrap_err();
        assert_eq!(e.field(), Some(field::DATA_FIELD));
        let w = SpacePacketWriter::begin(&mut out, &cfg, fields(5, 0), 0, 3).unwrap();
        let e = w.finish(4).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (
                ErrorKind::OutOfRange,
                ErrorDetail::Expected {
                    expected: 3,
                    found: 4
                }
            )
        );
        let mut w = SpacePacketWriter::begin(&mut out, &cfg, fields(5, 0), 2, 3).unwrap();
        assert!(w.user_data_mut().len() == 3 && w.secondary_header_mut().len() == 2);
        w.secondary_header_mut().copy_from_slice(&[7, 8]);
        assert_eq!(w.finish(0), Ok(8));
        let p = SpacePacket::decode(&out).unwrap();
        assert!(p.has_secondary_header() && p.data_field() == [7, 8] && p.len() == 8);
    }
}
