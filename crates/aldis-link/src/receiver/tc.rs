//! The `FrameReceiver` procedure for TC frames (232.0 §4.4.5–§4.4.7; LNK-050, LNK-081).

use aldis_core::{Error, ErrorKind, FrameFormat, Result, field};
use aldis_tc::PhysicalChannelConfig;

use super::{DiscardReason, FrameContext, ReceiveProtocol, ReceiverOutput, header, vc_of};
use crate::protocol::Tc;

/// The octets of the TC Primary Header and the Frame Sequence Number (232.0 §4.1.2).
const HEADER_LEN: usize = Tc::PRIMARY_HEADER_LEN;

/// The fields of the TC Primary Header the receiver reads, by bit position (232.0 §4.1.2;
/// GEN-051).
mod layout {
    use aldis_core::bits::Field;

    use super::HEADER_LEN as N;

    pub(super) type Vcid = Field<N, 16, 6>;
}

/// The parameters of a TC physical channel that the receiver needs: none.
///
/// FARM-1 and the MAP demultiplexing decide acceptance (232.0 §4.4.5–6; LNK-050). The configured
/// frame length is the Maximum Transfer Frame Length: TC frames vary in length (232.0 §2.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcChannel {
    physical: PhysicalChannelConfig,
}

impl TcChannel {
    /// The channel of the physical channel `physical`, whose maximum frame length and FECF
    /// are the receiver's (232.0 Table 5-1; LNK-050).
    #[must_use]
    pub const fn new(physical: PhysicalChannelConfig) -> TcChannel {
        TcChannel { physical }
    }

    /// The managed parameters of the physical channel.
    #[must_use]
    pub const fn physical(&self) -> &PhysicalChannelConfig {
        &self.physical
    }
}

/// The parameters of a TC VC that the receiver needs: whether the engine serves it.
///
/// Requirements: LNK-050, LNK-081; 232.0 §4.4.5–§4.4.7; docs/04-engines.md §3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcVc {
    served: bool,
}

impl TcVc {
    /// A VC whose frames go to FARM-1.
    ///
    /// Requirements: LNK-050, LNK-081; 232.0 §4.4.5–§4.4.7; docs/04-engines.md §3.1.
    pub const SERVED: TcVc = TcVc { served: true };
    /// A VC the receiver does not serve: its frames are discarded as `NotServed`.
    ///
    /// Requirements: LNK-050, LNK-081; 232.0 §4.4.5–§4.4.7; docs/04-engines.md §3.1.
    pub const UNSERVED: TcVc = TcVc { served: false };

    /// Whether the receiver serves the VC.
    ///
    /// Requirements: LNK-050, LNK-081; 232.0 §4.4.5–§4.4.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn served(&self) -> bool {
        self.served
    }
}

impl ReceiveProtocol for Tc {
    type Channel = TcChannel;
    type VcParams = TcVc;
    type Plain<'a> = aldis_core::Plain<aldis_tc::TcFrame<'a>>;

    fn plain_octets<'a>(frame: &Self::Plain<'a>) -> &'a [u8] {
        frame.octets()
    }

    fn physical(channel: &TcChannel) -> &PhysicalChannelConfig {
        &channel.physical
    }

    fn fits(frame_len: usize, channel: &TcChannel, _: Option<&TcVc>) -> bool {
        let fecf = channel.physical.fecf();
        // At least one data octet (CMN-041, OI-41); the Maximum Transfer Frame Length (232.0
        // Tables 5-1 to 5-3) bounds the physical channel's frame length, which no frame exceeds.
        frame_len
            >= HEADER_LEN
                .saturating_add(if fecf { 2 } else { 0 })
                .saturating_add(1)
    }

    fn raw_insert_zone_offset(_: &TcChannel, _: &[u8]) -> Result<usize> {
        Err(Error::new(ErrorKind::InvalidConfig).with_field(field::RAW_FRAMES))
    }

    fn receive<const V: usize>(
        cx: &mut FrameContext<'_, Tc, V>,
        plain: &Self::Plain<'_>,
    ) -> Result<()> {
        let frame = Self::plain_octets(plain);
        // The Frame Length field gives the length of this frame, which varies up to the
        // channel's maximum (232.0 §2.3.1, §4.1.2.7); the codec has checked that it is the
        // length of the octets, a property of a `Plain<TcFrame>` (RS-023).
        let h = header::<HEADER_LEN>(frame)?;
        // The VCID is the six bits above the 10-bit Frame Length (232.0 §4.1.2.7).
        let vc = vc_of::<Tc>(layout::Vcid::get_u8(&h))?;
        match cx.params(vc) {
            None => cx.discard(DiscardReason::UnknownChannel, Some(vc), None),
            Some(p) if !p.served => cx.discard(DiscardReason::NotServed, Some(vc), None),
            Some(_) => {
                let frame = cx.put(frame)?;
                cx.push(ReceiverOutput::TcFrame { vc, frame })
            }
        }
    }
}
