//! The frame format of TM Transfer Frames: their geometry for the engines (132.0-B-3 §4.1;
//! LNK-001, GEN-003).

use aldis_core::format::put_header;
use aldis_core::{FrameFormat, Result};

use crate::header::{PRIMARY_HEADER_LEN, TmPrimaryHeader};

/// The frame format of TM Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmFormat;

impl FrameFormat for TmFormat {
    type HeaderExtension = ();
    type Physical = crate::config::TmPhysicalChannelConfig;
    const PRIMARY_HEADER_LEN: usize = PRIMARY_HEADER_LEN;
    const PACKET_HEADER_LEN: usize = 0;
    const SDU_HEADER_LEN: usize = 0;

    type PrimaryHeader = TmPrimaryHeader;

    fn extension_len((): &()) -> usize {
        0
    }

    fn write_primary_header(header: &TmPrimaryHeader, out: &mut [u8]) -> Result<()> {
        put_header(&header.to_octets(), PRIMARY_HEADER_LEN, out)
    }

    fn write_oid_data_header(_: usize, _: &mut [u8]) {}
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameFormat, TmFormat};
    use crate::header::TmPrimaryHeader;
    use aldis_core::ErrorKind;
    use aldis_core::ids::{SpacecraftId, VcId};

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_132_0() {
        assert_eq!(TmFormat::PRIMARY_HEADER_LEN, 6);
        assert_eq!(TmFormat::primary_header_len(&()), 6);
        assert_eq!(TmFormat::PACKET_HEADER_LEN, 0);
        assert_eq!(TmFormat::SDU_HEADER_LEN, 0);
    }

    // req: LNK-001, GEN-052
    #[test]
    fn the_writer_puts_the_header_octets_first() {
        let header = TmPrimaryHeader::decode(&[0x1A, 0xB7, 0x12, 0x34, 0x18, 0x05]).unwrap();
        assert_eq!(header.scid, SpacecraftId::new(0x1AB).unwrap());
        assert_eq!(header.vcid, VcId::new(3).unwrap());
        let mut out = [0xEE_u8; 8];
        TmFormat::write_primary_header(&header, &mut out).unwrap();
        assert_eq!(out, [0x1A, 0xB7, 0x12, 0x34, 0x18, 0x05, 0xEE, 0xEE]);
        let e = TmFormat::write_primary_header(&header, &mut out[..5]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        let mut none = [];
        TmFormat::write_oid_data_header(9, &mut none);
    }
}
