//! The frame format of USLP Transfer Frames: their geometry for the engines (732.1-B-3 §4.1;
//! LNK-001, GEN-003).

use aldis_core::format::put_header;
use aldis_core::{FrameFormat, Result};

use crate::header::{PRIMARY_HEADER_LEN, UslpPrimaryHeader, VcfCountLen};
use crate::tfdf::{FIXED_TFDF_HEADER_LEN, TfdfHeader};

/// The frame format of USLP Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UslpFormat;

impl FrameFormat for UslpFormat {
    /// The length of the VCF Count field (732.1 §4.1.2.11).
    type HeaderExtension = VcfCountLen;
    const PRIMARY_HEADER_LEN: usize = PRIMARY_HEADER_LEN;
    const PACKET_HEADER_LEN: usize = FIXED_TFDF_HEADER_LEN;
    const SDU_HEADER_LEN: usize = FIXED_TFDF_HEADER_LEN;

    type PrimaryHeader = UslpPrimaryHeader;

    fn extension_len(count: &VcfCountLen) -> usize {
        count.octets()
    }

    fn write_primary_header(header: &UslpPrimaryHeader, out: &mut [u8]) -> Result<()> {
        put_header(&header.octets(), header.len(), out)
    }

    fn write_oid_data_header(data_len: usize, out: &mut [u8]) {
        for (d, s) in out
            .iter_mut()
            .zip(TfdfHeader::only_idle_data(data_len).octets())
        {
            *d = s;
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameFormat, UslpFormat};
    use crate::header::VcfCountLen;
    use crate::header::{
        ChannelFields, FrameKind, SourceOrDestination, UslpPrimaryHeader, VcfCount,
    };
    use crate::tfdf::TfdfHeader;
    use aldis_core::ErrorKind;

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_732_1() {
        assert_eq!(UslpFormat::PRIMARY_HEADER_LEN, 7);
        assert_eq!(UslpFormat::primary_header_len(&VcfCountLen::NONE), 7);
        let four = VcfCountLen::new(4).unwrap();
        assert_eq!(UslpFormat::primary_header_len(&four), 11);
        assert_eq!(UslpFormat::PACKET_HEADER_LEN, 3);
        assert_eq!(UslpFormat::SDU_HEADER_LEN, 3);
    }

    // req: LNK-001, USLP-001, USLP-009, GEN-052
    #[test]
    fn the_writer_puts_the_header_octets_first() {
        let channel = ChannelFields {
            spacecraft_id: crate::UslpSpacecraftId::new(0xF0A5).unwrap(),
            source_or_destination: SourceOrDestination::Destination,
            vc_id: crate::OID_VC,
            map_id: crate::UslpMapId::new(0).unwrap(),
        };
        let two = VcfCountLen::new(2).unwrap();
        let header = UslpPrimaryHeader::for_writer(
            channel,
            100,
            FrameKind::Expedited,
            true,
            VcfCount::wrapping(two, 0x1_0203),
        )
        .unwrap();
        let mut out = [0xEE_u8; 10];
        UslpFormat::write_primary_header(&header, &mut out).unwrap();
        assert_eq!(
            &out[..9],
            &[0xCF, 0x0A, 0x5F, 0xE0, 0x00, 0x63, 0x8A, 0x02, 0x03]
        );
        assert_eq!(out[9], 0xEE);
        let e = UslpFormat::write_primary_header(&header, &mut out[..8]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        let mut tfdf = [0xEE_u8; 4];
        UslpFormat::write_oid_data_header(20, &mut tfdf);
        assert_eq!(&tfdf[..3], &TfdfHeader::only_idle_data(20).octets());
        for frame_len in [0, crate::MAX_FRAME_LEN + 1] {
            let e = UslpPrimaryHeader::for_writer(
                channel,
                frame_len,
                FrameKind::Expedited,
                false,
                VcfCount::wrapping(two, 0),
            )
            .unwrap_err();
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert_eq!(e.field(), Some(crate::field::FRAME_LEN));
        }
        assert!(
            UslpPrimaryHeader::for_writer(
                channel,
                crate::MAX_FRAME_LEN,
                FrameKind::Expedited,
                false,
                VcfCount::wrapping(two, 0),
            )
            .is_ok()
        );
        assert_eq!(tfdf[3], 0xEE);
    }
}
