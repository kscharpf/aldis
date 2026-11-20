//! The protocol types of the ground uplink: [`Tc`] (§4.2) and [`Uslp`] (§4.3.3) behind one
//! refapp-local trait (D3 of the A4 plan).
//!
//! Specification: `docs/reference-application.md` §4.2, §4.3.3; APP-040…APP-044, APP-136,
//! APP-181…APP-184, APP-189.
//!
//! Each impl builds the FOP-1 target and the `MapSegmenter` configuration of its protocol from
//! the generated scenario constants. The USLP target is the single-MAP target of D6: FOP-1's
//! `Fop1Target::uslp` takes one MAP and the rule `111` only, so that the commands and the table
//! upload share MAP 0 and nothing is segmented. The reasons are the open library questions of
//! #235 and OI-71, and the `api-feedback` issue #719 for the single-MAP target (with #661).

use aldis::cop1::{Fop1Format, Fop1Target};
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::security::SecurityLayout;
use aldis::core::{Error, ErrorKind};
use aldis::link::protocol::{Tc, Uslp};
use aldis::link::segmenter::{
    MapEntry, MapSegmenterConfig, SegmenterProtocol, SegmenterService, TcSegmenterParams,
    UslpSegmenterParams,
};
use aldis::tc::PhysicalChannelConfig;
use aldis::uslp;
use refapp_scenario::Apid;
use refapp_scenario::generated::{SPACECRAFT_ID, apid, tc, uslp_up};

const fn invalid() -> Error {
    Error::new(ErrorKind::InvalidConfig)
}

/// A protocol the ground uplink serves: the frame protocol of an uplink variant.
pub(crate) trait UplinkProtocol: SegmenterProtocol + Fop1Format {
    /// The MAPs of the VC, the `M` of the `MapSegmenter` (APP-041, APP-181).
    const MAPS: usize;

    /// The MAP of the packets of `apid` (APP-041, §4.2, §4.3.3).
    #[must_use]
    fn map_id(apid: Apid) -> Option<u8>;

    /// The FOP-1 target: the VC, its frame length, and the format of its frames (COP-010).
    ///
    /// # Errors
    /// `InvalidConfig`, and the errors of the codec's configuration types, if the constants do
    /// not form one.
    fn target() -> Result<Fop1Target<Self>, Error>;

    /// The octets in front of the user data of the segmenter's FDU that FOP-1 writes itself:
    /// the TFDF Header of USLP (the Segment Header of TC is part of the FDU, OI-71).
    const HEADER_LEN: usize;

    /// The FDU of the segmenter as the data FOP-1 puts in the data field of a frame. TC: the
    /// FDU, Segment Header included. USLP: the TFDZ, after the TFDF Header that the target
    /// writes again (the workaround of D6: #235 and OI-71 are the open questions of how the
    /// segmenter hands a TFDF header to FOP-1). `None` if the FDU is not what the segmenter
    /// writes for this target, which its configuration rules out.
    #[must_use]
    fn request_data(fdu: &[u8]) -> Option<&[u8]>;

    /// The configuration of the segmenter, whose FDUs are the longest of `target` (APP-136,
    /// APP-182).
    ///
    /// # Errors
    /// As [`target`](Self::target); `InvalidConfig` if `M` is not [`MAPS`](Self::MAPS) or the
    /// target's lengths disagree with the scenario's.
    fn segmenter_config<const M: usize>(
        target: &Fop1Target<Self>,
    ) -> Result<MapSegmenterConfig<Self, M>, Error>;
}

/// A MAP of the TC VC as the scenario configures it (232.0 Table 5-4; APP-136).
fn tc_map_entry(id: u8, blocking: bool, segmentation: bool) -> Result<MapEntry<Tc>, Error> {
    let id = MapId::<6>::new(id).ok_or_else(invalid)?;
    let entry = MapEntry::<Tc>::new(id, SegmenterService::Any, ());
    let entry = if blocking {
        entry.with_blocking()
    } else {
        entry
    };
    if segmentation {
        entry.with_segmentation(usize::from(tc::MAX_PACKET_LENGTH))
    } else {
        Ok(entry)
    }
}

fn tc_physical() -> Result<PhysicalChannelConfig, Error> {
    PhysicalChannelConfig::new(usize::from(tc::MAX_FRAME_LENGTH), tc::FECF)
}

fn tc_vc() -> Result<VcId<6>, Error> {
    VcId::<6>::new(tc::VCID).ok_or_else(invalid)
}

impl UplinkProtocol for Tc {
    const MAPS: usize = 2;
    const HEADER_LEN: usize = 0;

    fn request_data(fdu: &[u8]) -> Option<&[u8]> {
        Some(fdu)
    }

    fn map_id(apid: Apid) -> Option<u8> {
        match apid.get() {
            apid::COMMANDS => Some(tc::MAP_COMMANDS),
            apid::TABLE_UPLOAD => Some(tc::MAP_TABLE_UPLOAD),
            _ => None,
        }
    }

    fn target() -> Result<Fop1Target<Self>, Error> {
        let spacecraft = SpacecraftId::<10>::new(SPACECRAFT_ID).ok_or_else(invalid)?;
        Fop1Target::tc(spacecraft, tc_vc()?, tc_physical()?)
    }

    fn segmenter_config<const M: usize>(
        target: &Fop1Target<Self>,
    ) -> Result<MapSegmenterConfig<Self, M>, Error> {
        // The frame data field is the FDU, Segment Header included (OI-71): the target's
        // longest FDU is the scenario's.
        let max_fdu = usize::from(tc::MAX_FDU_LENGTH);
        if M != Self::MAPS || target.max_fdu_len() != max_fdu || !tc::SEGMENT_HEADER {
            return Err(invalid());
        }
        MapSegmenterConfig::new(
            tc_vc()?,
            TcSegmenterParams::with_segment_header(tc_physical()?),
            max_fdu,
            &[
                tc_map_entry(
                    tc::MAP_COMMANDS,
                    tc::MAP_COMMANDS_BLOCKING,
                    tc::MAP_COMMANDS_SEGMENTATION,
                )?,
                tc_map_entry(
                    tc::MAP_TABLE_UPLOAD,
                    tc::MAP_TABLE_UPLOAD_BLOCKING,
                    tc::MAP_TABLE_UPLOAD_SEGMENTATION,
                )?,
            ],
        )
    }
}

/// The TFDF Header of the user data of a Type-AD frame: the rule `111` and the UPID "Space
/// Packets or Encapsulation packets" (APP-183). FOP-1 builds the one of a Type-BC frame.
const USLP_TFDF: uslp::TfdfHeader = uslp::TfdfHeader {
    rule: uslp::TfdzRule::NoSegmentation,
    upid: uslp::Upid::Packets,
};

fn uslp_scid() -> Result<uslp::UslpSpacecraftId, Error> {
    uslp::UslpSpacecraftId::new(SPACECRAFT_ID).ok_or_else(invalid)
}

fn uslp_vc_id() -> Result<uslp::UslpVcId, Error> {
    uslp::UslpVcId::new(uslp_up::VCID).ok_or_else(invalid)
}

/// The VC of §4.3.3: variable length, a one-octet VCF Count for the sequence-controlled
/// service, none for the expedited one, no truncated frames, no SDLS (APP-181).
fn uslp_vc_params() -> Result<uslp::VcParams, Error> {
    Ok(uslp::VcParams {
        vcf_count_len_sequence: uslp::VcfCountLen::new(uslp_up::VCF_COUNT_LENGTH_SEQUENCE)?,
        vcf_count_len_expedited: uslp::VcfCountLen::new(uslp_up::VCF_COUNT_LENGTH_EXPEDITED)?,
        ocf: if uslp_up::OCF_ALLOWED {
            uslp::OcfUse::Allowed
        } else {
            uslp::OcfUse::Absent
        },
        ..uslp::VcParams::new(uslp::TransferFrameType::Variable)
    })
}

impl UplinkProtocol for Uslp {
    const MAPS: usize = 1;
    const HEADER_LEN: usize = 1;

    fn request_data(fdu: &[u8]) -> Option<&[u8]> {
        // The segmenter writes the TFDF Header of a whole SDU: the rule `111` and the UPID of
        // the MAP, which is the header of the target (APP-183, APP-184).
        let [header, ..] = USLP_TFDF.octets();
        match fdu.split_first() {
            Some((first, data)) if *first == header => Some(data),
            Some(_) | None => None,
        }
    }

    /// The commands and the table upload share MAP 0 (D6, APP-181).
    fn map_id(apid: Apid) -> Option<u8> {
        match apid.get() {
            apid::COMMANDS | apid::TABLE_UPLOAD => Some(uslp_up::MAP_ID),
            _ => None,
        }
    }

    fn target() -> Result<Fop1Target<Self>, Error> {
        let scid = uslp_scid()?;
        let physical = uslp::PhysicalChannelConfig::new(
            uslp::TransferFrameType::Variable,
            usize::from(uslp_up::MAX_FRAME_LENGTH),
            0,
            uslp_up::FECF,
        )?;
        let vc = uslp::VcConfig::new(scid, uslp_vc_id()?, uslp_vc_params()?)?;
        let map = uslp::MapConfig::new(
            scid,
            uslp_vc_id()?,
            uslp::UslpMapId::new(uslp_up::MAP_ID).ok_or_else(invalid)?,
            SecurityLayout::NONE,
        );
        Fop1Target::uslp(physical, vc, map, USLP_TFDF)
    }

    fn segmenter_config<const M: usize>(
        target: &Fop1Target<Self>,
    ) -> Result<MapSegmenterConfig<Self, M>, Error> {
        // The target's longest FDU is the TFDZ of a Type-AD frame: the longest packet of the MAP
        // (APP-182). The segmenter's FDU is the TFDF, with the TFDF Header (one octet) in front.
        if M != Self::MAPS || target.max_fdu_len() != usize::from(uslp_up::MAX_PACKET_LENGTH) {
            return Err(invalid());
        }
        let max_fdu = target.max_fdu_len().saturating_add(Self::HEADER_LEN);
        let map = uslp::UslpMapId::new(uslp_up::MAP_ID).ok_or_else(invalid)?;
        let id = MapId::<4>::new(map.get()).ok_or_else(invalid)?;
        let entry = MapEntry::<Self>::new(id, SegmenterService::Packets, uslp::Upid::Packets);
        // MAP 0 permits blocking and prohibits segmentation (APP-181, APP-184).
        let entry = if uslp_up::MAP_BLOCKING {
            entry.with_blocking()
        } else {
            entry
        };
        let entry = if uslp_up::MAP_SEGMENTATION {
            entry.with_segmentation(usize::from(uslp_up::MAX_PACKET_LENGTH))?
        } else {
            entry
        };
        MapSegmenterConfig::new(
            VcId::<6>::new(uslp_up::VCID).ok_or_else(invalid)?,
            UslpSegmenterParams::new(uslp_vc_params()?)?,
            max_fdu,
            &[entry],
        )
    }
}
