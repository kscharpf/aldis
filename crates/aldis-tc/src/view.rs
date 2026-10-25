//! Zero-copy TC frame view (232.0 §4.1; TC-001…TC-006, API-010, API-011, API-013, API-014,
//! API-072, GEN-013, GEN-043).

use aldis_core::crc;
use aldis_core::ids::FrameSeqNum;
use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec};
use aldis_core::security::SecurityLayout;
use aldis_core::{AnomalySet, DecodeMode, Error, Result};

use crate::anomaly::TcAnomaly;
use crate::config::{PhysicalChannelConfig, VcConfig, VcLookup};
use crate::control::ControlCommand;
use crate::field;
use crate::header::{
    FrameFields, FrameType, PRIMARY_HEADER_LEN, RawHeader, SEGMENT_HEADER_LEN, SegmentHeader,
    TcFrameHeader, TcSpacecraftId, TcVcId, invalid, len_u64, reserved, unknown,
};
use crate::validate::{Reason, Rejection};

/// A decoded TC Transfer Frame borrowing its fields from the input (232.0 §4.1; TC-001…TC-006,
/// API-010, GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcFrame<'a> {
    header: TcFrameHeader,
    octets: &'a [u8],
    security_header: &'a [u8],
    data: &'a [u8],
    security_trailer: &'a [u8],
    fecf: Option<u16>,
    control_command: Option<ControlCommand>,
    anomalies: AnomalySet<TcAnomaly>,
}

/// Shorthand for a failed check.
const fn fail(reason: Reason, error: Error) -> Rejection {
    Rejection::new(reason, error)
}

/// `Incomplete` while decoding (`needed` = `len`), `LengthMismatch` (`frame_len`) while
/// validating, which needs exactly one frame (232.0 §4.4.9.3.3 d).
const fn length(exact: bool, len: usize) -> Rejection {
    let error = if exact {
        Error::new(aldis_core::ErrorKind::LengthMismatch).with_field(field::FRAME_LEN)
    } else {
        Error::incomplete(len)
    };
    fail(Reason::LengthMismatch, error)
}

/// The header of a frame that passed checks (a)–(c) (232.0 §4.4.9.3.3).
struct Head {
    raw: RawHeader,
    frame_type: FrameType,
    vc: VcConfig,
    physical: PhysicalChannelConfig,
    /// Whether the frame has a Segment Header: Type-D on a Virtual Channel with them (TC-004).
    segmented: bool,
}

/// Checks (a)–(c), which need only the header and the channels (232.0 §4.4.9.3.3 a–c;
/// TC-002, TC-003, TC-007, TC-021, TC-022).
fn check_head<L: VcLookup + ?Sized>(
    bytes: &[u8],
    channels: &L,
    mode: DecodeMode,
    exact: bool,
    anomalies: &mut AnomalySet<TcAnomaly>,
) -> core::result::Result<Head, Rejection> {
    let raw = RawHeader::parse(bytes).ok_or(length(exact, PRIMARY_HEADER_LEN))?;
    raw.check_version()
        .map_err(|e| fail(Reason::UnexpectedVersion, e))?;
    if !channels.has_spacecraft(raw.spacecraft_id) {
        return Err(fail(
            Reason::UnexpectedSpacecraft,
            unknown(field::SPACECRAFT_ID),
        ));
    }
    let frame_type = raw
        .frame_type()
        .map_err(|e| fail(Reason::ReservedFlags, e))?;
    if raw.spare != 0 {
        match mode {
            DecodeMode::Strict => {
                return Err(fail(
                    Reason::ReservedSpare,
                    reserved(field::TC_HEADER_SPARE),
                ));
            }
            DecodeMode::Lenient => anomalies.insert(TcAnomaly::ReservedSpareSet),
        }
    }
    let vc = channels
        .vc(raw.spacecraft_id, raw.vc_id)
        .ok_or(fail(Reason::InvalidVcid, unknown(field::VC_ID)))?;
    let physical = channels.physical_channel();
    let limit = vc.max_frame_len().min(physical.max_frame_len());
    if raw.frame_len > limit {
        return Err(fail(
            Reason::FrameTooLong,
            Error::out_of_range(field::FRAME_LEN, len_u64(limit), len_u64(raw.frame_len)),
        ));
    }
    let segmented = vc.has_segment_header() && !frame_type.control_command_flag();
    let shortest = PRIMARY_HEADER_LEN
        .saturating_add(if segmented { SEGMENT_HEADER_LEN } else { 0 })
        .saturating_add(1)
        .saturating_add(if physical.fecf() { FECF_LEN } else { 0 });
    if raw.frame_len < shortest {
        return Err(too_short());
    }
    Ok(Head {
        raw,
        frame_type,
        vc,
        physical,
        segmented,
    })
}

/// `InvalidField` (`frame_data_field`): the frame has no data octet (CMN-041).
const fn too_short() -> Rejection {
    fail(Reason::FrameTooShort, invalid(field::FRAME_DATA_FIELD))
}

/// The Control Command of a Type-C frame's `data` (232.0 §4.1.3.3; TC-005).
fn control_command(
    data: &[u8],
    mode: DecodeMode,
    anomalies: &mut AnomalySet<TcAnomaly>,
) -> core::result::Result<Option<ControlCommand>, Rejection> {
    let command = ControlCommand::parse(data);
    if command.is_none() {
        match mode {
            DecodeMode::Strict => {
                return Err(fail(
                    Reason::ReservedControlCommand,
                    reserved(field::CONTROL_COMMAND),
                ));
            }
            DecodeMode::Lenient => anomalies.insert(TcAnomaly::UnknownControlCommand),
        }
    }
    Ok(command)
}

/// The decoder and the Frame Validation Checks, in the check order of
/// [`Reason`] (232.0 §4.1, §4.4.9.3; TC-001…TC-006, TC-021…TC-023).
///
/// With `exact`, `bytes` must be exactly one frame (check d); otherwise any octets after the
/// frame are ignored (API-011).
pub(crate) fn parse<'a, L: VcLookup + ?Sized>(
    bytes: &'a [u8],
    channels: &L,
    mode: DecodeMode,
    exact: bool,
) -> core::result::Result<TcFrame<'a>, Rejection> {
    let mut anomalies = AnomalySet::empty();
    let head = check_head(bytes, channels, mode, exact, &mut anomalies)?;
    let (raw, total) = (head.raw, head.raw.frame_len);
    let frame = match bytes.get(..total) {
        Some(frame) if !exact || bytes.len() == total => frame,
        _ => return Err(length(exact, total)),
    };
    let fecf = head.physical.fecf();
    if fecf {
        crc::verify_fecf(frame).map_err(|e| fail(Reason::FecfMismatch, e))?;
    }
    let segment_header = if head.segmented {
        frame
            .get(PRIMARY_HEADER_LEN)
            .and_then(|&octet| SegmentHeader::from_octet(octet))
    } else {
        None
    };
    let control = head.frame_type.control_command_flag();
    let security = match segment_header {
        Some(sh) => channels
            .map(raw.spacecraft_id, raw.vc_id, sh.map_id)
            .ok_or(fail(Reason::InvalidMapId, unknown(field::MAP_ID)))?
            .security(),
        None if control => SecurityLayout::NONE,
        None => head.vc.security(),
    };
    let layout = FrameLayout::compute(&LayoutSpec {
        frame_len: total,
        primary_header_len: PRIMARY_HEADER_LEN,
        secondary_len: usize::from(segment_header.is_some()),
        security,
        ocf: false,
        fecf,
    })
    .map_err(|_| too_short())?;
    let data = frame.get(layout.data()).unwrap_or_default();
    let control_command = if control {
        control_command(data, mode, &mut anomalies)?
    } else {
        None
    };
    let fields = FrameFields {
        frame_type: head.frame_type,
        spacecraft_id: raw.spacecraft_id,
        vc_id: raw.vc_id,
        sequence_number: raw.sequence_number,
    };
    Ok(TcFrame {
        header: TcFrameHeader::new(fields, total, segment_header),
        octets: frame,
        security_header: frame.get(layout.security_header()).unwrap_or_default(),
        data,
        security_trailer: frame.get(layout.security_trailer()).unwrap_or_default(),
        fecf: frame
            .get(layout.fecf())
            .and_then(<[u8]>::first_chunk::<FECF_LEN>)
            .map(|&octets| u16::from_be_bytes(octets)),
        control_command,
        anomalies,
    })
}

impl<'a> TcFrame<'a> {
    /// Decodes one frame from the start of `bytes`, with the channels' managed parameters;
    /// any octets after it are ignored, so a stream can be parsed frame by frame with
    /// [`len`](Self::len) (232.0 §4.1; TC-001…TC-006, API-011).
    ///
    /// The checks are those of [`validate_frame`](crate::validate::validate_frame), in its
    /// order, except that the input may be longer than the frame.
    ///
    /// A receiver of delimited frames must run `validate_frame`, or compare
    /// [`len`](Self::len) with the octets it supplied, because a bit error in the Frame Length
    /// field can make `decode` accept a shorter frame (API-011, TC-021; VER-064).
    ///
    /// # Errors
    /// - `Incomplete` (`needed` = 5) for less than a Primary Header, then (`needed` = the
    ///   frame length) for less than the frame (TC-001).
    /// - `UnsupportedVersion` (`frame_version`) for a version other than `00`, and
    ///   `ReservedValue` (`control_command_flag`) for Bypass Flag `0` with Control Command
    ///   Flag `1`, in both modes (TC-002).
    /// - `UnknownChannel` (`spacecraft_id`, `vc_id`, `map_id`) for a Spacecraft ID, VCID, or
    ///   MAP ID the channels do not have (TC-021, TC-022).
    /// - `ReservedValue` (`tc_header_spare`) in strict mode for a Reserved Spare other than
    ///   `00`; lenient mode reports [`TcAnomaly::ReservedSpareSet`] (TC-003).
    /// - `OutOfRange` (`frame_len`) above the channel's Maximum Transfer Frame Length, and
    ///   `InvalidField` (`frame_data_field`) for a Frame Length that leaves no data octet
    ///   (TC-007, CMN-041).
    /// - `CrcMismatch` if the FECF is configured and wrong (TC-006).
    /// - `ReservedValue` (`control_command`) in strict mode for a Type-C data field that is
    ///   neither Unlock nor Set V(R); lenient mode reports
    ///   [`TcAnomaly::UnknownControlCommand`] (TC-005).
    pub fn decode<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        channels: &L,
        mode: DecodeMode,
    ) -> Result<TcFrame<'a>> {
        parse(bytes, channels, mode, false).map_err(Error::from)
    }

    /// Total length of the frame that starts `prefix`, from its first five octets (232.0
    /// §4.1.2.7; TC-001, API-013, GEN-053). It applies the checks of
    /// [`decode`](Self::decode) that need only the header and hold in both modes.
    ///
    /// # Errors
    /// `Incomplete` (`needed` = 5) if `prefix` is shorter; `UnsupportedVersion`
    /// (`frame_version`); `ReservedValue` (`control_command_flag`) (TC-002).
    pub fn peek_length(prefix: &[u8]) -> Result<usize> {
        TcFrame::peek_header(prefix).map(|h| h.frame_len())
    }

    /// The Primary Header of the frame that starts `prefix`, from its first five octets (232.0
    /// §4.1.2; TC-001, TC-002, API-014, GEN-053), with the checks of
    /// [`peek_length`](Self::peek_length).
    ///
    /// The rest of the frame is not read and no channel is looked up: for example to find the
    /// Spacecraft ID of a frame on a Physical Channel that carries several Master Channels,
    /// as the C ABI's `aldis_tc_header_decode` does (M2 plan D14). Whether a Segment Header
    /// follows depends on the Virtual Channel, so the header returned has none.
    ///
    /// # Errors
    /// The errors of [`peek_length`](Self::peek_length).
    pub fn peek_header(prefix: &[u8]) -> Result<TcFrameHeader> {
        let raw = RawHeader::parse(prefix).ok_or(Error::incomplete(PRIMARY_HEADER_LEN))?;
        raw.check_version()?;
        let fields = FrameFields {
            frame_type: raw.frame_type()?,
            spacecraft_id: raw.spacecraft_id,
            vc_id: raw.vc_id,
            sequence_number: raw.sequence_number,
        };
        Ok(TcFrameHeader::new(fields, raw.frame_len, None))
    }

    /// The header as plain values (232.0 §4.1.2, §4.1.3.2.2; API-014).
    #[must_use]
    pub const fn header(&self) -> TcFrameHeader {
        self.header
    }

    /// The frame type: the Bypass and Control Command Flags (232.0 Table 4-1; TC-002).
    #[must_use]
    pub const fn frame_type(&self) -> FrameType {
        self.header.frame_type()
    }

    /// Spacecraft Identifier (232.0 §4.1.2.5).
    #[must_use]
    pub const fn spacecraft_id(&self) -> TcSpacecraftId {
        self.header.spacecraft_id()
    }

    /// Virtual Channel Identifier (232.0 §4.1.2.6).
    #[must_use]
    pub const fn vc_id(&self) -> TcVcId {
        self.header.vc_id()
    }

    /// Frame Sequence Number N(S) (232.0 §4.1.2.8; TC-008).
    #[must_use]
    pub const fn sequence_number(&self) -> FrameSeqNum {
        self.header.sequence_number()
    }

    /// The Segment Header of a Type-D frame on a Virtual Channel that has one (232.0
    /// §4.1.3.2.2; TC-004).
    #[must_use]
    pub const fn segment_header(&self) -> Option<SegmentHeader> {
        self.header.segment_header()
    }

    /// The Security Header, empty without SDLS and in Type-C frames (232.0 §6.3.4; API-072).
    #[must_use]
    pub const fn security_header(&self) -> &'a [u8] {
        self.security_header
    }

    /// The data, never empty: the User Data of a Type-D frame's Frame Data Unit, after the
    /// Segment Header and the Security Header, or the octets of a Type-C frame's Control
    /// Command (232.0 §4.1.3, §6.3.5; TC-005, GEN-013).
    #[must_use]
    pub const fn data_field(&self) -> &'a [u8] {
        self.data
    }

    /// The Security Trailer, empty without SDLS and in Type-C frames (232.0 §6.3.6; API-072).
    #[must_use]
    pub const fn security_trailer(&self) -> &'a [u8] {
        self.security_trailer
    }

    /// The Frame Error Control Field, checked, when the Physical Channel has one (232.0
    /// §4.1.4; TC-006).
    #[must_use]
    pub const fn fecf(&self) -> Option<u16> {
        self.fecf
    }

    /// The Control Command of a Type-C frame, `None` for a Type-D frame and for a reserved
    /// pattern accepted in lenient mode (232.0 §4.1.3.3; TC-005).
    #[must_use]
    pub const fn control_command(&self) -> Option<ControlCommand> {
        self.control_command
    }

    /// The octets of the whole frame, borrowed from the input (232.0 §4.1.1; GEN-013).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        self.octets
    }

    /// Octets consumed from the input: the frame length (232.0 §4.1.2.7; TC-001, API-011).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.octets.len()
    }

    /// Always `false`: a frame has at least six octets (232.0 §4.1.1).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies; always empty after a strict decode (API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<TcAnomaly> {
        self.anomalies
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::TcFrame;
    use crate::anomaly::TcAnomaly;
    use crate::config::tests::standard;
    use crate::control::ControlCommand;
    use crate::field;
    use crate::header::{FrameType, SequenceFlags};
    use aldis_core::crc::crc16;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind};

    /// `body` followed by its FECF, in an `N`-octet array (`N` = body length + 2).
    fn with_fecf<const N: usize>(body: &[u8]) -> [u8; N] {
        let mut v = [0; N];
        v[..body.len()].copy_from_slice(body);
        v[body.len()..].copy_from_slice(&crc16(body).to_be_bytes());
        v
    }

    #[test]
    fn decodes_an_ad_frame() {
        // req: TC-001, TC-006, API-010, API-011, API-014, GEN-013
        let t = standard(true);
        let framed: [u8; 10] = with_fecf(&[0x01, 0x55, 0x04, 0x09, 0x04, 1, 2, 3]);
        let mut bytes = [0xEE; 11];
        bytes[..10].copy_from_slice(&framed);
        let f = TcFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        assert_eq!(
            (f.frame_type(), f.spacecraft_id().get(), f.vc_id().get()),
            (FrameType::Ad, 0x155, 1)
        );
        assert_eq!((f.sequence_number().get(), f.len()), (4, 10));
        assert_eq!(f.data_field(), [1, 2, 3]);
        assert!(f.security_header().is_empty() && f.security_trailer().is_empty());
        assert_eq!(f.fecf(), Some(crc16(&bytes[..8])));
        assert_eq!(
            (f.segment_header(), f.control_command(), f.is_empty()),
            (None, None, false)
        );
        assert_eq!(f.octets(), &bytes[..10]);
        assert!(f.anomalies().is_empty());
        assert_eq!(f.header().primary_header_octets(), bytes[..5]);
        assert_eq!(TcFrame::peek_length(&bytes), Ok(10));
    }

    #[test]
    fn segment_header_and_sdls_slices() {
        // req: TC-004, API-070, API-072
        let t = standard(false);
        let bytes = [
            0x01, 0x55, 0x08, 0x0B, 0x00, 0xC9, 0xA, 0xB, 0xC, 0x77, 0x88, 0xD,
        ];
        let f = TcFrame::decode(&bytes, &t, DecodeMode::Strict).unwrap();
        let sh = f.segment_header().unwrap();
        assert_eq!(
            (sh.sequence_flags, sh.map_id.get()),
            (SequenceFlags::Unsegmented, 9)
        );
        assert_eq!(f.security_header(), [0xA, 0xB, 0xC]);
        assert_eq!(f.data_field(), [0x77, 0x88]);
        assert_eq!(f.security_trailer(), [0xD]);
        assert_eq!(f.fecf(), None);
    }

    #[test]
    fn control_commands() {
        // req: TC-004, TC-005, API-031, API-032
        let t = standard(false);
        let unlock = [0x31, 0x55, 0x08, 0x05, 0x00, 0x00];
        let f = TcFrame::decode(&unlock, &t, DecodeMode::Strict).unwrap();
        assert_eq!(
            (f.frame_type(), f.control_command(), f.segment_header()),
            (FrameType::Bc, Some(ControlCommand::Unlock), None)
        );
        let odd = [0x31, 0x55, 0x04, 0x05, 0x00, 0x01];
        let e = TcFrame::decode(&odd, &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::ReservedValue, Some(field::CONTROL_COMMAND))
        );
        let f = TcFrame::decode(&odd, &t, DecodeMode::Lenient).unwrap();
        assert!(f.anomalies().contains(TcAnomaly::UnknownControlCommand));
        assert_eq!((f.control_command(), f.data_field()), (None, &[1][..]));
    }

    #[test]
    fn header_errors_in_check_order() {
        // req: TC-001, TC-002, TC-003, TC-021, TC-022, GEN-041
        let t = standard(false);
        let cases: [(&[u8], ErrorKind, Option<aldis_core::FieldId>); 8] = [
            (&[0x01, 0x55, 0x04, 0x05], ErrorKind::Incomplete, None),
            (
                &[0x41, 0x55, 0x04, 0x05, 0, 0],
                ErrorKind::UnsupportedVersion,
                Some(field::FRAME_VERSION),
            ),
            (
                &[0x01, 0x56, 0x04, 0x05, 0, 0],
                ErrorKind::UnknownChannel,
                Some(field::SPACECRAFT_ID),
            ),
            (
                &[0x11, 0x55, 0x04, 0x05, 0, 0],
                ErrorKind::ReservedValue,
                Some(field::CONTROL_COMMAND_FLAG),
            ),
            (
                &[0x05, 0x55, 0x04, 0x05, 0, 0],
                ErrorKind::ReservedValue,
                Some(field::TC_HEADER_SPARE),
            ),
            (
                &[0x01, 0x55, 0x10, 0x05, 0, 0],
                ErrorKind::UnknownChannel,
                Some(field::VC_ID),
            ),
            (
                &[0x01, 0x55, 0x04, 0x04, 0, 0],
                ErrorKind::InvalidField,
                Some(field::FRAME_DATA_FIELD),
            ),
            (
                &[0x01, 0x55, 0x08, 0x06, 0, 0xC7, 0],
                ErrorKind::UnknownChannel,
                Some(field::MAP_ID),
            ),
        ];
        for (bytes, kind, fld) in cases {
            let e = TcFrame::decode(bytes, &t, DecodeMode::Strict).unwrap_err();
            assert_eq!((e.kind(), e.field()), (kind, fld), "{bytes:02X?}");
        }
        let e =
            TcFrame::decode(&[0x01, 0x55, 0x04, 0x07, 0, 0], &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(8))
        );
        let lenient =
            TcFrame::decode(&[0x05, 0x55, 0x04, 0x05, 0, 9], &t, DecodeMode::Lenient).unwrap();
        assert!(lenient.anomalies().contains(TcAnomaly::ReservedSpareSet));
        let e = TcFrame::peek_length(&[0x01, 0x55, 0x04]).unwrap_err();
        assert_eq!(e.detail(), ErrorDetail::Needed(5));
        assert!(TcFrame::peek_length(&[0x81, 0x55, 0x04, 0x05, 0]).is_err());
        assert!(TcFrame::peek_length(&[0x11, 0x55, 0x04, 0x05, 0]).is_err());
    }

    #[test]
    fn the_header_peeks_alone() {
        // req: TC-001, TC-002, API-014, GEN-053
        // Type-BC, SCID 0x3FF, VCID 63, Frame Length field 6, N(S) 9; no channel is looked up.
        let h = TcFrame::peek_header(&[0x33, 0xFF, 0xFC, 0x06, 0x09]).unwrap();
        assert_eq!(
            (h.frame_type(), h.spacecraft_id().get(), h.vc_id().get()),
            (FrameType::Bc, 0x3FF, 63)
        );
        assert_eq!((h.frame_len(), h.sequence_number().get()), (7, 9));
        assert_eq!(h.segment_header(), None);
        let e = TcFrame::peek_header(&[0x01, 0x55, 0x04, 0x05]).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(5))
        );
        let e = TcFrame::peek_header(&[0x41, 0x55, 0x04, 0x05, 0]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::UnsupportedVersion, Some(field::FRAME_VERSION))
        );
        let e = TcFrame::peek_header(&[0x11, 0x55, 0x04, 0x05, 0]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::ReservedValue, Some(field::CONTROL_COMMAND_FLAG))
        );
    }

    #[test]
    fn length_limits_and_fecf() {
        // req: TC-006, TC-007, CMN-041
        let t = standard(true);
        let e = TcFrame::decode(&[0x01, 0x55, 0x07, 0xFF, 0], &t, DecodeMode::Strict);
        assert!(e.is_err_and(|e| e.kind() == ErrorKind::Incomplete));
        let mut ok: [u8; 8] = with_fecf(&[0x01, 0x55, 0x04, 0x07, 0x00, 0xAB]);
        assert!(TcFrame::decode(&ok, &t, DecodeMode::Strict).is_ok());
        ok[5] ^= 1;
        let e = TcFrame::decode(&ok, &t, DecodeMode::Lenient).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::CrcMismatch);
        // VC 3 has SDLS 4/2: a 13-octet frame leaves no data octet.
        let short: [u8; 13] = with_fecf(&[0x01, 0x55, 0x0C, 0x0C, 0, 0, 0, 0, 0, 0, 0]);
        let e = TcFrame::decode(&short, &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(e.field(), Some(field::FRAME_DATA_FIELD));
    }
}
