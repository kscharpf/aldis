//! The TC arm of the flight uplink, as the application's [`UpProtocol`] (A2, A3).
//!
//! Specification: docs/reference-application.md §4.2: one VC with MAPs 0 and 1 in the Segment
//! Header (APP-026, APP-041, APP-042), the FECF (APP-040), FARM-1 on every frame type (APP-043,
//! APP-044).
//!
//! Requirements: APP-040…APP-043, APP-139.

use aldis::cop1::farm1::Farm1Input;
use aldis::core::security::{NoSecurity, SecurityLayout};
use aldis::core::{DecodeMode, Error, ErrorKind, Result, SecuritySlices};
use aldis::link::protocol::Tc;
use aldis::link::reassembler::{MapReassemblerConfig, ReassemblerMap, TcReassemblerParams};
use aldis::link::receiver::{FrameReceiver, FrameReceiverConfig, TcChannel, TcVc, VcEntry};
use aldis::tc::{
    MapConfig, PhysicalChannelConfig, Reason, SegmentHeader, SegmentHeaderUse, TcMapId,
    TcSpacecraftId, TcVcId, VcConfig, VcTable, check_frame,
};
use refapp_scenario::generated::{SPACECRAFT_ID, tc};

use super::{Admitted, Refusal, UpProtocol};

/// The physical channel of the uplink: the maximum frame length and the FECF (APP-040).
fn physical() -> Result<PhysicalChannelConfig> {
    PhysicalChannelConfig::new(usize::from(tc::MAX_FRAME_LENGTH), tc::FECF)
}

impl UpProtocol for Tc {
    const VCID: u8 = tc::VCID;
    const GAP_DISCARD: Option<u16> = None;
    type Table = VcTable<1, 2>;
    /// The Segment Header of the frame, which FARM-1 does not deliver (232.0 §4.1.3.2).
    type Info = Option<SegmentHeader>;

    fn table() -> Result<Self::Table> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let scid = TcSpacecraftId::new(SPACECRAFT_ID).ok_or(bad)?;
        let vc = TcVcId::new(tc::VCID).ok_or(bad)?;
        let frame_len = usize::from(tc::MAX_FRAME_LENGTH);
        let map = |id: u8| {
            TcMapId::new(id)
                .map(|id| MapConfig::new(scid, vc, id, SecurityLayout::NONE))
                .ok_or(bad)
        };
        let header = if tc::SEGMENT_HEADER {
            SegmentHeaderUse::Present
        } else {
            SegmentHeaderUse::Absent(SecurityLayout::NONE)
        };
        VcTable::new(
            physical()?,
            &[VcConfig::new(scid, vc, frame_len, header)?],
            &[map(tc::MAP_COMMANDS)?, map(tc::MAP_TABLE_UPLOAD)?],
        )
    }

    fn receiver() -> Result<FrameReceiver<Self, 1>> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let vc = TcVcId::new(tc::VCID).ok_or(bad)?;
        Ok(FrameReceiver::new(FrameReceiverConfig::new(
            TcChannel::new(physical()?),
            &[VcEntry::new(vc, TcVc::SERVED)],
        )?))
    }

    fn reassembly() -> Result<MapReassemblerConfig<Self, 2>> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let vc = TcVcId::new(tc::VCID).ok_or(bad)?;
        let max_packet = usize::from(tc::MAX_PACKET_LENGTH);
        let packets = |id: u8| {
            TcMapId::new(id)
                .ok_or(bad)
                .and_then(|id| ReassemblerMap::packets(vc, id, max_packet))
        };
        MapReassemblerConfig::new(
            TcReassemblerParams::with_segment_header(),
            &[packets(tc::MAP_COMMANDS)?, packets(tc::MAP_TABLE_UPLOAD)?],
        )
    }

    fn admit<'a>(
        table: &Self::Table,
        octets: &'a [u8],
        buffer_available: bool,
    ) -> core::result::Result<Admitted<'a, Self>, Refusal> {
        let validated = check_frame(octets, table, DecodeMode::Strict)
            .map_err(|rejection| Refusal {
                fecf: rejection.reason() == Reason::FecfMismatch,
                kind: rejection.error().kind(),
            })?
            .into_view();
        let plain = validated
            .into_frame()
            .into_plain(NoSecurity)
            .map_err(|e| Refusal {
                fecf: false,
                kind: e.kind(),
            })?;
        Ok(Admitted {
            plain,
            farm: Farm1Input::from_tc(&validated, buffer_available),
            info: validated.segment_header(),
        })
    }

    /// FARM-1 delivers the user data of the frame; the reassembler reads the Frame Data Unit as
    /// the Segment Header followed by the user data (232.0 §4.1.3.2), so the header octet is
    /// put back in front.
    fn fdu<'b>(
        info: Self::Info,
        data: &'b [u8],
        buf: &'b mut [u8],
    ) -> Option<(Self::Meta, &'b [u8])> {
        let header = info?;
        let dst = buf.get_mut(..data.len().saturating_add(1))?;
        for (d, s) in dst
            .iter_mut()
            .zip(core::iter::once(&header.octet()).chain(data))
        {
            *d = *s;
        }
        Some(((), &*dst))
    }
}
