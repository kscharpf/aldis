//! The `Plain` frames of the `FrameReceiver` tests (RS-023, LNK-050).
//!
//! A frame reaches the receiver only as the codec's `Plain` frame, so a test hands its octets to
//! the codec first. The codec here knows no channel, as a caller whose configuration holds none
//! of the frame's VCs: every VC and MAP is unconfigured, the frame length is the octets given,
//! and the FECF is the receiver's (the tests that build frames without one configure none; the
//! AOS FHEC is not checked either). What the receiver refuses is then the receiver's own refusal; what the codec refuses is
//! returned as the codec's error.

use aldis_aos::{AosEdition, AosFrame, AosPhysicalChannelConfig, AosVirtualChannelConfig};
use aldis_core::Result;
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::security::NoSecurity;
use aldis_link::protocol::{Aos, Tc, Tm, Uslp};
use aldis_link::receiver::{FrameReceiverConfig, ReceiveProtocol};
use aldis_tc::{MapConfig, PhysicalChannelConfig as TcPhysical, TcFrame, TcSpacecraftId, TcVcId};
use aldis_tm::{TmFrame, TmPhysicalChannelConfig};
use aldis_uslp::{
    PhysicalChannelConfig as UslpPhysical, TransferFrameType, UslpFrame, UslpMapId,
    UslpSpacecraftId, UslpVcId,
};

/// A protocol whose `Plain` frame a test builds from octets.
pub(crate) trait Plained: ReceiveProtocol {
    /// Decodes `octets` as the one frame of a physical channel of their length, then takes the
    /// `Plain` step through `NoSecurity`.
    ///
    /// # Errors
    /// The codec's error for a frame it refuses.
    fn plain<'a, const V: usize>(
        config: &FrameReceiverConfig<Self, V>,
        octets: &'a [u8],
    ) -> Result<Self::Plain<'a>> {
        Self::plain_len(config, octets, octets.len())
    }

    /// As [`plain`](Self::plain), for a physical channel whose frame length is `frame_len`: a
    /// frame of another length is the codec's `LengthMismatch` (`frame_len`), before its FECF.
    ///
    /// # Errors
    /// The codec's error for a frame it refuses.
    fn plain_len<'a, const V: usize>(
        config: &FrameReceiverConfig<Self, V>,
        octets: &'a [u8],
        frame_len: usize,
    ) -> Result<Self::Plain<'a>>;
}

impl Plained for Tm {
    fn plain_len<'a, const V: usize>(
        config: &FrameReceiverConfig<Tm, V>,
        octets: &'a [u8],
        frame_len: usize,
    ) -> Result<Self::Plain<'a>> {
        let pc = TmPhysicalChannelConfig::new(frame_len, config.fecf())?;
        TmFrame::decode(octets, &pc)?.into_plain(NoSecurity)
    }
}

/// A lookup that holds no AOS virtual channel.
struct NoAosVcs;

impl aldis_aos::VcLookup for NoAosVcs {
    fn has_spacecraft(&self, _: SpacecraftId<10>) -> bool {
        false
    }

    fn vc(&self, _: SpacecraftId<10>, _: VcId<6>) -> Option<AosVirtualChannelConfig> {
        None
    }
}

impl Plained for Aos {
    fn plain_len<'a, const V: usize>(
        config: &FrameReceiverConfig<Aos, V>,
        octets: &'a [u8],
        frame_len: usize,
    ) -> Result<Self::Plain<'a>> {
        let channel = config.channel();
        let pc = AosPhysicalChannelConfig::new(
            frame_len,
            channel.fhec(),
            channel.insert_zone_len(),
            config.fecf(),
            AosEdition::Issue5,
        )?;
        AosFrame::decode(octets, &pc, &NoAosVcs)?.into_plain(NoSecurity)
    }
}

/// A lookup that holds no TC channel, on a physical channel of `max_frame_len` octets.
struct NoTcChannels(TcPhysical);

impl aldis_tc::VcLookup for NoTcChannels {
    fn physical_channel(&self) -> TcPhysical {
        self.0
    }

    fn has_spacecraft(&self, _: TcSpacecraftId) -> bool {
        false
    }

    fn vc(&self, _: TcSpacecraftId, _: TcVcId) -> Option<aldis_tc::VcConfig> {
        None
    }

    fn map(&self, _: TcSpacecraftId, _: TcVcId, _: aldis_tc::TcMapId) -> Option<MapConfig> {
        None
    }
}

impl Plained for Tc {
    fn plain_len<'a, const V: usize>(
        config: &FrameReceiverConfig<Tc, V>,
        octets: &'a [u8],
        frame_len: usize,
    ) -> Result<Self::Plain<'a>> {
        let channels = NoTcChannels(TcPhysical::new(frame_len, config.fecf())?);
        TcFrame::decode(octets, &channels)?.into_plain(NoSecurity)
    }
}

/// A lookup that holds no USLP channel.
struct NoUslpChannels(UslpPhysical);

impl aldis_uslp::VcLookup for NoUslpChannels {
    fn physical_channel(&self) -> UslpPhysical {
        self.0
    }

    fn has_spacecraft(&self, _: UslpSpacecraftId) -> bool {
        false
    }

    fn vc(&self, _: UslpSpacecraftId, _: UslpVcId) -> Option<aldis_uslp::VcConfig> {
        None
    }

    fn map(&self, _: UslpSpacecraftId, _: UslpVcId, _: UslpMapId) -> Option<aldis_uslp::MapConfig> {
        None
    }
}

impl Plained for Uslp {
    fn plain_len<'a, const V: usize>(
        config: &FrameReceiverConfig<Uslp, V>,
        octets: &'a [u8],
        frame_len: usize,
    ) -> Result<Self::Plain<'a>> {
        let channel = config.channel();
        let (frame_type, iz) = if channel.is_variable() {
            (TransferFrameType::Variable, 0)
        } else {
            (TransferFrameType::Fixed, channel.insert_zone_len())
        };
        let pc = UslpPhysical::new(frame_type, frame_len, iz, config.fecf())?;
        UslpFrame::decode(octets, &NoUslpChannels(pc))?.into_plain(NoSecurity)
    }
}
