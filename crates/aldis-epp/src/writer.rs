//! Encoders: one-shot [`encode`], the two-phase [`EncapsulationWriter`], and [`idle`] packets
//! (133.1 §4.1; EPP-008, EPP-009, API-020…API-023, GEN-014).

use aldis_core::{Error, Result};

use crate::config::{DataUnit, EppConfig};
use crate::field;
use crate::header::{EncapsulationFields, EncapsulationHeader, HeaderSize, invalid, len_u64};

/// Encodes a checked data unit as one packet into the start of `out`; returns the total length
/// (EPP-008, EPP-010, API-020, API-021).
///
/// The managed parameters were applied when the [`DataUnit`] was built (133.1 §4.2.1.1.2).
///
/// # Errors
/// The header size policy errors of [`EppConfig::header`], and `BufferTooSmall` (`needed` = total
/// length) if `out` is too short. On any error `out` is unchanged (API-022).
pub fn encode(unit: &DataUnit<'_>, size: HeaderSize, out: &mut [u8]) -> Result<usize> {
    let data = unit.data();
    let header = EncapsulationHeader::for_data(unit.fields(), size, data.len())?;
    let total = header.header_len().octets().saturating_add(data.len());
    let dst = out.get_mut(..total).ok_or(Error::buffer_too_small(total))?;
    header.write_to(dst);
    for (d, s) in dst.iter_mut().skip(header.header_len().octets()).zip(data) {
        *d = *s;
    }
    Ok(total)
}

/// Encodes an Encapsulation Idle Packet of `total_len` octets whose data field is filled with
/// `fill` (EPP-009). CCSDS does not specify the idle pattern (133.1 §4.1.3.1 note 3).
///
/// # Errors
/// `OutOfRange` as for [`EncapsulationHeader::idle`]; `BufferTooSmall` (`needed` =
/// `total_len`) if `out` is too short. On any error `out` is unchanged.
pub fn idle(total_len: usize, fill: u8, out: &mut [u8]) -> Result<usize> {
    let header = EncapsulationHeader::idle(total_len)?;
    let dst = out
        .get_mut(..total_len)
        .ok_or(Error::buffer_too_small(total_len))?;
    header.write_to(dst);
    for d in dst.iter_mut().skip(header.header_len().octets()) {
        *d = fill;
    }
    Ok(total_len)
}

/// Two-phase writer: the header is reserved in [`begin`](Self::begin), the caller fills the data
/// field in place, and [`finish`](Self::finish) writes the Packet Length (133.1 §4.1.2.8;
/// API-020…023).
///
/// Until `finish` is called, the buffer holds a header whose Packet Length is 0 (for 2-, 4-,
/// and 8-octet headers), which every decoder rejects with `LengthMismatch`, so an unfinished
/// packet cannot be mistaken for a valid one (API-023).
#[derive(Debug)]
pub struct EncapsulationWriter<'b> {
    out: &'b mut [u8],
    config: EppConfig,
    header: EncapsulationHeader,
    max_data_len: usize,
}

impl<'b> EncapsulationWriter<'b> {
    /// Reserves a packet with `fields` and room for up to `max_data_len` data octets. Under
    /// [`HeaderSize::Auto`] the header is the smallest that fits `max_data_len`. The fields are
    /// checked against `config` here and the length in [`finish`](Self::finish) (133.1
    /// §4.2.1.1.2, EPP-010).
    ///
    /// # Errors
    /// `UnknownChannel` as for [`EppConfig::check`]; the header size policy errors of
    /// [`EppConfig::header`] for `max_data_len`, so `InvalidField` (`epi`) if `max_data_len` is 0
    /// for a non-idle EPI, which could never finish (EPP-005); and `BufferTooSmall` if `out`
    /// cannot hold the header and `max_data_len` data octets. On error `out` is unchanged.
    pub fn begin(
        out: &'b mut [u8],
        config: &EppConfig,
        fields: EncapsulationFields,
        size: HeaderSize,
        max_data_len: usize,
    ) -> Result<EncapsulationWriter<'b>> {
        config.check_fields(fields)?;
        let header = EncapsulationHeader::for_data(fields, size, max_data_len)?;
        let total = header.header_len().octets().saturating_add(max_data_len);
        let buf = out.get_mut(..total).ok_or(Error::buffer_too_small(total))?;
        // Packet Length 0 until `finish` (API-023).
        header.with_packet_len(0).write_to(buf);
        Ok(EncapsulationWriter {
            out,
            config: *config,
            header,
            max_data_len,
        })
    }

    /// The data field to fill in place: `max_data_len` octets after the header (133.1 §4.1.3;
    /// API-020, GEN-014).
    pub fn data_mut(&mut self) -> &mut [u8] {
        let start = self.header.header_len().octets();
        let end = start.saturating_add(self.max_data_len);
        self.out.get_mut(start..end).unwrap_or_default()
    }

    /// Completes the packet with `data_len` octets of data; returns the total length (133.1
    /// §4.1.2.8.2; EPP-008, API-020, API-023).
    ///
    /// # Errors
    /// `OutOfRange` (`data_len`) if `data_len` exceeds the reservation or, for a non-idle EPI,
    /// the configured data unit limits (EPP-010); `InvalidField` (`epi`) for an empty data field
    /// with a non-idle EPI (EPP-005). On error the buffer keeps its unfinished state.
    pub fn finish(self, data_len: usize) -> Result<usize> {
        if data_len > self.max_data_len {
            return Err(Error::out_of_range(
                field::DATA_LEN,
                len_u64(self.max_data_len),
                len_u64(data_len),
            ));
        }
        if !self.header.epi().is_idle() {
            self.config.check_len(data_len)?;
            if data_len == 0 {
                return Err(invalid(field::EPI));
            }
        }
        // Fits: `begin` checked the header against `max_data_len` ≥ `data_len`.
        let total = self.header.header_len().octets().saturating_add(data_len);
        let packet_len = u32::try_from(total).unwrap_or(u32::MAX);
        self.header.with_packet_len(packet_len).write_to(self.out);
        Ok(total)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{EncapsulationWriter, encode, idle};
    use crate::config::{ANY, EpiSet, EppConfig, ExtendedEpiSet};
    use crate::field;
    use crate::header::{EncapsulationFields, Epi, HeaderLen, HeaderSize};
    use crate::view::EncapsulationPacket;
    use aldis_core::{ErrorDetail, ErrorKind};

    const MISSION: EncapsulationFields = EncapsulationFields::new(Epi::MISSION_SPECIFIC);

    #[test]
    fn writer_applies_the_managed_parameters() {
        // req: EPP-010
        let cfg = EppConfig::new(
            2,
            3,
            EpiSet::EMPTY.with(Epi::MISSION_SPECIFIC),
            ExtendedEpiSet::EMPTY,
        )
        .unwrap();
        let mut out = [0_u8; 16];
        let e = EncapsulationWriter::begin(
            &mut out,
            &cfg,
            EncapsulationFields::new(Epi::EXTENDED),
            HeaderSize::Auto,
            4,
        )
        .unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::UnknownChannel, Some(field::EPI))
        );
        for (len, expected) in [(1, 2), (4, 3)] {
            let w =
                EncapsulationWriter::begin(&mut out, &cfg, MISSION, HeaderSize::Auto, 4).unwrap();
            let e = w.finish(len).unwrap_err();
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected {
                    expected,
                    found: u64::try_from(len).unwrap()
                }
            );
        }
        let w = EncapsulationWriter::begin(&mut out, &cfg, MISSION, HeaderSize::Auto, 4).unwrap();
        assert_eq!(w.finish(3), Ok(5));
        // Idle packets carry no data unit: the configuration does not apply to them.
        let w = EncapsulationWriter::begin(
            &mut out,
            &cfg,
            EncapsulationFields::new(Epi::IDLE),
            HeaderSize::Auto,
            9,
        )
        .unwrap();
        assert_eq!(w.finish(9), Ok(11));
    }

    #[test]
    fn too_small_buffers_are_untouched() {
        // req: API-022, GEN-014
        let mut out = [0x55_u8; 4];
        let unit = ANY.data_unit(MISSION, &[1, 2, 3]).unwrap();
        let e = encode(&unit, HeaderSize::Auto, &mut out).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(5))
        );
        assert_eq!(out, [0x55; 4]);
        let e = idle(5, 0, &mut out).unwrap_err();
        assert_eq!(e.detail(), ErrorDetail::Needed(5));
        assert_eq!(out, [0x55; 4]);
        let e =
            EncapsulationWriter::begin(&mut out, &ANY, MISSION, HeaderSize::Auto, 3).unwrap_err();
        assert_eq!(e.detail(), ErrorDetail::Needed(5));
        assert_eq!(out, [0x55; 4]);
        assert_eq!(
            idle(0, 0, &mut out).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
    }

    #[test]
    fn unfinished_writer_leaves_an_invalid_packet() {
        // req: API-023, API-022
        for size in [HeaderLen::Two, HeaderLen::Four, HeaderLen::Eight] {
            let mut out = [0xFF_u8; 16];
            {
                let mut w =
                    EncapsulationWriter::begin(&mut out, &ANY, MISSION, HeaderSize::Fixed(size), 4)
                        .unwrap();
                w.data_mut().copy_from_slice(&[1, 2, 3, 4]);
            }
            let e = EncapsulationPacket::decode_lenient(&out).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::LengthMismatch, "{size:?}");
            assert_eq!(out[12..], [0xFF; 4], "nothing written past the reservation");
        }
    }

    #[test]
    fn writer_finishes_shorter_than_reserved() {
        // req: API-020, API-021
        let mut out = [0_u8; 300];
        let mut w =
            EncapsulationWriter::begin(&mut out, &ANY, MISSION, HeaderSize::Auto, 260).unwrap();
        assert_eq!(w.data_mut().len(), 260);
        w.data_mut()[0] = 0x42;
        assert_eq!(w.finish(1), Ok(5));
        // Auto sized the header for 260 octets: it stays 4 octets.
        assert_eq!(out[..5], [0xFE, 0x00, 0x00, 0x05, 0x42]);
    }

    #[test]
    fn writer_rejects_bad_finish() {
        // req: EPP-005, API-023
        let mut out = [0_u8; 8];
        let w = EncapsulationWriter::begin(&mut out, &ANY, MISSION, HeaderSize::Auto, 2).unwrap();
        let e = w.finish(3).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (
                ErrorKind::OutOfRange,
                ErrorDetail::Expected {
                    expected: 2,
                    found: 3
                }
            )
        );
        // A non-idle reservation of 0 octets could never finish: rejected up front (EPP-005).
        let e =
            EncapsulationWriter::begin(&mut out, &ANY, MISSION, HeaderSize::Auto, 0).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::EPI))
        );
        let w = EncapsulationWriter::begin(&mut out, &ANY, MISSION, HeaderSize::Auto, 2).unwrap();
        assert_eq!(w.finish(0).unwrap_err().kind(), ErrorKind::InvalidField);
        let idle_fields = EncapsulationFields::new(Epi::IDLE);
        let w =
            EncapsulationWriter::begin(&mut out, &ANY, idle_fields, HeaderSize::Auto, 0).unwrap();
        assert_eq!(w.finish(0), Ok(1));
        assert_eq!(out[0], 0xE0);
        let mut big = [0_u8; 3];
        assert_eq!(
            EncapsulationWriter::begin(
                &mut big,
                &ANY,
                MISSION,
                HeaderSize::Fixed(HeaderLen::One),
                1
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidField
        );
    }

    #[test]
    fn idle_fills_the_data_field() {
        // req: EPP-009, GEN-014
        let mut exact = [0_u8; 3];
        assert_eq!(
            idle(3, 0, &mut exact),
            Ok(3),
            "a buffer of exactly the packet length suffices"
        );
        let unit = ANY.data_unit(MISSION, &[7]).unwrap();
        assert_eq!(encode(&unit, HeaderSize::Auto, &mut exact), Ok(3));
        let mut out = [0_u8; 6];
        assert_eq!(idle(6, 0xA5, &mut out), Ok(6));
        assert_eq!(out, [0xE1, 0x06, 0xA5, 0xA5, 0xA5, 0xA5]);
    }
}
