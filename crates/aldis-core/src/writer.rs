//! The frame writer shared by the four frame codecs (RS-023, RS-021, API-020…API-023, API-074).
//!
//! [`FrameWriter`] holds a frame in a caller's buffer, laid out by [`FrameLayout`], and carries it
//! through the finishing states of [`crate::finishing`]: [`Unprotected`], then [`Protected`], then
//! [`Finalized`]. It writes the SDLS step (the hook, [`require_layout`]), the FHEC through the
//! plan's [`FinishPlan::seal`], and the FECF once, for every codec. A codec supplies its
//! [`FramePlan`], which says how its frame is laid out, and its [`FinishPlan`], which says how the
//! frame is finished; a plan with no [`FinishPlan`] ([`Unsealed`]) has no `protect`, `unsecured`,
//! or `finalize`.
//!
//! Two kinds of frame differ in how the length is fixed, and the marker type [`Fixed`] or
//! [`Variable`] states which:
//!
//! - [`Fixed`] (TM, AOS, and the frames of the `ChannelMux`): the data field fills the frame, the
//!   header is final at `reserve`, and `protect(hook, ctx)` seals in place.
//! - [`Variable`] (TC, USLP): `reserve` takes the longest frame, the header is unfinished
//!   (the codec's [`VariablePlan::write_header`]), and `protect(data_len, hook, ctx)` or
//!   `unsecured(data_len)` fix the data length: they move the Security Trailer, write the final
//!   header (the Frame Length, the version, the OCF a codec places by the length), and apply SDLS.
//!
//! A frame that the `ChannelMux` builds ([`Unsealed`]) stays [`Unprotected`] and leaves the
//! writer with [`FrameWriter::leave_unsealed`]: its Security Header and Trailer, FHEC, and FECF
//! are zero (LNK-035).
//!
//! Each transition consumes the writer, so a step out of order does not compile.

use core::fmt::Debug;
use core::marker::PhantomData;

use crate::crc::crc16;
use crate::error::{Error, ErrorKind, FieldId, Result};
use crate::field;
use crate::finishing::{
    Finalized, FrameState, Protected, Unprotected, require_layout, require_unsecured,
};
use crate::layout::{FECF_LEN, FrameLayout};
use crate::ocf::Ocf;
use crate::security::{FrameCtx, SecuredFields, SecurityHook};

mod sealed {
    /// Keeps [`Length`](super::Length) closed to [`Fixed`](super::Fixed) and
    /// [`Variable`](super::Variable).
    pub trait Sealed {}
}

/// How the length of a frame is fixed: [`Fixed`] or [`Variable`] (RS-021). Sealed.
pub trait Length: sealed::Sealed {}

/// The frame fills the reserved length, as in TM and AOS (132.0 §4.1; 732.0 §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixed {}

/// The frame is shorter than the reservation by the data field not used, as in TC and USLP
/// (232.0 §4.1.2.7; 732.1 §4.1.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variable {}

impl sealed::Sealed for Fixed {}
impl sealed::Sealed for Variable {}
impl Length for Fixed {}
impl Length for Variable {}

/// The layout half of the plan of one frame that a codec supplies to [`FrameWriter`]: what the
/// caller sees while it fills the frame (RS-023). The finishing half is [`FinishPlan`].
pub trait FramePlan: Debug {
    /// The field an error names when [`FrameWriter::set_ocf`] finds no OCF in the frame: the
    /// `ocf_flag` of TM, the `ocf` of AOS.
    const OCF_FIELD: FieldId = field::OCF_FLAG;

    /// The octets at the start of the secondary slot that the plan writes itself: the TM Secondary
    /// Header Identification Field (132.0 §4.1.3.2), 0 for the others. See
    /// [`FrameWriter::secondary_data_mut`].
    const SECONDARY_ID_LEN: usize = 0;
}

/// The finishing half of the plan of one frame (RS-023, RS-021).
///
/// It holds the part of the finishing that depends on the protocol. Only a plan with this trait
/// has [`FrameWriter::protect`], [`unsecured`](FrameWriter::unsecured), and
/// [`finalize`](FrameWriter::finalize); [`Unsealed`] has none, so a frame whose FHEC and FECF
/// stay zero cannot be finished by mistake.
///
/// # Examples
///
/// A plan with this trait calls `protect`, `unsecured`, and `finalize`:
///
/// ```
/// # use aldis_core::ids::{SpacecraftId, VcId};
/// # use aldis_core::layout::{FrameLayout, LayoutSpec};
/// # use aldis_core::security::{FrameCtx, NoSecurity, SecurityLayout};
/// # use aldis_core::writer::{FinishPlan, FramePlan, FrameWriter};
/// #[derive(Debug)]
/// struct Codec;
/// impl FramePlan for Codec {}
/// impl FinishPlan for Codec {
///     type OneShot = ();
/// }
/// # let spec = LayoutSpec {
/// #     frame_len: 0,
/// #     primary_header_len: 2,
/// #     secondary_len: 0,
/// #     security: SecurityLayout::NONE,
/// #     ocf: false,
/// #     fecf: false,
/// # };
/// # let layout = FrameLayout::compute_for_data(&spec, 4).unwrap();
/// # let mut out = [0_u8; 8];
/// # let ctx = FrameCtx::Tm {
/// #     scid: SpacecraftId::<10>::new(1).unwrap(),
/// #     vcid: VcId::<3>::new(0).unwrap(),
/// # };
/// let w = FrameWriter::reserve_fixed(&mut out, layout, Codec).unwrap();
/// let _frame = w.protect(&mut NoSecurity, &ctx).unwrap().finalize();
/// ```
///
/// The same code with [`Unsealed`], which is no `FinishPlan`, does not compile: there is no
/// `protect`, and so no `finalize`.
///
/// ```compile_fail
/// # use aldis_core::ids::{SpacecraftId, VcId};
/// # use aldis_core::layout::{FrameLayout, LayoutSpec};
/// # use aldis_core::security::{FrameCtx, NoSecurity, SecurityLayout};
/// # use aldis_core::writer::{FrameWriter, Unsealed};
/// # let spec = LayoutSpec {
/// #     frame_len: 0,
/// #     primary_header_len: 2,
/// #     secondary_len: 0,
/// #     security: SecurityLayout::NONE,
/// #     ocf: false,
/// #     fecf: false,
/// # };
/// # let layout = FrameLayout::compute_for_data(&spec, 4).unwrap();
/// # let mut out = [0_u8; 8];
/// # let ctx = FrameCtx::Tm {
/// #     scid: SpacecraftId::<10>::new(1).unwrap(),
/// #     vcid: VcId::<3>::new(0).unwrap(),
/// # };
/// let w = FrameWriter::reserve_fixed(&mut out, layout, Unsealed).unwrap();
/// let _frame = w.protect(&mut NoSecurity, &ctx).unwrap().finalize();
/// ```
///
/// Nor does `unsecured`:
///
/// ```compile_fail
/// # use aldis_core::layout::{FrameLayout, LayoutSpec};
/// # use aldis_core::security::SecurityLayout;
/// # use aldis_core::writer::{FrameWriter, Unsealed};
/// # let spec = LayoutSpec {
/// #     frame_len: 0,
/// #     primary_header_len: 2,
/// #     secondary_len: 0,
/// #     security: SecurityLayout::NONE,
/// #     ocf: false,
/// #     fecf: false,
/// # };
/// # let layout = FrameLayout::compute_for_data(&spec, 4).unwrap();
/// # let mut out = [0_u8; 8];
/// let w = FrameWriter::reserve_fixed(&mut out, layout, Unsealed).unwrap();
/// let _ = w.unsecured();
/// ```
pub trait FinishPlan: FramePlan {
    /// The proof, which only the plan's crate can build, that the caller is a one-shot encoder:
    /// [`FrameWriter::taken_as_given`] takes it and so cannot be called from outside the codec
    /// (RS-023, API-073). A codec declares a type with a private constructor.
    type OneShot;

    /// Completes the header before the FECF, the AOS FHEC (732.0 §4.2.7.4; AOS-013). `frame` is
    /// the whole frame, its FECF not yet written. Nothing by default.
    fn seal(&self, _frame: &mut [u8]) {}
}

/// The plan of a [`Variable`] frame: it writes the header again when the length is fixed
/// (RS-023, API-023).
pub trait VariablePlan: FinishPlan {
    /// Writes the Primary Header, and whatever else depends on the frame length (the USLP OCF at
    /// the end of the frame), for a frame of `total` octets: its final form when `finished`, the
    /// unfinished form, which no decoder accepts, otherwise (API-023). `frame` holds `total`
    /// octets when `finished`, the reserved length otherwise.
    fn write_header(&self, frame: &mut [u8], total: usize, finished: bool);

    /// Checks the codec's rules for a data field of `data_len` octets, after the writer has
    /// checked that it is not 0 and not above the reservation.
    ///
    /// # Errors
    /// The codec's, such as `LengthMismatch` (`frame_data_field`) below the reservation of a
    /// fixed-length USLP channel.
    fn check_data_len(&self, data_len: usize) -> Result<()>;
}

/// The plan of a frame whose FHEC and FECF stay zero for the caller (LNK-035).
///
/// The `ChannelMux` builds its frames with it and ends them with [`FrameWriter::leave_unsealed`],
/// which only this plan has. It is a [`FramePlan`] and no [`FinishPlan`], so `protect`,
/// `unsecured`, and `finalize` do not exist for it: they would compute a FECF and no FHEC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsealed;

impl FramePlan for Unsealed {}

/// A frame in a caller's buffer in the finishing state `S` (RS-023; see the [module](self)).
///
/// `L` says whether the frame is [`Fixed`] or [`Variable`]; `P` is the codec's plan.
#[derive(Debug)]
pub struct FrameWriter<'b, L: Length, P: FramePlan, S: FrameState = Unprotected> {
    /// Exactly the reserved frame length.
    frame: &'b mut [u8],
    /// The layout of the reserved frame.
    layout: FrameLayout,
    /// The data field octets of the frame: the whole data zone, until a [`Variable`] frame fixes it.
    data_len: usize,
    /// The frame length: the reserved one until the data length is fixed.
    total: usize,
    plan: P,
    state: PhantomData<(L, S)>,
}

/// The number of octets as `u64`, for an error detail.
fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

impl<'b, L: Length, P: FramePlan, S: FrameState> FrameWriter<'b, L, P, S> {
    /// The same frame in state `T`: the one place a state changes.
    fn into_state<T: FrameState>(self) -> FrameWriter<'b, L, P, T> {
        FrameWriter {
            frame: self.frame,
            layout: self.layout,
            data_len: self.data_len,
            total: self.total,
            plan: self.plan,
            state: PhantomData,
        }
    }
}

impl<'b, L: Length, P: FramePlan> FrameWriter<'b, L, P, Unprotected> {
    /// Takes the first frame-length octets of `out` for the frame of `layout`.
    fn take(
        out: &'b mut [u8],
        layout: FrameLayout,
        plan: P,
    ) -> Result<FrameWriter<'b, L, P, Unprotected>> {
        let len = layout.frame_len();
        let frame = out.get_mut(..len).ok_or(Error::buffer_too_small(len))?;
        Ok(FrameWriter {
            frame,
            data_len: layout.data_zone().len(),
            layout,
            total: len,
            plan,
            state: PhantomData,
        })
    }

    /// The octets of `range`, a range of the layout, which lies inside the frame by construction
    /// (CMN-040).
    ///
    /// `take` checked once that the buffer holds `layout.frame_len()` octets, so no range of the
    /// layout lies outside it (`FrameWriter::take`, `BufferTooSmall`).
    fn field_mut(&mut self, range: core::ops::Range<usize>) -> &mut [u8] {
        self.frame.get_mut(range).unwrap_or_default()
    }

    /// The octet ranges of the frame's fields, for a security hook (CMN-040, API-074). For a
    /// [`Variable`] frame the layout of the reservation, a provisional one: fixing the data
    /// length shortens the frame and moves the Security Trailer, the OCF, and the FECF.
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// The Primary Header to fill in place (CMN-040).
    pub fn primary_header_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.primary_header())
    }

    /// The secondary slot to fill in place: the TM Secondary Header with its Identification
    /// Field, the AOS or USLP Insert Zone, or the TC Segment Header; empty if absent (CMN-040).
    pub fn secondary_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.secondary())
    }

    /// The secondary slot after the octets the plan writes itself ([`FramePlan::SECONDARY_ID_LEN`]):
    /// the TM Secondary Header Data Field to fill in place; empty if the slot is absent (CMN-040).
    pub fn secondary_data_mut(&mut self) -> &mut [u8] {
        self.secondary_mut()
            .get_mut(P::SECONDARY_ID_LEN..)
            .unwrap_or_default()
    }

    /// The Security Header to fill in place; empty without SDLS (API-072).
    pub fn security_header_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.security_header())
    }

    /// The whole data field to fill in place, its header included, never empty (GEN-014).
    pub fn data_field_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.data())
    }

    /// The header at the start of the data field (the AOS `M_PDU` or `B_PDU` Header, the USLP TFDF
    /// Header) to fill in place; empty if the layout has none (CMN-040).
    pub fn data_header_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.data_header())
    }

    /// The data field after its header to fill in place: the TM data field, the AOS zone, the TC
    /// User Data, the USLP TFDZ, never empty; for a [`Variable`] frame the reserved maximum
    /// (GEN-014).
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.data_zone())
    }

    /// The Security Trailer to fill in place; empty without SDLS. For a [`Variable`] frame it
    /// lies after the reserved data and moves to follow the data with the data length (API-072).
    pub fn security_trailer_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.security_trailer())
    }
}

impl<'b, P: FramePlan> FrameWriter<'b, Fixed, P, Unprotected> {
    /// Takes the first frame-length octets of `out` for the frame of `layout`, with every octet
    /// zero until the caller fills it (API-023).
    ///
    /// # Errors
    /// `BufferTooSmall` (`needed` = the frame length) if `out` is shorter than the frame; `out`
    /// is then unchanged.
    pub fn reserve_fixed(
        out: &'b mut [u8],
        layout: FrameLayout,
        plan: P,
    ) -> Result<FrameWriter<'b, Fixed, P, Unprotected>> {
        let writer = FrameWriter::take(out, layout, plan)?;
        writer.frame.fill(0);
        Ok(writer)
    }

    /// The OCF to fill in place; empty if the frame has none (CMN-040).
    pub fn ocf_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.ocf())
    }

    /// Writes the Operational Control Field (132.0 §4.1.5; 732.0 §4.1.5).
    ///
    /// # Errors
    /// `InvalidField` ([`FramePlan::OCF_FIELD`]) if the frame has no OCF.
    pub fn set_ocf(&mut self, ocf: Ocf) -> Result<()> {
        let range = self.layout.ocf();
        if range.is_empty() {
            return Err(Error::new(ErrorKind::InvalidField)
                .with_field(P::OCF_FIELD)
                .with_detail(crate::error::ErrorDetail::Expected {
                    expected: 0,
                    found: 1,
                }));
        }
        for (d, s) in self.field_mut(range).iter_mut().zip(ocf.to_octets()) {
            *d = s;
        }
        Ok(())
    }
}

impl<'b, P: FinishPlan> FrameWriter<'b, Fixed, P, Unprotected> {
    /// Applies `hook` to the Security Header, the data field, and the Security Trailer, after
    /// every other field except the FHEC and the FECF is final (355.0 §2.3; API-074, RS-023).
    /// `ctx` identifies the channel for the hook's Security Association.
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len` or `security_trailer_len`) if the hook is for
    /// another layout than the frame's ([`SecurityHook::layout`]), such as `NoSecurity` on a
    /// frame with SDLS fields; the hook's error, `Security`; the frame is then dropped and must
    /// not be transmitted.
    pub fn protect<H: SecurityHook + ?Sized>(
        self,
        hook: &mut H,
        ctx: &FrameCtx,
    ) -> Result<FrameWriter<'b, Fixed, P, Protected>> {
        let fields = SecuredFields::from_layout(&self.layout);
        let result = require_layout(
            hook.layout(),
            self.layout.security_header().len(),
            self.layout.security_trailer().len(),
        )
        .and_then(|()| hook.protect(ctx, &mut *self.frame, &fields));
        result.map(|()| self.into_state())
    }

    /// Moves to [`Protected`] on a channel without SDLS, the `NoSecurity` path (API-073, RS-023).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len` or `security_trailer_len`) if the frame reserves a
    /// Security Header or Trailer, which need [`protect`](Self::protect).
    pub fn unsecured(self) -> Result<FrameWriter<'b, Fixed, P, Protected>> {
        require_unsecured(
            self.layout.security_header().len(),
            self.layout.security_trailer().len(),
        )?;
        Ok(self.into_state())
    }

    /// Moves to [`Protected`] with the Security Header and Trailer as the caller filled them, with
    /// no hook and no layout check, for the one-shot encoders, which take them as given
    /// (API-072). The two-phase path is [`protect`](Self::protect) or
    /// [`unsecured`](Self::unsecured). The [`FinishPlan::OneShot`] proof only the codec crate can
    /// build keeps every other caller from skipping the SDLS step (RS-023, API-073).
    #[must_use = "the protected frame"]
    pub fn taken_as_given(self, _: P::OneShot) -> FrameWriter<'b, Fixed, P, Protected> {
        self.into_state()
    }
}

impl<'b> FrameWriter<'b, Fixed, Unsealed, Unprotected> {
    /// Ends the frame without the SDLS step and the finishing: the Security Header and Trailer,
    /// the FHEC, and the FECF are the zero octets `reserve_fixed` wrote, or what the caller put
    /// there, for the caller to apply SDLS and finish (LNK-035). Only the [`Unsealed`] plan has
    /// it: a codec's frame leaves its writer only `Finalized` (RS-023).
    ///
    /// The same code compiles with the [`Unsealed`] plan, so the `compile_fail` example
    /// after it fails only for the missing method.
    ///
    /// ```
    /// # use aldis_core::layout::{FrameLayout, LayoutSpec};
    /// # use aldis_core::security::SecurityLayout;
    /// # use aldis_core::writer::{FrameWriter, Unsealed};
    /// let spec = LayoutSpec {
    ///     frame_len: 0,
    ///     primary_header_len: 2,
    ///     secondary_len: 0,
    ///     security: SecurityLayout::NONE,
    ///     ocf: false,
    ///     fecf: false,
    /// };
    /// let layout = FrameLayout::compute_for_data(&spec, 4).unwrap();
    /// let mut out = [0_u8; 8];
    /// let w = FrameWriter::reserve_fixed(&mut out, layout, Unsealed).unwrap();
    /// let _frame = w.leave_unsealed();
    /// ```
    ///
    /// ```compile_fail
    /// # use aldis_core::finishing::Unprotected;
    /// # use aldis_core::layout::{FrameLayout, LayoutSpec};
    /// # use aldis_core::security::SecurityLayout;
    /// # use aldis_core::writer::{FinishPlan, FramePlan, FrameWriter};
    /// #[derive(Debug)]
    /// struct Codec;
    /// impl FramePlan for Codec {}
    /// impl FinishPlan for Codec {
    ///     type OneShot = ();
    /// }
    /// let spec = LayoutSpec {
    ///     frame_len: 0,
    ///     primary_header_len: 2,
    ///     secondary_len: 0,
    ///     security: SecurityLayout::NONE,
    ///     ocf: false,
    ///     fecf: false,
    /// };
    /// let layout = FrameLayout::compute_for_data(&spec, 4).unwrap();
    /// let mut out = [0_u8; 8];
    /// let w = FrameWriter::reserve_fixed(&mut out, layout, Codec).unwrap();
    /// // A codec's plan has no `leave_unsealed`: the frame is not finished.
    /// let _frame = w.leave_unsealed();
    /// ```
    #[must_use = "the frame"]
    pub const fn leave_unsealed(self) -> &'b mut [u8] {
        self.frame
    }
}

impl<'b, P: VariablePlan> FrameWriter<'b, Variable, P, Unprotected> {
    /// Reserves the frame of `layout`, the longest, for a data field of up to
    /// `layout.data_zone().len()` octets: writes the unfinished header (API-023) and zeros in the
    /// Security Header and Trailer; the rest of the reserved frame is as `out` was.
    ///
    /// # Errors
    /// `BufferTooSmall` (`needed` = the frame length) if `out` cannot hold the longest frame;
    /// `out` is then unchanged.
    pub fn reserve_variable(
        out: &'b mut [u8],
        layout: FrameLayout,
        plan: P,
    ) -> Result<FrameWriter<'b, Variable, P, Unprotected>> {
        let mut writer = FrameWriter::take(out, layout, plan)?;
        let total = writer.total;
        writer.plan.write_header(writer.frame, total, false);
        writer.security_header_mut().fill(0);
        writer.security_trailer_mut().fill(0);
        Ok(writer)
    }

    /// Fixes the data field at `data_len` octets and applies `hook` to the Security Header, the
    /// data field, and the Security Trailer, after every other field except the FECF is final
    /// (232.0 §4.1.2.7, §6.3; 732.1 §4.1.2.7, §6.3; API-074, RS-023). `ctx` identifies the
    /// channel for the hook's Security Association.
    ///
    /// When `data_len` is below the reservation, the Security Trailer moves to follow the data
    /// and the reserved octets after the frame keep unspecified values, which may include octets
    /// of the Security Trailer as it was before the move; use only the finalized frame's octets.
    ///
    /// # Errors
    /// As [`unsecured`](Self::unsecured), without the SDLS check, and the hook's error,
    /// `Security`, after which the frame is dropped and must not be transmitted. On the other
    /// errors the buffer keeps its unfinished state; after the hook's error it holds the
    /// unfinished header again (API-023).
    pub fn protect<H: SecurityHook + ?Sized>(
        self,
        data_len: usize,
        hook: &mut H,
        ctx: &FrameCtx,
    ) -> Result<FrameWriter<'b, Variable, P, Protected>> {
        require_layout(
            hook.layout(),
            self.layout.security_header().len(),
            self.layout.security_trailer().len(),
        )
        .and_then(|()| self.fix_length(data_len))
        .and_then(|mut w| {
            let (frame, fields) = w.secured();
            let result = hook.protect(ctx, frame, &fields);
            w.checked(result)
        })
    }

    /// Fixes the data field at `data_len` octets on a channel without SDLS, the `NoSecurity` path
    /// (API-073, RS-023); the frame is then [`Protected`].
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len` or `security_trailer_len`) if the frame reserves a
    /// Security Header or Trailer, which need [`protect`](Self::protect); `OutOfRange`
    /// (`user_len`) above the reservation; `InvalidField` (`frame_data_field`) for 0; the
    /// codec's own check of the length ([`VariablePlan::check_data_len`]). On error the buffer
    /// keeps its unfinished state.
    pub fn unsecured(self, data_len: usize) -> Result<FrameWriter<'b, Variable, P, Protected>> {
        require_unsecured(
            self.layout.security_header().len(),
            self.layout.security_trailer().len(),
        )?;
        self.fix_length(data_len)
    }

    /// Fixes the data field at `data_len` octets with the Security Header and Trailer as the
    /// caller filled them, with no hook and no layout check, for the one-shot encoders, which
    /// take them as given (API-072).
    ///
    /// # Errors
    /// As [`unsecured`](Self::unsecured), without the SDLS check. The [`FinishPlan::OneShot`] proof
    /// only the codec crate can build keeps every other caller from skipping the SDLS step
    /// (RS-023, API-073).
    pub fn taken_as_given(
        self,
        data_len: usize,
        _: P::OneShot,
    ) -> Result<FrameWriter<'b, Variable, P, Protected>> {
        self.fix_length(data_len)
    }

    /// Fixes the length: moves the Security Trailer to follow `data_len` octets of data and
    /// writes the final header (232.0 §4.1.2.7; 732.1 §4.1.2.7; API-023).
    fn fix_length(self, data_len: usize) -> Result<FrameWriter<'b, Variable, P, Protected>> {
        let zone = self.layout.data_zone();
        let max = zone.len();
        if data_len > max {
            return Err(Error::out_of_range(
                field::USER_LEN,
                len_u64(max),
                len_u64(data_len),
            ));
        }
        if data_len == 0 {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::FRAME_DATA_FIELD));
        }
        self.plan.check_data_len(data_len)?;
        let mut w = self;
        let unused = max.saturating_sub(data_len);
        let total = w.layout.frame_len().saturating_sub(unused);
        // The unused data and the Security Trailer after it: move the trailer to the front.
        let tail_start = zone.start.saturating_add(data_len);
        let tail_end = tail_start
            .saturating_add(unused)
            .saturating_add(w.layout.security_trailer().len());
        // The ranges are the layout's own, inside the buffer sized to it at `reserve`: no wire input.
        let tail = w.frame.get_mut(tail_start..tail_end).unwrap_or_default();
        let from = unused.min(tail.len());
        tail.copy_within(from.., 0);
        w.plan
            .write_header(w.frame.get_mut(..total).unwrap_or_default(), total, true);
        w.total = total;
        w.data_len = data_len;
        Ok(w.into_state())
    }
}

impl<P: VariablePlan> FrameWriter<'_, Variable, P, Protected> {
    /// The frame and the ranges of its secured fields, for the hook of `protect`.
    fn secured(&mut self) -> (&mut [u8], SecuredFields) {
        let header = self.layout.security_header();
        let data_len = self
            .layout
            .data_zone()
            .start
            .saturating_sub(header.end)
            .saturating_add(self.data_len);
        let fields = SecuredFields::adjacent(
            header.start,
            header.len(),
            data_len,
            self.layout.security_trailer().len(),
        );
        // The range is the layout's own, inside the buffer sized to it at `reserve`: no wire input.
        (self.frame.get_mut(..self.total).unwrap_or_default(), fields)
    }

    /// The writer if the hook succeeded; the part of `protect` that does not depend on the hook
    /// type. After a failure the buffer holds the unfinished header again, which every decoder
    /// rejects (API-023), not a final-looking frame.
    fn checked(self, result: Result<()>) -> Result<Self> {
        match result {
            Ok(()) => Ok(self),
            Err(e) => {
                let reserved = self.layout.frame_len();
                self.plan.write_header(self.frame, reserved, false);
                Err(e)
            }
        }
    }
}

impl<'b, L: Length, P: FinishPlan> FrameWriter<'b, L, P, Protected> {
    /// Completes the frame: computes the FHEC, if the plan has one ([`FinishPlan::seal`]), then the
    /// FECF, if configured, over every other octet, last (132.0 §4.1.6.2; 732.0 §4.1.2.6; 232.0
    /// §4.1.4; 732.1 §4.1.6; API-021, API-074, RS-023).
    #[must_use = "the finalized frame"]
    pub fn finalize(self) -> FrameWriter<'b, L, P, Finalized> {
        let w = self;
        // The range is the layout's own, inside the buffer sized to it at `reserve`: no wire input.
        let frame = w.frame.get_mut(..w.total).unwrap_or_default();
        w.plan.seal(frame);
        if !w.layout.fecf().is_empty() {
            // The frame holds its FECF, so the split is the one chunk: no wire input.
            frame
                .split_at_mut_checked(w.total.saturating_sub(FECF_LEN))
                .into_iter()
                .for_each(|(body, fecf)| {
                    for (d, s) in fecf.iter_mut().zip(crc16(body).to_be_bytes()) {
                        *d = s;
                    }
                });
        }
        w.into_state()
    }
}

impl<'b, L: Length, P: FramePlan> FrameWriter<'b, L, P, Finalized> {
    /// The frame length (RS-023).
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
        // The range is the layout's own, inside the buffer sized to it at `reserve`: no wire input.
        self.frame.get(..self.total).unwrap_or_default()
    }

    /// The octets of the finalized frame, borrowed from the buffer `reserve` took, for
    /// transmission (RS-023).
    #[must_use]
    pub fn into_octets(self) -> &'b [u8] {
        let total = self.total;
        // The range is the layout's own, inside the buffer sized to it at `reserve`: no wire input.
        self.frame.get(..total).unwrap_or_default()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FinishPlan, FramePlan, FrameWriter, Unsealed, VariablePlan};
    use crate::error::{ErrorKind, Result};
    use crate::field;
    use crate::layout::{FECF_LEN, FrameLayout, LayoutSpec};
    use crate::ocf::Ocf;
    use crate::security::{FrameCtx, HookLayout, SecuredFields, SecurityHook, SecurityLayout};

    fn spec(security: SecurityLayout, ocf: bool, fecf: bool, secondary: usize) -> LayoutSpec {
        LayoutSpec {
            frame_len: 0,
            primary_header_len: 3,
            secondary_len: secondary,
            security,
            ocf,
            fecf,
        }
    }

    fn layout(s: LayoutSpec, data_len: usize) -> FrameLayout {
        FrameLayout::compute_for_data(&s, data_len).unwrap()
    }

    #[derive(Debug)]
    struct Fix;

    impl FramePlan for Fix {
        const OCF_FIELD: crate::error::FieldId = field::OCF;
    }

    impl FinishPlan for Fix {
        type OneShot = ();

        fn seal(&self, frame: &mut [u8]) {
            if let Some(b) = frame.get_mut(2) {
                *b = 0xEE;
            }
        }
    }

    /// A plan with the default layout and no sealing.
    #[derive(Debug)]
    struct Plain;

    impl FramePlan for Plain {}

    impl FinishPlan for Plain {
        type OneShot = ();
    }

    /// A variable plan: the header is `[version, total, 0]`; the length rule is at most 6.
    #[derive(Debug)]
    struct Var;

    impl FramePlan for Var {
        const OCF_FIELD: crate::error::FieldId = field::OCF;
    }

    impl FinishPlan for Var {
        type OneShot = ();
    }

    impl VariablePlan for Var {
        fn write_header(&self, frame: &mut [u8], total: usize, finished: bool) {
            let version = if finished { 0 } else { 3 };
            let t = u8::try_from(total).unwrap();
            for (d, s) in frame.iter_mut().zip([version, t, 0]) {
                *d = s;
            }
        }

        fn check_data_len(&self, data_len: usize) -> Result<()> {
            if data_len == 5 {
                Err(crate::error::Error::new(ErrorKind::LengthMismatch))
            } else {
                Ok(())
            }
        }
    }

    /// A hook for any layout, or for none, that fails or not.
    struct Hook {
        layout: HookLayout,
        fail: bool,
    }

    fn ok() -> Hook {
        Hook {
            layout: HookLayout::Any,
            fail: false,
        }
    }

    fn failing() -> Hook {
        Hook {
            layout: HookLayout::Any,
            fail: true,
        }
    }

    fn none() -> Hook {
        Hook {
            layout: HookLayout::Exact(SecurityLayout::NONE),
            fail: false,
        }
    }

    impl SecurityHook for Hook {
        fn layout(&self) -> HookLayout {
            self.layout
        }

        fn protect(&mut self, _: &FrameCtx, _: &mut [u8], _: &SecuredFields) -> Result<()> {
            if self.fail {
                Err(crate::error::Error::new(ErrorKind::Security))
            } else {
                Ok(())
            }
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            _: &SecuredFields,
        ) -> Result<core::ops::Range<usize>> {
            Err(crate::error::Error::new(ErrorKind::Security))
        }
    }

    fn ctx() -> FrameCtx {
        FrameCtx::Tm {
            scid: crate::ids::SpacecraftId::new(1).unwrap(),
            vcid: crate::ids::VcId::new(0).unwrap(),
        }
    }

    #[test]
    fn a_fixed_frame_is_finished_in_order() {
        // req: RS-023, API-074, CMN-002
        let l = layout(spec(SecurityLayout::NONE, true, true, 0), 5);
        let mut out = [0xFF_u8; 20];
        let mut w = FrameWriter::reserve_fixed(&mut out, l, Fix).unwrap();
        assert_eq!(w.layout().frame_len(), 14);
        w.primary_header_mut().copy_from_slice(&[1, 2, 3]);
        w.data_mut().fill(7);
        w.set_ocf(Ocf::decode([9, 9, 9, 9])).unwrap();
        assert_eq!(w.ocf_mut(), &[9, 9, 9, 9]);
        let done = w.protect(&mut ok(), &ctx()).unwrap().finalize();
        assert_eq!((done.len(), done.is_empty()), (14, false));
        let body = done.octets().to_vec();
        // The seal wrote octet 2 before the FECF over all the octets before it.
        assert_eq!(&body[..3], &[1, 2, 0xEE]);
        assert_eq!(crate::crc::syndrome(&body).get(), 0);
        assert_eq!(done.into_octets(), &body[..]);
        // Octets after the frame are untouched.
        assert_eq!(&out[14..], &[0xFF; 6]);
    }

    #[test]
    fn a_frame_taken_as_given_keeps_its_security_octets() {
        // req: RS-023, API-072
        let sec = SecurityLayout::new(1, 1).unwrap();
        let l = layout(spec(sec, false, true, 0), 4);
        let mut out = [0_u8; 20];
        let mut w = FrameWriter::reserve_fixed(&mut out, l.clone(), Fix).unwrap();
        w.security_header_mut().fill(5);
        let done = w.taken_as_given(()).finalize();
        assert_eq!(done.octets()[3], 5);
        let mut w = FrameWriter::reserve_variable(&mut out, l, Var).unwrap();
        w.security_header_mut().fill(6);
        let done = w.taken_as_given(3, ()).unwrap().finalize();
        assert_eq!(done.len(), 10);
        assert_eq!(done.octets()[3], 6);
    }

    #[test]
    fn the_secondary_data_follows_the_identification_octets() {
        // req: CMN-040
        #[derive(Debug)]
        struct Id;
        impl FramePlan for Id {
            const SECONDARY_ID_LEN: usize = 1;
        }
        let l = layout(spec(SecurityLayout::NONE, false, false, 3), 2);
        let mut out = [0_u8; 20];
        let mut w = FrameWriter::reserve_fixed(&mut out, l, Id).unwrap();
        assert_eq!(w.secondary_data_mut().len(), 2);
        let mut w = FrameWriter::reserve_fixed(
            &mut out,
            layout(spec(SecurityLayout::NONE, false, false, 0), 2),
            Id,
        )
        .unwrap();
        assert_eq!(w.secondary_data_mut().len(), 0);
    }

    #[test]
    fn a_fixed_frame_without_fecf_or_ocf() {
        // req: RS-023
        let l = layout(spec(SecurityLayout::NONE, false, true, 1), 4);
        let mut out = [0xFF_u8; 12];
        let mut w = FrameWriter::reserve_fixed(&mut out, l, Plain).unwrap();
        assert_eq!(
            w.set_ocf(Ocf::decode([0; 4])).unwrap_err().field(),
            Some(field::OCF_FLAG)
        );
        let with_ocf = layout(spec(SecurityLayout::NONE, true, false, 0), 2);
        let mut ocf_out = [0_u8; 12];
        let mut o = FrameWriter::reserve_fixed(&mut ocf_out, with_ocf, Plain).unwrap();
        o.set_ocf(Ocf::decode([1, 2, 3, 4])).unwrap();
        assert_eq!(o.ocf_mut(), &[1, 2, 3, 4]);
        assert_eq!(w.secondary_mut().len(), 1);
        assert_eq!(w.data_field_mut().len(), 4);
        assert_eq!(w.data_header_mut().len(), 0);
        let l = layout(spec(SecurityLayout::NONE, false, false, 0), 2);
        let mut small = [0_u8; 8];
        let mut f = FrameWriter::reserve_fixed(&mut small, l.clone(), Fix).unwrap();
        assert_eq!(
            f.set_ocf(Ocf::decode([0; 4])).unwrap_err().field(),
            Some(field::OCF)
        );
        let plain = FrameWriter::reserve_fixed(&mut small, l, Fix).unwrap();
        assert_eq!(plain.unsecured().unwrap().finalize().len(), 5);
        let done = w.unsecured().unwrap().finalize();
        assert_eq!(done.len(), 10);
        assert_eq!(&done.octets()[..8], &[0; 8]);
    }

    #[test]
    fn a_fixed_frame_needs_its_sdls_and_its_room() {
        // req: RS-023, API-070, API-073
        let sec = SecurityLayout::new(2, 1).unwrap();
        let l = layout(spec(sec, false, true, 0), 3);
        let mut small = [0_u8; 5];
        assert_eq!(
            FrameWriter::reserve_fixed(&mut small, l.clone(), Fix)
                .unwrap_err()
                .kind(),
            ErrorKind::BufferTooSmall
        );
        let mut out = [0_u8; 20];
        let w = FrameWriter::reserve_fixed(&mut out, l.clone(), Fix).unwrap();
        assert_eq!(
            w.unsecured().unwrap_err().field(),
            Some(field::SECURITY_HEADER_LEN)
        );
        let w = FrameWriter::reserve_fixed(&mut out, l.clone(), Fix).unwrap();
        assert_eq!(
            w.protect(&mut none(), &ctx()).unwrap_err().field(),
            Some(field::SECURITY_HEADER_LEN)
        );
        let w = FrameWriter::reserve_fixed(&mut out, l, Fix).unwrap();
        assert_eq!(
            w.protect(&mut failing(), &ctx()).unwrap_err().kind(),
            ErrorKind::Security
        );
    }

    #[test]
    fn an_unsealed_frame_leaves_with_zero_finishing_octets() {
        // req: LNK-035
        let l = layout(spec(SecurityLayout::new(2, 1).unwrap(), false, true, 0), 3);
        let mut out = [0xFF_u8; 12];
        let w = FrameWriter::reserve_fixed(&mut out, l, Unsealed).unwrap();
        let frame = w.leave_unsealed();
        assert_eq!(frame, &[0; 11][..]);
    }

    #[test]
    fn a_variable_frame_moves_its_trailer_and_rewrites_its_header() {
        // req: RS-023, TC-007, USLP-010
        let sec = SecurityLayout::new(1, 2).unwrap();
        let l = layout(spec(sec, false, true, 0), 6);
        let reserved = l.frame_len();
        let mut out = [0x55_u8; 32];
        let mut w = FrameWriter::reserve_variable(&mut out, l, Var).unwrap();
        // The unfinished header, and zero Security Header and Trailer.
        assert_eq!(
            &w.primary_header_mut()[..],
            &[3, u8::try_from(reserved).unwrap(), 0]
        );
        assert_eq!(w.security_header_mut(), &[0]);
        w.security_trailer_mut().copy_from_slice(&[8, 9]);
        w.data_mut().copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        let w = w.protect(2, &mut ok(), &ctx()).unwrap();
        let done = w.finalize();
        // Trailer is not moved by PreFilled: the trailer octets moved to follow 2 octets.
        let n = done.len();
        assert_eq!(n, reserved - 4);
        let o = done.octets();
        assert_eq!(&o[..3], &[0, u8::try_from(n).unwrap(), 0]);
        assert_eq!(&o[4..6], &[1, 2]);
        assert_eq!(&o[6..8], &[8, 9]);
        assert_eq!(crate::crc::syndrome(o).get(), 0);
    }

    #[test]
    fn a_variable_frame_checks_its_length() {
        // req: RS-023, TC-007, USLP-010
        let l = layout(spec(SecurityLayout::NONE, false, false, 0), 6);
        let mut out = [0_u8; 16];
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        let e = w.unsecured(7).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::USER_LEN))
        );
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        let e = w.unsecured(0).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD))
        );
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        assert_eq!(
            w.unsecured(5).unwrap_err().kind(),
            ErrorKind::LengthMismatch
        );
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        assert_eq!(w.unsecured(6).unwrap().finalize().len(), 9);
        let mut small = [0_u8; 4];
        assert_eq!(
            FrameWriter::reserve_variable(&mut small, l, Var)
                .unwrap_err()
                .kind(),
            ErrorKind::BufferTooSmall
        );
    }

    #[test]
    fn a_variable_frame_needs_its_sdls() {
        // req: RS-023, API-070, API-073
        let sec = SecurityLayout::new(1, 1).unwrap();
        let l = layout(spec(sec, false, true, 0), 4);
        let mut out = [0_u8; 20];
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        assert_eq!(
            w.unsecured(2).unwrap_err().field(),
            Some(field::SECURITY_HEADER_LEN)
        );
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        assert_eq!(
            w.protect(2, &mut none(), &ctx()).unwrap_err().field(),
            Some(field::SECURITY_HEADER_LEN)
        );
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        assert_eq!(
            w.protect(9, &mut ok(), &ctx()).unwrap_err().field(),
            Some(field::USER_LEN)
        );
        // A failing hook leaves the unfinished header: version 3 again.
        let w = FrameWriter::reserve_variable(&mut out, l.clone(), Var).unwrap();
        assert_eq!(
            w.protect(2, &mut failing(), &ctx()).unwrap_err().kind(),
            ErrorKind::Security
        );
        assert_eq!(out[0], 3);
        assert_eq!(usize::from(out[1]), l.frame_len());
        assert_eq!(FECF_LEN, 2);
    }
}
