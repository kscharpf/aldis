//! The frame format of AOS Transfer Frames: their geometry for the engines (732.0-B-5 §4.1;
//! LNK-001, GEN-003).

use aldis_core::format::put_header;
use aldis_core::{FrameFormat, Result};

use crate::header::{AosPrimaryHeader, FHEC_LEN, PRIMARY_HEADER_LEN};
use crate::mpdu::MPDU_HEADER_LEN;

/// The frame format of AOS Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosFormat;

impl FrameFormat for AosFormat {
    /// Whether the Primary Header has the Frame Header Error Control (732.0 §4.1.1.1 a, §4.1.2.6).
    type HeaderExtension = bool;
    type Physical = crate::config::AosPhysicalChannelConfig;
    const PRIMARY_HEADER_LEN: usize = PRIMARY_HEADER_LEN;
    const PACKET_HEADER_LEN: usize = MPDU_HEADER_LEN;
    const SDU_HEADER_LEN: usize = 0;

    type PrimaryHeader = AosPrimaryHeader;

    fn extension_len(fhec: &bool) -> usize {
        if *fhec { FHEC_LEN } else { 0 }
    }

    fn write_primary_header(header: &AosPrimaryHeader, out: &mut [u8]) -> Result<()> {
        put_header(&header.to_octets(), header.len(), out)
    }

    fn write_oid_data_header(_: usize, _: &mut [u8]) {}
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{AosFormat, FrameFormat};
    use crate::config::AosEdition;
    use crate::header::{AosPrimaryHeader, FrameCountCycle, VcFrameCount};
    use aldis_core::ErrorKind;
    use aldis_core::ids::{SpacecraftId, VcId};

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_732_0() {
        assert_eq!(AosFormat::PRIMARY_HEADER_LEN, 6);
        assert_eq!(AosFormat::primary_header_len(&false), 6);
        assert_eq!(AosFormat::primary_header_len(&true), 8);
        assert_eq!(AosFormat::PACKET_HEADER_LEN, 2);
        assert_eq!(AosFormat::SDU_HEADER_LEN, 0);
    }

    // req: LNK-001, AOS-002, AOS-019, GEN-052
    #[test]
    fn the_writer_puts_the_unsealed_header_octets_first() {
        let (count, cycle) = VcFrameCount::wrapping(0x05AB_CDEF);
        assert_eq!(count.get(), 0x00AB_CDEF);
        assert_eq!(cycle.get(), 5);
        let scid = SpacecraftId::new(0x2A5).unwrap();
        let vcid = VcId::new(9).unwrap();
        let plain = AosPrimaryHeader::unsealed(AosEdition::Issue5, scid, vcid, count, None, false);
        let mut out = [0xEE_u8; 9];
        AosFormat::write_primary_header(&plain, &mut out).unwrap();
        assert_eq!(out, [0x69, 0x49, 0xAB, 0xCD, 0xEF, 0x20, 0xEE, 0xEE, 0xEE]);
        let sealed = AosPrimaryHeader::unsealed(
            AosEdition::Issue4Compatible,
            SpacecraftId::new(0xA5).unwrap(),
            vcid,
            count,
            Some(cycle),
            true,
        );
        AosFormat::write_primary_header(&sealed, &mut out).unwrap();
        assert_eq!(out, [0x69, 0x49, 0xAB, 0xCD, 0xEF, 0x45, 0x00, 0x00, 0xEE]);
        let e = AosFormat::write_primary_header(&sealed, &mut out[..7]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(FrameCountCycle::new(5), Some(cycle));
        AosFormat::write_oid_data_header(9, &mut []);
    }
}
