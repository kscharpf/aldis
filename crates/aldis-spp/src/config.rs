//! Managed parameters of the Space Packet Protocol (133.0 §5.2 Table 5-1; SPP-005, SPP-006,
//! GEN-060, RS-012).
//!
//! The per-APID secondary header contents are not kept here: the caller looks them up and
//! passes a [`SecondaryHeaderLayout`](crate::secondary::SecondaryHeaderLayout) to
//! [`SpacePacket::split`](crate::view::SpacePacket::split) (OI-30).

use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::header::{
    MAX_PACKET_LEN, MIN_PACKET_LEN, PRIMARY_HEADER_LEN, PacketFields, invalid, len_u64,
};

/// The managed parameter Maximum Packet Length (133.0 Table 5-1, SPP-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SppConfig {
    max_packet_len: usize,
}

impl Default for SppConfig {
    fn default() -> SppConfig {
        SppConfig::DEFAULT
    }
}

impl SppConfig {
    /// The largest packet 133.0 allows, 65 542 octets (§4.1.2.2).
    pub const DEFAULT: SppConfig = SppConfig {
        max_packet_len: MAX_PACKET_LEN,
    };

    /// A configuration with packets of at most `max_packet_len` octets (SPP-006, GEN-060).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_packet_len`) outside 7–65 542 (133.0 §4.1.2.2).
    pub const fn new(max_packet_len: usize) -> Result<SppConfig> {
        if max_packet_len < MIN_PACKET_LEN || max_packet_len > MAX_PACKET_LEN {
            return Err(
                Error::new(ErrorKind::InvalidConfig).with_field(FieldId::new("max_packet_len"))
            );
        }
        Ok(SppConfig { max_packet_len })
    }

    /// Maximum Packet Length in octets (133.0 Table 5-1).
    #[must_use]
    pub const fn max_packet_len(&self) -> usize {
        self.max_packet_len
    }

    /// `OutOfRange` (`packet_len`) if a packet of `total` octets is longer than the maximum
    /// (SPP-006).
    pub(crate) fn check_len(self, total: usize) -> Result<()> {
        if total > self.max_packet_len {
            return Err(Error::out_of_range(
                "packet_len",
                len_u64(self.max_packet_len),
                len_u64(total),
            ));
        }
        Ok(())
    }

    /// Checks the parts of a packet against 133.0 and this configuration; the result is the only
    /// input [`encode`](crate::writer::encode) takes (RS-020). The Secondary Header Flag is set
    /// exactly when `secondary_header` is non-empty, so a flag without a secondary header cannot
    /// be expressed (SPP-005, API-021).
    ///
    /// # Errors
    /// - `InvalidField` (`data_field`) if both parts are empty (133.0 §4.1.4.1.2, SPP-005).
    /// - `InvalidField` (`secondary_header_flag`) for an Idle Packet with a secondary header
    ///   (§4.1.3.3.3.4).
    /// - `OutOfRange` (`packet_len`) above `max_packet_len` (SPP-006).
    pub fn packet<'a>(
        &self,
        fields: PacketFields,
        secondary_header: &'a [u8],
        user_data: &'a [u8],
    ) -> Result<PacketData<'a>> {
        let data_len = secondary_header.len().saturating_add(user_data.len());
        check_parts(fields, secondary_header.len(), data_len)?;
        self.check_len(data_len.saturating_add(PRIMARY_HEADER_LEN))?;
        Ok(PacketData {
            fields,
            secondary_header,
            user_data,
        })
    }
}

/// The 133.0 checks on a packet's parts that need no configuration (SPP-005).
pub(crate) const fn check_parts(
    fields: PacketFields,
    secondary_len: usize,
    data_len: usize,
) -> Result<()> {
    if data_len == 0 {
        return Err(invalid("data_field").with_detail(ErrorDetail::Expected {
            expected: 1,
            found: 0,
        }));
    }
    if fields.apid.is_idle() && secondary_len != 0 {
        return Err(invalid("secondary_header_flag"));
    }
    Ok(())
}

/// The parts of one packet, checked against 133.0 and an [`SppConfig`] (SPP-005, SPP-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketData<'a> {
    fields: PacketFields,
    secondary_header: &'a [u8],
    user_data: &'a [u8],
}

impl<'a> PacketData<'a> {
    /// The sender-chosen header fields.
    #[must_use]
    pub const fn fields(&self) -> PacketFields {
        self.fields
    }

    /// The Packet Secondary Header; empty when the packet has none (133.0 §4.1.4.2).
    #[must_use]
    pub const fn secondary_header(&self) -> &'a [u8] {
        self.secondary_header
    }

    /// The User Data Field; may be empty when a secondary header is present (133.0 §4.1.4.3).
    #[must_use]
    pub const fn user_data(&self) -> &'a [u8] {
        self.user_data
    }

    /// Total packet length in octets (133.0 §4.1.2.2).
    #[must_use]
    pub const fn packet_len(&self) -> usize {
        PRIMARY_HEADER_LEN
            .saturating_add(self.secondary_header.len())
            .saturating_add(self.user_data.len())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::SppConfig;
    use crate::header::{PacketFields, PacketType, SequenceFlags};
    use aldis_core::ids::{Apid, SeqCount};
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};

    fn fields(apid: u16) -> PacketFields {
        PacketFields {
            packet_type: PacketType::Telemetry,
            apid: Apid::new(apid).unwrap(),
            sequence_flags: SequenceFlags::Unsegmented,
            sequence_count: SeqCount::default(),
        }
    }

    #[test]
    fn max_packet_len_is_range_checked() {
        // req: SPP-006, GEN-060
        assert_eq!(SppConfig::default(), SppConfig::DEFAULT);
        assert_eq!(SppConfig::DEFAULT.max_packet_len(), 65_542);
        assert_eq!(SppConfig::new(7).unwrap().max_packet_len(), 7);
        for bad in [0, 6, 65_543] {
            let e = SppConfig::new(bad).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidConfig, Some("max_packet_len"))
            );
        }
    }

    #[test]
    fn packets_are_checked() {
        // req: SPP-005, SPP-006, SPP-004
        let cfg = SppConfig::new(10).unwrap();
        let p = cfg.packet(fields(1), &[1], &[2, 3, 4]).unwrap();
        assert_eq!(
            (
                p.fields(),
                p.secondary_header(),
                p.user_data(),
                p.packet_len()
            ),
            (fields(1), &[1][..], &[2, 3, 4][..], 10)
        );
        let e = cfg.packet(fields(1), &[], &[]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::InvalidField, Some("data_field"))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 1,
                found: 0
            }
        );
        let e = cfg.packet(fields(0x7FF), &[1], &[]).unwrap_err();
        assert_eq!(e.field().map(FieldId::name), Some("secondary_header_flag"));
        assert!(cfg.packet(fields(0x7FF), &[], &[0]).is_ok());
        let e = cfg.packet(fields(1), &[], &[0; 5]).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (
                ErrorKind::OutOfRange,
                ErrorDetail::Expected {
                    expected: 10,
                    found: 11
                }
            )
        );
    }
}
