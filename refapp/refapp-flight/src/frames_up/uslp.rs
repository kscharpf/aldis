//! The USLP arm of the flight uplink (A4 WP-3c), as the application's [`UpProtocol`].
//!
//! Specification: docs/reference-application.md §4.3.3: variable-length USLP frames of at most 512
//! octets with the FECF and no Insert Zone or OCF, on VC 0 with one MAP, MAP 0, which permits
//! blocking and prohibits segmentation, COP-1 in effect, a one-octet sequence-controlled VCF
//! Count, no expedited count, the rule `111` (APP-181, APP-182, APP-183).
//!
//! Requirements: APP-181, APP-183, APP-185, APP-186; USLP-013, USLP-014, COP-031.
//!
//! **FARM-1 over USLP (D7).** `Farm1Input::from_uslp` converts the checked USLP frame directly
//! into the FARM-1 frame input (issue #718): a Sequence-Controlled frame becomes Type-AD with its
//! one-octet VCF Count as N(S) (OI-101 (a)), an Expedited frame becomes Type-BD, and a Protocol
//! Control frame becomes Type-BC carrying a single COP-1 Control Command (OI-100 (a), APP-183,
//! APP-186); any other control frame is refused with `ReservedValue` and fed to FARM-1 as
//! `InvalidFrame` (E9). The single MAP of the uplink (the FOP-1 target takes one, D6) is issue
//! #719, with #235, OI-71 and #661 behind it.
//!
//! **The data FARM-1 delivers** is the whole TFDF, the TFDF Header and the TFDZ, which is what
//! the `MapReassembler` reads on USLP (LNK-070); with the rule `111` it parses the TFDZ as one
//! unit of whole packets, blocked (LNK-071).

use aldis::cop1::farm1::Farm1Input;
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::{DecodeMode, Error, ErrorKind, Result, SecuritySlices};
use aldis::link::protocol::{Protocol, Uslp};
use aldis::link::reassembler::{MapReassemblerConfig, ReassemblerMap, UslpFdu};
use aldis::link::receiver::{
    DeliveryService, FrameReceiver, FrameReceiverConfig, MapSet, UslpChannel, UslpVc, VcEntry,
};
use aldis::uslp::{
    MapConfig, OcfUse, PhysicalChannelConfig, Reason, TransferFrameType, UslpMapId,
    UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen, check_frame,
};
use refapp_scenario::generated::{SPACECRAFT_ID, uslp_up as up};

use super::{Admitted, Refusal, UpProtocol};

/// The geometry below is that of APP-181 to APP-183: the FECF, no OCF, a one-octet
/// sequence-controlled count and no expedited count, VC 0 and MAP 0 with blocking and without
/// segmentation. The node builds only with the scenario's values, so that a change of the
/// scenario fails here and not in a trace.
const _: () = {
    assert!(up::FECF && !up::OCF_ALLOWED);
    assert!(up::VCF_COUNT_LENGTH_SEQUENCE == 1 && up::VCF_COUNT_LENGTH_EXPEDITED == 0);
    assert!(up::VCID == 0 && up::MAP_ID == 0);
    assert!(up::MAP_BLOCKING && !up::MAP_SEGMENTATION);
};

/// The USLP identifiers of the scenario, which the types refuse out of range.
struct Ids {
    scid: UslpSpacecraftId,
    vc: UslpVcId,
    map: UslpMapId,
}

fn ids() -> Result<Ids> {
    let bad = Error::new(ErrorKind::InvalidConfig);
    Ok(Ids {
        scid: UslpSpacecraftId::new(SPACECRAFT_ID).ok_or(bad)?,
        vc: UslpVcId::new(up::VCID).ok_or(bad)?,
        map: UslpMapId::new(up::MAP_ID).ok_or(bad)?,
    })
}

/// The physical channel of the uplink: variable-length frames of at most the Maximum Transfer
/// Frame Length, with no Insert Zone and the FECF (APP-181, APP-182).
fn physical() -> Result<PhysicalChannelConfig> {
    PhysicalChannelConfig::new(
        TransferFrameType::Variable,
        usize::from(up::MAX_FRAME_LENGTH),
        0,
        up::FECF,
    )
}

impl UpProtocol for Uslp {
    const VCID: u8 = up::VCID;
    const GAP_DISCARD: Option<u16> = Some(super::APP_GAP_DISCARD);
    type Table = VcTable<1, 1>;
    /// The MAP and the gap flag of the frame, which the reassembler takes with the TFDF.
    type Info = UslpFdu;

    fn table() -> Result<Self::Table> {
        let ids = ids()?;
        let params = VcParams {
            vcf_count_len_sequence: VcfCountLen::new(up::VCF_COUNT_LENGTH_SEQUENCE)?,
            vcf_count_len_expedited: VcfCountLen::new(up::VCF_COUNT_LENGTH_EXPEDITED)?,
            ocf: OcfUse::Absent,
            ..VcParams::new(TransferFrameType::Variable)
        };
        VcTable::new(
            physical()?,
            &[VcConfig::new(ids.scid, ids.vc, params)?],
            &[MapConfig::new(
                ids.scid,
                ids.vc,
                ids.map,
                SecurityLayout::NONE,
            )],
        )
    }

    fn receiver() -> Result<FrameReceiver<Self, 1>> {
        let ids = ids()?;
        let vc = <Self as Protocol>::Vc::try_from(up::VCID)?;
        // One MAP: the FOP-1 target of the ground node takes one (issue #719, D6).
        // COP-1 is in effect, so FARM-1 decides the sequence and the receiver counts nothing
        // (732.1 §4.3.7.2, LNK-051). No OCF is delivered: the uplink has none (APP-181).
        let params = UslpVc::new(MapSet::new(&[ids.map]), true, true);
        Ok(FrameReceiver::new(FrameReceiverConfig::new(
            UslpChannel::new(physical()?, DeliveryService::None)?,
            &[VcEntry::new(vc, params)],
        )?))
    }

    fn reassembly() -> Result<MapReassemblerConfig<Self, 2>> {
        let ids = ids()?;
        MapReassemblerConfig::new(
            (),
            &[ReassemblerMap::packets(
                <Self as Protocol>::Vc::try_from(up::VCID)?,
                ids.map,
                usize::from(up::MAX_PACKET_LENGTH),
            )?],
        )
    }

    fn admit<'a>(
        table: &Self::Table,
        octets: &'a [u8],
        buffer_available: bool,
    ) -> core::result::Result<Admitted<'a, Self>, Refusal> {
        let refuse = |kind| Refusal { fecf: false, kind };
        let validated = check_frame(octets, table, DecodeMode::Strict)
            .map_err(|rejection| Refusal {
                fecf: rejection.reason() == Reason::FecfMismatch,
                kind: rejection.error().kind(),
            })?
            .into_view();
        let farm =
            Farm1Input::from_uslp(&validated, buffer_available).map_err(|e| refuse(e.kind()))?;
        let full = validated
            .as_frame()
            .full()
            .ok_or_else(|| refuse(ErrorKind::InvalidField))?;
        let info = UslpFdu {
            map: full.header().map_id(),
            // FARM-1 lets no gap through, so no frame follows a gap (APP-186).
            gap: false,
        };
        let plain = validated
            .into_frame()
            .into_plain(NoSecurity)
            .map_err(|e| refuse(e.kind()))?;
        Ok(Admitted { plain, farm, info })
    }

    /// FARM-1 delivered the TFDF, which is the FDU of the reassembler as it is.
    fn fdu<'b>(
        info: Self::Info,
        data: &'b [u8],
        _buf: &'b mut [u8],
    ) -> Option<(Self::Meta, &'b [u8])> {
        Some((info, data))
    }
}
