//! The AOS arm of the flight downlink (A4 WP-3a), as the application's [`DownProtocol`].
//!
//! Specification: docs/reference-application.md §4.3.1: AOS Version 2 frames of 223 octets with
//! the FHEC and the FECF and no Insert Zone (APP-169); VCs 0, 1, and 7 on one master channel,
//! each with the `VC_OCF`, fixed priority, and an idle `M_PDU` on VC 7 whenever VC 0 and VC 1
//! offer nothing, and no OID frame (APP-170); the VC counts (APP-171); the packet zones
//! (APP-172); the CLCW in every `VC_OCF` (APP-173).
//!
//! Requirements: APP-169…APP-173.
//!
//! **The idle VC.** VC 7's generator has the `IdleField` policy (LNK-014), so it always has a
//! field to offer: an `M_PDU` whose packet zone is the idle pattern and whose pointer says "only
//! idle data". The multiplexer serves the offered VC with the highest priority and VC 7 has the
//! lowest (APP-170), so its field is sent only when neither VC 0 nor VC 1 holds a released one
//! (LNK-031). An OID frame, which the mux would send if no VC offered anything, never occurs
//! (732.0 §4.1.4.2.3.4 note 1; OI-43).
//!
//! **The frame.** The mux leaves the FHEC and the FECF zero (LNK-035): the FHEC is written first,
//! over the header, and the FECF last, over the whole frame.

use aldis::aos::{
    AosEdition, AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent, fhec,
};
use aldis::core::crc::write_fecf;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::mux::{
    AosChannel, AosVc, ChannelMux, ChannelMuxConfig, MuxInput, Scope, VcConfig,
};
use aldis::link::policy::MuxPolicy;
use aldis::link::protocol::{Aos, AosParams, AosPointer};
use refapp_scenario::generated::{SPACECRAFT_ID, aos, tm};

use super::{DownProtocol, Mux, mux_priority, none};

/// Octets of the data field handed to the generator: the `M_PDU`, the header of 2 and the packet
/// zone of 207. The frame is the primary header (6), the FHEC (2), the `M_PDU` (209), the
/// `VC_OCF` (4), and the FECF (2) (APP-169).
const MPDU: usize = 209;

const _: () = assert!(6 + 2 + MPDU + 4 + 2 == super::FRAME);

/// The geometry above has the FHEC, the `VC_OCF`, and the FECF; the node builds only with the
/// scenario's `aos.fhec`, `aos.vc_ocf`, and `aos.fecf` true (APP-189, D4).
const _: () = assert!(aos::FHEC && aos::VC_OCF && aos::FECF);

/// The VCs that carry data are the VCs of TM, so that `vc_of` serves both (§4.1, §4.3.1).
const _: () = {
    assert!(aos::VCID_REALTIME == tm::VCID_REALTIME);
    assert!(aos::VCID_PAYLOAD == tm::VCID_PAYLOAD);
};

/// The VC of the AOS master channel with the number `id`, which has the `VC_OCF` (APP-170).
fn vc(scid: SpacecraftId<10>, id: u8, rank: u8) -> Result<VcConfig<Aos>> {
    let config =
        AosVirtualChannelConfig::new(scid, VcId::<6>::try_from(id)?, DataFieldContent::Mpdu)?;
    let params = AosVc::new(config.with_ocf()).with_initial_ocf(tm::OCF_INITIAL.to_be_bytes())?;
    Ok(VcConfig::<Aos>::new(params).with_priority(mux_priority(rank)))
}

impl DownProtocol for Aos {
    const FIELD_LEN: usize = MPDU;
    const DATA_VCS: [u8; 2] = [aos::VCID_REALTIME, aos::VCID_PAYLOAD];
    const DEADLINES_US: [u64; 2] = [
        aos::RELEASE_DEADLINE_REALTIME_US,
        aos::RELEASE_DEADLINE_PAYLOAD_US,
    ];
    const IDLE_VC: Option<u8> = Some(aos::VCID_IDLE);
    const MAX_PACKET_LEN: u16 = aos::MAX_PACKET_LENGTH;
    const IDLE_PATTERN: u8 = aos::IDLE_PATTERN;

    fn generator_params() -> Result<AosParams> {
        Ok(AosParams {
            edition: AosEdition::Issue5,
        })
    }

    /// The multiplexer: one master channel with the SCID of §3.4, frames of 223 octets with the
    /// FHEC and the FECF and no Insert Zone, VCs 0, 1, and 7 of `M_PDU`s with the `VC_OCF`, and
    /// fixed priority with VC 0 first and VC 7 last (APP-169, APP-170). The VC Frame Count Cycle
    /// is not in use (APP-171).
    fn mux() -> Result<Mux<Self>> {
        let scid =
            SpacecraftId::<10>::new(SPACECRAFT_ID).ok_or(Error::new(ErrorKind::InvalidConfig))?;
        let physical = AosPhysicalChannelConfig::new(
            usize::from(aos::FRAME_LENGTH),
            aos::FHEC,
            0,
            aos::FECF,
            AosEdition::Issue5,
        )?;
        let channel = AosChannel::new(physical, scid, false)?;
        let vcs = [
            vc(scid, aos::VCID_REALTIME, aos::PRIORITY_REALTIME)?,
            vc(scid, aos::VCID_PAYLOAD, aos::PRIORITY_PAYLOAD)?,
            vc(scid, aos::VCID_IDLE, aos::PRIORITY_IDLE)?,
        ];
        let config = ChannelMuxConfig::<Self, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
        ChannelMux::new(config)
    }

    /// An AOS frame has no pointer outside its field (the pointer is in the `M_PDU` header).
    fn offer_meta(_: AosPointer) {}

    /// The one OCF source feeds the `VC_OCF` of each of the three VCs, so every frame shows the
    /// CLCW of its slot whatever its VC (APP-173).
    fn set_ocf(mux: &mut Mux<Self>, ocf: [u8; 4]) -> Result<()> {
        for id in [aos::VCID_REALTIME, aos::VCID_PAYLOAD, aos::VCID_IDLE] {
            none(mux.step(
                MuxInput::SetOcf {
                    scope: Scope::Vc(VcId::<6>::try_from(id)?),
                    ocf,
                },
                &mut [],
            )?)?;
        }
        Ok(())
    }

    /// The FHEC over the header, then the FECF over the whole frame (LNK-035; 732.0 §4.1.2.6,
    /// §4.1.6.2).
    fn finish(frame: &mut [u8]) -> Result<()> {
        let (header, _) = frame
            .split_first_chunk_mut::<8>()
            .ok_or(Error::buffer_too_small(8))?;
        fhec::write(header);
        write_fecf(frame)
    }
}
