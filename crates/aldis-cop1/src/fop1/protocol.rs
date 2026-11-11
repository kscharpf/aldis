//! The frames FOP-1 builds, through the codec's frame format and frame-writer plan (COP-010,
//! COP-011, LNK-001, RS-023; OI-93).
//!
//! [`Fop1Format`] is what FOP-1 needs of a protocol beyond [`FrameFormat`]: the checked channel
//! its frames belong to, and the start of a frame in the codec's own [`FramePlan`](aldis_core::writer::FramePlan). FOP-1 itself
//! is generic over it, and fills the data field, fixes its length, and finishes the frame the same
//! way for every protocol; the lengths of a frame come from [`FrameFormat`], so FOP-1 restates no
//! fact of a Blue Book.

use core::fmt::Debug;
use core::hash::Hash;

use aldis_core::Result;
use aldis_core::format::{FrameFormat, PhysicalChannel};
use aldis_core::ids::FrameSeqNum;
use aldis_core::layout::FECF_LEN;
use aldis_core::writer::{FrameWriter, Variable, VariablePlan};
use aldis_tc::{
    FrameFields, FrameType, TcFormat, TcFrameWriter, TcPlan, TcSpacecraftId, TcVcId, VcTable,
};
#[cfg(feature = "uslp")]
use aldis_uslp::{
    FrameContent, FrameFields as UslpFields, SourceOrDestination, TfdfHeader, TfdzRule, Upid,
    UslpFormat, UslpFrameWriter, UslpMapId, UslpPlan, UslpSpacecraftId, UslpVcId,
    VcTable as UslpVcTable,
};

mod sealed {
    /// Keeps [`Fop1Format`](super::Fop1Format) closed to the formats of this crate.
    pub trait Sealed {}
}

/// What FOP-1 needs of a protocol to build its frames: the codec's [`FrameFormat`] and the start
/// of one frame in the codec's writer plan (COP-010, COP-011; OI-93).
///
/// Implemented for [`TcFormat`] and, with the feature `uslp`, for `UslpFormat`. The trait is
/// sealed: the set is closed, and no other type implements it. A
/// [`Fop1Target`](super::Fop1Target) of the protocol is built by its own constructor, which checks
/// the channel once (RS-020).
///
/// ```compile_fail,E0277
/// use aldis_cop1::Fop1Format;
///
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// struct Outside;
///
/// impl Fop1Format for Outside {
///     type Channel = ();
///     type Plan = aldis_tc::TcPlan;
///     fn begin<'b>(
///         _: &(),
///         _: aldis_tc::FrameType,
///         _: aldis_core::ids::FrameSeqNum,
///         _: bool,
///         _: usize,
///         _: &'b mut [u8],
///     ) -> aldis_core::Result<aldis_core::writer::FrameWriter<'b, aldis_core::writer::Variable, aldis_tc::TcPlan>> {
///         unreachable!()
///     }
/// }
/// ```
pub trait Fop1Format: FrameFormat + Hash + sealed::Sealed {
    /// The checked channel the frames belong to: its tables and identifiers.
    type Channel: Clone + Debug + Eq + Hash;

    /// The codec's plan of a variable-length frame (RS-023).
    type Plan: VariablePlan;

    /// Starts a frame of `frame_type` with N(S) = `sequence_number` on `channel`, with room for
    /// `data_len` octets of data field: the Frame Data Field of a TC frame, the TFDZ of a USLP
    /// frame. `control` says the data field is a Control Command (232.0 §4.1.3.3; 732.1 Table
    /// 4-1) and not an FDU. The caller fills `data_mut` and fixes the length with `unsecured`:
    /// FOP-1 frames have no SDLS fields.
    ///
    /// # Errors
    /// Those of the codec's `begin` (232.0 §4.1, 732.1 §4.1; TC-004, TC-007, USLP-010):
    /// `OutOfRange` (`frame_len`) above the channel's Maximum Transfer Frame Length;
    /// `BufferTooSmall` if `out` cannot hold the frame. On error `out` is unchanged.
    fn begin<'b>(
        channel: &Self::Channel,
        frame_type: FrameType,
        sequence_number: FrameSeqNum,
        control: bool,
        data_len: usize,
        out: &'b mut [u8],
    ) -> Result<FrameWriter<'b, Variable, Self::Plan>>;
}

/// The longest FDU a frame of `physical` carries when its Primary Header has the `extension` and
/// its data field starts with a header of `data_header_len` octets: what remains of the frame
/// length after the Primary Header, that header, and the FECF (232.0 §4.1.3; 732.1 §4.1.4.2;
/// COP-010).
pub(super) fn max_fdu_len<P: FrameFormat>(
    physical: &P::Physical,
    extension: &P::HeaderExtension,
    data_header_len: usize,
) -> usize {
    let trailer = if physical.fecf() { FECF_LEN } else { 0 };
    physical
        .frame_len()
        .saturating_sub(P::primary_header_len(extension))
        .saturating_sub(data_header_len)
        .saturating_sub(trailer)
}

/// The checked TC Virtual Channel of a FOP-1 target, and its table (232.0 §4.1.2.5, §4.1.2.6;
/// COP-010).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TcChannel {
    pub(super) table: VcTable<1, 0>,
    pub(super) spacecraft_id: TcSpacecraftId,
    pub(super) vc_id: TcVcId,
}

impl TcChannel {
    /// The Spacecraft Identifier.
    #[must_use]
    pub const fn spacecraft_id(&self) -> TcSpacecraftId {
        self.spacecraft_id
    }

    /// The Virtual Channel Identifier.
    #[must_use]
    pub const fn vc_id(&self) -> TcVcId {
        self.vc_id
    }
}

impl sealed::Sealed for TcFormat {}

impl Fop1Format for TcFormat {
    type Channel = TcChannel;
    type Plan = TcPlan;

    fn begin<'b>(
        channel: &TcChannel,
        frame_type: FrameType,
        sequence_number: FrameSeqNum,
        control: bool,
        data_len: usize,
        out: &'b mut [u8],
    ) -> Result<TcFrameWriter<'b>> {
        let fields = FrameFields {
            frame_type,
            spacecraft_id: channel.spacecraft_id,
            vc_id: channel.vc_id,
            sequence_number,
        };
        if control {
            TcPlan::begin_control(out, &channel.table, fields, data_len)
        } else {
            TcPlan::begin(out, &channel.table, fields, None, data_len)
        }
    }
}

/// The checked USLP Virtual Channel and MAP of a FOP-1 target, its table, and the TFDF Header of
/// its user data (732.1 §4.1.2.2–5, §4.1.4.2; COP-010, USLP-013).
#[cfg(feature = "uslp")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UslpChannel {
    pub(super) table: UslpVcTable<1, 1>,
    pub(super) spacecraft_id: UslpSpacecraftId,
    pub(super) vc_id: UslpVcId,
    pub(super) map_id: UslpMapId,
    pub(super) tfdf: TfdfHeader,
}

#[cfg(feature = "uslp")]
impl UslpChannel {
    /// The Spacecraft Identifier.
    #[must_use]
    pub const fn spacecraft_id(&self) -> UslpSpacecraftId {
        self.spacecraft_id
    }

    /// The Virtual Channel Identifier.
    #[must_use]
    pub const fn vc_id(&self) -> UslpVcId {
        self.vc_id
    }

    /// The MAP Identifier.
    #[must_use]
    pub const fn map_id(&self) -> UslpMapId {
        self.map_id
    }
}

#[cfg(feature = "uslp")]
impl sealed::Sealed for UslpFormat {}

#[cfg(feature = "uslp")]
impl Fop1Format for UslpFormat {
    type Channel = UslpChannel;
    type Plan = UslpPlan;

    fn begin<'b>(
        channel: &UslpChannel,
        frame_type: FrameType,
        sequence_number: FrameSeqNum,
        control: bool,
        data_len: usize,
        out: &'b mut [u8],
    ) -> Result<UslpFrameWriter<'b>> {
        // N(S) is the VCF Count of a sequence-controlled frame (732.1 §4.2.7.4 note 3); the
        // expedited counter is absent (the constructor checked it, OI-66). A ground sender names
        // the destination (§4.1.2.3.3).
        let fields = UslpFields {
            spacecraft_id: channel.spacecraft_id,
            source_or_destination: SourceOrDestination::Destination,
            vc_id: channel.vc_id,
            map_id: channel.map_id,
            kind: frame_type.into(),
            vcf_count: u64::from(sequence_number.get()),
        };
        // A Type-BC frame carries its command as the TFDZ with the UPID "COP-1 Control
        // Commands" (732.1 Table 4-1; USLP-013).
        let tfdf = if control {
            TfdfHeader {
                rule: TfdzRule::NoSegmentation,
                upid: Upid::Cop1ControlCommands,
            }
        } else {
            channel.tfdf
        };
        let content = FrameContent {
            tfdf,
            insert_zone: &[],
            ocf: None,
        };
        UslpPlan::begin(out, &channel.table, fields, content, data_len)
    }
}
