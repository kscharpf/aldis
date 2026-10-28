//! Truncated Transfer Frames (732.1 Annex D; USLP-002, USLP-012, API-075, M2 plan D12).
//!
//! A truncated frame has a 4-octet Primary Header, the first six fields of the non-truncated
//! one (Annex D1.3.1), a 1-octet TFDF Header (D1.4.1.2), and no Insert Zone, OCF, or FECF
//! (D1.2.2 note 1). It has no length field: its length is the Truncated Transfer Frame Length
//! of its VC (D1.3.2), which is allowed only on a variable-length VC (D1.2.1), so its rule is
//! one of `011`–`111` (USLP-016). With SDLS it has a 6-octet Security Header and a 16-octet
//! Security Trailer (D1.2.3; USLP-012). It is processed as expedited user data (D1.3.1 note 3).
//! Its rule and UPID are exposed and not checked beyond USLP-016 (OI-35 item 1).

use aldis_core::layout::{FrameLayout, LayoutSpec};
use aldis_core::security::SecurityLayout;
use aldis_core::{AnomalySet, Error, ErrorKind};

use crate::anomaly::UslpAnomaly;
use crate::config::{VcLookup, frame_security};
use crate::field;
use crate::header::{ChannelFields, OID_VCID, TRUNCATED_HEADER_LEN, invalid, unknown};
use crate::tfdf::TfdfHeader;
use crate::validate::{Reason, Rejection};
use crate::view::{fail, internal, length, tfdf_rejection};

/// A decoded truncated USLP Transfer Frame borrowing its fields from the input (732.1 Annex
/// D; USLP-002, USLP-012, API-010, GEN-013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UslpTruncatedFrame<'a> {
    channel: ChannelFields,
    octets: &'a [u8],
    layout: FrameLayout,
    security_header: &'a [u8],
    tfdf_header: TfdfHeader,
    tfdz: &'a [u8],
    security_trailer: &'a [u8],
}

/// The Security Header and Trailer of a truncated frame on a VC or MAP whose layout is
/// `channel`: none, or the fixed 6/16 octets of SDLS (Annex D1.2.3; USLP-012, API-075).
pub(crate) const fn truncated_security(channel: SecurityLayout) -> SecurityLayout {
    if channel.is_none() {
        SecurityLayout::NONE
    } else {
        SecurityLayout::USLP_TRUNCATED
    }
}

/// The checks of a truncated frame after the SCID (`docs/standards/uslp-frame.md` §3.1).
pub(crate) fn parse<'a, L: VcLookup + ?Sized>(
    bytes: &'a [u8],
    channel: ChannelFields,
    channels: &L,
    exact: bool,
) -> Result<
    (
        UslpTruncatedFrame<'a>,
        crate::config::VcConfig,
        Option<crate::config::MapConfig>,
    ),
    Rejection,
> {
    let vc = channels
        .vc(channel.spacecraft_id, channel.vc_id)
        .ok_or(fail(Reason::InvalidVcid, unknown(field::VC_ID)))?;
    let map = if channel.vc_id.get() == OID_VCID {
        None
    } else {
        let m = channels
            .map(channel.spacecraft_id, channel.vc_id, channel.map_id)
            .ok_or(fail(Reason::InvalidMapId, unknown(field::MAP_ID)))?;
        Some(m)
    };
    let len = vc
        .truncated_frame_len()
        .ok_or(fail(
            Reason::TruncatedFrameNotAllowed,
            invalid(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
        ))?
        .get();
    let frame = match bytes.get(..len) {
        Some(frame) if !exact || bytes.len() == len => frame,
        _ => return Err(length(exact, len)),
    };
    let security = truncated_security(frame_security(&vc, map.as_ref()));
    let parts = split(frame, security)?;
    Ok((
        UslpTruncatedFrame {
            channel,
            octets: frame,
            layout: parts.layout,
            security_header: parts.security_header,
            tfdf_header: parts.tfdf_header,
            tfdz: parts.tfdz,
            security_trailer: parts.security_trailer,
        },
        vc,
        map,
    ))
}

/// A truncated frame cut at its layout.
struct Parts<'a> {
    layout: FrameLayout,
    security_header: &'a [u8],
    tfdf_header: TfdfHeader,
    tfdz: &'a [u8],
    security_trailer: &'a [u8],
}

/// Cuts a truncated `frame` with the SDLS lengths `security` at its layout and parses the TFDF
/// Header; an octet range outside the frame is a broken invariant (GEN-020).
fn split(frame: &[u8], security: SecurityLayout) -> Result<Parts<'_>, Rejection> {
    FrameLayout::compute(&LayoutSpec {
        frame_len: frame.len(),
        primary_header_len: TRUNCATED_HEADER_LEN,
        secondary_len: 0,
        security,
        ocf: false,
        fecf: false,
    })
    .ok()
    .ok_or(fail(
        Reason::FrameTooShort,
        invalid(field::FRAME_DATA_FIELD),
    ))
    .and_then(|layout| {
        frame
            .get(layout.security_header())
            .zip(frame.get(layout.data()))
            .zip(frame.get(layout.security_trailer()))
            .ok_or(internal())
            .and_then(|((security_header, data), security_trailer)| {
                TfdfHeader::parse(data, false)
                    .map(|(tfdf_header, tfdz)| Parts {
                        layout,
                        security_header,
                        tfdf_header,
                        tfdz,
                        security_trailer,
                    })
                    .map_err(tfdf_rejection)
            })
    })
}

impl<'a> UslpTruncatedFrame<'a> {
    /// The truncated Primary Header's fields (Annex D1.3.1; API-014).
    #[must_use]
    pub const fn channel(&self) -> ChannelFields {
        self.channel
    }

    /// The 4-octet truncated Primary Header (Annex D1.3; API-020).
    #[must_use]
    pub fn header_octets(&self) -> [u8; TRUNCATED_HEADER_LEN] {
        crate::header::start_octets(crate::header::VERSION, self.channel, true)
    }

    /// The Security Header: empty, or 6 octets with SDLS (Annex D1.2.3; API-072, API-075).
    #[must_use]
    pub const fn security_header(&self) -> &'a [u8] {
        self.security_header
    }

    /// The 1-octet TFDF Header, whose rule is `011`–`111` (Annex D1.4.1.2; USLP-016).
    #[must_use]
    pub const fn tfdf_header(&self) -> TfdfHeader {
        self.tfdf_header
    }

    /// The Transfer Frame Data Zone, 1–27 octets (Annex D1.4.2.4).
    #[must_use]
    pub const fn tfdz(&self) -> &'a [u8] {
        self.tfdz
    }

    /// The Security Trailer: empty, or 16 octets with SDLS (Annex D1.2.3; API-072, API-075).
    #[must_use]
    pub const fn security_trailer(&self) -> &'a [u8] {
        self.security_trailer
    }

    /// Where the fields lie in [`octets`](Self::octets): the 4-octet header, the Security Header,
    /// the TFDF, and the Security Trailer, with the Insert Zone, OCF, and FECF empty (Annex D1.2;
    /// CMN-040).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// The octets of the whole frame, borrowed from the input (GEN-013).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        self.octets
    }

    /// Octets consumed from the input: the VC's truncated frame length (Annex D1.3.2;
    /// API-011).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.octets.len()
    }

    /// Always `false`: a truncated frame has at least six octets (Annex D1.3.2 note 2).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Always empty: a truncated frame has no field a lenient decoder tolerates (API-032).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<UslpAnomaly> {
        AnomalySet::empty()
    }
}

/// `LengthMismatch` (`frame_data_field`) for a TFDZ that does not fill a truncated frame
/// exactly (USLP-002).
pub(crate) const fn wrong_tfdz_len() -> Error {
    Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_DATA_FIELD)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use crate::config::tests::variable;
    use crate::field;
    use crate::header::SourceOrDestination;
    use crate::tfdf::TfdzRule;
    use crate::upid::Upid;
    use crate::view::UslpFrame;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind};
    use std::vec;

    #[test]
    fn layout_places_the_fields() {
        // req: CMN-040, USLP-002
        let t = variable();
        let mut bytes = vec![0xC0, 0x15, 0x58, 0x85, 0xE5];
        bytes.extend(1..=7);
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        let l = f.layout();
        // The 4-octet header, the 1-octet TFDF Header and the TFDZ; no Insert Zone, OCF, or FECF.
        assert_eq!(
            l.ranges(),
            [0..4, 4..4, 4..4, 4..12, 12..12, 12..12, 12..12]
        );
        assert_eq!(f.truncated().unwrap().layout(), l);
    }

    #[test]
    fn decodes_truncated_frames() {
        // req: USLP-002, API-011, API-013
        let t = variable();
        // VC 4 MAP 2, destination, truncated: 12 octets, rule 111, UPID 5.
        let mut bytes = vec![0xC0, 0x15, 0x58, 0x85, 0xE5];
        bytes.extend(1..=7);
        bytes.extend_from_slice(&[0xEE; 3]);
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        assert!(f.is_truncated() && f.full().is_none());
        let tr = f.truncated().unwrap();
        assert_eq!((tr.len(), f.len(), tr.is_empty()), (12, 12, false));
        assert_eq!(f.source_or_destination(), SourceOrDestination::Destination);
        assert_eq!((f.vc_id().get(), f.map_id().get()), (4, 2));
        assert_eq!(tr.tfdf_header().rule, TfdzRule::NoSegmentation);
        assert_eq!(f.tfdf_header().upid, Upid::MissionSpecificInformation1);
        assert_eq!(
            (tr.tfdz(), f.tfdz()),
            (&[1, 2, 3, 4, 5, 6, 7][..], &[1, 2, 3, 4, 5, 6, 7][..])
        );
        assert!(tr.security_header().is_empty() && tr.security_trailer().is_empty());
        assert!(f.security_header().is_empty() && f.security_trailer().is_empty());
        assert!(f.anomalies().is_empty() && tr.anomalies().is_empty());
        assert_eq!(tr.header_octets(), bytes[..4]);
        assert_eq!((tr.octets(), f.octets()), (&bytes[..12], &bytes[..12]));
        assert_eq!(tr.channel(), f.channel());
        assert_eq!(UslpFrame::peek_length(&bytes[..4], &t), Ok(12));
        let e = UslpFrame::decode(&bytes[..11], &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(12))
        );
    }

    #[test]
    fn validating_truncated_frames() {
        // req: USLP-014, USLP-002
        use crate::validate::{Reason, validate_frame};
        let t = variable();
        let mut bytes = vec![0xC0, 0x15, 0x58, 0x85, 0xE5];
        bytes.extend(1..=7);
        assert_eq!(
            validate_frame(&bytes, &t, DecodeMode::Strict).map(|f| f.len()),
            Ok(12)
        );
        bytes.push(0);
        let r = validate_frame(&bytes, &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(r.reason(), Reason::LengthMismatch);
        assert_eq!(r.error().field(), Some(field::FRAME_LEN));
    }

    #[test]
    fn truncated_sdls_layout() {
        // req: USLP-012, API-075, API-072
        let t = variable();
        // VC 5 MAP 1 (SDLS per MAP): 28 octets, 6 + 2 + 16.
        let mut bytes = vec![0xC0, 0x15, 0x50, 0xA3];
        bytes.extend_from_slice(&[0x51; 6]);
        bytes.extend_from_slice(&[0x60, 0x99]);
        bytes.extend_from_slice(&[0x52; 16]);
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        assert_eq!(
            (f.security_header(), f.security_trailer()),
            (&[0x51; 6][..], &[0x52; 16][..])
        );
        assert_eq!(
            (f.tfdz(), f.tfdf_header().rule),
            (&[0x99][..], TfdzRule::OctetStream)
        );
    }

    #[test]
    fn truncated_errors() {
        // req: USLP-002, USLP-016, USLP-014
        let t = variable();
        let fe = |b: &[u8]| {
            let e = UslpFrame::decode(b, &t, DecodeMode::Strict).unwrap_err();
            (e.kind(), e.field())
        };
        let mut rule = vec![0xC0, 0x15, 0x58, 0x85, 0x05];
        rule.extend_from_slice(&[0; 7]);
        assert_eq!(
            fe(&rule),
            (ErrorKind::InvalidField, Some(field::TFDZ_CONSTRUCTION_RULE))
        );
        // VC 6 has no truncated frame length; VC 7 does not exist; VC 4 MAP 1 does not.
        assert_eq!(
            fe(&[0xC0, 0x15, 0x50, 0xC1, 0xE0, 0, 0, 0, 0, 0, 0, 0]),
            (
                ErrorKind::InvalidField,
                Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG)
            )
        );
        assert_eq!(
            fe(&[0xC0, 0x15, 0x50, 0xE1, 0xE0, 0]),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        assert_eq!(
            fe(&[0xC0, 0x15, 0x50, 0x83, 0xE0, 0]),
            (ErrorKind::UnknownChannel, Some(field::MAP_ID))
        );
        let peek = |b: &[u8]| UslpFrame::peek_length(b, &t).unwrap_err().field();
        assert_eq!(
            peek(&[0xC0, 0x15, 0x50, 0xC1]),
            Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG)
        );
        assert_eq!(peek(&[0xC0, 0x15, 0x50, 0xE1]), Some(field::VC_ID));
        assert_eq!(peek(&[0xC0, 0x16, 0x50, 0x81]), Some(field::SPACECRAFT_ID));
        // VCID 63 has no MAP channel and no truncated frames (USLP-016).
        let f = crate::config::tests::fixed();
        let e = UslpFrame::decode(&[0xC0, 0x15, 0x57, 0xE1, 0x20, 0], &f, DecodeMode::Strict);
        assert_eq!(
            e.map_err(|e| e.field()),
            Err(Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG))
        );
        assert_eq!(
            super::wrong_tfdz_len().field(),
            Some(field::FRAME_DATA_FIELD)
        );
    }
}
