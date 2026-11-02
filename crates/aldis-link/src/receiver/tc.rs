//! The `FrameReceiver` procedure for TC frames (232.0 §4.4.5–§4.4.7; LNK-050, LNK-081).

use aldis_core::{Error, ErrorKind, Result, field};

use super::{DiscardReason, FrameContext, ReceiveProtocol, ReceiverOutput, header, vc_of};
use crate::protocol::Tc;

/// The octets of the TC Primary Header and the Frame Sequence Number (232.0 §4.1.2).
const HEADER_LEN: usize = 5;

/// The parameters of a TC physical channel that the receiver needs: none.
///
/// FARM-1 and the MAP demultiplexing decide acceptance (232.0 §4.4.5–6; LNK-050). The configured
/// frame length is the Maximum Transfer Frame Length: TC frames vary in length (232.0 §2.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TcChannel;

/// The parameters of a TC VC that the receiver needs: whether the engine serves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcVc {
    served: bool,
}

impl TcVc {
    /// A VC whose frames go to FARM-1.
    pub const SERVED: TcVc = TcVc { served: true };
    /// A VC the receiver does not serve: its frames are discarded as `NotServed`.
    pub const UNSERVED: TcVc = TcVc { served: false };

    /// Whether the receiver serves the VC.
    #[must_use]
    pub const fn served(&self) -> bool {
        self.served
    }
}

impl ReceiveProtocol for Tc {
    type Channel = TcChannel;
    type VcParams = TcVc;

    fn insert_zone_len(_: &TcChannel) -> usize {
        0
    }

    fn fits(frame_len: usize, fecf: bool, _: &TcChannel, _: Option<&TcVc>) -> bool {
        frame_len >= HEADER_LEN.saturating_add(if fecf { 2 } else { 0 })
    }

    fn variable_length(_: &TcChannel) -> bool {
        true
    }

    fn raw_insert_zone_offset(_: &TcChannel, _: &[u8]) -> Result<usize> {
        Err(Error::new(ErrorKind::InvalidConfig).with_field(field::RAW_FRAMES))
    }

    fn receive<const V: usize>(cx: &mut FrameContext<'_, Tc, V>, frame: &[u8]) -> Result<()> {
        let [_, _, b2, b3, _] = header::<HEADER_LEN>(frame)?;
        // The Frame Length field gives the length of this frame, which varies up to the
        // channel's maximum (232.0 §2.3.1, §4.1.2.7).
        let declared = usize::from(u16::from_be_bytes([b2 & 0x03, b3])).saturating_add(1);
        if declared != frame.len() {
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN));
        }
        // The VCID is the six bits above the 10-bit Frame Length (232.0 §4.1.2.7).
        let vc = vc_of::<Tc>(b2 >> 2)?;
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
