//! Encoders: one-shot [`encode`] and the two-phase [`TcFrameWriter`].
//!
//! The writer has a finishing typestate (232.0 §4.1, §6.3; TC-004, TC-007, TC-008, API-020…API-023, API-070, API-072,
//! API-074, RS-023, GEN-014, GEN-052).

use core::marker::PhantomData;

use aldis_core::crc;
use aldis_core::finishing::{Finalized, FrameState, Protected, Unprotected, require_unsecured};
use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec};
use aldis_core::security::{FrameCtx, SecuredFields, SecurityHook, SecurityLayout};
use aldis_core::{Error, Result};

use crate::config::VcLookup;
use crate::control::{ControlCommand, ControlOctets};
use crate::field;
use crate::header::{
    FrameFields, PRIMARY_HEADER_LEN, SegmentHeader, UNFINISHED_VERSION, invalid, len_u64,
    primary_octets, unknown,
};

/// Transfer Frame Version Number `00` (232.0 §4.1.2.2.2).
const VERSION: u8 = 0;

/// The content of a frame's data field (232.0 §4.1.3.1.3; TC-004, TC-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameData<'a> {
    /// A Frame Data Unit of a Type-D frame: the Segment Header, given exactly when the Virtual
    /// Channel has one, and the User Data, at least one octet (232.0 §4.1.3.2).
    Fdu {
        /// The Segment Header (232.0 §4.1.3.2.2).
        segment_header: Option<SegmentHeader>,
        /// The User Data.
        user_data: &'a [u8],
    },
    /// The Control Command of a Type-C frame (232.0 §4.1.3.3).
    Control(ControlCommand),
}

/// Where the fields of a frame go, checked against the channels (232.0 §4.1, §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    fields: FrameFields,
    segment_header: Option<SegmentHeader>,
    /// Offset of the data: after the Primary Header, the Segment Header, and the Security
    /// Header.
    data_start: usize,
    security_header_len: usize,
    security_trailer_len: usize,
    fecf: bool,
    /// The frame length with `data_len` data octets.
    total: usize,
}

impl Plan {
    /// Checks `fields` and a data field of `data_len` octets against the channels (232.0
    /// §4.1, §5, §6.3; TC-004, TC-007, API-001).
    ///
    /// # Errors
    /// `UnknownChannel` (`spacecraft_id`, `vc_id`, `map_id`); `InvalidField`
    /// (`control_command_flag`) when the frame type and the content disagree;
    /// `InvalidField` (`segment_header`) when a Segment Header is given on a Virtual Channel
    /// without them or missing on one with them; `InvalidField` (`frame_data_field`) for no
    /// data; `OutOfRange` (`frame_len`) above the channel's Maximum Transfer Frame Length.
    fn new<L: VcLookup + ?Sized>(
        channels: &L,
        fields: FrameFields,
        segment_header: Option<SegmentHeader>,
        control: bool,
        data_len: usize,
    ) -> Result<Plan> {
        let (scid, vcid) = (fields.spacecraft_id, fields.vc_id);
        if !channels.has_spacecraft(scid) {
            return Err(unknown(field::SPACECRAFT_ID));
        }
        let vc = channels.vc(scid, vcid).ok_or(unknown(field::VC_ID))?;
        if control != fields.frame_type.control_command_flag() {
            return Err(invalid(field::CONTROL_COMMAND_FLAG));
        }
        if !control && segment_header.is_some() != vc.has_segment_header() {
            return Err(invalid(field::SEGMENT_HEADER));
        }
        let security = match segment_header {
            Some(sh) => channels
                .map(scid, vcid, sh.map_id)
                .ok_or(unknown(field::MAP_ID))?
                .security(),
            None if control => SecurityLayout::NONE,
            None => vc.security(),
        };
        let physical = channels.physical_channel();
        let secondary_len = usize::from(segment_header.is_some());
        let total = PRIMARY_HEADER_LEN
            .saturating_add(secondary_len)
            .saturating_add(security.header_len())
            .saturating_add(data_len)
            .saturating_add(security.trailer_len())
            .saturating_add(if physical.fecf() { FECF_LEN } else { 0 });
        let layout = FrameLayout::compute(&LayoutSpec {
            frame_len: total,
            primary_header_len: PRIMARY_HEADER_LEN,
            secondary_len,
            security,
            ocf: false,
            fecf: physical.fecf(),
        })
        .map_err(|_| invalid(field::FRAME_DATA_FIELD))?;
        let limit = vc.max_frame_len().min(physical.max_frame_len());
        if total > limit {
            return Err(Error::out_of_range(
                field::FRAME_LEN,
                len_u64(limit),
                len_u64(total),
            ));
        }
        Ok(Plan {
            fields,
            segment_header,
            data_start: layout.data().start,
            security_header_len: security.header_len(),
            security_trailer_len: security.trailer_len(),
            fecf: physical.fecf(),
            total,
        })
    }
}

/// Encodes a frame into the start of `out`; returns its length (232.0 §4.1; TC-007).
///
/// The Frame Length and the FECF are computed, the Reserved Spare is `00`, and a Security
/// Header and Trailer are written as zeros for the SDLS layer to fill (API-072); the
/// [`TcFrameWriter`] exposes them in place (TC-004, TC-005, TC-008, API-020, API-021).
///
/// # Errors
/// - `UnknownChannel` (`spacecraft_id`, `vc_id`, `map_id`) for a channel the channels do not
///   have.
/// - `InvalidField` (`control_command_flag`) for a Type-BC frame with a Frame Data Unit, or a
///   Type-AD or Type-BD frame with a Control Command (232.0 Table 4-1).
/// - `InvalidField` (`segment_header`) for a Segment Header on a Virtual Channel without
///   them, or none on a Virtual Channel with them (TC-004).
/// - `InvalidField` (`frame_data_field`) for empty User Data (CMN-041).
/// - `OutOfRange` (`frame_len`) above the channel's Maximum Transfer Frame Length, which is
///   at most 1024 octets, so the data field is at most 1019 octets, 1017 with the FECF
///   (TC-007).
/// - `BufferTooSmall` if `out` is shorter than the frame.
///
/// On error `out` is unchanged (API-022).
pub fn encode<L: VcLookup + ?Sized>(
    channels: &L,
    fields: FrameFields,
    data: FrameData<'_>,
    out: &mut [u8],
) -> Result<usize> {
    let (segment_header, control, octets) = match data {
        FrameData::Fdu {
            segment_header,
            user_data,
        } => (segment_header, None, user_data),
        FrameData::Control(command) => (None, Some(command.to_octets()), &[][..]),
    };
    let payload = control.as_ref().map_or(octets, ControlOctets::as_slice);
    let plan = Plan::new(
        channels,
        fields,
        segment_header,
        control.is_some(),
        payload.len(),
    )?;
    let mut w = TcFrameWriter::start(out, plan)?;
    for (d, s) in w.user_data_mut().iter_mut().zip(payload) {
        *d = *s;
    }
    w.fix_length(payload.len()).map(|w| w.finalize().len())
}

/// Two-phase writer of a Type-D frame (232.0 §4.1, §6.3; API-020…API-023, API-072, API-074,
/// RS-023).
///
/// [`begin`](Self::begin) reserves the frame for up to a maximum of User Data and returns the
/// writer [`Unprotected`]; the caller fills the User Data and, with SDLS, the Security Header
/// and Trailer in place. [`protect`](TcFrameWriter::protect) (or
/// [`unsecured`](TcFrameWriter::unsecured) without SDLS) fixes the length of the User Data, which
/// decides where the Security Trailer lies and the Frame Length, writes the header, and applies
/// SDLS; the writer is then [`Protected`]. [`finalize`](TcFrameWriter::finalize) computes the
/// FECF, last, and gives the [`Finalized`] writer, the only state that exposes the frame's
/// octets. Each step consumes the writer, so a step out of order does not compile (RS-023,
/// RS-021).
///
/// Until `protect` or `unsecured`, the buffer holds a Primary Header whose version field is `11`
/// and whose Frame Length is the reserved frame length, which every decoder rejects with
/// `UnsupportedVersion`, so an unfinished frame cannot be mistaken for a valid one (API-023).
#[derive(Debug)]
pub struct TcFrameWriter<'b, S: FrameState = Unprotected> {
    out: &'b mut [u8],
    plan: Plan,
    max_user_len: usize,
    /// The frame length: the reserved one until the User Data length is fixed.
    total: usize,
    state: PhantomData<S>,
}

impl<'b, S: FrameState> TcFrameWriter<'b, S> {
    /// The same frame in state `T`: the one place a state changes.
    const fn into_state<T: FrameState>(self) -> TcFrameWriter<'b, T> {
        TcFrameWriter {
            out: self.out,
            plan: self.plan,
            max_user_len: self.max_user_len,
            total: self.total,
            state: PhantomData,
        }
    }
}

impl<'b> TcFrameWriter<'b, Unprotected> {
    /// Reserves a Type-D frame with room for up to `max_user_len` octets of User Data (232.0
    /// §4.1; TC-004, TC-007, API-020).
    ///
    /// # Errors
    /// The errors of [`encode`] for a frame with `max_user_len` octets of User Data:
    /// `InvalidField` (`control_command_flag`) for a Type-BC frame; `BufferTooSmall` if `out`
    /// cannot hold the largest frame. On error `out` is unchanged.
    pub fn begin<L: VcLookup + ?Sized>(
        out: &'b mut [u8],
        channels: &L,
        fields: FrameFields,
        segment_header: Option<SegmentHeader>,
        max_user_len: usize,
    ) -> Result<TcFrameWriter<'b, Unprotected>> {
        let plan = Plan::new(channels, fields, segment_header, false, max_user_len)?;
        TcFrameWriter::start(out, plan)
    }

    /// The writer of a checked plan (API-023): writes the unfinished header, the Segment
    /// Header, and zeros in the Security Header and Trailer.
    fn start(out: &'b mut [u8], plan: Plan) -> Result<TcFrameWriter<'b, Unprotected>> {
        let buf = out
            .get_mut(..plan.total)
            .ok_or(Error::buffer_too_small(plan.total))?;
        let header = primary_octets(UNFINISHED_VERSION, plan.fields, plan.total);
        let segment = plan.segment_header.map(SegmentHeader::octet);
        for (d, s) in buf.iter_mut().zip(header.into_iter().chain(segment)) {
            *d = s;
        }
        let max_user_len = plan
            .total
            .saturating_sub(plan.data_start)
            .saturating_sub(plan.security_trailer_len)
            .saturating_sub(if plan.fecf { FECF_LEN } else { 0 });
        let total = plan.total;
        let mut w = TcFrameWriter {
            out,
            plan,
            max_user_len,
            total,
            state: PhantomData,
        };
        w.security_header_mut().fill(0);
        w.security_trailer_mut().fill(0);
        Ok(w)
    }

    /// The Security Header to fill in place; empty without SDLS (232.0 §6.3.4; API-072).
    pub fn security_header_mut(&mut self) -> &mut [u8] {
        let end = self.plan.data_start;
        let start = end.saturating_sub(self.plan.security_header_len);
        self.out.get_mut(start..end).unwrap_or_default()
    }

    /// The User Data to fill in place, `max_user_len` octets (232.0 §4.1.3.2; GEN-014).
    pub fn user_data_mut(&mut self) -> &mut [u8] {
        let start = self.plan.data_start;
        let end = start.saturating_add(self.max_user_len);
        self.out.get_mut(start..end).unwrap_or_default()
    }

    /// The Security Trailer to fill in place, after the reserved User Data; empty without SDLS.
    /// [`protect`](Self::protect) moves it to follow the User Data (232.0 §6.3.6; API-072).
    pub fn security_trailer_mut(&mut self) -> &mut [u8] {
        let start = self.plan.data_start.saturating_add(self.max_user_len);
        let end = start.saturating_add(self.plan.security_trailer_len);
        self.out.get_mut(start..end).unwrap_or_default()
    }

    /// Fixes the User Data at `user_len` octets and applies `hook` to the Security Header, the
    /// User Data, and the Security Trailer, after every other field except the FECF is final
    /// (232.0 §4.1.2.7, §4.1.4, §6.3; TC-007, API-074, RS-023). `ctx` identifies the channel for
    /// the hook's Security Association.
    ///
    /// When `user_len` is below the reservation, the Security Trailer moves to follow the User
    /// Data, and the reserved octets after the frame keep unspecified values; use only the
    /// finalized frame's octets.
    ///
    /// # Errors
    /// As [`unsecured`](Self::unsecured), without the SDLS check: `OutOfRange` (`user_len`)
    /// above the reservation; `InvalidField` (`frame_data_field`) for 0; and the hook's error,
    /// `Security`, after which the frame is dropped and must not be transmitted. On the first
    /// two errors the buffer keeps its unfinished state.
    pub fn protect<H: SecurityHook + ?Sized>(
        self,
        user_len: usize,
        hook: &mut H,
        ctx: &FrameCtx,
    ) -> Result<TcFrameWriter<'b, Protected>> {
        self.fix_length(user_len).and_then(|mut w| {
            let result = {
                let (frame, fields) = w.secured(user_len);
                hook.protect(ctx, frame, &fields)
            };
            w.checked(result)
        })
    }

    /// Fixes the User Data at `user_len` octets on a channel without SDLS, the `NoSecurity`
    /// path (API-073, RS-023); the frame is then [`Protected`].
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len` or `security_trailer_len`) if the channel reserves
    /// a Security Header or Trailer, which need [`protect`](Self::protect); `OutOfRange`
    /// (`user_len`) above the reservation; `InvalidField` (`frame_data_field`) for 0. On error
    /// the buffer keeps its unfinished state.
    pub fn unsecured(self, user_len: usize) -> Result<TcFrameWriter<'b, Protected>> {
        require_unsecured(
            self.plan.security_header_len,
            self.plan.security_trailer_len,
        )?;
        self.fix_length(user_len)
    }

    /// Fixes the length: moves the Security Trailer to follow `user_len` octets of User Data and
    /// writes the final Primary Header (232.0 §4.1.2.7; TC-007, API-023).
    fn fix_length(self, user_len: usize) -> Result<TcFrameWriter<'b, Protected>> {
        if user_len > self.max_user_len {
            return Err(Error::out_of_range(
                field::USER_LEN,
                len_u64(self.max_user_len),
                len_u64(user_len),
            ));
        }
        if user_len == 0 {
            return Err(invalid(field::FRAME_DATA_FIELD));
        }
        let unused = self.max_user_len.saturating_sub(user_len);
        let total = self.plan.total.saturating_sub(unused);
        // The unused User Data and the Security Trailer after it: move the trailer to the front.
        let tail_start = self.plan.data_start.saturating_add(user_len);
        let tail_end = tail_start
            .saturating_add(unused)
            .saturating_add(self.plan.security_trailer_len);
        let mut w = self;
        let tail = w.out.get_mut(tail_start..tail_end).unwrap_or_default();
        let from = unused.min(tail.len());
        tail.copy_within(from.., 0);
        let frame = w.out.get_mut(..total).unwrap_or_default();
        let header = primary_octets(VERSION, w.plan.fields, total);
        for (d, s) in frame.iter_mut().zip(header) {
            *d = s;
        }
        w.total = total;
        Ok(w.into_state())
    }
}

impl<'b> TcFrameWriter<'b, Protected> {
    /// The frame and the ranges of its secured fields, for the hook of `protect`.
    fn secured(&mut self, user_len: usize) -> (&mut [u8], SecuredFields) {
        let fields = SecuredFields::adjacent(
            self.plan
                .data_start
                .saturating_sub(self.plan.security_header_len),
            self.plan.security_header_len,
            user_len,
            self.plan.security_trailer_len,
        );
        (self.out.get_mut(..self.total).unwrap_or_default(), fields)
    }

    /// The writer if the hook succeeded; the part of `protect` that does not depend on the
    /// hook type.
    fn checked(self, result: Result<()>) -> Result<Self> {
        result.map(|()| self)
    }

    /// Completes the frame: computes the FECF, if configured, over every other octet, last
    /// (232.0 §4.1.4; TC-007, API-021, API-074, RS-023).
    #[must_use = "the finalized frame"]
    pub fn finalize(self) -> TcFrameWriter<'b, Finalized> {
        if self.plan.fecf {
            let frame = self.out.get_mut(..self.total).unwrap_or_default();
            // The frame holds at least the Primary Header and the FECF, so this cannot fail.
            let _ = crc::write_fecf(frame);
        }
        self.into_state()
    }
}

impl<'b> TcFrameWriter<'b, Finalized> {
    /// The frame length (232.0 §4.1.2.7; TC-007).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.total
    }

    /// Always `false`: a frame holds at least its Primary Header.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The octets of the finalized frame, for transmission (RS-023).
    #[must_use]
    pub fn octets(&self) -> &[u8] {
        self.out.get(..self.total).unwrap_or_default()
    }

    /// The octets of the finalized frame, borrowed from the buffer `begin` took, for
    /// transmission (RS-023).
    #[must_use]
    pub fn into_octets(self) -> &'b [u8] {
        let total = self.total;
        self.out.get(..total).unwrap_or_default()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameData, TcFrameWriter, encode};
    use crate::config::tests::{mapid, scid, standard, vcid};
    use crate::control::ControlCommand;
    use crate::field;
    use crate::header::{FrameFields, FrameType, SegmentHeader, SequenceFlags};
    use crate::view::TcFrame;
    use aldis_core::crc::syndrome;
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::security::{FrameCtx, NoSecurity, SecuredFields, SecurityHook};
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind, Result};

    fn fields(frame_type: FrameType, vc: u8) -> FrameFields {
        FrameFields {
            frame_type,
            spacecraft_id: scid(0x155),
            vc_id: vcid(vc),
            sequence_number: FrameSeqNum::new(9),
        }
    }

    fn fdu(segment_header: Option<SegmentHeader>, user_data: &[u8]) -> FrameData<'_> {
        FrameData::Fdu {
            segment_header,
            user_data,
        }
    }

    #[test]
    fn encode_round_trips() {
        // req: TC-001, TC-005, TC-007, TC-008, API-020, API-021, GEN-052
        let t = standard(true);
        let mut out = [0_u8; 32];
        let n = encode(&t, fields(FrameType::Bd, 1), fdu(None, &[7, 8]), &mut out).unwrap();
        assert_eq!(n, 9);
        let f = TcFrame::decode(&out[..n], &t, DecodeMode::Strict).unwrap();
        assert_eq!(
            (f.frame_type(), f.sequence_number().get()),
            (FrameType::Bd, 9)
        );
        assert_eq!(f.data_field(), [7, 8]);
        let set = ControlCommand::SetVr(FrameSeqNum::new(3));
        let n = encode(
            &t,
            fields(FrameType::Bc, 2),
            FrameData::Control(set),
            &mut out,
        )
        .unwrap();
        let f = TcFrame::decode(&out[..n], &t, DecodeMode::Strict).unwrap();
        assert_eq!((f.control_command(), f.segment_header()), (Some(set), None));
    }

    #[test]
    fn encode_checks() {
        // req: TC-004, TC-007, API-001, API-022
        let t = standard(true);
        let mut out = [0xEE_u8; 16];
        let sh = SegmentHeader {
            sequence_flags: SequenceFlags::First,
            map_id: mapid(5),
        };
        let mut bad = fields(FrameType::Ad, 1);
        bad.spacecraft_id = scid(1);
        let unlock = FrameData::Control(ControlCommand::Unlock);
        let cases = [
            (
                bad,
                fdu(None, &[1]),
                ErrorKind::UnknownChannel,
                field::SPACECRAFT_ID,
            ),
            (
                fields(FrameType::Ad, 9),
                fdu(None, &[1]),
                ErrorKind::UnknownChannel,
                field::VC_ID,
            ),
            (
                fields(FrameType::Ad, 1),
                unlock,
                ErrorKind::InvalidField,
                field::CONTROL_COMMAND_FLAG,
            ),
            (
                fields(FrameType::Bc, 1),
                fdu(None, &[0]),
                ErrorKind::InvalidField,
                field::CONTROL_COMMAND_FLAG,
            ),
            (
                fields(FrameType::Ad, 1),
                fdu(Some(sh), &[1]),
                ErrorKind::InvalidField,
                field::SEGMENT_HEADER,
            ),
            (
                fields(FrameType::Ad, 2),
                fdu(None, &[1]),
                ErrorKind::InvalidField,
                field::SEGMENT_HEADER,
            ),
            (
                fields(FrameType::Ad, 2),
                fdu(
                    Some(SegmentHeader {
                        map_id: mapid(4),
                        ..sh
                    }),
                    &[1],
                ),
                ErrorKind::UnknownChannel,
                field::MAP_ID,
            ),
            (
                fields(FrameType::Ad, 1),
                fdu(None, &[]),
                ErrorKind::InvalidField,
                field::FRAME_DATA_FIELD,
            ),
            (
                fields(FrameType::Ad, 1),
                fdu(None, &[0; 1018]),
                ErrorKind::OutOfRange,
                field::FRAME_LEN,
            ),
        ];
        for (f, d, kind, fld) in cases {
            let e = encode(&t, f, d, &mut out).unwrap_err();
            assert_eq!((e.kind(), e.field()), (kind, Some(fld)), "{f:?} {d:?}");
        }
        let e = encode(&t, fields(FrameType::Ad, 1), fdu(None, &[0; 10]), &mut out).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(17))
        );
        assert_eq!(out, [0xEE; 16], "no partial frame is written");
        let e = encode(
            &t,
            fields(FrameType::Ad, 1),
            fdu(None, &[0; 1018]),
            &mut out,
        )
        .unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 1024,
                found: 1025
            }
        );
    }

    #[test]
    fn two_phase_writer_with_sdls() {
        // req: API-023, API-070, API-072, GEN-014
        let t = standard(true);
        let mut out = [0xEE_u8; 32];
        let sh = SegmentHeader {
            sequence_flags: SequenceFlags::Unsegmented,
            map_id: mapid(9),
        };
        let mut w =
            TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 2), Some(sh), 5).unwrap();
        assert_eq!(w.security_header_mut(), [0, 0, 0]);
        assert_eq!(w.security_trailer_mut(), [0]);
        assert_eq!(w.user_data_mut().len(), 5);
        w.security_header_mut().copy_from_slice(&[0xA, 0xB, 0xC]);
        w.security_trailer_mut().copy_from_slice(&[0xD]);
        w.user_data_mut()[..2].copy_from_slice(&[1, 2]);
        let ctx = FrameCtx::Tc {
            scid: scid(0x155),
            vcid: vcid(2),
            map_id: None,
        };
        let done = w.protect(2, &mut NoSecurity, &ctx).unwrap().finalize();
        let n = done.len();
        assert_eq!((n, done.octets().len()), (5 + 1 + 3 + 2 + 1 + 2, n));
        assert!(!done.is_empty());
        let f = TcFrame::decode(&out[..n], &t, DecodeMode::Strict).unwrap();
        assert_eq!(f.security_header(), [0xA, 0xB, 0xC]);
        assert_eq!(
            (f.data_field(), f.security_trailer()),
            (&[1, 2][..], &[0xD][..])
        );
    }

    /// A hook that checks the frame state it is called in, and fills the Security fields.
    struct OrderHook;

    impl SecurityHook for OrderHook {
        fn protect(
            &mut self,
            _: &FrameCtx,
            frame: &mut [u8],
            fields: &SecuredFields,
        ) -> Result<()> {
            // The Frame Length is final, the Security Trailer follows the 2 octets of User Data,
            // and the FECF is not yet computed: the buffer still holds its fill.
            assert_eq!(frame.len(), 14);
            assert_eq!(
                (fields.header(), fields.data(), fields.trailer()),
                (6..9, 9..11, 11..12)
            );
            assert_eq!(frame.last_chunk::<2>(), Some(&[0xEE, 0xEE]));
            frame[fields.header()].fill(0xA5);
            frame[fields.trailer()].fill(0x5A);
            Ok(())
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            fields: &SecuredFields,
        ) -> Result<core::ops::Range<usize>> {
            Ok(fields.data())
        }
    }

    /// A hook whose protection fails.
    struct Refuse;

    impl SecurityHook for Refuse {
        fn protect(&mut self, _: &FrameCtx, _: &mut [u8], _: &SecuredFields) -> Result<()> {
            Err(aldis_core::Error::new(ErrorKind::Security))
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            fields: &SecuredFields,
        ) -> Result<core::ops::Range<usize>> {
            Ok(fields.data())
        }
    }

    #[test]
    fn a_failed_protect_gives_no_frame() {
        // req: RS-023, API-073
        let t = standard(true);
        let ctx = FrameCtx::Tc {
            scid: scid(0x155),
            vcid: vcid(2),
            map_id: None,
        };
        let sh = SegmentHeader {
            sequence_flags: SequenceFlags::Unsegmented,
            map_id: mapid(9),
        };
        let mut out = [0xEE_u8; 32];
        let w = TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 2), Some(sh), 5).unwrap();
        let e = w.protect(2, &mut Refuse, &ctx).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Security);
    }

    #[test]
    fn protect_runs_after_the_length_and_before_the_fecf() {
        // req: RS-023, API-074, API-073, TC-007
        let t = standard(true);
        let ctx = FrameCtx::Tc {
            scid: scid(0x155),
            vcid: vcid(2),
            map_id: None,
        };
        let sh = SegmentHeader {
            sequence_flags: SequenceFlags::Unsegmented,
            map_id: mapid(9),
        };
        let mut out = [0xEE_u8; 32];
        let mut w =
            TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 2), Some(sh), 5).unwrap();
        w.user_data_mut()[..2].copy_from_slice(&[1, 2]);
        let octets = w
            .protect(2, &mut OrderHook, &ctx)
            .unwrap()
            .finalize()
            .into_octets();
        assert_eq!(syndrome(octets), 0);
        let f = TcFrame::decode(octets, &t, DecodeMode::Strict).unwrap();
        assert_eq!(
            (f.security_header(), f.data_field(), f.security_trailer()),
            (&[0xA5; 3][..], &[1, 2][..], &[0x5A][..])
        );
    }

    #[test]
    fn unsecured_needs_a_channel_without_sdls() {
        // req: RS-023, API-070
        let t = standard(true);
        let sh = SegmentHeader {
            sequence_flags: SequenceFlags::Unsegmented,
            map_id: mapid(9),
        };
        let mut out = [0xEE_u8; 32];
        let w = TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 2), Some(sh), 5).unwrap();
        let e = w.unsecured(2).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (
                ErrorKind::InvalidField,
                Some(aldis_core::field::SECURITY_HEADER_LEN)
            )
        );
    }

    #[test]
    fn unfinished_and_rejected_finish() {
        // req: API-023, TC-007
        let t = standard(false);
        let mut out = [0_u8; 16];
        let w = TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 1), None, 4).unwrap();
        let e = w.unsecured(5).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::USER_LEN))
        );
        let e = TcFrame::decode(&out, &t, DecodeMode::Lenient).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
        let w = TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 1), None, 4).unwrap();
        let e = w.unsecured(0).unwrap_err();
        assert_eq!(e.field(), Some(field::FRAME_DATA_FIELD));
        let e = TcFrameWriter::begin(&mut out, &t, fields(FrameType::Bc, 1), None, 1).unwrap_err();
        assert_eq!(e.field(), Some(field::CONTROL_COMMAND_FLAG));
        let mut w = TcFrameWriter::begin(&mut out, &t, fields(FrameType::Ad, 1), None, 4).unwrap();
        w.user_data_mut()[0] = 0x42;
        assert_eq!(w.unsecured(1).unwrap().finalize().len(), 6);
        assert_eq!(TcFrame::peek_length(&out), Ok(6));
    }
}
