//! The frame format of TM Transfer Frames: their geometry for the engines (132.0-B-3 §4.1;
//! LNK-001, GEN-003).

use aldis_core::FrameFormat;

use crate::header::PRIMARY_HEADER_LEN;

/// The frame format of TM Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmFormat;

impl FrameFormat for TmFormat {
    type HeaderExtension = ();
    const PRIMARY_HEADER_LEN: usize = PRIMARY_HEADER_LEN;
    const PACKET_HEADER_LEN: usize = 0;
    const SDU_HEADER_LEN: usize = 0;

    fn extension_len((): &()) -> usize {
        0
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameFormat, TmFormat};

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_132_0() {
        assert_eq!(TmFormat::PRIMARY_HEADER_LEN, 6);
        assert_eq!(TmFormat::primary_header_len(&()), 6);
        assert_eq!(TmFormat::PACKET_HEADER_LEN, 0);
        assert_eq!(TmFormat::SDU_HEADER_LEN, 0);
    }
}
