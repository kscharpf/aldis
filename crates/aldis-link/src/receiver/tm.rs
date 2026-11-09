//! The `FrameReceiver` procedure for TM frames (132.0 §4.3.3–§4.3.7; LNK-050…LNK-053, LNK-077;
//! OI-77).

use aldis_core::ids::VcId;
use aldis_core::{Error, ErrorKind, Result, field};

use super::{
    ChannelScope, DeliveryService, DiscardReason, FrameContext, ReceiveProtocol, ReceiverOutput,
    header, lay_out, ocf_of, spec, vc_of,
};
use crate::protocol::Tm;

/// The octets of the TM Primary Header (132.0 §4.1.2).
const PRIMARY_HEADER_LEN: usize = 6;
/// The First Header Pointer of an OID frame, "only idle data" (132.0 §4.1.4.6.3).
const OID_POINTER: u16 = 0x7FE;
/// The longest Secondary Header: the ID octet and 63 octets of data (132.0 §4.1.3.2).
const MAX_FSH_LEN: usize = 64;

/// The parameters of a TM physical channel that the receiver needs: where the OCF and the
/// Secondary Header are delivered (132.0 §3.5–3.9; LNK-053).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmChannel {
    ocf: DeliveryService,
    fsh: DeliveryService,
}

impl TmChannel {
    /// The channel whose OCF is delivered by `ocf` and whose Secondary Header by `fsh`.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-077; 132.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn new(ocf: DeliveryService, fsh: DeliveryService) -> TmChannel {
        TmChannel { ocf, fsh }
    }

    /// How the OCF is delivered.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-077; 132.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn ocf(&self) -> DeliveryService {
        self.ocf
    }

    /// How the Secondary Header is delivered.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-077; 132.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn fsh(&self) -> DeliveryService {
        self.fsh
    }
}

/// What a TM VC carries, and so what the receiver delivers for it (132.0 §4.1.4.4; LNK-050,
/// LNK-077).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TmContent {
    /// Packets, with the First Header Pointer: the data field goes to a `PacketExtractor`.
    Packets,
    /// `VCA_SDU`s: each data field is one SDU (Synchronization Flag 1).
    Vca,
    /// A VC whose service the receiver does not provide: `NotServed`.
    Unserved,
}

/// The parameters of a TM VC that the receiver needs (132.0 §5 Table 5-2).
///
/// The presence of the OCF and the length of the Secondary Header are fixed for a VC
/// (132.0 §4.1.2.4.3, §4.1.2.7.2.3), so a frame that differs is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmVc {
    content: TmContent,
    ocf: bool,
    fsh_len: Option<u8>,
}

impl TmVc {
    /// A VC with `content`, with an OCF if `ocf`, and a Secondary Header of `fsh_len` octets
    /// (the ID octet included, 2 to 64) if given.
    ///
    /// # Errors
    /// `InvalidConfig` (`fsh_len`) for a length outside 2 to 64 (132.0 §4.1.3.2).
    pub fn new(content: TmContent, ocf: bool, fsh_len: Option<usize>) -> Result<TmVc> {
        let fsh_len = match fsh_len {
            None => None,
            Some(n) if (2..=MAX_FSH_LEN).contains(&n) => u8::try_from(n).ok(),
            Some(_) => {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN));
            }
        };
        Ok(TmVc {
            content,
            ocf,
            fsh_len,
        })
    }

    /// What the VC carries.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-077; 132.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn content(&self) -> TmContent {
        self.content
    }

    /// Whether its frames have an OCF.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-077; 132.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub const fn ocf(&self) -> bool {
        self.ocf
    }

    /// The octets of its Secondary Header, the ID octet included, if it has one.
    ///
    /// Requirements: LNK-050…LNK-053, LNK-077; 132.0 §4.3.3–§4.3.7; docs/04-engines.md §3.1.
    #[must_use]
    pub fn fsh_len(&self) -> Option<usize> {
        self.fsh_len.map(usize::from)
    }
}

impl ReceiveProtocol for Tm {
    type Channel = TmChannel;
    type VcParams = TmVc;
    type Plain<'a> = aldis_tm::PlainTmFrame<'a>;

    fn plain_octets<'a>(frame: &Self::Plain<'a>) -> &'a [u8] {
        frame.as_bytes()
    }

    fn insert_zone_len(_: &TmChannel) -> usize {
        0
    }

    fn fits(frame_len: usize, fecf: bool, _: &TmChannel, vc: Option<&TmVc>) -> bool {
        let fixed = PRIMARY_HEADER_LEN
            .saturating_add(if fecf { 2 } else { 0 })
            .saturating_add(1);
        let own = vc.map_or(0, |v| {
            v.fsh_len()
                .unwrap_or(0)
                .saturating_add(if v.ocf { 4 } else { 0 })
        });
        frame_len >= fixed.saturating_add(own)
    }

    fn variable_length(_: &TmChannel) -> bool {
        false
    }

    fn raw_insert_zone_offset(_: &TmChannel, _: &[u8]) -> Result<usize> {
        Err(Error::new(ErrorKind::InvalidConfig).with_field(field::RAW_FRAMES))
    }

    fn receive<const V: usize>(cx: &mut FrameContext<'_, Tm, V>, frame: &[u8]) -> Result<()> {
        let [_, b1, mc, vcc, s0, s1, id] = header::<7>(frame)?;
        let vc: VcId<3> = vc_of::<Tm>((b1 >> 1) & 0x07)?;
        let ocf_flag = b1 & 1 == 1;
        let fsh_flag = s0 & 0x80 != 0;
        let sync = s0 & 0x40 != 0;
        let pointer = u16::from_be_bytes([s0 & 0x07, s1]);
        let fsh_len = if fsh_flag {
            usize::from(id & 0x3F).saturating_add(1)
        } else {
            0
        };
        let framed = lay_out(
            frame,
            spec(
                frame.len(),
                PRIMARY_HEADER_LEN,
                fsh_len,
                ocf_flag,
                cx.cfg().fecf(),
            ),
        )?;
        let params = cx.params(vc);
        if let Some(p) = params {
            // The OCF Flag and the Secondary Header Flag are static for a channel (132.0
            // §4.1.2.4.3, §4.1.2.7.2.3; TM-011): a frame that differs is not a validated one.
            let found = (ocf_flag, fsh_flag, fsh_len);
            let wanted = (p.ocf, p.fsh_len.is_some(), p.fsh_len().unwrap_or(0));
            if found.0 != wanted.0 {
                return Err(Error::new(ErrorKind::InvalidField).with_field(field::OCF_FLAG));
            }
            if found.1 != wanted.1 {
                return Err(Error::new(ErrorKind::InvalidField).with_field(field::FSH_FLAG));
            }
            if found.2 != wanted.2 {
                return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FSH_LEN));
            }
        }
        let fsh = fsh_flag.then(|| framed.secondary());
        let ocf = ocf_of(framed.ocf());
        let channel = *cx.cfg().channel();

        // The MC functions run for every frame of the master channel, also one that the VC
        // demultiplexing function discards (132.0 §4.3.5.2; OI-77).
        cx.count_mc(u64::from(mc), 8)?;
        let Some(p) = params else {
            // An unknown VC: the master channel services only.
            let mc_only = |service: DeliveryService| {
                (service == DeliveryService::Mc).then_some(ChannelScope::MasterChannel)
            };
            cx.deliver_fsh(mc_only(channel.fsh), fsh)?;
            cx.deliver_ocf(mc_only(channel.ocf), ocf, None)?;
            return cx.discard(DiscardReason::UnknownChannel, Some(vc), None);
        };
        let raised = cx.count_vc(vc, false, u64::from(vcc), 8)?;
        if raised && p.content != TmContent::Unserved {
            cx.mark_gap(vc);
        }
        cx.deliver_fsh(channel.fsh.scope(vc), fsh)?;
        cx.deliver_ocf(channel.ocf.scope(vc), ocf, None)?;
        if p.content == TmContent::Unserved {
            return cx.discard(DiscardReason::NotServed, Some(vc), None);
        }
        let carries_packets = p.content == TmContent::Packets;
        if carries_packets && !sync && pointer == OID_POINTER {
            // An OID frame: its counts and services count, its data is dropped (LNK-052).
            cx.count_oid();
            return Ok(());
        }
        let data = cx.put(framed.data())?;
        let gap = cx.take_gap(vc);
        cx.push(ReceiverOutput::DataField {
            vc,
            data,
            bit_count: None,
            pointer: carries_packets.then_some(pointer),
            gap,
            replay: None,
        })
    }
}
