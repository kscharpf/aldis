//! Zero-copy USLP frame views and the decoder they share (732.1 §4.1, §6.3, Annex D; USLP-001,
//! USLP-003…USLP-007, USLP-014, USLP-016, API-010…API-014, API-072, GEN-013, GEN-043).
//!
//! [`UslpFrame::decode`] returns a [`UslpFullFrame`] or, when the End of Frame Primary Header
//! Flag is 1, a [`UslpTruncatedFrame`] (M2 plan D12); the enum's accessors cover the fields the
//! two share (Annex D1.3.1 note 2). The checks, their order, and their errors are those of
//! `docs/standards/uslp-frame.md` §3.1.

use core::ops::Range;

use aldis_core::crc;
use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, FieldId, Result};

use crate::anomaly::UslpAnomaly;
use crate::config::{MapConfig, OcfUse, VcConfig, VcLookup, frame_layout, frame_security};
use crate::field;
use crate::header::{
    ChannelFields, FrameKind, OID_VCID, PRIMARY_HEADER_LEN, RawRest, RawStart, SourceOrDestination,
    TRUNCATED_HEADER_LEN, UslpMapId, UslpPrimaryHeader, UslpSpacecraftId, UslpVcId, VcfCount,
    invalid, len_u64, mismatch, read_vcf_count, unknown,
};
use crate::tfdf::{LastValidOctet, TfdfHeader, TfdzRule};
use crate::truncated::{self, UslpTruncatedFrame};
use crate::upid::Upid;
use crate::validate::{Reason, Rejection};

/// Shorthand for a failed check.
pub(crate) const fn fail(reason: Reason, error: Error) -> Rejection {
    Rejection::new(reason, error)
}

/// Check (d) on fewer octets than `len`: `Incomplete` (`needed` = `len`) while decoding, or
/// `LengthMismatch` (`frame_len`) when exactly one frame is required (§4.3.11.3.4 d).
pub(crate) const fn length(exact: bool, len: usize) -> Rejection {
    let error = if exact {
        mismatch(field::FRAME_LEN)
    } else {
        Error::incomplete(len)
    };
    fail(Reason::LengthMismatch, error)
}

/// The fields of a non-truncated frame, cut at its layout.
struct FullParts<'a> {
    insert_zone: &'a [u8],
    security_header: &'a [u8],
    tfdf_header: TfdfHeader,
    tfdz: &'a [u8],
    security_trailer: &'a [u8],
    ocf: &'a [u8],
    fecf: &'a [u8],
}

/// Cuts `frame` at `layout` and parses the TFDF Header of the data field.
///
/// A layout computed for the frame's length always lies inside the frame, so a missing range is
/// a broken invariant, reported as `InternalError` instead of a silently empty field (GEN-020).
fn split_full<'a>(
    frame: &'a [u8],
    layout: &FrameLayout,
    fixed_tfdz: bool,
) -> core::result::Result<FullParts<'a>, Rejection> {
    let at = |r: Range<usize>| frame.get(r);
    at(layout.secondary())
        .zip(at(layout.security_header()))
        .zip(at(layout.data()))
        .zip(at(layout.security_trailer()))
        .zip(at(layout.ocf()))
        .zip(at(layout.fecf()))
        .ok_or(internal())
        .and_then(
            |(((((insert_zone, security_header), data), security_trailer), ocf), fecf)| {
                TfdfHeader::parse(data, fixed_tfdz)
                    .map(|(tfdf_header, tfdz)| FullParts {
                        insert_zone,
                        security_header,
                        tfdf_header,
                        tfdz,
                        security_trailer,
                        ocf,
                        fecf,
                    })
                    .map_err(tfdf_rejection)
            },
        )
}

/// The rejection for a layout that does not lie inside its frame: a broken invariant.
pub(crate) const fn internal() -> Rejection {
    fail(
        Reason::LengthMismatch,
        Error::new(ErrorKind::InternalError).with_field(field::FRAME_LEN),
    )
}

/// The reason of a TFDF Header error (USLP-006, USLP-016, CMN-041).
pub(crate) fn tfdf_rejection(e: Error) -> Rejection {
    let reason = if e.kind() == ErrorKind::Malformed {
        Reason::PointerBeyondZone
    } else if e.field() == Some(field::TFDZ_CONSTRUCTION_RULE) {
        Reason::ConstructionRuleMismatch
    } else {
        Reason::FrameTooShort
    };
    fail(reason, e)
}

/// A decoded non-truncated USLP Transfer Frame borrowing its fields from the input (732.1
/// §4.1, §6.3; USLP-001, USLP-003…USLP-007, API-010, GEN-013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UslpFullFrame<'a> {
    header: UslpPrimaryHeader,
    octets: &'a [u8],
    layout: FrameLayout,
    insert_zone: &'a [u8],
    security_header: &'a [u8],
    tfdf_header: TfdfHeader,
    tfdz: &'a [u8],
    security_trailer: &'a [u8],
    ocf: Option<Ocf>,
    fecf: Option<u16>,
    anomalies: AnomalySet<UslpAnomaly>,
}

/// A decoded USLP frame: non-truncated or truncated (732.1 §4.1.2.6; M2 plan D12, USLP-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UslpFrame<'a> {
    /// End of Frame Primary Header Flag `0` (§4.1.2).
    Full(UslpFullFrame<'a>),
    /// End of Frame Primary Header Flag `1` (Annex D).
    Truncated(UslpTruncatedFrame<'a>),
}

/// The deviation of an OID frame's fixed fields, in check order, if any (732.1 §4.1.4.1.8–9;
/// OI-44 item 1).
fn oid_deviation(map_id: UslpMapId, tfdf: TfdfHeader, tfdz_len: usize) -> Option<FieldId> {
    let last = u16::try_from(tfdz_len.saturating_sub(1)).unwrap_or(u16::MAX);
    if map_id.get() != 0 {
        Some(field::MAP_ID)
    } else if !matches!(tfdf.rule, TfdzRule::SduStart(_)) {
        Some(field::TFDZ_CONSTRUCTION_RULE)
    } else if tfdf.upid != Upid::OnlyIdleData {
        Some(field::UPID)
    } else if tfdf.rule != TfdzRule::SduStart(LastValidOctet::from_raw(last)) {
        Some(field::FHP_LVO_POINTER)
    } else {
        None
    }
}

/// `true` unless the OCF Flag disagrees with the VC's OCF use (§4.1.5.3–4).
const fn ocf_flag_ok(vc: &VcConfig, flag: bool) -> bool {
    match vc.ocf() {
        OcfUse::Absent => !flag,
        OcfUse::Allowed => true,
        OcfUse::Required => flag,
    }
}

/// Test (c) on the fields before the Frame Length: the flags, the Reserve Spares, the VCID and
/// MAP ID, the VCF Count Length, and the OCF Flag (`docs/standards/uslp-frame.md` §3.1). The
/// MAP is `None` on VCID 63, which carries none.
fn check_channel<L: VcLookup + ?Sized>(
    ch: ChannelFields,
    rest: RawRest,
    channels: &L,
    mode: DecodeMode,
    anomalies: &mut AnomalySet<UslpAnomaly>,
) -> core::result::Result<(VcConfig, Option<MapConfig>, FrameKind), Rejection> {
    let kind = rest.kind().map_err(|e| fail(Reason::ReservedFlags, e))?;
    if rest.spare != 0 {
        match mode {
            DecodeMode::Strict => {
                return Err(fail(
                    Reason::ReservedSpare,
                    Error::new(ErrorKind::ReservedValue).with_field(field::USLP_HEADER_SPARE),
                ));
            }
            DecodeMode::Lenient => anomalies.insert(UslpAnomaly::ReserveSparesSet),
        }
    }
    let vc = channels
        .vc(ch.spacecraft_id, ch.vc_id)
        .ok_or(fail(Reason::InvalidVcid, unknown(field::VC_ID)))?;
    let map = if ch.vc_id.get() == OID_VCID {
        None
    } else {
        let m = channels
            .map(ch.spacecraft_id, ch.vc_id, ch.map_id)
            .ok_or(fail(Reason::InvalidMapId, unknown(field::MAP_ID)))?;
        Some(m)
    };
    if vc.truncated_frame_len().is_some() {
        return Err(fail(
            Reason::NonTruncatedFrameNotAllowed,
            invalid(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
        ));
    }
    if rest.vcf_count_len != vc.vcf_count_len(kind.bypass_flag()) {
        return Err(fail(
            Reason::VcfCountLengthMismatch,
            mismatch(field::VCF_COUNT_LEN),
        ));
    }
    if !ocf_flag_ok(&vc, rest.ocf_flag) {
        return Err(fail(Reason::OcfFlagMismatch, invalid(field::OCF_FLAG)));
    }
    Ok((vc, map, kind))
}

/// The checks of a non-truncated frame after the SCID (`docs/standards/uslp-frame.md` §3.1).
fn parse_full<'a, L: VcLookup + ?Sized>(
    bytes: &'a [u8],
    (start, rest): (RawStart, RawRest),
    channels: &L,
    mode: DecodeMode,
    exact: bool,
) -> core::result::Result<(UslpFullFrame<'a>, VcConfig, Option<MapConfig>), Rejection> {
    let mut anomalies = AnomalySet::empty();
    let (vc, map, kind) = check_channel(start.channel, rest, channels, mode, &mut anomalies)?;
    let ch = start.channel;
    let oid = map.is_none();
    let physical = channels.physical_channel();
    let total = rest.frame_len;
    if physical.is_fixed() && total != physical.frame_len() {
        return Err(fail(
            Reason::FixedLengthMismatch,
            mismatch(field::FRAME_LEN),
        ));
    }
    if !physical.is_fixed() && total > physical.frame_len() {
        return Err(fail(
            Reason::FrameTooLong,
            Error::out_of_range(
                field::FRAME_LEN,
                len_u64(physical.frame_len()),
                len_u64(total),
            ),
        ));
    }
    let security = if kind.protocol_control_flag() {
        SecurityLayout::NONE
    } else {
        frame_security(&vc, map.as_ref())
    };
    let spec = LayoutSpec {
        frame_len: total,
        primary_header_len: PRIMARY_HEADER_LEN.saturating_add(rest.vcf_count_len.octets()),
        secondary_len: physical.insert_zone_len(),
        security,
        ocf: rest.ocf_flag,
        fecf: physical.fecf(),
    };
    let layout = frame_layout(&spec, vc.fixed_tfdz()).ok_or(fail(
        Reason::FrameTooShort,
        invalid(field::FRAME_DATA_FIELD),
    ))?;
    let whole = exact || physical.is_fixed();
    let frame = match bytes.get(..total) {
        Some(frame) if !whole || bytes.len() == total => frame,
        _ => return Err(length(whole, total)),
    };
    if physical.fecf() {
        crc::verify_fecf(frame).map_err(|e| fail(Reason::FecfMismatch, e))?;
    }
    let parts = split_full(frame, &layout, vc.fixed_tfdz())?;
    let (tfdf_header, tfdz) = (parts.tfdf_header, parts.tfdz);
    if oid && let Some(f) = oid_deviation(ch.map_id, tfdf_header, tfdz.len()) {
        match mode {
            DecodeMode::Strict => return Err(fail(Reason::OidHeaderMismatch, invalid(f))),
            DecodeMode::Lenient => anomalies.insert(UslpAnomaly::OidHeaderMismatch),
        }
    }
    Ok((
        UslpFullFrame {
            header: UslpPrimaryHeader::new(
                ch,
                total,
                kind,
                rest.ocf_flag,
                read_vcf_count(frame, rest.vcf_count_len),
            ),
            octets: frame,
            layout,
            insert_zone: parts.insert_zone,
            security_header: parts.security_header,
            tfdf_header,
            tfdz,
            security_trailer: parts.security_trailer,
            ocf: parts
                .ocf
                .first_chunk::<OCF_LEN>()
                .map(|&octets| Ocf::decode(octets)),
            fecf: parts
                .fecf
                .first_chunk::<FECF_LEN>()
                .map(|&octets| u16::from_be_bytes(octets)),
            anomalies,
        },
        vc,
        map,
    ))
}

/// The decoder and the USLP Frame Validation Checks, in the order of
/// `docs/standards/uslp-frame.md` §3.1 (USLP-001…USLP-007, USLP-014, USLP-016).
///
/// With `exact`, `bytes` must be exactly one frame (test d); otherwise any octets after a
/// variable-length or truncated frame are ignored (API-011).
pub(crate) fn parse<'a, L: VcLookup + ?Sized>(
    bytes: &'a [u8],
    channels: &L,
    mode: DecodeMode,
    exact: bool,
) -> core::result::Result<(UslpFrame<'a>, VcConfig, Option<MapConfig>), Rejection> {
    let start = RawStart::parse(bytes).ok_or(length(exact, TRUNCATED_HEADER_LEN))?;
    let rest = if start.truncated {
        None
    } else {
        let r = RawRest::parse(bytes).ok_or(length(exact, PRIMARY_HEADER_LEN))?;
        Some(r)
    };
    start
        .check_version()
        .map_err(|e| fail(Reason::UnexpectedVersion, e))?;
    if !channels.has_spacecraft(start.channel.spacecraft_id) {
        return Err(fail(
            Reason::UnexpectedSpacecraft,
            unknown(field::SPACECRAFT_ID),
        ));
    }
    match rest {
        None => {
            let (frame, vc, map) = truncated::parse(bytes, start.channel, channels, exact)?;
            Ok((UslpFrame::Truncated(frame), vc, map))
        }
        Some(rest) => {
            let (frame, vc, map) = parse_full(bytes, (start, rest), channels, mode, exact)?;
            Ok((UslpFrame::Full(frame), vc, map))
        }
    }
}

impl<'a> UslpFrame<'a> {
    /// Decodes one frame from the start of `bytes` with the channels' managed parameters
    /// (732.1 §4.1, Annex D; USLP-001…USLP-007, USLP-016, API-010…API-012).
    ///
    /// A variable-length or truncated frame may be followed by other octets, which are ignored,
    /// so a stream can be parsed frame by frame with [`len`](Self::len) (API-011); on a
    /// fixed-length Physical Channel `bytes` must be exactly one frame (API-012). The checks are
    /// those of [`validate_frame`](crate::validate::validate_frame), in its order.
    ///
    /// A receiver of delimited frames must run `validate_frame`, or compare
    /// [`len`](Self::len) with the octets it supplied, because a bit error in the Frame Length
    /// field can make `decode` accept a shorter frame (API-011, USLP-014; VER-064).
    ///
    /// # Errors
    /// The decoder errors of `docs/standards/uslp-frame.md` §3.1, among them:
    /// - `Incomplete` (`needed`) for fewer than 4 octets, 7 for a non-truncated frame, or the
    ///   frame's length; `LengthMismatch` (`frame_len`) for another length than a fixed-length
    ///   channel's (USLP-003, API-012).
    /// - `UnsupportedVersion` (`frame_version`) and `ReservedValue`
    ///   (`protocol_control_command_flag`) in both modes (USLP-001); `ReservedValue`
    ///   (`uslp_header_spare`) in strict mode, [`UslpAnomaly::ReserveSparesSet`] in lenient.
    /// - `UnknownChannel` (`spacecraft_id`, `vc_id`, `map_id`); `InvalidField`
    ///   (`end_of_frame_primary_header_flag`) for a truncated frame on a VC without a truncated
    ///   frame length, and for a non-truncated frame on a VC with one (USLP-002; OI-48).
    /// - `LengthMismatch` (`vcf_count_len`) in both modes (USLP-004); `InvalidField`
    ///   (`ocf_flag`); `LengthMismatch` or `OutOfRange` (`frame_len`) against the channel;
    ///   `InvalidField` (`frame_data_field`) for no TFDZ octet (CMN-041).
    /// - `CrcMismatch` for a configured, wrong FECF.
    /// - `InvalidField` (`tfdz_construction_rule`) for a rule of the other TFDZ type (USLP-016);
    ///   `Malformed` (`fhp_lvo_pointer`) for a pointer beyond the TFDZ (USLP-006).
    /// - On VCID 63, `InvalidField` naming the deviating OID header field in strict mode,
    ///   [`UslpAnomaly::OidHeaderMismatch`] in lenient (OI-44 item 1).
    pub fn decode<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        channels: &L,
        mode: DecodeMode,
    ) -> Result<UslpFrame<'a>> {
        parse(bytes, channels, mode, false)
            .map(|(frame, _, _)| frame)
            .map_err(Error::from)
    }

    /// Total length of the frame that starts `prefix` (732.1 §4.1.2.7, Annex D1.3.2; API-013,
    /// GEN-053). A truncated frame's length is its VC's truncated frame length; a
    /// non-truncated frame's is the configured length of a fixed-length Physical Channel or the
    /// Frame Length field plus one.
    ///
    /// # Errors
    /// `Incomplete` (`needed` = 4) if `prefix` is shorter; `UnsupportedVersion`
    /// (`frame_version`); for a truncated header `UnknownChannel` (`spacecraft_id`, `vc_id`) or
    /// `InvalidField` (`end_of_frame_primary_header_flag`); for a non-truncated header on a
    /// configured VC that has a truncated frame length, `InvalidField`
    /// (`end_of_frame_primary_header_flag`: that VC carries only truncated frames, OI-48);
    /// otherwise, on a variable-length
    /// channel, `Incomplete` (`needed` = 6) for less than the Frame Length field.
    pub fn peek_length<L: VcLookup + ?Sized>(prefix: &[u8], channels: &L) -> Result<usize> {
        let start = RawStart::parse(prefix).ok_or(Error::incomplete(TRUNCATED_HEADER_LEN))?;
        start.check_version()?;
        let ch = start.channel;
        if start.truncated {
            if !channels.has_spacecraft(ch.spacecraft_id) {
                return Err(unknown(field::SPACECRAFT_ID));
            }
            let vc = channels
                .vc(ch.spacecraft_id, ch.vc_id)
                .ok_or(unknown(field::VC_ID))?;
            return vc
                .truncated_frame_len()
                .map(crate::config::TruncatedFrameLen::get)
                .ok_or(invalid(field::END_OF_FRAME_PRIMARY_HEADER_FLAG));
        }
        if channels
            .vc(ch.spacecraft_id, ch.vc_id)
            .is_some_and(|vc| vc.truncated_frame_len().is_some())
        {
            return Err(invalid(field::END_OF_FRAME_PRIMARY_HEADER_FLAG));
        }
        let physical = channels.physical_channel();
        if physical.is_fixed() {
            return Ok(physical.frame_len());
        }
        match prefix.get(4..6) {
            Some(&[l0, l1]) => Ok(usize::from(u16::from_be_bytes([l0, l1])).saturating_add(1)),
            _ => Err(Error::incomplete(6)),
        }
    }

    /// The non-truncated frame, if it is one.
    #[must_use]
    pub const fn full(&self) -> Option<&UslpFullFrame<'a>> {
        match self {
            UslpFrame::Full(f) => Some(f),
            UslpFrame::Truncated(_) => None,
        }
    }

    /// The truncated frame, if it is one (USLP-002).
    #[must_use]
    pub const fn truncated(&self) -> Option<&UslpTruncatedFrame<'a>> {
        match self {
            UslpFrame::Full(_) => None,
            UslpFrame::Truncated(t) => Some(t),
        }
    }

    /// Whether the End of Frame Primary Header Flag is 1 (732.1 §4.1.2.6; USLP-002).
    #[must_use]
    pub const fn is_truncated(&self) -> bool {
        matches!(self, UslpFrame::Truncated(_))
    }

    /// The fields of the first four octets (732.1 §4.1.2.2–5, Annex D1.3.1 note 2).
    #[must_use]
    pub const fn channel(&self) -> ChannelFields {
        match self {
            UslpFrame::Full(f) => f.header.channel(),
            UslpFrame::Truncated(t) => t.channel(),
        }
    }

    /// Spacecraft Identifier (732.1 §4.1.2.2.3).
    #[must_use]
    pub const fn spacecraft_id(&self) -> UslpSpacecraftId {
        self.channel().spacecraft_id
    }

    /// Source-or-Destination Identifier (732.1 §4.1.2.3).
    #[must_use]
    pub const fn source_or_destination(&self) -> SourceOrDestination {
        self.channel().source_or_destination
    }

    /// Virtual Channel Identifier (732.1 §4.1.2.4).
    #[must_use]
    pub const fn vc_id(&self) -> UslpVcId {
        self.channel().vc_id
    }

    /// MAP Identifier (732.1 §4.1.2.5).
    #[must_use]
    pub const fn map_id(&self) -> UslpMapId {
        self.channel().map_id
    }

    /// The TFDF Header (732.1 §4.1.4.2; USLP-005…USLP-007).
    #[must_use]
    pub const fn tfdf_header(&self) -> TfdfHeader {
        match self {
            UslpFrame::Full(f) => f.tfdf_header,
            UslpFrame::Truncated(t) => t.tfdf_header(),
        }
    }

    /// The Transfer Frame Data Zone, never empty (732.1 §4.1.4.3; GEN-013).
    #[must_use]
    pub const fn tfdz(&self) -> &'a [u8] {
        match self {
            UslpFrame::Full(f) => f.tfdz,
            UslpFrame::Truncated(t) => t.tfdz(),
        }
    }

    /// The Security Header, empty without SDLS (732.1 §6.3.4, Annex D1.2.3; API-072).
    #[must_use]
    pub const fn security_header(&self) -> &'a [u8] {
        match self {
            UslpFrame::Full(f) => f.security_header,
            UslpFrame::Truncated(t) => t.security_header(),
        }
    }

    /// The Security Trailer, empty without one (732.1 §6.3.6, Annex D1.2.3; API-072).
    #[must_use]
    pub const fn security_trailer(&self) -> &'a [u8] {
        match self {
            UslpFrame::Full(f) => f.security_trailer,
            UslpFrame::Truncated(t) => t.security_trailer(),
        }
    }

    /// Where the fields lie in [`octets`](Self::octets): the Primary Header, the Insert Zone (the
    /// secondary slot; empty for a truncated frame), the Security Header, the TFDF (header and
    /// TFDZ), the Security Trailer, the OCF, and the FECF (732.1 §4.1, Annex D1.2; CMN-040).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        match self {
            UslpFrame::Full(f) => &f.layout,
            UslpFrame::Truncated(t) => t.layout(),
        }
    }

    /// The octets of the whole frame, borrowed from the input (GEN-013).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        match self {
            UslpFrame::Full(f) => f.octets,
            UslpFrame::Truncated(t) => t.octets(),
        }
    }

    /// Octets consumed from the input: the frame length (732.1 §4.1.2.7, Annex D1.3.2;
    /// API-011).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.octets().len()
    }

    /// Always `false`: a frame has at least six octets.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<UslpAnomaly> {
        match self {
            UslpFrame::Full(f) => f.anomalies,
            UslpFrame::Truncated(t) => t.anomalies(),
        }
    }
}

impl<'a> UslpFullFrame<'a> {
    /// The Primary Header as plain values (732.1 §4.1.2; API-014).
    #[must_use]
    pub const fn header(&self) -> UslpPrimaryHeader {
        self.header
    }

    /// The Bypass/Sequence Control and Protocol Control Command Flags (Table 4-1; USLP-001).
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        self.header.kind()
    }

    /// The VCF Count with its length (§4.1.2.11–12; USLP-004).
    #[must_use]
    pub const fn vcf_count(&self) -> VcfCount {
        self.header.vcf_count()
    }

    /// The Insert Zone, empty without one (§4.1.3; USLP-008).
    #[must_use]
    pub const fn insert_zone(&self) -> &'a [u8] {
        self.insert_zone
    }

    /// The Security Header, empty without SDLS (§6.3.4; API-072).
    #[must_use]
    pub const fn security_header(&self) -> &'a [u8] {
        self.security_header
    }

    /// The TFDF Header (§4.1.4.2; USLP-005…USLP-007).
    #[must_use]
    pub const fn tfdf_header(&self) -> TfdfHeader {
        self.tfdf_header
    }

    /// The Transfer Frame Data Zone, never empty (§4.1.4.3; GEN-013).
    #[must_use]
    pub const fn tfdz(&self) -> &'a [u8] {
        self.tfdz
    }

    /// The Security Trailer, empty without one (§6.3.6; API-072).
    #[must_use]
    pub const fn security_trailer(&self) -> &'a [u8] {
        self.security_trailer
    }

    /// The Operational Control Field, when the OCF Flag is set (§4.1.5; CMN-010).
    #[must_use]
    pub const fn ocf(&self) -> Option<Ocf> {
        self.ocf
    }

    /// The Frame Error Control Field, checked, when the Physical Channel has one (§4.1.6).
    #[must_use]
    pub const fn fecf(&self) -> Option<u16> {
        self.fecf
    }

    /// Where the fields lie in [`octets`](Self::octets) (CMN-040).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// The octets of the whole frame, borrowed from the input (GEN-013).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        self.octets
    }

    /// Octets consumed from the input: the frame length (§4.1.2.7; API-011).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.octets.len()
    }

    /// Always `false`: a frame has at least nine octets.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<UslpAnomaly> {
        self.anomalies
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use super::{UslpFrame, oid_deviation, tfdf_rejection};
    use crate::anomaly::UslpAnomaly;
    use crate::config::tests::{fixed, mapid, plain_variable, variable};
    use crate::field;
    use crate::header::{FrameKind, SourceOrDestination};
    use crate::tfdf::tests::offset;
    use crate::tfdf::{FirstHeaderPointer, LastValidOctet, TfdfHeader, TfdzRule};
    use crate::upid::Upid;
    use crate::validate::Reason;
    use aldis_core::crc::crc16;
    use aldis_core::ocf::Ocf;
    use aldis_core::{DecodeMode, Error, ErrorDetail, ErrorKind, FieldId};
    use std::vec;
    use std::vec::Vec;

    /// `body` with its FECF appended.
    fn with_fecf(body: &[u8]) -> Vec<u8> {
        let mut v = body.to_vec();
        v.extend_from_slice(&crc16(body).to_be_bytes());
        v
    }

    /// A 64-octet frame on the fixed channel: VC 1, MAP 3, expedited (VCF Count of 2 octets),
    /// Insert Zone `IZ IZ`, rule `000` FHP 2, OCF.
    fn fixed_frame() -> Vec<u8> {
        // C155 shifted: 1100 0000 0001 0101 0101 | 0 | 000001 | 0011 | 0
        let mut b = vec![
            0xC0, 0x15, 0x50, 0x26, 0x00, 0x3F, 0x8A, 0x12, 0x34, 0xAA, 0xBB,
        ];
        b.extend_from_slice(&[0x00, 0x00, 0x02]);
        let tfdz_len = 64 - 11 - 3 - 4 - 2;
        b.extend((0..tfdz_len).map(|i| u8::try_from(i).unwrap()));
        b.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        with_fecf(&b)
    }

    /// An error's kind and field.
    type Outcome = (ErrorKind, Option<FieldId>);

    fn err_of(r: aldis_core::Result<UslpFrame<'_>>) -> Outcome {
        let e = r.unwrap_err();
        (e.kind(), e.field())
    }

    #[test]
    fn layout_places_the_fields() {
        // req: CMN-040, CMN-041, USLP-008
        let t = fixed();
        let bytes = fixed_frame();
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        let l = f.layout();
        // Primary Header with a 2-octet VCF Count, Insert Zone, TFDF Header and TFDZ, OCF, FECF.
        assert_eq!(
            l.ranges(),
            [0..9, 9..11, 11..11, 11..58, 58..58, 58..62, 62..64]
        );
        assert_eq!(f.full().unwrap().layout(), l);
        assert_eq!(l.frame_len(), f.len());
        assert_eq!(&bytes[l.secondary()], f.full().unwrap().insert_zone());
        assert_eq!(l.data().len(), 3 + f.tfdz().len());
    }

    #[test]
    fn decodes_a_fixed_frame() {
        // req: USLP-001, USLP-003, USLP-004, USLP-005, USLP-006, API-010, API-012, API-014, GEN-013
        let t = fixed();
        let bytes = fixed_frame();
        assert_eq!(bytes.len(), 64);
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        assert!(!f.is_truncated() && f.truncated().is_none() && !f.is_empty());
        assert_eq!(
            (f.spacecraft_id().get(), f.vc_id().get(), f.map_id().get()),
            (0x155, 1, 3)
        );
        assert_eq!(f.source_or_destination(), SourceOrDestination::Source);
        let full = f.full().unwrap();
        assert_eq!(full.kind(), FrameKind::Expedited);
        assert_eq!(
            (full.vcf_count().len().get(), full.vcf_count().value()),
            (2, 0x1234)
        );
        assert_eq!(full.insert_zone(), [0xAA, 0xBB]);
        assert_eq!(
            full.tfdf_header().rule,
            TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(offset(2)))
        );
        assert_eq!(full.tfdf_header().upid, Upid::Packets);
        assert_eq!(full.tfdz().len(), 44);
        assert_eq!(f.tfdz(), full.tfdz());
        assert_eq!(f.tfdf_header(), full.tfdf_header());
        assert_eq!(full.ocf(), Some(Ocf::decode([1, 2, 3, 4])));
        assert_eq!(full.fecf(), Some(crc16(&bytes[..62])));
        assert!(full.security_header().is_empty() && full.security_trailer().is_empty());
        assert!(f.security_header().is_empty() && f.security_trailer().is_empty());
        assert_eq!((f.len(), full.len(), full.is_empty()), (64, 64, false));
        assert_eq!((f.octets(), full.octets()), (&bytes[..], &bytes[..]));
        assert!(f.anomalies().is_empty() && full.anomalies().is_empty());
        let h = full.header();
        assert_eq!(h.octets()[..h.len()], bytes[..9]);
        assert_eq!(UslpFrame::peek_length(&bytes[..4], &t), Ok(64));
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            err_of(UslpFrame::decode(&longer, &t, DecodeMode::Strict)),
            (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
        );
    }

    #[test]
    fn decodes_variable_frames_with_sdls_and_ocf() {
        // req: USLP-003, USLP-004, USLP-016, API-011, API-070, API-072
        let t = plain_variable();
        // VC 5 MAP 1 (SDLS 6/16), sequence-controlled, rule 111, UPID 5, 2 TFDZ octets.
        let mut b = vec![0xC0, 0x15, 0x50, 0xA2, 0x00, 0x00, 0x00];
        b.extend_from_slice(&[0x51; 6]);
        b.extend_from_slice(&[0xE5, 0x77, 0x88]);
        b.extend_from_slice(&[0x52; 16]);
        let len = b.len() + 2;
        b[5] = u8::try_from(len - 1).unwrap();
        let mut bytes = with_fecf(&b);
        bytes.extend_from_slice(&[0xEE, 0xEE]);
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        assert_eq!(f.len(), len);
        assert_eq!(f.security_header(), [0x51; 6]);
        assert_eq!(f.security_trailer(), [0x52; 16]);
        assert_eq!(f.tfdz(), [0x77, 0x88]);
        assert_eq!(f.tfdf_header().rule, TfdzRule::NoSegmentation);
        assert_eq!(f.tfdf_header().upid, Upid::MissionSpecificInformation1);
        assert_eq!(f.full().unwrap().kind(), FrameKind::SequenceControlled);
        assert_eq!(UslpFrame::peek_length(&bytes, &t), Ok(len));
        // VC 4 MAP 2, expedited with a 1-octet VCF Count, OCF, protocol control.
        let mut b = vec![0xC0, 0x15, 0x50, 0x84, 0x00, 0x00, 0xC9, 0x07, 0xE1, 0x00];
        b.extend_from_slice(&[9, 8, 7, 6]);
        b[5] = u8::try_from(b.len() + 1).unwrap();
        let bytes = with_fecf(&b);
        let f = UslpFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        let full = f.full().unwrap();
        assert_eq!(full.kind(), FrameKind::ProtocolControl);
        assert_eq!(full.ocf(), Some(Ocf::decode([9, 8, 7, 6])));
        assert_eq!(
            (full.tfdz(), full.tfdf_header().upid),
            (&[0][..], Upid::Cop1ControlCommands)
        );
        assert_eq!(full.vcf_count().value(), 7);
        assert_eq!(
            err_of(UslpFrame::decode(&bytes[..10], &t, DecodeMode::Strict)),
            (ErrorKind::Incomplete, None)
        );
        assert_eq!(
            UslpFrame::peek_length(&bytes[..5], &t)
                .unwrap_err()
                .detail(),
            ErrorDetail::Needed(6)
        );
    }

    #[test]
    fn header_errors_in_check_order() {
        // req: USLP-001, USLP-002, USLP-003, USLP-004, USLP-014, GEN-041, CMN-041
        let t = fixed();
        let good = fixed_frame();
        let with = |at: usize, v: u8| {
            let mut b = good.clone();
            b[at] = v;
            b
        };
        let ic = |k, f| (k, Some(f));
        let cases: [(Vec<u8>, Outcome); 10] = [
            (good[..3].to_vec(), (ErrorKind::Incomplete, None)),
            (good[..6].to_vec(), (ErrorKind::Incomplete, None)),
            (
                with(0, 0xD0),
                ic(ErrorKind::UnsupportedVersion, field::FRAME_VERSION),
            ),
            (
                with(1, 0x16),
                ic(ErrorKind::UnknownChannel, field::SPACECRAFT_ID),
            ),
            (
                with(6, 0x4A),
                ic(
                    ErrorKind::ReservedValue,
                    field::PROTOCOL_CONTROL_COMMAND_FLAG,
                ),
            ),
            (
                with(6, 0x9A),
                ic(ErrorKind::ReservedValue, field::USLP_HEADER_SPARE),
            ),
            (with(3, 0x86), ic(ErrorKind::UnknownChannel, field::VC_ID)),
            (with(3, 0x24), ic(ErrorKind::UnknownChannel, field::MAP_ID)),
            (
                with(6, 0x89),
                ic(ErrorKind::LengthMismatch, field::VCF_COUNT_LEN),
            ),
            (with(6, 0x82), ic(ErrorKind::InvalidField, field::OCF_FLAG)),
        ];
        for (bytes, expected) in cases {
            assert_eq!(
                err_of(UslpFrame::decode(&bytes, &t, DecodeMode::Strict)),
                expected,
                "{bytes:02X?}"
            );
        }
        assert_eq!(
            err_of(UslpFrame::decode(&with(5, 0x3E), &t, DecodeMode::Strict)),
            ic(ErrorKind::LengthMismatch, field::FRAME_LEN)
        );
        let spare_bad_crc = with(6, 0x9A);
        let f = UslpFrame::decode(&spare_bad_crc, &t, DecodeMode::Lenient);
        assert_eq!(err_of(f), (ErrorKind::CrcMismatch, None));
        let mut spare = with(6, 0x9A);
        let n = spare.len();
        let crc = crc16(&spare[..n - 2]).to_be_bytes();
        spare[n - 2..].copy_from_slice(&crc);
        let f = UslpFrame::decode(&spare, &t, DecodeMode::Lenient).unwrap();
        assert!(f.anomalies().contains(UslpAnomaly::ReserveSparesSet));
        assert!(UslpFrame::peek_length(&[0xC0, 0x15], &t).is_err());
        assert!(UslpFrame::peek_length(&[0x10, 0x15, 0x50, 0x26], &t).is_err());
    }

    #[test]
    fn length_errors_on_a_variable_channel() {
        // req: USLP-003, USLP-014, CMN-041
        let t = plain_variable();
        // VC 4 MAP 0, sequence-controlled: Frame Length 600 > 512.
        let long = [0xC0, 0x15, 0x50, 0x80, 0x02, 0x57, 0x00, 0xE0, 0x00];
        assert_eq!(
            err_of(UslpFrame::decode(&long, &t, DecodeMode::Strict)),
            (ErrorKind::OutOfRange, Some(field::FRAME_LEN))
        );
        let e = UslpFrame::decode(&long, &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 512,
                found: 600
            }
        );
        // Frame Length 9 leaves no TFDZ octet after a 1-octet TFDF Header and the FECF.
        let short = [0xC0, 0x15, 0x50, 0x80, 0x00, 0x08, 0x00, 0xE0, 0x00];
        assert_eq!(
            err_of(UslpFrame::decode(&short, &t, DecodeMode::Strict)),
            (ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD))
        );
        // VC 6 (fixed-length TFDZ) with rule 111, and rule 000 with the pointer beyond.
        let rule = with_fecf(&[0xC0, 0x15, 0x50, 0xC0, 0x00, 0x0C, 0x00, 0xE0, 0, 0, 1]);
        assert_eq!(
            err_of(UslpFrame::decode(&rule, &t, DecodeMode::Strict)),
            (ErrorKind::InvalidField, Some(field::TFDZ_CONSTRUCTION_RULE))
        );
        let beyond = with_fecf(&[0xC0, 0x15, 0x50, 0xC0, 0x00, 0x0C, 0x00, 0x00, 0, 1, 1]);
        assert_eq!(
            err_of(UslpFrame::decode(&beyond, &t, DecodeMode::Strict)),
            (ErrorKind::Malformed, Some(field::FHP_LVO_POINTER))
        );
    }

    #[test]
    fn oid_header_checks() {
        // req: USLP-009, API-031, API-032
        let t = fixed();
        // VC 63, MAP 0, expedited (VCF 1/1), Insert Zone, rule 001 UPID idle, LVO last.
        let lvo = 64 - 8 - 2 - 3 - 2 - 1;
        let mut b = vec![
            0xC0, 0x15, 0x57, 0xE0, 0x00, 0x3F, 0x81, 0x05, 0xAA, 0xBB, 0x3F, 0x00,
        ];
        b.push(u8::try_from(lvo).unwrap());
        b.extend(core::iter::repeat_n(0xA5, lvo + 1));
        let ok = with_fecf(&b);
        let f = UslpFrame::decode(&ok, &t, DecodeMode::Strict).unwrap();
        assert!(f.anomalies().is_empty());
        let mut bad = b.clone();
        bad[10] = 0x3E;
        let bad = with_fecf(&bad);
        assert_eq!(
            err_of(UslpFrame::decode(&bad, &t, DecodeMode::Strict)),
            (ErrorKind::InvalidField, Some(field::UPID))
        );
        let f = UslpFrame::decode(&bad, &t, DecodeMode::Lenient).unwrap();
        assert!(f.anomalies().contains(UslpAnomaly::OidHeaderMismatch));
        let h = |rule, upid| TfdfHeader { rule, upid };
        let start = TfdzRule::SduStart(LastValidOctet::At(offset(9)));
        let idle = Upid::OnlyIdleData;
        assert_eq!(
            oid_deviation(mapid(1), h(start, idle), 10),
            Some(field::MAP_ID)
        );
        assert_eq!(
            oid_deviation(
                mapid(0),
                h(
                    TfdzRule::SduContinuation(LastValidOctet::At(offset(9))),
                    idle
                ),
                10
            ),
            Some(field::TFDZ_CONSTRUCTION_RULE)
        );
        assert_eq!(
            oid_deviation(mapid(0), h(start, Upid::Packets), 10),
            Some(field::UPID)
        );
        assert_eq!(
            oid_deviation(mapid(0), h(start, idle), 11),
            Some(field::FHP_LVO_POINTER)
        );
        assert_eq!(oid_deviation(mapid(0), h(start, idle), 10), None);
    }

    #[test]
    fn tfdf_reasons() {
        // req: USLP-006, USLP-014, USLP-016
        let r = |e: Error| tfdf_rejection(e).reason();
        assert_eq!(
            r(Error::new(ErrorKind::Malformed)),
            Reason::PointerBeyondZone
        );
        assert_eq!(
            r(Error::new(ErrorKind::InvalidField).with_field(field::TFDZ_CONSTRUCTION_RULE)),
            Reason::ConstructionRuleMismatch
        );
        assert_eq!(
            r(Error::new(ErrorKind::InvalidField).with_field(field::FRAME_DATA_FIELD)),
            Reason::FrameTooShort
        );
    }

    #[test]
    fn a_vc_with_a_truncated_length_refuses_non_truncated_frames() {
        // req: USLP-002, USLP-014, OI-48
        use crate::validate::validate_frame;
        // VC 4 MAP 0, sequence-controlled, rule 111, UPID 5, one TFDZ octet, FECF.
        let frame = with_fecf(&[0xC0, 0x15, 0x50, 0x80, 0x00, 0x0A, 0x00, 0xE5, 0xAA]);
        let plain = plain_variable();
        let t = variable();
        // Without a truncated frame length the frame is good.
        assert!(UslpFrame::decode(&frame, &plain, DecodeMode::Strict).is_ok());
        assert_eq!(UslpFrame::peek_length(&frame, &plain), Ok(11));
        let flag = (
            ErrorKind::InvalidField,
            Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
        );
        // With one, it is refused in both modes, by the decoder, the validation, and the length
        // peek, with a reason of its own; the check comes before the FECF and the lengths.
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            assert_eq!(err_of(UslpFrame::decode(&frame, &t, mode)), flag);
            let r = validate_frame(&frame, &t, mode).unwrap_err();
            assert_eq!(r.reason(), Reason::NonTruncatedFrameNotAllowed);
            assert_eq!((r.error().kind(), r.error().field()), flag);
            let mut bad = frame.clone();
            bad[10] ^= 1;
            bad[5] = 0x40;
            assert_eq!(err_of(UslpFrame::decode(&bad, &t, mode)), flag);
        }
        let peek = UslpFrame::peek_length(&frame, &t).unwrap_err();
        assert_eq!((peek.kind(), peek.field()), flag);
        // The flag is checked after the channel: an unknown MAP is reason 6, an unknown VC 5.
        let map3 = with_fecf(&[0xC0, 0x15, 0x50, 0x86, 0x00, 0x0A, 0x00, 0xE5, 0xAA]);
        let r = validate_frame(&map3, &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(r.reason(), Reason::InvalidMapId);
        // The length peek of an unknown VC is not refused.
        let vc9 = [0xC0, 0x15, 0x51, 0x20, 0x00, 0x09];
        assert_eq!(UslpFrame::peek_length(&vc9, &t), Ok(10));
    }
}
