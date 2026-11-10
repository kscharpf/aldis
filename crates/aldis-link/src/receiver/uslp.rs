//! The `FrameReceiver` procedure for USLP frames of a physical channel of fixed or variable
//! length, without truncated frames (732.1 §4.3.5–§4.3.11; LNK-050…LNK-053, LNK-079…LNK-081;
//! OI-77, OI-78).

use aldis_core::FrameFormat;
use aldis_core::engine::OutRange;
use aldis_core::ids::{MapId, VcId};
use aldis_core::{Error, ErrorKind, Result, field};
use aldis_uslp::{PhysicalChannelConfig as UslpPhysicalChannelConfig, TransferFrameType};

use super::{
    ChannelScope, DeliveryService, DiscardReason, FrameContext, ReceiveProtocol, ReceiverOutput,
    header, lay_out, ocf_of, spec,
};
use crate::protocol::Uslp;

/// The octets of the USLP Primary Header before the VCF Count (732.1 §4.1.2).
const PRIMARY_HEADER_LEN: usize = Uslp::PRIMARY_HEADER_LEN;
/// The VCID of an OID frame, all ones (732.1 §4.1.4.1.6).
const OID_VC: u8 = 63;

/// The parameters of a USLP physical channel that the receiver needs (732.1 §5 Table 5-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UslpChannel {
    physical: UslpPhysicalChannelConfig,
    ocf: DeliveryService,
}

impl UslpChannel {
    /// The channel of the physical channel `physical`, whose frame type, frame length, Insert
    /// Zone and FECF are the receiver's (a variable-length channel has frames of any length up
    /// to the maximum and no Insert Zone, 732.1 §4.1.2.7, §4.1.3.2), whose OCF is delivered by
    /// `ocf` (732.1 §3.11.2.4).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_len`) if a frame of that length would have no data field octet
    /// after the header, the Insert Zone and the FECF (CMN-041).
    pub fn new(physical: UslpPhysicalChannelConfig, ocf: DeliveryService) -> Result<UslpChannel> {
        let channel = UslpChannel { physical, ocf };
        if <Uslp as ReceiveProtocol>::fits(physical.frame_len(), &channel, None) {
            Ok(channel)
        } else {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FRAME_LEN))
        }
    }

    /// The managed parameters of the physical channel.
    #[must_use]
    pub const fn physical(&self) -> &UslpPhysicalChannelConfig {
        &self.physical
    }

    /// Whether the frames vary in length.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn is_variable(&self) -> bool {
        matches!(self.physical.frame_type(), TransferFrameType::Variable)
    }

    /// The octets of the Insert Zone.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn insert_zone_len(&self) -> usize {
        self.physical.insert_zone_len()
    }

    /// How the OCF is delivered.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn ocf(&self) -> DeliveryService {
        self.ocf
    }
}

/// The MAP IDs of a VC, a set of the 16 values of the 4-bit field (732.1 §4.1.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MapSet(u16);

impl MapSet {
    /// The set of `maps`.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub fn new(maps: &[MapId<4>]) -> MapSet {
        MapSet(maps.iter().fold(0u16, |set, map| {
            set | 1u16.wrapping_shl(u32::from(map.get()))
        }))
    }

    /// Whether `map` is in the set.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub fn contains(&self, map: MapId<4>) -> bool {
        self.0 & 1u16.wrapping_shl(u32::from(map.get())) != 0
    }
}

/// The parameters of a USLP VC that the receiver needs (732.1 §5 Table 5-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UslpVc {
    maps: MapSet,
    cop: bool,
    served: bool,
}

impl UslpVc {
    /// A VC with the MAPs `maps`. With `cop` the COP is in effect on it and the FARM-1 checks
    /// decide the sequence (732.1 §4.3.7.2), so the receiver does not count its frames
    /// (LNK-051). With `served` false the receiver does not provide its service: `NotServed`.
    #[must_use]
    pub const fn new(maps: MapSet, cop: bool, served: bool) -> UslpVc {
        UslpVc { maps, cop, served }
    }

    /// The MAPs of the VC.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn maps(&self) -> MapSet {
        self.maps
    }

    /// Whether the COP is in effect.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn cop(&self) -> bool {
        self.cop
    }

    /// Whether the receiver serves the VC.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-079…LNK-081; 732.1 §4.3.5–§4.3.11; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn served(&self) -> bool {
        self.served
    }
}

impl ReceiveProtocol for Uslp {
    type Channel = UslpChannel;
    type VcParams = UslpVc;
    type Plain<'a> = aldis_core::Plain<aldis_uslp::UslpFrame<'a>>;

    fn plain_octets<'a>(frame: &Self::Plain<'a>) -> &'a [u8] {
        frame.octets()
    }

    fn physical(channel: &UslpChannel) -> &UslpPhysicalChannelConfig {
        &channel.physical
    }

    fn fits(frame_len: usize, channel: &UslpChannel, _: Option<&UslpVc>) -> bool {
        let fecf = channel.physical.fecf();
        let needed = PRIMARY_HEADER_LEN
            .saturating_add(channel.physical.insert_zone_len())
            .saturating_add(if fecf { 2 } else { 0 })
            .saturating_add(1);
        frame_len >= needed
    }

    fn refuse_unserved(_: &UslpChannel, frame: &[u8]) -> Result<()> {
        // A truncated frame (Annex D, End of Frame Primary Header Flag 1, bit 0 of the fourth
        // octet) has a 4-octet header and no Frame Length: not served, whatever its length.
        match frame.get(3) {
            Some(octet) if octet & 0x01 != 0 => Err(Error::new(ErrorKind::InvalidField)
                .with_field(field::END_OF_FRAME_PRIMARY_HEADER_FLAG)),
            _ => Ok(()),
        }
    }

    fn raw_insert_zone_offset(_: &UslpChannel, frame: &[u8]) -> Result<usize> {
        // The Insert Zone follows a header of 7 octets only when the VCF Count Length (the low
        // three bits of the flags octet) is 0. That field belongs to the header the caller
        // rejected, so a frame that has a count is refused (OI-78).
        let [.., flags] = header::<PRIMARY_HEADER_LEN>(frame)?;
        if flags.trailing_zeros() >= 3 {
            Ok(PRIMARY_HEADER_LEN)
        } else {
            Err(Error::new(ErrorKind::InvalidField).with_field(field::VCF_COUNT_LEN))
        }
    }

    fn receive<const V: usize>(
        cx: &mut FrameContext<'_, Uslp, V>,
        plain: &Self::Plain<'_>,
    ) -> Result<()> {
        // The codec has checked that the Frame Length field (one less than the length of the
        // frame, 732.1 §4.1.2.7) and the Protocol Control Command Flag (Bypass 0 with Protocol
        // Control Command 1 is reserved, Table 4-1) are valid, and read the header: the fields
        // are those of the `UslpFrame` it decoded (RS-023). A truncated frame has no Frame
        // Length and no such flags; `refuse_unserved` refused it before the length check.
        let frame = plain.octets();
        let full = plain.full().ok_or(
            Error::new(ErrorKind::InvalidField).with_field(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
        )?;
        let (vc, map) = (full.header().vc_id(), full.header().map_id());
        let kind = full.kind();
        let (bypass, control) = (kind.bypass_flag(), kind.protocol_control_flag());
        let ocf_flag = full.header().ocf_flag();
        let vcf = full.vcf_count();
        let count_len = vcf.len().get();
        let count = vcf.value();
        let channel = *cx.cfg().channel();
        let framed = lay_out(
            frame,
            spec(
                frame.len(),
                PRIMARY_HEADER_LEN.saturating_add(usize::from(count_len)),
                channel.physical.insert_zone_len(),
                ocf_flag,
                cx.cfg().fecf(),
            ),
        )?;
        let oid = vc.get() == OID_VC;
        let params = cx.params(vc);
        let ocf = ocf_of(framed.ocf());
        let ocf_scope = channel.ocf.scope(vc);
        let iz = framed.secondary();
        // The Insert Zone and the OCF are extracted before the frame is discarded, whatever the
        // GVCID and the GMAP ID (732.1 §4.3.11.1.4, §4.3.9.2–3, §4.3.8 note 2; OI-77).
        if !oid && params.is_none() {
            deliver_tail(cx, iz, ocf_scope, ocf)?;
            return cx.discard(DiscardReason::UnknownChannel, Some(vc), Some(map));
        }
        // VC Reception comes before the MAP demultiplexing function (732.1 §4.3.7.4, §4.3.5.3):
        // the VCF Count of the VC for the quality of service of the frame, unless the COP is in
        // effect (LNK-051). A count length of 0 has no count.
        let counted = !oid && count_len > 0 && !params.is_some_and(|p| p.cop);
        let raised =
            counted && cx.count_vc(vc, bypass, count, u32::from(count_len).saturating_mul(8))?;
        if raised {
            cx.mark_gap(vc);
        }
        let known_map = oid || params.is_some_and(|p| p.maps.contains(map));
        if !known_map {
            deliver_tail(cx, iz, ocf_scope, ocf)?;
            return cx.discard(DiscardReason::UnknownChannel, Some(vc), Some(map));
        }
        if !oid && !params.is_some_and(|p| p.served) {
            deliver_tail(cx, iz, ocf_scope, ocf)?;
            return cx.discard(DiscardReason::NotServed, Some(vc), Some(map));
        }
        if control && !oid {
            // Protocol control commands go to the COP, never to an extractor (LNK-081). The
            // frame is copied once, and the Insert SDU is a range of that copy.
            let whole = cx.put(frame)?;
            let insert = OutRange::new(
                whole
                    .start()
                    .saturating_add(PRIMARY_HEADER_LEN.saturating_add(usize::from(count_len))),
                iz.len(),
            );
            cx.deliver_insert_range(insert, None)?;
            cx.deliver_ocf(ocf_scope, ocf, None)?;
            return cx.push(ReceiverOutput::ProtocolControl { vc, frame: whole });
        }
        deliver_tail(cx, iz, ocf_scope, ocf)?;
        if oid {
            cx.count_oid();
            return Ok(());
        }
        let data = cx.put(framed.data())?;
        let gap = cx.take_gap(vc);
        cx.push(ReceiverOutput::DataField {
            vc,
            data,
            bit_count: None,
            pointer: None,
            gap,
            replay: None,
        })
    }
}

/// The Insert SDU and the OCF of a frame (LNK-053).
fn deliver_tail<const V: usize>(
    cx: &mut FrameContext<'_, Uslp, V>,
    insert: &[u8],
    ocf_scope: Option<ChannelScope<VcId<6>>>,
    ocf: Option<aldis_core::ocf::Ocf>,
) -> Result<()> {
    cx.deliver_insert(insert, None)?;
    cx.deliver_ocf(ocf_scope, ocf, None)
}

impl<P: ReceiveProtocol, const V: usize> FrameContext<'_, P, V> {
    /// Delivers an Insert Zone that is already in the output area (a range of the whole frame,
    /// LNK-075); nothing for a channel without one.
    fn deliver_insert_range(&mut self, sdu: OutRange, replay: Option<bool>) -> Result<()> {
        if sdu.is_empty() {
            Ok(())
        } else {
            self.push(ReceiverOutput::InsertSdu { sdu, replay })
        }
    }
}
