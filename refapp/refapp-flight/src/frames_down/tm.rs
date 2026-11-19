//! The TM arm of the flight downlink (A2 WP-3a), as the application's [`DownProtocol`].
//!
//! Specification: docs/reference-application.md §4.1: VCs 0 and 1 carry Packets, VC 7 carries the
//! OID frames, the OCF is the MC's, and the FECF is the only field finished after the mux.
//!
//! Requirements: APP-030…APP-035, APP-132, APP-134.

use aldis::core::crc::write_fecf;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::mux::{
    ChannelMux, ChannelMuxConfig, MuxInput, Scope, TmChannel, TmOffer, TmVc, VcConfig,
};
use aldis::link::policy::MuxPolicy;
use aldis::link::protocol::{Tm, TmParams};
use aldis::tm::{
    DataFieldContent, FirstHeaderPointer, TmPhysicalChannelConfig, TmVirtualChannelConfig,
};
use refapp_scenario::generated::{SPACECRAFT_ID, tm};

use super::{DownProtocol, Mux, mux_priority, none};

/// Octets of the data field: the frame less the primary header (6), the OCF (4), and the FECF
/// (2) (§4.1, CMN-040).
const DATA_FIELD: usize = 211;

const _: () = assert!(DATA_FIELD + 6 + 4 + 2 == super::FRAME);

impl DownProtocol for Tm {
    const FIELD_LEN: usize = DATA_FIELD;
    const DATA_VCS: [u8; 2] = [tm::VCID_REALTIME, tm::VCID_PAYLOAD];
    const DEADLINES_US: [u64; 2] = [
        tm::RELEASE_DEADLINE_REALTIME_US,
        tm::RELEASE_DEADLINE_PAYLOAD_US,
    ];
    const IDLE_VC: Option<u8> = None;
    const MAX_PACKET_LEN: u16 = tm::MAX_PACKET_LENGTH;
    const IDLE_PATTERN: u8 = tm::IDLE_PATTERN;

    fn generator_params() -> Result<TmParams> {
        Ok(TmParams)
    }

    /// The multiplexer: one master channel with the SCID of §3.4 and the OCF in every frame, VCs
    /// 0, 1, and 7 carrying Packets, fixed priority with VC 0 first, and VC 7 for OID frames
    /// (§4.1, §4.4.3).
    fn mux() -> Result<Mux<Self>> {
        let scid =
            SpacecraftId::<10>::new(SPACECRAFT_ID).ok_or(Error::new(ErrorKind::InvalidConfig))?;
        let physical = TmPhysicalChannelConfig::new(usize::from(tm::FRAME_LENGTH), tm::FECF)?;
        let mut channel =
            TmChannel::new(physical, scid, VcId::<3>::from_const::<{ tm::VCID_IDLE }>());
        channel.mc_ocf = Some(tm::OCF_INITIAL.to_be_bytes());
        let vc = |id: VcId<3>| {
            VcConfig::<Self>::new(TmVc::new(TmVirtualChannelConfig::new(
                id,
                DataFieldContent::Packets,
            )))
        };
        let vcs = [
            vc(VcId::<3>::from_const::<{ tm::VCID_REALTIME }>())
                .with_priority(mux_priority(tm::PRIORITY_REALTIME)),
            vc(VcId::<3>::from_const::<{ tm::VCID_PAYLOAD }>())
                .with_priority(mux_priority(tm::PRIORITY_PAYLOAD)),
            vc(VcId::<3>::from_const::<{ tm::VCID_IDLE }>()),
        ];
        let config = ChannelMuxConfig::<Self, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
        ChannelMux::new(config)
    }

    fn offer_meta(pointer: FirstHeaderPointer) -> TmOffer {
        TmOffer {
            pointer: Some(pointer),
        }
    }

    /// The one OCF source of the master channel (§4.4.1).
    fn set_ocf(mux: &mut Mux<Self>, ocf: [u8; 4]) -> Result<()> {
        none(mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf,
            },
            &mut [],
        )?)
    }

    /// The FECF, the only field the mux leaves to the caller on TM (LNK-035).
    fn finish(frame: &mut [u8]) -> Result<()> {
        write_fecf(frame)
    }
}
