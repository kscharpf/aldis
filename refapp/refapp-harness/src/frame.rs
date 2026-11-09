//! Strict decoding of a link unit by level (docs/reference-application.md §6.1, APP-086).
//!
//! At A2 on a link unit is a frame: a TM frame on the downlink, a TC frame on the uplink. The
//! trace tests decode every `unit_sent` with the strict codecs; this module is the function
//! (the check over the golden traces is WP-6).

use aldis::core::security::SecurityLayout;
use aldis::tc::{
    MapConfig, PhysicalChannelConfig, SegmentHeaderUse, TcFrame, VcConfig, VcLookup, VcTable,
};
use aldis::tm::{TmFrame, TmPhysicalChannelConfig};

use crate::scenario::{Level, Scenario};
use crate::trace::Dir;

/// A strictly decoded frame: what the nodes and the trace checks read of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Virtual channel ID.
    pub vcid: u8,
    /// The data field (TM: the packet zone; TC: the Segment Header and the FDU).
    pub data: Vec<u8>,
}

/// Decodes the octets of a link unit sent in direction `dir`, in strict mode (APP-086): a TM
/// frame for `down`, a TC frame for `up`. A unit of a [`Level::Packets`] case is a Space Packet
/// ([`crate::trace::decode_strict`]).
///
/// The frame must carry the Spacecraft ID of the scenario (APP-026), and a TM frame must fill
/// the configured frame length.
///
/// # Errors
/// If `level` is [`Level::Packets`], the managed parameters of the scenario do not form a
/// configuration, or the codec rejects the frame (FECF, lengths, fields) or the frame has
/// another Spacecraft ID.
pub fn decode_frame(
    scenario: &Scenario,
    level: Level,
    dir: Dir,
    octets: &[u8],
) -> Result<DecodedFrame, String> {
    if level == Level::Packets {
        return Err("a unit of a packet-level case is a Space Packet, not a frame".to_owned());
    }
    let scid = scenario.spacecraft_id.get();
    match dir {
        Dir::Down => {
            let tm = &scenario.tm;
            let pc = TmPhysicalChannelConfig::new(usize::from(tm.frame_length.get()), tm.fecf)
                .map_err(|e| format!("TM channel of the scenario: {e:?}"))?;
            let frame = TmFrame::decode(octets, &pc)
                .map_err(|e| format!("not a strict TM frame: {e:?}"))?;
            if frame.scid().get() != scid {
                return Err(format!(
                    "TM frame has Spacecraft ID {}, expected {scid}",
                    frame.scid().get()
                ));
            }
            Ok(DecodedFrame {
                vcid: frame.vcid().get(),
                data: frame.data_field().to_vec(),
            })
        }
        Dir::Up => {
            let tc = &scenario.tc;
            let max = usize::from(tc.max_frame_length.get());
            let id_err =
                |what: &str, e: &dyn std::fmt::Debug| format!("TC {what} of the scenario: {e:?}");
            let out_of_range = |what: &str| format!("TC {what} of the scenario is out of range");
            let scid_t =
                aldis::tc::TcSpacecraftId::new(scid).ok_or_else(|| out_of_range("SCID"))?;
            let vc_t = aldis::tc::TcVcId::new(tc.vcid.get()).ok_or_else(|| out_of_range("VCID"))?;
            let map = |id: u8| {
                aldis::tc::TcMapId::new(id)
                    .map(|m| MapConfig::new(scid_t, vc_t, m, SecurityLayout::NONE))
                    .ok_or_else(|| out_of_range("MAP ID"))
            };
            let physical =
                PhysicalChannelConfig::new(max, tc.fecf).map_err(|e| id_err("channel", &e))?;
            let vc = VcConfig::new(scid_t, vc_t, max, SegmentHeaderUse::Present)
                .map_err(|e| id_err("VC", &e))?;
            let table: VcTable<1, 2> = VcTable::new(
                physical,
                &[vc],
                &[map(tc.map_commands.get())?, map(tc.map_table_upload.get())?],
            )
            .map_err(|e| id_err("channels", &e))?;
            let frame = TcFrame::decode(octets, &table as &dyn VcLookup)
                .map_err(|e| format!("not a strict TC frame: {e:?}"))?;
            if frame.spacecraft_id() != scid_t {
                return Err(format!(
                    "TC frame has Spacecraft ID {}, expected {scid}",
                    frame.spacecraft_id().get()
                ));
            }
            Ok(DecodedFrame {
                vcid: frame.vc_id().get(),
                data: frame.data_field().to_vec(),
            })
        }
    }
}
