//! The USLP arm of the flight uplink (A4 WP-3c), as the application's [`UpProtocol`].
//!
//! Specification: docs/reference-application.md §4.3.3: variable-length USLP frames of at most 512
//! octets with the FECF and no Insert Zone or OCF, on VC 0 with one MAP, MAP 0, which permits
//! blocking and prohibits segmentation, COP-1 in effect, a one-octet sequence-controlled VCF
//! Count, no expedited count, the rule `111` (APP-181, APP-182, APP-183).
//!
//! Requirements: APP-181, APP-183, APP-185, APP-186; USLP-013, USLP-014, COP-031.
//!
//! **FARM-1 over USLP (D7).** The library has no `Farm1Input::from_uslp`, so the node reads the
//! checked frame itself: a Type-AD frame (Bypass/Sequence Control Flag 0) is `Ad` with N(S) the
//! VCF Count; the flags `1` and `0` are `Bd`; the flags `1` and `1` are `Bc`, whose TFDZ is
//! exactly one Control Command, read as for TC (232.0 §4.1.3.3). Any other TFDZ makes the frame
//! invalid, which the node refuses as it refuses a TC frame whose Control Command the decoder
//! does not know (OI-100 (a), APP-183, APP-186). The workaround for the missing
//! `Farm1Input::from_uslp` is issue #718, and the single MAP of the uplink (the FOP-1 target takes
//! one, D6) is issue #719, with #235, OI-71 and #661 behind it.
//!
//! **The data FARM-1 delivers** is the whole TFDF, the TFDF Header and the TFDZ, which is what
//! the `MapReassembler` reads on USLP (LNK-070); with the rule `111` it parses the TFDZ as one
//! unit of whole packets, blocked (LNK-071).

use aldis::cop1::farm1::Farm1Frame;
use aldis::core::ids::FrameSeqNum;
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::{DecodeMode, Error, ErrorKind, Result, SecuritySlices};
use aldis::link::protocol::{Protocol, Uslp};
use aldis::link::reassembler::{MapReassemblerConfig, ReassemblerMap, UslpFdu};
use aldis::link::receiver::{
    DeliveryService, FrameReceiver, FrameReceiverConfig, MapSet, UslpChannel, UslpVc, VcEntry,
};
use aldis::tc::{ControlCommand, TcVcId};
use aldis::uslp::{
    FrameKind, MapConfig, OcfUse, PhysicalChannelConfig, Reason, TfdfHeader, TfdzRule,
    TransferFrameType, Upid, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable,
    VcfCountLen, check_frame,
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
    ) -> core::result::Result<Admitted<'a, Self>, Refusal> {
        // The conversion `Farm1Input::from_tc` does for TC, by hand: no `Farm1Input::from_uslp`
        // exists (issue #718).
        let refuse = |kind| Refusal { fecf: false, kind };
        let frame = check_frame(octets, table, DecodeMode::Strict)
            .map_err(|rejection| Refusal {
                fecf: rejection.reason() == Reason::FecfMismatch,
                kind: rejection.error().kind(),
            })?
            .into_view()
            .into_frame();
        // The scenario has no truncated frames, and the table refuses them (APP-181).
        let full = frame
            .full()
            .ok_or_else(|| refuse(ErrorKind::InvalidField))?;
        // The TFDF, the TFDF Header and the TFDZ: the data field of the frame (732.1 §4.1.4).
        let tfdf = full
            .octets()
            .get(full.layout().data())
            .ok_or_else(|| refuse(ErrorKind::InternalError))?;
        let farm = match full.kind() {
            FrameKind::SequenceControlled => Farm1Frame::Ad {
                sequence_number: FrameSeqNum::new(
                    u8::try_from(full.vcf_count().value())
                        .map_err(|_| refuse(ErrorKind::InternalError))?,
                ),
                data: tfdf,
            },
            FrameKind::Expedited => Farm1Frame::Bd { data: tfdf },
            FrameKind::ProtocolControl => Farm1Frame::Bc {
                command: Some(
                    control_command(full.tfdf_header(), full.tfdz())
                        .ok_or_else(|| refuse(ErrorKind::ReservedValue))?,
                ),
            },
        };
        let vc_id = TcVcId::new(full.header().vc_id().get())
            .ok_or_else(|| refuse(ErrorKind::InternalError))?;
        let info = UslpFdu {
            map: full.header().map_id(),
            // FARM-1 lets no gap through, so no frame follows a gap (APP-186).
            gap: false,
        };
        let plain = frame.into_plain(NoSecurity).map_err(|e| refuse(e.kind()))?;
        Ok(Admitted {
            plain,
            vc_id,
            frame: farm,
            info,
        })
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

/// The Control Command of a Type-BC frame: the TFDZ is exactly one COP-1 Control Command under
/// the UPID of that name and the rule `111`, no segmentation (232.0 §4.1.3.3; 732.1
/// §4.1.4.2.2.2.8 d; OI-100 (a); APP-183, APP-185); `None` for any other content.
fn control_command(header: TfdfHeader, tfdz: &[u8]) -> Option<ControlCommand> {
    if header.upid == Upid::Cop1ControlCommands && header.rule == TfdzRule::NoSegmentation {
        ControlCommand::parse(tfdz)
    } else {
        None
    }
}
