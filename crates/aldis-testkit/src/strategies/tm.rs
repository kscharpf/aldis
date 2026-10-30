//! Strategies for `aldis-tm` (132.0; VER-054).
//!
//! Primary Header fields, physical and master channel configurations with their virtual
//! channels, whole TM Transfer Frames built by the crate's own encoder, and frame octets next to
//! the decoder's boundaries.

use aldis_core::ids::VcId;
use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FirstHeaderPointer, FshLen, PacketOffset, PacketPointer,
    SegmentLengthId, TmFrameContent, TmFrameFields, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig, encode,
};
use proptest::prelude::*;

use super::common::{spacecraft_id, up_to_u16, vc_id};
use super::octets;

/// Any Segment Length Identifier (132.0 §4.1.2.7.5; TM-007).
pub fn segment_length_id() -> impl Strategy<Value = SegmentLengthId> {
    (0..=SegmentLengthId::MAX).prop_filter_map("in range", SegmentLengthId::new)
}

/// Any raw First Header Pointer, the two reserved values drawn more often (132.0 §4.1.2.7.6;
/// TM-008).
pub fn first_header_pointer() -> impl Strategy<Value = FirstHeaderPointer> {
    prop_oneof![
        1 => Just(FirstHeaderPointer::NO_PACKET_START),
        1 => Just(FirstHeaderPointer::ONLY_IDLE_DATA),
        4 => up_to_u16(FirstHeaderPointer::MAX).prop_filter_map("in range", FirstHeaderPointer::new),
    ]
}

/// Any packet offset, 0 to `0x7FD`, the edges drawn more often (132.0 §4.1.2.7.6.3–5; TM-008,
/// RS-020).
pub fn packet_offset() -> impl Strategy<Value = PacketOffset> {
    up_to_u16(PacketOffset::MAX).prop_filter_map("in range", PacketOffset::new)
}

/// Any Secondary Header length, 2–64 octets (132.0 §4.1.3.1.3; TM-003).
pub fn fsh_len() -> impl Strategy<Value = FshLen> {
    (FshLen::MIN..=FshLen::MAX).prop_filter_map("in range", FshLen::new)
}

/// A physical channel of 7–299 octet frames with or without the FECF (Table 5-1; TM-001).
pub fn physical_channel() -> impl Strategy<Value = TmPhysicalChannelConfig> {
    (7_usize..300, any::<bool>()).prop_filter_map("a valid frame length", |(len, fecf)| {
        TmPhysicalChannelConfig::new(len, fecf).ok()
    })
}

/// A virtual channel `vcid` with any content, an optional `VC_FSH` and `VC_OCF`, and a small
/// security layout (Table 5-3, Table 6-1).
fn virtual_channel(vcid: VcId<3>) -> impl Strategy<Value = TmVirtualChannelConfig> {
    (
        any::<bool>(),
        proptest::option::weighted(0.3, fsh_len()),
        proptest::bool::weighted(0.3),
        proptest::option::weighted(0.3, (0_usize..8, 0_usize..8)),
    )
        .prop_map(move |(vca, fsh, ocf, security)| {
            let content = if vca {
                DataFieldContent::VcaSdu
            } else {
                DataFieldContent::Packets
            };
            let mut vc = TmVirtualChannelConfig::new(vcid, content);
            if let Some(len) = fsh {
                vc = vc.with_fsh(len);
            }
            if ocf {
                vc = vc.with_ocf();
            }
            if let Some((h, t)) = security {
                vc = vc.with_security(SecurityLayout::new(h, t).unwrap_or(SecurityLayout::NONE));
            }
            vc
        })
}

/// A master channel with one to eight virtual channels (Tables 5-1 to 5-3; TM-011, API-040).
///
/// It has an optional `MC_FSH` and `MC_OCF`, and each virtual channel is accepted by `with_vc`.
/// Virtual channels the master channel rejects are left out; a master channel left with none is
/// not drawn.
pub fn master_channel() -> impl Strategy<Value = TmMasterChannelConfig> {
    (
        physical_channel(),
        spacecraft_id::<10>(),
        proptest::option::weighted(0.2, fsh_len()),
        proptest::bool::weighted(0.2),
        proptest::collection::btree_set(vc_id::<3>(), 1..=8),
    )
        .prop_flat_map(|(pc, scid, fsh, ocf, vcids)| {
            let vcs: Vec<_> = vcids.into_iter().map(virtual_channel).collect();
            (Just(TmMasterChannelConfig::new(pc, scid, fsh, ocf)), vcs)
        })
        .prop_filter_map("at least one virtual channel", |(mc, vcs)| {
            let mc = vcs
                .into_iter()
                .fold(mc, |mc, vc| mc.clone().with_vc(vc).unwrap_or(mc));
            let any = mc.vcs().next().is_some();
            any.then_some(mc)
        })
}

/// A TM Transfer Frame built by `encode`, with the parts it was built from (132.0 §4.1;
/// TM-009, API-020, API-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedTmFrame {
    /// The master channel the frame belongs to.
    pub mc: TmMasterChannelConfig,
    /// The Primary Header fields the caller chose.
    pub fields: TmFrameFields,
    /// The Secondary Header Data Field; empty without a Secondary Header.
    pub secondary_header_data: Vec<u8>,
    /// The Security Header; empty without SDLS.
    pub security_header: Vec<u8>,
    /// The data field.
    pub data: Vec<u8>,
    /// The Security Trailer; empty without SDLS.
    pub security_trailer: Vec<u8>,
    /// The OCF, if the VC has one.
    pub ocf: Option<Ocf>,
    /// The encoded frame.
    pub octets: Vec<u8>,
}

impl EncodedTmFrame {
    /// The content as `encode` takes it (TM-009).
    #[must_use]
    pub fn content(&self) -> TmFrameContent<'_> {
        TmFrameContent {
            secondary_header_data: &self.secondary_header_data,
            security_header: &self.security_header,
            data: &self.data,
            security_trailer: &self.security_trailer,
            ocf: self.ocf,
        }
    }
}

/// The status of a frame on a VC carrying `content` with a `data_len`-octet data field: a
/// packet pointer inside the field or a reserved value, or the raw fields of a `VCA_SDU` frame
/// (132.0 §4.1.2.7; TM-007…TM-009).
///
/// `OnlyIdleData` only when `sdls` is false: an Only Idle Data frame uses a VC that does not use
/// SDLS (132.0 §6.4.6.2; TM-010).
fn data_field_status(
    content: DataFieldContent,
    data_len: usize,
    sdls: bool,
) -> BoxedStrategy<DataFieldStatus> {
    match content {
        DataFieldContent::Packets => {
            let last = u16::try_from(data_len.min(0x7FE) - 1).unwrap_or(0x7FD);
            let idle = u32::from(!sdls);
            prop_oneof![
                3 => up_to_u16(last).prop_map(|n| PacketPointer::StartsAt(
                    PacketOffset::new(n).expect("at most the largest offset")
                )),
                1 => Just(PacketPointer::NoPacketStart),
                idle => Just(PacketPointer::OnlyIdleData),
            ]
            .prop_map(DataFieldStatus::Packets)
            .boxed()
        }
        DataFieldContent::VcaSdu => (any::<bool>(), segment_length_id(), first_header_pointer())
            .prop_map(
                |(packet_order_flag, segment_length_id, first_header_pointer)| {
                    DataFieldStatus::VcaSdu {
                        packet_order_flag,
                        segment_length_id,
                        first_header_pointer,
                    }
                },
            )
            .boxed(),
    }
}

/// A frame of any configured VC of `mc`, with any counts, status, and field contents.
///
/// # Panics
/// Never: the master channel has at least one VC, and every field fits its layout.
pub fn frame_of(mc: TmMasterChannelConfig) -> impl Strategy<Value = EncodedTmFrame> {
    let vcs: Vec<TmVirtualChannelConfig> = mc.vcs().copied().collect();
    proptest::sample::select(vcs)
        .prop_flat_map(move |vc| {
            let layout = mc.frame_layout(vc.vcid()).expect("configured").clone();
            (
                Just(mc.clone()),
                Just(vc.vcid()),
                any::<u8>(),
                any::<u8>(),
                data_field_status(vc.content(), layout.data().len(), !vc.security().is_none()),
                octets(layout.secondary().len().saturating_sub(1)),
                octets(layout.security_header().len()),
                octets(layout.data().len()),
                octets(layout.security_trailer().len()),
                any::<[u8; 4]>()
                    .prop_map(move |o| (!layout.ocf().is_empty()).then(|| Ocf::decode(o))),
            )
        })
        .prop_map(|(mc, vcid, mcc, vcc, status, fsh, sh, data, st, ocf)| {
            let fields = TmFrameFields::new(vcid, mcc, vcc, status);
            let mut frame = EncodedTmFrame {
                mc,
                fields,
                secondary_header_data: fsh,
                security_header: sh,
                data,
                security_trailer: st,
                ocf,
                octets: Vec::new(),
            };
            let mut out = vec![0_u8; frame.mc.pc().frame_len()];
            let n = encode(&frame.mc, &frame.fields, &frame.content(), &mut out)
                .expect("an encodable frame");
            out.truncate(n);
            frame.octets = out;
            frame
        })
}

/// A frame of any [`master_channel`] ([`frame_of`]).
pub fn tm_frame() -> impl Strategy<Value = EncodedTmFrame> {
    master_channel().prop_flat_map(frame_of)
}

// ----- Frame octets next to the decoder's boundaries -----

/// A copy of `frame` with `f` applied to its octets.
fn changed(
    frame: &EncodedTmFrame,
    f: impl FnOnce(&mut Vec<u8>),
) -> (TmMasterChannelConfig, Vec<u8>) {
    let mut octets = frame.octets.clone();
    f(&mut octets);
    (frame.mc.clone(), octets)
}

/// A master channel and frame octets next to the boundaries of `TmFrame::decode` (132.0 §4.1;
/// GEN-020, API-030, VER-023):
///
/// - a valid frame;
/// - one octet too short or too long (TM-001);
/// - one bit flipped anywhere, which the FECF detects if present (TM-005);
/// - a Transfer Frame Version Number other than `00` (TM-002);
/// - the Secondary Header Flag set, with any Identification Field (TM-003);
/// - the OCF Flag toggled (TM-004, TM-006);
/// - the Packet Order Flag, the Segment Length Identifier, or the First Header Pointer of a
///   Synchronization Flag `0` frame changed (TM-007, TM-008).
pub fn tm_frame_near_boundary() -> BoxedStrategy<(TmMasterChannelConfig, Vec<u8>)> {
    let frame = tm_frame().boxed();
    prop_oneof![
        frame.clone().prop_map(|f| (f.mc, f.octets)).boxed(),
        (frame.clone(), any::<bool>())
            .prop_map(|(f, longer)| changed(&f, |o| {
                if longer {
                    o.push(0);
                } else {
                    o.pop();
                }
            }))
            .boxed(),
        (frame.clone(), any::<prop::sample::Index>(), 0_u8..8)
            .prop_map(|(f, at, bit)| changed(&f, |o| {
                let i = at.index(o.len());
                o[i] ^= 1 << bit;
            }))
            .boxed(),
        (frame.clone(), 1_u8..=3)
            .prop_map(|(f, v)| changed(&f, |o| o[0] = (o[0] & 0x3F) | (v << 6)))
            .boxed(),
        (frame.clone(), any::<u8>())
            .prop_map(|(f, id)| changed(&f, |o| {
                o[4] |= 0x80;
                o[6] = id;
            }))
            .boxed(),
        frame
            .clone()
            .prop_map(|f| changed(&f, |o| o[1] ^= 1))
            .boxed(),
        (
            frame,
            any::<bool>(),
            segment_length_id(),
            first_header_pointer()
        )
            .prop_map(|(f, pof, slid, fhp)| changed(&f, |o| {
                let [hi, lo] = fhp.get().to_be_bytes();
                o[4] = (o[4] & 0x80) | (u8::from(pof) << 5) | (slid.get() << 3) | hi;
                o[5] = lo;
            }))
            .boxed(),
    ]
    .boxed()
}

/// Input for a TM frame decoder property: a master channel with arbitrary octets of its frame
/// length, or octets [near a boundary](tm_frame_near_boundary) (GEN-020, API-030, VER-023).
pub fn tm_frame_decoder_input() -> BoxedStrategy<(TmMasterChannelConfig, Vec<u8>)> {
    prop_oneof![
        1 => master_channel()
            .prop_flat_map(|mc| {
                let len = mc.pc().frame_len();
                (Just(mc), octets(len))
            }),
        3 => tm_frame_near_boundary(),
    ]
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::{master_channel, packet_offset, tm_frame, tm_frame_decoder_input};
    use crate::strategies::config;
    use aldis_tm::{PacketOffset, TmFrame};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(256, REGRESSIONS))]

        #[test]
        fn frames_decode_strictly(f in tm_frame()) {
            // req: VER-054, TM-009
            let d = TmFrame::decode_with(&f.octets, f.mc.pc(), &f.mc).unwrap();
            prop_assert_eq!(d.vcid(), f.fields.vcid);
            prop_assert_eq!(d.data_field(), f.data.as_slice());
            prop_assert!(d.validate(&f.mc).is_ok());
        }

        #[test]
        fn other_strategies_are_in_range(mc in master_channel(), (m, bytes) in tm_frame_decoder_input(), o in packet_offset()) {
            // req: VER-054, GEN-020
            prop_assert!(mc.vcs().next().is_some());
            prop_assert!(o.get() <= PacketOffset::MAX);
            let _ = TmFrame::decode_with(&bytes, m.pc(), &m);
            let _ = TmFrame::decode_with_lenient(&bytes, m.pc(), &m);
        }
    }
}
