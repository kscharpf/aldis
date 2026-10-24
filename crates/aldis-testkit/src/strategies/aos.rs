//! Strategies for `aldis-aos` (732.0; VER-054).
//!
//! Physical channel configurations in both editions, virtual channel tables, whole AOS Transfer
//! Frames built by the crate's own encoder, and frame octets next to the decoder's boundaries.

use aldis_aos::ValidBitCount;
use aldis_aos::fhec;
use aldis_aos::{
    AosEdition, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig, AosVirtualChannelConfig,
    BitstreamPointer, DataFieldContent, FrameCountCycle, PacketPointer, PduHeader, VcFrameCount,
    VcTable, encode,
};
use aldis_core::crc::write_fecf;
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use proptest::prelude::*;

use super::common::{spacecraft_id, up_to_u16};
use super::octets;

/// The virtual channel table the strategies build: room for four virtual channels.
pub type AosTable = VcTable<4>;

/// Either edition, Issue 5 more often (AOS-001, GEN-062).
pub fn edition() -> impl Strategy<Value = AosEdition> {
    prop_oneof![
        3 => Just(AosEdition::Issue5),
        1 => Just(AosEdition::Issue4Compatible),
    ]
}

/// Any Virtual Channel Frame Count (732.0 §4.1.2.4).
pub fn vc_frame_count() -> impl Strategy<Value = VcFrameCount> {
    (0..=VcFrameCount::MAX).prop_filter_map("in range", VcFrameCount::new)
}

/// A frame count cycle, or none (732.0 §4.1.2.5; AOS-012).
pub fn cycle() -> impl Strategy<Value = Option<FrameCountCycle>> {
    proptest::option::of(
        (0..=FrameCountCycle::MAX).prop_filter_map("in range", FrameCountCycle::new),
    )
}

/// A physical channel of 12–299 octet frames with or without the FHEC, an Insert Zone of up to
/// 8 octets, and the FECF, in either edition (Table 5-1; AOS-001, AOS-015).
pub fn physical_channel() -> impl Strategy<Value = AosPhysicalChannelConfig> {
    (
        12_usize..300,
        any::<bool>(),
        0_usize..=8,
        any::<bool>(),
        edition(),
    )
        .prop_filter_map("a valid frame length", |(len, fhec, iz, fecf, edition)| {
            AosPhysicalChannelConfig::new(len, fhec, iz, fecf, edition).ok()
        })
}

/// Any Data Field Content (Table 5-3).
pub fn content() -> impl Strategy<Value = DataFieldContent> {
    prop_oneof![
        Just(DataFieldContent::Mpdu),
        Just(DataFieldContent::Bpdu),
        Just(DataFieldContent::VcaSdu),
        Just(DataFieldContent::IdleData),
    ]
}

/// A data virtual channel (VCID 0–62) of `scid` with any content, an optional `VC_OCF`, and a
/// small security layout (Table 5-3, Table 6-1).
fn virtual_channel(
    scid: SpacecraftId<10>,
) -> impl Strategy<Value = Option<AosVirtualChannelConfig>> {
    (
        0_u8..=62,
        content(),
        proptest::bool::weighted(0.3),
        proptest::option::weighted(0.3, (0_usize..8, 0_usize..8)),
    )
        .prop_map(move |(vcid, content, ocf, security)| {
            let mut vc = AosVirtualChannelConfig::new(scid, VcId::new(vcid)?, content).ok()?;
            if ocf {
                vc = vc.with_ocf();
            }
            if let Some((h, t)) = security {
                vc = vc.with_security(SecurityLayout::new(h, t).unwrap_or(SecurityLayout::NONE));
            }
            Some(vc)
        })
}

/// A table of one to four virtual channels of one or two spacecraft on any
/// [`physical_channel`] (Tables 5-1 to 5-3; API-040, AOS-018).
///
/// Virtual channels the table rejects (duplicates, an SCID too wide for the edition, no room
/// for data, a bitstream zone too long) are left out; a table left with none is not drawn.
pub fn vc_table() -> impl Strategy<Value = AosTable> {
    (
        physical_channel(),
        spacecraft_id::<10>(),
        (0_u16..=255).prop_filter_map("8 bits", SpacecraftId::new),
    )
        .prop_flat_map(|(pc, wide, narrow)| {
            let vcs = proptest::collection::vec(
                proptest::sample::select(vec![wide, narrow]).prop_flat_map(virtual_channel),
                1..=4,
            );
            (Just(pc), vcs)
        })
        .prop_filter_map("at least one virtual channel", |(pc, vcs)| {
            let mut accepted: Vec<AosVirtualChannelConfig> = Vec::new();
            for vc in vcs.into_iter().flatten() {
                let mut with = accepted.clone();
                with.push(vc);
                if AosTable::new(pc, &with).is_ok() {
                    accepted = with;
                }
            }
            AosTable::new(pc, &accepted)
                .ok()
                .filter(|t| t.vcs().next().is_some())
        })
}

/// An AOS Transfer Frame built by `encode`, with the parts it was built from (732.0 §4.1;
/// AOS-019, API-020, API-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAosFrame {
    /// The virtual channels of the frame's physical channel.
    pub table: AosTable,
    /// The Primary Header fields and data unit header the caller chose.
    pub fields: AosFrameFields,
    /// The Insert Zone; empty without one.
    pub insert_zone: Vec<u8>,
    /// The Security Header; empty without SDLS.
    pub security_header: Vec<u8>,
    /// The octets after the `M_PDU` or `B_PDU` Header, or the whole data field.
    pub zone: Vec<u8>,
    /// The Security Trailer; empty without SDLS.
    pub security_trailer: Vec<u8>,
    /// The OCF, if the VC has one.
    pub ocf: Option<Ocf>,
    /// The encoded frame.
    pub octets: Vec<u8>,
}

impl EncodedAosFrame {
    /// The physical channel.
    #[must_use]
    pub const fn pc(&self) -> AosPhysicalChannelConfig {
        self.table.physical_channel()
    }

    /// The content as `encode` takes it (AOS-019).
    #[must_use]
    pub fn content(&self) -> AosFrameContent<'_> {
        AosFrameContent {
            insert_zone: &self.insert_zone,
            security_header: &self.security_header,
            zone: &self.zone,
            security_trailer: &self.security_trailer,
            ocf: self.ocf,
        }
    }
}

/// The data unit header of a frame of a VC carrying `content` with a `zone`-octet zone: a
/// pointer inside the zone or a reserved value (732.0 §4.1.4.2.2, §4.1.4.3.3; AOS-017).
fn pdu_header(
    content: DataFieldContent,
    zone: usize,
    edition: AosEdition,
) -> BoxedStrategy<PduHeader> {
    match content {
        DataFieldContent::Mpdu => {
            let max = usize::from(PacketPointer::max_offset(edition));
            let last = u16::try_from(zone.saturating_sub(1).min(max)).unwrap_or(0);
            prop_oneof![
                3 => up_to_u16(last).prop_map(PacketPointer::StartsAt),
                1 => Just(PacketPointer::NoPacketStart),
                1 => Just(PacketPointer::OnlyIdleData),
            ]
            .prop_map(PduHeader::Mpdu)
            .boxed()
        }
        DataFieldContent::Bpdu => {
            let last = u16::try_from(zone.saturating_mul(8).saturating_sub(1))
                .unwrap_or(1)
                .max(1);
            prop_oneof![
                3 => (1..=last)
                    .prop_filter_map("a count of valid bits", ValidBitCount::new)
                    .prop_map(BitstreamPointer::ValidBits),
                1 => Just(BitstreamPointer::AllValid),
                1 => Just(BitstreamPointer::OnlyIdle),
            ]
            .prop_map(PduHeader::Bpdu)
            .boxed()
        }
        DataFieldContent::VcaSdu | DataFieldContent::IdleData => Just(PduHeader::None).boxed(),
    }
}

/// A frame of any virtual channel of `table`, with any count, replay flag, cycle, data unit
/// header, and field contents.
///
/// # Panics
/// Never: the table has at least one VC, and every field fits its layout.
pub fn frame_of(table: AosTable) -> impl Strategy<Value = EncodedAosFrame> {
    let vcs: Vec<AosVirtualChannelConfig> = table.vcs().copied().collect();
    proptest::sample::select(vcs)
        .prop_flat_map(move |vc| {
            let layout = table
                .frame_layout(vc.scid(), vc.vcid())
                .expect("configured");
            let header = match vc.content() {
                DataFieldContent::Mpdu | DataFieldContent::Bpdu => 2,
                DataFieldContent::VcaSdu | DataFieldContent::IdleData => 0,
            };
            let zone = layout.data().len() - header;
            let edition = table.physical_channel().edition();
            (
                Just(table.clone()),
                Just(vc),
                vc_frame_count(),
                any::<bool>(),
                cycle(),
                pdu_header(vc.content(), zone, edition),
                octets(layout.secondary().len()),
                octets(layout.security_header().len()),
                octets(zone),
                octets(layout.security_trailer().len()),
                any::<[u8; 4]>()
                    .prop_map(move |o| (!layout.ocf().is_empty()).then(|| Ocf::decode(o))),
            )
        })
        .prop_map(
            |(table, vc, count, replay_flag, cycle, pdu, iz, sh, zone, st, ocf)| {
                let fields = AosFrameFields {
                    scid: vc.scid(),
                    vcid: vc.vcid(),
                    vc_frame_count: count,
                    replay_flag,
                    cycle,
                    pdu,
                };
                let mut frame = EncodedAosFrame {
                    table,
                    fields,
                    insert_zone: iz,
                    security_header: sh,
                    zone,
                    security_trailer: st,
                    ocf,
                    octets: Vec::new(),
                };
                let pc = frame.pc();
                let mut out = vec![0_u8; pc.frame_len()];
                let n = encode(&pc, &frame.table, &frame.fields, &frame.content(), &mut out)
                    .expect("an encodable frame");
                out.truncate(n);
                frame.octets = out;
                frame
            },
        )
}

/// A frame of any [`vc_table`] ([`frame_of`]).
pub fn aos_frame() -> impl Strategy<Value = EncodedAosFrame> {
    vc_table().prop_flat_map(frame_of)
}

// ----- Frame octets next to the decoder's boundaries -----

/// A copy of `frame` with `f` applied to its octets.
fn changed(frame: &EncodedAosFrame, f: impl FnOnce(&mut Vec<u8>)) -> (AosTable, Vec<u8>) {
    let mut octets = frame.octets.clone();
    f(&mut octets);
    (frame.table.clone(), octets)
}

/// A copy of `frame` with `f` applied to its octets, then its FHEC and FECF recomputed when
/// configured, so that the change reaches the checks after them.
fn resealed(frame: &EncodedAosFrame, f: impl FnOnce(&mut Vec<u8>)) -> (AosTable, Vec<u8>) {
    let pc = frame.pc();
    changed(frame, |o| {
        f(o);
        if pc.fhec() {
            let mut header: [u8; 8] = o[..8].try_into().expect("eight header octets");
            fhec::write(&mut header);
            o[..8].copy_from_slice(&header);
        }
        if pc.fecf() {
            write_fecf(o).expect("a frame longer than its FECF");
        }
    })
}

/// The offsets of the ten FHEC symbols' octets: 0, 1, 5, 6, 7 (732.0 §4.1.2.6.5 h).
const SYMBOL_OCTETS: [usize; 5] = [0, 1, 5, 6, 7];

/// A table and frame octets next to the boundaries of `AosFrame::decode` (732.0 §4.1;
/// GEN-020, API-030, VER-023):
///
/// - a valid frame;
/// - one octet too short or too long (API-012);
/// - one bit flipped anywhere, which the FHEC or FECF may detect (AOS-014);
/// - one to three header symbols changed, which the FHEC corrects, detects, or miscorrects
///   (AOS-013, AOS-014);
/// - with the FHEC and FECF recomputed, so that the change reaches the later checks: a
///   Transfer Frame Version Number other than `01` (AOS-011); octet 5 replaced, the Replay
///   Flag, the Use Flag, bits 42–43, and the cycle (AOS-002, AOS-012); the VCID changed, to 63
///   most often (AOS-016); the first two data field octets, an `M_PDU` or `B_PDU` Header,
///   replaced (AOS-003, AOS-017).
pub fn aos_frame_near_boundary() -> BoxedStrategy<(AosTable, Vec<u8>)> {
    let frame = aos_frame().boxed();
    prop_oneof![
        frame.clone().prop_map(|f| (f.table, f.octets)).boxed(),
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
        (
            frame.clone(),
            proptest::collection::vec((0_usize..5, any::<bool>(), 1_u8..16), 1..=3)
        )
            .prop_map(|(f, hits)| changed(&f, |o| {
                let header = if f.pc().fhec() { 5 } else { 3 };
                for (at, high, value) in hits {
                    let i = SYMBOL_OCTETS[at % header];
                    o[i] ^= if high { value << 4 } else { value };
                }
            }))
            .boxed(),
        (frame.clone(), 0_u8..=3)
            .prop_filter("another version", |(_, v)| *v != 1)
            .prop_map(|(f, v)| resealed(&f, |o| o[0] = (o[0] & 0x3F) | (v << 6)))
            .boxed(),
        (frame.clone(), any::<u8>())
            .prop_map(|(f, octet)| resealed(&f, |o| o[5] = octet))
            .boxed(),
        (frame.clone(), prop_oneof![3 => Just(63_u8), 1 => 0_u8..63])
            .prop_map(|(f, vcid)| resealed(&f, |o| o[1] = (o[1] & 0xC0) | vcid))
            .boxed(),
        (frame, any::<[u8; 2]>())
            .prop_map(|(f, pdu)| {
                let at = f
                    .table
                    .frame_layout(f.fields.scid, f.fields.vcid)
                    .map_or(0, |l| l.data().start);
                resealed(&f, |o| {
                    o[at] = pdu[0];
                    o[at + 1] = pdu[1];
                })
            })
            .boxed(),
    ]
    .boxed()
}

/// Input for an AOS frame decoder property: a table with arbitrary octets of its frame length,
/// or octets [near a boundary](aos_frame_near_boundary) (GEN-020, API-030, VER-023).
pub fn aos_frame_decoder_input() -> BoxedStrategy<(AosTable, Vec<u8>)> {
    prop_oneof![
        1 => vc_table().prop_flat_map(|t| {
            let len = t.physical_channel().frame_len();
            (Just(t), octets(len))
        }),
        3 => aos_frame_near_boundary(),
    ]
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::{aos_frame, aos_frame_decoder_input, vc_table};
    use crate::strategies::config;
    use aldis_aos::AosFrame;
    use aldis_core::DecodeMode;
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(256, REGRESSIONS))]

        #[test]
        fn frames_decode_strictly(f in aos_frame()) {
            // req: VER-054, AOS-019
            let pc = f.pc();
            let d = AosFrame::decode(&f.octets, &pc, &f.table, DecodeMode::Strict).unwrap();
            prop_assert_eq!(d.vcid(), f.fields.vcid);
            prop_assert_eq!(d.data_unit().zone(), f.zone.as_slice());
        }

        #[test]
        fn other_strategies_are_in_range(t in vc_table(), (table, bytes) in aos_frame_decoder_input()) {
            // req: VER-054, GEN-020
            prop_assert!(t.vcs().next().is_some());
            let pc = table.physical_channel();
            for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                let _ = AosFrame::decode(&bytes, &pc, &table, mode);
            }
        }
    }
}
