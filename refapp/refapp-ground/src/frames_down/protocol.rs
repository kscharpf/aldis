//! The protocol types of the ground downlink: [`Tm`] (§4.1), [`Aos`] (§4.3.1), and [`Uslp`]
//! (§4.3.2) behind one refapp-local trait (D3 of the A4 plan).
//!
//! Specification: `docs/reference-application.md` §4.1, §4.3.1, §4.3.2; APP-030…APP-036,
//! APP-169…APP-180.
//! Each impl builds the managed parameters of its protocol from the generated scenario constants
//! (APP-136, APP-189) and takes a frame to the `Plain` frame the receiver accepts (LNK-050).

use core::fmt::Debug;

use aldis::aos::{
    AosEdition, AosFrame, AosPhysicalChannelConfig, AosVirtualChannelConfig,
    DataFieldContent as AosContentKind, VcTable,
};
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::{Check, DecodeMode, Error, ErrorKind, SecuritySlices};
use aldis::link::common::{Qos, ValidPvns};
use aldis::link::extractor::PacketExtractorConfig;
use aldis::link::protocol::{Aos, AosParams, PacketProtocol, Tm, TmParams, Uslp, UslpParams};
use aldis::link::receiver::{
    AosChannel, AosContent, AosVc, DeliveryService, FrameReceiverConfig, MapSet, ReceiveProtocol,
    TmChannel, TmVc, UslpChannel, UslpVc, VcEntry,
};
use aldis::tm::{
    DataFieldContent, TmFrame, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig,
};
use aldis::uslp;
use refapp_scenario::generated::{SPACECRAFT_ID, aos, tm, uslp_down};

/// Capacity of the receiver's VC table: VC 0, 1, and 7.
pub(super) const VC_CAPACITY: usize = 3;

/// The idle `M_PDU`s of VC 7 and the Idle Packets go to no application (APP-174).
const AOS_DELIVER_IDLE_PACKETS: bool = false;

/// The Encapsulation Idle Packets of the TFDZs go to no application (APP-180).
const USLP_DELIVER_IDLE_PACKETS: bool = false;

/// Capacity of the managed MAPs: MAP 0 of VC 0 and of VC 1 (APP-176).
const USLP_MAPS: usize = 2;

/// A protocol the ground downlink serves: the frame protocol of a downlink variant.
///
/// The two `HeaderPointer` types of the receiver and the extractor are one: the pointer a data
/// field hands from the first to the second (LNK-060).
pub(crate) trait DownlinkProtocol:
    PacketProtocol + ReceiveProtocol<HeaderPointer = <Self as PacketProtocol>::HeaderPointer>
{
    /// The managed parameters of the strict decode (APP-036, APP-174).
    type Managed: Clone + Debug;

    /// VC of housekeeping and events (APP-032, APP-170).
    const VCID_REALTIME: u8;
    /// VC of payload and dump (APP-032, APP-170).
    const VCID_PAYLOAD: u8;
    /// The third VC: OID frames in TM (APP-034), idle `M_PDU`s in AOS (APP-170), the VCID 63 of
    /// the OID frames in USLP (APP-175), which has no entry in the receiver's table.
    const VCID_IDLE: u8;
    /// Whether the third VC has an extractor of its own: AOS (APP-138), whose idle `M_PDU`s
    /// give no event. In TM and USLP it has none and a data frame on it is not served.
    const IDLE_VC_EXTRACTED: bool;
    /// The OCF word before the first frame (§4.4.1).
    const OCF_INITIAL: u32;

    /// The three VCIDs, in the order of the receiver's table (TM and AOS: USLP builds its table
    /// of the two data VCs, and VCID 63 is valid without an entry).
    #[must_use]
    fn vcids() -> [u8; VC_CAPACITY] {
        [Self::VCID_REALTIME, Self::VCID_PAYLOAD, Self::VCID_IDLE]
    }

    /// The managed parameters.
    ///
    /// # Errors
    /// `InvalidConfig` and the errors of the codec's configuration types if the constants do not
    /// form one.
    fn managed() -> Result<Self::Managed, Error>;

    /// The configuration of the receiver (APP-036, APP-174).
    ///
    /// # Errors
    /// As [`managed`](Self::managed).
    fn receiver_config() -> Result<FrameReceiverConfig<Self, VC_CAPACITY>, Error>;

    /// The configuration of the extractor of each VC that has one (APP-136, APP-172).
    ///
    /// # Errors
    /// As [`managed`](Self::managed).
    fn extractor_config() -> Result<PacketExtractorConfig<Self>, Error>;

    /// The strict decode of `unit` and the check against the managed parameters: the frame the
    /// receiver takes (TM-001…TM-011; AOS-010…AOS-017; LNK-050).
    ///
    /// # Errors
    /// The codec's refusal, `CrcMismatch` for the FECF, `HeaderUncorrectable` for the AOS FHEC,
    /// `UnknownChannel` for a channel that is not configured.
    fn decode<'a>(managed: &Self::Managed, unit: &'a [u8]) -> Result<Self::Plain<'a>, Error>;
}

const fn invalid() -> Error {
    Error::new(ErrorKind::InvalidConfig)
}

impl DownlinkProtocol for Tm {
    type Managed = TmMasterChannelConfig;

    const VCID_REALTIME: u8 = tm::VCID_REALTIME;
    const VCID_PAYLOAD: u8 = tm::VCID_PAYLOAD;
    const VCID_IDLE: u8 = tm::VCID_IDLE;
    const IDLE_VC_EXTRACTED: bool = false;
    const OCF_INITIAL: u32 = tm::OCF_INITIAL;

    fn managed() -> Result<TmMasterChannelConfig, Error> {
        let scid = SpacecraftId::new(SPACECRAFT_ID).ok_or_else(invalid)?;
        // The MC_OCF is present, so no VC has an OCF of its own (§4.1).
        let mut channel = TmMasterChannelConfig::new(
            TmPhysicalChannelConfig::new(usize::from(tm::FRAME_LENGTH), tm::FECF)?,
            scid,
            None,
            true,
        );
        for id in Self::vcids() {
            let id = VcId::<3>::new(id).ok_or_else(invalid)?;
            channel =
                channel.with_vc(TmVirtualChannelConfig::new(id, DataFieldContent::Packets))?;
        }
        Ok(channel)
    }

    fn receiver_config() -> Result<FrameReceiverConfig<Self, VC_CAPACITY>, Error> {
        // The receiver checks a frame's OCF Flag against its VC's configuration: the frames carry the
        // MC_OCF, so each received VC expects the flag set (`ocf = true` of the former constructor).
        let entry = |id: u8| -> Result<VcEntry<Self>, Error> {
            let id = VcId::<3>::new(id).ok_or_else(invalid)?;
            Ok(VcEntry::new(
                id,
                TmVc::new(
                    TmVirtualChannelConfig::new(id, DataFieldContent::Packets).with_ocf(),
                    true,
                ),
            ))
        };
        let [realtime, payload, idle] = Self::vcids();
        FrameReceiverConfig::new(
            TmChannel::new(
                TmPhysicalChannelConfig::new(usize::from(tm::FRAME_LENGTH), tm::FECF)?,
                DeliveryService::Mc,
                DeliveryService::None,
            ),
            &[entry(realtime)?, entry(payload)?, entry(idle)?],
        )
    }

    fn extractor_config() -> Result<PacketExtractorConfig<Self>, Error> {
        Ok(PacketExtractorConfig::new(
            TmParams,
            ValidPvns::from_versions(&[tm::PACKET_VERSION])?,
            usize::from(tm::MAX_PACKET_LENGTH),
        )?
        .with_delivery(tm::DELIVER_INCOMPLETE_PACKETS, tm::DELIVER_IDLE_PACKETS))
    }

    fn decode<'a>(
        managed: &TmMasterChannelConfig,
        unit: &'a [u8],
    ) -> Result<<Self as ReceiveProtocol>::Plain<'a>, Error> {
        TmFrame::decode_with(unit, managed.pc(), managed)?
            .check(managed)?
            .into_frame()
            .into_plain(NoSecurity)
    }
}

impl DownlinkProtocol for Aos {
    type Managed = VcTable<VC_CAPACITY>;

    const VCID_REALTIME: u8 = aos::VCID_REALTIME;
    const VCID_PAYLOAD: u8 = aos::VCID_PAYLOAD;
    const VCID_IDLE: u8 = aos::VCID_IDLE;
    const IDLE_VC_EXTRACTED: bool = true;
    // The OCF slot of the flight node starts as in TM: the CLCW of FARM-1 for the uplink VC,
    // which is the same TC uplink in `aos` (APP-173, LNK-033).
    const OCF_INITIAL: u32 = tm::OCF_INITIAL;

    fn managed() -> Result<VcTable<VC_CAPACITY>, Error> {
        let scid = SpacecraftId::<10>::new(SPACECRAFT_ID).ok_or_else(invalid)?;
        let vc = |id: u8| -> Result<AosVirtualChannelConfig, Error> {
            let id = VcId::<6>::new(id).ok_or_else(invalid)?;
            let vc = AosVirtualChannelConfig::new(scid, id, AosContentKind::Mpdu)?;
            Ok(if aos::VC_OCF { vc.with_ocf() } else { vc })
        };
        let [realtime, payload, idle] = Self::vcids();
        VcTable::new(physical()?, &[vc(realtime)?, vc(payload)?, vc(idle)?])
    }

    fn receiver_config() -> Result<FrameReceiverConfig<Self, VC_CAPACITY>, Error> {
        let entry = |id: u8| -> Result<VcEntry<Self>, Error> {
            Ok(VcEntry::new(
                VcId::<6>::new(id).ok_or_else(invalid)?,
                AosVc::new(AosContent::Packets, aos::VC_OCF),
            ))
        };
        let [realtime, payload, idle] = Self::vcids();
        FrameReceiverConfig::new(
            AosChannel::new(physical()?),
            &[entry(realtime)?, entry(payload)?, entry(idle)?],
        )
    }

    fn extractor_config() -> Result<PacketExtractorConfig<Self>, Error> {
        Ok(PacketExtractorConfig::new(
            AosParams {
                edition: AosEdition::Issue5,
            },
            ValidPvns::from_versions(&[aos::PACKET_VERSION])?,
            usize::from(aos::MAX_PACKET_LENGTH),
        )?
        .with_delivery(aos::DELIVER_INCOMPLETE_PACKETS, AOS_DELIVER_IDLE_PACKETS))
    }

    fn decode<'a>(
        managed: &VcTable<VC_CAPACITY>,
        unit: &'a [u8],
    ) -> Result<<Self as ReceiveProtocol>::Plain<'a>, Error> {
        AosFrame::decode(unit, &managed.physical_channel(), managed)?
            .check(managed.slots())?
            .into_frame()
            .into_plain(NoSecurity)
    }
}

impl DownlinkProtocol for Uslp {
    type Managed = uslp::VcTable<VC_CAPACITY, USLP_MAPS>;

    const VCID_REALTIME: u8 = uslp_down::VCID_REALTIME;
    const VCID_PAYLOAD: u8 = uslp_down::VCID_PAYLOAD;
    const VCID_IDLE: u8 = uslp::OID_VCID;
    const IDLE_VC_EXTRACTED: bool = false;
    // As in AOS: the CLCW of FARM-1 for the uplink VC, the same COP-1 uplink (APP-179).
    const OCF_INITIAL: u32 = tm::OCF_INITIAL;

    fn managed() -> Result<Self::Managed, Error> {
        let scid = uslp_scid()?;
        let count_len = |octets: u8| uslp::VcfCountLen::new(octets);
        let ocf = if uslp_down::OCF_REQUIRED {
            uslp::OcfUse::Required
        } else {
            uslp::OcfUse::Absent
        };
        let data = uslp::VcParams {
            vcf_count_len_sequence: count_len(uslp_down::VCF_COUNT_LENGTH_SEQUENCE)?,
            vcf_count_len_expedited: count_len(uslp_down::VCF_COUNT_LENGTH_EXPEDITED)?,
            ocf,
            ..uslp::VcParams::new(uslp::TransferFrameType::Fixed)
        };
        // An entry for VCID 63 gives the OID frames the OCF of the master channel and no VCF
        // Count (732.1 Table 5-2 note 2, §4.1.4.1.10 note 4).
        let oid = uslp::VcParams {
            ocf,
            ..uslp::VcParams::new(uslp::TransferFrameType::Fixed)
        };
        let vc = |id: u8, params: uslp::VcParams| uslp::VcConfig::new(scid, uslp_vc(id)?, params);
        let map = |id: u8| {
            Ok::<_, Error>(uslp::MapConfig::new(
                scid,
                uslp_vc(id)?,
                uslp::UslpMapId::new(uslp_down::MAP_ID).ok_or_else(invalid)?,
                SecurityLayout::NONE,
            ))
        };
        let [realtime, payload, oid_vc] = Self::vcids();
        uslp::VcTable::new(
            physical_uslp()?,
            &[vc(realtime, data)?, vc(payload, data)?, vc(oid_vc, oid)?],
            &[map(realtime)?, map(payload)?],
        )
    }

    fn receiver_config() -> Result<FrameReceiverConfig<Self, VC_CAPACITY>, Error> {
        let maps = MapSet::new(&[MapId::<4>::new(uslp_down::MAP_ID).ok_or_else(invalid)?]);
        // The COP is not in effect on the downlink: the receiver counts the frames of the VC
        // (LNK-051).
        let entry = |id: u8| -> Result<VcEntry<Self>, Error> {
            Ok(VcEntry::new(
                VcId::<6>::new(id).ok_or_else(invalid)?,
                UslpVc::new(maps, false, true),
            ))
        };
        let [realtime, payload, _] = Self::vcids();
        FrameReceiverConfig::new(
            UslpChannel::new(physical_uslp()?, DeliveryService::Mc)?,
            &[entry(realtime)?, entry(payload)?],
        )
    }

    fn extractor_config() -> Result<PacketExtractorConfig<Self>, Error> {
        Ok(PacketExtractorConfig::new(
            UslpParams::new(
                uslp::Upid::Packets,
                MapId::<4>::new(uslp_down::MAP_ID).ok_or_else(invalid)?,
                Qos::Expedited,
            )?,
            ValidPvns::from_versions(&[
                uslp_down::PACKET_VERSION,
                uslp_down::ENCAPSULATION_PACKET_VERSION,
            ])?,
            usize::from(uslp_down::MAX_PACKET_LENGTH),
        )?
        .with_delivery(
            uslp_down::DELIVER_INCOMPLETE_PACKETS,
            USLP_DELIVER_IDLE_PACKETS,
        ))
    }

    fn decode<'a>(
        managed: &Self::Managed,
        unit: &'a [u8],
    ) -> Result<<Self as ReceiveProtocol>::Plain<'a>, Error> {
        // The strict decode with the FECF and the Frame Validation Checks (732.1 §4.3.11.3).
        uslp::check_frame(unit, managed, DecodeMode::Strict)?
            .into_view()
            .into_frame()
            .into_plain(NoSecurity)
    }
}

fn uslp_scid() -> Result<uslp::UslpSpacecraftId, Error> {
    uslp::UslpSpacecraftId::new(SPACECRAFT_ID).ok_or_else(invalid)
}

fn uslp_vc(id: u8) -> Result<uslp::UslpVcId, Error> {
    uslp::UslpVcId::new(id).ok_or_else(invalid)
}

/// The physical channel of §4.3.2: fixed length, 223 octets, no Insert Zone, FECF (APP-175).
fn physical_uslp() -> Result<uslp::PhysicalChannelConfig, Error> {
    uslp::PhysicalChannelConfig::new(
        uslp::TransferFrameType::Fixed,
        usize::from(uslp_down::FRAME_LENGTH),
        0,
        uslp_down::FECF,
    )
}

/// The physical channel of §4.3.1: 223 octets, FHEC and FECF, no Insert Zone, Issue 5
/// (APP-169).
fn physical() -> Result<AosPhysicalChannelConfig, Error> {
    AosPhysicalChannelConfig::new(
        usize::from(aos::FRAME_LENGTH),
        aos::FHEC,
        0,
        aos::FECF,
        AosEdition::Issue5,
    )
}
