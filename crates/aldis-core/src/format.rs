//! The frame format of a codec: the geometry its engines share (LNK-001, GEN-003).
//!
//! Each frame codec (`aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`) implements
//! [`FrameFormat`] for one marker type and states there the lengths of its frame: the Primary
//! Header and the part the managed parameters add to it, and the data field headers that the
//! engines put before the user data. The link engines are generic over a protocol trait that has
//! the format as a supertrait, so no engine restates a length of the standard (M6 plan WP-2a).
//! The header writers and parsers, the managed-parameter sets, and the frame writer are later
//! members of the same trait (WP-2b to WP-2d).

use core::fmt::Debug;

/// The geometry of the frames of one CCSDS data link protocol (LNK-001).
///
/// The marker type that implements it carries no data; the managed parameters that vary a length
/// are the [`HeaderExtension`](Self::HeaderExtension) the caller passes in.
pub trait FrameFormat: Copy + Debug + Eq + 'static {
    /// The managed parameter that adds octets to the Primary Header: nothing for TM and TC, the
    /// presence of the FHEC for AOS, the VCF Count Length for USLP.
    type HeaderExtension: Copy + Debug + Eq;

    /// The octets of the Primary Header that every frame of the protocol has, without the part
    /// of [`HeaderExtension`](Self::HeaderExtension): 132.0 §4.1.2 (6); 232.0 §4.1.2 (5); 732.0
    /// §4.1.2 (6); 732.1 §4.1.2 (7).
    const PRIMARY_HEADER_LEN: usize;

    /// The octets of the data field header that precedes the Packet Zone of a data field that
    /// carries packets: the `M_PDU` header of AOS (732.0 §4.1.4.2.1), the TFDF Header of USLP
    /// (732.1 §4.1.4.2); none in TM (the pointer is in the Primary Header) and TC.
    const PACKET_HEADER_LEN: usize;

    /// The octets of the data field header that precedes an SDU of a Virtual Channel Access
    /// service, and the data of an OID frame: the TFDF Header of USLP (732.1 §4.1.4.2); none in
    /// TM, TC and AOS, whose SDU is the whole data field.
    const SDU_HEADER_LEN: usize;

    /// The octets that `extension` adds to the Primary Header.
    fn extension_len(extension: &Self::HeaderExtension) -> usize;

    /// The octets of the Primary Header of a frame whose managed parameters give `extension`.
    #[must_use]
    fn primary_header_len(extension: &Self::HeaderExtension) -> usize {
        Self::PRIMARY_HEADER_LEN.saturating_add(Self::extension_len(extension))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::FrameFormat;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Marker;

    impl FrameFormat for Marker {
        type HeaderExtension = usize;
        const PRIMARY_HEADER_LEN: usize = 6;
        const PACKET_HEADER_LEN: usize = 0;
        const SDU_HEADER_LEN: usize = 0;

        fn extension_len(extension: &usize) -> usize {
            *extension
        }
    }

    #[test]
    fn primary_header_len_adds_the_extension() {
        // req: LNK-001
        assert_eq!(Marker::primary_header_len(&0), 6);
        assert_eq!(Marker::primary_header_len(&2), 8);
    }

    #[test]
    fn primary_header_len_saturates() {
        // req: LNK-001
        assert_eq!(Marker::primary_header_len(&usize::MAX), usize::MAX);
    }
}
