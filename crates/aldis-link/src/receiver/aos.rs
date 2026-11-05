//! The `FrameReceiver` procedure for AOS frames (732.0 §4.3.3–§4.3.7; LNK-050…LNK-054, LNK-077;
//! OI-70, OI-77, OI-78).

use aldis_aos::{BitstreamPointer, Bpdu};
use aldis_core::Result;
use aldis_core::ids::VcId;

use super::{
    ChannelScope, DiscardReason, FrameContext, ReceiveProtocol, ReceiverOutput, header, lay_out,
    ocf_of, spec, vc_of,
};
use crate::protocol::Aos;

/// The octets of the AOS Primary Header without the FHEC (732.0 §4.1.2).
const PRIMARY_HEADER_LEN: usize = 6;
/// The octets of the Frame Header Error Control (732.0 §4.1.2.7).
const FHEC_LEN: usize = 2;
/// The VCID of an OID frame, all ones (732.0 §4.1.2.3.3, §4.1.4.1.5.1).
const OID_VC: u8 = 63;

/// The parameters of an AOS physical channel that the receiver needs (732.0 §5 Table 5-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosChannel {
    insert_zone_len: usize,
    fhec: bool,
}

impl AosChannel {
    /// The channel with an Insert Zone of `insert_zone_len` octets (0 for none) and a Frame
    /// Header Error Control if `fhec`.
    ///
    /// Requirements: LNK-050…LNK-054, LNK-077; 732.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn new(insert_zone_len: usize, fhec: bool) -> AosChannel {
        AosChannel {
            insert_zone_len,
            fhec,
        }
    }

    /// The octets of the Insert Zone.
    ///
    /// Requirements: LNK-050…LNK-054, LNK-077; 732.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn insert_zone_len(&self) -> usize {
        self.insert_zone_len
    }

    /// Whether frames have an FHEC.
    ///
    /// Requirements: LNK-050…LNK-054, LNK-077; 732.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn fhec(&self) -> bool {
        self.fhec
    }

    const fn header_len(&self) -> usize {
        if self.fhec {
            PRIMARY_HEADER_LEN + FHEC_LEN
        } else {
            PRIMARY_HEADER_LEN
        }
    }
}

/// What an AOS VC carries, and so what the receiver delivers for it (732.0 §4.1.4; LNK-050,
/// LNK-077).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AosContent {
    /// `M_PDU`s: the data field goes to a `PacketExtractor`.
    Packets,
    /// `VCA_SDU`s: each data field is one SDU.
    Vca,
    /// `B_PDU`s: the valid bits of each are delivered, the idle bits dropped.
    Bitstream,
    /// A VC whose service the receiver does not provide: `NotServed`.
    Unserved,
}

/// The parameters of an AOS VC that the receiver needs (732.0 §5 Table 5-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosVc {
    content: AosContent,
    ocf: bool,
}

impl AosVc {
    /// A VC with `content`, whose frames carry an OCF if `ocf` (the `VC_OCF` service).
    ///
    /// Requirements: LNK-050…LNK-054, LNK-077; 732.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn new(content: AosContent, ocf: bool) -> AosVc {
        AosVc { content, ocf }
    }

    /// What the VC carries.
    ///
    /// Requirements: LNK-050…LNK-054, LNK-077; 732.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn content(&self) -> AosContent {
        self.content
    }

    /// Whether its frames have an OCF.
    ///
    /// Requirements: LNK-050…LNK-054, LNK-077; 732.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn ocf(&self) -> bool {
        self.ocf
    }
}

impl ReceiveProtocol for Aos {
    type Channel = AosChannel;
    type VcParams = AosVc;
    type Plain<'a> = aldis_aos::PlainAosFrame<'a>;

    fn plain_octets<'a>(frame: &Self::Plain<'a>) -> &'a [u8] {
        frame.as_bytes()
    }

    fn insert_zone_len(channel: &AosChannel) -> usize {
        channel.insert_zone_len
    }

    fn fits(frame_len: usize, fecf: bool, channel: &AosChannel, vc: Option<&AosVc>) -> bool {
        let needed = channel
            .header_len()
            .saturating_add(channel.insert_zone_len)
            .saturating_add(if fecf { 2 } else { 0 })
            .saturating_add(if vc.is_some_and(|v| v.ocf) { 4 } else { 0 })
            .saturating_add(1);
        frame_len >= needed
    }

    fn variable_length(_: &AosChannel) -> bool {
        false
    }

    fn raw_insert_zone_offset(channel: &AosChannel, _: &[u8]) -> Result<usize> {
        Ok(channel.header_len())
    }

    fn receive<const V: usize>(cx: &mut FrameContext<'_, Aos, V>, frame: &[u8]) -> Result<()> {
        let [_, b1, c0, c1, c2, sig] = header::<PRIMARY_HEADER_LEN>(frame)?;
        let vc: VcId<6> = vc_of::<Aos>(b1 & 0x3F)?;
        let replay = sig & 0x80 != 0;
        let cycle_use = sig & 0x40 != 0;
        let oid = vc.get() == OID_VC;
        let params = cx.params(vc);
        let channel = *cx.cfg().channel();
        // The OCF is a service of the VC (VC_OCF): a frame of a VC that is not configured has
        // none to extract (732.0 §4.3.7.4).
        let ocf_present = params.is_some_and(|p| p.ocf);
        let framed = lay_out(
            frame,
            spec(
                frame.len(),
                channel.header_len(),
                channel.insert_zone_len,
                ocf_present,
                cx.cfg().fecf(),
            ),
        )?;
        // A B_PDU is read before any state changes: a pointer beyond the zone refuses the frame.
        let served_bitstream = !oid && params.is_some_and(|p| p.content == AosContent::Bitstream);
        let bpdu = if served_bitstream {
            Some(Bpdu::decode(framed.data())?)
        } else {
            None
        };
        let replay_flag = Some(replay);
        if !oid && params.is_none() {
            // The Insert Zone is extracted whatever the GVCID (732.0 §4.3.7.4; LNK-050).
            cx.deliver_insert(framed.secondary(), replay_flag)?;
            return cx.discard(DiscardReason::UnknownChannel, Some(vc), None);
        }
        // Frames of the Replay service take no part in the counting (OI-70).
        let mut raised = false;
        if !oid && !replay {
            let (value, bits) = if cycle_use {
                (u64::from(u32::from_be_bytes([sig & 0x0F, c0, c1, c2])), 28)
            } else {
                (u64::from(u32::from_be_bytes([0, c0, c1, c2])), 24)
            };
            // A change of the Cycle Use Flag restarts the count: a discontinuity (OI-78).
            let changed = cx.change_cycle_use(vc, cycle_use);
            let counted = cx.count_vc(vc, false, value, bits)?;
            if changed {
                cx.discontinuity(vc)?;
            }
            raised = counted || changed;
        }
        let content = params.map(|p| p.content);
        if content == Some(AosContent::Unserved) {
            cx.deliver_insert(framed.secondary(), replay_flag)?;
            return cx.discard(DiscardReason::NotServed, Some(vc), None);
        }
        if raised {
            cx.mark_gap(vc);
        }
        cx.deliver_insert(framed.secondary(), replay_flag)?;
        cx.deliver_ocf(
            Some(ChannelScope::VirtualChannel(vc)),
            ocf_of(framed.ocf()).filter(|_| ocf_present),
            replay_flag,
        )?;
        if oid {
            cx.count_oid();
            return Ok(());
        }
        let (data, bit_count) = match bpdu {
            Some(b) => match b.pointer() {
                // The B_PDU holds no valid bit: it is dropped (732.0 §4.1.4.3.3.4; LNK-077).
                BitstreamPointer::OnlyIdle => return Ok(()),
                BitstreamPointer::AllValid => {
                    let zone = b.data_zone();
                    let bits = zone.len().saturating_mul(8);
                    (cx.put_bits(zone, bits)?, Some(bits))
                }
                BitstreamPointer::ValidBits(count) => {
                    let bits = usize::from(count.get());
                    (cx.put_bits(b.data_zone(), bits)?, Some(bits))
                }
            },
            None => (cx.put(framed.data())?, None),
        };
        let gap = cx.take_gap(vc);
        cx.push(ReceiverOutput::DataField {
            vc,
            data,
            bit_count,
            pointer: None,
            gap,
            replay: replay_flag,
        })
    }
}
