//! The frame format of a codec: the geometry its engines share (LNK-001, GEN-003).
//!
//! Each frame codec (`aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`) implements
//! [`FrameFormat`] for one marker type and states there the lengths of its frame: the Primary
//! Header and the part the managed parameters add to it, and the data field headers that the
//! engines put before the user data. The link engines are generic over a protocol trait that has
//! the format as a supertrait, so no engine restates a length of the standard (M6 plan WP-2a).
//! The Primary Header writer and the header of an Only Idle Data frame are members of the same
//! trait (WP-2b); the managed-parameter sets and the frame writer are later (WP-2c, WP-2d).

use core::fmt::Debug;

use crate::error::{Error, Result};

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

    /// The Primary Header as plain values, as the codec's decoder reports it and its writers take
    /// it (API-014); its fields carry the extension, so a header knows its own length.
    type PrimaryHeader: Copy + Debug + Eq;

    /// The octets that `extension` adds to the Primary Header.
    fn extension_len(extension: &Self::HeaderExtension) -> usize;

    /// The octets of the Primary Header of a frame whose managed parameters give `extension`.
    #[must_use]
    fn primary_header_len(extension: &Self::HeaderExtension) -> usize {
        Self::PRIMARY_HEADER_LEN.saturating_add(Self::extension_len(extension))
    }

    /// Writes `header` into the first octets of `out`: its version, its reserved spares zero, and
    /// an error-control field the header has (the AOS FHEC) as the header holds it; no other
    /// octet of `out` changes (GEN-052).
    ///
    /// # Errors
    /// `BufferTooSmall` if `out` is shorter than the header, with its length; `out` is then
    /// unchanged.
    fn write_primary_header(header: &Self::PrimaryHeader, out: &mut [u8]) -> Result<()>;

    /// Writes the data field header of an Only Idle Data frame whose data field has `data_len`
    /// octets into the first [`SDU_HEADER_LEN`](Self::SDU_HEADER_LEN) octets of `out`: the TFDF
    /// Header of rule `001` and UPID "Only Idle Data" (732.1 §4.1.4.1.9–11); nothing for TM and
    /// AOS, whose OID frame has no such header (132.0 §4.1.4.6; 732.0 §4.1.4.1).
    fn write_oid_data_header(data_len: usize, out: &mut [u8]);
}

/// Copies the header `octets`, of which the first `len` are the header, to the start of `out`
/// (the body of [`FrameFormat::write_primary_header`] for each codec).
///
/// # Errors
/// `BufferTooSmall` if `out` is shorter than `len`; `out` is then unchanged.
pub fn put_header(octets: &[u8], len: usize, out: &mut [u8]) -> Result<()> {
    let dest = out
        .get_mut(..len)
        .ok_or_else(|| Error::buffer_too_small(len))?;
    for (d, s) in dest.iter_mut().zip(octets) {
        *d = *s;
    }
    Ok(())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameFormat, put_header};
    use crate::error::{ErrorDetail, ErrorKind, Result};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Two;

    impl FrameFormat for Two {
        type HeaderExtension = bool;
        type PrimaryHeader = ();
        const PRIMARY_HEADER_LEN: usize = 5;
        const PACKET_HEADER_LEN: usize = 0;
        const SDU_HEADER_LEN: usize = 0;

        fn extension_len(extension: &bool) -> usize {
            if *extension { 2 } else { 0 }
        }

        fn write_primary_header((): &(), _: &mut [u8]) -> Result<()> {
            Ok(())
        }

        fn write_oid_data_header(_: usize, _: &mut [u8]) {}
    }

    /// A format whose extension is a length, which can reach `usize::MAX`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Marker;

    impl FrameFormat for Marker {
        type HeaderExtension = usize;
        type PrimaryHeader = ();
        const PRIMARY_HEADER_LEN: usize = 6;
        const PACKET_HEADER_LEN: usize = 0;
        const SDU_HEADER_LEN: usize = 0;

        fn extension_len(extension: &usize) -> usize {
            *extension
        }

        fn write_primary_header((): &(), _: &mut [u8]) -> Result<()> {
            Ok(())
        }

        fn write_oid_data_header(_: usize, _: &mut [u8]) {}
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

    #[test]
    fn the_header_length_adds_the_extension() {
        // req: LNK-001
        assert_eq!(Two::primary_header_len(&false), 5);
        assert_eq!(Two::primary_header_len(&true), 7);
        assert!(Two::write_primary_header(&(), &mut []).is_ok());
        Two::write_oid_data_header(0, &mut []);
    }

    #[test]
    fn a_header_goes_to_the_start_of_the_buffer_or_nowhere() {
        // req: LNK-001, GEN-052
        let mut out = [0xEE_u8; 5];
        put_header(&[1, 2, 3, 4], 3, &mut out).unwrap();
        assert_eq!(out, [1, 2, 3, 0xEE, 0xEE]);
        let e = put_header(&[1, 2, 3, 4], 4, &mut out[..3]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(e.detail(), ErrorDetail::Needed(4));
        assert_eq!(out, [1, 2, 3, 0xEE, 0xEE]);
    }
}
