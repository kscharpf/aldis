//! The frame format of USLP Transfer Frames: their geometry for the engines (732.1-B-3 §4.1;
//! LNK-001, GEN-003).

use aldis_core::FrameFormat;

use crate::header::{PRIMARY_HEADER_LEN, VcfCountLen};
use crate::tfdf::FIXED_TFDF_HEADER_LEN;

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

    fn extension_len(count: &VcfCountLen) -> usize {
        count.octets()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameFormat, UslpFormat};
    use crate::header::VcfCountLen;

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
}
