//! The `FrameReceiver` procedure for USLP frames of a physical channel of fixed or variable
//! length, without truncated frames (732.1 §4.3.5–§4.3.11; LNK-050…LNK-053, LNK-079…LNK-081;
//! OI-77, OI-78).

use aldis_core::engine::OutRange;
use aldis_core::ids::{MapId, VcId};
use aldis_core::{Error, ErrorKind, Result, field};

use super::{
    ChannelScope, DiscardReason, FrameContext, ReceiveProtocol, ReceiverOutput, Service, header,
    lay_out, map_of, ocf_of, spec, vc_of,
};
use crate::protocol::Uslp;

/// The octets of the USLP Primary Header before the VCF Count (732.1 §4.1.2).
const PRIMARY_HEADER_LEN: usize = 7;
/// The VCID of an OID frame, all ones (732.1 §4.1.4.1.6).
const OID_VC: u8 = 63;

/// The parameters of a USLP physical channel that the receiver needs (732.1 §5 Table 5-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UslpChannel {
    insert_zone_len: usize,
    ocf: Service,
    variable: bool,
}

impl UslpChannel {
    /// The channel with an Insert Zone of `insert_zone_len` octets (0 for none) whose OCF is
    /// delivered by `ocf` (732.1 §3.11.2.4).
    #[must_use]
    pub const fn new(insert_zone_len: usize, ocf: Service) -> UslpChannel {
        UslpChannel {
            insert_zone_len,
            ocf,
            variable: false,
        }
    }

    /// A variable-length channel, whose frames are of any length up to the configured maximum
    /// and have no Insert Zone (732.1 §4.1.2.7, §4.1.3.2). Its OCF is delivered by `ocf`.
    #[must_use]
    pub const fn variable(ocf: Service) -> UslpChannel {
        UslpChannel {
            insert_zone_len: 0,
            ocf,
            variable: true,
        }
    }

    /// Whether the frames vary in length.
    #[must_use]
    pub const fn is_variable(&self) -> bool {
        self.variable
    }

    /// The octets of the Insert Zone.
    #[must_use]
    pub const fn insert_zone_len(&self) -> usize {
        self.insert_zone_len
    }

    /// How the OCF is delivered.
    #[must_use]
    pub const fn ocf(&self) -> Service {
        self.ocf
    }
}

/// The MAP IDs of a VC, a set of the 16 values of the 4-bit field (732.1 §4.1.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MapSet(u16);

impl MapSet {
    /// The set of `maps`.
    #[must_use]
    pub fn new(maps: &[MapId<4>]) -> MapSet {
        MapSet(maps.iter().fold(0u16, |set, map| {
            set | 1u16.wrapping_shl(u32::from(map.get()))
        }))
    }

    /// Whether `map` is in the set.
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
    #[must_use]
    pub const fn maps(&self) -> MapSet {
        self.maps
    }

    /// Whether the COP is in effect.
    #[must_use]
    pub const fn cop(&self) -> bool {
        self.cop
    }

    /// Whether the receiver serves the VC.
    #[must_use]
    pub const fn served(&self) -> bool {
        self.served
    }
}

impl ReceiveProtocol for Uslp {
    type Channel = UslpChannel;
    type VcParams = UslpVc;

    fn insert_zone_len(channel: &UslpChannel) -> usize {
        channel.insert_zone_len
    }

    fn fits(frame_len: usize, fecf: bool, channel: &UslpChannel, _: Option<&UslpVc>) -> bool {
        let needed = PRIMARY_HEADER_LEN
            .saturating_add(channel.insert_zone_len)
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

    fn variable_length(channel: &UslpChannel) -> bool {
        channel.variable
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

    fn receive<const V: usize>(cx: &mut FrameContext<'_, Uslp, V>, frame: &[u8]) -> Result<()> {
        let [_, _, t2, t3, l0, l1, flags] = header::<PRIMARY_HEADER_LEN>(frame)?;
        // The Frame Length field is one less than the length of this frame (732.1 §4.1.2.7).
        if usize::from(u16::from_be_bytes([l0, l1])).saturating_add(1) != frame.len() {
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN));
        }
        // TFVN (4 bits), SCID (16), Source or Destination (1), VCID (6), MAP ID (4), End of
        // Frame Primary Header Flag (1), from the most significant bit (732.1 §4.1.2).
        let vc: VcId<6> = vc_of::<Uslp>(((t2 & 0x07) << 3) | (t3 >> 5))?;
        let map: MapId<4> = map_of((t3 >> 1) & 0x0F)?;
        let bypass = flags & 0x80 != 0;
        let control = flags & 0x40 != 0;
        let ocf_flag = flags & 0x08 != 0;
        let count_len = flags & 0x07;
        if !bypass && control {
            // Bypass 0 with Protocol Control Command 1 is reserved (732.1 Table 4-1).
            return Err(Error::new(ErrorKind::ReservedValue)
                .with_field(field::PROTOCOL_CONTROL_COMMAND_FLAG));
        }
        let channel = *cx.cfg().channel();
        let framed = lay_out(
            frame,
            spec(
                frame.len(),
                PRIMARY_HEADER_LEN.saturating_add(usize::from(count_len)),
                channel.insert_zone_len,
                ocf_flag,
                cx.cfg().fecf(),
            ),
        )?;
        let count = framed
            .primary_header()
            .iter()
            .skip(PRIMARY_HEADER_LEN)
            .fold(0u64, |count, &octet| {
                count.wrapping_shl(8) | u64::from(octet)
            });
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
