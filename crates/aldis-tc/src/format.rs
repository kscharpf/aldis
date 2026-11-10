//! The frame format of TC Transfer Frames: their geometry for the engines (232.0-B-4 §4.1;
//! LNK-001, GEN-003).

use aldis_core::format::put_header;
use aldis_core::{FrameFormat, Result};

use crate::header::{PRIMARY_HEADER_LEN, TcFrameHeader};

/// The frame format of TC Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcFormat;

impl FrameFormat for TcFormat {
    type HeaderExtension = ();
    const PRIMARY_HEADER_LEN: usize = PRIMARY_HEADER_LEN;
    const PACKET_HEADER_LEN: usize = 0;
    const SDU_HEADER_LEN: usize = 0;

    type PrimaryHeader = TcFrameHeader;

    fn extension_len((): &()) -> usize {
        0
    }

    fn write_primary_header(header: &TcFrameHeader, out: &mut [u8]) -> Result<()> {
        put_header(&header.primary_header_octets(), PRIMARY_HEADER_LEN, out)
    }

    fn write_oid_data_header(_: usize, _: &mut [u8]) {}
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameFormat, TcFormat};
    use crate::header::{FrameFields, FrameType, TcFrameHeader};
    use aldis_core::ErrorKind;
    use aldis_core::ids::FrameSeqNum;

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_232_0() {
        assert_eq!(TcFormat::PRIMARY_HEADER_LEN, 5);
        assert_eq!(TcFormat::primary_header_len(&()), 5);
        assert_eq!(TcFormat::PACKET_HEADER_LEN, 0);
        assert_eq!(TcFormat::SDU_HEADER_LEN, 0);
    }

    // req: LNK-001, TC-001, GEN-052
    #[test]
    fn the_writer_puts_the_header_octets_first() {
        let fields = FrameFields {
            frame_type: FrameType::Bd,
            spacecraft_id: crate::TcSpacecraftId::new(0x2A5).unwrap(),
            vc_id: crate::TcVcId::new(0x15).unwrap(),
            sequence_number: FrameSeqNum::new(7),
        };
        let header = TcFrameHeader::new(fields, 20, None);
        let mut out = [0xEE_u8; 6];
        TcFormat::write_primary_header(&header, &mut out).unwrap();
        assert_eq!(out, [0x22, 0xA5, 0x54, 0x13, 0x07, 0xEE]);
        let e = TcFormat::write_primary_header(&header, &mut out[..4]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        TcFormat::write_oid_data_header(9, &mut []);
    }
}
