//! The frame format of TC Transfer Frames: their geometry for the engines (232.0-B-4 §4.1;
//! LNK-001, GEN-003).

use aldis_core::FrameFormat;

use crate::header::PRIMARY_HEADER_LEN;

/// The frame format of TC Transfer Frames, the marker the link engines are generic over
/// (LNK-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcFormat;

impl FrameFormat for TcFormat {
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
    use super::{FrameFormat, TcFormat};

    // req: LNK-001
    #[test]
    fn geometry_is_that_of_232_0() {
        assert_eq!(TcFormat::PRIMARY_HEADER_LEN, 5);
        assert_eq!(TcFormat::primary_header_len(&()), 5);
        assert_eq!(TcFormat::PACKET_HEADER_LEN, 0);
        assert_eq!(TcFormat::SDU_HEADER_LEN, 0);
    }
}
