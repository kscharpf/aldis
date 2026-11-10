//! The frame format of AOS Transfer Frames: their geometry for the engines (732.0-B-5 §4.1;
//! LNK-001, GEN-003).

use aldis_core::FrameFormat;

use crate::header::{FHEC_LEN, PRIMARY_HEADER_LEN};
use crate::mpdu::MPDU_HEADER_LEN;

/// The frame format of AOS Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosFormat;

impl FrameFormat for AosFormat {
    /// Whether the Primary Header has the Frame Header Error Control (732.0 §4.1.1.1 a, §4.1.2.6).
    type HeaderExtension = bool;
    const PRIMARY_HEADER_LEN: usize = PRIMARY_HEADER_LEN;
    const PACKET_HEADER_LEN: usize = MPDU_HEADER_LEN;
    const SDU_HEADER_LEN: usize = 0;

    fn extension_len(fhec: &bool) -> usize {
        if *fhec { FHEC_LEN } else { 0 }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{AosFormat, FrameFormat};

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_732_0() {
        assert_eq!(AosFormat::PRIMARY_HEADER_LEN, 6);
        assert_eq!(AosFormat::primary_header_len(&false), 6);
        assert_eq!(AosFormat::primary_header_len(&true), 8);
        assert_eq!(AosFormat::PACKET_HEADER_LEN, 2);
        assert_eq!(AosFormat::SDU_HEADER_LEN, 0);
    }
}
